//! Adapters from a resolved [`Theme`] to the component theme objects.
//!
//! Reference: `l/src/theme/tui-adapters.ts`.
//!
//! # Divergences
//!
//! - The reference reads a module-level `theme` (and `ensureThemeSync`);
//!   this crate has no global theme, so every getter takes `&Theme`.
//! - The reference's component themes are closures over the global theme. The
//!   crate's [`MarkdownTheme`](crate::components::markdown::MarkdownTheme),
//!   [`SelectListTheme`](crate::components::select_list::SelectListTheme),
//!   [`SettingsListTheme`](crate::components::settings_list::SettingsListTheme)
//!   and [`EditorTheme`](crate::components::editor::EditorTheme) hold `Style`
//!   values instead, so roles map onto fields (see each getter).
//! - No native syntax highlighter exists in this crate, so `highlightCode`,
//!   `createHighlightStream`, `warmHighlighter` and the highlight fields of the
//!   reference's `MarkdownTheme` are absent.
//! - `markdownMermaidRendering` / `markdownColorful` are parameters of
//!   [`get_markdown_theme`] rather than process-wide toggles.
//! - `getMarkdownThemeWithLinkTargets` and `canCacheMarkdownListItems` rely on
//!   `WeakMap` object identity and mutable closure fields; neither is
//!   expressible, and the crate's `Markdown` resolves links through its own
//!   callback.

use crate::components::editor::EditorTheme;
use crate::components::markdown::MarkdownTheme;
use crate::components::select_list::SelectListTheme;
use crate::components::settings_list::SettingsListTheme;
use crate::style::{Color, Style};
use crate::symbols::{LineSymbols, SymbolTheme, TableSymbols};
use crate::theme::schema::AnsiColor;
use crate::theme::symbols::{SpinnerType, SymbolPreset};
use crate::theme::theme_class::{BoxRoundSymbols, BoxSharpSymbols};
use crate::theme::Theme;

/// Parse the SGR parameters of a painted string (empty text) into a `Style`.
fn style_from_sgr(painted: &str) -> Style {
	let params = match painted.strip_prefix("\x1b[") {
		Some(rest) => rest.split('m').next().unwrap_or(""),
		None => return Style::default(),
	};
	let parts: Vec<&str> = params.split(';').collect();
	match parts.as_slice() {
		["38", "2", r, g, b] => match (r.parse(), g.parse(), b.parse()) {
			(Ok(r), Ok(g), Ok(b)) => Style::fg(Color::Rgb(r, g, b)),
			_ => Style::default(),
		},
		["38", "5", index] => match index.parse() {
			Ok(index) => Style::fg(Color::Indexed(index)),
			Err(_) => Style::default(),
		},
		["48", "2", r, g, b] => match (r.parse(), g.parse(), b.parse()) {
			(Ok(r), Ok(g), Ok(b)) => Style::bg(Color::Rgb(r, g, b)),
			_ => Style::default(),
		},
		["48", "5", index] => match index.parse() {
			Ok(index) => Style::bg(Color::Indexed(index)),
			Err(_) => Style::default(),
		},
		_ => Style::default(),
	}
}

/// Foreground token → `Style`; unknown tokens and terminal defaults stay unstyled.
pub fn fg_style(theme: &Theme, token: &str) -> Style {
	match theme.get_fg_ansi(token) {
		Ok(ansi) => style_from_sgr(&ansi),
		Err(_) => Style::default(),
	}
}

/// Background token → `Style`; unknown tokens stay unstyled.
pub fn bg_style(theme: &Theme, token: &str) -> Style {
	match theme.get_bg_ansi(token) {
		Ok(ansi) => style_from_sgr(&ansi),
		Err(_) => Style::default(),
	}
}

/// One slot of the theme's 16-color ANSI palette as a `Style`.
fn palette_style(theme: &Theme, color: AnsiColor) -> Style {
	style_from_sgr(&theme.ansi_fg(color, ""))
}

fn line_symbols(source: &BoxRoundSymbols) -> LineSymbols {
	LineSymbols {
		top_left: source.top_left.clone(),
		top_right: source.top_right.clone(),
		bottom_left: source.bottom_left.clone(),
		bottom_right: source.bottom_right.clone(),
		horizontal: source.horizontal.clone(),
		vertical: source.vertical.clone(),
	}
}

fn sharp_line_symbols(source: &BoxSharpSymbols) -> LineSymbols {
	LineSymbols {
		top_left: source.top_left.clone(),
		top_right: source.top_right.clone(),
		bottom_left: source.bottom_left.clone(),
		bottom_right: source.bottom_right.clone(),
		horizontal: source.horizontal.clone(),
		vertical: source.vertical.clone(),
	}
}

