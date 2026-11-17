//! Reference: `l/src/prompt/attachment-chips.ts`.
//!
//! The composer attachment band: one rounded card per staged attachment,
//! rendered directly above the prompt box. Image and video cards show a live
//! thumbnail with the pixel dimensions as the bottom caption; text-paste cards
//! show the leading snippet with a `+N lines` / `N chars` caption. The top
//! caption is the same `<icon> #N` token that sits in the editor buffer, in the
//! same identity color. Cards that no longer fit the terminal width are omitted
//! rather than wrapped. Renders to nothing while no attachment is staged.
//!
//! Deviations:
//! - No global theme and no `TERMINAL` singleton: the band takes `Option<&Theme>`
//!   styling and a caller-injected [`ChipThumbnailRenderer`].
//! - The crate has no image renderer or live-graphics budget, so the Kitty
//!   placeholder thumbnail path is not ported: without a renderer (or for a
//!   non-Kitty terminal) the interior falls back to the centered icon, exactly
//!   the reference's fallback branch. The band never transmits or converts
//!   images itself.
//! - Pixel dimensions come from the draft's probe cache
//!   ([`ImageContent::cached_dimensions`]); the header probe itself lives with
//!   the image loader.

use crate::component::Component;
use crate::prompt::composer_attachments::{ChipKind, attachment_sgr};
use crate::prompt::custom_editor::{ComposerChipDescriptor, CustomEditor};
use crate::prompt::image_source::{ImageContent, ImageDimensions};
use crate::render::hyperlink::file_hyperlink;
use crate::render::render_utils::replace_tabs;
use crate::text_shape::{Ellipsis, truncate_to_width, visible_width};
use crate::theme::theme_class::Theme;

/// Content area of a chip card.
pub const INNER_COLS: usize = 12;
/// Rows of a chip card's content area.
pub const INNER_ROWS: usize = 4;
/// A card including its one-cell border.
pub const CARD_COLS: usize = INNER_COLS + 2;
/// Blank cells between cards.
pub const CARD_GAP: usize = 2;
/// Rows a band renders per card (two borders plus the content area).
pub const CARD_ROWS: usize = INNER_ROWS + 2;

const RESET_FG: &str = "\x1b[39m";

/// Host-provided thumbnail painter: a Kitty Unicode-placeholder cell grid for
/// `(image, dims)` inside `cols` × `rows`, or `None` to fall back to the icon.
pub trait ChipThumbnailRenderer {
	fn render_thumbnail(
		&mut self,
		image: &ImageContent,
		dims: Option<&ImageDimensions>,
		kind: ChipKind,
		cols: usize,
		rows: usize,
	) -> Option<Vec<String>>;
}

/// The composer attachment band: one rounded card per staged attachment.
pub struct AttachmentChipsBand<'a> {
	editor: &'a mut CustomEditor,
	theme: Option<&'a Theme>,
	thumbnails: Option<Box<dyn ChipThumbnailRenderer + 'a>>,
}

impl<'a> AttachmentChipsBand<'a> {
	pub fn new(editor: &'a mut CustomEditor, theme: Option<&'a Theme>) -> Self {
		AttachmentChipsBand {
			editor,
			theme,
			thumbnails: None,
		}
	}

	/// Attach the terminal's thumbnail painter (Kitty placeholders).
	pub fn with_thumbnail_renderer(mut self, renderer: Box<dyn ChipThumbnailRenderer + 'a>) -> Self {
		self.thumbnails = Some(renderer);
		self
	}

	fn symbol(&self, key: &str) -> String {
		match self.theme {
			Some(theme) => theme.symbol(key).to_string(),
			None => key.to_string(),
		}
	}

	fn chip_icon(&self, kind: ChipKind) -> String {
		let key = match kind {
			ChipKind::Paste => "chip.paste",
			ChipKind::Video => "chip.video",
			ChipKind::Image => "chip.image",
		};
		self.symbol(key)
	}

	fn muted(&self, text: &str) -> String {
		match self.theme {
			Some(theme) => theme.fg("muted", text).unwrap_or_else(|_| text.to_string()),
			None => text.to_string(),
		}
	}

	/// Horizontal border with an optional centered, bold caption padded by one
	/// space per side.
	fn border_row(&self, sgr: &str, caption: &str, top: bool) -> String {
		let left = self.symbol(if top { "boxRound.topLeft" } else { "boxRound.bottomLeft" });
		let right = self.symbol(if top { "boxRound.topRight" } else { "boxRound.bottomRight" });
		let horizontal = self.symbol("boxRound.horizontal");
		if caption.is_empty() {
			return format!("{sgr}{left}{}{right}{RESET_FG}", horizontal.repeat(INNER_COLS));
		}
		let cut = truncate_to_width(caption, INNER_COLS - 2, Ellipsis::Omit);
		let fill = INNER_COLS.saturating_sub(visible_width(&cut)).saturating_sub(2);
		let left_fill = fill / 2;
		let right_fill = fill - left_fill;
		format!(
			"{sgr}{left}{} \x1b[1m{cut}\x1b[22m {}{right}{RESET_FG}",
			horizontal.repeat(left_fill),
			horizontal.repeat(right_fill)
		)
	}

