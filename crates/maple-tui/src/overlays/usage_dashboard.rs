//! Usage dashboard overlay (subscriptions grid + activity heatmap).
//!
//! Reference: `l/src/overlays/usage-dashboard.ts`.
//!
//! Deviations:
//! - Runtime/data loading and rendering are trait-injected; this crate has no
//!   global theme, auth storage, or async overlay host.
//! - Visual output is plain text lines (no ANSI theme styling).

use std::collections::HashMap;

use crate::component::Component;
use crate::keybinding_matchers::{
    matches_select_cancel, matches_select_down, matches_select_page_down, matches_select_page_up,
    matches_select_up,
};
use crate::keybindings::KeybindingsManager;
use crate::keys::{is_arrow_down, is_arrow_up, is_ctrl_c, is_enter, is_escape, Key};
use crate::text_shape::{truncate_to_width, wrap_text_with_ansi, Ellipsis};

/// Local calendar-day activity consumed by the usage heatmap.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyActivityPoint {
    pub day: String,
    pub cost: f64,
    pub requests: u64,
}

/// One quota bucket on a provider card.
#[derive(Debug, Clone, PartialEq)]
pub struct CardWindowRow {
    pub label: String,
    pub window_tag: Option<String>,
    pub fraction: Option<f64>,
    pub status: String,
    pub reset_ms: Option<i64>,
    pub used_text: Option<String>,
}

/// Connected account whose usage lookup produced no attributable report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableUsageAccount {
    pub provider: String,
    pub label: String,
}

/// Reset-credit summary shown on a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardResetCredits {
    pub banked_count: u32,
    pub redeemable_count: u32,
    pub soonest_expiry_ms: Option<i64>,
    pub unavailable_reasons: Vec<String>,
}

/// Compact per-provider summary backing one card.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCard {
    pub provider: String,
    pub name: String,
    pub accounts: usize,
    pub unavailable_accounts: Vec<String>,
    pub windows: Vec<CardWindowRow>,
    pub unlimited: bool,
    pub notes: Vec<String>,
    pub idle: bool,
    pub reset_credits: Option<CardResetCredits>,
    pub daybreak_accounts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatmapLayout {
    pub month_labels: Vec<Option<String>>,
    pub cells: Vec<Vec<Option<u8>>>,
    pub total_cost_milli: i64,
    pub total_requests: u64,
    pub start_day: String,
}

const CARD_MAX_WINDOWS: usize = 3;
const DEFAULT_WEEKS: usize = 53;
const MONTH_NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Weekday column labels for the heatmap grid (Monday-first).
pub const HEATMAP_DAY_LABELS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

fn aggregate_status(statuses: &[&str]) -> &'static str {
    let has_ok = statuses.iter().any(|s| *s == "ok");
    let has_warning = statuses.iter().any(|s| *s == "warning");
    let has_exhausted = statuses.iter().any(|s| *s == "exhausted");
    if has_ok {
        return if has_warning || has_exhausted { "warning" } else { "ok" };
    }
    if has_warning {
        return "warning";
    }
    if has_exhausted {
        return "exhausted";
    }
    "unknown"
}

