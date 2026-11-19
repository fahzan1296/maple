//! Agent hub roster and metric rendering helpers.
//!
//! Reference: `l/src/overlays/agent-hub-renderer.ts`.
//!
//! Deviations:
//! - ANSI/theme rendering (`theme.fg/bold/status/symbol`, styled progress bars,
//!   metric/table layout helpers) is not ported. Functions return plain-text or
//!   simple tokenized structures preserving the same labels/ordering/algorithms.
//! - `content_width()` uses a fixed fallback width (80) because
//!   `process.stdout.columns` is not available; callers can clamp externally.
//! - `visible_width`/`truncate_to_width`/`sanitize_display_single_line` are
//!   implemented locally with char/line sanitization and conservative truncation.

use std::collections::HashMap;

use crate::overlays::agent_hub_projection::AgentMetrics;
use crate::overlays::agent_hub_types::{AgentStatus, MAIN_AGENT_ID};

/// Rendered roster lines and hit rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RosterRender {
	pub lines: Vec<String>,
	pub hit_rows: Vec<Option<usize>>,
}

/// Thinking level values recognized by hub badges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThinkingLevel {
	Off,
	Inherit,
	Minimal,
	Low,
	Medium,
	High,
	XHigh,
	Max,
}

/// Host-resolved model-role label and color token.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRoleDisplay {
	pub color: Option<String>,
	pub tag: Option<String>,
	pub name: Option<String>,
}

/// Picker chip for a model badge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelChip {
	pub text: String,
	pub level: Option<String>,
	pub dot: Option<String>,
	pub fallback: bool,
}

/// Minimal record view needed by renderer helpers.
pub trait RenderAgentRecord {
	fn id(&self) -> &str;
	fn status(&self) -> AgentStatus;
	fn children(&self) -> Vec<String> {
		Vec::new()
	}
}

/// Semantic tone token for a status.
#[must_use]
pub fn status_tone(status: AgentStatus) -> &'static str {
	match status {
		AgentStatus::Running => "accent",
		AgentStatus::Idle => "success",
		AgentStatus::Parked => "muted",
		AgentStatus::Aborted => "error",
	}
}

/// Picker dot tone for a status.
#[must_use]
pub fn status_dot(status: AgentStatus) -> &'static str {
	match status {
		AgentStatus::Running => "pending",
		AgentStatus::Idle => "success",
		AgentStatus::Parked => "muted",
		AgentStatus::Aborted => "error",
	}
}

/// Status symbol (textual; theme symbols unavailable).
#[must_use]
pub fn status_symbol(status: AgentStatus) -> &'static str {
	match status {
		AgentStatus::Running => "●",
		AgentStatus::Idle => "✓",
		AgentStatus::Parked => "◌",
		AgentStatus::Aborted => "✗",
	}
}

/// Status glyph text.
#[must_use]
pub fn status_glyph(status: AgentStatus) -> String {
	status_symbol(status).to_string()
}

/// Sanitize display text to a single line.
#[must_use]
pub fn sanitize_display_single_line(text: &str) -> String {
	text.replace(['\r', '\n', '\t'], " ")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

/// Visible width approximation (char count).
#[must_use]
pub fn visible_width(text: &str) -> usize {
	text.chars().count()
}

/// Truncate to width by chars.
#[must_use]
pub fn truncate_to_width(text: &str, max_width: usize) -> String {
	if max_width == 0 {
		return String::new();
	}
	let chars: Vec<char> = text.chars().collect();
	if chars.len() <= max_width {
		return text.to_string();
	}
	chars.into_iter().take(max_width).collect()
}

/// Compute max content width (fallback; no stdout columns in crate).
#[must_use]
pub fn content_width() -> usize {
	74.max(12)
}

/// Sanitize and truncate a line for TUI display.
#[must_use]
pub fn sanitize_line(text: &str, max_width: Option<usize>) -> String {
	truncate_to_width(&sanitize_display_single_line(text), max_width.unwrap_or_else(content_width))
}

/// Clamp a hub line to width after stripping newlines.
#[must_use]
pub fn clamp_hub_line(line: &str, width: usize) -> String {
	let one = line.replace(['\r', '\n'], " ");
	truncate_to_width(&one, width.max(1))
}

const ASSIGNMENT_PREFIX: &str = "Complete assignment thoroughly:";

/// One-line task summary: strip fixed assignment preamble.
#[must_use]
pub fn task_summary(task: &str) -> String {
	let trimmed = task.trim_start();
	let rest = if trimmed.to_ascii_lowercase().starts_with(&ASSIGNMENT_PREFIX.to_ascii_lowercase()) {
		trimmed[ASSIGNMENT_PREFIX.len()..].trim_start()
	} else {
		task
	};
	sanitize_display_single_line(rest)
}

/// Textual model-role badge.
#[must_use]
pub fn format_role_badge(role: &str, info: &AgentRoleDisplay) -> String {
	info.tag
		.as_ref()
		.or(info.name.as_ref())
		.cloned()
		.unwrap_or_else(|| sanitize_display_single_line(role))
}

fn shows_thinking_level(level: Option<ThinkingLevel>) -> bool {
	matches!(
		level,
		Some(
			ThinkingLevel::Minimal
				| ThinkingLevel::Low
				| ThinkingLevel::Medium
				| ThinkingLevel::High
				| ThinkingLevel::XHigh
				| ThinkingLevel::Max
		)
	)
}

fn thinking_label(level: ThinkingLevel) -> &'static str {
	match level {
		ThinkingLevel::Off => "off",
		ThinkingLevel::Inherit => "inherit",
		ThinkingLevel::Minimal => "minimal",
		ThinkingLevel::Low => "low",
		ThinkingLevel::Medium => "medium",
		ThinkingLevel::High => "high",
		ThinkingLevel::XHigh => "xhigh",
		ThinkingLevel::Max => "max",
	}
}

