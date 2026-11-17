//! Agent tree row formatting data and helpers.
//!
//! Reference: `l/src/tools/agent-tree.ts`.
//!
//! Deviations:
//! - TS renders styled terminal strings using `Theme`, `visibleWidth`,
//!   `truncateToWidth`, badges, context-usage formatting, and feed/model badge
//!   helpers. Those TUI/render primitives are not ported here; Rust provides
//!   the data model and plain-text stat-run formatting. Visual row rendering is
//!   a host concern.

/// Counters displayed alongside an agent name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentStats {
	/// Number of tool calls.
	pub tool_count: Option<u64>,
	/// Number of model requests.
	pub requests: Option<u64>,
	/// Context tokens used.
	pub context_tokens: Option<u64>,
	/// Context window size.
	pub context_window: Option<u64>,
	/// Cost in USD.
	pub cost: Option<f64>,
}

/// Agent row lifecycle status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
	/// Not started.
	Pending,
	/// In progress.
	Running,
	/// Finished successfully.
	Completed,
	/// Failed.
	Failed,
	/// Aborted/cancelled.
	Aborted,
}

/// Tool-specific presentation kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentPresentation {
	/// Task/subagent presentation.
	Task,
	/// Eval presentation.
	Eval,
}

/// Configured thinking level placeholder (render-specific in TS).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfiguredThinkingLevel(pub String);

/// Tool-specific presentation layered onto a bounded agent progress row.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentTreeRowOptions {
	/// Row status.
	pub status: AgentStatus,
	/// Presentation mode.
	pub presentation: AgentPresentation,
	/// Tree prefix.
	pub prefix: String,
	/// Agent identifier.
	pub id: String,
	/// Available width; non-finite/absent in TS meant unlimited.
	pub width: Option<usize>,
	/// Model name.
	pub model: Option<String>,
	/// Thinking level.
	pub thinking_level: Option<ConfiguredThinkingLevel>,
	/// Advisor flag.
	pub advisor: bool,
	/// Spinner frame index.
	pub spinner_frame: Option<usize>,
	/// Frozen/live-snapshot flag.
	pub frozen: bool,
	/// Role badge text.
	pub role_badge: Option<String>,
	/// Status badge text.
	pub status_badge: Option<String>,
	/// Description text.
	pub description: Option<String>,
	/// Preview suffix.
	pub preview: Option<String>,
	/// Stats counters.
	pub stats: Option<AgentStats>,
	/// Completion estimate (0-100), shown only while live.
	pub completion_percent: Option<u8>,
	/// Duration in milliseconds.
	pub duration_ms: Option<u64>,
}

/// Format a number with comma grouping for plain-text stats.
pub fn format_number_plain(n: u64) -> String {
	let s = n.to_string();
	let mut out = String::with_capacity(s.len() + s.len() / 3);
	for (i, ch) in s.chars().rev().enumerate() {
		if i > 0 && i % 3 == 0 {
			out.push(',');
		}
		out.push(ch);
	}
	out.chars().rev().collect()
}

/// Format duration in a compact human form (ms/s/m).
pub fn format_duration_plain(ms: u64) -> String {
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

/// Format the shared tool-count, request, context, and cost stat run as plain text.
///
/// Uses `·` separators like the TS theme separator.
pub fn format_agent_stat_run_plain(stats: &AgentStats) -> String {
	let mut parts: Vec<String> = Vec::new();
	if let Some(tc) = stats.tool_count.filter(|&v| v > 0) {
		parts.push(format!("{} tools", format_number_plain(tc)));
	}
	if let Some(req) = stats.requests.filter(|&v| v > 0) {
		parts.push(format!("{} req", format_number_plain(req)));
	}
	if let Some(ctx) = stats.context_tokens.filter(|&v| v > 0) {
		if let Some(win) = stats.context_window.filter(|&v| v > 0) {
			let pct = (ctx as f64 / win as f64) * 100.0;
			parts.push(format!("{pct:.0}% of {}", format_number_plain(win)));
		} else {
			parts.push(format_number_plain(ctx));
		}
	}
	if let Some(cost) = stats.cost.filter(|&v| v > 0.0) {
		parts.push(format!("${cost:.2}"));
	}
	if parts.is_empty() {
		String::new()
	} else {
		format!(" · {}", parts.join(" · "))
	}
}

/// Render result for a plain agent row (text-only approximation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTreeRowRender {
	/// Rendered line.
	pub line: String,
	/// Whether description was shown.
	pub description_shown: bool,
}

