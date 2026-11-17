//! Vibe (worker-session) tool render-model types and helpers.
//!
//! Reference: `l/src/tools/vibe.ts`.
//!
//! Deviations:
//! - TS renders live TUI components (mini composer, TV wall cards, shimmer,
//!   spinners, width-aware truncation, native describe nodes/memos, animated
//!   components). Rust ports the operation/session/outcome data model and pure
//!   text/describe helpers (frame text, composer lines, call labels, state/status
//!   mapping, screen summary lines). Live/animated rendering is host concern for
//!   renderer-completion pass.
//! - Timestamps/durations use plain integers (`u64` ms).

/// Operation represented by a worker-session tool result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VibeOp {
	Spawn,
	Send,
	Wait,
	Kill,
	List,
}

impl VibeOp {
	/// String label.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Spawn => "spawn",
			Self::Send => "send",
			Self::Wait => "wait",
			Self::Kill => "kill",
			Self::List => "list",
		}
	}
}

/// Worker CLI flavors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VibeCli {
	Fast,
	Good,
}

impl VibeCli {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Fast => "fast",
			Self::Good => "good",
		}
	}
}

/// Worker session lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VibeSessionState {
	Starting,
	Running,
	Idle,
	Dead,
}

impl VibeSessionState {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Starting => "starting",
			Self::Running => "running",
			Self::Idle => "idle",
			Self::Dead => "dead",
		}
	}
}

/// Live per-session screen snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VibeScreenSnapshot {
	pub id: String,
	pub cli: Option<VibeCli>,
	pub state: Option<VibeSessionState>,
	pub killed: bool,
	pub model: Option<String>,
	pub turns: u32,
	pub queued: u32,
	pub turn_started_at: Option<u64>,
	pub turn_message: Option<String>,
	pub current_tool: Option<String>,
	pub current_tool_args: Option<String>,
	pub last_intent: Option<String>,
	pub trace: Vec<String>,
	pub output_tail: Vec<String>,
	pub last_activity: Option<String>,
	pub last_activity_at: u64,
}

/// Delivery mode for a worker message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VibeSendMode {
	Turn,
	Steered,
	Queued,
}

impl VibeSendMode {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Turn => "turn",
			Self::Steered => "steered",
			Self::Queued => "queued",
		}
	}
}

/// Delivery mode and optional turn job for a worker message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VibeSendOutcome {
	pub id: String,
	pub mode: Option<VibeSendMode>,
	pub job_id: Option<String>,
}

/// Worker shutdown outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VibeKillOutcome {
	pub id: String,
	pub cancelled_turn: bool,
}

/// Settled wait entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VibeWaitSettled {
	pub id: String,
	pub job_id: String,
	pub status: String,
}

/// Wait outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VibeWaitOutcome {
	pub settled: Vec<VibeWaitSettled>,
	pub still_running: Vec<String>,
	pub timed_out: bool,
	pub waiting: bool,
}

/// Details payload shared by every vibe tool for rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VibeToolDetails {
	pub op: Option<VibeOp>,
	pub screens: Vec<VibeScreenSnapshot>,
	pub hidden_killed: Vec<String>,
	pub spawned_id: Option<String>,
	pub spawned_cli: Option<VibeCli>,
	pub spawned_job_id: Option<String>,
	pub send: Option<VibeSendOutcome>,
	pub wait: Option<VibeWaitOutcome>,
	pub killed: Option<VibeKillOutcome>,
}

/// Vibe render call args.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VibeRenderArgs {
	pub cli: Option<VibeCli>,
	pub prompt: Option<String>,
	pub name: Option<String>,
	pub session: Option<String>,
	pub message: Option<String>,
	pub sessions: Vec<String>,
}

pub const COMPOSER_LINE_MAX: usize = 96;
pub const TV_LINE_MAX: usize = 110;
pub const TV_TRACE_COLLAPSED: usize = 2;
pub const TV_TRACE_EXPANDED: usize = 6;
pub const TV_OUTPUT_COLLAPSED: usize = 1;
pub const TV_OUTPUT_EXPANDED: usize = 3;
pub const CURSOR_GLYPH: &str = "▌";

/// One-line, tab/linebreak-collapsed fragment.
pub fn frame_text(text: &str, max: usize) -> String {
	let s = text.replace(['\t', '\n', '\r'], " ");
	let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
	let count = s.chars().count();
	if count <= max {
		s
	} else if max <= 1 {
		"…".to_string()
	} else {
		let mut out: String = s.chars().take(max - 1).collect();
		out.push('…');
		out
	}
}