/// Theme token name for a thinking level.
#[must_use]
pub fn thinking_token(level: ThinkingLevel) -> &'static str {
	match level {
		ThinkingLevel::Minimal => "thinkingMinimal",
		ThinkingLevel::Low => "thinkingLow",
		ThinkingLevel::Medium => "thinkingMedium",
		ThinkingLevel::High => "thinkingHigh",
		ThinkingLevel::XHigh | ThinkingLevel::Max => "thinkingXhigh",
		_ => "thinkingOff",
	}
}

/// Format model id + thinking level badge text.
#[must_use]
pub fn format_model_badge(model_id: &str, level: Option<ThinkingLevel>) -> String {
	let model = sanitize_display_single_line(model_id);
	if shows_thinking_level(level) {
		let l = level.expect("checked");
		format!("{model} {}", thinking_label(l))
	} else {
		model
	}
}

fn parse_thinking_level(s: &str) -> Option<ThinkingLevel> {
	match s.trim().to_ascii_lowercase().as_str() {
		"off" => Some(ThinkingLevel::Off),
		"inherit" => Some(ThinkingLevel::Inherit),
		"minimal" => Some(ThinkingLevel::Minimal),
		"low" => Some(ThinkingLevel::Low),
		"medium" => Some(ThinkingLevel::Medium),
		"high" => Some(ThinkingLevel::High),
		"xhigh" => Some(ThinkingLevel::XHigh),
		"max" => Some(ThinkingLevel::Max),
		_ => None,
	}
}

/// Resolve selector badge parts from a resolved selector string.
#[must_use]
pub fn resolve_selector_badge(
	resolved: &str,
	fallback: bool,
	preserve_provider: bool,
	fallback_level: Option<ThinkingLevel>,
) -> (String, Option<ThinkingLevel>, bool) {
	let clean = sanitize_display_single_line(resolved);
	let (selector, explicit_level) = if let Some(colon) = clean.rfind(':') {
		let suffix = &clean[colon + 1..];
		if let Some(lvl) = parse_thinking_level(suffix) {
			(clean[..colon].to_string(), Some(lvl))
		} else {
			(clean.clone(), None)
		}
	} else {
		(clean.clone(), None)
	};
	let label = if preserve_provider {
		selector
	} else if let Some(idx) = selector.find('/') {
		selector[idx + 1..].to_string()
	} else {
		selector
	};
	(label, explicit_level.or(fallback_level), fallback)
}

/// Format a cost amount.
#[must_use]
pub fn format_cost(cost: f64) -> String {
	let amount = if cost.is_finite() { cost } else { 0.0 };
	if amount < 0.01 {
		format!("${amount:.4}")
	} else if amount < 1.0 {
		format!("${amount:.3}")
	} else {
		format!("${amount:.2}")
	}
}

/// Format duration label for metrics.
#[must_use]
pub fn format_metric_duration(metrics: &AgentMetrics) -> Option<String> {
	if metrics.duration_ms == 0 {
		return None;
	}
	let secs = metrics.duration_ms as f64 / 1000.0;
	let label = match metrics.duration_kind {
		"active" => "active",
		"span" => "span",
		_ => "duration",
	};
	let dur = if secs < 60.0 {
		format!("{secs:.0}s")
	} else {
		format!("{:.1}m", secs / 60.0)
	};
	Some(format!("{dur} {label}"))
}

