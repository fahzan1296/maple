//! Reference: `l/src/prompt/composer.ts`.
//!
//! Composer data layer: the live settings that shape the composer before and
//! after session adoption ([`ComposerPreferences`] and [`COMPOSER_DEFAULTS`]),
//! the welcome/status payloads the first frame paints from
//! ([`ComposerWelcomeUpdate`], [`ComposerStatusCache`]), and the pure viewport
//! click routing seam ([`route_viewport_click`]).
//!
//! Deviations:
//! - The reference's `Composer` class owns the terminal, the welcome header,
//!   transcript retirement, resize replay and link-hint painting. This crate's
//!   engine ([`crate::tui::Tui`], [`crate::screen::Screen`],
//!   [`crate::chrome::transcript_container`], [`crate::components::composer`])
//!   owns those concerns with a different frame contract (no
//!   `TerminalFrameProvider`, no history offer/acknowledge watermark), so the
//!   terminal-owning class is not re-created here; its data and pure seams are.
//! - `StatusLineStartupData` has no port in this crate yet: the cached status
//!   payload stays a [`serde_json::Value`] validated structurally.
//! - `ResizeScrollbackMode` and `WordCompletionMethod` are declared here / in
//!   [`crate::prompt::word_completion`] rather than in the TUI engine.

use std::sync::LazyLock;

use serde_json::Value;

use crate::prompt::word_completion::WordCompletionMethod;

/// How the engine reconciles scrollback across a resize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeScrollbackMode {
	Append,
	Rebuild,
	Preserve,
}

impl ResizeScrollbackMode {
	pub fn as_str(self) -> &'static str {
		match self {
			ResizeScrollbackMode::Append => "append",
			ResizeScrollbackMode::Rebuild => "rebuild",
			ResizeScrollbackMode::Preserve => "preserve",
		}
	}

	pub fn from_str(name: &str) -> Option<Self> {
		match name {
			"append" => Some(ResizeScrollbackMode::Append),
			"rebuild" => Some(ResizeScrollbackMode::Rebuild),
			"preserve" => Some(ResizeScrollbackMode::Preserve),
			_ => None,
		}
	}
}

/// Live settings that affect the composer before and after session adoption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposerPreferences {
	pub quiet: bool,
	pub composer_shape: String,
	pub show_hardware_cursor: bool,
	pub max_inline_images: usize,
	pub resize_scrollback: ResizeScrollbackMode,
	/// Send rows a growing HUD or dialog pushes off screen straight to scrollback
	/// instead of holding them back until it shrinks.
	pub complete_scrollback: bool,
	pub ime_safe_cursor: bool,
	pub autocomplete_max_visible: usize,
	pub spelling_typo_detection: bool,
	pub spelling_autocomplete: WordCompletionMethod,
	pub spelling_autocorrect: bool,
}

impl Default for ComposerPreferences {
	fn default() -> Self {
		ComposerPreferences {
			quiet: false,
			composer_shape: "band".to_string(),
			show_hardware_cursor: true,
			max_inline_images: 8,
			resize_scrollback: ResizeScrollbackMode::Rebuild,
			complete_scrollback: false,
			ime_safe_cursor: false,
			autocomplete_max_visible: 10,
			spelling_typo_detection: true,
			spelling_autocomplete: WordCompletionMethod::Auto,
			spelling_autocorrect: false,
		}
	}
}

/// Settings-schema-compatible defaults used when constructing a
/// dependency-free composer.
pub static COMPOSER_DEFAULTS: LazyLock<ComposerPreferences> =
	LazyLock::new(ComposerPreferences::default);

/// Background self-update activity after the version label.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VersionUpdate {
	/// Text drawn after the version, e.g. `↑ 1.2.3`.
	pub label: String,
	/// Whether the update is being installed.
	pub installing: bool,
}

/// Welcome data that can be supplied initially or patched as startup resolves
/// it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposerWelcomeUpdate {
	pub version: Option<String>,
	/// `Some(None)` clears the activity.
	pub version_update: Option<Option<VersionUpdate>>,
}

/// ANSI wrapper of the editor border when persisted (session accent or thinking
/// color).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorderColor {
	pub prefix: String,
	pub suffix: String,
}

/// Status-bar inputs persisted by the last session.
#[derive(Clone, Debug, PartialEq)]
pub struct ComposerStatusCache {
	pub border_color: Option<BorderColor>,
	/// Status-line startup data; see the module note on why this stays a JSON
	/// payload.
	pub status_line: Value,
}

/// Optional dependencies and initial state for a standalone composer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComposerOptions {
	pub preferences: Option<ComposerPreferences>,
	pub welcome: Option<ComposerWelcomeUpdate>,
	pub status: Option<ComposerStatusCache>,
}

/// Controls the first terminal paint for a composer that does not already own
/// the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ComposerStartOptions {
	pub clear_scrollback: bool,
	pub play_welcome_intro: bool,
	/// Paint without owning stdin: the tty keeps cooked-mode echo/editing so
	/// typing stays visible while startup module loading blocks the event loop.
	pub defer_input: bool,
}

/// Terminal ownership states of the reference `Composer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerPhase {
	/// Constructed, not started.
	Idle,
	/// Owns the terminal.
	Started,
	/// Handed to the interactive mode.
	Transferred,
	/// Stopped.
	Stopped,
}

/// Double-interrupt tracker: a second interrupt inside [`DOUBLE_INTERRUPT_MS`]
/// exits.
pub const DOUBLE_INTERRUPT_MS: u64 = 500;

