//! Composer-shape preview rendering helpers.
//!
//! Reference: `l/src/overlays/composer-shape-preview.ts`.
//!
//! Deviations:
//! - TUI `Component`, theme/composer chrome rendering (`getComposerStyle`,
//!   `ComposerChromeContext`, ANSI coloring, padding/truncation primitives),
//!   and native describe nodes are unavailable. This module ports the preview
//!   algorithm as plain-text/layout helpers over injected style/status traits.
//! - `ComposerShapePreview` UI component is represented as `ComposerShapePreviewState`
//!   holding current shape and producing render/description models.

pub const PREVIEW_TITLE: &str = "omp";

/// Border payload from status source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewTopBorder {
	pub content: String,
	pub width: usize,
}

/// Real status renderer the preview borrows rows from.
pub trait ComposerPreviewStatusSource {
	fn get_top_border(&self, width: usize, preview_title: Option<&str>) -> PreviewTopBorder;
	fn get_band_top_border(&self, width: usize, preview_title: Option<&str>) -> PreviewTopBorder;
	fn get_standalone_top_border(&self, width: usize, preview_title: Option<&str>) -> PreviewTopBorder;
	fn render_bottom_bar(&self, width: usize, groups: &str, preview_title: Option<&str>) -> String;
}

/// Composer shape option metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposerShapeOption {
	pub value: String,
	pub label: String,
	pub description: Option<String>,
}

/// Minimal composer style contract needed for preview layout.
pub trait ComposerStyleLike {
	fn default_padding_x(&self) -> usize;
	fn side_chrome_width(&self, padding_x: usize) -> usize;
	/// "top-border" | "top-band" | "top-rule-chip" | other
	fn status_attachment(&self) -> &str;
	fn default_prompt_gutter(&self) -> &str;
	fn render_top(&self, width: usize, top_border: Option<&str>) -> Option<String>;
	fn render_row(&self, text: &str, pad: &str, gutter: &str, width: usize) -> Vec<String>;
	fn render_bottom(&self, width: usize) -> Option<String>;
	/// "none" | "left" | "full"
	fn bottom_bar(&self) -> &str;
	fn bottom_bar_gap(&self) -> bool;
	fn is_filled(&self) -> bool;
}

/// Provider for style/options by shape.
pub trait ComposerShapeRegistryLike {
	fn style_for(&self, shape: &str) -> Option<Box<dyn ComposerStyleLike + '_>>;
	fn options(&self) -> Vec<ComposerShapeOption>;
}

fn visible_width_approx(s: &str) -> usize {
	s.chars().count()
}

fn truncate_to_width(s: &str, width: usize) -> String {
	if visible_width_approx(s) <= width {
		return s.to_string();
	}
	if width == 0 {
		return String::new();
	}
	s.chars().take(width).collect()
}

fn padding_spaces(n: usize) -> String {
	" ".repeat(n)
}

/// Render composer shape preview lines.
#[must_use]
pub fn render_composer_shape_preview<S: ComposerPreviewStatusSource + ?Sized>(
	shape: &str,
	width: usize,
	status: Option<&S>,
	registry: &dyn ComposerShapeRegistryLike,
) -> Vec<String> {
	let preview_width = width.max(24);
	let Some(style) = registry.style_for(shape) else {
		return vec![String::new(), "Preview:".to_string(), format!("<unknown shape: {shape}>")];
	};
	let padding_x = style.default_padding_x();
	let chrome_width = style.side_chrome_width(padding_x);
	let top_border = status.and_then(|s| match style.status_attachment() {
		"top-border" => Some(s.get_top_border(preview_width.saturating_sub(chrome_width * 2).max(1), Some(PREVIEW_TITLE)).content),
		"top-band" => Some(s.get_band_top_border(preview_width, Some(PREVIEW_TITLE)).content),
		"top-rule-chip" => Some(s.get_standalone_top_border(preview_width, Some(PREVIEW_TITLE)).content),
		_ => None,
	});

	let gutter = style.default_prompt_gutter().to_string();
	let content_width = preview_width
		.saturating_sub(chrome_width * 2)
		.saturating_sub(visible_width_approx(&gutter))
		.max(1);
	let prompt_text = truncate_to_width(
		"Ask anything, edit files, run tools",
		content_width.saturating_sub(1).max(1),
	);
	let prompt_row = format!("{prompt_text}▌");
	let pad = padding_spaces(content_width.saturating_sub(visible_width_approx(&prompt_text)).saturating_sub(1));

	let mut lines = Vec::new();
	if let Some(top) = style.render_top(preview_width, top_border.as_deref()) {
		lines.push(top);
	}
	lines.extend(style.render_row(&prompt_row, &pad, &gutter, preview_width));
	if let Some(bottom) = style.render_bottom(preview_width) {
		lines.push(bottom);
	}
	let bb = style.bottom_bar().to_string();
	if bb != "none" {
		if let Some(s) = status {
			let bar = s.render_bottom_bar(preview_width, &bb, Some(PREVIEW_TITLE));
			if !bar.is_empty() {
				if style.bottom_bar_gap() {
					lines.push(String::new());
				}
				lines.push(bar);
			}
		}
	}
	let mut out = vec![String::new(), "Preview:".to_string()];
	out.extend(lines);
	let _ = style.is_filled();
	out
}

