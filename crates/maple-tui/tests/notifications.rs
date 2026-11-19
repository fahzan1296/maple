//! Reference: `l/test/notifications.test.ts`.
//!
//! The reference covers OSC 99 formatting plus process-level delivery routing
//! (tmux/Zellij/cmux/herdr), the capability probe and the D-Bus gate. The crate
//! ports the formatting core and the tmux passthrough envelope; the delivery
//! singleton, probe loop and desktop gate have no equivalent, so those cases
//! are ignored with the absent facility named.

use maple_tui::terminal_capabilities::{
	format_osc99_notification, get_terminal_info, set_osc99_supported, wrap_tmux_passthrough,
	Notification, Platform, TerminalId, TerminalNotification, TestEnv,
};

fn kitty() -> maple_tui::terminal_capabilities::TerminalInfo {
	get_terminal_info(TerminalId::Kitty, Platform::Linux, &TestEnv::new())
}

#[test]
fn osc99_formatting_matches_the_reference() {
	set_osc99_supported(false);
	let terminal = kitty();

	assert_eq!(
		terminal.format_notification(Notification::Message("hello")),
		"\x1b]99;;hello\x1b\\"
	);
	assert_eq!(
		terminal.format_notification(Notification::Structured(&TerminalNotification {
			title: Some("Session".into()),
			body: Some("Complete".into()),
			..TerminalNotification::default()
		})),
		"\x1b]99;;Session: Complete\x1b\\"
	);

	set_osc99_supported(true);
	let rich = TerminalNotification {
		title: Some("Session".into()),
		body: Some("Complete".into()),
		id: Some("complete-1".into()),
		notification_type: Some(vec!["completion".into()]),
		urgency: Some(maple_tui::terminal_capabilities::NotificationUrgency::Normal),
		icon_name: Some("info".into()),
		sound: Some(maple_tui::terminal_capabilities::NotificationSound::Info),
		actions: Some(maple_tui::terminal_capabilities::NotificationActions::Focus),
		expires_ms: Some(5000.0),
	};
	assert_eq!(
		terminal.format_notification(Notification::Structured(&rich)),
		"\x1b]99;i=complete-1:f=b21w:a=focus:u=1:t=Y29tcGxldGlvbg==:n=aW5mbw==:s=aW5mbw==:w=5000:d=0;Session\x1b\\"
			.to_string()
			+ "\x1b]99;i=complete-1:p=body;Complete\x1b\\"
	);

	let unsafe_payload = TerminalNotification {
		title: Some("Line 1\nLine 2".into()),
		id: Some("unsafe".into()),
		..TerminalNotification::default()
	};
	assert_eq!(
		format_osc99_notification(&unsafe_payload),
		"\x1b]99;i=unsafe:f=b21w:e=1;TGluZSAxCkxpbmUgMg==\x1b\\"
	);
	set_osc99_supported(false);
}

#[test]
fn tmux_passthrough_doubles_escapes_and_keeps_the_sterminator() {
	assert_eq!(
		wrap_tmux_passthrough("\x1b]99;;Hello\x1b\\"),
		"\x1bPtmux;\x1b\x1b]99;;Hello\x1b\x1b\\\x1b\\"
	);
}

#[test]
#[ignore = "missing facility: no TERMINAL.sendNotification delivery singleton or cmux/herdr/tmux/Zellij routing, so notifier argv and fallback writes cannot be asserted"]
fn routes_notifications_to_the_innermost_notifier() {}

#[test]
#[ignore = "missing facility: no ProcessTerminal OSC 99 capability probe loop (no probe write/reply parsing), so probe suppression and confirmation cannot be asserted"]
fn probes_and_confirms_osc99_support() {}

#[test]
#[ignore = "missing facility: desktop_notify has no should_deliver gate or TERMINAL fan-out, so D-Bus dispatch gating cannot be asserted"]
fn gates_desktop_dbus_notifications() {}
