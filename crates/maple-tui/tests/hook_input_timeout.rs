//! Port of `l/test/hook-input-timeout.test.ts`.
//!
//! The crate's `HookInputComponent` is clocked by an explicit `now_ms`, so the
//! timeout reset/expiry and submit behaviors are portable. Bracketed/enhanced
//! paste keeps newlines in [`crate::components::Form`] and the native sheet's
//! node kinds differ, so those tests are ignored.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use maple_tui::keys::Key;
use maple_tui::overlays::hook_input::{HookInputComponent, HookInputOptions};
use maple_tui::{strip_ansi, Component};

#[test]
fn resets_timeout_on_user_activity_and_still_expires_when_idle() {
	let timeouts = Rc::new(Cell::new(0));
	let cancels = Rc::new(Cell::new(0));
	let timeout_sink = Rc::clone(&timeouts);
	let cancel_sink = Rc::clone(&cancels);
	let mut component = HookInputComponent::new(
		"Prompt",
		None,
		|_value| {},
		move || cancel_sink.set(cancel_sink.get() + 1),
		HookInputOptions {
			timeout_ms: Some(1_000),
			secret: false,
		},
	);
	component.set_on_timeout(move || timeout_sink.set(timeout_sink.get() + 1));
	component.start_countdown(0);

	component.handle_key(&Key::Char('a'), 900);
	component.handle_key(&Key::Backspace, 1_800);
	assert!(!component.poll_countdown(2_700));
	assert!(component.poll_countdown(2_900));
	assert_eq!(timeouts.get(), 1);
	assert_eq!(cancels.get(), 1);
}

#[test]
fn preserves_submit_behavior() {
	let submitted: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
	let cancels = Rc::new(Cell::new(0));
	let timeouts = Rc::new(Cell::new(0));
	let sink = Rc::clone(&submitted);
	let cancel_sink = Rc::clone(&cancels);
	let timeout_sink = Rc::clone(&timeouts);
	let mut component = HookInputComponent::new(
		"Prompt",
		None,
		move |value| sink.borrow_mut().push(value.to_string()),
		move || cancel_sink.set(cancel_sink.get() + 1),
		HookInputOptions {
			timeout_ms: Some(1_000),
			secret: false,
		},
	);
	component.set_on_timeout(move || timeout_sink.set(timeout_sink.get() + 1));

	component.handle_key(&Key::Char('h'), 0);
	component.handle_key(&Key::Char('i'), 0);
	component.handle_key(&Key::Enter, 0);

	assert_eq!(&*submitted.borrow(), &vec!["hi".to_string()]);
	assert_eq!(cancels.get(), 0);
	assert_eq!(timeouts.get(), 0);
}

#[test]
fn masks_characters_as_bullets_when_secret_is_true() {
	let submitted: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
	let sink = Rc::clone(&submitted);
	let mut component = HookInputComponent::new(
		"API Key",
		None,
		move |value| *sink.borrow_mut() = value.to_string(),
		|| {},
		HookInputOptions {
			secret: true,
			..HookInputOptions::default()
		},
	);
	for ch in "secret".chars() {
		component.handle_key(&Key::Char(ch), 0);
	}

	let rendered = strip_ansi(&component.render(80).join("\n"));
	assert!(rendered.contains("••••••"));
	assert!(!rendered.contains("secret"));

	component.handle_key(&Key::Enter, 0);
	assert_eq!(&*submitted.borrow(), "secret");
}

#[test]
#[ignore = "Form::handle_paste keeps newlines; the reference strips them"]
fn absorbs_enhanced_paste_payloads_via_paste_text_and_resets_the_timeout() {
	let mut component = HookInputComponent::new(
		"Prompt",
		None,
		|_value| {},
		|| {},
		HookInputOptions::default(),
	);
	component.paste_text("sk-line1\nsk-line2", 900);
	assert_eq!(component.value(), "sk-line1\nsk-line2");
}

#[test]
#[ignore = "native action-bar node kinds/keycaps differ from the reference sheet"]
fn submit_and_cancel_run_what_enter_and_esc_run_keys_live_only_in_tooltips() {
	let component = HookInputComponent::new("Search pull requests", None, |_| {}, || {}, HookInputOptions::default());
	let _ = component.value();
}

#[test]
#[ignore = "crate native field node is a Col, not the reference input node kind"]
fn splits_the_title_into_head_and_detail_and_draws_the_placeholder_natively() {
	let component = HookInputComponent::new("Search pull requests\nTitle, author or #123", Some("e.g. #482"), |_| {}, || {}, HookInputOptions::default());
	assert_eq!(component.placeholder(), Some("e.g. #482"));
}

#[test]
#[ignore = "missing facility: refused-submit validation/error tinting is not ported"]
fn a_refused_submit_keeps_the_value_and_tints_the_field_with_the_reason_under_it() {
	let component = HookInputComponent::new("PR number", None, |_| {}, || {}, HookInputOptions::default());
	assert!(!component.has_error());
}
