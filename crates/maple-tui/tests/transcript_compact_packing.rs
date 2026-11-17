//! Port of `l/test/transcript-compact-packing.test.ts`.
//!
//! The reference packs runs of compact blocks gaplessly and owes a blank
//! separator to the first non-compact block. The crate expresses both through
//! `CompactTranscriptBlock::is_transcript_block_compact` plus the separator
//! rule, so the port asserts the rendered rows and the folded native column.

use maple_tui::chrome::transcript_container::{
	AppendOnlyTranscriptBlock, CompactTranscriptBlock, FinalizableBlock, ToolActivityBlock,
	TranscriptContainer, TranscriptPresentationTarget, UsageRowBlock,
};
use maple_tui::component::Component;
use maple_tui::native::node::DescribeContext;

struct PackBlock {
	rows: Vec<String>,
	compact: bool,
}

impl PackBlock {
	fn compact(rows: &[&str]) -> Self {
		PackBlock { rows: rows.iter().map(|row| row.to_string()).collect(), compact: true }
	}

	fn mutable(rows: &[&str]) -> Self {
		PackBlock { rows: rows.iter().map(|row| row.to_string()).collect(), compact: false }
	}
}

impl Component for PackBlock {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.rows.clone()
	}
}

impl TranscriptPresentationTarget for PackBlock {}
impl AppendOnlyTranscriptBlock for PackBlock {}
impl CompactTranscriptBlock for PackBlock {
	fn is_transcript_block_compact(&self) -> bool {
		self.compact
	}
}
impl FinalizableBlock for PackBlock {}
impl ToolActivityBlock for PackBlock {}
impl UsageRowBlock for PackBlock {}

#[test]
fn compact_blocks_pack_without_a_separator() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(PackBlock::compact(&["a", "b"]));
	transcript.add_block(PackBlock::compact(&["c", "d"]));

	assert_eq!(transcript.render_semantic(20), vec!["a", "b", "c", "d"]);
	assert_eq!(transcript.live_row_count(20, usize::MAX), 4);
}

#[test]
fn a_mutable_block_owes_the_next_block_a_separator() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(PackBlock::mutable(&["a"]));
	transcript.add_block(PackBlock::compact(&["b"]));
	transcript.add_block(PackBlock::mutable(&["c"]));

	assert_eq!(transcript.render_semantic(20), vec!["a", "", "b", "", "c"]);
	assert_eq!(transcript.live_row_count(20, usize::MAX), 5);
}

#[test]
fn a_run_of_compact_blocks_folds_into_one_gapless_native_column() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(PackBlock::compact(&["a"]));
	transcript.add_block(PackBlock::compact(&["b"]));
	transcript.add_block(PackBlock::mutable(&["c"]));

	let mut cx = DescribeContext::new(80);
	let blocks = transcript.native_blocks(&cx);
	assert_eq!(blocks.len(), 2);
	assert_eq!(blocks[0].role(), Some("omp.compact"));
	assert_eq!(blocks[0].props.get_str("gap"), Some("none"));
	assert_eq!(blocks[0].children.len(), 2);
	assert_eq!(blocks[1].role(), None);
	assert_eq!(blocks[1].children.len(), 0);
}

#[test]
fn live_row_count_charges_one_separator_between_mutable_blocks() {
	let mut transcript = TranscriptContainer::new();
	transcript.add_block(PackBlock::mutable(&["a"]));
	transcript.add_block(PackBlock::mutable(&["b", "c"]));
	transcript.add_block(PackBlock::mutable(&["d"]));

	assert_eq!(transcript.live_row_count(20, usize::MAX), 6);
	assert_eq!(transcript.render_semantic(20), vec!["a", "", "b", "c", "", "d"]);
}
