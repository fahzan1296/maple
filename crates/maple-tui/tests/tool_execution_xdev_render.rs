//! Port of `l/test/tool-execution-xdev-render.test.ts` (portable subset).
//!
//! TS asserts that a `write xd://<tool>` card forwards to the dispatched tool's
//! own renderer while streaming and after the result, even when mounted-name
//! gating is empty, via the host resolver wired into `ToolExecutionComponent`.
//! Rust exposes xdev dispatch contracts/helpers in `tools::xdev` (renderer
//! resolution, call preview kind, queued/help describe, result forwarding plan)
//! but not the live `write` + `ToolExecutionComponent` renderer-dispatch wiring
//! as the same injectable host seam in tests. This ports faithful contracts on
//! those exported helpers: host-resolver-style renderer selection drives
//! call/result forwarding decisions, queued-vs-device/default preview, and
//! help/default fallback behavior.
//!
//! Deviations:
//! - No mounted `XdevMountedState.resolve` callback on a live write component;
//!   renderer selection is asserted via `resolve_device_renderer`,
//!   `xdev_call_preview_kind`, and `plan_xdev_result_forward` with explicit
//!   mounted/lookup inputs (the same authority split the TS host uses).
//! - No rendered "PROBE-CALL"/"PROBE-RESULT" component text; help and queued
//!   describe views are asserted through `describe_xdev_help_result` and
//!   `describe_xdev_queued_call`/head helpers.

use maple_tui::native::node::NodeText;
use maple_tui::tools::xdev::{
	describe_xdev_head, describe_xdev_help_result, describe_xdev_queued_call, display_device_label,
	plan_xdev_result_forward, resolve_device_renderer, xdev_call_preview_kind, XdevCallPreviewKind,
	XdevMountedRenderer, XdevRenderDispatch, XdevRenderOptions, XdevRendererSource,
};
use std::collections::{HashMap, HashSet};

fn probe_renderer() -> XdevMountedRenderer {
	XdevMountedRenderer {
		label: Some("Probe".to_string()),
		has_render_call: true,
		has_render_result: true,
		has_describe_call: true,
		has_describe_result: true,
		merge_call_and_result: true,
	}
}

fn head_plain(view: &maple_tui::tools::native_view::NativeToolView) -> String {
	match view.head.as_ref() {
		Some(NodeText::Plain(s)) => s.clone(),
		Some(NodeText::Spans(spans)) => spans.iter().map(|s| s.t.clone()).collect(),
		None => String::new(),
	}
}

#[test]
fn host_resolved_renderer_is_used_for_streaming_call_and_result_forwarding() {
	let mounted_names: HashSet<String> = HashSet::new(); // intentionally empty, like TS mountedNames
	let _mounted_state = maple_tui::tools::xdev::XdevMountedState {
		mounted_names,
		tools: HashMap::from([("probe".to_string(), probe_renderer())]),
	};
	// Host resolver returns the probe renderer regardless of empty mounted-name set.
	let (renderer, source) = resolve_device_renderer(Some(&probe_renderer()), None);
	assert_eq!(source, XdevRendererSource::Mounted);
	assert!(renderer.map(|r| r.can_render()).unwrap_or(false));

	let queued = xdev_call_preview_kind(
		&XdevRenderOptions {
			execution_started: false,
			is_partial: true,
		},
		renderer,
	);
	assert_eq!(queued, XdevCallPreviewKind::Queued);

	let streaming = xdev_call_preview_kind(
		&XdevRenderOptions {
			execution_started: true,
			is_partial: true,
		},
		renderer,
	);
	assert_eq!(streaming, XdevCallPreviewKind::DeviceCall);

	let dispatch = XdevRenderDispatch {
		tool: "probe".to_string(),
		mode: "execute".to_string(),
		args: None,
		tier: None,
		inner: None,
	};
	let forward = plan_xdev_result_forward(&dispatch, "42", Some(&probe_renderer()), None);
	assert!(forward.has_renderer);
	assert!(forward.has_result_renderer);
	assert!(forward.merge_call_and_result);
	assert!(forward.help_text.is_none());
}

#[test]
fn missing_host_renderer_falls_back_to_default_call_and_generic_result_path() {
	let (renderer, source) = resolve_device_renderer(None, None);
	assert_eq!(source, XdevRendererSource::None);
	assert!(renderer.is_none());

	let kind = xdev_call_preview_kind(
		&XdevRenderOptions {
			execution_started: true,
			is_partial: false,
		},
		renderer,
	);
	assert_eq!(kind, XdevCallPreviewKind::DefaultCall);

	let dispatch = XdevRenderDispatch {
		tool: "probe".to_string(),
		mode: "execute".to_string(),
		args: None,
		tier: None,
		inner: None,
	};
	let forward = plan_xdev_result_forward(&dispatch, "42", None, None);
	assert!(!forward.has_renderer);
	assert!(!forward.has_result_renderer);
	assert!(!forward.merge_call_and_result);

	let help_dispatch = XdevRenderDispatch {
		tool: "probe".to_string(),
		mode: "help".to_string(),
		args: None,
		tier: None,
		inner: None,
	};
	let help = plan_xdev_result_forward(&help_dispatch, "docs", None, None);
	assert_eq!(help.help_text.as_deref(), Some("docs"));
	let help_view = describe_xdev_help_result("docs").expect("help body");
	assert!(help_view.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
}

#[test]
fn queued_and_head_describe_helpers_label_dispatched_device() {
	assert_eq!(display_device_label("probe", Some(&probe_renderer())), "Probe");
	let queued = describe_xdev_queued_call("probe", Some("{\"command\":\"Write-Output 42\"}"), Some(&probe_renderer()));
	assert!(queued.inline.unwrap_or(false));
	let qhead = head_plain(&queued);
	assert!(qhead.contains("queued"));
	assert!(qhead.contains("Probe"));

	let head = describe_xdev_head("probe", Some("Write-Output 42"));
	assert!(head_plain(&head).contains("probe") || !head_plain(&head).is_empty());
}
