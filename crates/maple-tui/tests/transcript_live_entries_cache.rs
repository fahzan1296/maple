//! Port of `l/test/transcript-live-entries-cache.test.ts`.

use maple_tui::chrome::transcript_container::{
	AnimationFrame, FinalizableBlock, TranscriptBlockMode, TranscriptContainer,
};
use maple_tui::component::Component;

struct Block {
	rows: Vec<String>,
	finalized: bool,
}

impl Block {
	fn new(rows: &[&str]) -> Self {
		Block {
			rows: rows.iter().map(|row| (*row).to_string()).collect(),
			finalized: true,
		}
	}

	fn active(rows: &[&str]) -> Self {
		Block {
			rows: rows.iter().map(|row| (*row).to_string()).collect(),
			finalized: false,
		}
	}
}

impl Component for Block {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.rows.clone()
	}

	fn invalidate(&mut self) {}

	fn dispose(&mut self) {}
}

impl maple_tui::chrome::transcript_container::TranscriptPresentationTarget for Block {}
impl maple_tui::chrome::transcript_container::AppendOnlyTranscriptBlock for Block {}
impl maple_tui::chrome::transcript_container::CompactTranscriptBlock for Block {}
impl FinalizableBlock for Block {
	fn is_transcript_block_finalized(&self) -> Option<bool> {
		Some(self.finalized)
	}
}
impl maple_tui::chrome::transcript_container::ToolActivityBlock for Block {}
impl maple_tui::chrome::transcript_container::UsageRowBlock for Block {}

fn frame() -> AnimationFrame {
	AnimationFrame { tick: 0, now: 0 }
}

fn prime(transcript: &mut TranscriptContainer, width: u16, rows: usize) {
	let frame = frame();
	transcript.begin_frame(frame);
	transcript.render_viewport(width, rows, frame);
}

#[test]
fn includes_blocks_added_after_the_cache_was_primed() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::new(&["first"]));
	prime(&mut transcript, 80, 10);

	transcript.add_block(Block::new(&["second"]));

	assert_eq!(
		transcript.render_viewport(80, 10, frame()),
		vec!["first".to_string(), String::new(), "second".to_string()]
	);
}

#[test]
fn excludes_committed_blocks_from_the_viewport_after_retirement() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::new(&["old"]));
	transcript.add_block(Block::active(&["fresh"]));
	prime(&mut transcript, 80, 10);

	let batch = transcript.peek_finalized_batch(80, 1).expect("batch");
	assert_eq!(batch.rows, vec!["old".to_string(), String::new()]);
	transcript.acknowledge_finalized_batch(batch.id);

	assert_eq!(
		transcript.render_viewport(80, 10, frame()),
		vec!["fresh".to_string()]
	);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Committed, Active]");
}

#[test]
fn keeps_live_indexes_valid_after_removing_a_live_block() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::new(&["settled"]));
	transcript.add_block(Block::active(&["doomed"]));
	transcript.add_block(Block::active(&["survivor"]));
	prime(&mut transcript, 80, 10);

	assert!(transcript.remove_block(1));

	assert_eq!(
		transcript.render_viewport(80, 10, frame()),
		vec!["settled".to_string(), String::new(), "survivor".to_string()]
	);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Settled, Active]");
}

#[test]
fn shows_every_block_again_after_the_transcript_is_cleared_and_rebuilt() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::new(&["stale"]));
	prime(&mut transcript, 80, 10);
	transcript.clear();
	transcript.add_block(Block::new(&["rebuilt"]));

	assert_eq!(
		transcript.render_viewport(80, 10, frame()),
		vec!["rebuilt".to_string()]
	);
	assert_eq!(format!("{:?}", transcript.block_states()), "[Settled]");
}

#[test]
fn picks_up_a_block_swapped_in_place_without_a_length_change() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::new(&["old rows"]));
	prime(&mut transcript, 80, 10);

	assert!(transcript.remove_block(0));
	transcript.insert_block_at(0, Block::new(&["new rows"]));

	assert_eq!(
		transcript.render_viewport(80, 10, frame()),
		vec!["new rows".to_string()]
	);
}

#[test]
fn retires_the_settled_head_the_viewport_no_longer_shows() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(Block::new(&["old"]));
	transcript.add_block(Block::active(&["fresh"]));
	prime(&mut transcript, 80, 10);

	let batch = transcript.peek_finalized_batch(80, 1);

	assert_eq!(
		batch.expect("batch").rows,
		vec!["old".to_string(), String::new()]
	);
	// The settled head kept its append-only default; the mode is a permanent
	// declaration and does not affect this retirement.
	assert_eq!(
		transcript.block_modes(),
		vec![TranscriptBlockMode::Mutable, TranscriptBlockMode::Mutable]
	);
}