pub fn format_provider_name(provider: &str) -> String {
    if provider.is_empty() {
        return "Provider".to_string();
    }
    let mut out = String::new();
    let mut upper_next = true;
    for ch in provider.chars() {
        if ch == '-' || ch == '_' || ch == ' ' {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() { "Provider".to_string() } else { trimmed }
}

/// Sanitize an activity-loading error for safe single-line display: strip
/// ANSI/control sequences, expand tabs, collapse whitespace runs, shorten the
/// home directory to `~`, and drop trailing dots.
#[must_use]
pub fn format_activity_error_detail(error: &str, home_dir: Option<&str>) -> String {
    let sanitized = crate::overlays::extensions::display_text::sanitize_display_text(error);
    let mut text = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(home) = home_dir.filter(|h| !h.is_empty()) {
        text = replace_home_case_insensitive(&text, home);
        let forward = home.replace('\\', "/");
        if forward != home {
            text = replace_home_case_insensitive(&text, &forward);
        }
    }
    text.trim_end_matches('.').to_string()
}

/// Replace every case-insensitive literal occurrence of `needle` with `~`.
fn replace_home_case_insensitive(text: &str, needle: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let needle_chars: Vec<char> = needle.to_lowercase().chars().collect();
    let chars: Vec<char> = text.chars().collect();
    let width = needle_chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let matches = i + width <= chars.len()
            && chars[i..i + width]
                .iter()
                .flat_map(|c| c.to_lowercase())
                .eq(needle_chars.iter().copied());
        if matches {
            out.push('~');
            i += width;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn parse_ymd(day: &str) -> Option<(i32, u32, u32)> {
    let mut parts = day.split('-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((y, m, d))
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m as i64 + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn iso_day(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn weekday_mon0(days: i64) -> usize {
    ((days + 3).rem_euclid(7)) as usize
}

/// Build/sort provider cards (cap windows, dedupe tags/notes, derive idle/unlimited).
pub fn build_provider_cards(mut inputs: Vec<ProviderCard>) -> Vec<ProviderCard> {
    for card in &mut inputs {
        if card.windows.len() > CARD_MAX_WINDOWS {
            card.windows.sort_by(|a, b| {
                b.fraction
                    .partial_cmp(&a.fraction)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut visible = card.windows.iter().take(CARD_MAX_WINDOWS).cloned().collect::<Vec<_>>();
            let rest = card.windows.iter().skip(CARD_MAX_WINDOWS).cloned().collect::<Vec<_>>();
            visible.extend(rest);
            card.windows = visible;
        }
        let labels: Vec<String> = card.windows.iter().map(|w| w.label.clone()).collect();
        for (i, w) in card.windows.iter_mut().enumerate() {
            let dup = labels.iter().enumerate().any(|(j, l)| j != i && l == &w.label);
            if !dup {
                w.window_tag = None;
            }
        }
        card.idle = card.unavailable_accounts.is_empty()
            && card.reset_credits.is_none()
            && card.daybreak_accounts.is_empty()
            && card.windows.iter().all(|w| w.fraction.unwrap_or(0.0) < 0.005);
        card.unlimited = card.windows.is_empty() && card.unavailable_accounts.is_empty();
        card.notes.sort();
        card.notes.dedup();
    }
    inputs.sort_by(|a, b| {
        let aw = a.windows.iter().filter_map(|w| w.fraction).fold(-1.0f64, f64::max);
        let bw = b.windows.iter().filter_map(|w| w.fraction).fold(-1.0f64, f64::max);
        bw.partial_cmp(&aw)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    inputs
}

/// Build a Monday-first week heatmap ending at `today_day` (YYYY-MM-DD).
pub fn build_heatmap_layout(points: &[DailyActivityPoint], weeks: usize, today_day: &str) -> HeatmapLayout {
    let weeks = weeks.max(1);
    let by_day: HashMap<&str, &DailyActivityPoint> = points.iter().map(|p| (p.day.as_str(), p)).collect();
    let today_days = parse_ymd(today_day).map(|(y, m, d)| days_from_civil(y, m, d)).unwrap_or(0);
    let monday_offset = weekday_mon0(today_days) as i64;
    let current_monday = today_days - monday_offset;
    let start_days = current_monday - ((weeks as i64 - 1) * 7);
    let start_iso = iso_day(start_days);

    let in_range: Vec<&DailyActivityPoint> = points
        .iter()
        .filter(|p| p.day.as_str() >= start_iso.as_str() && p.day.as_str() <= today_day)
        .collect();
    let any_cost = in_range.iter().any(|p| p.cost > 0.0);
    let metric = |p: &DailyActivityPoint| if any_cost { p.cost } else { p.requests as f64 };
    let max = in_range.iter().map(|p| metric(p)).fold(0.0f64, f64::max);
    let level = |value: f64| -> u8 {
        if value <= 0.0 || max <= 0.0 {
            0
        } else {
            ((value / max).sqrt() * 4.0).ceil().clamp(1.0, 4.0) as u8
        }
    };

    let mut month_labels = vec![None; weeks];
    let mut cells = vec![vec![None; weeks]; 7];
    let mut prev_month = 0u32;
    for week in 0..weeks {
        let week_start = start_days + (week as i64 * 7);
        let (_, m, _) = civil_from_days(week_start);
        if week == 0 || m != prev_month {
            month_labels[week] = Some(MONTH_NAMES[(m.saturating_sub(1)) as usize].to_string());
            prev_month = m;
        }
        for day in 0..7usize {
            let d = week_start + day as i64;
            if d > today_days {
                continue;
            }
            let iso = iso_day(d);
            let value = by_day.get(iso.as_str()).map(|p| metric(p)).unwrap_or(0.0);
            cells[day][week] = Some(level(value));
        }
    }

    let total_requests = in_range.iter().map(|p| p.requests).sum();
    let total_cost_milli = (in_range.iter().map(|p| p.cost).sum::<f64>() * 1000.0).round() as i64;
    HeatmapLayout {
        month_labels,
        cells,
        total_cost_milli,
        total_requests,
        start_day: start_iso,
    }
}

/// Dependencies for fetching/closing/render requests.
pub trait UsageDashboardRuntime: Send {
    fn load_cards(&mut self) -> Vec<ProviderCard>;
    fn load_activity(&mut self, weeks: usize, today_day: &str) -> Vec<DailyActivityPoint>;
    fn refresh(&mut self) {}
    fn close(&mut self) {}
    fn request_render(&mut self) {}
    fn keybindings(&self) -> Option<&KeybindingsManager> {
        None
    }
}

pub struct UsageDashboardOptions<R: UsageDashboardRuntime> {
    pub runtime: R,
    pub terminal_height: Option<usize>,
    pub today_day: String,
}

/// Fullscreen `/usage` dashboard component.
pub struct UsageDashboard<R: UsageDashboardRuntime> {
    runtime: R,
    cards: Vec<ProviderCard>,
    heatmap: HeatmapLayout,
    detail_mode: bool,
    scroll_offset: usize,
    selected_card: usize,
    terminal_height: usize,
    today_day: String,
    weeks: usize,
    closed: bool,
}

impl<R: UsageDashboardRuntime> UsageDashboard<R> {
    pub fn new(options: UsageDashboardOptions<R>) -> Self {
        let terminal_height = options.terminal_height.unwrap_or(24);
        let weeks = DEFAULT_WEEKS;
        let mut runtime = options.runtime;
        let cards = build_provider_cards(runtime.load_cards());
        let activity = runtime.load_activity(weeks, &options.today_day);
        let heatmap = build_heatmap_layout(&activity, weeks, &options.today_day);
        Self {
            runtime,
            cards,
            heatmap,
            detail_mode: false,
            scroll_offset: 0,
            selected_card: 0,
            terminal_height,
            today_day: options.today_day,
            weeks,
            closed: false,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn refresh(&mut self) {
        self.runtime.refresh();
        self.cards = build_provider_cards(self.runtime.load_cards());
        let activity = self.runtime.load_activity(self.weeks, &self.today_day);
        self.heatmap = build_heatmap_layout(&activity, self.weeks, &self.today_day);
        if self.cards.is_empty() {
            self.selected_card = 0;
        } else {
            self.selected_card = self.selected_card.min(self.cards.len() - 1);
        }
        self.runtime.request_render();
    }

    fn close(&mut self) {
        self.closed = true;
        self.runtime.close();
    }

    fn render_overview(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();
		lines.push(truncate_to_width("Usage", w, Ellipsis::Unicode));
		lines.push(truncate_to_width(
			"Subscriptions grid + local activity heatmap. Enter: details  r: refresh  Esc: close",
			w,
			Ellipsis::Unicode,
		));
        lines.push(String::new());

        if self.cards.is_empty() {
			lines.push(truncate_to_width("No usage providers connected.", w, Ellipsis::Unicode));
        } else {
            for (i, card) in self.cards.iter().enumerate() {
                let marker = if i == self.selected_card { ">" } else { " " };
                let statuses: Vec<&str> = card.windows.iter().map(|win| win.status.as_str()).collect();
                let status = if statuses.is_empty() {
                    if card.unlimited { "unlimited" } else { "unknown" }
                } else {
                    aggregate_status(&statuses)
                };
				lines.push(truncate_to_width(
					&format!("{marker} {}  ({} accts, {status})", card.name, card.accounts),
					w,
					Ellipsis::Unicode,
				));
                if card.unlimited {
					lines.push(truncate_to_width("    unlimited / no quota windows", w, Ellipsis::Unicode));
                }
                for window in card.windows.iter().take(CARD_MAX_WINDOWS) {
                    let pct = window
                        .fraction
                        .map(|f| format!("{:>3.0}%", (f * 100.0).clamp(0.0, 999.0)))
                        .unwrap_or_else(|| "  -".to_string());
                    let tag = window
                        .window_tag
                        .as_ref()
                        .map(|t| format!(" {t}"))
                        .unwrap_or_default();
                    let used = window.used_text.clone().unwrap_or_default();
					lines.push(truncate_to_width(
						&format!("    {:<14} {}{} {}", window.label, pct, tag, used),
						w,
						Ellipsis::Unicode,
					));
                }
                if !card.unavailable_accounts.is_empty() {
					lines.push(truncate_to_width(
						&format!("    unavailable: {}", card.unavailable_accounts.join(", ")),
						w,
						Ellipsis::Unicode,
					));
                }
                if let Some(reset) = &card.reset_credits {
					lines.push(truncate_to_width(
						&format!(
							"    resets banked: {} (redeemable {})",
							reset.banked_count, reset.redeemable_count
						),
						w,
						Ellipsis::Unicode,
					));
                }
                lines.push(String::new());
            }
        }

		lines.push(truncate_to_width("Activity", w, Ellipsis::Unicode));
		lines.push(truncate_to_width(&self.render_heatmap_row(), w, Ellipsis::Unicode));
		lines.push(truncate_to_width(
			&format!(
				"Past year: {} requests, ${:.2}",
				self.heatmap.total_requests,
				self.heatmap.total_cost_milli as f64 / 1000.0
			),
			w,
			Ellipsis::Unicode,
		));
        lines
    }

    fn render_heatmap_row(&self) -> String {
        let mut out = String::from("  ");
        let weeks = self.heatmap.cells.first().map(|r| r.len()).unwrap_or(0);
        for week in 0..weeks {
            let mut max_lvl = 0u8;
            for day in 0..7 {
                if let Some(Some(v)) = self.heatmap.cells.get(day).and_then(|r| r.get(week)) {
                    max_lvl = max_lvl.max(*v);
                }
            }
            out.push(match max_lvl {
                0 => '·',
                1 => '░',
                2 => '▒',
                3 => '▓',
                _ => '█',
            });
        }
        out
    }

    fn render_detail(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();
		lines.push(truncate_to_width("Usage Details", w, Ellipsis::Unicode));
		lines.push(truncate_to_width("Esc: back   r: refresh", w, Ellipsis::Unicode));
        lines.push(String::new());
        for card in &self.cards {
			lines.push(truncate_to_width(&format!("{} ({})", card.name, card.provider), w, Ellipsis::Unicode));
			lines.push(truncate_to_width(&format!("Accounts: {}", card.accounts), w, Ellipsis::Unicode));
			if card.unlimited {
				lines.push(truncate_to_width("Unlimited / no quota windows reported.", w, Ellipsis::Unicode));
			}
            for win in &card.windows {
                let pct = win
                    .fraction
                    .map(|f| format!("{:.1}%", f * 100.0))
                    .unwrap_or_else(|| "n/a".to_string());
                let reset = win
                    .reset_ms
                    .map(|ms| format!(" reset in {}s", (ms / 1000).max(0)))
                    .unwrap_or_default();
				lines.push(truncate_to_width(
					&format!("  - {}: {} {}{}", win.label, pct, win.status, reset),
					w,
					Ellipsis::Unicode,
				));
                if let Some(used) = &win.used_text {
					for l in wrap_text_with_ansi(used, w.saturating_sub(4).max(8)) {
						lines.push(truncate_to_width(&format!("    {l}"), w, Ellipsis::Unicode));
					}
                }
            }
            for note in &card.notes {
				for l in wrap_text_with_ansi(note, w.saturating_sub(6).max(8)) {
					lines.push(truncate_to_width(&format!("  note: {l}"), w, Ellipsis::Unicode));
				}
            }
            if !card.unavailable_accounts.is_empty() {
				lines.push(truncate_to_width(
					&format!("  unavailable: {}", card.unavailable_accounts.join(", ")),
					w,
					Ellipsis::Unicode,
				));
            }
            lines.push(String::new());
        }
        if self.scroll_offset >= lines.len() {
			vec![truncate_to_width("(end)", w, Ellipsis::Unicode)]
        } else {
            lines.into_iter().skip(self.scroll_offset).collect()
        }
    }

    fn page_size(&self) -> usize {
        self.terminal_height.saturating_sub(6).max(5)
    }
}

impl<R: UsageDashboardRuntime> Component for UsageDashboard<R> {
    fn render(&mut self, width: u16) -> Vec<String> {
        if self.detail_mode {
            self.render_detail(width)
        } else {
            self.render_overview(width)
        }
    }

    fn handle_input(&mut self, key: &Key) {
        let kb = self.runtime.keybindings();
        let up = is_arrow_up(key) || kb.is_some_and(|k| matches_select_up(key, k));
        let down = is_arrow_down(key) || kb.is_some_and(|k| matches_select_down(key, k));
        let page_up = kb.is_some_and(|k| matches_select_page_up(key, k));
        let page_down = kb.is_some_and(|k| matches_select_page_down(key, k));
        let cancel = is_escape(key) || is_ctrl_c(key) || kb.is_some_and(|k| matches_select_cancel(key, k));

        if cancel {
            if self.detail_mode {
                self.detail_mode = false;
                self.scroll_offset = 0;
                self.runtime.request_render();
            } else {
                self.close();
            }
            return;
        }

        if matches!(key, Key::Char('r' | 'R')) {
            self.refresh();
            return;
        }

        if is_enter(key) && !self.detail_mode {
            self.detail_mode = true;
            self.scroll_offset = 0;
            self.runtime.request_render();
            return;
        }

        if self.detail_mode {
            if up {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            } else if down {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            } else if page_up {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.page_size());
            } else if page_down {
                self.scroll_offset = self.scroll_offset.saturating_add(self.page_size());
            }
            return;
        }

        if up {
            self.selected_card = self.selected_card.saturating_sub(1);
        } else if down && !self.cards.is_empty() {
            self.selected_card = (self.selected_card + 1).min(self.cards.len() - 1);
        } else if page_up {
            self.selected_card = self.selected_card.saturating_sub(self.page_size());
        } else if page_down && !self.cards.is_empty() {
            self.selected_card = (self.selected_card + self.page_size()).min(self.cards.len() - 1);
        }
    }

    fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRuntime {
        cards: Vec<ProviderCard>,
        activity: Vec<DailyActivityPoint>,
        refreshed: usize,
        closed: bool,
    }

    impl UsageDashboardRuntime for MockRuntime {
        fn load_cards(&mut self) -> Vec<ProviderCard> {
            self.cards.clone()
        }
        fn load_activity(&mut self, _weeks: usize, _today_day: &str) -> Vec<DailyActivityPoint> {
            self.activity.clone()
        }
        fn refresh(&mut self) {
            self.refreshed += 1;
        }
        fn close(&mut self) {
            self.closed = true;
        }
    }

    fn card(name: &str, frac: f64) -> ProviderCard {
        ProviderCard {
            provider: name.to_lowercase(),
            name: name.to_string(),
            accounts: 1,
            unavailable_accounts: vec![],
            windows: vec![CardWindowRow {
                label: "5h".into(),
                window_tag: None,
                fraction: Some(frac),
                status: "ok".into(),
                reset_ms: None,
                used_text: None,
            }],
            unlimited: false,
            notes: vec![],
            idle: false,
            reset_credits: None,
            daybreak_accounts: vec![],
        }
    }

    #[test]
    fn build_provider_cards_sorts_by_worst_usage() {
        let cards = build_provider_cards(vec![card("Low", 0.1), card("High", 0.9)]);
        assert_eq!(cards[0].name, "High");
    }

    #[test]
    fn build_heatmap_layout_has_shape_and_totals() {
        let points = vec![
            DailyActivityPoint { day: "2026-11-16".into(), cost: 2.0, requests: 5 },
            DailyActivityPoint { day: "2026-11-15".into(), cost: 1.0, requests: 2 },
        ];
        let hm = build_heatmap_layout(&points, 2, "2026-11-16");
        assert_eq!(hm.cells.len(), 7);
        assert_eq!(hm.cells[0].len(), 2);
        assert_eq!(hm.total_requests, 7);
    }

    #[test]
    fn dashboard_renders_and_toggles_detail() {
        let rt = MockRuntime {
            cards: vec![card("OpenAI", 0.4)],
            activity: vec![DailyActivityPoint { day: "2026-11-16".into(), cost: 1.0, requests: 1 }],
            refreshed: 0,
            closed: false,
        };
        let mut dash = UsageDashboard::new(UsageDashboardOptions {
            runtime: rt,
            terminal_height: Some(24),
            today_day: "2026-11-16".into(),
        });
        let overview = dash.render(80).join("\n");
        assert!(overview.contains("Usage"));
        assert!(overview.contains("OpenAI"));
        dash.handle_input(&Key::Enter);
        let detail = dash.render(80).join("\n");
        assert!(detail.contains("Usage Details"));
        dash.handle_input(&Key::Esc);
        assert!(!dash.is_closed());
        let overview2 = dash.render(80).join("\n");
        assert!(overview2.contains("Enter: details"));
    }

    #[test]
    fn format_provider_name_titleizes() {
        assert_eq!(format_provider_name("open-ai"), "Open Ai");
        assert_eq!(format_provider_name("github_copilot"), "Github Copilot");
    }

    #[test]
    fn heatmap_day_labels_are_monday_first() {
        assert_eq!(HEATMAP_DAY_LABELS, ["M", "T", "W", "T", "F", "S", "S"]);
    }

    #[test]
    fn format_activity_error_detail_sanitizes_and_shortens_home() {
        let err = "\u{1b}[31mboom\u{1b}[0m\tat /home/fahzan/x\r\nnext...";
        assert_eq!(
            format_activity_error_detail(err, Some("/home/fahzan")),
            "boom at ~/x next"
        );
        assert_eq!(format_activity_error_detail("  a\tb  ", None), "a b");
    }
}
