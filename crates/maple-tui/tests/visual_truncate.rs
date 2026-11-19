//! Port of `l/test/visual-truncate.test.ts`.

use maple_tui::chrome::visual_truncate::truncate_to_visual_lines;

#[test]
fn returns_empty_output_for_empty_text() {
	let result = truncate_to_visual_lines("", 3, 10, 0);
	assert!(result.visual_lines.is_empty());
	assert_eq!(result.skipped_count, 0);
}

#[test]
fn truncates_to_the_last_visual_lines_after_wrapping() {
	let result = truncate_to_visual_lines("one two three four", 1, 10, 0);
	assert_eq!(result.visual_lines, vec!["three four".to_string()]);
	assert_eq!(result.skipped_count, 1);
}

#[test]
fn applies_horizontal_padding_to_rendered_lines() {
	let result = truncate_to_visual_lines("one", 1, 5, 1);
	assert_eq!(result.visual_lines, vec![" one ".to_string()]);
	assert_eq!(result.skipped_count, 0);
}
