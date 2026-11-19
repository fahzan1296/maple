//! Composer chrome contract.
//!
//! Reference: `l/src/components/composer/types.ts`.
//!
//! A [`ComposerStyle`] owns everything about how the input editor's frame
//! looks — top/bottom chrome, per-row side chrome, default padding and prompt
//! gutter — plus metadata telling the host where the status bar attaches.
//! The editor, the settings preview, and the setup-wizard preview all render
//! through the same style object, so the three surfaces cannot drift apart.
//!
//! Deviations forced by Rust:
//! - The reference's `ComposerStyle` members are TypeScript closures. Rust has
//!   no inline object-literal closures, so the function members are
//!   `Arc<dyn Fn>` trait objects; `ComposerChromeContext`/`ComposerRowContext`
//!   paints are `Arc<dyn Fn>` as well so a row context can copy its chrome.
//! - The reference's `box` field keeps its exact name as the raw identifier
//!   `r#box` (`box` is a Rust keyword).

use std::sync::Arc;

use crate::symbols::LineSymbols;

/// Box-drawing glyph set used for composer chrome (the theme's `boxRound`).
pub type ComposerBox = LineSymbols;

/// Paint one string; the reference's `(str: string) => string`.
pub type ComposerPaint = Arc<dyn Fn(&str) -> String>;

/// `defaultPaddingX(themePaddingX)`.
pub type DefaultPaddingXFn = Arc<dyn Fn(Option<usize>) -> usize + Send + Sync>;
/// `sideChromeWidth(paddingX)`.
pub type SideChromeWidthFn = Arc<dyn Fn(usize) -> usize + Send + Sync>;
/// `renderTop(ctx)`.
pub type RenderTopFn = Arc<dyn Fn(&ComposerChromeContext) -> Option<String> + Send + Sync>;
/// `renderRow(ctx)`.
pub type RenderRowFn = Arc<dyn Fn(&ComposerRowContext) -> Vec<String> + Send + Sync>;
/// `renderBottom(ctx)`.
pub type RenderBottomFn = Arc<dyn Fn(&ComposerChromeContext) -> Option<String> + Send + Sync>;

/// Built-in composer shape identifiers shipped by maple-tui.
pub const BUILTIN_EDITOR_BORDER_STYLES: [&str; 8] = [
	"box",
	"band",
	"claude",
	"pi",
	"borderless",
	"rule",
	"field",
	"rail",
];

/// Identifier for a built-in composer shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinEditorBorderStyle {
	Box,
	Band,
	Claude,
	Pi,
	Borderless,
	Rule,
	Field,
	Rail,
}

impl BuiltinEditorBorderStyle {
	/// The identifier string used by settings and the registry.
	pub fn as_str(self) -> &'static str {
		match self {
			BuiltinEditorBorderStyle::Box => "box",
			BuiltinEditorBorderStyle::Band => "band",
			BuiltinEditorBorderStyle::Claude => "claude",
			BuiltinEditorBorderStyle::Pi => "pi",
			BuiltinEditorBorderStyle::Borderless => "borderless",
			BuiltinEditorBorderStyle::Rule => "rule",
			BuiltinEditorBorderStyle::Field => "field",
			BuiltinEditorBorderStyle::Rail => "rail",
		}
	}

	/// The built-in variant named by `id`, if any.
	pub fn from_id(id: &str) -> Option<Self> {
		Some(match id {
			"box" => BuiltinEditorBorderStyle::Box,
			"band" => BuiltinEditorBorderStyle::Band,
			"claude" => BuiltinEditorBorderStyle::Claude,
			"pi" => BuiltinEditorBorderStyle::Pi,
			"borderless" => BuiltinEditorBorderStyle::Borderless,
			"rule" => BuiltinEditorBorderStyle::Rule,
			"field" => BuiltinEditorBorderStyle::Field,
			"rail" => BuiltinEditorBorderStyle::Rail,
			_ => return None,
		})
	}
}

impl std::fmt::Display for BuiltinEditorBorderStyle {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.as_str())
	}
}

/// Composer shape identifier; extensions may register additional strings.
pub type EditorBorderStyle = String;

/// Pre-rendered status content injected into the top chrome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorTopBorder {
	/// The status content (already styled).
	pub content: String,
	/// Visible width of the content.
	pub width: usize,
	/// Optional logical revision that changes independently of available width.
	pub revision: Option<usize>,
}

/// Rows consumed by top and bottom chrome: `0`, `1`, or `2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VerticalChrome {
	Zero,
	One,
	Two,
}

