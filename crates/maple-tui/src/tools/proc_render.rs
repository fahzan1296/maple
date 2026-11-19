//! Process/job/daemon/IRC render-model types and helpers.
//!
 //! Reference: `l/src/tools/proc-render.ts`.
 //!
 //! Deviations:
//! - Rich TUI components (tree lists, framed/streaming previews, terminal row
//!   styling widgets) not fully ported. Snapshot/detail types, summaries, and
//!   themed/native render+describe helpers are provided.
//! - `*_themed` string helpers take `&Theme`; native describe emits token-styled
//!   nodes for host theme-aware rendering.
/// Process operation selected by a write URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcWriteAction {
	Stdin,
	Mode,
	Kill,
}

impl ProcWriteAction {
	/// Action label.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Stdin => "stdin",
			Self::Mode => "mode",
			Self::Kill => "kill",
		}
	}
}

/// Minimal job snapshot fields used for process rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobSnapshot {
	pub id: String,
	pub r#type: String,
	pub status: String, // running/completed/failed/cancelled
	pub intent: Option<String>,
	pub label: Option<String>,
	pub duration_ms: u64,
	pub error_text: Option<String>,
	pub result_text: Option<String>,
}

/// Minimal daemon snapshot fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DaemonSnapshot {
	pub name: Option<String>,
	pub state: String,
	pub pid: Option<u32>,
	pub started_at: i64,
	pub exited_at: Option<i64>,
	pub detached: bool,
	pub persist: bool,
}

/// Minimal agent activity snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentActivitySnapshot {
	pub id: String,
	pub live: bool,
	pub age_ms: u64,
}

/// IRC delivery receipt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IrcDeliveryReceipt {
	pub to: String,
	pub outcome: String, // injected/woken/revived/failed
	pub error: Option<String>,
}

/// Coordination details for agent writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoordinationDetails {
	pub to: Option<String>,
	pub receipts: Vec<IrcDeliveryReceipt>,
	pub op: Option<String>,
	pub jobs: Vec<JobSnapshot>,
	pub cancelled: Vec<ProcCancelOutcome>,
}

/// Proc cancel outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcCancelOutcome {
	pub id: String,
	pub status: String,
}

/// Read details for proc:// reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcReadDetails {
	pub jobs: Vec<JobSnapshot>,
	pub agents: Vec<AgentActivitySnapshot>,
	pub daemons: Vec<DaemonSnapshot>,
	pub job: Option<JobSnapshot>,
	pub daemon: Option<DaemonSnapshot>,
	pub log: Option<String>,
	pub terminal_rows: Vec<String>,
}

/// Write details for proc:// writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcWriteDetails {
	pub coordination: Option<CoordinationDetails>,
	pub action: Option<String>,
	pub daemon: Option<DaemonSnapshot>,
	pub input: Option<String>,
	pub mode: Option<String>,
}

/// Format duration in ms.
pub fn format_duration(ms: u64) -> String {
	if ms < 1000 {
		format!("{ms}ms")
	} else if ms < 60_000 {
		let s = ms as f64 / 1000.0;
		if s < 10.0 {
			format!("{s:.1}s")
		} else {
			format!("{:.0}s", s)
		}
	} else {
		let m = ms / 60_000;
		let s = (ms % 60_000) / 1000;
		if s == 0 {
			format!("{m}m")
		} else {
			format!("{m}m{s}s")
		}
	}
}

/// Daemon meta summary parts.
pub fn daemon_meta_text(daemon: &DaemonSnapshot, now_ms: i64) -> String {
	let mut meta = vec![daemon.state.clone()];
	if let Some(pid) = daemon.pid {
		meta.push(format!("pid {pid}"));
	}
	let end = daemon.exited_at.unwrap_or(now_ms);
	let dur = (end - daemon.started_at).max(0) as u64;
	meta.push(format!(
		"{} {}",
		if daemon.exited_at.is_none() { "up" } else { "ran" },
		format_duration(dur)
	));
	if daemon.detached {
		meta.push("detached".to_string());
	} else if daemon.persist {
		meta.push("persistent".to_string());
	}
	meta.join(" · ")
}

/// Receipt tone/color token.
pub fn receipt_tone(outcome: &str) -> &'static str {
	match outcome {
		"failed" => "error",
		"revived" => "warning",
		_ => "success",
	}
}

/// Job tone/color token.
pub fn job_tone(status: &str) -> &'static str {
	match status {
		"failed" => "error",
		"cancelled" => "warning",
		"running" => "info",
		_ => "success",
	}
}

/// Summarize IRC receipts.
pub fn summarize_receipts(receipts: &[IrcDeliveryReceipt]) -> (usize, usize) {
	let delivered = receipts.iter().filter(|r| r.outcome != "failed").count();
	let failed = receipts.len().saturating_sub(delivered);
	(delivered, failed)
}

