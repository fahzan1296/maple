//! Roster row layout: identity, task, metrics, and tree rails.

use ratatui::style::{Color, Style};
use ratatui::text::Line;

use crate::tui::glyph;
use crate::tui::nav;
use crate::tui::theme::{self, Tone};

use super::roster::{Peer, Status};

/// Selected-row background, matching omp's `selectedBg`.
pub const SELECTED_BG: Color = Color::Indexed(237);

pub fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        "$0.0000".to_string()
    } else if cost < 1.0 {
        format!("${cost:.3}")
    } else {
        format!("${cost:.2}")
    }
}

pub fn format_duration(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else if ms < 3_600_000 {
        format!("{}m", ms / 60_000)
    } else {
        format!("{}h", ms / 3_600_000)
    }
}

/// Duration fact, omitting the unit when there is no measured span.
pub fn format_metric_duration(ms: u64) -> Option<String> {
    if ms == 0 {
        None
    } else {
        Some(format_duration(ms))
    }
}

pub fn format_number(value: u64) -> String {
    if value < 1_000 {
        value.to_string()
    } else if value < 1_000_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    }
}

/// Compact age label for the roster grid (`2m`, `1h`, `3d`).
pub fn age_label(age_ms: u128) -> String {
    let ms = age_ms.min(u64::MAX as u128) as u64;
    if ms < 60_000 {
        format!("{}s", ms / 1_000)
    } else if ms < 3_600_000 {
        format!("{}m", ms / 60_000)
    } else if ms < 86_400_000 {
        format!("{}h", ms / 3_600_000)
    } else {
        format!("{}d", ms / 86_400_000)
    }
}

/// Row-grid metrics: one shared column layout for every agent.
pub fn metric_columns(peer: &Peer, age: &str) -> String {
    [
        cell(&format_cost(peer.cost), 8, false),
        cell(&format_metric_duration(peer.duration_ms).unwrap_or_else(|| "—".to_string()), 13, true),
        cell(&format!("{} req", format_number(peer.requests)), 8, true),
        cell(&format!("{} tools", format_number(peer.tools)), 9, true),
        cell(&format!("{} tok", format_number(peer.tokens)), 8, true),
        cell(age, 8, true),
    ]
    .join(" ")
}

/// Sidebar-floor metrics: the facts that fit a narrow rail.
pub fn mini_metrics(peer: &Peer, _age: &str) -> String {
    [
        format_cost(peer.cost),
        format!("{} req", format_number(peer.requests)),
        format!("{} tok", format_number(peer.tokens)),
    ]
    .join(nav::SEP_DOT)
}

/// Narrow-variant metrics: dot-separated facts that wrap cleanly in a sidebar.
pub fn compact_metrics(peer: &Peer, age: &str) -> String {
    [
        format_cost(peer.cost),
        format_metric_duration(peer.duration_ms).unwrap_or_else(|| "—".to_string()),
        format!("{} req", format_number(peer.requests)),
        format!("{} tools", format_number(peer.tools)),
        format!("{} tok", format_number(peer.tokens)),
        age.to_string(),
    ]
    .join(nav::SEP_DOT)
}

fn cell(text: &str, width: usize, right: bool) -> String {
    let text = theme::truncate(text, width);
    let pad = width.saturating_sub(theme::visible_width(&text));
    if right {
        format!("{}{}", " ".repeat(pad), text)
    } else {
        format!("{}{}", text, " ".repeat(pad))
    }
}

/// Context gauge: `━━━─────── 1.2K/200K 6%`.
pub fn context_gauge(tokens: u64, window: u64) -> String {
    if window == 0 {
        return format!("{} tokens", format_number(tokens));
    }
    let ratio = (tokens as f64 / window as f64).clamp(0.0, 1.0);
    let filled = (ratio * 10.0).round() as usize;
    let bar = format!(
        "{}{}",
        glyph::SEPARATOR.repeat(filled),
        "─".repeat(10 - filled.min(10))
    );
    format!(
        "{} {}/{} {}%",
        bar,
        format_number(tokens),
        format_number(window),
        (ratio * 100.0).round() as u64
    )
}