/// One click target's row span within the mutable viewport (half-open
/// `[start, end)`).
#[derive(Clone)]
pub struct ViewportClickSpan {
	pub start: usize,
	pub end: usize,
	/// Candidate subagent ids for a span-local row.
	pub candidates: std::sync::Arc<dyn Fn(usize) -> Vec<String>>,
}

impl std::fmt::Debug for ViewportClickSpan {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ViewportClickSpan")
			.field("start", &self.start)
			.field("end", &self.end)
			.finish_non_exhaustive()
	}
}

/// Row-level click target: maps rendered rows to subagent ids.
pub trait ViewportClickRowTarget {
	fn get_click_agent_at_row(&self, row: usize) -> Option<String>;
}

/// Candidates under a mutable-viewport line: the first span containing it.
pub fn route_viewport_click(spans: &[ViewportClickSpan], index: usize) -> Vec<String> {
	for span in spans {
		if index < span.start || index >= span.end {
			continue;
		}
		return (span.candidates)(index - span.start);
	}
	Vec::new()
}

/// Shift one span into viewport coordinates, clipping its head and offsetting
/// the callback so a clipped first row does not hit-test as span-local row 0.
pub fn shift_click_span(
	span: &ViewportClickSpan,
	base: isize,
	viewport_length: usize,
) -> Option<ViewportClickSpan> {
	let start = span.start as isize + base;
	let end = ((span.end as isize + base).max(0) as usize).min(viewport_length);
	let clamped = start.max(0) as usize;
	if end <= clamped {
		return None;
	}
	let skew = clamped as isize - start;
	let candidates = span.candidates.clone();
	Some(ViewportClickSpan {
		start: clamped,
		end,
		candidates: std::sync::Arc::new(move |local: usize| candidates((local as isize + skew).max(0) as usize)),
	})
}

/// Nested background opens inside a hovered row (SGR 40–47/100–107/48;…),
/// stripped before the hover band wraps the line.
pub static NESTED_BG_OPEN_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
	regex::Regex::new("\u{1b}\\[(?:4[0-7]|10[0-7]|48;[0-9;]*)m").expect("nested background open pattern")
});

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Arc;

	fn span(start: usize, end: usize, ids: &[&str]) -> ViewportClickSpan {
		let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
		ViewportClickSpan {
			start,
			end,
			candidates: Arc::new(move |_local: usize| ids.clone()),
		}
	}

	#[test]
	fn defaults_match_the_settings_schema() {
		let defaults = &*COMPOSER_DEFAULTS;
		assert!(!defaults.quiet);
		assert_eq!(defaults.composer_shape, "band");
		assert!(defaults.show_hardware_cursor);
		assert_eq!(defaults.max_inline_images, 8);
		assert_eq!(defaults.resize_scrollback, ResizeScrollbackMode::Rebuild);
		assert!(!defaults.complete_scrollback);
		assert!(!defaults.ime_safe_cursor);
		assert_eq!(defaults.autocomplete_max_visible, 10);
		assert!(defaults.spelling_typo_detection);
		assert_eq!(defaults.spelling_autocomplete.as_str(), "auto");
		assert!(!defaults.spelling_autocorrect);
	}

	#[test]
	fn routes_clicks_through_the_first_containing_span() {
		let spans = vec![span(2, 4, &["a1"]), span(4, 6, &["b1", "b2"])];
		assert_eq!(route_viewport_click(&spans, 0), Vec::<String>::new());
		assert_eq!(route_viewport_click(&spans, 2), vec!["a1".to_string()]);
		assert_eq!(route_viewport_click(&spans, 3), vec!["a1".to_string()]);
		assert_eq!(route_viewport_click(&spans, 5), vec!["b1".to_string(), "b2".to_string()]);
		assert_eq!(route_viewport_click(&spans, 6), Vec::<String>::new());
	}

	#[test]
	fn shifted_spans_clip_and_offset_the_callback() {
		let original = ViewportClickSpan {
			start: 1,
			end: 3,
			candidates: Arc::new(|local: usize| vec![format!("row{local}")]),
		};
		// Base -2 clips one head row; the first visible row must still report local 1.
		let clipped = shift_click_span(&original, -2, 10).expect("span");
		assert_eq!((clipped.start, clipped.end), (0, 1));
		assert_eq!((clipped.candidates)(0), vec!["row1".to_string()]);
		// Base -1 fits the whole span at the top.
		let kept = shift_click_span(&original, -1, 10).expect("span");
		assert_eq!((kept.start, kept.end), (0, 2));
		assert_eq!((kept.candidates)(0), vec!["row0".to_string()]);
		// A span fully past the viewport end disappears.
		assert!(shift_click_span(&span(20, 30, &["x"]), 0, 10).is_none());
		assert!(shift_click_span(&span(0, 1, &["x"]), -5, 10).is_none());
	}

	#[test]
	fn strips_nested_background_opens() {
		let line = "\u{1b}[41mred\u{1b}[49m \u{1b}[48;5;236mdark\u{1b}[39m";
		assert_eq!(
			NESTED_BG_OPEN_PATTERN.replace_all(line, ""),
			"red\u{1b}[49m dark\u{1b}[39m"
		);
		assert_eq!(NESTED_BG_OPEN_PATTERN.replace_all("plain", ""), "plain");
	}

	#[test]
	fn scrollback_mode_round_trips() {
		for mode in [
			ResizeScrollbackMode::Append,
			ResizeScrollbackMode::Rebuild,
			ResizeScrollbackMode::Preserve,
		] {
			assert_eq!(ResizeScrollbackMode::from_str(mode.as_str()), Some(mode));
		}
		assert_eq!(ResizeScrollbackMode::from_str("other"), None);
	}
}