/// Plain render of an `agent://` write.
pub fn render_agent_write(to: &str, body: &str, output: &str, is_error: bool) -> Vec<String> {
	if is_error { return vec![format!("message to {to} failed: {}", output.lines().next().unwrap_or(output.trim()))]; }
	let mut lines = vec![format!("→ {to}")];
	if !body.trim().is_empty() { lines.push(body.lines().next().unwrap_or("").to_string()); }
	if !output.trim().is_empty() { lines.push(output.lines().next().unwrap_or("").to_string()); }
	lines
}

/// Plain render of a `proc://` write.
pub fn render_proc_write(id: &str, action: ProcWriteAction, output: &str, is_error: bool) -> Vec<String> {
	let label = match action { ProcWriteAction::Kill => "stop", ProcWriteAction::Stdin => "stdin", ProcWriteAction::Mode => "mode" };
	if is_error { vec![format!("{label} {id} failed: {}", output.lines().next().unwrap_or(output.trim()))] }
	else if output.trim().is_empty() { vec![format!("{label} {id}")] } else { vec![format!("{label} {id}"), output.lines().next().unwrap_or("").to_string()] }
}

/// Plain render of a `proc://` read.
pub fn render_proc_read(id: &str, output: &str, details: Option<&ProcReadDetails>, is_error: bool) -> Vec<String> {
	if is_error { return vec![format!("proc read {id} failed: {}", output.lines().next().unwrap_or(output.trim()))]; }
	if let Some(d) = details {
		if !d.jobs.is_empty() { return vec![format!("{id}: {} jobs", d.jobs.len())]; }
		if !d.daemons.is_empty() { return vec![format!("{id}: {} daemons", d.daemons.len())]; }
		if let Some(job) = &d.job { return vec![format!("{} {}", id, job.status)]; }
	}
	if output.trim().is_empty() { vec![format!("proc {id}")] } else { output.trim_end().lines().take(40).map(|s| s.to_string()).collect() }
}

/// Native describe an `agent://` write.
pub fn describe_agent_write(to: &str, body: &str, output: &str, is_error: bool) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	if is_error { return error_view("Message", output.trim(), &[Some(to.to_string())]); }
	let head = tool_head("Message", &[ToolHeadDetail::Span(Span::styled(format!("→ {to}"), "accent"))]);
	let mut children = Vec::new();
	if !body.trim().is_empty() { let mut p=Props::new(); p.set_str("wrap","word"); children.push(ntext(NodeText::Plain(body.trim_end().to_string()), p)); }
	if !output.trim().is_empty() { children.push(note_text(output.lines().next().unwrap_or(""), "muted", Some(1))); }
	if children.is_empty() { children.push(note_text("sent", "dim", None)); }
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(children), tone: Some(Tone::Success), ..NativeToolView::default() }
}

/// Native describe a `proc://` write.
pub fn describe_proc_write(id: &str, action: ProcWriteAction, output: &str, is_error: bool) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	let label = match action { ProcWriteAction::Kill => "Stop", ProcWriteAction::Stdin => "Stdin", ProcWriteAction::Mode => "Mode" };
	if is_error { return error_view(label, output.trim(), &[Some(id.to_string())]); }
	let head = tool_head(label, &[ToolHeadDetail::Span(Span::styled(id.to_string(), "accent"))]);
	let body = if output.trim().is_empty() { vec![note_text("ok", "dim", None)] } else { let mut p=Props::new(); p.set_str("wrap","word"); vec![ntext(NodeText::Plain(output.trim_end().to_string()), p)] };
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(body), tone: Some(Tone::Success), ..NativeToolView::default() }
}

/// Native describe a `proc://` read.
pub fn describe_proc_read(id: &str, output: &str, details: Option<&ProcReadDetails>, is_error: bool) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	if is_error { return error_view("Proc", output.trim(), &[Some(id.to_string())]); }
	let mut head_details = vec![ToolHeadDetail::Span(Span::styled(id.to_string(), "accent"))];
	if let Some(d) = details { if !d.jobs.is_empty() { head_details.push(ToolHeadDetail::Span(Span::styled(format!("{} jobs", d.jobs.len()), "muted"))); } }
	let head = tool_head("Proc", &head_details);
	let body_text = output.trim_end();
	if body_text.is_empty() { return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("No output", "dim", None)]), ..NativeToolView::default() }; }
	let mut p=Props::new(); p.set_str("wrap","word");
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![ntext(NodeText::Plain(body_text.to_string()), p)]), ..NativeToolView::default() }
}

