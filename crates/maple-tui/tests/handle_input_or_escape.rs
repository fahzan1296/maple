//! Port of `l/test/handle-input-or-escape.test.ts`.
//!
//! The crate's `handle_input_or_escape` takes an already-decoded `Key`, so the
//! legacy bare-Escape and printable-forwarding cases port directly. The kitty
//! CSI-u case needs the global keyboard-protocol flag and string decoding, which
//! the crate does not expose.

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::plugin_settings::handle_input_or_escape;

#[derive(Default)]
struct Capture {
	keys: Vec<Key>,
}

impl Component for Capture {
	fn handle_input(&mut self, key: &Key) {
		self.keys.push(key.clone());
	}
	fn render(&mut self, _width: u16) -> Vec<String> {
		Vec::new()
	}
}

fn run(key: &Key) -> (bool, Vec<Key>) {
	let mut input = Capture::default();
	let mut cancelled = false;
	handle_input_or_escape(key, &mut input, &mut || cancelled = true);
	(cancelled, input.keys)
}

#[test]
fn cancels_on_a_legacy_bare_escape_without_forwarding() {
	let (cancelled, forwarded) = run(&Key::Esc);
	assert!(cancelled);
	assert!(forwarded.is_empty());
}

#[test]
fn forwards_a_printable_keystroke_to_the_input() {
	let (cancelled, forwarded) = run(&Key::Char('g'));
	assert!(!cancelled);
	assert_eq!(forwarded, vec![Key::Char('g')]);
}

#[test]
#[ignore = "missing API: global kitty-protocol-active flag and CSI-u string decoding (Escape arrives as Key::Esc)"]
fn cancels_on_a_kitty_csi_u_escape() {
}
