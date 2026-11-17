//! Theme JSON schema: the exact color-token key sets, the serializable
//! Reference: `l/src/theme/theme-schema.ts`.
//! [`ThemeJson`] shape, and the validation guard used by custom themes.
//!
//! Mirrors `l/src/theme/schema.ts` and `l/src/theme/schema-validation.ts`.

use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

pub use crate::theme::color::ColorValue;
use crate::theme::symbols::{SpinnerFramesOverride, SymbolPreset};

/// Every `ThemeColor` token, in schema order.
pub const THEME_COLOR_KEYS: &[&str] = &[
	"accent",
	"border",
	"borderAccent",
	"borderMuted",
	"success",
	"error",
	"warning",
	"muted",
	"dim",
	"text",
	"assistantMessageText",
	"thinkingText",
	"userMessageText",
	"customMessageText",
	"customMessageLabel",
	"toolTitle",
	"toolOutput",
	"mdHeading",
	"mdLink",
	"mdExtensionLink",
	"mdLinkUrl",
	"mdCode",
	"mdCodeBlock",
	"mdCodeBlockBorder",
	"mdQuote",
	"mdQuoteBorder",
	"mdHr",
	"mdListBullet",
	"toolDiffAdded",
	"toolDiffRemoved",
	"toolDiffContext",
	"syntaxComment",
	"syntaxKeyword",
	"syntaxFunction",
	"syntaxVariable",
	"syntaxString",
	"syntaxNumber",
	"syntaxType",
	"syntaxOperator",
	"syntaxPunctuation",
	"thinkingOff",
	"thinkingMinimal",
	"thinkingLow",
	"thinkingMedium",
	"thinkingHigh",
	"thinkingXhigh",
	"thinkingMax",
	"bashMode",
	"pythonMode",
	"statusLineSep",
	"statusLineModel",
	"statusLinePath",
	"statusLineGitClean",
	"statusLineGitDirty",
	"statusLineContext",
	"statusLineSpend",
	"statusLineStaged",
	"statusLineDirty",
	"statusLineUntracked",
	"statusLineOutput",
	"statusLineCost",
	"statusLineSubagents",
];

/// Every `ThemeBg` token, in schema order.
pub const THEME_BG_KEYS: &[&str] = &[
	"selectedBg",
	"userMessageBg",
	"customMessageBg",
	"toolPendingBg",
	"toolSuccessBg",
	"toolErrorBg",
	"statusLineBg",
];

/// Tokens a theme may omit; they resolve to the terminal default foreground.
pub const OPTIONAL_THEME_COLOR_KEYS: &[&str] = &["assistantMessageText", "mdExtensionLink"];

/// Required `colors` keys in `schema-validation.ts` declaration order: every
/// `ThemeColor` except `assistantMessageText`, `mdExtensionLink` and
/// `thinkingMax`, plus all seven `ThemeBg` tokens.
pub const REQUIRED_THEME_COLOR_KEYS: &[&str] = &[
	"accent",
	"border",
	"borderAccent",
	"borderMuted",
	"success",
	"error",
	"warning",
	"muted",
	"dim",
	"text",
	"thinkingText",
	"selectedBg",
	"userMessageBg",
	"userMessageText",
	"customMessageBg",
	"customMessageText",
	"customMessageLabel",
	"toolPendingBg",
	"toolSuccessBg",
	"toolErrorBg",
	"toolTitle",
	"toolOutput",
	"mdHeading",
	"mdLink",
	"mdLinkUrl",
	"mdCode",
	"mdCodeBlock",
	"mdCodeBlockBorder",
	"mdQuote",
	"mdQuoteBorder",
	"mdHr",
	"mdListBullet",
	"toolDiffAdded",
	"toolDiffRemoved",
	"toolDiffContext",
	"syntaxComment",
	"syntaxKeyword",
	"syntaxFunction",
	"syntaxVariable",
	"syntaxString",
	"syntaxNumber",
	"syntaxType",
	"syntaxOperator",
	"syntaxPunctuation",
	"thinkingOff",
	"thinkingMinimal",
	"thinkingLow",
	"thinkingMedium",
	"thinkingHigh",
	"thinkingXhigh",
	"bashMode",
	"pythonMode",
	"statusLineBg",
	"statusLineSep",
	"statusLineModel",
	"statusLinePath",
	"statusLineGitClean",
	"statusLineGitDirty",
	"statusLineContext",
	"statusLineSpend",
	"statusLineStaged",
	"statusLineDirty",
	"statusLineUntracked",
	"statusLineOutput",
	"statusLineCost",
	"statusLineSubagents",
];

