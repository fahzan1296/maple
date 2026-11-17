//! One tool call's card: call head, streamed args, and the result.
//!
//! Reference: `l/src/chat/tool-execution.ts`.
//!
//! Header, params and result are ordered product requirements; `Ctrl+O`
//! expands, `└⌐` frames. This ports the block's state machine — partial results,
//! background parking, sealing, displacement, late diagnostics, path routing,
//! and the native card description — over the crate's [`Component`] trait.
//!
//! Deviations:
//! - No per-tool JS renderer registry: [`ToolViewRenderer`] is the injection
//!   seam, and a card without one draws the crate's default tool view.
//! - No terminal painting internals (image protocol, Kitty PNG conversion,
//!   sixel, spinner/elapsed timers, shell replay, memoized display keys): the
//!   host schedules repaints through [`ToolExecutionUi`], and figures/images
//!   are the renderer's problem.
//! - Rust has no exceptions, so [`GuardedRenderer`] cannot catch a panicking
//!   renderer the way `SafeToolRendererComponent` does; it documents the seam.

use std::collections::HashSet;

use serde_json::Value;

use crate::chat::execution_shared::DEFAULT_TERMINAL_PREVIEW_LINES;
use crate::chat::transcript_presentation::ResolvedPresentation;
use crate::component::Component;
use crate::native::describe::{card, col, keyed, node, span};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Tone};
use crate::render::render_utils::{format_status_icon, ToolUiStatus};
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::theme::theme_class::Theme;
use crate::tools::default_renderer::{describe_default_tool_execution, DefaultRenderOptions, DefaultToolRenderInput};
use crate::tools::native_view::{
	diagnostics_section, display_path, result_text, ContentBlock, DiagnosticsSectionOpts,
	FileDiagnosticsResult, NativePreview, NativeToolView, RenderResultOptions, ToolRenderResult,
};

/// Tools whose returned call hands its card to the background job it started.
pub const JOB_CARD_TOOLS: [(&str, &str); 3] = [("task", "always"), ("bash", "native"), ("eval", "native")];

/// Tools whose result snapshot the next call of the same tool replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplaceableToolName {
	/// A `wait` showing all-running.
	Wait,
	/// A per-turn `todo` snapshot.
	Todo,
}

impl DisplaceableToolName {
	/// Wire name.
	pub fn as_str(self) -> &'static str {
		match self {
			DisplaceableToolName::Wait => "wait",
			DisplaceableToolName::Todo => "todo",
		}
	}
}

/// Resolves the canonical renderer key while retaining the provider's wire name.
pub fn tool_render_name(wire_name: &str, tool_name: Option<&str>) -> String {
	tool_name.unwrap_or(wire_name).to_string()
}

/// The background job state a result reports (`details.async.state`).
pub fn async_job_state(details: Option<&Value>) -> Option<&str> {
	details?.get("async")?.get("state")?.as_str()
}

/// Whether a frame is a background job's last one.
pub fn is_settled_job_frame(details: Option<&Value>) -> bool {
	matches!(async_job_state(details), Some("completed" | "failed" | "cancelled"))
}

/// Whether a call that returned with a running job stays parked so later job
/// frames keep filling its card.
pub fn parks_as_background(tool_name: &str, details: Option<&Value>, native_rendering: bool) -> bool {
	let Some((_, mode)) = JOB_CARD_TOOLS.iter().find(|(name, _)| *name == tool_name) else {
		return false;
	};
	if *mode == "native" && !native_rendering {
		return false;
	}
	async_job_state(details) == Some("running")
}

/// Settle a parked card with its job's last frame.
pub fn settle_with_job_frame<H: ToolExecutionHandle + ?Sized>(
	component: &mut H,
	mut frame: ToolResultFrame,
	tool_call_id: &str,
) {
	let state = async_job_state(frame.details.as_ref());
	if state == Some("cancelled") {
		component.update_result(frame, true, Some(tool_call_id));
		component.seal();
		return;
	}
	frame.is_error = state == Some("failed");
	component.update_result(frame, false, Some(tool_call_id));
}

/// Card status a settled or streaming call presents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardStatus {
	/// No result yet and not started.
	Pending,
	/// Calling, or streaming a partial result.
	Running,
	/// Settled fine.
	Done,
	/// Settled with `isError`.
	Error,
	/// Sealed without a result.
	Cancelled,
}

