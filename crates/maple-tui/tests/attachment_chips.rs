//! Port of `l/test/attachment-chips.test.ts`.
//!
//! Deviations: the reference probes PNG headers and draws Kitty placeholder
//! thumbnails; the crate reads [`ImageContent::cached_dimensions`] and renders
//! the centered icon when no thumbnail renderer is attached, so the card tests
//! assert the caption/snippet contracts rather than image pixels. The Kitty
//! terminal-capability describes in the reference have no crate counterpart.

use maple_tui::component::Component;
use maple_tui::prompt::attachment_chips::AttachmentChipsBand;
use maple_tui::prompt::composer_attachments::{chip_label, ChipKind};
use maple_tui::prompt::custom_editor::CustomEditor;
use maple_tui::prompt::image_source::{ImageContent, ImageDimensions};
use maple_tui::theme::{default_theme, Theme};

fn strip(rows: &[String]) -> Vec<String> {
	rows.iter().map(|row| maple_tui::strip_ansi(row)).collect()
}

fn tiny_image() -> ImageContent {
	let mut image = ImageContent::new("aVCTdWZm", "image/png");
	image.set_cached_image_dimensions(Some(ImageDimensions { width: 2, height: 2 }));
	image
}

#[test]
fn renders_nothing_while_no_attachment_is_staged() {
	let mut editor = CustomEditor::new();
	let theme = default_theme();
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	assert!(band.render(80).is_empty());
}

#[test]
fn renders_a_six_row_card_with_the_chip_token_and_paste_line_caption() {
	let mut editor = CustomEditor::new();
	let n = editor.add_pending_text("line1\nline2\nline3", None);
	let theme = default_theme();
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	let rows = strip(&band.render(80));
	assert_eq!(rows.len(), 6);
	assert!(
		rows[0].contains(chip_label(ChipKind::Paste, n, Some(&theme)).as_str()),
		"{}",
		rows[0]
	);
	assert!(rows[5].contains("+3 lines"), "{}", rows[5]);
	assert!(rows[1].contains("line1"), "{}", rows[1]);
	assert!(rows[2].contains("line2"), "{}", rows[2]);
}

#[test]
fn captions_a_single_line_paste_with_its_character_count() {
	let mut editor = CustomEditor::new();
	editor.add_pending_text("x".repeat(42), None);
	let theme = default_theme();
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	let rows = strip(&band.render(80));
	assert!(rows[5].contains("42 chars"), "{}", rows[5]);
}

#[test]
fn hides_the_card_once_its_inline_token_is_deleted_from_the_buffer() {
	let mut editor = CustomEditor::new();
	editor.add_pending_text("line1\nline2", None);
	let theme = default_theme();
	{
		let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
		assert_eq!(band.render(80).len(), 6);
	}
	editor.set_text("no token here");
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	assert!(band.render(80).is_empty());
}

#[test]
fn captions_an_image_card_with_pixel_dimensions_and_shows_both_cards_side_by_side() {
	let mut editor = CustomEditor::new();
	editor.add_pending_image(tiny_image(), None);
	let paste_n = editor.add_pending_text("hello", None);
	let image_n = editor
		.composer_chips()
		.iter()
		.find(|chip| chip.kind() == ChipKind::Image)
		.expect("image chip")
		.n();
	let theme = default_theme();
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	let rows = strip(&band.render(80));
	assert_eq!(rows.len(), 6);
	assert!(
		rows[0].contains(chip_label(ChipKind::Image, image_n, Some(&theme)).as_str()),
		"{}",
		rows[0]
	);
	assert!(
		rows[0].contains(chip_label(ChipKind::Paste, paste_n, Some(&theme)).as_str()),
		"{}",
		rows[0]
	);
	assert!(rows[5].contains("2x2"), "{}", rows[5]);
	assert!(rows[5].contains("5 chars"), "{}", rows[5]);
}

#[test]
fn renders_a_contact_sheet_preview_under_a_distinct_video_chip() {
	let mut editor = CustomEditor::new();
	editor.add_pending_image(tiny_image(), Some("/tmp/launch.mp4".to_string()));
	let chips = editor.composer_chips();
	let video_n = chips
		.iter()
		.find(|chip| chip.kind() == ChipKind::Video)
		.expect("video chip")
		.n();
	assert!(!chips.iter().any(|chip| chip.kind() == ChipKind::Image));
	let theme = default_theme();
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	let rows = strip(&band.render(80));
	assert!(
		rows[0].contains(chip_label(ChipKind::Video, video_n, Some(&theme)).as_str()),
		"{}",
		rows[0]
	);
	assert!(rows[5].contains("2x2"), "{}", rows[5]);
}

#[test]
fn omits_cards_that_do_not_fit_the_terminal_width_instead_of_wrapping() {
	let mut editor = CustomEditor::new();
	let first = editor.add_pending_text("first", None);
	let second = editor.add_pending_text("second", None);
	let theme = default_theme();
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	let rows = strip(&band.render(20));
	assert!(
		rows[0].contains(chip_label(ChipKind::Paste, first, Some(&theme)).as_str()),
		"{}",
		rows[0]
	);
	assert!(
		!rows[0].contains(chip_label(ChipKind::Paste, second, Some(&theme)).as_str()),
		"14 cols per card + 2 gap: only the first fits at width 20: {}",
		rows[0]
	);
}

#[test]
#[ignore = "missing facility: no TERMINAL image-protocol/cell-dimension global or Kitty placeholder renderer"]
fn kitty_placeholder_thumbnails_keep_card_rows_aligned() {
	let mut editor = CustomEditor::new();
	editor.add_pending_image(tiny_image(), None);
	let theme: Theme = default_theme();
	let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
	let rows = strip(&band.render(80));
	assert!(rows.iter().all(|row| row.chars().count() <= 14));
}