/// Typed view of the `ThemeColor` union.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeColor {
	Accent,
	Border,
	BorderAccent,
	BorderMuted,
	Success,
	Error,
	Warning,
	Muted,
	Dim,
	Text,
	AssistantMessageText,
	ThinkingText,
	UserMessageText,
	CustomMessageText,
	CustomMessageLabel,
	ToolTitle,
	ToolOutput,
	MdHeading,
	MdLink,
	MdExtensionLink,
	MdLinkUrl,
	MdCode,
	MdCodeBlock,
	MdCodeBlockBorder,
	MdQuote,
	MdQuoteBorder,
	MdHr,
	MdListBullet,
	ToolDiffAdded,
	ToolDiffRemoved,
	ToolDiffContext,
	SyntaxComment,
	SyntaxKeyword,
	SyntaxFunction,
	SyntaxVariable,
	SyntaxString,
	SyntaxNumber,
	SyntaxType,
	SyntaxOperator,
	SyntaxPunctuation,
	ThinkingOff,
	ThinkingMinimal,
	ThinkingLow,
	ThinkingMedium,
	ThinkingHigh,
	ThinkingXhigh,
	ThinkingMax,
	BashMode,
	PythonMode,
	StatusLineSep,
	StatusLineModel,
	StatusLinePath,
	StatusLineGitClean,
	StatusLineGitDirty,
	StatusLineContext,
	StatusLineSpend,
	StatusLineStaged,
	StatusLineDirty,
	StatusLineUntracked,
	StatusLineOutput,
	StatusLineCost,
	StatusLineSubagents,
}

impl ThemeColor {
	/// Every variant, in schema order.
	pub const ALL: &[ThemeColor] = &[
		ThemeColor::Accent,
		ThemeColor::Border,
		ThemeColor::BorderAccent,
		ThemeColor::BorderMuted,
		ThemeColor::Success,
		ThemeColor::Error,
		ThemeColor::Warning,
		ThemeColor::Muted,
		ThemeColor::Dim,
		ThemeColor::Text,
		ThemeColor::AssistantMessageText,
		ThemeColor::ThinkingText,
		ThemeColor::UserMessageText,
		ThemeColor::CustomMessageText,
		ThemeColor::CustomMessageLabel,
		ThemeColor::ToolTitle,
		ThemeColor::ToolOutput,
		ThemeColor::MdHeading,
		ThemeColor::MdLink,
		ThemeColor::MdExtensionLink,
		ThemeColor::MdLinkUrl,
		ThemeColor::MdCode,
		ThemeColor::MdCodeBlock,
		ThemeColor::MdCodeBlockBorder,
		ThemeColor::MdQuote,
		ThemeColor::MdQuoteBorder,
		ThemeColor::MdHr,
		ThemeColor::MdListBullet,
		ThemeColor::ToolDiffAdded,
		ThemeColor::ToolDiffRemoved,
		ThemeColor::ToolDiffContext,
		ThemeColor::SyntaxComment,
		ThemeColor::SyntaxKeyword,
		ThemeColor::SyntaxFunction,
		ThemeColor::SyntaxVariable,
		ThemeColor::SyntaxString,
		ThemeColor::SyntaxNumber,
		ThemeColor::SyntaxType,
		ThemeColor::SyntaxOperator,
		ThemeColor::SyntaxPunctuation,
		ThemeColor::ThinkingOff,
		ThemeColor::ThinkingMinimal,
		ThemeColor::ThinkingLow,
		ThemeColor::ThinkingMedium,
		ThemeColor::ThinkingHigh,
		ThemeColor::ThinkingXhigh,
		ThemeColor::ThinkingMax,
		ThemeColor::BashMode,
		ThemeColor::PythonMode,
		ThemeColor::StatusLineSep,
		ThemeColor::StatusLineModel,
		ThemeColor::StatusLinePath,
		ThemeColor::StatusLineGitClean,
		ThemeColor::StatusLineGitDirty,
		ThemeColor::StatusLineContext,
		ThemeColor::StatusLineSpend,
		ThemeColor::StatusLineStaged,
		ThemeColor::StatusLineDirty,
		ThemeColor::StatusLineUntracked,
		ThemeColor::StatusLineOutput,
		ThemeColor::StatusLineCost,
		ThemeColor::StatusLineSubagents,
	];

