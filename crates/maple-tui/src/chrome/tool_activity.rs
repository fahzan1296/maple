//! Tool-activity visibility wrapper.
//!
//! Reference: `l/src/chrome/tool-activity.ts`.
//!
//! Deviations:
//! - The reference wraps arbitrary `Component` children in a `Container` and
//!   forwards `setExpanded` dynamically. This port stores render/describe
//!   callbacks and an optional expansion forwarder per child, because the
//!   crate's component trait-object/capability model differs.

use crate::component::Component;
use crate::native::describe::col;
use crate::native::node::{NativeNode, Props};

/// A component whose tool-activity chrome can be shown/hidden.
pub trait ToolActivityComponent {
	/// Toggle visibility of tool-activity rows.
	fn set_tool_activity_visible(&mut self, visible: bool);
}

/// A child participating in tool-activity composition.
pub struct ToolActivityChild {
	/// ANSI rows provider.
	pub render: Box<dyn FnMut(u16) -> Vec<String> + Send>,
	/// Native description provider.
	pub describe: Box<dyn Fn() -> NativeNode + Send + Sync>,
	/// Optional expansion forwarder.
	pub set_expanded: Option<Box<dyn FnMut(bool) + Send>>,
}

/// Container that hides wrapped tool activity while keeping it mounted.
pub struct ToolActivityContainer {
	children: Vec<ToolActivityChild>,
	visible: bool,
}

impl ToolActivityContainer {
	/// Create a container from a single child.
	pub fn new(child: ToolActivityChild) -> Self {
		ToolActivityContainer {
			children: vec![child],
			visible: true,
		}
	}

	/// Create a container from multiple children.
	pub fn from_children(children: Vec<ToolActivityChild>) -> Self {
		ToolActivityContainer {
			children,
			visible: true,
		}
	}

	/// Whether activity is currently visible.
	pub fn is_visible(&self) -> bool {
		self.visible
	}

	/// Forward expansion state to wrapped children that support it.
	pub fn set_expanded(&mut self, expanded: bool) {
		for child in &mut self.children {
			if let Some(set) = child.set_expanded.as_mut() {
				set(expanded);
			}
		}
	}

	/// Describe wrapped children; hidden activity stays mounted as a hidden column.
	pub fn describe(&self) -> NativeNode {
		let children: Vec<NativeNode> = self.children.iter().map(|c| (c.describe)()).collect();
		let mut props = Props::new();
		props.set_str("role", "omp.activity");
		if !self.visible {
			props.set_bool("hidden", true);
		}
		col(children, props)
	}
}

impl ToolActivityComponent for ToolActivityContainer {
	fn set_tool_activity_visible(&mut self, visible: bool) {
		if self.visible == visible {
			return;
		}
		self.visible = visible;
	}
}

impl Component for ToolActivityContainer {
	fn render(&mut self, width: u16) -> Vec<String> {
		if !self.visible {
			return Vec::new();
		}
		let mut out = Vec::new();
		for child in &mut self.children {
			out.extend((child.render)(width));
		}
		out
	}
}

/// Helper mirroring the reference predicate shape.
pub fn is_tool_activity_component_visible(container: &ToolActivityContainer) -> bool {
	container.is_visible()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::native::describe::{col, text};
	use crate::native::node::{NodeKind, NodeText, Props};
	use std::sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	};

	fn leaf(label: &str) -> ToolActivityChild {
		let label = label.to_string();
		let label2 = label.clone();
		ToolActivityChild {
			render: Box::new(move |_w| vec![label.clone()]),
			describe: Box::new(move || text(NodeText::Plain(label2.clone()), Props::new())),
			set_expanded: None,
		}
	}

	#[test]
	fn hides_and_shows_render_output() {
		let mut c = ToolActivityContainer::from_children(vec![leaf("a"), leaf("b")]);
		assert_eq!(c.render(10), vec!["a".to_string(), "b".to_string()]);
		c.set_tool_activity_visible(false);
		assert!(c.render(10).is_empty());
		c.set_tool_activity_visible(true);
		assert_eq!(c.render(10).len(), 2);
	}

	#[test]
	fn forwards_expansion() {
		let seen = Arc::new(AtomicBool::new(false));
		let seen2 = Arc::clone(&seen);
		let mut c = ToolActivityContainer::new(ToolActivityChild {
			render: Box::new(|_| Vec::new()),
			describe: Box::new(|| col(Vec::new(), Props::new())),
			set_expanded: Some(Box::new(move |v| {
				seen2.store(v, Ordering::SeqCst);
			})),
		});
		c.set_expanded(true);
		assert!(seen.load(Ordering::SeqCst));
	}

	#[test]
	fn describe_marks_hidden_role() {
		let mut c = ToolActivityContainer::new(leaf("x"));
		c.set_tool_activity_visible(false);
		let node = c.describe();
		assert_eq!(node.kind, NodeKind::Col);
		assert_eq!(node.prop("role").and_then(|v| v.as_str()), Some("omp.activity"));
		assert_eq!(node.prop("hidden").and_then(|v| v.as_bool()), Some(true));
		assert_eq!(node.children.len(), 1);
	}
}
