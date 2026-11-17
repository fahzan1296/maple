//! Anchored `/cleanse` overlay model and presentation helpers.
//!
//! Reference: `l/src/overlays/cleanse-panel.ts`.
//!
//! Deviations:
//! - `CleansePanelComponent` extends `OverlayPanel` and uses TUI components,
//!   timers, theme rendering, and native describe primitives unavailable here.
//!   This module ports constants, outcome types/options, log buffering, model-
//!   driven state transitions, and presentation/footer logic as host-renderable
//!   data.
//! - `AgentProgress` and cleanse board types are modeled locally as narrow data
//!   traits/structs; host adapts runtime board/progress implementations.
//! - Spinner/timer repaint is represented as `tick_frame()` instead of
//!   `setInterval`; native mode skips timer by host choice.

pub const SPINNER_INTERVAL_MS: u64 = 80;
pub const MAX_LOG_LINES: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleansePanelRunStatus {
	Clean,
	Unresolved,
	Unsupported,
	Cancelled,
}

/// Terminal state of the run, mirrored into footer once core settles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleansePanelOutcome {
	Clean,
	Unresolved,
	Unsupported,
	Cancelled,
	Error,
}

impl From<CleansePanelRunStatus> for CleansePanelOutcome {
	fn from(value: CleansePanelRunStatus) -> Self {
		match value {
			CleansePanelRunStatus::Clean => Self::Clean,
			CleansePanelRunStatus::Unresolved => Self::Unresolved,
			CleansePanelRunStatus::Unsupported => Self::Unsupported,
			CleansePanelRunStatus::Cancelled => Self::Cancelled,
		}
	}
}

/// Tone of finished sheet per outcome.
#[must_use]
pub fn outcome_tone(outcome: CleansePanelOutcome) -> &'static str {
	match outcome {
		CleansePanelOutcome::Clean => "success",
		CleansePanelOutcome::Unresolved
		| CleansePanelOutcome::Unsupported
		| CleansePanelOutcome::Cancelled => "warning",
		CleansePanelOutcome::Error => "error",
	}
}

/// Minimal checker descriptor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanseCheckerDescriptor {
	pub id: String,
	pub label: String,
}

/// Minimal check result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanseCheckResult {
	pub checker_id: String,
	pub ok: bool,
	pub summary: Option<String>,
}

/// Repair assignment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanseAssignment {
	pub agent: String,
	pub summary: String,
}

/// Agent outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanseAgentOutcome {
	pub agent: String,
	pub ok: bool,
	pub summary: Option<String>,
}

/// Agent progress snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentProgress {
	pub status: Option<String>,
	pub message: Option<String>,
}

/// Live board model contract consumed by panel.
pub trait CleanseBoardModel {
	fn phase(&self, text: Option<&str>);
	fn checker_started(&self, checker: &CleanseCheckerDescriptor);
	/// Returns a display line for settled checker.
	fn checker_finished(&self, check: &CleanseCheckResult, duration_ms: u64) -> String;
	fn repair_finished(&self);
	fn agent_started(&self, name: &str, assignment: &CleanseAssignment);
	fn agent_progress(&self, name: &str, progress: &AgentProgress);
	/// Returns a display line for settled agent.
	fn agent_finished(&self, outcome: &CleanseAgentOutcome, assignment: &CleanseAssignment) -> String;
	fn render_live(&self, spinner_frame: &str) -> Vec<String>;
}

/// Panel options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleansePanelOptions {
	pub request: Option<String>,
}

/// A semantic/native log entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanseLogEntry {
	pub seq: u64,
	pub text: String,
	pub tone: Option<&'static str>,
}

/// Footer status/hint model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleansePanelFooter {
	pub status: Option<String>,
	pub tone: Option<&'static str>,
	pub hint: String,
}

/// Presentation snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleansePanelPresentation {
	pub title: String,
	pub log_lines: Vec<String>,
	pub live_lines: Vec<String>,
	pub error_message: Option<String>,
	pub footer: CleansePanelFooter,
	pub tone: Option<&'static str>,
}

/// `/cleanse` overlay state machine/model.
pub struct CleansePanelState<M: CleanseBoardModel> {
	pub model: M,
	pub request: Option<String>,
	log_lines: Vec<String>,
	native_log: Vec<CleanseLogEntry>,
	log_seq: u64,
	outcome: Option<CleansePanelOutcome>,
	error_message: Option<String>,
	frame: usize,
	live_closed: bool,
}