	/// The theme JSON token for this color.
	pub fn as_str(self) -> &'static str {
		match self {
			ThemeColor::Accent => "accent",
			ThemeColor::Border => "border",
			ThemeColor::BorderAccent => "borderAccent",
			ThemeColor::BorderMuted => "borderMuted",
			ThemeColor::Success => "success",
			ThemeColor::Error => "error",
			ThemeColor::Warning => "warning",
			ThemeColor::Muted => "muted",
			ThemeColor::Dim => "dim",
			ThemeColor::Text => "text",
			ThemeColor::AssistantMessageText => "assistantMessageText",
			ThemeColor::ThinkingText => "thinkingText",
			ThemeColor::UserMessageText => "userMessageText",
			ThemeColor::CustomMessageText => "customMessageText",
			ThemeColor::CustomMessageLabel => "customMessageLabel",
			ThemeColor::ToolTitle => "toolTitle",
			ThemeColor::ToolOutput => "toolOutput",
			ThemeColor::MdHeading => "mdHeading",
			ThemeColor::MdLink => "mdLink",
			ThemeColor::MdExtensionLink => "mdExtensionLink",
			ThemeColor::MdLinkUrl => "mdLinkUrl",
			ThemeColor::MdCode => "mdCode",
			ThemeColor::MdCodeBlock => "mdCodeBlock",
			ThemeColor::MdCodeBlockBorder => "mdCodeBlockBorder",
			ThemeColor::MdQuote => "mdQuote",
			ThemeColor::MdQuoteBorder => "mdQuoteBorder",
			ThemeColor::MdHr => "mdHr",
			ThemeColor::MdListBullet => "mdListBullet",
			ThemeColor::ToolDiffAdded => "toolDiffAdded",
			ThemeColor::ToolDiffRemoved => "toolDiffRemoved",
			ThemeColor::ToolDiffContext => "toolDiffContext",
			ThemeColor::SyntaxComment => "syntaxComment",
			ThemeColor::SyntaxKeyword => "syntaxKeyword",
			ThemeColor::SyntaxFunction => "syntaxFunction",
			ThemeColor::SyntaxVariable => "syntaxVariable",
			ThemeColor::SyntaxString => "syntaxString",
			ThemeColor::SyntaxNumber => "syntaxNumber",
			ThemeColor::SyntaxType => "syntaxType",
			ThemeColor::SyntaxOperator => "syntaxOperator",
			ThemeColor::SyntaxPunctuation => "syntaxPunctuation",
			ThemeColor::ThinkingOff => "thinkingOff",
			ThemeColor::ThinkingMinimal => "thinkingMinimal",
			ThemeColor::ThinkingLow => "thinkingLow",
			ThemeColor::ThinkingMedium => "thinkingMedium",
			ThemeColor::ThinkingHigh => "thinkingHigh",
			ThemeColor::ThinkingXhigh => "thinkingXhigh",
			ThemeColor::ThinkingMax => "thinkingMax",
			ThemeColor::BashMode => "bashMode",
			ThemeColor::PythonMode => "pythonMode",
			ThemeColor::StatusLineSep => "statusLineSep",
			ThemeColor::StatusLineModel => "statusLineModel",
			ThemeColor::StatusLinePath => "statusLinePath",
			ThemeColor::StatusLineGitClean => "statusLineGitClean",
			ThemeColor::StatusLineGitDirty => "statusLineGitDirty",
			ThemeColor::StatusLineContext => "statusLineContext",
			ThemeColor::StatusLineSpend => "statusLineSpend",
			ThemeColor::StatusLineStaged => "statusLineStaged",
			ThemeColor::StatusLineDirty => "statusLineDirty",
			ThemeColor::StatusLineUntracked => "statusLineUntracked",
			ThemeColor::StatusLineOutput => "statusLineOutput",
			ThemeColor::StatusLineCost => "statusLineCost",
			ThemeColor::StatusLineSubagents => "statusLineSubagents",
		}
	}

	/// Look up a variant by its JSON token.
	pub fn from_str(token: &str) -> Option<Self> {
		Self::ALL.iter().copied().find(|v| v.as_str() == token)
	}
}

