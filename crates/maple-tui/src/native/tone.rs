//! Theme colours → TSP tones.
//!
//! Components that only hold ANSI stylers (`Fn(&str) -> String`) sample them
//! with a probe string to recover the theme token behind the escape, then map
//! the token to a card/text tone. [`tone_color`] is the inverse: it picks the
//! theme colour an ANSI renderer draws a tone with.
//!
//! Reference: `l/src/native/tone.ts`.
//!
//! Deviation: the reference reads the process-global theme singleton, so its
//! functions take no theme argument; maple-tui has no global theme, so every
//! sampler takes `&Theme` explicitly. Stylers are accepted as
//! `Option<&dyn Fn(&str) -> String>` so an absent styler stays absent without a
//! wrapper type.

use crate::native::node::Tone;
use crate::native::spans::{styled_background, styled_spans};
use crate::theme::Theme;
use crate::theme::schema::{is_valid_theme_color, ThemeBg, ThemeColor};

/// Card chrome derived from a background fill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackgroundChrome {
	/// Semantic tone of the card surface.
	pub tone: Option<Tone>,
	/// Accessible role the card carries.
	pub role: Option<&'static str>,
	/// Whether the surface is selected.
	pub selected: Option<bool>,
}

/// Theme background token of an ANSI background styler, sampled with a probe string.
///
/// Returns `None` when `bg_fn` is absent or its output carries no known
/// background escape.
pub fn sample_background(
	bg_fn: Option<&dyn Fn(&str) -> String>,
	theme: &Theme,
) -> Option<ThemeBg> {
	let bg_fn = bg_fn?;
	styled_background(&bg_fn("x"), theme)
}

/// Theme foreground token of an ANSI colour styler, sampled with a probe string.
///
/// Returns `None` when `color` is absent, its output has no style token, or the
/// first token is not a valid `ThemeColor`.
pub fn sample_foreground(
	color: Option<&dyn Fn(&str) -> String>,
	theme: &Theme,
) -> Option<ThemeColor> {
	let color = color?;
	let spans = styled_spans(&color("x"), theme);
	let first = spans.first()?.s.as_deref()?;
	let token = first.split(' ').next().unwrap_or("");
	if is_valid_theme_color(token) {
		ThemeColor::from_str(token)
	} else {
		None
	}
}

/// Map a omp background token to card tone/role.
///
/// An absent token yields empty chrome; callers pick their own fallback.
pub fn background_chrome(bg: Option<ThemeBg>) -> BackgroundChrome {
	match bg {
		Some(ThemeBg::UserMessageBg) => BackgroundChrome {
			tone: Some(Tone::User),
			role: Some("omp.user"),
			selected: None,
		},
		Some(ThemeBg::CustomMessageBg) => BackgroundChrome {
			tone: Some(Tone::Info),
			role: Some("omp.custom"),
			selected: None,
		},
		Some(ThemeBg::ToolPendingBg) => BackgroundChrome {
			tone: Some(Tone::Pending),
			role: Some("omp.tool"),
			selected: None,
		},
		Some(ThemeBg::ToolSuccessBg) => BackgroundChrome {
			tone: Some(Tone::Success),
			role: Some("omp.tool"),
			selected: None,
		},
		Some(ThemeBg::ToolErrorBg) => BackgroundChrome {
			tone: Some(Tone::Error),
			role: Some("omp.tool"),
			selected: None,
		},
		Some(ThemeBg::SelectedBg) => BackgroundChrome {
			tone: Some(Tone::Accent),
			role: None,
			selected: Some(true),
		},
		Some(ThemeBg::StatusLineBg) => BackgroundChrome {
			tone: Some(Tone::Neutral),
			role: Some("omp.status"),
			selected: None,
		},
		None => BackgroundChrome::default(),
	}
}

/// The omp theme colour an ANSI renderer draws a TSP tone with (the inverse of
/// [`color_tone`]).
pub fn tone_color(tone: Tone) -> ThemeColor {
	match tone {
		Tone::Neutral => ThemeColor::Text,
		Tone::Accent | Tone::Info => ThemeColor::Accent,
		Tone::Success => ThemeColor::Success,
		Tone::Warning | Tone::Pending => ThemeColor::Warning,
		Tone::Error => ThemeColor::Error,
		Tone::Muted => ThemeColor::Muted,
		Tone::User => ThemeColor::CustomMessageLabel,
	}
}

