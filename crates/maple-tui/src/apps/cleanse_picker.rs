//! Standalone TUI pickers for `omp cleanse`.
//!
//! Reference: `l/src/apps/cleanse-picker.ts`.
//!
//! One-shot prompts, resolved on select/submit/cancel so the command can keep
//! writing plain stdout afterwards.
//!
//! Deviations:
//! - Interactive picker/text prompt runtime is unavailable in this crate;
//!   [`pick_cleanse_target`] and [`prompt_cleanse_request`] delegate to injected
//!   [`StandaloneUi`] for real host/test-driven interaction instead of returning
//!   fixed cancel/none stubs.
//! - Pure helpers [`cleanse_target_items`] and [`resolve_cleanse_target`] expose
//!   the same item shape/order and selection resolution logic as the TS flow.

use crate::apps::standalone_picker::{
	prompt_standalone_text, select_standalone_item, SelectItem, StandaloneSelectOptions, StandaloneUi,
};

/// Checker metadata presented by the target picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanseCheckerChoice {
	pub id: String,
	pub label: String,
	pub language: String,
	pub command: String,
}

/// Selected checker scope or free-form repair request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanseTargetChoice {
	All,
	Checker { id: String },
	Request { request: String },
	Cancel,
}

/// Build picker items for cleanse target selection (same shape/order as TS).
pub fn cleanse_target_items(checkers: &[CleanseCheckerChoice]) -> Vec<SelectItem> {
	let mut items = Vec::with_capacity(checkers.len() + 2);
	items.push(SelectItem {
		value: "all".to_string(),
		label: format!(
			"Run all {} discovered checker{}",
			checkers.len(),
			if checkers.len() == 1 { "" } else { "s" }
		),
		description: None,
	});
	for checker in checkers {
		items.push(SelectItem {
			value: format!("checker:{}", checker.id),
			label: checker.label.clone(),
			description: Some(format!("{} — {}", checker.language, checker.command)),
		});
	}
	items.push(SelectItem {
		value: "request".to_string(),
		label: "Describe what to fix…".to_string(),
		description: Some("A discovery agent figures out the command to run".to_string()),
	});
	items
}

/// Resolve a picker selection string into a target choice.
/// `request_text` is used when selection is `"request"`.
pub fn resolve_cleanse_target(
	selection: Option<&str>,
	request_text: Option<&str>,
) -> CleanseTargetChoice {
	match selection {
		None => CleanseTargetChoice::Cancel,
		Some("all") => CleanseTargetChoice::All,
		Some("request") => match request_text.map(str::trim).filter(|s| !s.is_empty()) {
			Some(r) => CleanseTargetChoice::Request { request: r.to_string() },
			None => CleanseTargetChoice::Cancel,
		},
		Some(s) if s.starts_with("checker:") => {
			CleanseTargetChoice::Checker { id: s["checker:".len()..].to_string() }
		}
		Some(_) => CleanseTargetChoice::Cancel,
	}
}

/// Pick between running every discovered checker, one specific checker, or a
/// free-form request via injected UI.
pub fn pick_cleanse_target(
	ui: &mut dyn StandaloneUi,
	checkers: &[CleanseCheckerChoice],
) -> CleanseTargetChoice {
	let items = cleanse_target_items(checkers);
	let selection = select_standalone_item(
		ui,
		"Select what to cleanse:",
		&items,
		Some(StandaloneSelectOptions {
			max_visible: Some(12),
			noun: Some("checkers".to_string()),
			icon: Some("wand".to_string()),
			subtitle: Some("Run the checkers, then repair what they find".to_string()),
			confirm: Some("Cleanse".to_string()),
			..Default::default()
		}),
	);
	if selection.as_deref() == Some("request") {
		let req = prompt_cleanse_request(ui);
		return resolve_cleanse_target(Some("request"), req.as_deref());
	}
	resolve_cleanse_target(selection.as_deref(), None)
}

pub use pick_cleanse_target as pickCleanseTarget;
pub use prompt_cleanse_request as promptCleanseRequest;

/// One-shot text prompt for a free-form cleanse request; `None` when cancelled
/// or left empty.
pub fn prompt_cleanse_request(ui: &mut dyn StandaloneUi) -> Option<String> {
	prompt_standalone_text(
		ui,
		"Describe what to detect and fix (e.g. \"ts errors\"):",
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::apps::standalone_picker::{SelectItem, StandaloneSelectOptions, StandaloneUi};

	#[derive(Default)]
	struct Ui {
		select: Option<String>,
		text: Option<String>,
		seen_noun: Option<String>,
		seen_icon: Option<String>,
		seen_max: Option<usize>,
	}
	impl StandaloneUi for Ui {
		fn select_item(
			&mut self,
			_title: &str,
			_items: &[SelectItem],
			options: &StandaloneSelectOptions,
		) -> Option<String> {
			self.seen_noun = options.noun.clone();
			self.seen_icon = options.icon.clone();
			self.seen_max = options.max_visible;
			self.select.clone()
		}
		fn prompt_text(&mut self, _title: &str) -> Option<String> {
			self.text.clone()
		}
	}

	fn sample() -> Vec<CleanseCheckerChoice> {
		vec![
			CleanseCheckerChoice { id: "ts".into(), label: "TypeScript".into(), language: "ts".into(), command: "tsc".into() },
		]
	}

	#[test]
	fn target_items_shape() {
		let items = cleanse_target_items(&sample());
		assert_eq!(items[0].value, "all");
		assert!(items[0].label.contains("Run all 1 discovered checker"));
		assert_eq!(items[1].value, "checker:ts");
		assert_eq!(items[2].value, "request");
	}

	#[test]
	fn resolve_variants() {
		assert_eq!(resolve_cleanse_target(None, None), CleanseTargetChoice::Cancel);
		assert_eq!(resolve_cleanse_target(Some("all"), None), CleanseTargetChoice::All);
		assert_eq!(
			resolve_cleanse_target(Some("checker:x"), None),
			CleanseTargetChoice::Checker { id: "x".into() }
		);
		assert_eq!(
			resolve_cleanse_target(Some("request"), Some(" fix it ")),
			CleanseTargetChoice::Request { request: "fix it".into() }
		);
		assert_eq!(resolve_cleanse_target(Some("request"), Some("  ")), CleanseTargetChoice::Cancel);
	}

	#[test]
	fn pick_delegates_and_passes_options() {
		let mut ui = Ui { select: Some("all".into()), ..Default::default() };
		let got = pick_cleanse_target(&mut ui, &sample());
		assert_eq!(got, CleanseTargetChoice::All);
		assert_eq!(ui.seen_noun.as_deref(), Some("checkers"));
		assert_eq!(ui.seen_icon.as_deref(), Some("wand"));
		assert_eq!(ui.seen_max, Some(12));
	}

	#[test]
	fn pick_request_prompts_and_resolves() {
		let mut ui = Ui { select: Some("request".into()), text: Some("ts errors".into()), ..Default::default() };
		assert_eq!(
			pick_cleanse_target(&mut ui, &sample()),
			CleanseTargetChoice::Request { request: "ts errors".into() }
		);
		ui.text = None;
		assert_eq!(pick_cleanse_target(&mut ui, &sample()), CleanseTargetChoice::Cancel);
	}

	#[test]
	fn prompt_request_trims_and_cancels() {
		let mut ui = Ui { text: Some("  abc  ".into()), ..Default::default() };
		assert_eq!(prompt_cleanse_request(&mut ui).as_deref(), Some("abc"));
		ui.text = Some("   ".into());
		assert_eq!(prompt_cleanse_request(&mut ui), None);
	}
}