impl AsRef<str> for ThemeColor {
	fn as_ref(&self) -> &str {
		self.as_str()
	}
}

/// Typed view of the `ThemeBg` union.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeBg {
	SelectedBg,
	UserMessageBg,
	CustomMessageBg,
	ToolPendingBg,
	ToolSuccessBg,
	ToolErrorBg,
	StatusLineBg,
}

impl ThemeBg {
	/// Every variant, in schema order.
	pub const ALL: &[ThemeBg] = &[
		ThemeBg::SelectedBg,
		ThemeBg::UserMessageBg,
		ThemeBg::CustomMessageBg,
		ThemeBg::ToolPendingBg,
		ThemeBg::ToolSuccessBg,
		ThemeBg::ToolErrorBg,
		ThemeBg::StatusLineBg,
	];

	/// The theme JSON token for this background color.
	pub fn as_str(self) -> &'static str {
		match self {
			ThemeBg::SelectedBg => "selectedBg",
			ThemeBg::UserMessageBg => "userMessageBg",
			ThemeBg::CustomMessageBg => "customMessageBg",
			ThemeBg::ToolPendingBg => "toolPendingBg",
			ThemeBg::ToolSuccessBg => "toolSuccessBg",
			ThemeBg::ToolErrorBg => "toolErrorBg",
			ThemeBg::StatusLineBg => "statusLineBg",
		}
	}

	/// Look up a variant by its JSON token.
	pub fn from_str(token: &str) -> Option<Self> {
		Self::ALL.iter().copied().find(|v| v.as_str() == token)
	}
}

impl AsRef<str> for ThemeBg {
	fn as_ref(&self) -> &str {
		self.as_str()
	}
}

/// True when `key` is a valid `ThemeColor` token.
pub fn is_valid_theme_color(key: &str) -> bool {
	THEME_COLOR_KEYS.contains(&key)
}

/// True when `key` is a valid `ThemeBg` token.
pub fn is_valid_theme_bg(key: &str) -> bool {
	THEME_BG_KEYS.contains(&key)
}

/// The 16 `terminal.ansi` slots (`Theme.ansiFg` paints by slot name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnsiColor {
	Black = 0,
	Red = 1,
	Green = 2,
	Yellow = 3,
	Blue = 4,
	Magenta = 5,
	Cyan = 6,
	White = 7,
	BrightBlack = 8,
	BrightRed = 9,
	BrightGreen = 10,
	BrightYellow = 11,
	BrightBlue = 12,
	BrightMagenta = 13,
	BrightCyan = 14,
	BrightWhite = 15,
}

impl AnsiColor {
	/// Slot name → color, matching `ANSI_COLOR_INDEX` in `schema.ts`.
	pub const INDEX: &[(&str, AnsiColor)] = &[
		("black", AnsiColor::Black),
		("red", AnsiColor::Red),
		("green", AnsiColor::Green),
		("yellow", AnsiColor::Yellow),
		("blue", AnsiColor::Blue),
		("magenta", AnsiColor::Magenta),
		("cyan", AnsiColor::Cyan),
		("white", AnsiColor::White),
		("brightBlack", AnsiColor::BrightBlack),
		("brightRed", AnsiColor::BrightRed),
		("brightGreen", AnsiColor::BrightGreen),
		("brightYellow", AnsiColor::BrightYellow),
		("brightBlue", AnsiColor::BrightBlue),
		("brightMagenta", AnsiColor::BrightMagenta),
		("brightCyan", AnsiColor::BrightCyan),
		("brightWhite", AnsiColor::BrightWhite),
	];

	/// The slot index (0–15).
	pub fn index(self) -> usize {
		self as usize
	}

