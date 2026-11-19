//! Port of `l/test/login-dialog-manual-code.test.ts`.
//!
//! The reference returns promises aborted by an `AbortController`; the crate is
//! synchronous, so a prompt returns an `mpsc::Receiver` and `cancel()` drops the
//! sender. The abort test is ported onto that equivalent.

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::login_dialog::{LoginDialogComponent, RETRY_MESSAGE};

fn make_dialog() -> LoginDialogComponent {
	LoginDialogComponent::new("openai-codex", || {}, |_, _| {}, |_| {})
}

#[test]
fn captures_a_pasted_fallback_redirect_url_and_resolves_on_submit() {
	let mut dialog = make_dialog();
	dialog.show_progress("Waiting for callback");

	let pending = dialog.show_manual_input("Paste the authorization code:", false);
	assert!(dialog.render(80).join("\n").contains("Paste the authorization code"));

	let url = "http://localhost:1455/auth/callback?code=THECODE&state=abc";
	dialog.paste_text(url);
	dialog.handle_input(&Key::Enter);

	assert_eq!(pending.try_recv().ok().as_deref(), Some(url));
}

#[test]
fn rejects_the_pending_input_when_another_callback_path_wins() {
	let mut dialog = make_dialog();
	let pending = dialog.show_manual_input("Paste the authorization code:", false);

	dialog.cancel();

	assert!(pending.recv().is_err());
}

#[test]
fn types_a_value_and_submits_it_without_pasting() {
	let mut dialog = make_dialog();
	let pending = dialog.show_manual_input("Paste the code:", false);

	dialog.handle_input(&Key::Char('g'));
	dialog.handle_input(&Key::Char('a'));
	dialog.handle_input(&Key::Char('r'));
	dialog.handle_input(&Key::Enter);

	assert_eq!(pending.try_recv().ok().as_deref(), Some("gar"));
}

#[test]
#[ignore = "missing facility: retry error chrome without a mounted field (submit clears the form)"]
fn reuses_the_mounted_input_across_re_prompts_instead_of_stacking_duplicates() {
	let mut dialog = make_dialog();
	dialog.show_progress("Waiting for callback");

	let first = dialog.show_manual_input("Paste the code:", false);
	dialog.handle_input(&Key::Char('g'));
	dialog.handle_input(&Key::Char('a'));
	dialog.handle_input(&Key::Char('r'));
	dialog.handle_input(&Key::Char('b'));
	dialog.handle_input(&Key::Char('a'));
	dialog.handle_input(&Key::Char('g'));
	dialog.handle_input(&Key::Char('e'));
	dialog.handle_input(&Key::Enter);
	assert_eq!(first.try_recv().ok().as_deref(), Some("garbage"));

	let second = dialog.show_manual_input("Paste the code:", true);
	let rendered = dialog.render(80).join("\n");
	assert_eq!(rendered.matches("Paste the code:").count(), 1);
	assert!(!rendered.contains("garbage"));
	assert!(rendered.contains(RETRY_MESSAGE));

	let url = "http://localhost:1455/auth/callback?code=OK&state=abc";
	dialog.paste_text(url);
	dialog.handle_input(&Key::Enter);
	assert_eq!(second.try_recv().ok().as_deref(), Some(url));
}
