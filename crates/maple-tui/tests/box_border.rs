//! Port of `l/test/box-border.test.ts`.

use std::sync::Arc;

use maple_tui::components::r#box::{Box, BoxBorder, BoxBorderChars};
use maple_tui::components::Text;
use maple_tui::text_shape::visible_width;
use maple_tui::{strip_ansi, Component};

fn chars() -> BoxBorderChars {
	BoxBorderChars::new("+", "+", "+", "+", "-", "|")
}

fn bordered_box() -> Box {
	let border = BoxBorder {
		chars: chars(),
		color: Some(Arc::new(|text: &str| format!("\x1b[31m{text}\x1b[39m"))),
	};
	let mut box_component = Box::styled(
		1,
		0,
		Some(Arc::new(|text: &str| format!("\x1b[48;5;236m{text}\x1b[49m"))),
		Some(border),
	);
	box_component.set_ignore_tight(true);
	box_component.add_child(Text::new("hi"));
	box_component
}

fn widths(rows: &[String]) -> Vec<usize> {
	rows.iter().map(|row| visible_width(&strip_ansi(row))).collect()
}

fn plain(rows: &[String]) -> Vec<String> {
	rows.iter().map(|row| strip_ansi(row)).collect()
}

#[test]
fn frames_content_without_exceeding_the_given_width() {
	let rows = bordered_box().render(20);
	// top rule + single content row + bottom rule
	assert_eq!(rows.len(), 3);
	assert_eq!(widths(&rows), vec![20, 20, 20]);

	let flat = plain(&rows);
	assert_eq!(flat[0], format!("+{}+", "-".repeat(18)));
	assert_eq!(flat[2], format!("+{}+", "-".repeat(18)));
	// Interior rows are wrapped by the vertical glyph on both edges.
	assert!(flat[1].starts_with('|'));
	assert!(flat[1].ends_with('|'));
	assert!(flat[1].contains("hi"));
}

#[test]
fn paints_border_glyphs_with_the_supplied_colorizer() {
	let rows = bordered_box().render(20);
	// The top rule is emitted through the color fn (red SGR), the interior is not.
	assert!(rows[0].contains("\x1b[31m"));
}

#[test]
fn recomputes_width_when_the_border_is_toggled_off() {
	let mut box_component = bordered_box();
	assert_eq!(widths(&box_component.render(20)), vec![20, 20, 20]);

	box_component.set_border(None);
	let rows = box_component.render(20);
	// No border rows now; the single content row fills the full width.
	assert_eq!(rows.len(), 1);
	assert_eq!(visible_width(&strip_ansi(&rows[0])), 20);
	assert!(!strip_ansi(&rows[0]).contains('+'));
}

#[test]
fn frames_at_the_boundary_width_without_overflowing() {
	// paddingX 1 → the border needs width >= 5 (2 borders + 2 padding + 1 content col).
	let rows = bordered_box().render(5);
	assert!(rows.iter().all(|row| visible_width(&strip_ansi(row)) == 5));
	assert_eq!(plain(&rows)[0], format!("+{}+", "-".repeat(3)));
}

#[test]
fn drops_the_border_when_the_interior_cant_fit_padding_plus_content() {
	for width in [3, 4] {
		let rows = bordered_box().render(width);
		for row in &rows {
			assert!(visible_width(&strip_ansi(row)) <= width);
		}
		for line in plain(&rows) {
			assert!(!line.contains('+'));
		}
	}
}