/// Composer rows (`>` prompt) for a message.
pub fn composer_rows_plain(message: &str, cursor: bool, expanded: bool) -> Vec<String> {
	let raw_lines: Vec<String> = message
		.lines()
		.filter(|l| !l.trim().is_empty())
		.map(|l| frame_text(l, COMPOSER_LINE_MAX))
		.collect();
	let max_rows = if expanded { 6 } else { 2 };
	let mut visible: Vec<String> = raw_lines.iter().take(max_rows).cloned().collect();
	if visible.is_empty() {
		visible.push(String::new());
	}
	if raw_lines.len() > max_rows {
		if let Some(last) = visible.last_mut() {
			last.push_str(" …");
		}
	} else if cursor {
		if let Some(last) = visible.last_mut() {
			last.push_str(CURSOR_GLYPH);
		}
	}
	visible
		.into_iter()
		.enumerate()
		.map(|(i, l)| if i == 0 { format!("> {l}") } else { format!("  {l}") })
		.collect()
}

/// Untruncated native call label.
pub fn vibe_native_label(op: VibeOp, args: Option<&VibeRenderArgs>) -> String {
	match op {
		VibeOp::Spawn => {
			let cli = args.and_then(|a| a.cli).map(|c| c.as_str()).unwrap_or("?");
			if let Some(name) = args.and_then(|a| a.name.as_deref()).filter(|s| !s.is_empty()) {
				format!("spawn {cli} · {name}")
			} else {
				format!("spawn {cli}")
			}
		}
		VibeOp::Send => format!("send → {}", args.and_then(|a| a.session.as_deref()).unwrap_or("?")),
		VibeOp::Wait => {
			let sessions = args.map(|a| a.sessions.as_slice()).unwrap_or(&[]);
			if sessions.is_empty() {
				"wait on running sessions".to_string()
			} else {
				format!("wait on {}", sessions.join(", "))
			}
		}
		VibeOp::Kill => format!("kill {}", args.and_then(|a| a.session.as_deref()).unwrap_or("?")),
		VibeOp::List => "sessions".to_string(),
	}
}

/// Truncated call description label.
pub fn describe_call(op: VibeOp, args: Option<&VibeRenderArgs>) -> String {
	match op {
		VibeOp::Spawn => {
			let cli = args.and_then(|a| a.cli).map(|c| c.as_str()).unwrap_or("?");
			if let Some(name) = args.and_then(|a| a.name.as_deref()).filter(|s| !s.is_empty()) {
				format!("spawn {cli} · {}", frame_text(name, 40))
			} else {
				format!("spawn {cli}")
			}
		}
		VibeOp::Send => format!("send → {}", frame_text(args.and_then(|a| a.session.as_deref()).unwrap_or("?"), 40)),
		VibeOp::Wait => {
			let sessions = args.map(|a| a.sessions.as_slice()).unwrap_or(&[]);
			if sessions.is_empty() {
				"wait on running sessions".to_string()
			} else {
				format!("wait on {}", frame_text(&sessions.join(", "), 60))
			}
		}
		VibeOp::Kill => format!("kill {}", frame_text(args.and_then(|a| a.session.as_deref()).unwrap_or("?"), 40)),
		VibeOp::List => "sessions".to_string(),
	}
}

/// Status icon token (plain).
pub fn state_icon_token(state: VibeSessionState) -> &'static str {
	match state {
		VibeSessionState::Running => "running",
		VibeSessionState::Starting => "pending",
		VibeSessionState::Idle => "success",
		VibeSessionState::Dead => "aborted",
	}
}

/// State color token (plain).
pub fn state_color_token(state: VibeSessionState) -> &'static str {
	match state {
		VibeSessionState::Running | VibeSessionState::Starting => "accent",
		VibeSessionState::Idle => "success",
		VibeSessionState::Dead => "muted",
	}
}

/// Plain TV screen header summary line.
pub fn tv_screen_header_line(screen: &VibeScreenSnapshot, settled_status: Option<&str>) -> String {
	let cli = screen.cli.map(|c| c.as_str()).unwrap_or("?");
	let state = settled_status
		.map(|s| s.to_string())
		.or_else(|| screen.state.map(|s| s.as_str().to_string()))
		.unwrap_or_else(|| "unknown".to_string());
	let turns = if screen.queued > 0 {
		format!("{}t+{}q", screen.turns, screen.queued)
	} else {
		format!("{}t", screen.turns)
	};
	let mut parts = vec![format!("[{cli}]"), screen.id.clone(), state, turns];
	if let Some(model) = &screen.model {
		parts.push(frame_text(model, 40));
	}
	parts.join(" ")
}

