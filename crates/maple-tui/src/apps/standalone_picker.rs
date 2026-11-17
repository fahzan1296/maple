//! Tightly scoped adapter for one-shot standalone TUI prompts.
//!
//! Reference: `l/src/apps/standalone-picker.ts`.
//!
//! Owns the `ProcessTerminal`/`TUI` lifecycle (mount, idempotent finish,
//! teardown) in the TS implementation so individual pickers only declare their
//! form field. Styling goes through plain-safe helpers when no theme is
//! initialized.
//!
//! Deviations:
//! - The crate has no `ProcessTerminal`/`TUI`/overlay/form/select-list runtime
//!   from `@oh-my-pi/pi-tui`, and no global theme init path to reuse here.
//! - Interactive selection/text entry is injected via [`StandaloneUi`] (narrow
//!   trait) so behavior is real for the caller/host instead of hardcoded stubs.
//! - `run_standalone_tui` executes the builder synchronously and collects the
//!   idempotent finish value; mount/teardown side effects are host-owned.
//! - Field chrome is plain-safe pass-through (`fgOrPlain` equivalent before
//!   theme init is "no styling"); [`StandaloneFieldTheme`] exposes that.

/// A selectable item in a single-column picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectItem {
	pub value: String,
	pub label: String,
	pub description: Option<String>,
}

/// Build context for a standalone prompt: the owned UI handle plus resolver.
#[derive(Debug)]
pub struct StandaloneTuiContext<T> {
	finished: Option<T>,
}

impl<T> StandaloneTuiContext<T> {
	pub fn new() -> Self {
		Self { finished: None }
	}
	/// Idempotent: record the finished value (first wins).
	pub fn finish(&mut self, value: T) {
		if self.finished.is_none() {
			self.finished = Some(value);
		}
	}
	/// Take the finished value, if any.
	pub fn take(&mut self) -> Option<T> {
		self.finished.take()
	}
}

impl<T> Default for StandaloneTuiContext<T> {
	fn default() -> Self {
		Self::new()
	}
}

/// Presentation controls for a standalone prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StandaloneTuiOptions {
	/// Present the component as an overlay instead of a plain child.
	pub overlay: bool,
}

/// Options for a standalone single-column item picker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StandaloneSelectOptions {
	pub current_value: Option<String>,
	pub max_visible: Option<usize>,
	pub noun: Option<String>,
	pub icon: Option<String>,
	pub subtitle: Option<String>,
	pub confirm: Option<String>,
}

/// Plain-safe field chrome: unstyled until theme initialization exists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StandaloneFieldTheme;

impl StandaloneFieldTheme {
	pub fn label(&self, text: &str) -> String {
		text.to_string()
	}
	pub fn description(&self, text: &str) -> String {
		text.to_string()
	}
	pub fn error(&self, text: &str) -> String {
		text.to_string()
	}
	pub fn hint(&self, text: &str) -> String {
		text.to_string()
	}
}

/// Interactive UI capabilities required by standalone pickers/prompts.
///
/// Injected because `ProcessTerminal`/`TUI` and form components are not
/// available in this crate; implementations may be interactive, scripted, or
/// test doubles.
pub trait StandaloneUi {
	/// Resolve selected item value, or `None` if cancelled.
	fn select_item(
		&mut self,
		title: &str,
		items: &[SelectItem],
		options: &StandaloneSelectOptions,
	) -> Option<String>;
	/// Resolve text input, or `None` if cancelled.
	fn prompt_text(&mut self, title: &str) -> Option<String>;
}

/// Run a one-shot TUI component built from `build(&mut ctx)` until finished.
///
/// The builder must call `ctx.finish(...)` to produce a value (idempotent).
pub fn run_standalone_tui<T, B>(mut build: B, _options: Option<StandaloneTuiOptions>) -> Option<T>
where
	B: FnMut(&mut StandaloneTuiContext<T>),
{
	let mut ctx = StandaloneTuiContext::new();
	build(&mut ctx);
	ctx.take()
}

/// Show a single-column item picker and resolve with the chosen item's value,
/// or `None` if the user cancelled.
pub fn select_standalone_item(
	ui: &mut dyn StandaloneUi,
	title: &str,
	items: &[SelectItem],
	options: Option<StandaloneSelectOptions>,
) -> Option<String> {
	let opts = options.unwrap_or_default();
	let chosen = ui.select_item(title, items, &opts);
	// Validate against provided items to preserve reference invariant.
	match chosen {
		Some(v) if items.iter().any(|i| i.value == v) => Some(v),
		Some(_) => None,
		None => None,
	}
}

