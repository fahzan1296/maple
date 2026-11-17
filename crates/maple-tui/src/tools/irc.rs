//! Peer-message types and roster constants shared by messaging, `wait`, and
//! child prompts.
//!
//! Reference: `l/src/tools/irc.ts`.
//!
//! Deviations:
//! - `ts: number` becomes `f64`, the closest Rust equivalent of a JavaScript
//!   number.
//! - `LIST_STATUS_ORDER`'s string-keyed record becomes a const slice plus
//!   [`list_status_order`], since Rust has no string-keyed record literal.

/// Maximum live peer rows embedded in child prompts.
pub const DEFAULT_PEER_ROSTER_LIMIT: usize = 32;

/// Serializable peer message retained in coordination result snapshots.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IrcMessage {
	/// Message id.
	pub id: String,
	/// Sender agent id.
	pub from: String,
	/// Recipient agent id (resolved; "all" is expanded by the tool, not stored).
	pub to: String,
	pub body: String,
	/// Timestamp.
	pub ts: f64,
	/// Message id being answered.
	pub reply_to: Option<String>,
	/// Automated wake-turn relay of a woken subagent's stop output. Relays are
	/// answers, never wake sources.
	pub wake_relay: bool,
	/// One leg of an `all` broadcast. Broadcasts never draw an automatic
	/// wake-turn reply.
	pub broadcast: bool,
	/// Interim acknowledgement or progress on something the recipient asked for.
	pub progress: bool,
}

/// Delivery outcome for one peer recipient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrcDeliveryOutcome {
	/// Injected into a live terminal.
	Injected,
	/// Woken into a new turn.
	Woken,
	/// Revived a parked agent.
	Revived,
	/// Delivery failed.
	Failed,
}

impl IrcDeliveryOutcome {
	/// The reference's wire label.
	pub fn as_str(self) -> &'static str {
		match self {
			IrcDeliveryOutcome::Injected => "injected",
			IrcDeliveryOutcome::Woken => "woken",
			IrcDeliveryOutcome::Revived => "revived",
			IrcDeliveryOutcome::Failed => "failed",
		}
	}
}

/// Delivery outcome for one peer recipient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrcDeliveryReceipt {
	/// Recipient agent id.
	pub to: String,
	/// Delivery outcome.
	pub outcome: IrcDeliveryOutcome,
	/// Failure detail.
	pub error: Option<String>,
}

/// Status ordering for peer rosters in child prompts.
pub const LIST_STATUS_ORDER: &[(&str, u8)] =
	&[("running", 0), ("idle", 1), ("parked", 2)];

/// The roster sort rank of a peer status; `None` when unknown.
pub fn list_status_order(state: &str) -> Option<u8> {
	LIST_STATUS_ORDER
		.iter()
		.find(|(name, _)| *name == state)
		.map(|(_, rank)| *rank)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roster_limit_is_thirty_two() {
		assert_eq!(DEFAULT_PEER_ROSTER_LIMIT, 32);
	}

	#[test]
	fn status_order_ranks_running_first() {
		assert_eq!(list_status_order("running"), Some(0));
		assert_eq!(list_status_order("idle"), Some(1));
		assert_eq!(list_status_order("parked"), Some(2));
		assert_eq!(list_status_order("unknown"), None);
	}

	#[test]
	fn message_flags_default_off() {
		let message = IrcMessage::default();
		assert!(!message.wake_relay);
		assert!(!message.broadcast);
		assert!(!message.progress);
		assert_eq!(message.reply_to, None);
	}

	#[test]
	fn receipt_outcome_labels() {
		assert_eq!(IrcDeliveryOutcome::Injected.as_str(), "injected");
		assert_eq!(IrcDeliveryOutcome::Woken.as_str(), "woken");
		assert_eq!(IrcDeliveryOutcome::Revived.as_str(), "revived");
		assert_eq!(IrcDeliveryOutcome::Failed.as_str(), "failed");
	}
}
