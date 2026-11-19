//! Shared streaming-overlay body/footer shell.
//!
//! Reference: `l/src/chrome/streaming-panel.ts`.
//!
//! Deviations:
//! - The reference composes `Container`/`Spacer`/`Text` component children and
//!   stores arbitrary section components. This port models sections and the
//!   footer as ANSI row providers and rebuilds a native description snapshot
//!   from them, because the crate's component composition differs.
//! - The footer can be static or dynamic (re-read on render/describe/refresh),
//!   matching the reference behavior.
//! - Section spacing is one blank row between sections.

use crate::component::Component;
use crate::native::describe::{col, text};
use crate::native::node::{NativeNode, NodeText, Props, Span};

/// One refresh of a streaming overlay's presentation.
pub struct StreamingPanelPresentation {
	/// Non-empty sections; each section is one or more lines.
	pub sections: Vec<Vec<String>>,
	/// Footer line, static or dynamic.
	pub footer: StreamingPanelFooter,
}

/// Static or dynamic footer text.
pub enum StreamingPanelFooter {
	/// A fixed footer line.
	Static(String),
	/// A footer line re-read on each refresh.
	Dynamic(Box<dyn Fn() -> String + Send + Sync>),
}

impl StreamingPanelFooter {
	fn line(&self) -> String {
		match self {
			StreamingPanelFooter::Static(s) => s.clone(),
			StreamingPanelFooter::Dynamic(f) => f(),
		}
	}
}

/// Shared body/footer shell for streaming overlays.
///
/// The overlay controller keeps its own state machine and calls [`Self::refresh`]
/// after each transition.
pub struct StreamingPanelContent {
	presentation: Box<dyn Fn() -> StreamingPanelPresentation + Send + Sync>,
	render_rows: Vec<String>,
	native: NativeNode,
}

impl StreamingPanelContent {
	/// Create a shell driven by `presentation`.
	pub fn new(
		presentation: impl Fn() -> StreamingPanelPresentation + Send + Sync + 'static,
	) -> Self {
		let mut this = StreamingPanelContent {
			presentation: Box::new(presentation),
			render_rows: Vec::new(),
			native: col(Vec::new(), Props::new()),
		};
		this.refresh();
		this
	}

	/// Rebuild sections and the footer from controller-owned state.
	pub fn refresh(&mut self) {
		let presentation = (self.presentation)();
		let mut rows: Vec<String> = vec![String::new()];
		let mut native_sections: Vec<NativeNode> = Vec::new();
		for section in &presentation.sections {
			if section.is_empty() {
				continue;
			}
			for line in section {
				rows.push(line.clone());
			}
			rows.push(String::new());
			let mut props = Props::new();
			props.set_str("wrap", "word");
			native_sections.push(text(NodeText::Plain(section.join("\n")), props));
		}
		let footer_line = presentation.footer.line();
		rows.push(footer_line.clone());
		let mut footer_props = Props::new();
		footer_props.set_str("wrap", "word");
		footer_props.set_str("role", "omp.panel.footer");
		native_sections.push(text(
			NodeText::Spans(vec![Span::styled(footer_line, "muted")]),
			footer_props,
		));
		self.render_rows = rows;
		let mut root_props = Props::new();
		root_props.set_str("gap", "sm");
		self.native = col(native_sections, root_props);
	}

	/// The sections as a spaced stack followed by the footer.
	pub fn describe(&self) -> NativeNode {
		self.native.clone()
	}
}

impl Component for StreamingPanelContent {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.refresh();
		self.render_rows.clone()
	}

	fn invalidate(&mut self) {
		self.refresh();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	};

	#[test]
	fn renders_sections_spacers_and_footer() {
		let mut panel = StreamingPanelContent::new(|| StreamingPanelPresentation {
			sections: vec![vec!["A".to_string()], vec!["B".to_string(), "C".to_string()]],
			footer: StreamingPanelFooter::Static("footer".to_string()),
		});
		let rows = panel.render(40);
		assert_eq!(rows[0], "");
		assert!(rows.contains(&"A".to_string()));
		assert!(rows.contains(&"B".to_string()));
		assert!(rows.contains(&"C".to_string()));
		assert_eq!(rows.last().cloned(), Some("footer".to_string()));
	}

	#[test]
	fn dynamic_footer_rerenders() {
		let n = Arc::new(AtomicUsize::new(0));
		let n2 = Arc::clone(&n);
		let mut panel = StreamingPanelContent::new(move || StreamingPanelPresentation {
			sections: Vec::new(),
			footer: StreamingPanelFooter::Dynamic(Box::new({
				let n2 = Arc::clone(&n2);
				move || format!("v{}", n2.load(Ordering::SeqCst))
			})),
		});
		assert_eq!(panel.render(20).last().cloned(), Some("v0".to_string()));
		n.store(3, Ordering::SeqCst);
		assert_eq!(panel.render(20).last().cloned(), Some("v3".to_string()));
	}

	#[test]
	fn describe_returns_column_with_footer() {
		let panel = StreamingPanelContent::new(|| StreamingPanelPresentation {
			sections: vec![vec!["one".to_string()]],
			footer: StreamingPanelFooter::Static("end".to_string()),
		});
		let node = panel.describe();
		assert_eq!(node.kind, crate::native::node::NodeKind::Col);
		assert_eq!(node.children.len(), 2);
	}
}