	/// The slot name.
	pub fn name(self) -> &'static str {
		match self {
			AnsiColor::Black => "black",
			AnsiColor::Red => "red",
			AnsiColor::Green => "green",
			AnsiColor::Yellow => "yellow",
			AnsiColor::Blue => "blue",
			AnsiColor::Magenta => "magenta",
			AnsiColor::Cyan => "cyan",
			AnsiColor::White => "white",
			AnsiColor::BrightBlack => "brightBlack",
			AnsiColor::BrightRed => "brightRed",
			AnsiColor::BrightGreen => "brightGreen",
			AnsiColor::BrightYellow => "brightYellow",
			AnsiColor::BrightBlue => "brightBlue",
			AnsiColor::BrightMagenta => "brightMagenta",
			AnsiColor::BrightCyan => "brightCyan",
			AnsiColor::BrightWhite => "brightWhite",
		}
	}

	/// Look up a slot by name.
	pub fn from_name(name: &str) -> Option<Self> {
		Self::INDEX.iter().copied().find(|(n, _)| *n == name).map(|(_, c)| c)
	}
}

/// Alias of [`AnsiColor::INDEX`] under the name the TypeScript schema uses.
pub const ANSI_COLOR_INDEX: &[(&str, AnsiColor)] = AnsiColor::INDEX;

/// Insertion-ordered string→[`ColorValue`] map, used for `vars` and `colors`.
///
/// A plain `HashMap` would lose the declaration order that
/// `Theme::get_all_theme_color_hexes` reports, so entries stay in a `Vec`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorMap {
	entries: Vec<(String, ColorValue)>,
}

impl ColorMap {
	/// An empty map.
	pub fn new() -> Self {
		Self { entries: Vec::new() }
	}

	/// Look up a value by key; the last declaration wins.
	pub fn get(&self, key: &str) -> Option<&ColorValue> {
		self.entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
	}

	/// Iterate entries in declaration order.
	pub fn iter(&self) -> impl Iterator<Item = (&str, &ColorValue)> {
		self.entries.iter().map(|(k, v)| (k.as_str(), v))
	}

	/// Number of entries, including duplicate keys.
	pub fn len(&self) -> usize {
		self.entries.len()
	}

	/// True when there are no entries.
	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	/// True when `key` is present.
	pub fn contains_key(&self, key: &str) -> bool {
		self.entries.iter().any(|(k, _)| k == key)
	}

	/// Append an entry (used when building maps programmatically).
	pub fn insert(&mut self, key: String, value: ColorValue) {
		self.entries.push((key, value));
	}

	/// Replace the last declaration of `key` in place, preserving order.
	///
	/// Appends when `key` is absent.
	pub fn set(&mut self, key: &str, value: ColorValue) {
		if let Some(entry) = self.entries.iter_mut().rev().find(|(k, _)| k.as_str() == key) {
			entry.1 = value;
		} else {
			self.entries.push((key.to_string(), value));
		}
	}

	/// Consume the map into its entries.
	pub fn into_entries(self) -> Vec<(String, ColorValue)> {
		self.entries
	}
}

impl<'de> Deserialize<'de> for ColorMap {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct ColorMapVisitor;

		impl<'de> Visitor<'de> for ColorMapVisitor {
			type Value = ColorMap;

			fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				f.write_str("a color map object")
			}

			fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ColorMap, A::Error> {
				let mut entries = Vec::new();
				while let Some((key, value)) = map.next_entry::<String, ColorValue>()? {
					entries.push((key, value));
				}
				Ok(ColorMap { entries })
			}
		}

		deserializer.deserialize_map(ColorMapVisitor)
	}
}

/// `export` block: colors a host derives its chrome from.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ThemeExport {
	/// Page background.
	#[serde(default, rename = "pageBg")]
	pub page_bg: Option<ColorValue>,
	/// Card background.
	#[serde(default, rename = "cardBg")]
	pub card_bg: Option<ColorValue>,
	/// Info-callout background.
	#[serde(default, rename = "infoBg")]
	pub info_bg: Option<ColorValue>,
}