/// Render an agent row in plain text, approximating TS layout.
///
/// Width truncation is byte-safe via char boundaries; theme/icons/badges are
/// simplified.
pub fn render_agent_tree_row_plain(options: &AgentTreeRowOptions) -> AgentTreeRowRender {
	let live = matches!(options.status, AgentStatus::Pending | AgentStatus::Running);
	let failed = matches!(options.status, AgentStatus::Failed | AgentStatus::Aborted);
	let icon = match options.status {
		AgentStatus::Pending => "○",
		AgentStatus::Running => "●",
		AgentStatus::Completed => "✓",
		AgentStatus::Failed | AgentStatus::Aborted => "✗",
	};
	let lead = format!(
		"{}{}{} ",
		options.prefix,
		if options.prefix.is_empty()
			&& matches!(options.presentation, AgentPresentation::Task)
		{
			""
		} else {
			" "
		},
		icon
	);
	let status_badge = options
		.status_badge
		.clone()
		.unwrap_or_else(|| if failed { " [error]".into() } else { String::new() });
	let role_badge = options.role_badge.clone().unwrap_or_default();
	let badges = format!("{role_badge}{status_badge}");
	let description_shown = options
		.description
		.as_ref()
		.map(|d| {
			if let Some(w) = options.width {
				lead.chars().count() + options.id.chars().count() + 2 + d.chars().count() + badges.chars().count()
					<= w
			} else {
				true
			}
		})
		.unwrap_or(false);
	let title = if description_shown {
		format!("{}: {}", options.id, options.description.as_deref().unwrap_or(""))
	} else {
		options.id.clone()
	};
	let mut line = format!("{lead}{title}{badges}");
	if let Some(preview) = &options.preview {
		line.push_str(preview);
	}
	if live {
		if let Some(pct) = options.completion_percent {
			line.push_str(&format!(" · {pct}%"));
		}
	}
	if let Some(stats) = &options.stats {
		line.push_str(&format_agent_stat_run_plain(stats));
	}
	if let Some(ms) = options.duration_ms.filter(|&v| v > 0) {
		line.push_str(&format!(" · {}", format_duration_plain(ms)));
	}
	if let Some(w) = options.width {
		if line.chars().count() > w {
			line = line.chars().take(w).collect();
		}
	}
	AgentTreeRowRender {
		line,
		description_shown,
	}
}

use crate::render::render_utils::{
	format_badge, format_feed_model_badge, format_status_icon, is_feed_model_badge_enabled,
	paint, ToolUiStatus, ToolUIColor, FEED_MODEL_BADGE_WIDTH,
};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::{Theme, ThemeColor};

fn status_ui(status: AgentStatus) -> ToolUiStatus {
	match status {
		AgentStatus::Pending => ToolUiStatus::Pending,
		AgentStatus::Running => ToolUiStatus::Running,
		AgentStatus::Completed => ToolUiStatus::Success,
		AgentStatus::Failed | AgentStatus::Aborted => ToolUiStatus::Error,
	}
}