impl<M: CleanseBoardModel> CleansePanelState<M> {
	#[must_use]
	pub fn new(model: M, request: Option<String>) -> Self {
		Self {
			model,
			request,
			log_lines: Vec::new(),
			native_log: Vec::new(),
			log_seq: 0,
			outcome: None,
			error_message: None,
			frame: 0,
			live_closed: false,
		}
	}

	#[must_use]
	pub fn title(&self) -> String {
		match &self.request {
			Some(req) => format!("/cleanse {}", req.replace('\t', "    ")),
			None => "/cleanse".to_string(),
		}
	}

	pub fn log(&mut self, line: &str) {
		self.push_log(line.replace('\t', "    "), None);
	}

	pub fn log_error(&mut self, line: &str) {
		self.push_log(line.replace('\t', "    "), Some("error"));
	}

	fn push_log(&mut self, line: String, tone: Option<&'static str>) {
		self.log_lines.push(line.clone());
		if self.log_lines.len() > MAX_LOG_LINES {
			let drop = self.log_lines.len() - MAX_LOG_LINES;
			self.log_lines.drain(0..drop);
		}
		self.native_log.push(CleanseLogEntry {
			seq: self.log_seq,
			text: line,
			tone,
		});
		self.log_seq += 1;
		if self.native_log.len() > MAX_LOG_LINES {
			let drop = self.native_log.len() - MAX_LOG_LINES;
			self.native_log.drain(0..drop);
		}
	}

	pub fn phase(&mut self, text: Option<&str>) {
		self.model.phase(text);
	}

	pub fn checker_started(&mut self, checker: &CleanseCheckerDescriptor) {
		self.model.checker_started(checker);
	}

	pub fn checker_finished(&mut self, check: &CleanseCheckResult, duration_ms: u64) {
		let line = self.model.checker_finished(check, duration_ms);
		self.push_log(line, None);
	}

	pub fn repair_finished(&mut self) {
		self.model.repair_finished();
	}

	pub fn agent_started(&mut self, name: &str, assignment: &CleanseAssignment) {
		self.model.agent_started(name, assignment);
	}

	pub fn agent_progress(&mut self, name: &str, progress: &AgentProgress) {
		self.model.agent_progress(name, progress);
	}

	pub fn agent_finished(&mut self, outcome: &CleanseAgentOutcome, assignment: &CleanseAssignment) {
		let line = self.model.agent_finished(outcome, assignment);
		self.push_log(line, None);
	}

	pub fn close(&mut self) {
		self.live_closed = true;
	}

	pub fn finish(&mut self, status: CleansePanelRunStatus) {
		self.outcome = Some(status.into());
		self.close();
	}

	pub fn mark_error(&mut self, message: &str) {
		self.outcome = Some(CleansePanelOutcome::Error);
		self.error_message = Some(message.to_string());
		self.close();
	}

	pub fn tick_frame(&mut self, spinner_frame_count: usize) {
		if spinner_frame_count > 0 {
			self.frame = (self.frame + 1) % spinner_frame_count;
		}
	}

	#[must_use]
	pub fn outcome(&self) -> Option<CleansePanelOutcome> {
		self.outcome
	}

	#[must_use]
	pub fn error_message(&self) -> Option<&str> {
		self.error_message.as_deref()
	}

	#[must_use]
	pub fn log_entries(&self) -> &[CleanseLogEntry] {
		&self.native_log
	}

	#[must_use]
	pub fn footer(&self) -> CleansePanelFooter {
		match self.outcome {
			None => CleansePanelFooter {
				status: None,
				tone: None,
				hint: "esc cancel /cleanse".into(),
			},
			Some(CleansePanelOutcome::Clean) => CleansePanelFooter {
				status: Some("Clean".into()),
				tone: Some("success"),
				hint: "esc dismiss".into(),
			},
			Some(CleansePanelOutcome::Unresolved) => CleansePanelFooter {
				status: Some("Diagnostics remain".into()),
				tone: Some("warning"),
				hint: "esc dismiss".into(),
			},
			Some(CleansePanelOutcome::Unsupported) => CleansePanelFooter {
				status: Some("No runnable checker".into()),
				tone: Some("warning"),
				hint: "esc dismiss".into(),
			},
			Some(CleansePanelOutcome::Cancelled) => CleansePanelFooter {
				status: Some("Cancelled".into()),
				tone: Some("warning"),
				hint: "esc dismiss".into(),
			},
			Some(CleansePanelOutcome::Error) => CleansePanelFooter {
				status: Some("Error".into()),
				tone: Some("error"),
				hint: "esc dismiss".into(),
			},
		}
	}