/// `terminal` block: the terminal a theme was made for.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TerminalJson {
	/// Terminal background.
	#[serde(default)]
	pub background: Option<ColorValue>,
	/// Default text color.
	#[serde(default)]
	pub foreground: Option<ColorValue>,
	/// Window chrome (sidebar, tab bar, title bar).
	#[serde(default)]
	pub chrome: Option<ColorValue>,
	/// Lifted surfaces (popovers, menus, buttons).
	#[serde(default)]
	pub widget: Option<ColorValue>,
	/// ANSI colors 0–15; exactly 16 entries.
	#[serde(default)]
	pub ansi: Option<Vec<ColorValue>>,
}

/// `symbols` block: preset selection and per-key overrides.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SymbolsJson {
	/// Symbol preset name.
	#[serde(default)]
	pub preset: Option<SymbolPreset>,
	/// Per-key symbol overrides.
	#[serde(default)]
	pub overrides: Option<std::collections::HashMap<String, String>>,
	/// Spinner frame overrides.
	#[serde(default, rename = "spinnerFrames")]
	pub spinner_frames: Option<SpinnerFramesOverride>,
}

/// A theme definition as stored in `dark.json`/`light.json`/custom theme files.
#[derive(Clone, Debug, Deserialize)]
pub struct ThemeJson {
	/// JSON-schema URL (informational).
	#[serde(rename = "$schema", default)]
	pub schema: Option<String>,
	/// Theme name.
	pub name: String,
	/// Named color references, resolved recursively.
	#[serde(default)]
	pub vars: ColorMap,
	/// Color tokens (plus any extra keys, which `Theme` ignores).
	pub colors: ColorMap,
	/// Host-facing export colors.
	#[serde(default)]
	pub export: Option<ThemeExport>,
	/// Terminal colors the theme was designed for.
	#[serde(default)]
	pub terminal: Option<TerminalJson>,
	/// Symbol preset and overrides.
	#[serde(default)]
	pub symbols: Option<SymbolsJson>,
}

/// Validate a parsed theme against the schema contract.
///
/// Returns `Err(detail)` where `detail` is `missing keys: a, b` when required
/// color tokens are absent, or a descriptive message otherwise. The caller
/// ([`crate::theme::loader::parse_theme_json`]) wraps it in the user-facing
/// `Invalid theme "<name>":` envelope.
pub fn validate_theme_json(value: &serde_json::Value) -> Result<(), String> {
	let obj = value
		.as_object()
		.ok_or_else(|| "expected a theme object at the top level".to_string())?;

	if let Some(schema) = obj.get("$schema") {
		if !schema.is_string() {
			return Err("invalid type for \"$schema\": expected a string".into());
		}
	}
	if !obj.get("name").map(serde_json::Value::is_string).unwrap_or(false) {
		return Err("missing key: name".into());
	}
	if let Some(vars) = obj.get("vars") {
		let vars = vars
			.as_object()
		.ok_or_else(|| "invalid type for \"vars\": expected an object".to_string())?;
		for (key, v) in vars {
			check_color_value(v).map_err(|e| format!("invalid type for \"vars.{}\": {}", key, e))?;
		}
	}

	let colors = obj
		.get("colors")
		.and_then(serde_json::Value::as_object)
		.ok_or_else(|| "missing key: colors".to_string())?;
	let mut missing = Vec::new();
	for key in REQUIRED_THEME_COLOR_KEYS {
		if !colors.contains_key(*key) {
			missing.push((*key).to_string());
		}
	}
	if !missing.is_empty() {
		return Err(format!("missing keys: {}", missing.join(", ")));
	}
	for (key, v) in colors {
		check_color_value(v).map_err(|e| format!("invalid type for \"colors.{}\": {}", key, e))?;
	}

	if let Some(export) = obj.get("export") {
		let export = export
			.as_object()
			.ok_or_else(|| "invalid type for \"export\": expected an object".to_string())?;
		for key in ["pageBg", "cardBg", "infoBg"] {
			if let Some(v) = export.get(key) {
				check_color_value(v).map_err(|e| format!("invalid type for \"export.{}\": {}", key, e))?;
			}
		}
	}

	if let Some(terminal) = obj.get("terminal") {
		let terminal = terminal
			.as_object()
			.ok_or_else(|| "invalid type for \"terminal\": expected an object".to_string())?;
		for key in ["background", "foreground", "chrome", "widget"] {
			if let Some(v) = terminal.get(key) {
				check_color_value(v)
					.map_err(|e| format!("invalid type for \"terminal.{}\": {}", key, e))?;
			}
		}
		if let Some(ansi) = terminal.get("ansi") {
			let ansi = ansi
				.as_array()
				.ok_or_else(|| "invalid type for \"terminal.ansi\": expected an array".to_string())?;
			if ansi.len() != 16 {
				return Err(format!(
					"invalid length for \"terminal.ansi\": expected 16 entries, found {}",
					ansi.len()
				));
			}
			for (i, v) in ansi.iter().enumerate() {
				check_color_value(v)
					.map_err(|e| format!("invalid type for \"terminal.ansi[{}]\": {}", i, e))?;
			}
		}
	}

	if let Some(symbols) = obj.get("symbols") {
		let symbols = symbols
			.as_object()
			.ok_or_else(|| "invalid type for \"symbols\": expected an object".to_string())?;
		if let Some(preset) = symbols.get("preset") {
			let name = preset
				.as_str()
				.ok_or_else(|| "invalid type for \"symbols.preset\": expected a string".to_string())?;
			if crate::theme::symbols::is_valid_symbol_preset(name) == false {
				return Err(format!(
					"invalid value for \"symbols.preset\": expected \"unicode\", \"nerd\" or \"ascii\", found \"{}\"",
					name
				));
			}
		}
		if let Some(overrides) = symbols.get("overrides") {
			let overrides = overrides
				.as_object()
				.ok_or_else(|| "invalid type for \"symbols.overrides\": expected an object".to_string())?;
			for (key, v) in overrides {
				if !v.is_string() {
					return Err(format!(
						"invalid type for \"symbols.overrides.{}\": expected a string",
						key
					));
				}
			}
		}
		if let Some(frames) = symbols.get("spinnerFrames") {
			validate_spinner_frames(frames)?;
		}
	}

	Ok(())
}

