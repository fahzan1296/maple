//! Shared renderer for a horizontal row of colored "segments" styled after the
//! status line: each segment is colored by its track position from the theme's
//! own palette, the active one is filled as a powerline chip (its color as the
//! background, a luminance-matched label, flanked by triangle caps) and the
//! rest are plain colored labels joined by a thin separator.
//!
//! Reference: `l/src/chrome/segment-track.ts`.
//!
//! Deviations:
//! - The reference reads the process-global `theme`; this port passes `&Theme`
//!   explicitly.
//! - The reference's `ThemeColor` string union maps to the typed
//!   [`ThemeColor`](crate::theme::ThemeColor) enum.

use crate::native::describe::{node, row};
use crate::native::node::{NativeNode, NodeKind, Props, Span, Text as NodeText};
use crate::theme::color::{BG_RESET, FG_RESET};
use crate::theme::{Theme, ThemeColor};

/// One labeled segment of a track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackSegment {
	/// Visible label.
	pub label: String,
}

impl TrackSegment {
	/// Create a segment with `label`.
	pub fn new(label: impl Into<String>) -> Self {
		TrackSegment { label: label.into() }
	}
}

/// Vivid theme colors for position-based segment coloring, in preference order.
/// Themes alias many of these to the same value, so [`resolve_segment_palette`]
/// dedupes by resolved escape and hands position `i` the `i`-th distinct color.
const SEGMENT_COLOR_CANDIDATES: &[ThemeColor] = &[
	ThemeColor::Accent,
	ThemeColor::Success,
	ThemeColor::Warning,
	ThemeColor::Error,
	ThemeColor::MdCode,
	ThemeColor::MdLink,
	ThemeColor::SyntaxString,
	ThemeColor::SyntaxKeyword,
	ThemeColor::SyntaxFunction,
	ThemeColor::SyntaxNumber,
	ThemeColor::SyntaxOperator,
	ThemeColor::SyntaxVariable,
];

/// Resolve up to `count` theme colors that render distinctly under the active
/// theme, in candidate preference order. May return fewer than `count` when the
/// theme has fewer distinct hues (e.g. monochrome themes) — callers wrap with
/// modulo. Never returns an empty array: `accent` always resolves.
pub fn resolve_segment_palette(count: usize, theme: &Theme) -> Vec<ThemeColor> {
	let mut palette: Vec<ThemeColor> = Vec::new();
	let mut seen: Vec<String> = Vec::new();
	for &color in SEGMENT_COLOR_CANDIDATES {
		let ansi = theme.get_fg_ansi(color).unwrap_or_default();
		if seen.contains(&ansi) {
			continue;
		}
		seen.push(ansi);
		palette.push(color);
		if palette.len() >= count {
			break;
		}
	}
	palette
}

/// Native [`render_segment_track`]: a row of position-colored labels, the
/// active one roled `omp.track.active` so the terminal fills it as a chip in
/// its own color. No caps or separators travel; the terminal draws them.
pub fn describe_segment_track(
	segments: &[TrackSegment],
	active_index: usize,
	theme: &Theme,
) -> NativeNode {
	let palette = resolve_segment_palette(segments.len(), theme);
	let children = segments
		.iter()
		.enumerate()
		.map(|(index, segment)| {
			let mut props = Props::new();
			props.set_spans(vec![Span::styled(
				segment.label.clone(),
				palette[index % palette.len()].as_str(),
			)]);
			props.set_str("wrap", "none");
			props.set_str(
				"role",
				if index == active_index {
					"omp.track.active"
				} else {
					"omp.track.item"
				},
			);
			node(NodeKind::Text, props, Vec::new(), Some(&index.to_string()))
		})
		.collect();
	let mut props = Props::new();
	props.set_str("gap", "sm");
	props.set_str("align", "center");
	props.set_str("role", "omp.track");
	row(children, props)
}

