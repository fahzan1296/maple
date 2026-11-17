//! Reference: `l/src/prompt/composer-hints.ts`.
//!
//! Composer hints: the right-aligned placeholder shown in the empty composer to
//! surface gestures that matter right now (e.g. background agents are running).
//!
//! Hints are checked in priority order; the first one that applies wins. A hint
//! retires for good once the user has performed its gesture
//! [`ComposerHint::learned_after`] times — the host records each use under the
//! hint's id and reports counts through [`ComposerHintContext::uses`].
//!
//! Deviations:
//! - The global `theme` becomes an explicit [`Theme`] argument.
//! - `key-hint-format.ts` has no port in this crate yet, so `formatKeyHint` /
//!   `formatDoubleTap` resolve through
//!   [`crate::native::overlay::format_tooltip_key`] (the unicode keycap preset
//!   the reference also uses for tooltips).
//! - `uses` is a borrowed counting function rather than a method on a class
//!   context.

use std::sync::LazyLock;

use crate::keybindings::KeybindingsManager;
use crate::native::overlay::format_tooltip_key;
use crate::theme::symbols::{preset_symbols, SymbolPreset};
use crate::theme::Theme;

/// Stable hint ids; persisted as usage-counter keys, so never rename one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComposerHintId {
	Agents,
	Effort,
}

impl ComposerHintId {
	/// The persisted counter key.
	pub fn as_str(self) -> &'static str {
		match self {
			ComposerHintId::Agents => "agents",
			ComposerHintId::Effort => "effort",
		}
	}

	/// Parse a persisted counter key.
	pub fn from_str(name: &str) -> Option<Self> {
		match name {
			"agents" => Some(ComposerHintId::Agents),
			"effort" => Some(ComposerHintId::Effort),
			_ => None,
		}
	}
}

/// Live state that hint conditions read. Built by the host on every render of
/// the empty composer.
pub struct ComposerHintContext<'a> {
	/// Subagents currently running in the background.
	pub running_agents: usize,
	/// The view is focused on a subagent session instead of the main one.
	pub focused_on_agent: bool,
	/// The viewed session already has messages.
	pub conversation_started: bool,
	pub keybindings: &'a KeybindingsManager,
	uses: &'a dyn Fn(ComposerHintId) -> u32,
}

impl<'a> ComposerHintContext<'a> {
	pub fn new(
		keybindings: &'a KeybindingsManager,
		uses: &'a dyn Fn(ComposerHintId) -> u32,
	) -> Self {
		Self {
			running_agents: 0,
			focused_on_agent: false,
			conversation_started: false,
			keybindings,
			uses,
		}
	}

	/// Times the user has performed the gesture a hint teaches.
	pub fn uses(&self, id: ComposerHintId) -> u32 {
		(self.uses)(id)
	}
}

/// A resolved gesture tip: `key` renders highlighted, `label` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedHint {
	pub key: String,
	pub label: String,
}

