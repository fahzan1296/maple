//! Port of `l/test/dialogs-native.test.ts`.
//!
//! The crate's dialogs are state-driven (`handle_input`/native actions) rather
//! than the reference's keypath-routed native surface, so the port covers the
//! state-level contracts: ask submit/cancel, approval detail styling and
//! expansion, and the login sheet's manual-code submit/cancel. The keypath
//! action-bar, draft-interplay, timeout and pointer cases stay ignored.

use maple_tui::keys::Key;
use maple_tui::component::Component;
use maple_tui::native::backend::NativeUiEvent;
use maple_tui::overlays::approval_dialog::{segment_row_range, style_approval_details, ApprovalDialogState};
use maple_tui::overlays::ask_dialog::{
	AskDialogCallbacks, AskDialogComponent, ExtensionAskDialogOption, ExtensionAskDialogQuestion,
	ExtensionAskDialogResult,
};
use maple_tui::overlays::login_dialog::LoginDialogComponent;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
struct AskCallbacks {
	result: Rc<RefCell<Option<ExtensionAskDialogResult>>>,
}

impl AskDialogCallbacks for AskCallbacks {
	fn on_result(&self, result: ExtensionAskDialogResult) {
		*self.result.borrow_mut() = Some(result);
	}

	fn request_render(&self) {}
}

fn questions() -> Vec<ExtensionAskDialogQuestion> {
	vec![
		ExtensionAskDialogQuestion {
			id: "policy".to_string(),
			question: "How should frames without credits be handled?".to_string(),
			options: vec![
				ExtensionAskDialogOption { label: "Drop them".to_string(), ..Default::default() },
				ExtensionAskDialogOption { label: "Park them".to_string(), ..Default::default() },
				ExtensionAskDialogOption { label: "Block".to_string(), ..Default::default() },
			],
			recommended: Some("1".to_string()),
			..Default::default()
		},
		ExtensionAskDialogQuestion {
			id: "flag".to_string(),
			question: "Ship behind a flag?".to_string(),
			options: vec![
				ExtensionAskDialogOption { label: "Yes".to_string(), ..Default::default() },
				ExtensionAskDialogOption { label: "No".to_string(), ..Default::default() },
			],
			..Default::default()
		},
	]
}

#[test]
fn ask_enter_advances_question_by_question_and_submits_the_answers() {
	let callbacks = AskCallbacks { result: Rc::new(RefCell::new(None)) };
	let mut dialog = AskDialogComponent::new(questions(), callbacks.clone());

	dialog.handle_input(&Key::Down); // highlight "Park them"
	dialog.handle_input(&Key::Enter); // answer question 1, advance
	dialog.handle_input(&Key::Enter); // answer question 2, submit

	let result = callbacks.result.borrow_mut().take().expect("submitted");
	let ExtensionAskDialogResult::Submit(submit) = result else {
		panic!("expected submit, got {result:?}");
	};
	let answers: Vec<Vec<String>> = submit
		.answers
		.iter()
		.map(|answer| answer.selected.clone())
		.collect();
	assert_eq!(answers, vec![vec!["Park them".to_string()], vec!["Yes".to_string()]]);
}

#[test]
fn ask_escape_cancels_with_the_partial_answers() {
	let callbacks = AskCallbacks { result: Rc::new(RefCell::new(None)) };
	let mut dialog = AskDialogComponent::new(questions(), callbacks.clone());

	dialog.handle_input(&Key::Enter); // answer question 1, advance
	dialog.handle_input(&Key::Esc);

	let result = callbacks.result.borrow_mut().take().expect("cancelled");
	let ExtensionAskDialogResult::Cancel(cancel) = result else {
		panic!("expected cancel, got {result:?}");
	};
	assert_eq!(cancel.partial.answers.len(), 2);
	assert_eq!(cancel.partial.answers[0].selected, vec!["Drop them".to_string()]);
}

#[test]
fn approval_details_style_covered_and_prompted_segments_distinctly() {
	let details: Vec<String> = [
		"Command:",
		"  ✓ git status (allowed: git status*)",
		"  ? bun test (requires approval)",
	]
	.iter()
	.map(|line| line.to_string())
	.collect();

	assert_eq!(segment_row_range(&details), (1, 3));
	let styled = style_approval_details(&details);
	assert_eq!(styled.len(), 3);
	assert_eq!(styled[0].segments[0].tone, Some("accent"));
	assert_eq!(styled[1].segments[1].text, "✓");
	assert_eq!(styled[1].segments[1].tone, Some("success"));
	assert_eq!(styled[1].segments[3].tone, Some("dim"));
	assert_eq!(styled[2].segments[1].text, "?");
	assert_eq!(styled[2].segments[1].tone, Some("warning"));
	assert_eq!(styled[2].segments[3].tone, Some("accent"));
	assert!(styled[2].segments[3].bold);
}

