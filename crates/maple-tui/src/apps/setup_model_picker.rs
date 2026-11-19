//! Standalone TUI model picker used by `omp setup speech`.
//!
//! Reference: `l/src/apps/setup-model-picker.ts`.
//!
//! One-shot prompt: resolve on select/cancel and let the adapter tear the UI
//! down.
//!
//! Deviations:
//! - Interactive picker runtime is unavailable in this crate; selection is
//!   delegated to injected [`StandaloneUi`] (real host/test implementation)
//!   instead of heuristic defaults.

use crate::apps::standalone_picker::{
	select_standalone_item, SelectItem, StandaloneSelectOptions, StandaloneUi,
};

/// Show a single-column model picker and resolve with the chosen item's value,
/// or `None` if the user cancelled. `current_value` pre-selects the matching row.
pub fn select_setup_model(
	ui: &mut dyn StandaloneUi,
	title: &str,
	items: &[SelectItem],
	current_value: &str,
) -> Option<String> {
	select_standalone_item(
		ui,
		title,
		items,
		Some(StandaloneSelectOptions {
			current_value: Some(current_value.to_string()),
			max_visible: Some(10),
			noun: Some("models".to_string()),
			icon: Some("model".to_string()),
			subtitle: None,
			confirm: None,
		}),
	)
}

pub use select_setup_model as selectSetupModel;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::apps::standalone_picker::{SelectItem, StandaloneSelectOptions, StandaloneUi};

 	#[derive(Default)]

	#[derive(Default)]
	struct Ui {
		choice: Option<String>,
		seen_current: Option<String>,
		seen_max: Option<usize>,
		seen_noun: Option<String>,
		seen_icon: Option<String>,
	}
	impl StandaloneUi for Ui {
		fn select_item(
			&mut self,
			_title: &str,
			_items: &[SelectItem],
			options: &StandaloneSelectOptions,
		) -> Option<String> {
			self.seen_current = options.current_value.clone();
			self.seen_max = options.max_visible;
			self.seen_noun = options.noun.clone();
			self.seen_icon = options.icon.clone();
			self.choice.clone()
		}
		fn prompt_text(&mut self, _title: &str) -> Option<String> {
			None
		}
	}

	fn items() -> Vec<SelectItem> {
		vec![
			SelectItem { value: "a".into(), label: "Model A".into(), description: None },
			SelectItem { value: "b".into(), label: "Model B".into(), description: Some("desc".into()) },
		]
	}

	#[test]
	fn delegates_to_ui_with_picker_options() {
		let mut ui = Ui { choice: Some("b".into()), ..Default::default() };
		let got = select_setup_model(&mut ui, "Pick", &items(), "a");
		assert_eq!(got, Some("b".into()));
		assert_eq!(ui.seen_current.as_deref(), Some("a"));
		assert_eq!(ui.seen_max, Some(10));
		assert_eq!(ui.seen_noun.as_deref(), Some("models"));
		assert_eq!(ui.seen_icon.as_deref(), Some("model"));
	}

	#[test]
	fn cancel_or_invalid_returns_none() {
		let mut ui = Ui { choice: None, ..Default::default() };
		assert_eq!(select_setup_model(&mut ui, "Pick", &items(), "a"), None);
		ui.choice = Some("z".into());
		assert_eq!(select_setup_model(&mut ui, "Pick", &items(), "a"), None);
	}
}