fn check_color_value(value: &serde_json::Value) -> Result<(), &'static str> {
	if value.is_string() || value.is_number() {
		Ok(())
	} else {
		Err("expected a string or number")
	}
}

/// Validate `symbols.spinnerFrames`: a non-empty string array, or an object
/// with at least one non-empty `status`/`activity` string array.
pub fn validate_spinner_frames(value: &serde_json::Value) -> Result<(), String> {
	if let Some(items) = value.as_array() {
		if !items.is_empty() && items.iter().all(serde_json::Value::is_string) {
			return Ok(());
		}
		return Err("invalid type for \"symbols.spinnerFrames\": expected a non-empty string array".into());
	}
	let obj = value.as_object().ok_or_else(|| {
		"invalid type for \"symbols.spinnerFrames\": expected an array or object".to_string()
	})?;
	let mut any = false;
	for key in ["status", "activity"] {
		if let Some(items) = obj.get(key) {
			let items = items.as_array().ok_or_else(|| {
				format!("invalid type for \"symbols.spinnerFrames.{}\": expected an array", key)
			})?;
			if items.is_empty() || !items.iter().all(serde_json::Value::is_string) {
				return Err(format!(
					"invalid type for \"symbols.spinnerFrames.{}\": expected a non-empty string array",
					key
				));
			}
			any = true;
		}
	}
	if !any {
		return Err(
			"invalid type for \"symbols.spinnerFrames\": expected at least one of \"status\" or \"activity\""
				.into(),
		);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn theme_color_key_set_is_exact() {
		assert_eq!(THEME_COLOR_KEYS.len(), 62);
		assert!(is_valid_theme_color("mdHeading"));
		assert!(is_valid_theme_color("thinkingMax"));
		assert!(!is_valid_theme_color("link"));
		assert_eq!(THEME_BG_KEYS.len(), 7);
		assert!(is_valid_theme_bg("statusLineBg"));
		assert!(!is_valid_theme_bg("accent"));
		assert_eq!(REQUIRED_THEME_COLOR_KEYS.len(), 66);
		for key in THEME_BG_KEYS {
			assert!(REQUIRED_THEME_COLOR_KEYS.contains(key), "{key} must be required");
		}
		assert_eq!(ThemeColor::MdHeading.as_str(), "mdHeading");
		assert_eq!(ThemeColor::from_str("statusLineSubagents"), Some(ThemeColor::StatusLineSubagents));
		assert_eq!(ThemeBg::ToolPendingBg.as_str(), "toolPendingBg");
	}

	#[test]
	fn ansi_index_covers_sixteen_slots() {
		assert_eq!(ANSI_COLOR_INDEX.len(), 16);
		assert_eq!(AnsiColor::from_name("brightWhite"), Some(AnsiColor::BrightWhite));
		assert_eq!(AnsiColor::BrightWhite.index(), 15);
		assert_eq!(AnsiColor::Red.name(), "red");
	}

	#[test]
	fn missing_required_tokens_are_reported_in_order() {
		let value: serde_json::Value = serde_json::from_str(
			r##"{"name":"x","colors":{"accent":"#fff"}}"##,
		)
		.unwrap();
		let err = validate_theme_json(&value).unwrap_err();
		assert!(err.starts_with("missing keys: "), "{err}");
		assert!(err.contains("border"));
		assert!(err.contains("statusLineSubagents"));
	}

	#[test]
	fn valid_minimal_theme_passes() {
		let mut colors = serde_json::Map::new();
		for key in REQUIRED_THEME_COLOR_KEYS {
			colors.insert((*key).to_string(), serde_json::Value::String("#ffffff".into()));
		}
		let value = serde_json::json!({"name": "minimal", "colors": colors});
		assert!(validate_theme_json(&value).is_ok());
	}

	#[test]
	fn ansi_palette_must_have_sixteen_entries() {
		let mut colors = serde_json::Map::new();
		for key in REQUIRED_THEME_COLOR_KEYS {
			colors.insert((*key).to_string(), serde_json::Value::String("#ffffff".into()));
		}
		let value = serde_json::json!({
			"name": "bad",
			"colors": colors,
			"terminal": {"ansi": ["#000000", "#ffffff"]}
		});
		let err = validate_theme_json(&value).unwrap_err();
		assert!(err.contains("expected 16 entries"), "{err}");
	}

	#[test]
	fn color_map_preserves_declaration_order() {
		let mut map = ColorMap::new();
		map.insert("b".into(), ColorValue::Hex("#000001".into()));
		map.insert("a".into(), ColorValue::Hex("#000002".into()));
		let keys: Vec<&str> = map.iter().map(|(k, _)| k).collect();
		assert_eq!(keys, vec!["b", "a"]);
		assert_eq!(map.len(), 2);
		assert!(map.contains_key("a"));
	}

	#[test]
	fn theme_json_parses_dark_shape() {
		let json = r##"{
			"$schema": "https://example/schema",
			"name": "t",
			"vars": {"accent": "#fff"},
			"colors": {"accent": "accent"},
			"export": {"pageBg": "#000000", "cardBg": "#111111", "infoBg": "#222222"},
			"terminal": {"ansi": [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]},
			"symbols": {"preset": "nerd", "overrides": {"sep.dot": "."}, "spinnerFrames": {"status": ["a"]}}
		}"##;
		let parsed: ThemeJson = serde_json::from_str(json).unwrap();
		assert_eq!(parsed.name, "t");
		assert_eq!(parsed.vars.get("accent"), Some(&ColorValue::Hex("#fff".into())));
		let export = parsed.export.unwrap();
		assert_eq!(export.page_bg, Some(ColorValue::Hex("#000000".into())));
		assert_eq!(export.card_bg, Some(ColorValue::Hex("#111111".into())));
		assert_eq!(export.info_bg, Some(ColorValue::Hex("#222222".into())));
		assert_eq!(parsed.terminal.unwrap().ansi.unwrap().len(), 16);
		let symbols = parsed.symbols.unwrap();
		assert_eq!(symbols.preset, Some(crate::theme::symbols::SymbolPreset::Nerd));
		assert_eq!(symbols.overrides.unwrap().get("sep.dot"), Some(&".".to_string()));
		assert_eq!(
			symbols.spinner_frames,
			Some(crate::theme::symbols::SpinnerFramesOverride::ByType {
				status: Some(vec!["a".into()]),
				activity: None
			})
		);
}
}