/// Show a one-shot text prompt and resolve with trimmed input, or `None` when
/// cancelled or left empty.
pub fn prompt_standalone_text(ui: &mut dyn StandaloneUi, title: &str) -> Option<String> {
	match ui.prompt_text(title) {
		Some(v) => {
			let trimmed = v.trim().to_string();
			if trimmed.is_empty() {
				None
			} else {
				Some(trimmed)
			}
		}
		None => None,
	}
}

pub use prompt_standalone_text as promptStandaloneText;
pub use run_standalone_tui as runStandaloneTui;
pub use select_standalone_item as selectStandaloneItem;

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Default)]
	struct FakeUi {
		select: Option<String>,
		text: Option<String>,
		last_title: String,
		last_values: Vec<String>,
		last_current: Option<String>,
		last_max_visible: Option<usize>,
	}
	impl StandaloneUi for FakeUi {
		fn select_item(
			&mut self,
			title: &str,
			items: &[SelectItem],
			options: &StandaloneSelectOptions,
		) -> Option<String> {
			self.last_title = title.to_string();
			self.last_values = items.iter().map(|i| i.value.clone()).collect();
			self.last_current = options.current_value.clone();
			self.last_max_visible = options.max_visible;
			self.select.clone()
		}
		fn prompt_text(&mut self, title: &str) -> Option<String> {
			self.last_title = title.to_string();
			self.text.clone()
		}
	}

	fn items() -> Vec<SelectItem> {
		vec![
			SelectItem { value: "x".into(), label: "X".into(), description: None },
			SelectItem { value: "y".into(), label: "Y".into(), description: Some("yy".into()) },
		]
	}

	#[test]
	fn context_finish_is_idempotent_first_value_wins() {
		let mut ctx = StandaloneTuiContext::new();
		ctx.finish("a".to_string());
		ctx.finish("b".to_string());
		assert_eq!(ctx.take(), Some("a".to_string()));
		assert_eq!(ctx.take(), None);
	}

	#[test]
	fn run_standalone_tui_returns_finished_value() {
		let v = run_standalone_tui(|ctx: &mut StandaloneTuiContext<i32>| ctx.finish(7), None);
		assert_eq!(v, Some(7));
	}

	#[test]
	fn run_standalone_tui_returns_none_when_unfinished() {
		let v = run_standalone_tui(|_ctx: &mut StandaloneTuiContext<i32>| {}, None);
		assert_eq!(v, None);
	}

	#[test]
	fn select_standalone_item_uses_injected_choice_and_options() {
		let mut ui = FakeUi { select: Some("y".into()), ..Default::default() };
		let sel = select_standalone_item(
			&mut ui,
			"Pick:",
			&items(),
			Some(StandaloneSelectOptions {
				current_value: Some("x".into()),
				max_visible: Some(10),
				..Default::default()
			}),
		);
		assert_eq!(sel, Some("y".into()));
		assert_eq!(ui.last_title, "Pick:");
		assert_eq!(ui.last_values, vec!["x", "y"]);
		assert_eq!(ui.last_current.as_deref(), Some("x"));
		assert_eq!(ui.last_max_visible, Some(10));
	}

	#[test]
	fn select_standalone_item_rejects_unknown_or_cancelled() {
		let mut ui = FakeUi { select: Some("z".into()), ..Default::default() };
		assert_eq!(select_standalone_item(&mut ui, "Pick:", &items(), None), None);
		ui.select = None;
		assert_eq!(select_standalone_item(&mut ui, "Pick:", &items(), None), None);
	}

	#[test]
	fn prompt_standalone_text_trims_and_rejects_empty_or_cancel() {
		let mut ui = FakeUi { text: Some("  hello  ".into()), ..Default::default() };
		assert_eq!(prompt_standalone_text(&mut ui, "Name:"), Some("hello".into()));
		assert_eq!(ui.last_title, "Name:");
		ui.text = Some("   ".into());
		assert_eq!(prompt_standalone_text(&mut ui, "Name:"), None);
		ui.text = None;
		assert_eq!(prompt_standalone_text(&mut ui, "Name:"), None);
	}

	#[test]
	fn field_theme_passthrough() {
		let t = StandaloneFieldTheme;
		assert_eq!(t.label("a"), "a");
		assert_eq!(t.description("b"), "b");
		assert_eq!(t.error("c"), "c");
		assert_eq!(t.hint("d"), "d");
	}
}