/// Plain-text metrics row.
#[must_use]
pub fn metrics_text(metrics: &AgentMetrics) -> String {
	format!(
		"{} · {} · {} req · {} tools · {} tok",
		format_cost(metrics.cost),
		format_metric_duration(metrics).unwrap_or_else(|| "time —".to_string()),
		metrics.requests,
		metrics.tools,
		metrics.tokens
	)
}

/// Fixed-width metric columns string (plain-text approximation).
#[must_use]
pub fn format_metric_columns(metrics: &AgentMetrics, age: &str) -> String {
	let dur = format_metric_duration(metrics).unwrap_or_else(|| "—".to_string());
	format!(
		"{:<8} {:>13} {:>8} {:>9} {:>8} {:>8}",
		format_cost(metrics.cost),
		truncate_to_width(&dur, 13),
		format!("{} req", metrics.requests),
		format!("{} tools", metrics.tools),
		format!("{} tok", metrics.tokens),
		truncate_to_width(age, 8),
	)
}

/// Context gauge text.
#[must_use]
pub fn context_gauge(tokens: u64, window: u64) -> String {
	if window == 0 {
		return format!("{tokens}/0 0%");
	}
	let ratio = (tokens as f64 / window as f64).clamp(0.0, 1.0);
	let filled = (ratio * 10.0).round() as usize;
	let bar = format!("{}{}", "━".repeat(filled.min(10)), "─".repeat(10 - filled.min(10)));
	format!("{bar} {tokens}/{window} {}%", (ratio * 100.0).round() as u64)
}

/// Fit child ids preview to width.
#[must_use]
pub fn format_child_ids(children: &[String], width: usize) -> String {
	let max = width.max(1);
	let mut shown = 0usize;
	let mut text = String::new();
	while shown < children.len() {
		let id = sanitize_line(&children[shown], Some(max));
		let candidate = if text.is_empty() { id.clone() } else { format!("{text}, {id}") };
		let remaining = children.len().saturating_sub(shown + 1);
		let suffix = if remaining > 0 { format!(", … +{remaining}") } else { String::new() };
		if visible_width(&format!("{candidate}{suffix}")) > max {
			let includes_current = text.is_empty();
			let omitted = children.len().saturating_sub(shown).saturating_sub(usize::from(includes_current));
			let base = if includes_current { id } else { text };
			let tail = if omitted > 0 { format!(", … +{omitted}") } else { String::new() };
			return truncate_to_width(&format!("{base}{tail}"), max);
		}
		text = candidate;
		shown += 1;
	}
	text
}

const TREE_SEGMENT_WIDTH: usize = 4;
const TREE_DETAIL_BASE_INDENT: usize = 4;

fn tree_prefix(
	ref_id: &str,
	max_width: usize,
	depth_by_id: &HashMap<String, usize>,
	parent_by_id: &HashMap<String, String>,
	last_sibling_by_id: &HashMap<String, bool>,
	continuation: bool,
) -> String {
	if depth_by_id.get(ref_id).copied().unwrap_or(0) == 0 {
		return String::new();
	}
	let last_sibling = last_sibling_by_id.get(ref_id).copied().unwrap_or(false);
	let mut segments: Vec<String> = vec![if continuation {
		if last_sibling { "    ".to_string() } else { "│   ".to_string() }
	} else if last_sibling {
		"└── ".to_string()
	} else {
		"├── ".to_string()
	}];
	let mut ancestry: std::collections::HashSet<String> = std::collections::HashSet::new();
	let mut parent = parent_by_id.get(ref_id).cloned();
	while let Some(p) = parent {
		if p == MAIN_AGENT_ID || ancestry.contains(&p) {
			break;
		}
		let grandparent = parent_by_id.get(&p).cloned();
		if grandparent.as_deref().unwrap_or(MAIN_AGENT_ID) == MAIN_AGENT_ID {
			break;
		}
		ancestry.insert(p.clone());
		let p_last = last_sibling_by_id.get(&p).copied().unwrap_or(false);
		segments.push(if p_last { "    ".to_string() } else { "│   ".to_string() });
		parent = grandparent;
	}
	let max_segments = (max_width.saturating_sub(2).max(TREE_SEGMENT_WIDTH) / TREE_SEGMENT_WIDTH).max(1);
	let omitted = segments.len().saturating_sub(max_segments);
	let prefix: String = segments.into_iter().take(max_segments).rev().collect();
	let omitted_prefix = if omitted > 0 { if continuation { "  " } else { "… " } } else { "" };
	format!("{omitted_prefix}{prefix}")
}

