//! Port of `l/test/output-block.test.ts`.

use maple_tui::chrome::overlay_box::{OverlayPanel, PanelRows};
use maple_tui::component::Component;
use maple_tui::render::code_cell::{render_markdown_cell, CodeCellStatus, MarkdownCellOptions};
use maple_tui::render::output_block::{render_output_block, OutputBlockOptions, OutputSection};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};
use maple_tui::strip_ansi;

fn theme() -> Theme {
	let json = builtin_theme_json("dark").expect("dark theme");
	create_theme(json, CreateThemeOptions::default()).expect("theme")
}

fn plain(lines: &[String]) -> Vec<String> {
	lines.iter().map(|line| strip_ansi(line)).collect()
}

#[test]
fn keeps_frame_rows_inside_a_one_column_viewport() {
	let theme = theme();
	let lines = render_output_block(
		&OutputBlockOptions {
			width: 1,
			header: "Heading".to_string(),
			sections: vec![OutputSection::new(vec!["x".to_string()])],
			..OutputBlockOptions::default()
		},
		&theme,
	);
	assert!(!lines.is_empty());
	for line in &lines {
		assert_eq!(visible_width(line), 1, "{line:?}");
	}
}

#[test]
fn reserves_symmetric_default_padding_inside_content_borders() {
	let theme = theme();
	let lines = plain(&render_output_block(
		&OutputBlockOptions {
			width: 16,
			apply_bg: false,
			sections: vec![OutputSection::new(vec!["abcdefghijklmnop".to_string()])],
			..OutputBlockOptions::default()
		},
		&theme,
	));
	assert_eq!(
		lines.iter().filter(|line| line.starts_with('│')).collect::<Vec<_>>(),
		vec![&"│ abcdefghijkl │".to_string(), &"│ mnop         │".to_string()]
	);
}

#[test]
fn draws_footer_meta_inside_the_bottom_border_at_full_width() {
	let theme = theme();
	let lines = plain(&render_output_block(
		&OutputBlockOptions {
			width: 30,
			apply_bg: false,
			footer_meta: "Wall 0.16s".to_string(),
			sections: vec![OutputSection::new(vec!["out".to_string()])],
			..OutputBlockOptions::default()
		},
		&theme,
	));
	assert_eq!(lines.last().map(String::as_str), Some("╰─── Wall 0.16s ─────────────╯"));
	assert!(!lines.iter().any(|line| line.starts_with('│') && line.contains("Wall")));
}

#[test]
fn wraps_overflowing_footer_meta_into_body_rows() {
	let theme = theme();
	let meta = "Wall: 1.20s · Timeout: 300s · Artifact: a123 · Exit: 7";
	let lines = plain(&render_output_block(
		&OutputBlockOptions {
			width: 40,
			apply_bg: false,
			footer_meta: meta.to_string(),
			sections: vec![OutputSection::new(vec!["out".to_string()])],
			..OutputBlockOptions::default()
		},
		&theme,
	));
	let expected_bottom = format!("╰{}╯", "─".repeat(38));
	assert_eq!(lines.last().map(String::as_str), Some(expected_bottom.as_str()));
	let body: String = lines
		.iter()
		.filter(|line| line.starts_with('│'))
		.cloned()
		.collect::<Vec<_>>()
		.join(" ");
	assert!(body.contains("Artifact: a123"), "{body}");
	assert!(body.contains("Exit: 7"), "{body}");
}

#[test]
fn keeps_explicitly_flush_content_flush_on_both_sides() {
	let theme = theme();
	let lines = plain(&render_output_block(
		&OutputBlockOptions {
			width: 16,
			apply_bg: false,
			content_padding_left: Some(0),
			sections: vec![OutputSection::new(vec!["abcdefghijklmn".to_string()])],
			..OutputBlockOptions::default()
		},
		&theme,
	));
	assert_eq!(
		lines.iter().filter(|line| line.starts_with('│')).collect::<Vec<_>>(),
		vec![&"│abcdefghijklmn│".to_string()]
	);
}

#[test]
fn budgets_collapsed_markdown_rows_against_the_padded_block_width() {
	let theme = theme();
	let lines = plain(&render_markdown_cell(
		&MarkdownCellOptions {
			content: "x".repeat(27),
			content_max_lines: Some(1),
			status: Some(CodeCellStatus::Complete),
			title: Some("Read".to_string()),
			width: 30,
		},
		&theme,
	));
	assert_eq!(lines[1], "│ ".to_string() + &"x".repeat(26) + " │");
	assert!(lines[2].starts_with("│ … 1 more line"), "{:?}", lines[2]);
}

#[test]
fn keeps_overlay_frame_rows_inside_a_one_column_viewport() {
	let mut panel = OverlayPanel::new("Heading", "omp.overlay");
	let mut rows = PanelRows::new();
	rows.set_lines(&["x".to_string()]);
	panel.add_child(rows);
	let lines = panel.render(1);
	assert!(!lines.is_empty());
	for line in &lines {
		assert_eq!(visible_width(line), 1, "{line:?}");
	}
}

