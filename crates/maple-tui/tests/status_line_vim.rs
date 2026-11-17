//! Port of `l/test/status-line-vim.test.ts`.
//!
//! The vim segment renders from `VimState`; expectations are built from the same
//! `FallbackTheme` the renderer uses. One case stays ignored: the reference walks
//! all three symbol presets, while `FallbackTheme` is pinned to the unicode
//! preset (the crate's preset switch lives on `Theme`, not the status-line
//! adapter).

use maple_tui::status_line::schema::SegmentId;
use maple_tui::status_line::segments::{
	render_segment_with, SegmentContext, VimDisplay, VimMode, VimState,
};
use maple_tui::status_line::{FallbackTheme, StatusTheme};
use maple_tui::text_shape::visible_width;

fn vim_context(vim: Option<VimState>) -> SegmentContext {
	let mut ctx = SegmentContext::default();
	ctx.width = 80;
	ctx.vim = vim;
	ctx
}

fn text_state(mode: VimMode, pending: &str, selected_lines: u32) -> VimState {
	VimState {
		mode,
		pending: pending.to_string(),
		selected_lines,
		display: VimDisplay::Text,
	}
}

#[test]
fn stays_hidden_when_vim_mode_is_off() {
	let theme = FallbackTheme::new();
	let rendered = render_segment_with(SegmentId::Vim, &theme, &vim_context(None));
	assert!(!rendered.visible);
	assert_eq!(rendered.content, "");
}

#[test]
fn names_the_active_mode() {
	let theme = FallbackTheme::new();
	let cases = [
		(VimMode::Normal, "NORMAL"),
		(VimMode::Insert, "INSERT"),
		(VimMode::Visual, "VISUAL"),
		(VimMode::VisualLine, "V-LINE"),
	];
	for (mode, label) in cases {
		let state = text_state(mode, "", 0);
		let rendered = render_segment_with(SegmentId::Vim, &theme, &vim_context(Some(state)));
		assert!(rendered.visible);
		assert_eq!(rendered.content, theme.fg(mode.color(), label), "{label}");
	}
}

#[test]
fn echoes_a_half_typed_command_beside_the_mode() {
	let theme = FallbackTheme::new();
	let state = text_state(VimMode::Normal, "2d", 0);
	let rendered = render_segment_with(SegmentId::Vim, &theme, &vim_context(Some(state)));
	assert_eq!(
		rendered.content,
		format!(
			"{}{}",
			theme.fg("accent", "NORMAL"),
			theme.fg("muted", " 2d")
		)
	);
}

#[test]
fn appends_the_visual_selection_height_only_once_it_spans_multiple_lines() {
	let theme = FallbackTheme::new();
	let single = render_segment_with(
		SegmentId::Vim,
		&theme,
		&vim_context(Some(text_state(VimMode::VisualLine, "", 1))),
	);
	assert_eq!(single.content, theme.fg("warning", "V-LINE"));

	let spanning = render_segment_with(
		SegmentId::Vim,
		&theme,
		&vim_context(Some(text_state(VimMode::VisualLine, "", 4))),
	);
	assert_eq!(spanning.content, theme.fg("warning", "V-LINE 4L"));
}

#[test]
fn colors_insert_differently_from_normal_so_the_mode_reads_at_a_glance() {
	let theme = FallbackTheme::new();
	let insert = render_segment_with(
		SegmentId::Vim,
		&theme,
		&vim_context(Some(text_state(VimMode::Insert, "", 0))),
	);
	let normal = render_segment_with(
		SegmentId::Vim,
		&theme,
		&vim_context(Some(text_state(VimMode::Normal, "", 0))),
	);
	assert_ne!(insert.content, normal.content);
	assert!(insert.content.contains('\x1b'));
}

#[test]
#[ignore = "missing facility: FallbackTheme is pinned to the unicode preset; the crate has no per-call symbol-preset switch for the status-line adapter"]
fn collapses_each_mode_to_one_distinct_cell_in_every_symbol_preset() {
	let _ = FallbackTheme::new();
}

#[test]
fn keeps_pending_and_selection_height_in_the_icon_display() {
	let theme = FallbackTheme::new();
	let state = VimState {
		mode: VimMode::VisualLine,
		pending: "2".to_string(),
		selected_lines: 3,
		display: VimDisplay::Icon,
	};
	let rendered = render_segment_with(SegmentId::Vim, &theme, &vim_context(Some(state)));
	let glyph = theme.symbol("icon.vimVisualLine");
	assert_eq!(
		rendered.content,
		format!(
			"{}{}",
			theme.fg("warning", &format!("{glyph} 3L")),
			theme.fg("muted", " 2")
		)
	);
	assert_eq!(visible_width(&rendered.content), 6);
}

#[test]
fn hides_the_segment_entirely_when_display_is_none() {
	let theme = FallbackTheme::new();
	let state = VimState {
		mode: VimMode::Normal,
		pending: "2d".to_string(),
		selected_lines: 4,
		display: VimDisplay::None,
	};
	let rendered = render_segment_with(SegmentId::Vim, &theme, &vim_context(Some(state)));
	assert!(!rendered.visible);
	assert_eq!(rendered.content, "");
}