impl VerticalChrome {
	/// The numeric budget value used by the host.
	pub fn as_usize(self) -> usize {
		match self {
			VerticalChrome::Zero => 0,
			VerticalChrome::One => 1,
			VerticalChrome::Two => 2,
		}
	}
}

impl Default for VerticalChrome {
	fn default() -> Self {
		VerticalChrome::Zero
	}
}

/// Where the host should attach the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusAttachment {
	/// Embedded in the top border.
	TopBorder,
	/// Rendered as a flush soft-capped band above the input.
	TopBand,
	/// Docked onto a top rule.
	TopRuleChip,
	/// Detached into a standalone bottom bar.
	None,
}

impl StatusAttachment {
	/// The identifier string used by settings.
	pub fn as_str(self) -> &'static str {
		match self {
			StatusAttachment::TopBorder => "top-border",
			StatusAttachment::TopBand => "top-band",
			StatusAttachment::TopRuleChip => "top-rule-chip",
			StatusAttachment::None => "none",
		}
	}
}

/// Which segment groups the standalone bottom status bar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BottomBar {
	None,
	Left,
	Full,
}

impl BottomBar {
	/// The identifier string used by settings.
	pub fn as_str(self) -> &'static str {
		match self {
			BottomBar::None => "none",
			BottomBar::Left => "left",
			BottomBar::Full => "full",
		}
	}
}

/// Inputs shared by every chrome row.
#[derive(Clone)]
pub struct ComposerChromeContext {
	/// Full terminal width available to the composer.
	pub width: usize,
	/// Horizontal padding inside the side chrome.
	pub padding_x: usize,
	/// Cells consumed per side on content rows.
	pub side_chrome_width: SideChromeWidthFn,
	/// Border paint.
	pub border_color: ComposerPaint,
	/// Stable accent used by shape-defining chrome such as field caps and rails.
	pub accent_color: ComposerPaint,
	/// Background fill for composer surfaces; preserves the fill across nested SGR resets.
	pub surface_color: ComposerPaint,
	/// Box-drawing glyph set (theme's `boxRound`).
	pub r#box: ComposerBox,
	/// Status content for the top chrome; box embeds it after the corner while
	/// rule-based styles dock it against the right edge.
	pub top_border: Option<EditorTopBorder>,
}

/// Inputs for one content row.
#[derive(Clone)]
pub struct ComposerRowContext {
	/// Full terminal width available to the composer.
	pub width: usize,
	/// Horizontal padding inside the side chrome.
	pub padding_x: usize,
	/// Cells consumed per side on content rows.
	pub side_chrome_width: SideChromeWidthFn,
	/// Border paint.
	pub border_color: ComposerPaint,
	/// Stable accent used by shape-defining chrome such as field caps and rails.
	pub accent_color: ComposerPaint,
	/// Background fill for composer surfaces; preserves the fill across nested SGR resets.
	pub surface_color: ComposerPaint,
	/// Box-drawing glyph set (theme's `boxRound`).
	pub r#box: ComposerBox,
	/// Status content for the top chrome.
	pub top_border: Option<EditorTopBorder>,
	/// Fully decorated row text (cursor glyph / IME marker included).
	pub text: String,
	/// Spaces padding the text out to the content width.
	pub pad: String,
	/// Prompt gutter cells for this row (`""` when the style has none).
	pub gutter: String,
	/// Whether this is the final content row.
	pub is_last_row: bool,
	/// Cells the end-of-line cursor overflowed into the right chrome (box).
	pub cursor_overflow: usize,
	/// Emit an empty right chrome after the cursor so terminal-local IME
	/// preedit cannot shift the frame (box last row).
	pub ime_safe_cursor_tail: bool,
	/// Row lies inside the right-border scrollbar thumb (box).
	pub scrollbar_thumb: bool,
}

/// Chrome geometry and rendering behaviour for one composer shape.
#[derive(Clone)]
pub struct ComposerStyle {
	/// Shape identifier.
	pub id: EditorBorderStyle,
	/// True when rows paint their own foreground through `surfaceColor`.
	/// Built-ins default to transparent; registered extensions that omit this
	/// field retain the pre-field behavior and receive undecorated row text.
	pub filled_surface: Option<bool>,
	/// Content rows carry left/right border glyphs; drives the cursor-reserve
	/// column, IME-safe layout, and the right-border scrollbar.
	pub side_borders: bool,
	/// Rows consumed by top+bottom chrome (drives maxHeight budgeting).
	pub vertical_chrome: VerticalChrome,
	/// Where the host should attach the status bar.
	pub status_attachment: StatusAttachment,
	/// Which segment groups the standalone bottom status bar shows.
	pub bottom_bar: BottomBar,
	/// Insert a blank spacer row between the editor and the standalone bottom
	/// bar. Styles without bottom chrome need it so the bar doesn't sit flush
	/// against the last input row.
	pub bottom_bar_gap: bool,
	/// Default prompt gutter when the host sets none.
	pub default_prompt_gutter: Option<String>,
	/// Default horizontal padding; `themePaddingX` is the theme's request.
	pub default_padding_x: DefaultPaddingXFn,
	/// Cells consumed per side on content rows (border glyph + padding).
	pub side_chrome_width: SideChromeWidthFn,
	/// Top chrome row; `None` renders none.
	pub render_top: RenderTopFn,
	/// Chrome-wrapped content row; box's IME-safe last row emits two rows.
	pub render_row: RenderRowFn,
	/// Bottom chrome row; `None` renders none (box merges the bottom border
	/// into its last content row).
	pub render_bottom: RenderBottomFn,
}