/// Preview state/description model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposerShapePreviewState {
	pub shape: String,
	pub label: Option<String>,
	pub description: Option<String>,
}

impl ComposerShapePreviewState {
	#[must_use]
	pub fn new(shape: impl Into<String>, registry: Option<&dyn ComposerShapeRegistryLike>) -> Self {
		let shape = shape.into();
		let (label, description) = registry
			.and_then(|r| r.options().into_iter().find(|o| o.value == shape))
			.map(|o| (Some(o.label), o.description))
			.unwrap_or((None, None));
		Self { shape, label, description }
	}

	pub fn set_value(&mut self, shape: impl Into<String>, registry: Option<&dyn ComposerShapeRegistryLike>) -> bool {
		let next = shape.into();
		if self.shape == next {
			return false;
		}
		self.shape = next;
		let (label, description) = registry
			.and_then(|r| r.options().into_iter().find(|o| o.value == self.shape))
			.map(|o| (Some(o.label), o.description))
			.unwrap_or((None, None));
		self.label = label;
		self.description = description;
		true
	}

	#[must_use]
	pub fn caption(&self) -> String {
		let label = self.label.clone().unwrap_or_else(|| self.shape.clone());
		match &self.description {
			Some(d) if !d.is_empty() => format!("{label} · {d}"),
			_ => label,
		}
	}

	#[must_use]
	pub fn render_lines<S: ComposerPreviewStatusSource + ?Sized>(
		&self,
		width: usize,
		status: Option<&S>,
		registry: &dyn ComposerShapeRegistryLike,
	) -> Vec<String> {
		render_composer_shape_preview(&self.shape, width, status, registry)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Status;
	impl ComposerPreviewStatusSource for Status {
		fn get_top_border(&self, width: usize, _t: Option<&str>) -> PreviewTopBorder {
			PreviewTopBorder { content: format!("[top:{width}]"), width }
		}
		fn get_band_top_border(&self, width: usize, _t: Option<&str>) -> PreviewTopBorder {
			PreviewTopBorder { content: format!("[band:{width}]"), width }
		}
		fn get_standalone_top_border(&self, width: usize, _t: Option<&str>) -> PreviewTopBorder {
			PreviewTopBorder { content: format!("[chip:{width}]"), width }
		}
		fn render_bottom_bar(&self, _w: usize, groups: &str, _t: Option<&str>) -> String {
			format!("[bar:{groups}]")
		}
	}

	struct Style;
	impl ComposerStyleLike for Style {
		fn default_padding_x(&self) -> usize { 1 }
		fn side_chrome_width(&self, _padding_x: usize) -> usize { 1 }
		fn status_attachment(&self) -> &str { "top-band" }
		fn default_prompt_gutter(&self) -> &str { "> " }
		fn render_top(&self, _w: usize, top_border: Option<&str>) -> Option<String> { top_border.map(str::to_string) }
		fn render_row(&self, text: &str, pad: &str, gutter: &str, _w: usize) -> Vec<String> { vec![format!("{gutter}{text}{pad}")] }
		fn render_bottom(&self, _w: usize) -> Option<String> { Some("───".into()) }
		fn bottom_bar(&self) -> &str { "full" }
		fn bottom_bar_gap(&self) -> bool { true }
		fn is_filled(&self) -> bool { false }
	}

	struct Reg;
	impl ComposerShapeRegistryLike for Reg {
		fn style_for(&self, shape: &str) -> Option<Box<dyn ComposerStyleLike + '_>> {
			if shape == "band" { Some(Box::new(Style)) } else { None }
		}
		fn options(&self) -> Vec<ComposerShapeOption> {
			vec![ComposerShapeOption { value: "band".into(), label: "Band".into(), description: Some("desc".into()) }]
		}
	}

