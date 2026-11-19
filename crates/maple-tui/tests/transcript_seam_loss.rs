//! Port of `l/test/transcript-seam-loss.test.ts`.
//!
//! A transcript seam is the boundary between rows already in scrollback and the
//! live tail. Committing a split must leave the committed prefix byte-identical
//! to the block's own render, or the seam shows a tear. The crate exposes the
//! prefix predicate and the blank-edge trim, so the port asserts both plus the
//! container's split commit.

use maple_tui::chrome::transcript_container::{
	is_row_prefix, trim_blank_edges, AppendOnlyTranscriptBlock, CompactTranscriptBlock,
	FinalizableBlock, ToolActivityBlock, TranscriptContainer, TranscriptPresentationTarget,
	UsageRowBlock,
};
use maple_tui::component::Component;

struct SeamBlock {
	rows: Vec<String>,
}

impl SeamBlock {
	fn finalized(rows: &[&str]) -> Self {
		SeamBlock { rows: rows.iter().map(|row| row.to_string()).collect() }
	}
}

impl Component for SeamBlock {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.rows.clone()
	}
}

impl TranscriptPresentationTarget for SeamBlock {}
impl AppendOnlyTranscriptBlock for SeamBlock {}
impl CompactTranscriptBlock for SeamBlock {}
impl FinalizableBlock for SeamBlock {
	fn is_transcript_block_finalized(&self) -> Option<bool> {
		Some(true)
	}
}
impl ToolActivityBlock for SeamBlock {}
impl UsageRowBlock for SeamBlock {}

#[test]
fn a_committed_split_is_a_byte_identical_prefix_of_the_block() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(SeamBlock::finalized(&["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8"]));

	let offer = transcript.peek_finalized_batch(20, 4).expect("split offer");
	assert_eq!(offer.rows, vec!["s1", "s2", "s3", "s4"]);
	transcript.acknowledge_finalized_batch(offer.id);

	let semantic = transcript.render_semantic(20);
	assert_eq!(semantic, vec!["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8"]);
	assert!(is_row_prefix(&offer.rows, &semantic));
	assert_eq!(transcript.live_row_count(20, usize::MAX), 4);
}

#[test]
fn the_live_tail_after_a_split_is_the_uncommitted_suffix() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(SeamBlock::finalized(&["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8"]));

	let offer = transcript.peek_finalized_batch(20, 4).expect("split offer");
	transcript.acknowledge_finalized_batch(offer.id);

	assert_eq!(transcript.render_tail(20, 3), vec!["s6", "s7", "s8"]);
	assert_eq!(transcript.render_tail(20, 4), vec!["s5", "s6", "s7", "s8"]);
	assert_eq!(transcript.render_tail(20, 9), vec!["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8"]);
}

#[test]
fn trim_blank_edges_keeps_an_interior_seam_row() {
	let rows = vec!["".to_string(), "a".to_string(), "".to_string(), "b".to_string(), "".to_string()];
	assert_eq!(trim_blank_edges(&rows), vec!["a", "", "b"]);
	let only_blanks = vec!["".to_string(), "   ".to_string(), "\t".to_string()];
	assert!(trim_blank_edges(&only_blanks).is_empty());
}

#[test]
fn row_prefix_requires_byte_identical_rows() {
	assert!(is_row_prefix(&[], &["a".to_string()]));
	assert!(is_row_prefix(
		&["a".to_string(), "b".to_string()],
		&["a".to_string(), "b".to_string(), "c".to_string()]
	));
	assert!(!is_row_prefix(&["a".to_string()], &[]));
	assert!(!is_row_prefix(&["a".to_string(), "x".to_string()], &["a".to_string(), "b".to_string()]));
	assert!(!is_row_prefix(&["a".to_string(), "b".to_string()], &["a".to_string()]));
}
