//! Shared scaffolding for the TUI selector/list/dashboard components: viewport
//! windowing, scrollbar-aware row widths, ScrollView rendering, selection
//! clamping, tab-cycling keys, and full-screen padding.
//!
//! Reference: `l/src/chrome/selector-helpers.ts`.
//!
//! Deviations:
//! - The reference reads the process-global `theme`; this port passes `&Theme`
//!   explicitly ([`render_scrollable_list`]).
//! - The reference's `ScrollView` paints its scrollbar through theme closures;
//!   the Rust [`ScrollView`] only accepts [`Style`] values, so the theme tokens
//!   are resolved to a foreground [`Color`] and painted with a full reset
//!   instead of the theme's foreground reset.

use crate::component::Component;
use crate::components::scroll_view::ScrollView;
use crate::components::scroll_viewport::{
	centered_viewport_range, scroll_offset_for_row, viewport_overflows, ScrollbarMode,
	ScrollViewTheme, ViewportAlignment,
};
use crate::keys::{is_arrow_left, is_arrow_right, is_shift_tab, is_tab, Key};
use crate::style::{Color, Style};
use crate::theme::Theme;

/// Options for [`render_scrollable_list`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollableListOptions {
	/// Viewport width in columns.
	pub width: u16,
	/// Logical row count, which may exceed the supplied `rows`.
	pub total_rows: usize,
	/// Scroll offset in logical rows.
	pub scroll_offset: usize,
}

/// Selection-centered logical row window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CenteredWindow {
	/// First visible logical row.
	pub start_index: usize,
	/// Exclusive end of the visible rows.
	pub end_index: usize,
}

/// A clamped selection pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClampedSelection {
	/// Selection clamped into `[0, total)`.
	pub selected_index: usize,
	/// Offset that keeps the selection visible.
	pub scroll_offset: usize,
}

/// Render `rows` through a [`ScrollView`] with the shared list theme (muted
/// track / accent thumb) and an "auto" scrollbar, positioned at
/// `options.scroll_offset`. Returns the rendered lines for the caller to append
/// to its output.
pub fn render_scrollable_list(
	rows: &[String],
	options: ScrollableListOptions,
	theme: &Theme,
) -> Vec<String> {
	let mut view = ScrollView::new(rows.to_vec(), rows.len());
	view.set_scrollbar_mode(ScrollbarMode::Auto);
	view.set_total_rows(Some(options.total_rows));
	view.set_scrollbar_style(ScrollViewTheme {
		track: theme_foreground_style(theme, "muted"),
		thumb: theme_foreground_style(theme, "accent"),
	});
	view.scroll_to(options.scroll_offset);
	view.render(options.width)
}

/// Center a viewport window of `max_visible` rows on `selected_index` within a
/// list of `total` rows, clamped to valid bounds.
pub fn centered_window(selected_index: usize, total: usize, max_visible: usize) -> CenteredWindow {
	let range = centered_viewport_range(selected_index, total, max_visible);
	CenteredWindow {
		start_index: range.start,
		end_index: range.end,
	}
}

/// Width available for row content, reserving the rightmost column for the
/// scrollbar when the list overflows its visible window.
pub fn content_row_width(width: usize, total: usize, max_visible: usize) -> usize {
	width.saturating_sub(if viewport_overflows(total, max_visible) {
		1
	} else {
		0
	})
}

/// Clamp `selected_index` into `[0, total)` and nudge `scroll_offset` so the
/// selection stays within the visible window of `max_visible` rows. On an empty
/// list both reset to 0.
pub fn clamp_selection(
	selected_index: usize,
	scroll_offset: usize,
	total: usize,
	max_visible: usize,
) -> ClampedSelection {
	if total == 0 {
		return ClampedSelection {
			selected_index: 0,
			scroll_offset: 0,
		};
	}
	let selected = selected_index.min(total - 1);
	let scroll = scroll_offset_for_row(
		scroll_offset,
		selected,
		total,
		max_visible,
		ViewportAlignment::Nearest,
	);
	ClampedSelection {
		selected_index: selected,
		scroll_offset: scroll,
	}
}

/// Handle the shared tab-cycling keys: Tab/Right advance to the next tab,
/// Shift+Tab/Left to the previous. Invokes `switch_tab` with the direction
/// (`1` / `-1`) and returns true when the key was consumed.
pub fn handle_tab_switch_key(key: &Key, switch_tab: &mut dyn FnMut(i8)) -> bool {
	if is_tab(key) || is_arrow_right(key) {
		switch_tab(1);
		return true;
	}
	if is_shift_tab(key) || is_arrow_left(key) {
		switch_tab(-1);
		return true;
	}
	false
}

/// Pad `lines` with blank rows up to `rows` so a full-screen overlay covers the
/// viewport instead of letting the transcript peek through below it. Copies
/// before padding — the source slice may be component-owned.
pub fn pad_lines_to_height(lines: &[String], rows: usize) -> Vec<String> {
	if lines.len() >= rows {
		return lines.to_vec();
	}
	let mut padded = lines.to_vec();
	padded.resize(rows, String::new());
	padded
}

/// Resolve a theme foreground token into the [`Style`] the scrollbar painter
/// accepts. Unknown tokens and terminal-default colors stay unstyled.
fn theme_foreground_style(theme: &Theme, token: &str) -> Style {
	let Ok(ansi) = theme.get_fg_ansi(token) else {
		return Style::default();
	};
	style_from_foreground_ansi(&ansi)
}

