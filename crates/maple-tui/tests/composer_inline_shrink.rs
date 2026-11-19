//! Port of `l/test/composer-inline-shrink.test.ts` (viewport-budget parts).
//!
//! Deviations: the reference grows an inline widget above the composer and
//! asserts the transcript viewport shrinks without a blank band above the
//! editor, plus a streamed-answer retirement path. The crate's transcript
//! container is fed the resulting viewport budget by its host, so the port
//! drives [`TranscriptContainer::render_viewport`] with shrinking budgets over
//! settled rows and checks the same invariants: the budget is honoured, the
//! newest rows survive, and no unfillable blank band appears.

use maple_tui::chrome::transcript_container::{AnimationFrame, TranscriptBlock, TranscriptContainer};
use maple_tui::components::text::Text;

const WIDTH: u16 = 100;
const ROWS: usize = 40;

fn settled_transcript(rows: usize) -> TranscriptContainer {
	let mut block = TranscriptBlock::new();
	for index in 0..rows {
		block.add_row(Text::new(format!("Settled transcript row {index}")));
	}
	let mut container = TranscriptContainer::new();
	container.add_block(block);
	container
}

fn longest_blank_run(lines: &[String]) -> usize {
	let mut longest = 0usize;
	let mut run = 0usize;
	for line in lines {
		if line.trim().is_empty() {
			run += 1;
			longest = longest.max(run);
		} else {
			run = 0;
		}
	}
	longest
}

#[test]
fn full_budget_shows_every_settled_row() {
	let mut container = settled_transcript(ROWS);
	let frame = AnimationFrame { tick: 1, now: 0 };
	let lines = container.render_viewport(WIDTH, ROWS, frame);
	assert_eq!(lines.len(), ROWS);
	assert!(lines.contains(&"Settled transcript row 0".to_string()));
	assert!(lines.contains(&"Settled transcript row 39".to_string()));
}

#[test]
fn shrinking_budget_keeps_the_newest_rows() {
	let mut container = settled_transcript(ROWS);
	let frame = AnimationFrame { tick: 1, now: 0 };
	let lines = container.render_viewport(WIDTH, 5, frame);
	assert!(lines.len() <= 5, "got {} rows", lines.len());
	assert_eq!(lines.last().map(String::as_str), Some("Settled transcript row 39"));
	assert!(!lines.iter().any(|line| line == "Settled transcript row 3"), "old rows fall out");
}

#[test]
fn no_blank_band_appears_when_the_inline_chrome_grows() {
	let frame = AnimationFrame { tick: 1, now: 0 };
	for budget in 1..=ROWS {
		let mut container = settled_transcript(ROWS);
		let lines = container.render_viewport(WIDTH, budget, frame);
		assert!(lines.len() <= budget, "budget {budget} produced {} rows", lines.len());
		assert!(
			longest_blank_run(&lines) <= 1,
			"budget {budget} left a blank band: {lines:?}"
		);
		assert_eq!(lines.last().map(String::as_str), Some("Settled transcript row 39"));
	}
}

#[test]
fn zero_budget_renders_nothing_and_live_count_tracks_the_tail() {
	let mut container = settled_transcript(ROWS);
	let frame = AnimationFrame { tick: 1, now: 0 };
	assert!(container.render_viewport(WIDTH, 0, frame).is_empty());
	assert_eq!(container.live_row_count(WIDTH, usize::MAX), ROWS);
	assert_eq!(container.render_viewport(WIDTH, 7, frame).len(), 7);
	assert_eq!(container.live_row_count(WIDTH, 7), ROWS, "the tail still exists off-screen");
}
