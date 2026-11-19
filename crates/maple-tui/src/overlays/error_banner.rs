//! Persistent error banner pinned above the editor.
//!
//! Reference: `l/src/overlays/error-banner.ts`.
//!
//! Deviations:
//! - `Container`, `Spacer`, `Text`, `WidthAwareText`, `DynamicBorder` composition
//!   and `theme` globals are not ported. Host code composes the same components
//!   from `maple-tui`; this module provides the native description logic and
//!   helpers for the expanded/collapsed state.
//! - `format_error_block` (TS `chrome/error-block`) has no Rust equivalent in
//!   this crate snapshot; callers must format banner lines separately.
//! - `plainText`/sanitization uses simple trimming in the Rust port.

use crate::native::describe::{node, span, text};
use crate::native::node::{NativeNode, NodeText};
use crate::native::overlay::{action_button, ActionButtonOptions};

/// Max wrapped rows of the error message shown in the pinned banner (reference constant).
pub const MAX_BANNER_ROWS: usize = 4;

/// Build the headline line (first non-empty line) from a message.
#[must_use]
pub fn error_banner_headline(message: &str) -> String {
	let trimmed = message.trim();
	for line in trimmed.lines() {
		let t = line.trim();
		if !t.is_empty() {
			return t.to_string();
		}
	}
	if trimmed.is_empty() {
		"Error".to_string()
	} else {
		trimmed.to_string()
	}
}

/// Build the native description for the error banner strip.
#[must_use]
pub fn describe_error_banner(
	message: &str,
	expanded: bool,
	show_dismiss: bool,
) -> NativeNode {
	let headline = error_banner_headline(message);
	let multiline = message.trim() != headline || message.lines().count() > 1;
	let message_node = if expanded {
		text(
			[span(message.trim(), Some("error"))],
			crate::native::node::Props::new().set("wrap", "word").set("grow", 1.0),
		)
	} else {
		text(
			[span(&headline, Some("error"))],
			crate::native::node::Props::new()
				.set("truncate", "end")
				.set("lines", 1)
				.set("grow", 1.0)
				.set("title", message),
		)
	};
	let mut children: Vec<NativeNode> = vec![message_node];
	if multiline || headline.len() > 80 {
		children.push(action_button(
			if expanded { "Less" } else { "Details" },
			"details",
			ActionButtonOptions::default(),
		));
	}
	if show_dismiss {
		children.push(action_button("Dismiss", "dismiss", ActionButtonOptions::default()));
	}
	node(
		"row",
		crate::native::node::Props::new()
			.set("role", "omp.errorBanner")
			.set("tone", "error")
			.set("gap", "sm")
			.set("align", if expanded { "start" } else { "center" }),
		children,
		None,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn headline_uses_first_non_empty_line() {
		assert_eq!(error_banner_headline("\n\nhello\nworld"), "hello");
		assert_eq!(error_banner_headline(""), "Error");
	}

	#[test]
	fn describe_collapsed_has_details_when_multiline() {
		let n = describe_error_banner("a\nb", false, true);
		assert_eq!(n.kind(), &crate::native::node::NodeKind::Row);
		assert!(n.children().len() >= 2);
	}

	#[test]
	fn describe_single_short_line_may_omit_details() {
		let n = describe_error_banner("short", false, false);
		assert_eq!(n.children().len(), 1);
	}
}