impl CardStatus {
	/// Wire name.
	pub fn as_str(self) -> &'static str {
		match self {
			CardStatus::Pending => "pending",
			CardStatus::Running => "running",
			CardStatus::Done => "done",
			CardStatus::Error => "error",
			CardStatus::Cancelled => "cancelled",
		}
	}

	/// Card tone; settled cards stay neutral.
	pub fn tone(self) -> Option<Tone> {
		match self {
			CardStatus::Pending | CardStatus::Running => Some(Tone::Pending),
			CardStatus::Error => Some(Tone::Error),
			CardStatus::Cancelled => Some(Tone::Info),
			CardStatus::Done => None,
		}
	}

	/// Status glyph state.
	pub fn ui_status(self) -> ToolUiStatus {
		match self {
			CardStatus::Pending => ToolUiStatus::Pending,
			CardStatus::Running => ToolUiStatus::Running,
			CardStatus::Done => ToolUiStatus::Success,
			CardStatus::Error => ToolUiStatus::Error,
			CardStatus::Cancelled => ToolUiStatus::Aborted,
		}
	}
}

/// One content block of a tool result frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolResultBlock {
	/// `text`, `image`, or a provider-specific kind.
	pub kind: String,
	/// Text payload.
	pub text: Option<String>,
	/// Base64 image payload.
	pub data: Option<String>,
	/// Image MIME type.
	pub mime_type: Option<String>,
}

/// A tool result or progress frame as a card shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolResultFrame {
	/// Result content blocks.
	pub content: Vec<ToolResultBlock>,
	/// Tool-specific details payload.
	pub details: Option<Value>,
	/// Whether the tool reported a failure.
	pub is_error: bool,
}

impl ToolResultFrame {
	/// The [`ToolRenderResult`] a renderer sees.
	pub fn as_render_result(&self) -> ToolRenderResult<Value> {
		ToolRenderResult {
			content: self
				.content
				.iter()
				.map(|block| ContentBlock {
					type_: block.kind.clone(),
					text: block.text.clone(),
				})
				.collect(),
			details: self.details.clone(),
			is_error: Some(self.is_error),
		}
	}
}

/// Card options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolExecutionOptions {
	/// Show result images when the terminal can.
	pub show_images: bool,
	/// Use the name-keyed built-in renderer.
	pub use_built_in_renderer: bool,
	/// Ask the extension transcript presenters.
	pub transcript_presenters: bool,
}

impl Default for ToolExecutionOptions {
	fn default() -> Self {
		ToolExecutionOptions {
			show_images: true,
			use_built_in_renderer: true,
			transcript_presenters: true,
		}
	}
}

/// The TUI surface a card uses to schedule repaints.
pub trait ToolExecutionUi {
	/// Mark the transcript dirty.
	fn request_render(&self);
	/// Re-render one child component.
	fn request_component_render(&self, _component: &dyn Component) {}
	/// Drop the terminal's incremental display state.
	fn reset_display(&self) {}
}

/// The host's repaint sink, for hosts that only need [`ToolExecutionUi`].
#[derive(Default)]
pub struct NoopToolExecutionUi;

impl ToolExecutionUi for NoopToolExecutionUi {
	fn request_render(&self) {}
}

/// The mutable surface a live call and the transcript rebuild both drive.
pub trait ToolExecutionHandle: Component {
	/// Take streamed call arguments.
	fn update_args(&mut self, args: Option<Value>, tool_call_id: Option<&str>);
	/// Take a streamed preview frame.
	fn update_stream_preview(&mut self, _update: Option<Value>) {}
	/// Take a result frame.
	fn update_result(&mut self, result: ToolResultFrame, is_partial: bool, tool_call_id: Option<&str>);
	/// The call's arguments are complete.
	fn set_args_complete(&mut self, _tool_call_id: Option<&str>) {}
	/// Execution started.
	fn set_execution_started(&mut self, _tool_call_id: Option<&str>) {}
	/// Transcript-wide expansion.
	fn set_expanded(&mut self, expanded: bool);
	/// Returned, but stays tracked for the job's frames.
	fn park_as_background(&mut self);
	/// Seal as final history.
	fn seal(&mut self);
}

