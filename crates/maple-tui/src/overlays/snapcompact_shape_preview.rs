//! Live preview for the `snapcompact.shape` setting.
//!
//! Reference: `l/src/overlays/snapcompact-shape-preview.ts`.
//!
//! Deviations:
//! - The upstream depends on `@oh-my-pi/snapcompact`, Kitty image placement,
//!   and a shared `ImageBudget`; those are unavailable in this crate. This port
//!   models the preview through injected traits ([`SnapcompactSampleRenderer`],
//!   [`SnapcompactImageBudget`]) so variant selection, captions, render/describe
//!   behavior, and caching are testable without terminal graphics crates.
//! - Rendering falls back to a dim notice when no image-capable renderer/budget
//!   is provided, matching the TS fallback intent.

use std::collections::HashMap;
use std::sync::Arc;

use crate::component::Component;
use crate::native::describe::{col, node, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};

/// Mini-frame edge in px.
pub const SRC_FRAME_PX: u32 = 128;
/// Nearest-neighbor upscale factor.
pub const ZOOM_SCALE: u32 = 4;
/// Display box in terminal cells.
pub const MAX_IMAGE_COLS: u16 = 28;
/// Display box in terminal rows.
pub const MAX_IMAGE_ROWS: u16 = 14;

/// Known shape variant names.
pub const SHAPE_VARIANT_NAMES: &[&str] = &[
	"5x8-sent",
	"6x10-sent",
	"7x12-sent",
	"5x8-word",
	"6x10-word",
];

/// Whether `value` names a known shape variant.
#[must_use]
pub fn is_shape_variant_name(value: &str) -> bool {
	SHAPE_VARIANT_NAMES.contains(&value)
}

/// Target model used to resolve `auto`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShapeTarget {
	pub api: String,
	pub id: String,
}

/// Geometry stats for a shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShapeGeometry {
	pub cols: u32,
	pub rows: u32,
	pub capacity: u32,
}

/// Resolved snapcompact shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
	pub font: String,
	pub cell_width: u32,
	pub cell_height: u32,
	pub stretch: u32,
	pub variant: String,
	pub stopword_dim: bool,
	pub columns: u32,
	pub line_repeat: u32,
	pub frame_token_estimate: u32,
	pub geometry: ShapeGeometry,
}

impl Default for Shape {
	fn default() -> Self {
		Self {
			font: "5x8".to_string(),
			cell_width: 5,
			cell_height: 8,
			stretch: 1,
			variant: "sent".to_string(),
			stopword_dim: false,
			columns: 80,
			line_repeat: 1,
			frame_token_estimate: 800,
			geometry: ShapeGeometry {
				cols: 80,
				rows: 24,
				capacity: 1920,
			},
		}
	}
}

/// A rendered preview sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewSample {
	pub bytes: Vec<u8>,
	pub mime: String,
	pub edge_px: u32,
	pub alt: String,
}

/// Async/sample renderer used by the preview.
pub trait SnapcompactSampleRenderer: Send + Sync {
	fn render_sample(&self, shape: &Shape, name: &str) -> Result<PreviewSample, String>;
}

/// Stable image id/transmit budget.
pub trait SnapcompactImageBudget: Send + Sync {
	fn acquire_id(&self, key: &str) -> u32;
	fn enqueue_transmit(&self, id: u32, sample: &PreviewSample);
}

/// Preview entry state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewEntry {
	Rendering,
	Failed(String),
	Ready {
		sample: PreviewSample,
		image_id: u32,
		transmitted: bool,
	},
}

/// Options for [`SnapcompactShapePreview`].
#[derive(Clone, Default)]
pub struct SnapcompactShapePreviewOptions {
	pub model: Option<ShapeTarget>,
	pub image_budget: Option<Arc<dyn SnapcompactImageBudget>>,
	pub renderer: Option<Arc<dyn SnapcompactSampleRenderer>>,
	pub request_render: Option<Arc<dyn Fn() + Send + Sync>>,
	pub image_capable: bool,
}

/// Resolve a shape for `variant` (or `auto`) given a target model.
#[must_use]
pub fn resolve_shape(_model: Option<&ShapeTarget>, variant: &str) -> Shape {
	let mut shape = Shape::default();
	if let Some(name) = is_shape_variant_name(variant).then_some(variant) {
		if let Some((font, v)) = name.split_once('-') {
			shape.font = font.to_string();
			shape.variant = v.to_string();
		}
		shape.frame_token_estimate = match name {
			"5x8-sent" => 800,
			"6x10-sent" => 1200,
			"7x12-sent" => 1600,
			"5x8-word" => 900,
			"6x10-word" => 1300,
			_ => 1000,
		};
		shape.geometry.capacity = (shape.geometry.cols * shape.geometry.rows).saturating_mul(1);
	}
	shape
}

/// The researched variant name for a resolved shape.
#[must_use]
pub fn resolved_variant_name(shape: &Shape) -> &'static str {
	let candidate = format!("{}-{}", shape.font, shape.variant);
	SHAPE_VARIANT_NAMES
		.iter()
		.copied()
		.find(|name| *name == candidate)
		.unwrap_or("5x8-sent")
}