/// Render an agent row with theme glyphs, feed/model badge, and stats.
///
/// Reference: `l/src/tools/agent-tree.ts::renderAgentTreeRow`.
pub fn render_agent_tree_row_themed(options: &AgentTreeRowOptions, theme: &Theme) -> AgentTreeRowRender {
	let width = options.width;
	let live = matches!(options.status, AgentStatus::Pending | AgentStatus::Running);
	let failed = matches!(options.status, AgentStatus::Failed | AgentStatus::Aborted);
	let task = matches!(options.presentation, AgentPresentation::Task);
	let name_color = if task && live && options.frozen {
		"dim"
	} else if task && options.status == AgentStatus::Completed {
		"text"
	} else {
		"accent"
	};
	let spinner_frame = if !task && options.status == AgentStatus::Running {
		options.spinner_frame
	} else {
		None
	};
	let icon = if !task && options.status == AgentStatus::Completed {
		theme
			.styled_symbol("tool.eval", "accent")
			.unwrap_or_else(|_| theme.symbol("tool.eval").to_string())
	} else if task && live && options.frozen {
		let glyph = if options.status == AgentStatus::Running {
			theme.status().running
		} else {
			theme.status().pending
		};
		paint(theme, ThemeColor::Dim, &glyph)
	} else if spinner_frame.is_none() {
		format_status_icon(status_ui(options.status), theme, None)
	} else {
		paint(
			theme,
			ThemeColor::Accent,
			&format_status_icon(status_ui(options.status), theme, spinner_frame),
		)
	};
	let separator = if options.prefix.is_empty() && task { "" } else { " " };
	let lead = format!("{}{separator}{icon} ", options.prefix);
	let status_badge = options.status_badge.clone().unwrap_or_else(|| {
		if failed {
			let label = if options.status == AgentStatus::Aborted { "aborted" } else { "failed" };
			format!(" {}", format_badge(label, ToolUIColor::Error, theme))
		} else {
			String::new()
		}
	});
	let id_limit = width.unwrap_or(usize::MAX).saturating_sub(visible_width(&lead) + visible_width(&status_badge));
	let id = truncate_to_width(&options.id, id_limit, Ellipsis::Omit);
	let role_badge = match (&options.role_badge, width) {
		(Some(badge), Some(width)) => truncate_to_width(
			badge,
			width.saturating_sub(visible_width(&lead) + visible_width(&id) + visible_width(&status_badge)),
			Ellipsis::Omit,
		),
		(Some(badge), None) => badge.clone(),
		(None, _) => String::new(),
	};
	let badges = format!("{role_badge}{status_badge}");
	let model_width = width
		.unwrap_or(usize::MAX)
		.saturating_sub(visible_width(&lead) + visible_width(&id) + visible_width(&badges) + 1);
	let model = if is_feed_model_badge_enabled() && (task || options.model.is_some()) {
		format_feed_model_badge(
			options.model.as_deref(),
			options.thinking_level.as_ref().map(|level| crate::thinking::ConfiguredThinkingLevel::parse_configured_thinking_level(Some(&level.0)).unwrap_or(crate::thinking::ConfiguredThinkingLevel::Inherit)),
			options.advisor,
			theme,
			model_width.min(FEED_MODEL_BADGE_WIDTH),
		)
	} else {
		String::new()
	};
	let model_lead = if model.is_empty() { String::new() } else { format!("{model} ") };
	let description_shown = options.description.as_ref().is_some_and(|description| {
		width.map_or(true, |width| {
			visible_width(&format!("{lead}{model_lead}{id}: {description}{badges}")) <= width
		})
	});
	let title = if description_shown {
		let description = options.description.as_deref().unwrap_or_default();
		if task && live {
			format!(
				"{}{} {}",
				paint(theme, name_color, &theme.bold(&id)),
				paint(theme, name_color, ":"),
				paint(theme, name_color, description)
			)
		} else {
			paint(theme, name_color, &format!("{}: {}", theme.bold(&id), description))
		}
	} else if task {
		paint(theme, name_color, &id)
	} else {
		paint(theme, name_color, &theme.bold(&id))
	};
	let mut line = format!("{lead}{model_lead}{title}{badges}");
	if let Some(preview) = &options.preview {
		line.push_str(preview);
	}
	if live {
		if let Some(percent) = options.completion_percent {
			line.push_str(&format!("{} {}", theme.sep().dot, paint(theme, ThemeColor::Accent, &format!("{percent}%"))));
		}
	}
	if let Some(stats) = &options.stats {
		line.push_str(&format_agent_stat_run_plain(stats));
	}
	if let Some(duration) = options.duration_ms.filter(|value| *value > 0) {
		line.push_str(&format!(
			"{} {}",
			theme.sep().dot,
			paint(theme, ThemeColor::Dim, &format_duration_plain(duration))
		));
	}
	if let Some(width) = width {
		line = truncate_to_width(&line, width, Ellipsis::Omit);
	}

	AgentTreeRowRender {
		line,
		description_shown,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_number_plain_groups() {
		assert_eq!(format_number_plain(0), "0");
		assert_eq!(format_number_plain(999), "999");
		assert_eq!(format_number_plain(1000), "1,000");
		assert_eq!(format_number_plain(12_345_678), "12,345,678");
	}

	#[test]
	fn duration_plain_formats() {
		assert_eq!(format_duration_plain(500), "500ms");
		assert_eq!(format_duration_plain(1500), "1.5s");
		assert_eq!(format_duration_plain(65_000), "1m5s");
	}

	#[test]
	fn stat_run_plain_includes_parts() {
		let stats = AgentStats {
			tool_count: Some(3),
			requests: Some(2),
			context_tokens: Some(50),
			context_window: Some(100),
			cost: Some(0.12),
		};
		let s = format_agent_stat_run_plain(&stats);
		assert!(s.contains("3 tools"));
		assert!(s.contains("2 req"));
		assert!(s.contains("50%"));
		assert!(s.contains("$0.12"));
	}

	#[test]
	fn plain_row_basic() {
		let opts = AgentTreeRowOptions {
			status: AgentStatus::Running,
			presentation: AgentPresentation::Task,
			prefix: String::new(),
			id: "A1".into(),
			width: Some(80),
			model: None,
			thinking_level: None,
			advisor: false,
			spinner_frame: None,
			frozen: false,
			role_badge: None,
			status_badge: None,
			description: Some("doing work".into()),
			preview: None,
			stats: None,
			completion_percent: Some(42),
			duration_ms: None,
		};
		let r = render_agent_tree_row_plain(&opts);
		assert!(r.line.contains("A1"));
		assert!(r.line.contains("42%"));
		assert!(r.description_shown);
	}
}
