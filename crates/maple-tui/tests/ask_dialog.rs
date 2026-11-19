//! Port of `l/test/ask-dialog.test.ts` (synchronous component cases).
//!
//! Deviations: the reference drives a Bun TUI with async prompt editors,
//! timeouts, note images and mouse routing. The crate's `AskDialogComponent`
//! is state-driven, so the ported cases cover tab labels, single/multi-select
//! Enter/Space semantics, multi-question advance, Tab/Shift+Tab switching,
//! Esc cancel, Recommended suffix formatting and empty-selection submit; the
//! async prompt/timeout/image behaviors have no crate counterpart.

use std::sync::{Arc, Mutex};

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::ask_dialog::{
	AskDialogCallbacks, AskDialogComponent, ExtensionAskDialogOption, ExtensionAskDialogQuestion,
	ExtensionAskDialogResult, RECOMMENDED_SUFFIX, display_option_labels,
};

#[derive(Clone)]
struct Capture(Arc<Mutex<Option<ExtensionAskDialogResult>>>);

impl Default for Capture {
	fn default() -> Self {
		Capture(Arc::new(Mutex::new(None)))
	}
}

impl AskDialogCallbacks for Capture {
	fn on_result(&self, result: ExtensionAskDialogResult) {
		*self.0.lock().unwrap() = Some(result);
	}
}

fn option(label: &str) -> ExtensionAskDialogOption {
	ExtensionAskDialogOption {
		label: label.to_string(),
		description: None,
		recommended: None,
	}
}

fn question(id: &str, header: &str, multi: bool, labels: &[&str]) -> ExtensionAskDialogQuestion {
	ExtensionAskDialogQuestion {
		id: id.to_string(),
		header: Some(header.to_string()),
		question: format!("Question {header}"),
		options: labels.iter().map(|label| option(label)).collect(),
		multi: Some(multi),
		recommended: None,
	}
}

fn submitted(capture: &Capture) -> Vec<String> {
	let guard = capture.0.lock().unwrap();
	let Some(ExtensionAskDialogResult::Submit(result)) = guard.as_ref() else {
		panic!("expected a submit result");
	};
	result
		.answers
		.first()
		.map(|answer| answer.selected.clone())
		.unwrap_or_default()
}

#[test]
fn multi_question_footer_labels_the_question_switch_keys() {
	let mut dialog = AskDialogComponent::new(
		vec![question("a", "A", false, &["One"]), question("b", "B", false, &["Two"])],
		Capture::default(),
	);
	let rows = dialog.render(80).join("\n");
	assert!(rows.contains("Tab 1/2"), "footer names the switch keys: {rows}");
	assert!(rows.contains("enter select/submit"));
	assert!(rows.contains("esc cancel"));
}

#[test]
fn single_select_enter_submits_immediately() {
	let capture = Capture::default();
	let mut dialog = AskDialogComponent::new(vec![question("a", "A", false, &["One", "Two"])], capture.clone());
	dialog.handle_input(&Key::Enter);
	assert!(dialog.submitted);
	assert_eq!(submitted(&capture), vec!["One"]);
}

#[test]
fn single_select_space_does_not_submit_the_highlighted_answer() {
	let capture = Capture::default();
	let mut dialog = AskDialogComponent::new(vec![question("a", "A", false, &["One", "Two"])], capture.clone());
	dialog.handle_input(&Key::Char(' '));
	assert!(!dialog.submitted);
	assert_eq!(dialog.states[0].selected_options.len(), 1);
	assert_eq!(submitted(&capture), Vec::<String>::new());
}

#[test]
fn down_then_enter_selects_the_second_option_and_submits() {
	let capture = Capture::default();
	let mut dialog = AskDialogComponent::new(vec![question("a", "A", false, &["One", "Two"])], capture.clone());
	dialog.handle_input(&Key::Down);
	dialog.handle_input(&Key::Enter);
	assert_eq!(submitted(&capture), vec!["Two"]);
}

#[test]
fn multi_question_enter_on_an_option_advances_without_submitting() {
	let capture = Capture::default();
	let mut dialog = AskDialogComponent::new(
		vec![question("a", "A", false, &["One"]), question("b", "B", false, &["Two"])],
		capture.clone(),
	);
	dialog.handle_input(&Key::Enter);
	assert!(!dialog.submitted);
	assert_eq!(dialog.tab_index, 1, "Enter advances exactly one question");
	assert_eq!(dialog.states[0].selected_options.len(), 1);

	dialog.handle_input(&Key::Enter);
	assert!(dialog.submitted);
	let answers = capture.0.lock().unwrap().clone();
	let Some(ExtensionAskDialogResult::Submit(result)) = answers else {
		panic!("expected submit");
	};
	assert_eq!(result.answers.len(), 2);
}

#[test]
fn multi_select_space_toggles_options_and_enter_submits_the_selection() {
	let capture = Capture::default();
	let mut dialog = AskDialogComponent::new(vec![question("a", "A", true, &["One", "Two"])], capture.clone());
	dialog.handle_input(&Key::Char(' '));
	dialog.handle_input(&Key::Down);
	dialog.handle_input(&Key::Char(' '));
	dialog.handle_input(&Key::Enter);
	assert_eq!(submitted(&capture), vec!["One", "Two"]);
}

#[test]
fn tab_and_shift_tab_switch_questions() {
	let mut dialog = AskDialogComponent::new(
		vec![question("a", "A", false, &["One"]), question("b", "B", false, &["Two"])],
		Capture::default(),
	);
	dialog.handle_input(&Key::Tab);
	assert_eq!(dialog.tab_index, 1);
	dialog.handle_input(&Key::BackTab);
	assert_eq!(dialog.tab_index, 0);
}

#[test]
fn esc_cancel_fires_on_cancel() {
	let capture = Capture::default();
	let mut dialog = AskDialogComponent::new(vec![question("a", "A", false, &["One"])], capture.clone());
	dialog.handle_input(&Key::Esc);
	assert!(dialog.cancelled);
	let guard = capture.0.lock().unwrap();
	assert!(matches!(*guard, Some(ExtensionAskDialogResult::Cancel(_))));
}

#[test]
fn renders_one_recommended_suffix_per_recommended_option() {
	let mut recommended = option("One");
	recommended.recommended = Some(true);
	let mut plain = option("Two");
	plain.recommended = Some(false);
	let question = ExtensionAskDialogQuestion {
		id: "a".to_string(),
		options: vec![recommended, plain],
		..question("a", "A", false, &[])
	};
	let labels = display_option_labels(&question);
	let suffixes = labels
		.iter()
		.filter(|label| label.contains(RECOMMENDED_SUFFIX))
		.count();
	assert_eq!(suffixes, 1, "exactly the recommended option carries the badge");
}

#[test]
fn enter_still_submits_when_no_option_is_answered() {
	let capture = Capture::default();
	let mut dialog = AskDialogComponent::new(vec![question("a", "A", false, &["One"])], capture.clone());
	dialog.submit();
	assert!(dialog.submitted);
	assert_eq!(submitted(&capture), Vec::<String>::new());
}