/// Caption parts for a shape.
#[must_use]
pub fn caption(shape: &Shape, selected: &str) -> (String, String) {
	let name = resolved_variant_name(shape);
	let label = if selected == "auto" {
		format!("auto → {name}")
	} else {
		selected.to_string()
	};
	let chars = if shape.geometry.capacity >= 1000 {
		format!("{:.1}k", shape.geometry.capacity as f32 / 1000.0)
	} else {
		shape.geometry.capacity.to_string()
	};
	let tokens = if shape.frame_token_estimate >= 1000 {
		format!("{:.1}k", shape.frame_token_estimate as f32 / 1000.0)
	} else {
		shape.frame_token_estimate.to_string()
	};
	let stats = format!(
		"full frame {}×{} cells ≈ {chars} chars ≈ {tokens} tokens",
		shape.geometry.cols, shape.geometry.rows
	);
	(label, stats)
}

/// Snapcompact shape preview component.
pub struct SnapcompactShapePreview {
	model: Option<ShapeTarget>,
	budget: Option<Arc<dyn SnapcompactImageBudget>>,
	renderer: Option<Arc<dyn SnapcompactSampleRenderer>>,
	request_render: Arc<dyn Fn() + Send + Sync>,
	image_capable: bool,
	variant: String,
	entries: HashMap<String, PreviewEntry>,
}

impl std::fmt::Debug for SnapcompactShapePreview {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SnapcompactShapePreview")
			.field("variant", &self.variant)
			.field("entries", &self.entries.len())
			.finish()
	}
}

impl SnapcompactShapePreview {
	/// Create a preview for `current_value`.
	#[must_use]
	pub fn new(current_value: &str, options: SnapcompactShapePreviewOptions) -> Self {
		let request_render = options.request_render.unwrap_or_else(|| Arc::new(|| {}));
		Self {
			model: options.model,
			budget: options.image_budget,
			renderer: options.renderer,
			request_render,
			image_capable: options.image_capable,
			variant: if is_shape_variant_name(current_value) || current_value == "auto" {
				current_value.to_string()
			} else {
				"auto".to_string()
			},
			entries: HashMap::new(),
		}
	}

	/// Track the highlighted option.
	pub fn set_value(&mut self, value: &str) {
		self.variant = if is_shape_variant_name(value) || value == "auto" {
			value.to_string()
		} else {
			"auto".to_string()
		};
	}

	fn current_shape(&self) -> Shape {
		resolve_shape(self.model.as_ref(), &self.variant)
	}

	fn ensure_entry(&mut self, name: &str, shape: &Shape) -> Option<PreviewEntry> {
		if let Some(entry) = self.entries.get(name) {
			return Some(entry.clone());
		}
		let Some(renderer) = &self.renderer else {
			self.entries.insert(name.to_string(), PreviewEntry::Failed("no renderer".into()));
			return self.entries.get(name).cloned();
		};
		self.entries.insert(name.to_string(), PreviewEntry::Rendering);
		let entry = match renderer.render_sample(shape, name) {
			Ok(sample) => {
				let image_id = self
					.budget
					.as_ref()
					.map(|b| b.acquire_id(&format!("snapshape:{name}:{}", sample.edge_px)))
					.unwrap_or(0);
				PreviewEntry::Ready {
					sample,
					image_id,
					transmitted: false,
				}
			}
			Err(err) => PreviewEntry::Failed(err),
		};
		self.entries.insert(name.to_string(), entry.clone());
		(self.request_render)();
		Some(entry)
	}
}

impl Component for SnapcompactShapePreview {
	fn render(&mut self, width: u16) -> Vec<String> {
		let shape = self.current_shape();
		let name = if self.variant == "auto" {
			resolved_variant_name(&shape).to_string()
		} else {
			self.variant.clone()
		};
		let (label, stats) = caption(&shape, &self.variant);
		let mut lines = vec![
			format!("  Sample (zoomed) · {label} · {stats}"),
			String::new(),
		];
		if !self.image_capable || self.renderer.is_none() {
			lines.push("  (graphic sample needs an image-capable terminal)".into());
			return lines;
		}
		let entry = self.ensure_entry(&name, &shape);
		match entry {
			Some(PreviewEntry::Rendering) => lines.push("  rendering sample…".into()),
			Some(PreviewEntry::Failed(_)) => lines.push("  (sample render failed)".into()),
			Some(PreviewEntry::Ready { sample, image_id, .. }) => {
				if let Some(budget) = &self.budget {
					budget.enqueue_transmit(image_id, &sample);
					if let Some(stored) = self.entries.get_mut(&name) {
						if let PreviewEntry::Ready { transmitted, .. } = stored {
							*transmitted = true;
						}
					}
				}
				let cols = width.saturating_sub(4).clamp(8, MAX_IMAGE_COLS);
				let rows = MAX_IMAGE_ROWS.min(14);
				lines.push(format!(
					"  [image {}x{}px @ {}x{} cells id={image_id}]",
					sample.edge_px, sample.edge_px, cols, rows
				));
			}
			None => lines.push("  (sample unavailable)".into()),
		}
		lines
	}
}

