//! Hub surface: sidebar roster, inspector body, and frame chrome.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::theme::{self, Tone};

use super::frame::HubFrame;
use super::roster::Peer;
use super::row::{self, age_label};
use crate::tui::app::App;

pub fn draw_hub(f: &mut Frame, app: &mut App, area: Rect) {
    let frame = HubFrame::new(super::frame::TITLE, super::frame::SIDEBAR_MIN, super::frame::SIDEBAR_MAX);
    let sidebar = frame.sidebar_width(&app.hub.roster);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    f.render_widget(Paragraph::new(frame.top(area.width, sidebar)), rows[0]);
    let body = Layout::horizontal([Constraint::Length(sidebar), Constraint::Min(20)]).split(rows[1]);
    draw_sidebar(f, app, body[0]);
    draw_detail(f, app, body[1]);
    f.render_widget(Paragraph::new(frame.divider(area.width, sidebar)), rows[2]);
    f.render_widget(
        Paragraph::new(frame.chips(area.width, &[super::super::nav::CHIP_AGENTS, super::super::nav::CHIP_ACTIVITY], 0)),
        rows[3],
    );
    f.render_widget(Paragraph::new(frame.bottom(area.width, sidebar)), rows[4]);
}

/// Roster rows visible under the current filter, with the selection kept in view.
pub fn filtered<'a>(roster: &'a [Peer], filter: &str) -> Vec<&'a Peer> {
    let needle = filter.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return roster.iter().collect();
    }
    roster
        .iter()
        .filter(|peer| {
            let haystack = format!(
                "{} {} {} {}",
                peer.session_id,
                peer.title.clone().unwrap_or_default(),
                peer.cwd,
                peer.task
            )
            .to_ascii_lowercase();
            haystack.contains(&needle)
        })
        .collect()
}

fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    let peers = filtered(&app.hub.roster, &app.hub.filter);
    let branches = if app.hub.tree {
        row::tree_branches(&app.hub.roster)
    } else {
        Default::default()
    };
    let width = usize::from(area.width).saturating_sub(1);
    let height = usize::from(area.height);
    let selected = app.hub.selected.min(peers.len().saturating_sub(1));
    let scroll = selected.saturating_sub(height.saturating_sub(1));

    let mut lines: Vec<Line<'static>> = Vec::new();
    for peer in peers.iter().skip(scroll).take(height) {
        let is_selected = peer.session_id == peers.get(selected).map(|p| p.session_id.as_str()).unwrap_or("");
        let branch = branches.get(&peer.session_id).map(String::as_str);
        lines.extend(row::entry_rows(peer, is_selected, width, branch));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let peers = filtered(&app.hub.roster, &app.hub.filter);
    let selected = app.hub.selected.min(peers.len().saturating_sub(1));
    let lines = match peers.get(selected) {
        Some(peer) => inspector(peer),
        None => vec![Line::from(theme::span(
            "No agent selected".to_string(),
            Tone::Dim,
        ))],
    };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

/// Inspector body: identity, task, usage, lineage.
pub fn inspector(peer: &Peer) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        theme::span(
            format!("{} ", row::status_glyph(peer.status)),
            status_tone(peer.status),
        ),
        theme::span(peer.session_id.clone(), Tone::Accent),
        theme::span(format!(" · {}", peer.status.label()), Tone::Dim),
    ]));
    lines.push(kv("model", &peer.model));
    lines.push(kv("tool", &peer.tool));
    lines.push(kv("age", &age_label(peer.age_ms)));
    lines.push(Line::from(""));
    lines.push(section("Task"));
    if peer.task.is_empty() {
        lines.push(Line::from(theme::span("—".to_string(), Tone::Muted)));
    } else {
        lines.push(Line::from(theme::span(peer.task.clone(), Tone::Dim)));
    }
    lines.push(Line::from(""));
    lines.push(section("Usage"));
    lines.push(Line::from(theme::span(
        row::compact_metrics(peer, &age_label(peer.age_ms)),
        Tone::Dim,
    )));
    lines.push(Line::from(theme::span(
        row::context_gauge(peer.tokens, 200_000),
        Tone::Muted,
    )));
    lines.push(Line::from(""));
    lines.push(section("Lineage"));
    lines.push(kv("Parent", peer.parent.as_deref().unwrap_or("—")));
    lines.push(kv(
        "Registered",
        peer.registered_at.get(..19).unwrap_or(&peer.registered_at),
    ));
    lines.push(kv("cwd", &peer.cwd));
    lines
        .into_iter()
        .map(|line| {
            if line.spans.iter().all(|span| span.content.is_empty()) {
                line
            } else {
                let mut spans = vec![Span::raw(" ")];
                spans.extend(line.spans);
                Line::from(spans)
            }
        })
        .collect()
}

fn status_tone(status: super::roster::Status) -> Tone {
    match status {
        super::roster::Status::Active => Tone::Success,
        super::roster::Status::Busy => Tone::Accent,
        super::roster::Status::Idle => Tone::Muted,
        super::roster::Status::Error => Tone::Error,
    }
}

fn kv(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        theme::span(format!("{label:<11}"), Tone::Muted),
        theme::span(value.to_string(), Tone::Dim),
    ])
}

fn section(title: &str) -> Line<'static> {
    Line::from(theme::span(title.to_string(), Tone::Accent))
}
