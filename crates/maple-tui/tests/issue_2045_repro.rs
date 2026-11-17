//! Port of `l/test/issue-2045-repro.test.ts`.
//!
//! The renderer bound is ported into `screen.rs` (`bound_row_escapes`): a row's
//! escape bytes are capped while its printable text is preserved. The tests
//! drive the crate's differential painter with a virtual terminal.

use maple_tui::screen::Screen;
use maple_tui::terminal::virt::VirtualTerminal;

fn paint(rows: &[String], width: u16) -> String {
	let mut screen = Screen::new();
	let mut terminal = VirtualTerminal::new(width, 4);
	screen.flush(&mut terminal, rows, width);
	terminal.into_output()
}

#[test]
fn preserves_visible_text_after_pathological_zero_width_ansi_prefixes() {
	let line = format!("{}payload", "\x1b[31m".repeat(20_000));
	let rendered = paint(&[line], 80);

	assert!(rendered.contains("payload"));
	assert!(rendered.len() < 12_000, "rendered {} bytes", rendered.len());
}

#[test]
fn preserves_visible_suffix_text_after_long_zero_width_combining_prefixes() {
	let line = format!("a{}bc", "\u{0301}".repeat(4096));
	let rendered = paint(&[line], 3);

	assert!(rendered.contains("bc"));
}

#[test]
fn preserves_visible_text_after_oversized_osc_hyperlink_prefixes() {
	let line = format!("\x1b]8;;https://example.com/{}\x07link-label\x1b]8;;\x07", "a".repeat(70_000));
	let rendered = paint(&[line], 80);

	assert!(rendered.contains("link-label"));
	assert!(rendered.len() < 12_000, "rendered {} bytes", rendered.len());
}

#[test]
fn preserves_osc_66_text_sizing_payloads_at_the_start_of_long_rows() {
	let visible_text = "H".repeat(70);
	let line = format!("\x1b]66;s=1;{visible_text}\x1b\\{}", "\x1b[31m".repeat(20_000));
	let rendered = paint(&[line], 80);

	assert!(rendered.contains(&visible_text));
}