/// Render `segments` as a colored chip track with `active_index` filled. Returns
/// a single line of styled text with no surrounding caption or arrows — callers
/// frame it as they need.
pub fn render_segment_track(
	segments: &[TrackSegment],
	active_index: usize,
	theme: &Theme,
) -> String {
	// Powerline triangles point *into* the chip so the colored caps merge with
	// the filled body: left cap points left, right cap points right.
	let separators = theme.sep();
	let cap_left = separators.powerline_right.clone();
	let cap_right = separators.powerline_left.clone();
	let thin_sep = theme
		.fg("statusLineSep", &separators.powerline_thin)
		.unwrap_or_else(|_| separators.powerline_thin.clone());
	let palette = resolve_segment_palette(segments.len(), theme);

	let mut track = String::new();
	for (index, segment) in segments.iter().enumerate() {
		if index > 0 {
			// A thin separator reads cleanly only between two plain labels; the
			// chip caps already delimit the active segment, so pad around it.
			if index == active_index || index - 1 == active_index {
				track.push_str("  ");
			} else {
				track.push_str(" ");
				track.push_str(&thin_sep);
				track.push(' ');
			}
		}
		let color = palette[index % palette.len()];
		let fg = theme.get_fg_ansi(color).unwrap_or_default();
		if index != active_index {
			track.push_str(&fg);
			track.push_str(&segment.label);
			track.push_str(FG_RESET);
			continue;
		}
		let bg = fg.replace("\x1b[38;", "\x1b[48;");
		let contrast = theme.get_contrast_fg_ansi(color.as_str());
		track.push_str(&fg);
		track.push_str(&cap_left);
		track.push_str(&bg);
		track.push_str(&contrast);
		track.push_str("\x1b[1m ");
		track.push_str(&segment.label);
		track.push_str(" \x1b[22m");
		track.push_str(BG_RESET);
		track.push_str(&fg);
		track.push_str(&cap_right);
		track.push_str(FG_RESET);
	}
	track
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::color::ColorMode;
	use crate::theme::{create_theme, CreateThemeOptions};

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

	fn aliased_theme() -> Theme {
		let mut json = builtin::builtin_theme_json("dark").expect("builtin theme").clone();
		let accent = json.colors.get("accent").expect("accent").clone();
		for token in [
			"mdCode",
			"mdLink",
			"syntaxString",
			"syntaxKeyword",
			"syntaxFunction",
			"syntaxNumber",
			"syntaxOperator",
			"syntaxVariable",
		] {
			json.colors.insert(token.to_string(), accent.clone());
		}
		create_theme(
			&json,
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.expect("theme resolves")
	}

	fn segments(labels: &[&str]) -> Vec<TrackSegment> {
		labels.iter().map(|label| TrackSegment::new(*label)).collect()
	}

	#[test]
	fn palette_follows_candidate_order_and_stops_at_count() {
		let palette = resolve_segment_palette(4, &theme());
		assert_eq!(
			palette,
			vec![
				ThemeColor::Accent,
				ThemeColor::Success,
				ThemeColor::Warning,
				ThemeColor::Error
			]
		);
	}

	#[test]
	fn palette_dedupes_aliased_colors_and_never_empties() {
		let palette = resolve_segment_palette(12, &aliased_theme());
		assert_eq!(palette, vec![ThemeColor::Accent, ThemeColor::Success, ThemeColor::Warning, ThemeColor::Error]);
		// `count` 0 still resolves the first candidate before the length check.
		assert_eq!(resolve_segment_palette(0, &theme()), vec![ThemeColor::Accent]);
	}

	#[test]
	fn describe_marks_the_active_segment() {
		let described = describe_segment_track(&segments(&["a", "b", "c"]), 1, &theme());
		assert_eq!(described.kind, NodeKind::Row);
		assert_eq!(described.props.role(), Some("omp.track"));
		let children = described.children;
		assert_eq!(children.len(), 3);
		assert_eq!(children[1].props.role(), Some("omp.track.active"));
		assert_eq!(children[0].props.role(), Some("omp.track.item"));
		assert_eq!(children[1].key.as_deref(), Some("1"));
	}

	#[test]
	fn describe_colours_each_position_from_the_palette() {
		let described = describe_segment_track(&segments(&["a", "b"]), 0, &theme());
		let first = described.children[0].props.spans().expect("spans");
		let NodeText::Spans(first) = first.as_slice() else {
			panic!("expected spans");
		};
		assert_eq!(first[0].s.as_deref(), Some("accent"));
		let second = described.children[1].props.spans().expect("spans");
		let NodeText::Spans(second) = second.as_slice() else {
			panic!("expected spans");
		};
		assert_eq!(second[0].s.as_deref(), Some("success"));
	}

	#[test]
	fn render_fills_the_active_chip_with_caps_and_contrast_label() {
		let rendered = render_segment_track(&segments(&["fast", "safe"]), 1, &theme());
		assert!(rendered.contains("\x1b[38;2;137;210;129m"), "{rendered}");
		assert!(rendered.contains("\x1b[48;2;137;210;129m"), "{rendered}");
		assert!(rendered.contains("\x1b[1m safe \x1b[22m"), "{rendered}");
		assert!(rendered.contains("◀"), "{rendered}");
		assert!(rendered.contains("▶"), "{rendered}");
	}

	#[test]
	fn render_separates_plain_labels_with_the_thin_separator() {
		let rendered = render_segment_track(&segments(&["a", "b", "c"]), 0, &theme());
		assert!(rendered.contains("┆"), "{rendered}");
		// The chip's neighbours are padded instead of separated.
		assert!(rendered.contains("  "), "{rendered}");
	}

	#[test]
	fn render_wraps_the_palette_for_longer_tracks() {
		let many = segments(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"]);
		let rendered = render_segment_track(&many, 12, &theme());
		// Position 12 wraps back to the accent colour.
		assert!(rendered.contains("\x1b[38;2;254;188;56m"), "{rendered}");
	}
}