/// Symbol set for a theme: its preset's box/table glyphs, markdown rails, and
/// the activity spinner frames.
///
/// The reference's `SymbolTheme` also carries `cursor`, `inputCursor` and
/// `colorSwatch`; the crate's `SymbolTheme` has no such fields.
pub fn get_symbol_theme(theme: &Theme) -> SymbolTheme {
	let box_round = theme.box_round();
	let box_sharp = theme.box_sharp();
	let md = theme.md();
	SymbolTheme {
		box_round: line_symbols(&box_round),
		box_sharp: sharp_line_symbols(&box_sharp),
		table: TableSymbols {
			top_left: box_sharp.top_left.clone(),
			top_right: box_sharp.top_right.clone(),
			bottom_left: box_sharp.bottom_left.clone(),
			bottom_right: box_sharp.bottom_right.clone(),
			horizontal: box_sharp.horizontal.clone(),
			vertical: box_sharp.vertical.clone(),
			tee_down: box_sharp.tee_down.clone(),
			tee_up: box_sharp.tee_up.clone(),
			tee_left: box_sharp.tee_left.clone(),
			tee_right: box_sharp.tee_right.clone(),
			cross: box_sharp.cross.clone(),
		},
		quote_border: md.quote_border.clone(),
		hr_char: md.hr_char.chars().next().unwrap_or('-'),
		spinner_frames: theme.get_spinner_frames(SpinnerType::Activity).to_vec(),
	}
}

/// Markdown theme for a theme.
///
/// `colorful` mirrors the reference's `markdownColorful`: headings, inline code,
/// emphasis and table headers take palette hues instead of the single
/// `mdHeading` color. The reference picks the heading hue per depth (H1
/// magenta, H2 yellow, deeper blue); the crate's `heading` is one `Style`, so
/// the H1 hue is used.
pub fn get_markdown_theme(theme: &Theme, colorful: bool) -> MarkdownTheme {
	let heading = if colorful {
		palette_style(theme, AnsiColor::Magenta)
	} else {
		fg_style(theme, "mdHeading")
	};
	let code = if colorful {
		palette_style(theme, AnsiColor::Green)
	} else {
		fg_style(theme, "mdCode")
	};
	let (bold, italic, table_header) = if colorful {
		(
			palette_style(theme, AnsiColor::Magenta).bold(),
			palette_style(theme, AnsiColor::Magenta).italic(),
			palette_style(theme, AnsiColor::Yellow).bold(),
		)
	} else {
		(Style::default().bold(), Style::default().italic(), Style::default().bold())
	};
	MarkdownTheme {
		heading,
		prose: None,
		text: fg_style(theme, "text"),
		link: fg_style(theme, "mdLink"),
		extension_link: fg_style(theme, "mdExtensionLink"),
		link_url: fg_style(theme, "mdLinkUrl"),
		code,
		code_block: fg_style(theme, "mdCodeBlock"),
		code_block_border: fg_style(theme, "mdCodeBlockBorder"),
		quote: fg_style(theme, "mdQuote"),
		quote_border: fg_style(theme, "mdQuoteBorder"),
		hr: fg_style(theme, "mdHr"),
		list_bullet: fg_style(theme, "mdListBullet"),
		bold,
		italic,
		underline: Style::default().underline(),
		strikethrough: Style::default().strikethrough(),
		table_header,
		symbols: get_symbol_theme(theme),
		code_block_indent: 2,
	}
}

/// Select-list theme for a theme.
///
/// Roles: `selectedPrefix`/`selectedText`/`icon` → accent, `description`/
/// `scrollInfo`/`noMatch` → muted, `hovered` → `selectedBg`. The crate's extra
/// `border` field uses `borderMuted`.
pub fn get_select_list_theme(theme: &Theme) -> SelectListTheme {
	SelectListTheme {
		muted: fg_style(theme, "muted"),
		selected: fg_style(theme, "accent"),
		selected_row: bg_style(theme, "selectedBg"),
		border: fg_style(theme, "borderMuted"),
		select_label: fg_style(theme, "accent"),
		hint: fg_style(theme, "muted"),
		symbols: get_symbol_theme(theme),
	}
}

/// Editor theme for a theme.
///
/// The reference's `accentColor` and `surfaceColor` roles have no crate field
/// (`EditorTheme` has no accent/surface); `hintStyle` maps to `hint`.
pub fn get_editor_theme(theme: &Theme) -> EditorTheme {
	EditorTheme {
		border: fg_style(theme, "borderMuted"),
		text: fg_style(theme, "text"),
		hint: fg_style(theme, "dim"),
		placeholder: fg_style(theme, "dim"),
		cursor: Style::default().reverse(),
		symbols: get_symbol_theme(theme).box_round,
		padding_x: 1,
		padding_y: 0,
	}
}

