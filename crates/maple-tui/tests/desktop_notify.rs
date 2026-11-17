//! Port of `l/test/desktop-notify.test.ts`.
//!
//! The crate's notifier is a thin `notify-send`/`terminal-notifier` spawner
//! with a bell fallback; the reference's session detection, delivery policy,
//! PATH resolution, argv building and spawn/unref behaviour have no crate
//! equivalent, so those stay ignored.

use maple_tui::desktop_notify::DesktopNotifier;

#[test]
fn bell_writes_the_terminal_bell_byte() {
	let notifier = DesktopNotifier::new();
	let mut out: Vec<u8> = Vec::new();
	notifier.bell(&mut out);
	assert_eq!(out, b"\x07");
}

#[test]
fn constructing_and_notifying_without_a_daemon_is_a_silent_no_op() {
	let notifier = DesktopNotifier::new();
	notifier.notify("title", "body");
}

#[test]
#[ignore = "missing API: has_linux_desktop_session (Linux session-bus detection)"]
fn requires_linux_and_a_session_bus_address() {
}

#[test]
#[ignore = "missing API: should_deliver_desktop_notification (VTE fallback policy + PI_NO_DESKTOP_NOTIFY)"]
fn delivery_policy_for_vte_fallbacks_and_opt_out() {
}

#[test]
#[ignore = "missing API: resolve_desktop_notifier / reset_desktop_notifier_cache (PATH probe + cache)"]
fn resolves_and_caches_the_notifier_binary() {
}

#[test]
#[ignore = "missing API: build_desktop_notify_command (notify-send/gdbus argv construction)"]
fn builds_the_notifier_argv() {
}

#[test]
#[ignore = "missing API: send_desktop_notification (spawn with stdio ignore + unref)"]
fn spawns_the_notifier_without_blocking_exit() {
}