/// A tool's describe hooks, as the renderer registry exposes them.
///
/// **Injection point** for the `tools/` renderers: the crate builds the view
/// from the same arguments the reference's registry would receive.
pub trait ToolViewRenderer {
	/// The call's view.
	fn describe_call(&self, _args: Option<&Value>) -> NativeToolView {
		NativeToolView::default()
	}

	/// The result's view.
	fn describe_result(&self, _result: &ToolRenderResult<Value>, _options: &RenderResultOptions) -> NativeToolView {
		NativeToolView::default()
	}

	/// One view for a call whose result already arrived.
	fn merge_call_and_result(&self, _args: Option<&Value>, _result: &ToolRenderResult<Value>) -> Option<NativeToolView> {
		None
	}
}

/// A renderer behind its fallback — the reference's guard, minus exceptions.
pub struct GuardedRenderer {
	inner: Option<Box<dyn ToolViewRenderer>>,
}

impl GuardedRenderer {
	/// Wrap `inner`.
	pub fn new(inner: Option<Box<dyn ToolViewRenderer>>) -> Self {
		GuardedRenderer { inner }
	}

	/// The wrapped renderer, if any.
	pub fn inner(&self) -> Option<&dyn ToolViewRenderer> {
		self.inner.as_deref()
	}
}

impl ToolViewRenderer for GuardedRenderer {
	fn describe_call(&self, args: Option<&Value>) -> NativeToolView {
		self.inner
			.as_ref()
			.map(|renderer| renderer.describe_call(args))
			.unwrap_or_default()
	}

	fn describe_result(&self, result: &ToolRenderResult<Value>, options: &RenderResultOptions) -> NativeToolView {
		self.inner
			.as_ref()
			.map(|renderer| renderer.describe_result(result, options))
			.unwrap_or_default()
	}

	fn merge_call_and_result(&self, args: Option<&Value>, result: &ToolRenderResult<Value>) -> Option<NativeToolView> {
		self.inner
			.as_ref()
			.and_then(|renderer| renderer.merge_call_and_result(args, result))
	}
}

/// A patch produced by `apply_patch`/`edit` — the streaming body case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditMode {
	/// A single-file `edit`.
	Edit,
	/// A multi-file patch.
	ApplyPatch,
	/// A whole-file `write`.
	Write,
}

/// Tools whose arguments stream a growing file body or diff into the card.
pub fn streams_body(tool_name: &str) -> bool {
	is_edit_like_tool_name(tool_name) || tool_name == "write"
}

/// Edit-family tool.
pub fn is_edit_like_tool_name(tool_name: &str) -> bool {
	matches!(tool_name, "edit" | "apply_patch")
}

/// The edit mode a tool renders with.
pub fn resolve_edit_mode_for_tool(tool_name: &str, tool_mode: Option<EditMode>) -> Option<EditMode> {
	match tool_name {
		"apply_patch" => Some(EditMode::ApplyPatch),
		"write" => Some(EditMode::Write),
		"edit" => Some(tool_mode.unwrap_or(EditMode::Edit)),
		_ => None,
	}
}

/// The card's collapsed clamp for a renderer's preview.
pub fn card_preview(preview: Option<&NativePreview>) -> Option<NativePreview> {
	match preview {
		None => Some(NativePreview::Lines(DEFAULT_TERMINAL_PREVIEW_LINES)),
		Some(NativePreview::None | NativePreview::Children) => None,
		Some(NativePreview::Tail(lines)) => Some(NativePreview::Lines(*lines)),
		Some(other) => Some(other.clone()),
	}
}

/// The snapshot tool a returned call may be displaced by.
pub fn displaceable_tool_name(tool_name: &str, details: Option<&Value>, is_error: bool, is_partial: bool) -> Option<DisplaceableToolName> {
	if is_error {
		return None;
	}
	if tool_name == "wait" && is_waiting_poll_json(details) {
		return Some(DisplaceableToolName::Wait);
	}
	if tool_name == "todo" && !is_partial && is_todo_tool_details(details) {
		return Some(DisplaceableToolName::Todo);
	}
	None
}

fn is_todo_tool_details(details: Option<&Value>) -> bool {
	details
		.and_then(|details| details.get("phases"))
		.map(|phases| phases.is_array())
		.unwrap_or(false)
}