	/// Pixel dimensions for the caption and thumbnail fit, from the draft's
	/// probe cache.
	fn image_dims<'b>(&self, image: &'b ImageContent) -> Option<&'b ImageDimensions> {
		match &image.cached_dimensions {
			Some(Some(dims)) => Some(dims),
			_ => None,
		}
	}

	/// `12x4` thumbnail from the host renderer, centered; the icon otherwise.
	fn image_interior(
		&mut self,
		image: &ImageContent,
		dims: Option<&ImageDimensions>,
		kind: ChipKind,
	) -> Vec<String> {
		if let Some(renderer) = self.thumbnails.as_mut() {
			if let Some(lines) = renderer.render_thumbnail(image, dims, kind, INNER_COLS, INNER_ROWS) {
				return center_grid(&lines, INNER_COLS, INNER_ROWS);
			}
		}
		let icon = self.chip_icon(kind);
		let icon_width = visible_width(&icon);
		let pad = INNER_COLS.saturating_sub(icon_width);
		let icon_row = format!(
			"{}{}{}",
			" ".repeat(pad / 2),
			self.muted(&icon),
			" ".repeat(pad - pad / 2)
		);
		let blank = " ".repeat(INNER_COLS);
		let mut rows = vec![blank.clone(); INNER_ROWS];
		if let Some(slot) = rows.get_mut(1) {
			*slot = icon_row;
		}
		rows
	}

	/// Leading 4 rows × 12 cols of the pasted text, muted.
	fn text_interior(&self, text: &str) -> Vec<String> {
		// Only the leading rows are drawn; don't split a multi-KB paste every frame.
		let lines: Vec<&str> = text.split('\n').take(INNER_ROWS).collect();
		let mut rows = Vec::with_capacity(INNER_ROWS);
		for index in 0..INNER_ROWS {
			let cut = truncate_to_width(
				&replace_tabs(lines.get(index).copied().unwrap_or("")),
				INNER_COLS,
				Ellipsis::Omit,
			);
			let pad = INNER_COLS.saturating_sub(visible_width(&cut));
			rows.push(format!("{}{}", self.muted(&cut), " ".repeat(pad)));
		}
		rows
	}

	fn caption(&self, chip: &ComposerChipDescriptor) -> String {
		match chip {
			ComposerChipDescriptor::Image { image, .. } | ComposerChipDescriptor::Video { image, .. } => {
				match self.image_dims(image) {
					Some(dims) => format!("{}x{}", dims.width, dims.height),
					None => String::new(),
				}
			}
			ComposerChipDescriptor::Paste { text, .. } => {
				if text.line_count > 1 {
					format!("+{} lines", text.line_count)
				} else {
					format!("{} chars", text.char_count)
				}
			}
		}
	}

	/// A whole card: top border, [`INNER_ROWS`] content rows, bottom border.
	fn card(&mut self, chip: &ComposerChipDescriptor) -> Vec<String> {
		let kind = chip.kind();
		let n = chip.n();
		let sgr = attachment_sgr(kind, n);
		let icon = self.chip_icon(kind);
		let (bottom_caption, interior) = match chip {
			ComposerChipDescriptor::Paste { text, .. } => {
				let caption = self.caption(chip);
				(caption, self.text_interior(&text.content))
			}
			_ => {
				let image = match chip {
					ComposerChipDescriptor::Image { image, .. } | ComposerChipDescriptor::Video { image, .. } => image,
					ComposerChipDescriptor::Paste { .. } => unreachable!(),
				};
				let dims = self.image_dims(image);
				let interior = self.image_interior(image, dims, kind);
				(self.caption(chip), interior)
			}
		};

		let vertical = format!("{sgr}{}{RESET_FG}", self.symbol("boxRound.vertical"));

		let token = format!("{icon} #{n}");
		let title = match chip.link() {
			Some(link) if kind != ChipKind::Paste => file_hyperlink(link, &token, Default::default()),
			_ => token,
		};
		let mut rows = Vec::with_capacity(CARD_ROWS);
		rows.push(self.border_row(&sgr, &title, true));
		for row in interior {
			rows.push(format!("{vertical}{row}{vertical}"));
		}
		rows.push(self.border_row(&sgr, &bottom_caption, false));
		rows
	}
}

impl Component for AttachmentChipsBand<'_> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width as usize;
		let chips = self.editor.composer_chips();
		if chips.is_empty() {
			return Vec::new();
		}
		let mut rows: Vec<String> = vec![String::new(); CARD_ROWS];
		let gap = " ".repeat(CARD_GAP);
		let mut used = 0;
		let mut cards = Vec::new();
		for chip in &chips {
			// `used` counts the cells taken so far; every card after the first
			// also needs a gap.
			if used + CARD_COLS > width {
				break;
			}
			cards.push(self.card(chip));
			used += CARD_COLS + CARD_GAP;
		}
		for (index, card) in cards.into_iter().enumerate() {
			for (row, line) in rows.iter_mut().zip(card) {
				if index > 0 {
					row.push_str(&gap);
				}
				row.push_str(&line);
			}
		}
		rows
	}
}

