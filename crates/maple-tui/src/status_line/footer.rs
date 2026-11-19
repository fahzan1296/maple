//! Legacy footer: working directory, usage counters, billing, context usage,
//! and the model id.
//!
//! Reference: `l/src/status-line/footer.ts`.
//!
//! # Divergences
//!
//! - `describe()` is not ported: the crate's [`crate::native::node::NodeKind`]
//!   vocabulary has no `status`/`seg` kinds, and the `pi-wire` span/tone props
//!   it builds do not exist here.
//! - The branch label arrives through [`FooterComponent::set_git_branch`]
//!   instead of `vcs.repoForDisplay(getProjectDir())`; the `pi-natives/vcs`
//!   package is not ported. `host.git_enabled()` still gates it.
//! - `getProjectDir()` → [`FooterComponent::set_paths`].
//! - `formatBillingSummary` is inlined for the footer's option set (cost,
//!   subscription, premium requests, three decimals); `metrics.ts` is not
//!   ported.
//! - Path truncation is char-based rather than UTF-16-code-unit based.
//! - Extension statuses sort by byte order rather than `localeCompare`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::ansi_scan::strip_ansi;
use crate::component::Component;
use crate::keys::Key;
use crate::mouse::MouseEvent;
use crate::render::render_utils::padding;
use crate::status_line::host::{FooterHost, FooterSession, UsageStatistics};
use crate::status_line::segments::{
	context_usage_color, context_usage_level, format_context_usage, format_number,
	sanitize_status_text, shorten_path,
};
use crate::status_line::{ActiveTheme, StatusTheme};
use crate::text_shape::{Ellipsis, truncate_to_width, visible_width};

/// Round `value` to two decimals the way `normalizePremiumRequests` does.
fn normalize_premium_requests(value: f64) -> f64 {
	((value + f64::EPSILON) * 100.0).round() / 100.0
}

/// `$`/subscription-marked spend; the unit prints at most once per summary.
fn format_spend(
	amount: f64,
	using_subscription: bool,
	fraction_digits: usize,
	theme: &dyn StatusTheme,
) -> String {
	let formatted = format!("{amount:.fraction_digits$}");
	if !using_subscription {
		return format!("${formatted}");
	}
	let icon = theme.symbol("icon.subscription");
	if theme.symbol_preset() == "nerd" && !icon.is_empty() {
		format!("{icon} {formatted}")
	} else {
		format!("S{formatted}")
	}
}

/// Billing summary for the footer's option set, or `None` when nothing is billed.
fn billing_summary(
	cost: f64,
	using_subscription: bool,
	premium_requests: f64,
	theme: &dyn StatusTheme,
) -> Option<String> {
	let premium_requests = normalize_premium_requests(premium_requests);
	if cost == 0.0 && !using_subscription && premium_requests == 0.0 {
		return None;
	}
	let mut parts: Vec<String> = Vec::new();
	if cost != 0.0 {
		parts.push(format_spend(cost, using_subscription, 3, theme));
	} else if using_subscription {
		parts.push(if theme.symbol_preset() == "nerd" && !theme.symbol("icon.subscription").is_empty()
		{
			theme.symbol("icon.subscription")
		} else {
			"(sub)".to_string()
		});
	}
	if premium_requests != 0.0 {
		parts.push(format!("★ {}", format_number(premium_requests)));
	}
	Some(parts.join(" "))
}

/// Footer component: pwd, token stats, and context usage.
pub struct FooterComponent<'a> {
	session: &'a dyn FooterSession,
	host: &'a dyn FooterHost,
	project_dir: String,
	home_dir: String,
	git_branch: Option<String>,
	auto_compact_enabled: bool,
	extension_statuses: BTreeMap<String, String>,
}

impl<'a> FooterComponent<'a> {
	/// Build a footer over `session` and `host`.
	pub fn new(session: &'a dyn FooterSession, host: &'a dyn FooterHost) -> Self {
		Self {
			session,
			host,
			project_dir: ".".to_string(),
			home_dir: String::new(),
			git_branch: None,
			auto_compact_enabled: true,
			extension_statuses: BTreeMap::new(),
		}
	}