/// A `wait` snapshot whose jobs are all still running and nothing was
/// cancelled — the JSON twin of [`crate::tools::wait::is_waiting_poll_details`],
/// which reads the typed payload this card never gets.
pub fn is_waiting_poll_json(details: Option<&Value>) -> bool {
	let Some(details) = details else {
		return false;
	};
	let Some(jobs) = details.get("jobs").and_then(Value::as_array) else {
		return false;
	};
	if jobs.is_empty() {
		return false;
	}
	if details
		.get("cancelled")
		.and_then(Value::as_array)
		.map(|cancelled| !cancelled.is_empty())
		.unwrap_or(false)
	{
		return false;
	}
	jobs.iter().all(|job| {
		job.get("status")
			.and_then(Value::as_str)
			.map(|status| status == "running")
			.unwrap_or(false)
	})
}

/// Agent ids a task card names, for click-to-focus.
pub fn task_card_agent_ids(details: Option<&Value>) -> Vec<String> {
	let Some(agents) = details.and_then(|details| details.get("agents")).and_then(Value::as_array) else {
		return Vec::new();
	};
	agents
		.iter()
		.filter_map(|agent| {
			agent
				.get("id")
				.and_then(Value::as_str)
				.or_else(|| agent.get("agentId").and_then(Value::as_str))
				.map(str::to_string)
		})
		.collect()
}

/// The display paths a call targets (`path`, `file_path`, `edits[].path`,
/// `files[].path`).
pub fn target_paths(args: Option<&Value>) -> Vec<String> {
	let Some(args) = args.and_then(Value::as_object) else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut add = |value: Option<&Value>| {
		if let Some(path) = value.and_then(Value::as_str).filter(|path| !path.is_empty()) {
			out.push(display_path(path));
		}
	};
	add(args.get("path"));
	add(args.get("file_path"));
	for key in ["edits", "files"] {
		if let Some(list) = args.get(key).and_then(Value::as_array) {
			for entry in list {
				add(entry.get("path"));
			}
		}
	}
	out
}

/// Whether a call targets `file_path`.
pub fn matches_path(args: Option<&Value>, file_path: &str) -> bool {
	let wanted = display_path(file_path);
	target_paths(args).iter().any(|path| *path == wanted)
}

/// Diagnostics for one file, matched to a call by its path.
#[derive(Clone, Debug)]
pub struct LateDiagnosticFile {
	/// The file the diagnostics belong to.
	pub path: String,
	/// The LSP report.
	pub diagnostics: FileDiagnosticsResult,
}

/// A live-or-replayed tool card.
pub struct ToolExecutionComponent {
	theme: Theme,
	tool_name: String,
	tool_label: String,
	tool_call_id: Option<String>,
	args: Option<Value>,
	result: Option<ToolResultFrame>,
	renderer: GuardedRenderer,
	options: ToolExecutionOptions,
	edit_mode: Option<EditMode>,
	expanded: bool,
	is_partial: bool,
	args_complete: bool,
	execution_started: bool,
	sealed: bool,
	parked_background: bool,
	tool_activity_visible: bool,
	block_version: u64,
	displaceable_by: Option<DisplaceableToolName>,
	late_diagnostics: Vec<LateDiagnosticFile>,
	last_focus_agents: Vec<String>,
}

impl ToolExecutionComponent {
	/// A pending call of `tool_name`.
	pub fn new(
		tool_name: impl Into<String>,
		args: Option<Value>,
		options: ToolExecutionOptions,
		renderer: Option<Box<dyn ToolViewRenderer>>,
		theme: Theme,
		tool_call_id: Option<String>,
	) -> Self {
		let tool_name = tool_name.into();
		let edit_mode = resolve_edit_mode_for_tool(&tool_name, None);
		ToolExecutionComponent {
			theme,
			tool_label: tool_name.clone(),
			tool_name,
			tool_call_id,
			args,
			result: None,
			renderer: GuardedRenderer::new(renderer),
			options,
			edit_mode,
			expanded: false,
			is_partial: true,
			args_complete: false,
			execution_started: false,
			sealed: false,
			parked_background: false,
			tool_activity_visible: true,
			block_version: 0,
			displaceable_by: None,
			late_diagnostics: Vec::new(),
			last_focus_agents: Vec::new(),
		}
	}

	/// The wire tool name.
	pub fn tool_name(&self) -> &str {
		&self.tool_name
	}

	/// The renderer key when the built-in registry is allowed.
	pub fn renderer_key(&self) -> Option<&str> {
		if self.options.use_built_in_renderer {
			Some(&self.tool_name)
		} else {
			None
		}
	}