/// Themed render of an `agent://` write.
pub fn render_agent_write_themed(to: &str, body: &str, output: &str, is_error: bool, theme: &crate::theme::Theme) -> Vec<String> {
	if is_error { return vec![format!("{} {}", theme.fg("error", &format!("message to {to} failed:")).unwrap_or_else(|_| format!("message to {to} failed:")), output.lines().next().unwrap_or(output.trim()))]; }
	let mut lines = vec![format!("{} {}", theme.fg("accent", "→").unwrap_or_else(|_| "→".into()), theme.fg("accent", to).unwrap_or_else(|_| to.to_string()))];
	if !body.trim().is_empty() { lines.push(theme.fg("toolOutput", body.lines().next().unwrap_or("")).unwrap_or_else(|_| body.lines().next().unwrap_or("").to_string())); }
	if !output.trim().is_empty() { lines.push(theme.fg("muted", output.lines().next().unwrap_or("")).unwrap_or_else(|_| output.lines().next().unwrap_or("").to_string())); }
	lines
}
/// Themed render of a `proc://` write.
pub fn render_proc_write_themed(id: &str, action: ProcWriteAction, output: &str, is_error: bool, theme: &crate::theme::Theme) -> Vec<String> {
	let label = match action { ProcWriteAction::Kill => "stop", ProcWriteAction::Stdin => "stdin", ProcWriteAction::Mode => "mode" };
	if is_error { vec![format!("{} {}", theme.fg("error", &format!("{label} {id} failed:")).unwrap_or_else(|_| format!("{label} {id} failed:")), output.lines().next().unwrap_or(output.trim()))] }
	else if output.trim().is_empty() { vec![format!("{} {}", theme.fg("success", label).unwrap_or_else(|_| label.into()), theme.fg("accent", id).unwrap_or_else(|_| id.to_string()))] }
	else { vec![format!("{} {}", theme.fg("success", label).unwrap_or_else(|_| label.into()), theme.fg("accent", id).unwrap_or_else(|_| id.to_string())), theme.fg("toolOutput", output.lines().next().unwrap_or("")).unwrap_or_else(|_| output.lines().next().unwrap_or("").to_string())] }
}
/// Themed render of a `proc://` read.
pub fn render_proc_read_themed(id: &str, output: &str, details: Option<&ProcReadDetails>, is_error: bool, theme: &crate::theme::Theme) -> Vec<String> {
	if is_error { return vec![format!("{} {}", theme.fg("error", &format!("proc read {id} failed:")).unwrap_or_else(|_| format!("proc read {id} failed:")), output.lines().next().unwrap_or(output.trim()))]; }
	if let Some(d) = details {
		if let Some(job) = &d.job { let tone = job_tone(&job.status); return vec![format!("{} {}", theme.fg("accent", id).unwrap_or_else(|_| id.to_string()), theme.fg(tone, &job.status).unwrap_or_else(|_| job.status.clone()))]; }
	}
	if output.trim().is_empty() { vec![format!("{} {}", theme.fg("accent", "proc").unwrap_or_else(|_| "proc".into()), theme.fg("muted", id).unwrap_or_else(|_| id.to_string()))] }
	else { output.trim_end().lines().take(40).map(|s| theme.fg("toolOutput", s).unwrap_or_else(|_| s.to_string())).collect() }
}
#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn proc_render_helpers() {
		let a = render_agent_write("bob", "hi", "ok", false);
		assert!(a[0].contains("bob"));
		let w = render_proc_write("j1", ProcWriteAction::Kill, "", false);
		assert!(w[0].contains("stop"));
		let r = render_proc_read("j1", "log", None, false);
		assert_eq!(r[0], "log");
		let d = describe_proc_read("j1", "log", None, false);
		assert!(d.head.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn duration_format() {
		assert_eq!(format_duration(10), "10ms");
		assert_eq!(format_duration(1500), "1.5s");
		assert_eq!(format_duration(65_000), "1m5s");
	}

	#[test]
	fn daemon_meta() {
		let d = DaemonSnapshot {
			state: "ready".into(),
			pid: Some(7),
			started_at: 0,
			exited_at: None,
			detached: false,
			persist: true,
			name: None,
		};
		let s = daemon_meta_text(&d, 5_000);
		assert!(s.contains("ready"));
		assert!(s.contains("pid 7"));
		assert!(s.contains("up"));
		assert!(s.contains("persistent"));
	}

	#[test]
	fn receipt_summary() {
		let rs = vec![
			IrcDeliveryReceipt {
				to: "a".into(),
				outcome: "injected".into(),
				error: None,
			},
			IrcDeliveryReceipt {
				to: "b".into(),
				outcome: "failed".into(),
				error: Some("x".into()),
			},
		];
		assert_eq!(summarize_receipts(&rs), (1, 1));
		assert_eq!(receipt_tone("revived"), "warning");
		assert_eq!(job_tone("running"), "info");
	}
}
