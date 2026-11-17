//! Multiplexer notification routing (port of `l/src/terminal-multiplexer.ts`).
//!
//! Reference: `l/src/terminal-multiplexer.ts`.
//!
//! Session classification lives in [`crate::terminal_capabilities`] and the
//! per-host notifiers live in [`crate::multiplexers`]. This module is the
//! composition point the reference file adds on top of them: offer a
//! notification to every active session's notifier, innermost tier first.

use std::io::IsTerminal;

use crate::multiplexers::{Multiplexer, NotificationDeliveryTier, NotificationRequest};
use crate::terminal_capabilities::{terminal_multiplexer_sessions, EnvReader, MultiplexerSession};

/// Adapter so a [`NotificationRequest`] environment closure can drive the
/// [`EnvReader`-based session classification.
struct FnEnv<'a>(&'a dyn Fn(&str) -> Option<String>);

impl EnvReader for FnEnv<'_> {
	fn get(&self, key: &str) -> Option<String> {
		(self.0)(key)
	}

	fn is_tty(&self) -> bool {
		std::io::stdout().is_terminal()
	}
}

/// Delivery order: pane, then surface, then in-band.
fn tier_rank(tier: NotificationDeliveryTier) -> u8 {
	match tier {
		NotificationDeliveryTier::Pane => 0,
		NotificationDeliveryTier::Surface => 1,
		NotificationDeliveryTier::InBand => 2,
	}
}

fn multiplexer_for_session(session: MultiplexerSession) -> Multiplexer {
	match session {
		MultiplexerSession::Herdr => Multiplexer::Herdr,
		MultiplexerSession::Tmux => Multiplexer::Tmux,
		MultiplexerSession::Screen => Multiplexer::Screen,
		MultiplexerSession::Zellij => Multiplexer::Zellij,
		MultiplexerSession::Cmux => Multiplexer::Cmux,
		MultiplexerSession::Wmux => Multiplexer::Wmux,
		MultiplexerSession::Orca => Multiplexer::Orca,
	}
}

/// Offer `request` to every active session's notifier, innermost tier first.
/// Returns whether one delivered it; otherwise the terminal fallback applies
/// unchanged.
pub fn route_terminal_multiplexer_notification(request: &mut NotificationRequest<'_>) -> bool {
	let mut notifiers: Vec<(NotificationDeliveryTier, Multiplexer)> = {
		let env = FnEnv(request.env);
		terminal_multiplexer_sessions(&env)
			.into_iter()
			.map(multiplexer_for_session)
			.filter_map(|multiplexer| {
				multiplexer
					.notification_tier()
					.map(|tier| (tier, multiplexer))
			})
			.collect()
	};
	// Stable sort: registry order breaks ties within a tier.
	notifiers.sort_by_key(|(tier, _)| tier_rank(*tier));
	notifiers
		.iter()
		.any(|(_, multiplexer)| multiplexer.send_notification(request))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::multiplexers::NotificationMessage;

	fn lookup(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + '_ {
		move |key| {
			values
				.iter()
				.find(|(name, _)| *name == key)
				.map(|(_, value)| value.to_string())
		}
	}

	fn drive(get: &dyn Fn(&str) -> Option<String>, written: &mut String) -> bool {
		let mut write = |data: &str| written.push_str(data);
		let mut request = NotificationRequest::new(
			NotificationMessage::Text("hi"),
			get,
			Some("\x1b]9;hi\x1b\\"),
			&mut write,
		);
		route_terminal_multiplexer_notification(&mut request)
	}

	#[test]
	fn no_active_session_leaves_the_terminal_fallback() {
		let get = lookup(&[]);
		let mut written = String::new();
		assert!(!drive(&get, &mut written));
		assert!(written.is_empty());
	}

	#[test]
	fn tmux_delivers_in_band_through_the_passthrough_envelope() {
		let get = lookup(&[("TMUX", "/tmp/tmux-0")]);
		let mut written = String::new();
		assert!(drive(&get, &mut written));
		assert_eq!(written, "\x1bPtmux;\x1b\x1b]9;hi\x1b\x1b\\\x1b\\\x07");
	}

	#[test]
	fn tiers_are_tried_innermost_first() {
		assert!(tier_rank(NotificationDeliveryTier::Pane) < tier_rank(NotificationDeliveryTier::Surface));
		assert!(tier_rank(NotificationDeliveryTier::Surface) < tier_rank(NotificationDeliveryTier::InBand));
	}
}