/// Tone of an omp colour used as a severity or border colour; `None` for
/// colours that carry no tone (callers pick their own fallback).
pub fn color_tone(color: Option<ThemeColor>) -> Option<Tone> {
	color.and_then(|color| color_tone_token(color.as_str()))
}

/// Tone of an omp colour token, accepting raw strings.
///
/// Unknown tokens return `None`; thinking-level tokens all map to
/// [`Tone::Accent`] exactly as the reference's prefix rule does.
pub fn color_tone_token(token: &str) -> Option<Tone> {
	match token {
		"border" => Some(Tone::Neutral),
		"borderAccent" | "accent" => Some(Tone::Accent),
		"borderMuted" | "muted" | "dim" => Some(Tone::Muted),
		"success" => Some(Tone::Success),
		"error" => Some(Tone::Error),
		"warning" => Some(Tone::Warning),
		_ if token.starts_with("thinking") => Some(Tone::Accent),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::color::{ColorMode, ColorValue};
	use crate::theme::schema::ColorMap;
	use crate::theme::symbols::{NormalizedSpinnerFrames, SymbolPreset};
	use std::collections::HashMap;

	fn test_theme() -> Theme {
		let mut fg = ColorMap::new();
		for (key, hex) in [
			("accent", "#febc38"),
			("text", "#e6e6e6"),
			("muted", "#777d88"),
			("dim", "#5a5f66"),
			("success", "#89d281"),
			("error", "#fc3a4b"),
			("warning", "#e4c00f"),
			("border", "#3d424a"),
			("borderAccent", "#0088fa"),
			("borderMuted", "#596069"),
			("toolTitle", "#abcdef"),
			("customMessageLabel", "#b281d6"),
			("thinkingLow", "#ca8ecf"),
		] {
			fg.insert(key.to_string(), ColorValue::Hex(hex.to_string()));
		}
		let mut bg = ColorMap::new();
		for (key, hex) in [
			("selectedBg", "#31363f"),
			("userMessageBg", "#1b1d22"),
			("customMessageBg", "#1c1e24"),
			("toolPendingBg", "#1a1c20"),
			("toolSuccessBg", "#152a18"),
			("toolErrorBg", "#2a1518"),
			("statusLineBg", "#121212"),
		] {
			bg.insert(key.to_string(), ColorValue::Hex(hex.to_string()));
		}
		Theme::new(
			&fg,
			&bg,
			ColorMode::TrueColor,
			SymbolPreset::Unicode,
			&HashMap::new(),
			&NormalizedSpinnerFrames::default(),
			&[],
		)
		.expect("valid theme")
	}

	#[test]
	fn background_chrome_maps_every_token() {
		assert_eq!(background_chrome(None), BackgroundChrome::default());
		assert_eq!(
			background_chrome(Some(ThemeBg::UserMessageBg)),
			BackgroundChrome { tone: Some(Tone::User), role: Some("omp.user"), selected: None }
		);
		assert_eq!(
			background_chrome(Some(ThemeBg::CustomMessageBg)),
			BackgroundChrome { tone: Some(Tone::Info), role: Some("omp.custom"), selected: None }
		);
		assert_eq!(
			background_chrome(Some(ThemeBg::ToolPendingBg)),
			BackgroundChrome { tone: Some(Tone::Pending), role: Some("omp.tool"), selected: None }
		);
		assert_eq!(
			background_chrome(Some(ThemeBg::ToolSuccessBg)),
			BackgroundChrome { tone: Some(Tone::Success), role: Some("omp.tool"), selected: None }
		);
		assert_eq!(
			background_chrome(Some(ThemeBg::ToolErrorBg)),
			BackgroundChrome { tone: Some(Tone::Error), role: Some("omp.tool"), selected: None }
		);
		assert_eq!(
			background_chrome(Some(ThemeBg::SelectedBg)),
			BackgroundChrome { tone: Some(Tone::Accent), role: None, selected: Some(true) }
		);
		assert_eq!(
			background_chrome(Some(ThemeBg::StatusLineBg)),
			BackgroundChrome { tone: Some(Tone::Neutral), role: Some("omp.status"), selected: None }
		);
	}

	#[test]
	fn tone_color_is_total_over_tones() {
		assert_eq!(tone_color(Tone::Neutral), ThemeColor::Text);
		assert_eq!(tone_color(Tone::Accent), ThemeColor::Accent);
		assert_eq!(tone_color(Tone::Info), ThemeColor::Accent);
		assert_eq!(tone_color(Tone::Success), ThemeColor::Success);
		assert_eq!(tone_color(Tone::Warning), ThemeColor::Warning);
		assert_eq!(tone_color(Tone::Pending), ThemeColor::Warning);
		assert_eq!(tone_color(Tone::Error), ThemeColor::Error);
		assert_eq!(tone_color(Tone::Muted), ThemeColor::Muted);
		assert_eq!(tone_color(Tone::User), ThemeColor::CustomMessageLabel);
	}

	#[test]
	fn color_tone_resolves_severity_colours() {
		assert_eq!(color_tone(None), None);
		assert_eq!(color_tone(Some(ThemeColor::Border)), Some(Tone::Neutral));
		assert_eq!(color_tone(Some(ThemeColor::BorderAccent)), Some(Tone::Accent));
		assert_eq!(color_tone(Some(ThemeColor::Accent)), Some(Tone::Accent));
		assert_eq!(color_tone(Some(ThemeColor::BorderMuted)), Some(Tone::Muted));
		assert_eq!(color_tone(Some(ThemeColor::Muted)), Some(Tone::Muted));
		assert_eq!(color_tone(Some(ThemeColor::Dim)), Some(Tone::Muted));
		assert_eq!(color_tone(Some(ThemeColor::Success)), Some(Tone::Success));
		assert_eq!(color_tone(Some(ThemeColor::Error)), Some(Tone::Error));
		assert_eq!(color_tone(Some(ThemeColor::Warning)), Some(Tone::Warning));
	}

	#[test]
	fn color_tone_maps_every_thinking_level_to_accent() {
		for color in [
			ThemeColor::ThinkingText,
			ThemeColor::ThinkingOff,
			ThemeColor::ThinkingMinimal,
			ThemeColor::ThinkingLow,
			ThemeColor::ThinkingMedium,
			ThemeColor::ThinkingHigh,
			ThemeColor::ThinkingXhigh,
			ThemeColor::ThinkingMax,
		] {
			assert_eq!(color_tone(Some(color)), Some(Tone::Accent));
		}
	}

	#[test]
	fn color_tone_leaves_content_colours_without_a_tone() {
		for color in [
			ThemeColor::AssistantMessageText,
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
		] {
			assert_eq!(color_tone(Some(color)), None, "{color:?}");
		}
	}

	#[test]
	fn color_tone_token_rejects_unknown_names() {
		assert_eq!(color_tone_token("bogus"), None);
		assert_eq!(color_tone_token(""), None);
		assert_eq!(color_tone_token("statusLineModel"), None);
		assert_eq!(color_tone_token("thinking"), Some(Tone::Accent));
		assert_eq!(color_tone_token("thinkingish"), Some(Tone::Accent));
		assert_eq!(ThemeColor::from_str("bogus"), None);
	}

	#[test]
	fn samples_background_from_a_styler() {
		let theme = test_theme();
		for bg in ThemeBg::ALL {
			let name = bg.as_str();
			let value = theme.clone();
			let styler = move |text: &str| value.bg(name, text).expect("valid background");
			assert_eq!(sample_background(Some(&styler), &theme), Some(*bg));
		}
		assert_eq!(sample_background(None, &theme), None);
	}

	#[test]
	fn samples_foreground_from_a_styler() {
		let theme = test_theme();
		for (token, expected) in [
			("accent", ThemeColor::Accent),
			("border", ThemeColor::Border),
			("borderAccent", ThemeColor::BorderAccent),
			("borderMuted", ThemeColor::BorderMuted),
			("muted", ThemeColor::Muted),
			("dim", ThemeColor::Dim),
			("success", ThemeColor::Success),
			("error", ThemeColor::Error),
			("warning", ThemeColor::Warning),
			("toolTitle", ThemeColor::ToolTitle),
			("customMessageLabel", ThemeColor::CustomMessageLabel),
			("thinkingLow", ThemeColor::ThinkingLow),
		] {
			let value = theme.clone();
			let styler = move |text: &str| value.fg(token, text).expect("valid colour");
			assert_eq!(sample_foreground(Some(&styler), &theme), Some(expected));
		}
		assert_eq!(sample_foreground(None, &theme), None);

		// A colour the theme cannot reverse-map leaves no token.
		let unknown = |_text: &str| "\x1b[38;2;1;2;3mx".to_string();
		assert_eq!(sample_foreground(Some(&unknown), &theme), None);

		// Attributes alone are not colours.
		let bold = |text: &str| format!("\x1b[1m{text}\x1b[22m");
		assert_eq!(sample_foreground(Some(&bold), &theme), None);
	}
}
