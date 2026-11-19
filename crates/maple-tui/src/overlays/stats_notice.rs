//! Stats notice overlay helper.
//!
//! Reference: `l/src/overlays/stats-notice.ts`.
//!
//! Deviations:
//! - `Spacer`, `Text`, `Container`, `theme` globals, and `TUI` container APIs
//!   are not ported. This module provides the native description builder and
//!   the message/url data used by callers to construct a notice UI.
//! - `actionButton` with `href` is produced via [`crate::native::overlay::action_button`]
//!   options; the `href`/link behavior is terminal-driven by the node props.

use crate::native::describe::{node, span};
use crate::native::node::NativeNode;
use crate::native::overlay::{action_button, ActionButtonOptions};

/// Build the native row for the stats notice: message text plus an open link.
#[must_use]
pub fn describe_stats_notice(message: &str, url: &str) -> NativeNode {
	node(
		"row",
		crate::native::node::Props::new()
			.set("gap", "md")
			.set("align", "center")
			.set("wrap", true),
		vec![
			node(
				"text",
				crate::native::node::Props::new()
					.set("role", "omp.stats")
					.set("wrap", "word")
					.set("grow", 1.0)
					.set("spans", vec![serde_json::json!({"t": message})]),
				vec![],
				None,
			),
			action_button(
				"Open dashboard ↗",
				"open",
				ActionButtonOptions {
					href: Some(url.to_string()),
					title: Some(url.to_string()),
					..Default::default()
				},
			),
		],
		None,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn describe_stats_notice_builds_row_with_link_action() {
		let n = describe_stats_notice("ready", "http://localhost:1234");
		assert_eq!(n.kind(), &crate::native::node::NodeKind::Row);
		assert_eq!(n.children().len(), 2);
	}
}