/// Bash `tree`-style branch for an agent identity row.
#[must_use]
pub fn tree_branch(
	ref_id: &str,
	max_width: usize,
	depth_by_id: &HashMap<String, usize>,
	parent_by_id: &HashMap<String, String>,
	last_sibling_by_id: &HashMap<String, bool>,
) -> String {
	tree_prefix(ref_id, max_width, depth_by_id, parent_by_id, last_sibling_by_id, false)
}

/// Ancestry rails for detail rows beneath an agent identity.
#[must_use]
pub fn tree_continuation(
	ref_id: &str,
	max_width: usize,
	depth_by_id: &HashMap<String, usize>,
	parent_by_id: &HashMap<String, String>,
	last_sibling_by_id: &HashMap<String, bool>,
) -> String {
	tree_prefix(ref_id, max_width, depth_by_id, parent_by_id, last_sibling_by_id, true)
}

/// One roster-wide origin for metric columns.
#[must_use]
pub fn tree_metadata_indent(max_width: usize, max_depth: usize) -> usize {
	(max_width.saturating_sub(1)).min(TREE_DETAIL_BASE_INDENT + max_depth * TREE_SEGMENT_WIDTH)
}

/// Right-align text inside a fixed-width cell (truncate overflow).
#[must_use]
pub fn align_right_cell(text: &str, width: usize) -> String {
	let t = truncate_to_width(text, width);
	let w = visible_width(&t);
	if w >= width {
		t
	} else {
		format!("{}{}", " ".repeat(width - w), t)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::HashMap;

	#[test]
	fn sanitize_and_truncate_helpers_work() {
		assert_eq!(sanitize_display_single_line("a\nb\t c"), "a b c");
		assert_eq!(clamp_hub_line("a\nb", 10), "a b");
		assert_eq!(truncate_to_width("héllo", 3), "hél");
	}

	#[test]
	fn task_summary_strips_assignment_preamble() {
		assert_eq!(
			task_summary("  Complete assignment thoroughly:   fix bug\nnow"),
			"fix bug now"
		);
		assert_eq!(task_summary("keep this"), "keep this");
	}

	#[test]
	fn format_cost_and_metrics_text() {
		assert_eq!(format_cost(0.0012), "$0.0012");
		assert_eq!(format_cost(0.123), "$0.123");
		assert_eq!(format_cost(2.5), "$2.50");
		let m = AgentMetrics {
			tokens: 10,
			requests: 2,
			tools: 3,
			cost: 0.5,
			duration_ms: 2000,
			duration_kind: "active",
			context_tokens: None,
			context_window: None,
		};
		let text = metrics_text(&m);
		assert!(text.contains("$0.500"));
		assert!(text.contains("2 req"));
		assert!(text.contains("3 tools"));
		assert!(text.contains("10 tok"));
		assert!(text.contains("active"));
	}

	#[test]
	fn tree_helpers_produce_branches_and_indents() {
		let mut depth = HashMap::new();
		let mut parent = HashMap::new();
		let mut last = HashMap::new();
		depth.insert("a".into(), 0);
		depth.insert("b".into(), 1);
		depth.insert("c".into(), 1);
		parent.insert("b".into(), "a".into());
		parent.insert("c".into(), "a".into());
		last.insert("b".into(), false);
		last.insert("c".into(), true);
		assert_eq!(tree_branch("a", 40, &depth, &parent, &last), "");
		assert!(tree_branch("b", 40, &depth, &parent, &last).contains("├──"));
		assert!(tree_branch("c", 40, &depth, &parent, &last).contains("└──"));
		assert!(tree_continuation("c", 40, &depth, &parent, &last).trim_end().ends_with("   "));
		assert_eq!(tree_metadata_indent(80, 2), 12);
	}

	#[test]
	fn child_ids_and_gauge_formatting() {
		let ids = vec!["one".to_string(), "two".to_string(), "three".to_string()];
		assert!(format_child_ids(&ids, 40).contains("one"));
		assert!(format_child_ids(&ids, 8).contains("…"));
		assert_eq!(context_gauge(50, 100), "━━━━━───── 50/100 50%");
		assert_eq!(align_right_cell("x", 4), "   x");
	}
}