	/// Set the project directory and the home directory used to abbreviate it.
	pub fn set_paths(&mut self, project_dir: impl Into<String>, home_dir: impl Into<String>) {
		self.project_dir = project_dir.into();
		self.home_dir = home_dir.into();
	}

	/// Set the branch label, or `None` outside a repository.
	pub fn set_git_branch(&mut self, branch: Option<String>) {
		self.git_branch = branch;
	}

	/// Enable or disable the auto-compact indicator.
	pub fn set_auto_compact_enabled(&mut self, enabled: bool) {
		self.auto_compact_enabled = enabled;
	}

	/// Set extension status text; `None` clears the key.
	///
	/// ANSI/VT escape sequences and most control characters are stripped; tabs
	/// and newlines become spaces.
	pub fn set_extension_status(&mut self, key: impl Into<String>, text: Option<String>) {
		let key: String = key.into();
		match text {
			Some(text) => {
				self.extension_statuses.insert(key, text);
			}
			None => {
				self.extension_statuses.remove(&key);
			}
		}
	}
}

impl Component for FooterComponent<'_> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let theme = ActiveTheme::active();
		let theme: &dyn StatusTheme = &*theme;
		let width = width as usize;

		let usage: UsageStatistics = self.session.usage_statistics();
		let context_usage = self.session.context_usage();
		let model = self.session.model();

		// Cumulative usage comes from the session manager's running rollup,
		// which spans every entry in the session and folds in the usage
		// completed `task` tool results carry.
		let context_window = context_usage
			.map(|usage| usage.context_window)
			.or_else(|| {
				model.and_then(|model| model.get("contextWindow")).and_then(Value::as_u64)
			})
			.unwrap_or(0);
		let context_tokens = context_usage.map(|usage| usage.tokens).unwrap_or(0);
		let context_percent = if context_window > 0 {
			Some(context_usage.and_then(|usage| usage.percent).unwrap_or(0.0))
		} else {
			None
		};

		let mut pwd = shorten_path(&self.project_dir, &self.home_dir);
		if self.host.git_enabled() {
			if let Some(branch) = self.git_branch.as_deref() {
				pwd = format!("{pwd} ({branch})");
			}
		}
		let pwd_len = pwd.chars().count();
		if pwd_len > width {
			let half = (width / 2).saturating_sub(1);
			if half > 1 {
				let start: String = pwd.chars().take(half).collect();
				let end: String = pwd.chars().skip(pwd_len - (half - 1)).collect();
				pwd = format!("{start}…{end}");
			} else {
				pwd = pwd.chars().take(width.max(1)).collect();
			}
		}

		let mut stats_parts: Vec<String> = Vec::new();
		for (glyph, amount) in [
			("↑", usage.input),
			("↓", usage.output),
			("R", usage.cache_read),
			("W", usage.cache_write),
		] {
			if amount != 0 {
				stats_parts.push(format!("{glyph}{}", format_number(amount as f64)));
			}
		}

		let using_subscription = model
			.map(|model| self.session.is_using_subscription(model))
			.unwrap_or(false);
		if let Some(summary) = billing_summary(usage.cost, using_subscription, usage.premium_requests as f64, theme)
		{
			stats_parts.push(summary);
		}

		let auto_indicator = if self.auto_compact_enabled && !theme.symbol("icon.auto").is_empty() {
			format!(" {}", theme.symbol("icon.auto"))
		} else {
			String::new()
		};
		let context_display =
			format!("{}{auto_indicator}", format_context_usage(context_percent, context_window, context_tokens));
		let context_percent_str = match (context_usage, context_percent) {
			(Some(_), Some(percent)) => {
				let color = context_usage_color(context_usage_level(percent, context_window));
				if color == "statusLineContext" {
					context_display
				} else {
					theme.fg(color, &context_display)
				}
			}
			_ => context_display,
		};
		stats_parts.push(context_percent_str);

		let mut stats_left = stats_parts.join(" ");
		let right_side = {
			let model_name = model
				.and_then(|model| model.get("id"))
				.and_then(Value::as_str)
				.unwrap_or("no-model");
			let mut right_side = model_name.to_string();
			if model
				.and_then(|model| model.get("thinking"))
				.and_then(Value::as_bool)
				.unwrap_or(false)
			{
				if self.session.is_auto_thinking() {
					right_side = match self.session.auto_resolved_thinking_level() {
						Some(level) => format!("{model_name} • {level}"),
						None => format!("{model_name} • {} auto", theme.symbol("thinking.autoPending")),
					};
				} else {
					let level = self.session.thinking_level().unwrap_or("off");
					right_side = format!("{model_name} • {level}");
				}
			}
			right_side
		};
		let right_side_width = visible_width(&right_side);

		if visible_width(&stats_left) > width {
			// Drop styling and truncate by terminal cells (not code points) so
			// wide glyphs and non-SGR escapes can't overflow the line.
			stats_left = truncate_to_width(&strip_ansi(&stats_left), width, Ellipsis::Unicode);
		}

		// Minimum two spaces between stats and model.
		let min_padding = 2;
		let stats_left_width = visible_width(&stats_left);
		let total_needed = stats_left_width + min_padding + right_side_width;

		let stats_line = if total_needed <= width {
			let pad = padding(width - stats_left_width - right_side_width);
			format!("{stats_left}{pad}{right_side}")
		} else {
			let available_for_right = width
				.saturating_sub(stats_left_width)
				.saturating_sub(min_padding);
			if available_for_right > 3 {
				let truncated =
					truncate_to_width(&strip_ansi(&right_side), available_for_right, Ellipsis::Unicode);
				let pad = padding(width - stats_left_width - visible_width(&truncated));
				format!("{stats_left}{pad}{truncated}")
			} else {
				stats_left.clone()
			}
		};

		// Dim the parts separately: `stats_left` may end in a reset that would
		// clear an outer dim wrapper.
		let dim_stats_left = theme.fg("dim", &stats_left);
		let remainder = &stats_line[stats_left.len()..];
		let dim_remainder = theme.fg("dim", remainder);

		let mut lines = vec![theme.fg("dim", &pwd), format!("{dim_stats_left}{dim_remainder}")];

		if !self.extension_statuses.is_empty() {
			let status_line = self
				.extension_statuses
				.values()
				.map(|text| sanitize_status_text(text))
				.collect::<Vec<_>>()
				.join(" ");
			lines.push(truncate_to_width(&status_line, width, Ellipsis::Unicode));
		}

		lines
	}

	fn handle_input(&mut self, _key: &Key) {}

	fn handle_mouse(&mut self, _event: &MouseEvent) {}

	fn handle_paste(&mut self, _text: &str) {}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::status_line::host::ContextUsage;
	use crate::status_line::FallbackTheme;
	use std::rc::Rc;

	struct StubSession {
		model: Value,
		thinking: Option<String>,
		auto: bool,
		resolved: Option<String>,
		usage: UsageStatistics,
		context: Option<ContextUsage>,
		subscription: bool,
	}

	impl FooterSession for StubSession {
		fn model(&self) -> Option<&Value> {
			Some(&self.model)
		}

		fn thinking_level(&self) -> Option<&str> {
			self.thinking.as_deref()
		}

		fn is_auto_thinking(&self) -> bool {
			self.auto
		}

		fn auto_resolved_thinking_level(&self) -> Option<String> {
			self.resolved.clone()
		}

		fn context_usage(&self) -> Option<ContextUsage> {
			self.context
		}

		fn is_using_subscription(&self, _model: &Value) -> bool {
			self.subscription
		}

		fn usage_statistics(&self) -> UsageStatistics {
			self.usage
		}
	}

	struct StubHost {
		git: bool,
	}

	impl FooterHost for StubHost {
		fn git_enabled(&self) -> bool {
			self.git
		}
	}

	fn session() -> StubSession {
		StubSession {
			model: serde_json::json!({"id": "claude-x", "contextWindow": 200000, "thinking": true}),
			thinking: Some("high".to_string()),
			auto: false,
			resolved: None,
			usage: UsageStatistics {
				input: 1_200,
				output: 300,
				cache_read: 40,
				cache_write: 25,
				cost: 0.1234,
				premium_requests: 2,
				..UsageStatistics::default()
			},
			context: Some(ContextUsage {
				tokens: 100_000,
				context_window: 200_000,
				percent: Some(50.0),
			}),
			subscription: false,
		}
	}

	fn render(session: &StubSession, width: u16) -> Vec<String> {
		ActiveTheme::set_active(Rc::new(FallbackTheme::new()));
		let host = StubHost { git: true };
		let mut footer = FooterComponent::new(session, &host);
		footer.set_paths("/home/me/project", "/home/me");
		footer.set_git_branch(Some("main".to_string()));
		footer.render(width)
	}

	#[test]
	fn renders_path_stats_and_model() {
		let session = session();
		let lines = render(&session, 100);
		assert_eq!(lines.len(), 2);
		assert!(lines[0].contains("~/project (main)"), "{}", lines[0]);
		assert!(lines[1].contains("↑1.2k"), "{}", lines[1]);
		assert!(lines[1].contains("↓300"), "{}", lines[1]);
		assert!(lines[1].contains("R40"), "{}", lines[1]);
		assert!(lines[1].contains("W25"), "{}", lines[1]);
		assert!(lines[1].contains("$0.123"), "{}", lines[1]);
		assert!(lines[1].contains("★ 2"), "{}", lines[1]);
		assert!(lines[1].contains("50.0%/200k"), "{}", lines[1]);
		assert!(lines[1].contains("claude-x • high"), "{}", lines[1]);
	}

	#[test]
	fn model_without_thinking_omits_the_level() {
		let mut session = session();
		session.model["thinking"] = Value::Bool(false);
		let lines = render(&session, 120);
		assert!(lines[1].contains("claude-x"), "{}", lines[1]);
		assert!(!lines[1].contains("•"), "{}", lines[1]);
	}

	#[test]
	fn auto_thinking_shows_pending_marker_or_level() {
		let mut session = session();
		session.auto = true;
		session.resolved = None;
		let lines = render(&session, 120);
		assert!(lines[1].contains("claude-x • ⟳ auto"), "{}", lines[1]);

		session.resolved = Some("high".to_string());
		let lines = render(&session, 120);
		assert!(lines[1].contains("claude-x • high"), "{}", lines[1]);
	}

	#[test]
	fn zero_usage_renders_context_only() {
		let mut session = session();
		session.usage = UsageStatistics::default();
		let lines = render(&session, 120);
		assert!(lines[1].contains("50.0%"), "{}", lines[1]);
		assert!(!lines[1].contains("↑"), "{}", lines[1]);
	}

	#[test]
	fn extension_statuses_render_on_their_own_line() {
		ActiveTheme::set_active(Rc::new(FallbackTheme::new()));
		let session = session();
		let host = StubHost { git: true };
		let mut footer = FooterComponent::new(&session, &host);
		footer.set_paths("/w", "");
		footer.set_extension_status("b", Some("second".to_string()));
		footer.set_extension_status("a", Some("first\x1b[31m".to_string()));
		let lines = footer.render(120);
		assert_eq!(lines.len(), 3);
		assert_eq!(lines[2], "first second");
	}

	#[test]
	fn narrow_width_drops_the_model_side() {
		let session = session();
		let lines = render(&session, 20);
		assert_eq!(lines.len(), 2);
		assert!(visible_width(&strip_ansi(&lines[1])) <= 20);
	}

	#[test]
	fn unknown_window_renders_tokens_with_question_mark() {
		let mut session = session();
		session.context = None;
		session.model["contextWindow"] = Value::from(0);
		let lines = render(&session, 120);
		assert!(lines[1].contains("/?"), "{}", lines[1]);
	}

	#[test]
	fn subscription_without_cost_shows_the_sub_mark() {
		let mut session = session();
		session.subscription = true;
		session.usage.cost = 0.0;
		let lines = render(&session, 120);
		assert!(lines[1].contains("(sub)"), "{}", lines[1]);
	}
}