	/// The call's id.
	pub fn tool_call_id(&self) -> Option<&str> {
		self.tool_call_id.as_deref()
	}

	/// The current arguments.
	pub fn args(&self) -> Option<&Value> {
		self.args.as_ref()
	}

	/// The current result frame.
	pub fn result(&self) -> Option<&ToolResultFrame> {
		self.result.as_ref()
	}

	/// The edit mode this call streams with.
	pub fn edit_mode(&self) -> Option<EditMode> {
		self.edit_mode
	}

	/// Whether the call is expanded.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// Whether args are still streaming.
	pub fn args_complete(&self) -> bool {
		self.args_complete
	}

	/// Whether a result frame exists and is still streaming.
	pub fn is_partial(&self) -> bool {
		self.is_partial
	}

	/// Whether the block was sealed as final history.
	pub fn is_sealed(&self) -> bool {
		self.sealed
	}

	/// Whether the call is parked on a background job.
	pub fn is_parked_background(&self) -> bool {
		self.parked_background
	}

	/// Post-finalize mutation counter.
	pub fn transcript_block_version(&self) -> u64 {
		self.block_version
	}

	/// Whether hidden from the transcript.
	pub fn tool_activity_visible(&self) -> bool {
		self.tool_activity_visible
	}

	/// Hide or show the card with transcript-wide tool activity.
	pub fn set_tool_activity_visible(&mut self, visible: bool) {
		self.tool_activity_visible = visible;
	}

	/// Whether result images are requested.
	pub fn show_images(&self) -> bool {
		self.options.show_images
	}

	/// Ask for result images.
	pub fn set_show_images(&mut self, show: bool) {
		self.options.show_images = show;
		self.block_version += 1;
	}

	/// The status the head shows.
	pub fn native_status(&self) -> CardStatus {
		if self.result.is_none() {
			return if self.sealed {
				CardStatus::Cancelled
			} else if self.execution_started || self.args_complete {
				CardStatus::Running
			} else {
				CardStatus::Pending
			};
		}
		if self.is_partial {
			return CardStatus::Running;
		}
		if self.result.as_ref().map(|result| result.is_error).unwrap_or(false) {
			CardStatus::Error
		} else {
			CardStatus::Done
		}
	}

	/// Whether the card must be held back: pending images or figures. The
	/// crate has neither here, so only a streaming result holds it.
	pub fn is_transcript_block_pending(&self) -> bool {
		self.tool_activity_visible && self.is_partial && self.result.is_some()
	}

	/// Whether the block is ready to retire as immutable history.
	pub fn is_transcript_block_finalized(&self) -> bool {
		if !self.tool_activity_visible || self.sealed {
			return true;
		}
		if self.result.is_none() {
			return false;
		}
		if self.parked_background {
			return true;
		}
		!self.is_partial
	}

	/// A supersedable snapshot that has not been sealed.
	pub fn is_displaceable_block(&self) -> bool {
		self.displaceable_by.is_some() && !self.sealed
	}

	/// Whether `next_tool_name` replaces this snapshot.
	pub fn can_be_displaced_by(&self, next_tool_name: Option<&str>) -> bool {
		self.displaceable_by.is_some()
			&& self.displaceable_by.map(DisplaceableToolName::as_str) == next_tool_name
			&& !self.sealed
	}

	/// Subagent ids this card names (task cards only).
	pub fn click_focus_agent_ids(&self) -> &[String] {
		&self.last_focus_agents
	}

	/// The resolved presentation. No presenter registry exists in this port, so
	/// every card is full.
	pub fn presentation(&self) -> &ResolvedPresentation {
		&ResolvedPresentation::Full
	}

	/// Whether a compact card is drawn.
	pub fn is_transcript_block_compact(&self) -> bool {
		false
	}

	/// Attach LSP diagnostics that arrived after the result.
	pub fn attach_late_diagnostics(&mut self, files: Vec<LateDiagnosticFile>) -> bool {
		let mine: Vec<_> = files
			.into_iter()
			.filter(|file| matches_path(self.args.as_ref(), &file.path))
			.collect();
		if mine.is_empty() {
			return false;
		}
		self.late_diagnostics = mine;
		self.block_version += 1;
		true
	}

