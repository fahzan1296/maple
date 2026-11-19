//! Port of `l/test/snapcompact-shape-preview.test.ts` (portable subset).
//!
//! Divergence: TS used real snapcompact sample rendering (`getNativeBlob` bytes),
//! async render settling, Kitty/image feature context, and model-specific
//! geometry/token captions (Cursor Claude 1024px cap, Anthropic opus line).
//! Rust exposes `SnapcompactShapePreview` with injected renderer/budget and
//! synchronous sample rendering, caption/variant helpers, and `NativeDescribe`.
//! Current `resolve_shape` returns a fixed default geometry/token estimate (not
//! model-dependent), so exact TS caption numbers/byte-difference assertions are
//! not portable; this subset asserts available real behavior: variant naming/
//! resolution, auto label, set_value tracking, render states, and describe role.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use maple_tui::component::Component;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NodeKind};
use maple_tui::overlays::snapcompact_shape_preview::{
	caption, is_shape_variant_name, resolve_shape, resolved_variant_name, PreviewSample, Shape,
	SnapcompactImageBudget, SnapcompactSampleRenderer, SnapcompactShapePreview,
	SnapcompactShapePreviewOptions,
};

struct FakeRenderer;
impl SnapcompactSampleRenderer for FakeRenderer {
	fn render_sample(&self, _shape: &Shape, name: &str) -> Result<PreviewSample, String> {
		Ok(PreviewSample {
			bytes: vec![1, 2, 3],
			mime: "image/png".into(),
			edge_px: 512,
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
fn variant_resolution_and_auto_caption_name_the_variant() {
	let shape = resolve_shape(None, "auto");
	let name = resolved_variant_name(&shape);
	assert!(is_shape_variant_name(name));
	let (label, stats) = caption(&shape, "auto");
	assert!(label.starts_with("auto → "));
	assert!(label.contains(name));
	assert!(stats.contains("full frame"));
	assert!(stats.contains("cells"));
	assert!(stats.contains("tokens"));

	let explicit = resolve_shape(None, "6x10-word");
	assert_eq!(resolved_variant_name(&explicit), "6x10-word");
	let (label, _) = caption(&explicit, "6x10-word");
	assert_eq!(label, "6x10-word");
}

#[test]
fn set_value_tracks_known_variants_and_rejects_unknown() {
	let mut preview = SnapcompactShapePreview::new("auto", SnapcompactShapePreviewOptions::default());
	preview.set_value("6x10-sent");
	// internal variant is not public; observe via render caption line
	let line = preview.render(80).join("\n");
	assert!(line.contains("· 6x10-sent ·"));
	preview.set_value("bogus");
	let line = preview.render(80).join("\n");
	assert!(line.contains("· auto → "));
}

#[test]
fn render_uses_renderer_budget_and_falls_back_without_capability() {
	let mut capable = SnapcompactShapePreview::new(
		"5x8-sent",
		SnapcompactShapePreviewOptions {
			renderer: Some(Arc::new(FakeRenderer)),
			image_budget: Some(Arc::new(FakeBudget(AtomicU32::new(0)))),
			image_capable: true,
			..Default::default()
		},
	);
	let lines = capable.render(60).join("\n");
	assert!(lines.contains("Sample (zoomed)"));
	assert!(lines.contains("[image"));
	assert!(lines.contains("512x512px"));

	let mut plain = SnapcompactShapePreview::new("auto", SnapcompactShapePreviewOptions::default());
	let lines = plain.render(60).join("\n");
	assert!(lines.contains("graphic sample needs an image-capable terminal"));
}

#[test]
fn describe_reports_preview_role_and_image_node_when_capable() {
	let preview = SnapcompactShapePreview::new(
		"5x8-word",
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
	assert!(node.children.iter().any(|c| c.kind == NodeKind::Image));
}