#[test]
fn approval_expanding_swaps_the_summary_for_the_full_details() {
	let prompt_lines = vec![
		"Allow tool: bash".to_string(),
		"Command:".to_string(),
		"  ✓ git status (allowed: git status*)".to_string(),
	];
	let full_prompt = "Allow tool: bash\nCommand:\n  ✓ git status (allowed: git status*)\n  ? bun test (requires approval)";
	let mut state = ApprovalDialogState::new(
		"Allow tool: bash".to_string(),
		prompt_lines,
		Some(full_prompt.to_string()),
	);

	assert_eq!(state.visible_details().len(), 2);
	state.expanded = true;
	assert_eq!(state.visible_details().len(), 3);
	assert!(state.visible_details().last().is_some_and(|line| line.contains("bun test")));
}

#[test]
fn login_manual_input_submit_and_cancel() {
	let completed: Rc<RefCell<Option<(bool, Option<String>)>>> = Rc::new(RefCell::new(None));
	let mut dialog = {
		let completed = Rc::clone(&completed);
		LoginDialogComponent::new(
			"openai-codex",
			|| {},
			move |ok, error| *completed.borrow_mut() = Some((ok, error)),
			|_| {},
		)
	};

	let mut pending = dialog.show_manual_input("Paste the authorization code:", false);
	dialog.paste_text("code-123");
	dialog.handle_native_event(&NativeUiEvent::Action {
		key: vec!["x".to_string()],
		act: "submit".to_string(),
		value: None,
		mods: Vec::new(),
	});
	assert_eq!(pending.recv().expect("answer"), "code-123");

	let mut pending = dialog.show_manual_input("Paste the authorization code:", false);
	dialog.handle_native_event(&NativeUiEvent::Action {
		key: vec!["x".to_string()],
		act: "cancel".to_string(),
		value: None,
		mods: Vec::new(),
	});
	assert!(pending.recv().is_err());
	assert_eq!(*completed.borrow(), Some((false, Some("Login cancelled".to_string()))));
}

#[test]
#[ignore = "missing harness: native keypath action-bar routing (described keypaths, pointer actions)"]
fn ask_submit_button_answers_exactly_as_enter_does() {
}

#[test]
#[ignore = "missing harness: native keypath select/activate events through describe()"]
fn ask_tab_and_option_events_reach_the_dialog_through_keypaths() {
}

#[test]
#[ignore = "missing harness: native keypath routing + custom-answer editor"]
fn ask_next_after_a_cancelled_other_moves_to_the_review() {
}

#[test]
#[ignore = "missing harness: native surface docking (composer replacement vs modal sheet)"]
fn ask_docks_in_place_of_the_composer() {
}

#[test]
#[ignore = "missing harness: native described badge/markup for chosen answers"]
fn ask_marks_a_chosen_answer_apart_from_the_highlighted_row() {
}

#[test]
#[ignore = "missing harness: native action-bar labels and recommended suffix"]
fn ask_skip_cancels_like_escape() {
}

#[test]
#[ignore = "missing harness: hide/show key handling in the native surface"]
fn ask_hiding_never_answers() {
}

#[test]
#[ignore = "missing harness: composer-draft input guard interplay"]
fn ask_hidden_while_a_draft_owns_input() {
}

#[test]
#[ignore = "missing harness: approval arm timeout and native keypath routing"]
fn approval_enter_is_ignored_until_armed() {
}

#[test]
#[ignore = "missing harness: approval session-row navigation via native keypaths"]
fn approval_session_row_sits_between_approve_and_deny() {
}

#[test]
#[ignore = "missing harness: approval grants toggle persistence via native keypaths"]
fn approval_grants_sit_above_approve_once() {
}

#[test]
#[ignore = "missing harness: approval draft-vs-deny focus interplay"]
fn approval_draft_holds_enter_on_deny_then_returns() {
}

#[test]
#[ignore = "missing harness: approval draft editing and sending"]
fn approval_composer_draft_edits_and_sends() {
}

#[test]
#[ignore = "missing harness: approval timeout restart/expiry reporting"]
fn approval_any_key_restarts_the_timeout() {
}

#[test]
#[ignore = "missing harness: approval details expansion restarting the timeout"]
fn approval_expanding_details_restarts_the_timeout() {
}

#[test]
#[ignore = "missing harness: approval timeout expiry reporting"]
fn approval_expired_timeout_denies() {
}

#[test]
#[ignore = "missing harness: approval expand key mid-draft"]
fn approval_expand_swaps_summary_for_details_mid_draft() {
}

#[test]
#[ignore = "missing harness: native described detail spans and scroll offset"]
fn approval_styles_segments_in_terminal_and_native_views() {
}

#[test]
#[ignore = "missing harness: native surface docking for the approval sheet"]
fn approval_docks_in_place_of_the_composer() {
}

#[test]
#[ignore = "missing harness: plan review overlay keypaths and model slider"]
fn plan_review_title_heading_and_decision_bar() {
}

#[test]
#[ignore = "missing harness: plan review overlay keypaths and model slider"]
fn plan_review_arrow_keys_walk_the_decision_bar() {
}