#[cfg(test)]
pub(crate) fn test_chrome(width: usize, padding_x: usize) -> ComposerChromeContext {
	ComposerChromeContext {
		width,
		padding_x,
		side_chrome_width: Arc::new(|padding_x| padding_x),
		border_color: Arc::new(|text| text.to_string()),
		accent_color: Arc::new(|text| text.to_string()),
		surface_color: Arc::new(|text| text.to_string()),
		r#box: LineSymbols {
			top_left: "╭".to_string(),
			top_right: "╮".to_string(),
			bottom_left: "╰".to_string(),
			bottom_right: "╯".to_string(),
			horizontal: "─".to_string(),
			vertical: "│".to_string(),
		},
		top_border: None,
	}
}

#[cfg(test)]
pub(crate) fn test_row(chrome: &ComposerChromeContext, text: &str) -> ComposerRowContext {
	ComposerRowContext {
		width: chrome.width,
		padding_x: chrome.padding_x,
		side_chrome_width: Arc::clone(&chrome.side_chrome_width),
		border_color: Arc::clone(&chrome.border_color),
		accent_color: Arc::clone(&chrome.accent_color),
		surface_color: Arc::clone(&chrome.surface_color),
		r#box: chrome.r#box.clone(),
		top_border: chrome.top_border.clone(),
		text: text.to_string(),
		pad: String::new(),
		gutter: String::new(),
		is_last_row: false,
		cursor_overflow: 0,
		ime_safe_cursor_tail: false,
		scrollbar_thumb: false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn lists_builtin_ids_in_reference_order() {
		assert_eq!(
			BUILTIN_EDITOR_BORDER_STYLES,
			["box", "band", "claude", "pi", "borderless", "rule", "field", "rail"]
		);
	}

	#[test]
	fn builtin_style_round_trips_every_id() {
		for id in BUILTIN_EDITOR_BORDER_STYLES {
			let style = BuiltinEditorBorderStyle::from_id(id).expect("builtin id");
			assert_eq!(style.as_str(), id);
			assert_eq!(style.to_string(), id);
		}
		assert_eq!(BuiltinEditorBorderStyle::from_id("nope"), None);
	}

	#[test]
	fn vertical_chrome_maps_to_budget_value() {
		assert_eq!(VerticalChrome::Zero.as_usize(), 0);
		assert_eq!(VerticalChrome::One.as_usize(), 1);
		assert_eq!(VerticalChrome::Two.as_usize(), 2);
		assert_eq!(VerticalChrome::default(), VerticalChrome::Zero);
	}

	#[test]
	fn enums_expose_reference_identifier_strings() {
		let attachments = [
			(StatusAttachment::TopBorder, "top-border"),
			(StatusAttachment::TopBand, "top-band"),
			(StatusAttachment::TopRuleChip, "top-rule-chip"),
			(StatusAttachment::None, "none"),
		];
		for (value, id) in attachments {
			assert_eq!(value.as_str(), id);
		}
		let bars = [
			(BottomBar::None, "none"),
			(BottomBar::Left, "left"),
			(BottomBar::Full, "full"),
		];
		for (value, id) in bars {
			assert_eq!(value.as_str(), id);
		}
	}

	#[test]
	fn row_context_copies_chrome_inputs() {
		let chrome = test_chrome(12, 2);
		let mut row = test_row(&chrome, "hi");
		row.gutter = "❯ ".to_string();
		row.pad = "  ".to_string();
		row.is_last_row = true;
		assert_eq!(row.width, 12);
		assert_eq!(row.padding_x, 2);
		assert_eq!(row.text, "hi");
		assert_eq!(row.gutter, "❯ ");
		assert!(row.is_last_row);
		assert_eq!(row.r#box.horizontal, "─");
	}
}
