//! Reference: `l/src/prompt/editor-top-gap.ts`.
//!
//! One-line top margin between the working/status HUD row and the editor. The
//! band composer's status band is designed to sit flush under the working row,
//! so the gap collapses there — but only while that row actually rendered
//! content (the loader and idle title bring their own leading blank). An empty
//! status row keeps the gap so the band never sits flush against the transcript.
//!
//! Deviation: the TypeScript keeps the composer shape in a module-level
//! variable set by [`set_editor_gap_composer_shape`]. Rust has no module-scope
//! mutable state, so it lives in a `Mutex`; the setter and render-time read are
//! otherwise identical, including runtime changes applying immediately.

use std::sync::{LazyLock, Mutex};

use crate::component::Component;
use crate::native::describe::{col, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, Props};

/// Process-wide composer shape (`"box"` or `"band"`).
static COMPOSER_SHAPE: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new("box".to_string()));

/// Set the process-wide editor gap layout.
pub fn set_editor_gap_composer_shape(shape: impl Into<String>) {
	let mut guard = COMPOSER_SHAPE
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner());
	*guard = shape.into();
}

fn current_composer_shape() -> String {
	COMPOSER_SHAPE
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.clone()
}

const GAP: [&str; 1] = [""];
const FLUSH: [&str; 0] = [];

/// `EditorTopGap` — the gap above the editor, honouring the band composer.
pub struct EditorTopGap {
	status_row_occupied: Box<dyn Fn() -> bool + Send + Sync>,
}

impl EditorTopGap {
	/// `status_row_occupied` reports whether the status/working row directly
	/// above rendered lines this frame.
	pub fn new(status_row_occupied: impl Fn() -> bool + Send + Sync + 'static) -> Self {
		Self {
			status_row_occupied: Box::new(status_row_occupied),
		}
	}

	/// Whether the gap collapses this frame.
	pub fn is_flush(&self) -> bool {
		current_composer_shape() == "band" && (self.status_row_occupied)()
	}
}

impl Component for EditorTopGap {
	fn render(&mut self, _width: u16) -> Vec<String> {
		if self.is_flush() {
			FLUSH.iter().map(|s| (*s).to_string()).collect()
		} else {
			GAP.iter().map(|s| (*s).to_string()).collect()
		}
	}
}

impl NativeDescribe for EditorTopGap {
	/// A TSP terminal spaces the dock itself; the gap describes nothing.
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		col(Vec::new(), Props::default())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn collapses_only_for_occupied_band() {
		let cases: [(&str, bool, usize); 4] = [
			("box", true, 1),
			("box", false, 1),
			("band", true, 0),
			("band", false, 1),
		];
		for (shape, occupied, expected_rows) in cases {
			set_editor_gap_composer_shape(shape);
			let mut gap = EditorTopGap::new(move || occupied);
			assert_eq!(gap.render(80).len(), expected_rows, "shape {shape} occupied {occupied}");
		}
		set_editor_gap_composer_shape("box");
	}
}
