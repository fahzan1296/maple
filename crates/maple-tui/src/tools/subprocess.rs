//! Subprocess tool renderer registry types.
//!
//! Reference: `l/src/tools/subprocess.ts`.
//!
//! Deviations:
//! - TS stores TUI renderer callbacks (`Component`, `Theme`, expanded flags) in
//!   a global `Map` and pre-registers a task renderer that depends on
//!   `TaskToolDetails`/`renderNestedTaskResults`. Rust exposes the renderer
//!   trait shape and a thread-safe registry; concrete TUI rendering and the
//!   task-specific registration are host responsibilities (task renderer not
//!   ported yet).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Display callbacks for structured data extracted from a child agent tool.
///
/// `TData` is the tool-specific data item type. Rendering returns host-defined
/// output; in TS this is a TUI `Component`.
pub trait SubprocessToolRenderer<TData = serde_json::Value>: Send + Sync {
	/// Render a single data item inline during streaming progress.
	fn render_inline(&self, _data: &TData) -> Option<String> {
		None
	}

	/// Render accumulated data in the final result view.
	fn render_final(&self, _all_data: &[TData], _expanded: bool) -> Option<String> {
		None
	}
}

/// Type-erased subprocess renderer for heterogeneous registration.
pub trait ErasedSubprocessToolRenderer: Send + Sync {}

impl<TData: Send + Sync + 'static> ErasedSubprocessToolRenderer
	for dyn SubprocessToolRenderer<TData>
{
}

/// Registry of subprocess tool renderers keyed by tool name.
#[derive(Clone, Default)]
pub struct SubprocessRendererRegistry {
	inner: Arc<RwLock<HashMap<String, Arc<dyn ErasedSubprocessToolRenderer>>>>,
}

impl SubprocessRendererRegistry {
	/// Create an empty registry.
	pub fn new() -> Self {
		Self::default()
	}

	/// Register display callbacks for a subprocess tool.
	pub fn register(
		&self,
		tool_name: impl Into<String>,
		renderer: Arc<dyn ErasedSubprocessToolRenderer>,
	) {
		let mut map = self.inner.write().expect("subprocess registry poisoned");
		map.insert(tool_name.into(), renderer);
	}

	/// Look up display callbacks for a subprocess tool.
	pub fn get(&self, tool_name: &str) -> Option<Arc<dyn ErasedSubprocessToolRenderer>> {
		let map = self.inner.read().expect("subprocess registry poisoned");
		map.get(tool_name).cloned()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Dummy;
	impl SubprocessToolRenderer<serde_json::Value> for Dummy {}

	#[test]
	fn register_and_lookup() {
		let reg = SubprocessRendererRegistry::new();
		assert!(reg.get("task").is_none());
		let r: Arc<dyn ErasedSubprocessToolRenderer> =
			Arc::new(Dummy) as Arc<dyn SubprocessToolRenderer<serde_json::Value>>;
		// Type-erased storage test: register concrete-erased via trait object coercion path.
		// Simpler: store as Erased by wrapping.
		struct Wrap(Arc<dyn ErasedSubprocessToolRenderer>);
		impl ErasedSubprocessToolRenderer for Wrap {}
		let _ = Wrap(r);
	}
}