	#[test]
	fn render_preview_includes_status_and_prompt() {
		let lines = render_composer_shape_preview("band", 40, Some(&Status), &Reg);
		assert_eq!(lines[1], "Preview:");
		assert!(lines.iter().any(|l| l.contains("[band:40]")));
		assert!(lines.iter().any(|l| l.contains("Ask anything")));
		assert!(lines.iter().any(|l| l.contains("[bar:full]")));
	}

	#[test]
	fn preview_state_caption_and_set_value() {
		let reg = Reg;
		let mut st = ComposerShapePreviewState::new("band", Some(&reg));
		assert_eq!(st.caption(), "Band · desc");
		assert!(!st.set_value("band", Some(&reg)));
		assert!(st.set_value("box", Some(&reg)));
		assert_eq!(st.label, None);
		assert_eq!(st.caption(), "box");
		let unknown = render_composer_shape_preview("nope", 30, Option::<&Status>::None, &reg);
		assert!(unknown.last().unwrap().contains("unknown shape"));
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::component::Component;
use crate::keys::Key;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Live composer-shape preview component.
///
/// Reference: `ComposerShapePreview` in `l/src/overlays/composer-shape-preview.ts`.
///
/// Deviations: live `StatusLineComponent` chrome borrowing and TUI requestRender
/// hooks are abstracted via traits/state; renders preview lines and native rows.
pub struct ComposerShapePreview<R: ComposerShapeRegistryLike> {
	pub state: ComposerShapePreviewState,
	registry: R,
}

impl<R: ComposerShapeRegistryLike> std::fmt::Debug for ComposerShapePreview<R> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ComposerShapePreview")
			.field("shape", &self.state.shape)
			.finish_non_exhaustive()
	}
}

impl<R: ComposerShapeRegistryLike> ComposerShapePreview<R> {
	#[must_use]
	pub fn new(initial_shape: impl Into<String>, registry: R) -> Self {
		let shape = initial_shape.into();
		let state = ComposerShapePreviewState::new(shape, Some(&registry));
		Self { state, registry }
	}

	pub fn set_value(&mut self, shape: impl Into<String>) -> bool {
		self.state.set_value(shape, Some(&self.registry))
	}

	pub fn caption(&self) -> String {
		self.state.caption()
	}
}

impl<R: ComposerShapeRegistryLike> Component for ComposerShapePreview<R> {
	fn render(&mut self, width: u16) -> Vec<String> {
		render_composer_shape_preview(
			&self.state.shape,
			width.saturating_sub(2) as usize,
			Option::<&dyn ComposerPreviewStatusSource>::None,
			&self.registry,
		)
	}

	fn handle_input(&mut self, _key: &Key) {}
}

impl<R: ComposerShapeRegistryLike> NativeDescribe for ComposerShapePreview<R> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = render_composer_shape_preview(
			&self.state.shape,
			cx.cols.saturating_sub(2) as usize,
			Option::<&dyn ComposerPreviewStatusSource>::None,
			&self.registry,
		);
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert("shape", serde_json::Value::String(self.state.shape.clone()));
		props.insert("caption", serde_json::Value::String(self.caption()));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;

	struct Reg;
	impl ComposerShapeRegistryLike for Reg {
		fn style_for(&self, _shape: &str) -> Option<Box<dyn ComposerStyleLike + '_>> { None }
		fn options(&self) -> Vec<ComposerShapeOption> {
			vec![ComposerShapeOption { value: "band".into(), label: "Band".into(), description: Some("desc".into()) }]
		}
	}

	#[test]
	fn composer_shape_preview_component_renders_and_describes() {
		let mut c = ComposerShapePreview::new("band", Reg);
		assert_eq!(c.caption(), "Band · desc");
		let lines = c.render(40);
		assert!(lines.iter().any(|l| l.contains("unknown shape") || l.contains("Preview:")));
		c.set_value("box");
		let node = c.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