	#[must_use]
	pub fn presentation(&self, spinner_frames: &[&str]) -> CleansePanelPresentation {
		let spinner = spinner_frames.get(self.frame % spinner_frames.len().max(1)).copied().unwrap_or("");
		let live_lines = if self.live_closed {
			Vec::new()
		} else {
			self.model.render_live(spinner)
		};
		CleansePanelPresentation {
			title: self.title(),
			log_lines: self.log_lines.clone(),
			live_lines,
			error_message: self.error_message.clone(),
			footer: self.footer(),
			tone: self.outcome.map(outcome_tone),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;

	#[derive(Default)]
	struct MockBoard {
		phases: RefCell<Vec<String>>,
	}
	impl CleanseBoardModel for MockBoard {
		fn phase(&self, text: Option<&str>) {
			self.phases.borrow_mut().push(text.unwrap_or("").into());
		}
		fn checker_started(&self, _checker: &CleanseCheckerDescriptor) {}
		fn checker_finished(&self, check: &CleanseCheckResult, _duration_ms: u64) -> String {
			format!("check:{}", check.checker_id)
		}
		fn repair_finished(&self) {}
		fn agent_started(&self, _name: &str, _assignment: &CleanseAssignment) {}
		fn agent_progress(&self, _name: &str, _progress: &AgentProgress) {}
		fn agent_finished(&self, outcome: &CleanseAgentOutcome, _assignment: &CleanseAssignment) -> String {
			format!("agent:{}", outcome.agent)
		}
		fn render_live(&self, spinner_frame: &str) -> Vec<String> {
			vec![format!("live {spinner_frame}")]
		}
	}

	#[test]
	fn log_buffer_caps_and_tracks_tone() {
		let mut p = CleansePanelState::new(MockBoard::default(), Some("fix".into()));
		for i in 0..(MAX_LOG_LINES + 3) {
			p.log(&format!("l{i}"));
		}
		assert_eq!(p.log_lines.len(), MAX_LOG_LINES);
		assert_eq!(p.log_lines.first().unwrap(), "l3");
		p.log_error("boom");
		assert_eq!(p.log_entries().last().unwrap().tone, Some("error"));
		assert_eq!(p.title(), "/cleanse fix");
	}

	#[test]
	fn outcomes_and_footer_presentation() {
		let mut p = CleansePanelState::new(MockBoard::default(), None);
		p.phase(Some("scan"));
		p.tick_frame(4);
		let snap = p.presentation(&["-", "\\", "|", "/"]);
		assert_eq!(snap.live_lines, vec!["live \\".to_string()]);
		assert_eq!(snap.footer.hint, "esc cancel /cleanse");
		p.finish(CleansePanelRunStatus::Clean);
		let snap = p.presentation(&["-"]);
		assert!(snap.live_lines.is_empty());
		assert_eq!(snap.footer.status.as_deref(), Some("Clean"));
		assert_eq!(outcome_tone(p.outcome().unwrap()), "success");
	}

	#[test]
	fn checker_agent_lines_and_error() {
		let mut p = CleansePanelState::new(MockBoard::default(), None);
		p.checker_finished(&CleanseCheckResult { checker_id: "fmt".into(), ok: false, summary: None }, 10);
		p.agent_finished(&CleanseAgentOutcome { agent: "a".into(), ok: true, summary: None }, &CleanseAssignment::default());
		assert_eq!(p.log_lines, vec!["check:fmt".to_string(), "agent:a".to_string()]);
		p.mark_error("bad");
		assert_eq!(p.outcome(), Some(CleansePanelOutcome::Error));
		assert_eq!(p.error_message(), Some("bad"));
		assert_eq!(p.footer().tone, Some("error"));
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Host callbacks for cleanse panel dismissal/render requests.
pub trait CleansePanelCallbacks {
	fn on_dismiss(&self);
	fn request_render(&self) {}
}

/// Anchored `/cleanse` overlay component.
///
/// Reference: `CleansePanelComponent` in `l/src/overlays/cleanse-panel.ts`.
///
/// Deviations: `OverlayPanel`/`StreamingPanelContent`, live TUI timers, and
/// native chrome integration are reduced to a state-driven bordered panel backed
/// by [`CleansePanelState`].
pub struct CleansePanelComponent<M: CleanseBoardModel, C: CleansePanelCallbacks> {
	pub panel: CleansePanelState<M>,
	callbacks: C,
	theme: OverlayTheme,
}

impl<M: CleanseBoardModel, C: CleansePanelCallbacks> std::fmt::Debug for CleansePanelComponent<M, C> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("CleansePanelComponent")
			.field("outcome", &self.panel.outcome())
			.finish_non_exhaustive()
	}
}

impl<M: CleanseBoardModel, C: CleansePanelCallbacks> CleansePanelComponent<M, C> {
	#[must_use]
	pub fn new(model: M, request: Option<String>, callbacks: C) -> Self {
		Self {
			panel: CleansePanelState::new(model, request),
			callbacks,
			theme: OverlayTheme::default(),
		}
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let snap = self.panel.presentation(&["-", "\\", "|", "/"]);
		let mut out = vec![top_border(width, &snap.title, &self.theme)];
		for l in &snap.log_lines {
			out.push(row(l, width, &self.theme));
		}
		for l in &snap.live_lines {
			out.push(row(l, width, &self.theme));
		}
		if let Some(err) = &snap.error_message {
			out.push(row(err, width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		let footer = if let Some(status) = snap.footer.status {
			format!("{status} · {}", snap.footer.hint)
		} else {
			snap.footer.hint
		};
		out.push(row(&footer, width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<M: CleanseBoardModel, C: CleansePanelCallbacks> Component for CleansePanelComponent<M, C> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.panel.tick_frame(4);
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if keys::is_escape(key) {
			if self.panel.outcome().is_none() {
				self.panel.finish(CleansePanelRunStatus::Cancelled);
				self.callbacks.request_render();
			} else {
				self.callbacks.on_dismiss();
			}
		}
	}

	fn invalidate(&mut self) {}
}

impl<M: CleanseBoardModel, C: CleansePanelCallbacks> NativeDescribe for CleansePanelComponent<M, C> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_rows(cx.cols.max(20));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert(
			"outcome",
			serde_json::Value::String(
				self.panel
					.outcome()
					.map(|o| format!("{o:?}"))
					.unwrap_or_else(|| "running".to_string()),
			),
		);
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
	use std::cell::RefCell;

	#[derive(Default)]
	struct Board;
	impl CleanseBoardModel for Board {
		fn phase(&self, _text: Option<&str>) {}
		fn checker_started(&self, _checker: &CleanseCheckerDescriptor) {}
		fn checker_finished(&self, check: &CleanseCheckResult, _duration_ms: u64) -> String { check.checker_id.clone() }
		fn repair_finished(&self) {}
		fn agent_started(&self, _name: &str, _assignment: &CleanseAssignment) {}
		fn agent_progress(&self, _name: &str, _progress: &AgentProgress) {}
		fn agent_finished(&self, outcome: &CleanseAgentOutcome, _assignment: &CleanseAssignment) -> String { outcome.agent.clone() }
		fn render_live(&self, _spinner_frame: &str) -> Vec<String> { vec!["live".into()] }
	}
	struct Cb { dismissed: RefCell<bool> }
	impl CleansePanelCallbacks for Cb { fn on_dismiss(&self) { *self.dismissed.borrow_mut() = true; } fn request_render(&self) {} }

	#[test]
	fn cleanse_panel_component_renders_and_handles_escape() {
		let mut c = CleansePanelComponent::new(Board, Some("fix".into()), Cb { dismissed: RefCell::new(false) });
		let lines = c.render(60);
		assert!(lines.iter().any(|l| l.contains("/cleanse fix")));
		c.handle_input(&Key::Esc); // cancel run
		assert_eq!(c.panel.outcome(), Some(CleansePanelOutcome::Cancelled));
		c.handle_input(&Key::Esc); // dismiss
		assert!(*c.callbacks.dismissed.borrow());
		let node = c.describe_native(&DescribeContext { cols: 40, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