pub fn status_glyph(status: Status) -> &'static str {
    match status {
        Status::Active => glyph::STATUS_ACTIVE,
        Status::Busy => glyph::STATUS_BUSY,
        Status::Idle => glyph::STATUS_IDLE,
        Status::Error => glyph::STATUS_ERROR,
    }
}

/// Three-row entry: identity, task, metrics. `branch` carries tree rails.
pub fn entry_rows(
    peer: &Peer,
    selected: bool,
    width: usize,
    branch: Option<&str>,
) -> Vec<Line<'static>> {
    let rail = branch.unwrap_or("  ");
    let cursor = if selected { glyph::CURSOR } else { " " };
    let label = peer
        .title
        .clone()
        .or_else(|| {
            if peer.task.is_empty() {
                None
            } else {
                Some(peer.task.clone())
            }
        })
        .unwrap_or_else(|| peer.cwd.clone());
    let identity = format!(
        "{rail}{cursor} {} {}  {}",
        status_glyph(peer.status),
        short_id(&peer.session_id),
        theme::truncate(&label, width.saturating_sub(12)),
    );
    let indent = " ".repeat(theme::visible_width(rail) + 2);
    let age = age_label(peer.age_ms);
    let metrics = if width >= 64 {
        metric_columns(peer, &age)
    } else if width >= 44 {
        compact_metrics(peer, &age)
    } else {
        mini_metrics(peer, &age)
    };
    vec![
        row_line(identity, width, selected, Tone::Accent),
        row_line(
            format!("{indent}{}", theme::truncate(&peer.task, width.saturating_sub(4))),
            width,
            selected,
            Tone::Dim,
        ),
        row_line(format!("{indent}{metrics}"), width, selected, Tone::Muted),
    ]
}

fn row_line(text: String, width: usize, selected: bool, tone: Tone) -> Line<'static> {
    let fitted = theme::fit(&text, width);
    if selected {
        Line::styled(fitted, Style::default().bg(SELECTED_BG).fg(tone.color()))
    } else {
        Line::styled(fitted, Style::default().fg(tone.color()))
    }
}

/// Session id short form used in rows and the inspector.
pub fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// Ordered tree branches for a roster: maps session id to its rail prefix.
pub fn tree_branches(peers: &[Peer]) -> std::collections::HashMap<String, String> {
    let parent: std::collections::HashMap<String, String> = peers
        .iter()
        .filter_map(|p| p.parent.clone().map(|par| (p.session_id.clone(), par)))
        .collect();
    let mut last: std::collections::HashSet<String> = std::collections::HashSet::new();
    for peer in peers {
        if let Some(par) = peer.parent.as_deref() {
            let siblings: Vec<&Peer> = peers
                .iter()
                .filter(|p| p.parent.as_deref() == Some(par))
                .collect();
            if siblings.last().is_some_and(|p| p.session_id == peer.session_id) {
                last.insert(peer.session_id.clone());
            }
        }
    }
    let mut out = std::collections::HashMap::new();
    for peer in peers {
        let mut chain = Vec::new();
        let mut cur = peer.session_id.clone();
        while let Some(par) = parent.get(&cur) {
            chain.push(par.clone());
            cur = par.clone();
            if chain.len() > 32 {
                break;
            }
        }
        let mut prefix = String::new();
        for ancestor in chain.iter().take(chain.len().saturating_sub(1)) {
            prefix.push_str(if last.contains(ancestor) {
                glyph::TREE_SPACE
            } else {
                glyph::TREE_VERT
            });
        }
        if !chain.is_empty() {
            prefix.push_str(if last.contains(&peer.session_id) {
                glyph::TREE_LAST
            } else {
                glyph::TREE_BRANCH
            });
        }
        let prefix = if prefix.is_empty() { "  ".to_string() } else { prefix };
        out.insert(peer.session_id.clone(), prefix);
    }
    out
}