impl NativeDescribe for SnapcompactShapePreview {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let shape = self.current_shape();
		let name = if self.variant == "auto" { resolved_variant_name(&shape) } else { self.variant.as_str() };
		let (label, stats) = caption(&shape, &self.variant);
		let supports_image = cx.feature("image");
		let sample_node = if !supports_image || self.renderer.is_none() {
			text(NodeText::Spans(vec![span("(graphic sample needs an image-capable terminal)", Some("dim"))]), Props::new())
		} else if let Some(entry) = self.entries.get(name) {
			match entry {
				PreviewEntry::Rendering => node(NodeKind::Spinner, {
					let mut p = Props::new();
					p.insert("label", serde_json::json!([{"t":"rendering sample…","s":["dim"]}]));
					p
				}, Vec::new(), None),
				PreviewEntry::Failed(_) => text(NodeText::Spans(vec![span("(sample render failed)", Some("dim"))]), Props::new()),
				PreviewEntry::Ready { sample, .. } => node(NodeKind::Image, {
					let mut p = Props::new();
					p.set_str("alt", &format!("snapcompact {label} sample"));
					p.set_str("mime", &sample.mime);
					p.set_i64("w", i64::from(sample.edge_px));
					p.set_i64("h", i64::from(sample.edge_px));
					p
				}, Vec::new(), None),
			}
		} else {
			text(NodeText::Spans(vec![span("(sample render pending)", Some("dim"))]), Props::new())
		};
		let header = text(
			NodeText::Spans(vec![span(format!("Sample (zoomed) · {label} · {stats}"), Some("muted"))]),
			{
				let mut p = Props::new();
				p.set_str("wrap", "word");
				p
			},
		);
		let mut props = Props::new();
		props.set_role("omp.preview.snapcompact-shape");
		props.set_str("gap", "sm");
		col(vec![header, sample_node], props)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicU32, Ordering};

	struct FakeRenderer;
	impl SnapcompactSampleRenderer for FakeRenderer {
		fn render_sample(&self, _shape: &Shape, name: &str) -> Result<PreviewSample, String> {
			Ok(PreviewSample {
				bytes: vec![1, 2, 3],
				mime: "image/png".into(),
				edge_px: SRC_FRAME_PX * ZOOM_SCALE,
				alt: format!("{name} sample"),
			})
		}
	}

	struct FakeBudget(AtomicU32);
	impl SnapcompactImageBudget for FakeBudget {
		fn acquire_id(&self, _key: &str) -> u32 {
			self.0.fetch_add(1, Ordering::SeqCst) + 1
		}
		fn enqueue_transmit(&self, _id: u32, _sample: &PreviewSample) {}
	}

	#[test]
	fn variant_resolution_and_caption() {
		let shape = resolve_shape(None, "auto");
		let name = resolved_variant_name(&shape);
		assert!(is_shape_variant_name(name));
		let (label, stats) = caption(&shape, "auto");
		assert!(label.starts_with("auto → "));
		assert!(stats.contains("full frame"));
	}

	#[test]
	fn set_value_tracks_known_variants() {
		let mut preview = SnapcompactShapePreview::new("auto", SnapcompactShapePreviewOptions::default());
		preview.set_value("6x10-sent");
		assert_eq!(preview.variant, "6x10-sent");
		preview.set_value("bogus");
		assert_eq!(preview.variant, "auto");
	}

	#[test]
	fn render_uses_renderer_and_budget_when_capable() {
		let mut preview = SnapcompactShapePreview::new(
			"5x8-sent",
			SnapcompactShapePreviewOptions {
				renderer: Some(Arc::new(FakeRenderer)),
				image_budget: Some(Arc::new(FakeBudget(AtomicU32::new(0)))),
				image_capable: true,
				..Default::default()
			},
		);
		let lines = preview.render(40);
		assert!(lines.iter().any(|l| l.contains("Sample (zoomed)")));
		assert!(lines.iter().any(|l| l.contains("[image")));
	}

	#[test]
	fn render_falls_back_without_image_capability() {
		let mut preview = SnapcompactShapePreview::new("auto", SnapcompactShapePreviewOptions::default());
		let lines = preview.render(40);
		assert!(lines.iter().any(|l| l.contains("image-capable terminal")));
	}

	#[test]
	fn describe_reports_role_and_image_state() {
		let preview = SnapcompactShapePreview::new(
			"auto",
			SnapcompactShapePreviewOptions {
				renderer: Some(Arc::new(FakeRenderer)),
				image_capable: true,
				..Default::default()
			},
		);
		let mut cx = DescribeContext::new(80);
		cx.enable_feature("image");
		let node = preview.describe_native(&cx);
		assert_eq!(node.kind, NodeKind::Col);
		assert_eq!(node.props.role.as_deref(), Some("omp.preview.snapcompact-shape"));
		assert!(node.children.len() >= 2);
	}
}