	/// Diagnostics attached after the result.
	pub fn late_diagnostics(&self) -> &[LateDiagnosticFile] {
		&self.late_diagnostics
	}

	/// Re-ask the presenters; always false here (see [`Self::presentation`]).
	pub fn refresh_transcript_presentation(&mut self, _ids: Option<&HashSet<String>>) -> bool {
		false
	}

	/// The streamed call's id changed.
	pub fn rename_tool_call_id(&mut self, tool_call_id: &str) {
		if self.tool_call_id.as_deref() == Some(tool_call_id) {
			return;
		}
		self.tool_call_id = Some(tool_call_id.to_string());
		self.block_version += 1;
	}

	/// The activity summary the transcript's compact card draws.
	pub fn activity_summary(&self) -> (String, Option<String>) {
		let detail = self
			.args
			.as_ref()
			.and_then(|args| {
				args.get("path")
					.or_else(|| args.get("command"))
					.or_else(|| args.get("query"))
					.or_else(|| args.get("pattern"))
			})
			.and_then(Value::as_str)
			.map(str::to_string);
		(self.tool_label.clone(), detail)
	}

	/// The result's text, images omitted.
	pub fn result_text(&self) -> String {
		self.result
			.as_ref()
			.map(|result| result_text(&result.as_render_result()))
			.unwrap_or_default()
	}

	/// The renderer-specific view when one answers, else the crate's default.
	fn tool_view(&self) -> NativeToolView {
		let result = self.result.as_ref().map(ToolResultFrame::as_render_result);
		if let Some(result) = result.as_ref() {
			if let Some(merged) = self.renderer.merge_call_and_result(self.args.as_ref(), result) {
				return merged;
			}
			let options = RenderResultOptions {
				expanded: self.expanded,
				is_partial: self.is_partial,
				..RenderResultOptions::default()
			};
			let view = self.renderer.describe_result(result, &options);
			if view.head.is_some() || view.body.is_some() || view.tool.is_some() {
				return view;
			}
		}
		let call_view = self.renderer.describe_call(self.args.as_ref());
		if call_view.head.is_some() || call_view.tool.is_some() {
			return call_view;
		}
		describe_default_tool_execution(&DefaultToolRenderInput {
			label: self.tool_label.clone(),
			args: self.args.clone(),
			result: self.result.as_ref().map(|result| crate::tools::default_renderer::DefaultToolRenderResult {
				output: self.result_text(),
				is_error: result.is_error,
				skipped: false,
			}),
			options: DefaultRenderOptions {
				expanded: self.expanded,
				is_partial: self.is_partial,
				spinner_frame: None,
				json: false,
			},
		})
	}

	/// Preview rows per file while collapsed.
	fn preview_lines(&self) -> usize {
		if self.expanded {
			usize::MAX
		} else {
			crate::render::render_utils::PREVIEW_LIMITS.OUTPUT_COLLAPSED
		}
	}

	fn header_plain(&self) -> String {
		let icon = format_status_icon(self.native_status().ui_status(), &self.theme, None);
		let view = self.tool_view();
		let head = match view.head {
			Some(head) => head.plain_text(),
			None => self
				.args
				.as_ref()
				.map(|args| format!("{} {}", self.tool_label, args))
				.unwrap_or_else(|| self.tool_label.clone()),
		};
		format!("{icon} {head}")
	}
}

impl Component for ToolExecutionComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		if !self.tool_activity_visible {
			return Vec::new();
		}
		let width = width.max(1) as usize;
		let mut rows = vec![truncate_to_width(&self.header_plain(), width, Ellipsis::Omit)];
		let output = self.result_text();
		for line in output.lines().take(self.preview_lines()) {
			rows.push(truncate_to_width(&crate::render::render_utils::replace_tabs(line), width, Ellipsis::Omit));
		}
		rows
	}

	fn invalidate(&mut self) {
		self.block_version += 1;
	}
}

impl ToolExecutionHandle for ToolExecutionComponent {
	fn update_args(&mut self, args: Option<Value>, tool_call_id: Option<&str>) {
		self.args = args;
		if let Some(tool_call_id) = tool_call_id {
			self.rename_tool_call_id(tool_call_id);
		}
		self.block_version += 1;
		self.last_focus_agents = task_card_agent_ids(self.result.as_ref().and_then(|result| result.details.as_ref()));
		self.displaceable_by = displaceable_tool_name(
			&self.tool_name,
			self.result.as_ref().and_then(|result| result.details.as_ref()),
			self.result.as_ref().map(|result| result.is_error).unwrap_or(false),
			self.is_partial,
		);
	}