/// Center a placeholder cell grid inside the content area.
pub fn center_grid(lines: &[String], cols: usize, rows: usize) -> Vec<String> {
	let mut out = Vec::with_capacity(rows);
	let top_pad = rows.saturating_sub(lines.len()) / 2;
	for index in 0..rows {
		let Some(line) = lines.get(index.wrapping_sub(top_pad)) else {
			out.push(" ".repeat(cols));
			continue;
		};
		let pad = cols.saturating_sub(visible_width(line));
		out.push(format!("{}{}{}", " ".repeat(pad / 2), line, " ".repeat(pad - pad / 2)));
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::prompt::custom_editor::CustomEditor;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};

	fn theme() -> Theme {
		let json = builtin_theme_json("dark").expect("builtin theme");
		create_theme(json, CreateThemeOptions::default()).expect("theme")
	}

	fn image(width: u32, height: u32) -> ImageContent {
		let mut image = ImageContent::new("aGk=", "image/png");
		image.set_cached_image_dimensions(Some(ImageDimensions { width, height }));
		image
	}

	#[test]
	fn empty_band_renders_nothing() {
		let mut editor = CustomEditor::new();
		let mut band = AttachmentChipsBand::new(&mut editor, None);
		assert!(band.render(80).is_empty());
	}

	#[test]
	fn cards_are_six_rows_wide_and_omit_what_does_not_fit() {
		let theme = theme();
		let mut editor = CustomEditor::new();
		editor.add_pending_text("one\ntwo", None);
		let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
		let rows = band.render(80);
		assert_eq!(rows.len(), CARD_ROWS);
		for row in &rows {
			assert_eq!(visible_width(row), CARD_COLS);
		}
		// Two cards need 14 + 2 + 14 columns.
		editor.add_pending_text("three", None);
		let mut band = AttachmentChipsBand::new(&mut editor, Some(&theme));
		let rows = band.render(CARD_COLS * 2 + 1);
		assert_eq!(visible_width(&rows[0]), CARD_COLS);
		let rows = band.render(CARD_COLS * 2 + CARD_GAP);
		assert_eq!(visible_width(&rows[0]), CARD_COLS * 2 + CARD_GAP);
	}

	#[test]
	fn paste_cards_show_the_leading_rows_and_a_count() {
		let mut editor = CustomEditor::new();
		editor.add_pending_text("alpha\nbeta\n\ngamma\ndelta", None);
		let mut band = AttachmentChipsBand::new(&mut editor, None);
		let rows = band.render(80);
		assert!(rows[1].contains("alpha"));
		assert!(rows[2].contains("beta"));
		// A blank third row, then only four content rows exist.
		assert!(!rows.join("\n").contains("delta"));
		assert!(rows[CARD_ROWS - 1].contains("+5 lines"));
	}

	#[test]
	fn short_paste_reports_chars() {
		let mut editor = CustomEditor::new();
		editor.add_pending_text("abc", None);
		let mut band = AttachmentChipsBand::new(&mut editor, None);
		let rows = band.render(80);
		assert!(rows[CARD_ROWS - 1].contains("3 chars"));
	}

	#[test]
	fn image_cards_fall_back_to_the_icon_and_show_dimensions() {
		let mut editor = CustomEditor::new();
		editor.add_pending_image(image(640, 480), None);
		let mut band = AttachmentChipsBand::new(&mut editor, None);
		let rows = band.render(80);
		assert!(rows[CARD_ROWS - 1].contains("640x480"));
		// The icon row is the second content row.
		assert_eq!(visible_width(&rows[2]), CARD_COLS);
	}

	#[test]
	fn injected_thumbnail_renderer_wins() {
		struct Fixed;
		impl ChipThumbnailRenderer for Fixed {
			fn render_thumbnail(
				&mut self,
				_image: &ImageContent,
				_dims: Option<&ImageDimensions>,
				_kind: ChipKind,
				cols: usize,
				_rows: usize,
			) -> Option<Vec<String>> {
				Some(vec!["x".repeat(cols)])
			}
		}
		let mut editor = CustomEditor::new();
		editor.add_pending_image(image(10, 10), None);
		let mut band = AttachmentChipsBand::new(&mut editor, None).with_thumbnail_renderer(Box::new(Fixed));
		let rows = band.render(80);
		assert!(rows[2].contains(&"x".repeat(INNER_COLS)));
		// The single provided row centers vertically, so the content area's
		// second row (index 1 under the top border) carries it.
		assert!(rows[CARD_ROWS - 1].contains("10x10"));
	}

	#[test]
	fn center_grid_centers_both_axes() {
		let rows = center_grid(&["abc".to_string()], 12, 4);
		assert_eq!(rows.len(), 4);
		assert_eq!(visible_width(&rows[0]), 12);
		assert!(rows[1].contains("abc"));
		assert_eq!(visible_width(&rows[1]), 12);
	}
}