/// Plain describe result summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VibeDescribeResult {
	pub label: String,
	pub inline: bool,
	pub is_error: bool,
	pub body_lines: Vec<String>,
}

/// Describe vibe result (plain).
pub fn describe_vibe_result_plain(
	op: VibeOp,
	args: Option<&VibeRenderArgs>,
	details: Option<&VibeToolDetails>,
	is_error: bool,
	fallback_text: &str,
) -> VibeDescribeResult {
	let label = vibe_native_label(op, args);
	if is_error {
		return VibeDescribeResult {
			label,
			inline: true,
			is_error: true,
			body_lines: vec![if fallback_text.trim().is_empty() { "vibe failed".into() } else { fallback_text.trim().into() }],
		};
	}
	let Some(details) = details else {
		let t = fallback_text.trim();
		return VibeDescribeResult { label, inline: true, is_error: false, body_lines: if t.is_empty() { vec![] } else { vec![t.into()] } };
	};
	match op {
		VibeOp::Spawn => VibeDescribeResult {
			label: format!("spawn {}", details.spawned_cli.map(|c| c.as_str()).unwrap_or(args.and_then(|a| a.cli).map(|c| c.as_str()).unwrap_or("?"))),
			inline: true,
			is_error: false,
			body_lines: vec![format!("turn started{}", details.spawned_job_id.as_ref().map(|j| format!(" (job {j})")).unwrap_or_default())],
		},
		VibeOp::Send => VibeDescribeResult {
			label,
			inline: true,
			is_error: false,
			body_lines: vec![match details.send.as_ref().and_then(|s| s.mode) {
				Some(VibeSendMode::Steered) => "steered into the running turn".into(),
				Some(VibeSendMode::Queued) => "mid-turn — queued as the next turn".into(),
				_ => format!("turn started{}", details.send.as_ref().and_then(|s| s.job_id.as_ref()).map(|j| format!(" (job {j})")).unwrap_or_default()),
			}],
		},
		VibeOp::Kill => VibeDescribeResult {
			label,
			inline: true,
			is_error: false,
			body_lines: if details.killed.as_ref().map(|k| k.cancelled_turn).unwrap_or(false) { vec!["in-flight turn cancelled".into()] } else { vec![] },
		},
		VibeOp::List | VibeOp::Wait => {
			if details.screens.is_empty() {
				VibeDescribeResult { label, inline: true, is_error: false, body_lines: vec!["no sessions".into()] }
			} else {
				VibeDescribeResult {
					label,
					inline: true,
					is_error: false,
					body_lines: details.screens.iter().map(|s| tv_screen_header_line(s, None)).collect(),
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn frame_text_collapses_and_truncates() {
		assert_eq!(frame_text("a\n\t b", 20), "a b");
		assert_eq!(frame_text("hello world", 6), "hello…");
	}

	#[test]
	fn composer_rows_cursor_and_cap() {
		let rows = composer_rows_plain("hi", true, false);
		assert_eq!(rows[0], "> hi▌");
		let multi = "a\nb\nc\nd";
		let rows = composer_rows_plain(multi, false, false);
		assert_eq!(rows.len(), 2);
		assert!(rows[1].ends_with('…'));
	}

	#[test]
	fn labels() {
		let args = VibeRenderArgs { cli: Some(VibeCli::Fast), name: Some("worker".into()), ..Default::default() };
		assert_eq!(vibe_native_label(VibeOp::Spawn, Some(&args)), "spawn fast · worker");
		assert_eq!(describe_call(VibeOp::List, None), "sessions");
	}

	#[test]
	fn state_strings() {
		assert_eq!(VibeSessionState::Running.as_str(), "running");
		assert_eq!(VibeCli::Good.as_str(), "good");
	}

	#[test]
	fn tv_header_and_describe() {
		let screen = VibeScreenSnapshot { id: "w1".into(), cli: Some(VibeCli::Fast), state: Some(VibeSessionState::Running), turns: 2, queued: 1, ..Default::default() };
		let h = tv_screen_header_line(&screen, None);
		assert!(h.contains("[fast]"));
		assert!(h.contains("2t+1q"));
		let d = describe_vibe_result_plain(VibeOp::List, None, Some(&VibeToolDetails { op: Some(VibeOp::List), screens: vec![screen], ..Default::default() }), false, "");
		assert!(!d.body_lines.is_empty());
	}
}