	fn update_stream_preview(&mut self, _update: Option<Value>) {
		self.block_version += 1;
	}

	fn update_result(&mut self, result: ToolResultFrame, is_partial: bool, tool_call_id: Option<&str>) {
		self.is_partial = is_partial;
		if let Some(tool_call_id) = tool_call_id {
			self.rename_tool_call_id(tool_call_id);
		}
		self.last_focus_agents = task_card_agent_ids(result.details.as_ref());
		self.displaceable_by = displaceable_tool_name(
			&self.tool_name,
			result.details.as_ref(),
			result.is_error,
			is_partial,
		);
		self.result = Some(result);
		self.block_version += 1;
	}

	fn set_args_complete(&mut self, _tool_call_id: Option<&str>) {
		if !self.args_complete {
			self.args_complete = true;
			self.block_version += 1;
		}
	}

	fn set_execution_started(&mut self, _tool_call_id: Option<&str>) {
		if !self.execution_started {
			self.execution_started = true;
			self.block_version += 1;
		}
	}

	fn set_expanded(&mut self, expanded: bool) {
		if self.expanded != expanded {
			self.block_version += 1;
		}
		self.expanded = expanded;
	}

	fn park_as_background(&mut self) {
		self.parked_background = true;
	}

	fn seal(&mut self) {
		if self.sealed {
			return;
		}
		self.sealed = true;
		self.block_version += 1;
		self.displaceable_by = None;
	}
}

impl crate::native::describe::NativeDescribe for ToolExecutionComponent {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		if !self.tool_activity_visible {
			return card(Props::new(), Vec::new());
		}
		let status = self.native_status();
		let view = self.tool_view();
		let mut props = Props::new();
		props.set_str("role", format!("omp.tool.{}", self.tool_name));
		props.set_str("name", self.tool_name.clone());
		if let Some(tone) = view.tone.or_else(|| status.tone()) {
			props.set_tone(tone);
		}
		let head = view.head.unwrap_or_else(|| {
			NodeText::Spans(vec![
				span(self.tool_label.clone(), Some("toolTitle strong")),
				span(" ", None),
			])
		});
		props.set_head(head);
		if let Some(preview) = card_preview(view.preview.as_ref()) {
			match preview {
				NativePreview::Lines(lines) => props.set_i64("preview", lines as i64),
				NativePreview::Tail(lines) => props.set_i64("preview", lines as i64),
				_ => {}
			}
		}
		props.set_bool("collapsible", true);
		props.set_bool("collapsed", !self.expanded);
		if view.open == Some(true) {
			props.set_bool("collapsed", false);
		}
		if view.inline == Some(true) {
			props.set_str("frame", "inline");
		}
		let mut body = view.body.unwrap_or_default();
		for file in &self.late_diagnostics {
			if let Some(section) = diagnostics_section(
				Some(&file.diagnostics),
				Some(DiagnosticsSectionOpts { with_path: true }),
			) {
				body.push(keyed(section, format!("late-diagnostics:{}", file.path)));
			}
		}
		let mut described = card(props, body);
		described = node(NodeKind::Tool, described.props, described.children, None);
		let _ = cx;
		described
	}
}