/// Settings-list theme for a theme.
///
/// The reference's per-row `selected`/`changed` argument and its `section` role
/// have no crate field; `label`/`selectedLabel`/`value`/`selectedValue` collapse
/// to the four crate styles.
pub fn get_settings_list_theme(theme: &Theme) -> SettingsListTheme {
	SettingsListTheme {
		label: Style::default(),
		selected_label: fg_style(theme, "accent"),
		value: fg_style(theme, "muted"),
		selected_value: fg_style(theme, "accent"),
		description: fg_style(theme, "dim"),
		warning: fg_style(theme, "warning"),
		cursor: fg_style(theme, "accent"),
		hint: fg_style(theme, "dim"),
		heading: fg_style(theme, "muted").bold().underline(),
		selected_row: bg_style(theme, "selectedBg"),
		warning_mark: theme.status().warning,
		symbols: get_symbol_theme(theme),
	}
}

/// Resolve the autocomplete type-indicator glyph for a slash command.
///
/// `None` under the ascii preset (a partially lettered icon column reads as
/// noise) or for an unknown/empty icon, collapsing the column.
pub fn get_slash_command_type_icon(theme: &Theme, name: &str) -> Option<String> {
	if theme.get_symbol_preset() == SymbolPreset::Ascii {
		return None;
	}
	let icon = theme.symbol(&format!("cmd.{name}"));
	if icon.is_empty() {
		None
	} else {
		Some(icon.to_string())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::color::ColorMode;
	use crate::theme::{create_theme, CreateThemeOptions};

	fn theme(preset: SymbolPreset) -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin theme"),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				symbol_preset_override: Some(preset),
				..Default::default()
			},
		)
		.expect("theme resolves")
	}

	#[test]
	fn symbol_theme_follows_the_active_preset() {
		let unicode = get_symbol_theme(&theme(SymbolPreset::Unicode));
		assert_eq!(unicode.box_round.top_left, "╭");
		assert_eq!(unicode.hr_char, '─');
		assert!(!unicode.spinner_frames.is_empty());

		let ascii = get_symbol_theme(&theme(SymbolPreset::Ascii));
		assert_eq!(ascii.box_round.top_left, "+");
		assert_eq!(ascii.hr_char, '-');
	}

	#[test]
	fn markdown_theme_maps_tokens_and_honours_colorful() {
		let dark = theme(SymbolPreset::Unicode);
		let plain = get_markdown_theme(&dark, false);
		assert_eq!(plain.heading, fg_style(&dark, "mdHeading"));
		assert_eq!(plain.link, fg_style(&dark, "mdLink"));
		assert_eq!(plain.list_bullet, fg_style(&dark, "mdListBullet"));

		let colorful = get_markdown_theme(&dark, true);
		assert_eq!(colorful.heading, palette_style(&dark, AnsiColor::Magenta));
		assert_eq!(colorful.table_header, palette_style(&dark, AnsiColor::Yellow).bold());
		assert_ne!(plain.heading, colorful.heading);
	}

	#[test]
	fn component_themes_map_tokens() {
		let dark = theme(SymbolPreset::Unicode);
		let select = get_select_list_theme(&dark);
		assert_eq!(select.selected, fg_style(&dark, "accent"));
		assert_eq!(select.muted, fg_style(&dark, "muted"));
		assert_eq!(select.selected_row, bg_style(&dark, "selectedBg"));

		let editor = get_editor_theme(&dark);
		assert_eq!(editor.border, fg_style(&dark, "borderMuted"));
		assert_eq!(editor.hint, fg_style(&dark, "dim"));
		assert_eq!(editor.symbols.top_left, "╭");

		let settings = get_settings_list_theme(&dark);
		assert_eq!(settings.selected_label, fg_style(&dark, "accent"));
		assert_eq!(settings.warning, fg_style(&dark, "warning"));
		assert_eq!(settings.warning_mark, dark.status().warning);
	}

	#[test]
	fn slash_command_icon_needs_a_non_ascii_preset() {
		let unicode = theme(SymbolPreset::Unicode);
		assert!(get_slash_command_type_icon(&unicode, "settings").is_some());
		assert!(get_slash_command_type_icon(&unicode, "missingIcon").is_none());

		let ascii = theme(SymbolPreset::Ascii);
		assert!(get_slash_command_type_icon(&ascii, "settings").is_none());
	}

	#[test]
	fn unknown_tokens_stay_unstyled() {
		let dark = theme(SymbolPreset::Unicode);
		assert_eq!(fg_style(&dark, "notAToken"), Style::default());
		assert_eq!(bg_style(&dark, "notAToken"), Style::default());
	}
}