/// Parse a `38;2;R;G;B` or `38;5;N` foreground SGR into a [`Color`].
fn style_from_foreground_ansi(ansi: &str) -> Style {
	if let Some(rest) = ansi.strip_prefix("\x1b[38;2;") {
		let rest = rest.strip_suffix('m').unwrap_or(rest);
		let parts: Vec<u8> = rest.split(';').filter_map(|part| part.parse().ok()).collect();
		if let [r, g, b] = parts.as_slice() {
			return Style::fg(Color::Rgb(*r, *g, *b));
		}
	}
	if let Some(rest) = ansi.strip_prefix("\x1b[38;5;") {
		let rest = rest.strip_suffix('m').unwrap_or(rest);
		if let Ok(index) = rest.parse::<u8>() {
			return Style::fg(Color::Indexed(index));
		}
	}
	Style::default()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::keys::Key;
	use crate::theme::builtin;
	use crate::theme::color::ColorMode;
	use crate::theme::{create_theme, CreateThemeOptions};
	use std::cell::Cell;

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin theme"),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.expect("theme resolves")
	}

	#[test]
	fn centered_window_clamps_to_bounds() {
		assert_eq!(centered_window(0, 10, 4), CenteredWindow { start_index: 0, end_index: 4 });
		assert_eq!(
			centered_window(9, 10, 4),
			CenteredWindow { start_index: 6, end_index: 10 }
		);
		assert_eq!(centered_window(5, 10, 4), CenteredWindow { start_index: 4, end_index: 8 });
		assert_eq!(centered_window(3, 0, 4), CenteredWindow { start_index: 0, end_index: 0 });
	}

	#[test]
	fn content_row_width_reserves_the_scrollbar_column_only_on_overflow() {
		assert_eq!(content_row_width(20, 10, 4), 19);
		assert_eq!(content_row_width(20, 4, 4), 20);
		assert_eq!(content_row_width(0, 10, 4), 0);
	}

	#[test]
	fn clamp_selection_resets_on_empty_and_keeps_selection_visible() {
		assert_eq!(
			clamp_selection(7, 3, 0, 4),
			ClampedSelection { selected_index: 0, scroll_offset: 0 }
		);
		assert_eq!(
			clamp_selection(9, 0, 10, 4),
			ClampedSelection { selected_index: 9, scroll_offset: 6 }
		);
		assert_eq!(
			clamp_selection(2, 0, 10, 4),
			ClampedSelection { selected_index: 2, scroll_offset: 0 }
		);
	}

	#[test]
	fn tab_switch_keys_report_their_direction() {
		let seen = Cell::new(0i8);
		assert!(handle_tab_switch_key(&Key::Tab, &mut |direction| seen.set(direction)));
		assert_eq!(seen.get(), 1);
		assert!(handle_tab_switch_key(&Key::Right, &mut |direction| seen.set(direction)));
		assert_eq!(seen.get(), 1);
		assert!(handle_tab_switch_key(&Key::BackTab, &mut |direction| seen.set(direction)));
		assert_eq!(seen.get(), -1);
		assert!(handle_tab_switch_key(&Key::Left, &mut |direction| seen.set(direction)));
		assert_eq!(seen.get(), -1);
		assert!(!handle_tab_switch_key(&Key::Enter, &mut |_| {}));
	}

	#[test]
	fn pad_lines_to_height_copies_then_fills() {
		let lines = vec!["a".to_string()];
		let padded = pad_lines_to_height(&lines, 3);
		assert_eq!(padded, vec!["a".to_string(), String::new(), String::new()]);
		assert_eq!(lines.len(), 1);
		assert_eq!(pad_lines_to_height(&lines, 1), vec!["a".to_string()]);
		assert_eq!(pad_lines_to_height(&lines, 0), lines);
	}

	#[test]
	fn scrollable_list_shows_the_muted_track_and_accent_thumb() {
		let rows: Vec<String> = (0..6).map(|index| format!("row {index}")).collect();
		let rendered = render_scrollable_list(
			&rows,
			ScrollableListOptions { width: 12, total_rows: 8, scroll_offset: 0 },
			&theme(),
		);
		assert_eq!(rendered.len(), 6);
		// The track column is the theme's muted gray; the thumb is the accent.
		assert!(rendered[0].ends_with("\x1b[38;2;254;188;56m█\x1b[0m"));
		assert!(rendered[4].ends_with("\x1b[38;2;119;125;136m│\x1b[0m"));
	}

	#[test]
	fn scrollable_list_hides_the_scrollbar_when_content_fits() {
		let rows = vec!["only".to_string()];
		let rendered = render_scrollable_list(
			&rows,
			ScrollableListOptions { width: 8, total_rows: 1, scroll_offset: 0 },
			&theme(),
		);
		assert_eq!(rendered, vec!["only".to_string()]);
	}

	#[test]
	fn scrollable_list_scrolls_to_the_offset_row() {
		let rows: Vec<String> = (0..6).map(|index| format!("r{index}")).collect();
		let rendered = render_scrollable_list(
			&rows,
			ScrollableListOptions { width: 12, total_rows: 8, scroll_offset: 2 },
			&theme(),
		);
		assert!(rendered[0].starts_with("r2"));
	}
}