/// A column of cards, matching the reference's container order.
pub fn tool_cards(children: Vec<NativeNode>) -> NativeNode {
	col(children, Props::new())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::native::describe::NativeDescribe;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};
	use crate::theme::theme_class::ColorMode;
	use serde_json::json;

	fn theme() -> Theme {
		create_theme(
			builtin_theme_json("dark").unwrap(),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.unwrap()
	}

	fn card_for(tool: &str) -> ToolExecutionComponent {
		ToolExecutionComponent::new(
			tool,
			Some(json!({"path": "src/main.rs"})),
			ToolExecutionOptions::default(),
			None,
			theme(),
			Some("call-1".to_string()),
		)
	}

	#[test]
	fn job_gates_and_states() {
		assert_eq!(async_job_state(Some(&json!({"async": {"state": "running"}}))), Some("running"));
		assert_eq!(async_job_state(Some(&json!({}))), None);
		assert!(is_settled_job_frame(Some(&json!({"async": {"state": "failed"}}))));
		assert!(!is_settled_job_frame(Some(&json!({"async": {"state": "running"}}))));
		assert!(parks_as_background("task", Some(&json!({"async": {"state": "running"}})), false));
		assert!(!parks_as_background("bash", Some(&json!({"async": {"state": "running"}})), false));
		assert!(parks_as_background("bash", Some(&json!({"async": {"state": "running"}})), true));
		assert!(!parks_as_background("read", Some(&json!({"async": {"state": "running"}})), true));
	}

	#[test]
	fn settled_job_frames_land_on_the_card() {
		let mut card = card_for("task");
		card.park_as_background();
		assert!(card.is_parked_background());
		settle_with_job_frame(
			&mut card,
			ToolResultFrame {
				details: Some(json!({"async": {"state": "failed"}})),
				..ToolResultFrame::default()
			},
			"call-1",
		);
		assert_eq!(card.native_status(), CardStatus::Error);

		let mut cancelled = card_for("task");
		settle_with_job_frame(
			&mut cancelled,
			ToolResultFrame {
				details: Some(json!({"async": {"state": "cancelled"}})),
				..ToolResultFrame::default()
			},
			"call-1",
		);
		assert!(cancelled.is_sealed());
		assert!(cancelled.is_partial(), "a cancelled job keeps its partial output");
	}

	#[test]
	fn displacement_only_inside_the_same_snapshot_family() {
		let wait_details = Some(json!({"jobs": [{"status": "running"}], "cancelled": []}));
		let todo_details = Some(json!({"phases": []}));
		let mut wait = card_for("wait");
		wait.update_result(
			ToolResultFrame {
				details: wait_details,
				..ToolResultFrame::default()
			},
			false,
			None,
		);
		assert!(wait.is_displaceable_block());
		assert!(wait.can_be_displaced_by(Some("wait")));
		assert!(!wait.can_be_displaced_by(Some("todo")));

		let mut todo = card_for("todo");
		todo.update_result(
			ToolResultFrame {
				details: todo_details,
				..ToolResultFrame::default()
			},
			true,
			None,
		);
		assert!(!todo.is_displaceable_block(), "partial snapshots are not displaced");
	}

	#[test]
	fn sealing_and_finality_follow_the_result() {
		let mut card = card_for("read");
		assert_eq!(card.native_status(), CardStatus::Pending);
		assert!(!card.is_transcript_block_finalized());
		card.set_execution_started(None);
		assert_eq!(card.native_status(), CardStatus::Running);
		card.update_result(
			ToolResultFrame {
				content: vec![ToolResultBlock {
					kind: "text".to_string(),
					text: Some("fn main() {}".to_string()),
					..ToolResultBlock::default()
				}],
				..ToolResultFrame::default()
			},
			false,
			None,
		);
		assert_eq!(card.native_status(), CardStatus::Done);
		assert!(card.is_transcript_block_finalized());
		assert!(card.result_text().contains("fn main"), "{}", card.result_text());
		card.seal();
		assert!(card.is_sealed());
		assert!(!card.is_displaceable_block());
	}

	#[test]
	fn paths_route_late_diagnostics() {
		let card = card_for("edit");
		assert!(card.args().is_some());
		assert!(matches_path(card.args(), "src/main.rs"));
		assert_eq!(target_paths(Some(&json!({"edits": [{"path": "src/lib.rs"}]}))), vec![display_path("src/lib.rs")]);
		assert_eq!(resolve_edit_mode_for_tool("apply_patch", None), Some(EditMode::ApplyPatch));
		assert_eq!(resolve_edit_mode_for_tool("read", None), None);
		assert!(streams_body("write"));
		assert!(!streams_body("read"));
	}

	#[test]
	fn native_card_is_a_tool_node_with_the_name() {
		let mut card = card_for("bash");
		card.set_expanded(true);
		let described = card.describe_native(&DescribeContext::new(80));
		assert_eq!(described.kind, NodeKind::Tool);
		assert_eq!(described.props.get_str("name"), Some("bash"));
		assert_eq!(described.props.get_bool("collapsed"), Some(false));
	}

	#[test]
	fn hidden_cards_draw_nothing() {
		let mut card = card_for("grep");
		card.set_tool_activity_visible(false);
		assert!(card.render(80).is_empty());
		assert!(card.is_transcript_block_finalized());
	}
}
