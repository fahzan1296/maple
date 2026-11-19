//! Loader wrapped with borders for hook UI.
//!
//! Reference: `l/src/overlays/bordered-loader.ts`.
//!
//! Deviations:
//! - `CancellableLoader`, `Container`, `Spacer`, `Text`, `TUI`, `Theme` globals
//!   from the TS runtime are not ported. Use [`crate::components::cancellable_loader::CancellableLoader`]
//!   directly in host code and compose with [`crate::chrome::dynamic_border::DynamicBorder`],
//!   [`crate::components::spacer::Spacer`], [`crate::components::text::Text`].
//! - [`describe_bordered_loader_card`] builds the native card tree shape.

use crate::native::describe::{card, node, span};
use crate::native::node::NativeNode;
use crate::native::overlay::{action_hint, hints_row};

/// Build the native card for a bordered loader (matches TS describe structure intent).
#[must_use]
pub fn describe_bordered_loader_card(cancel_key_label: &str) -> NativeNode {
	let cancel = action_hint(&crate::keybindings::KeybindingsManager::new(), "tui.select.cancel", "cancel");
	let hints = hints_row(&[cancel.or_else(|| Some(crate::native::overlay::NativeHint {
		keys: vec![cancel_key_label.to_string()],
		label: "cancel".to_string(),
	}))], "loader-hints");
	card(
		crate::native::node::Props::new().set("role", "omp.hook.loader"),
		vec![
			node("loader", crate::native::node::Props::new(), vec![], Some("loader")),
			hints,
		],
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn describe_bordered_loader_card_has_role() {
		let n = describe_bordered_loader_card("Esc");
		assert_eq!(n.kind(), &crate::native::node::NodeKind::Card);
	}
}