/// A gesture tip.
pub struct ComposerHint {
	pub id: ComposerHintId,
	/// Uses of the gesture after which the hint stops showing.
	pub learned_after: u32,
	resolve: Box<dyn Fn(&ComposerHintContext<'_>) -> Option<ResolvedHint> + Send + Sync>,
}

impl ComposerHint {
	/// Key and label when the hint applies, otherwise `None`.
	pub fn resolve(&self, ctx: &ComposerHintContext<'_>) -> Option<ResolvedHint> {
		(self.resolve)(ctx)
	}
}

/// `/[A-Za-z0-9]/` — whether the label is a word rather than a glyph.
fn has_letter(label: &str) -> bool {
	label.chars().any(|ch| ch.is_ascii_alphanumeric())
}

/// Format a double-tap gesture: `←←` with glyphs, `Left Left` with words.
pub fn format_double_tap(key: &str) -> String {
	let label = format_tooltip_key(key);
	let joiner = preset_symbols(SymbolPreset::Unicode)
		.get("key.joiner")
		.unwrap_or("");
	if has_letter(&label) {
		format!("{label} {label}")
	} else {
		format!("{label}{joiner}{label}")
	}
}

/// Registered hints, highest priority first.
pub static COMPOSER_HINTS: LazyLock<Vec<ComposerHint>> = LazyLock::new(|| {
	vec![
		// Matches the ←← gesture in the input controller, which opens the agent hub
		// from the main session (in a focused view it hops to the parent instead).
		ComposerHint {
			id: ComposerHintId::Agents,
			learned_after: 3,
			resolve: Box::new(|ctx| {
				if ctx.running_agents == 0 || ctx.focused_on_agent {
					return None;
				}
				let noun = if ctx.running_agents == 1 { "agent" } else { "agents" };
				Some(ResolvedHint {
					key: format_double_tap("left"),
					label: format!("to see {} running {noun}", ctx.running_agents),
				})
			}),
		},
		ComposerHint {
			id: ComposerHintId::Effort,
			learned_after: 3,
			resolve: Box::new(|ctx| {
				if ctx.conversation_started {
					return None;
				}
				let key = ctx.keybindings.get_keys("app.thinking.cycle").into_iter().next()?;
				Some(ResolvedHint {
					key: format_tooltip_key(&key),
					label: "to change thinking effort".to_string(),
				})
			}),
		},
	]
});

/// Render the highest-priority applicable, not-yet-learned hint, if any.
pub fn resolve_composer_hint(ctx: &ComposerHintContext<'_>, theme: &Theme) -> Option<String> {
	for hint in COMPOSER_HINTS.iter() {
		if ctx.uses(hint.id) >= hint.learned_after {
			continue;
		}
		let Some(resolved) = hint.resolve(ctx) else {
			continue;
		};
		let key = theme.fg("accent", &resolved.key).unwrap_or(resolved.key);
		let label = theme.fg("dim", &theme.italic(&resolved.label)).unwrap_or(resolved.label);
		return Some(format!("{key} {label}"));
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::create_theme;

	fn theme() -> Theme {
		create_theme("dark", None).expect("dark theme")
	}

	fn hint_for(ctx: &ComposerHintContext<'_>) -> Option<String> {
		resolve_composer_hint(ctx, &theme()).map(|hint| crate::ansi_scan::strip_ansi(&hint))
	}

	#[test]
	fn agents_hint_only_while_agents_run_on_the_main_view() {
		let bindings = KeybindingsManager::new();
		let uses = |_id| 0;
		let mut ctx = ComposerHintContext::new(&bindings, &uses);

		ctx.running_agents = 2;
		let hint = hint_for(&ctx).expect("hint");
		assert!(hint.ends_with("to see 2 running agents"), "hint {hint}");

		ctx.running_agents = 1;
		let hint = hint_for(&ctx).expect("hint");
		assert!(hint.ends_with("to see 1 running agent"), "hint {hint}");

		ctx.focused_on_agent = true;
		assert_eq!(hint_for(&ctx), None);
	}

	#[test]
	fn effort_hint_only_before_conversation_starts() {
		let bindings = KeybindingsManager::new();
		let uses = |_id| 0;
		let mut ctx = ComposerHintContext::new(&bindings, &uses);
		let hint = hint_for(&ctx).expect("hint");
		assert!(hint.ends_with("to change thinking effort"), "hint {hint}");

		ctx.conversation_started = true;
		assert_eq!(hint_for(&ctx), None);
	}

	#[test]
	fn learned_hints_retire() {
		let bindings = KeybindingsManager::new();
		let uses_agents = |id: ComposerHintId| if id == ComposerHintId::Agents { 3 } else { 0 };
		let mut ctx = ComposerHintContext::new(&bindings, &uses_agents);
		ctx.running_agents = 1;
		let hint = hint_for(&ctx).expect("effort hint falls through");
		assert!(hint.ends_with("to change thinking effort"), "hint {hint}");
	}

	#[test]
	fn double_tap_repeats_the_label() {
		let cases = ["left", "f5"];
		for key in cases {
			let formatted = format_double_tap(key);
			let label = format_tooltip_key(key);
			assert_eq!(formatted.matches(&label).count(), 2, "key {key}");
		}
	}
}
