//! Port of `l/test/transcript-committed-release.test.ts`.
//!
//! The reference watches a `WeakSet` of render caches to see a committed block
//! released. The crate's `TranscriptChild::release_render_caches` hook has only
//! the blanket no-op implementation, so the port asserts the observable half of
//! the same contract: an acknowledged commit moves the block out of the live
//! tail, marks it `Committed`, and forgets its emission ledger, while an offer
//! that is never acknowledged changes nothing.

use maple_tui::chrome::transcript_container::{
	AppendOnlyTranscriptBlock, CompactTranscriptBlock, FinalizableBlock, ToolActivityBlock,
	TranscriptBatchKind, TranscriptContainer, TranscriptPresentationTarget, UsageRowBlock,
};
use maple_tui::component::Component;

struct Block {
	rows: Vec<String>,
	finalized: Option<bool>,
}

impl Block {
	fn settled(rows: &[&str]) -> Self {
		Block { rows: rows.iter().map(|row| row.to_string()).collect(), finalized: Some(true) }
	}

	fn streaming(rows: &[&str]) -> Self {
		Block { rows: rows.iter().map(|row| row.to_string()).collect(), finalized: Some(false) }
	}
}

impl Component for Block {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.rows.clone()
	}
}

impl TranscriptPresentationTarget for Block {}
impl AppendOnlyTranscriptBlock for Block {}
impl CompactTranscriptBlock for Block {}
impl FinalizableBlock for Block {
	fn is_transcript_block_finalized(&self) -> Option<bool> {
		self.finalized
	}
}
impl ToolActivityBlock for Block {}
impl UsageRowBlock for Block {}

#[test]
fn acknowledging_a_commit_releases_the_block_from_the_live_tail() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::settled(&["a1", "a2", "a3", "a4", "a5"]));
	transcript.add_block(Block::settled(&["b1", "b2", "b3"]));

	let first = transcript.peek_finalized_batch(20, 4).expect("first offer");
	assert_eq!(first.kind, TranscriptBatchKind::Append);
	assert_eq!(first.rows.len(), 6);
	assert_eq!(
		maple_tui::chrome::transcript_container::trim_blank_edges(&first.rows),
		vec!["a1", "a2", "a3", "a4", "a5"]
	);
	assert!(transcript.has_active_block());
	assert_eq!(transcript.live_row_count(20, usize::MAX), 9);

	transcript.acknowledge_finalized_batch(first.id);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Committed, Settled]");
	assert_eq!(transcript.live_row_count(20, usize::MAX), 3);
	assert_eq!(transcript.emitted_stable_rows(), vec![0, 0]);

	let second = transcript.peek_finalized_batch(20, 0).expect("second offer");
	assert_eq!(
		maple_tui::chrome::transcript_container::trim_blank_edges(&second.rows),
		vec!["b1", "b2", "b3"]
	);
	transcript.acknowledge_finalized_batch(second.id);
	assert!(!transcript.has_active_block());
	assert_eq!(transcript.live_row_count(20, usize::MAX), 0);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Committed, Committed]");
}

#[test]
fn an_unacknowledged_offer_changes_nothing_until_it_is_accepted() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::settled(&["a1", "a2", "a3", "a4", "a5"]));
	transcript.add_block(Block::settled(&["b1", "b2", "b3"]));

	let offer = transcript.peek_finalized_batch(20, 4).expect("offer");
	assert_eq!(format!("{:?}", transcript.block_states()), "[Settled, Settled]");
	assert_eq!(transcript.live_row_count(20, usize::MAX), 9);

	let rerendered = transcript.rerender_offered_batch(20).expect("re-offer");
	assert_eq!(rerendered.id, offer.id);
	assert_eq!(rerendered.rows, offer.rows);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Settled, Settled]");
	assert_eq!(transcript.live_row_count(20, usize::MAX), 9);
}

#[test]
fn a_stale_acknowledge_is_ignored() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::settled(&["a1", "a2", "a3"]));
	transcript.add_block(Block::settled(&["b1", "b2"]));

	let offer = transcript.peek_finalized_batch(20, 2).expect("offer");
	transcript.acknowledge_finalized_batch(offer.id + 1);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Settled, Settled]");
	assert_eq!(transcript.live_row_count(20, usize::MAX), 6);

	transcript.acknowledge_finalized_batch(offer.id);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Committed, Settled]");
	assert_eq!(transcript.live_row_count(20, usize::MAX), 2);
}

#[test]
fn replay_carries_the_committed_ledger_and_cancel_drops_it() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::settled(&["a1", "a2", "a3"]));
	transcript.add_block(Block::settled(&["b1", "b2"]));

	let first = transcript.peek_finalized_batch(20, 0).expect("first offer");
	transcript.acknowledge_finalized_batch(first.id);
	let second = transcript.peek_finalized_batch(20, 0).expect("second offer");
	transcript.acknowledge_finalized_batch(second.id);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Committed, Committed]");

	transcript.cancel_replay();
	assert!(transcript.peek_replay_batch(20).is_none());

	transcript.begin_replay();
	let replay = transcript.peek_replay_batch(20).expect("replay");
	assert_eq!(replay.kind, TranscriptBatchKind::Replay);
	assert_eq!(
		maple_tui::chrome::transcript_container::trim_blank_edges(&replay.rows),
		vec!["a1", "a2", "a3", "", "b1", "b2"]
	);

	transcript.acknowledge_finalized_batch(replay.id);
	assert!(transcript.peek_finalized_batch(20, 0).is_none());
	assert!(!transcript.has_active_block());
}

#[test]
fn pressure_never_commits_past_a_streaming_block() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::streaming(&["a1", "a2", "a3", "a4", "a5", "a6"]));
	transcript.add_block(Block::settled(&["b1", "b2"]));

	assert!(transcript.peek_finalized_batch(20, 2).is_none());
	assert_eq!(format!("{:?}", transcript.block_states()), "[Active, Settled]");
	assert_eq!(transcript.live_row_count(20, usize::MAX), 9);
	assert!(transcript.has_active_block());
	assert!(transcript.take_warnings().is_empty());
}
