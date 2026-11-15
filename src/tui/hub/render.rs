//! Hub surface: sidebar roster, inspector body, and frame chrome.

use maple_tui::style::Style;
use maple_tui::text_shape::{pad_to_width, truncate_to_width, wrap_text_with_ansi, Ellipsis};

use crate::tui::theme::{self, Tone};

use super::frame::HubFrame;
use super::row::short_id;
use super::roster::{Activity, Peer};
use super::row::{self, age_label};
use crate::tui::app::{App, AskForm};

/// Hub rows for a `width × height` viewport: top border, body, divider, chips,
/// bottom border.
pub fn hub_rows(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let frame = HubFrame::new(super::frame::TITLE, super::frame::SIDEBAR_MIN, super::frame::SIDEBAR_MAX);
    let sidebar = frame.sidebar_width(&app.hub.roster);
    let body_height = usize::from(height.saturating_sub(4));
    let mut rows = Vec::with_capacity(usize::from(height));
    rows.push(truncate_to_width(&frame.top(width, sidebar), usize::from(width), Ellipsis::Omit));

    let left = if app.hub.tab == crate::tui::nav::Tab::Activity {
        activity_rows(app, sidebar, body_height)
    } else {
        sidebar_rows(app, sidebar, body_height)
    };
    let right = detail_rows(app, width.saturating_sub(sidebar), body_height);
    for index in 0..body_height {
        let left_line = left.get(index).map(String::as_str).unwrap_or("");
        let right_line = right.get(index).map(String::as_str).unwrap_or("");
        let joined = format!(
            "{}{}",
            pad_to_width(left_line, usize::from(sidebar)),
            right_line
        );
        rows.push(truncate_to_width(
            &joined,
            usize::from(width),
            Ellipsis::Omit,
        ));
    }

    rows.push(truncate_to_width(
        &frame.divider(width, sidebar),
        usize::from(width),
        Ellipsis::Omit,
    ));
    let tab_index = match app.hub.tab {
        crate::tui::nav::Tab::Agents => 0,
        crate::tui::nav::Tab::Activity => 1,
    };
    rows.push(truncate_to_width(
        &frame.chips(
            width,
            &[crate::tui::nav::CHIP_AGENTS, crate::tui::nav::CHIP_ACTIVITY],
            tab_index,
        ),
        usize::from(width),
        Ellipsis::Omit,
    ));
    rows.push(truncate_to_width(
        &frame.bottom(width, sidebar),
        usize::from(width),
        Ellipsis::Omit,
    ));
    rows
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

fn sidebar_rows(app: &mut App, width: u16, height: usize) -> Vec<String> {
    let peers = filtered(&app.hub.roster, &app.hub.filter);
    let branches = if app.hub.tree {
        row::tree_branches(&app.hub.roster)
    } else {
        Default::default()
    };
    let inner_width = usize::from(width).saturating_sub(1);
    let selected = app.hub.selected.min(peers.len().saturating_sub(1));
    let scroll = selected.saturating_sub(height.saturating_sub(1));

    let mut lines = Vec::new();
    for peer in peers.iter().skip(scroll).take(height) {
        let is_selected = peer.session_id
            == peers
                .get(selected)
                .map(|peer| peer.session_id.as_str())
                .unwrap_or("");
        let branch = branches.get(&peer.session_id).map(String::as_str);
        lines.extend(row::entry_rows(peer, is_selected, inner_width, branch));
    }
    lines
}

/// Activity feed rows: `title  kind  text`, newest first.
fn activity_rows(app: &App, width: u16, height: usize) -> Vec<String> {
    let items = app.filtered_activity();
    let inner_width = usize::from(width).saturating_sub(1);
    let selected = app.hub.activity_selected.min(items.len().saturating_sub(1));
    let scroll = selected.saturating_sub(height.saturating_sub(1));

    let mut lines = Vec::new();
    for item in items.iter().skip(scroll).take(height) {
        let is_selected = item.session_id
            == items
                .get(selected)
                .map(|entry| entry.session_id.as_str())
                .unwrap_or("")
            && item.timestamp
                == items
                    .get(selected)
                    .map(|entry| entry.timestamp.as_str())
                    .unwrap_or("");
        let text = format!(
            "{} {}  {}",
            theme::truncate(&item.title, inner_width.saturating_sub(24)),
            theme::truncate(&item.kind, 8),
            theme::truncate(&item.text, inner_width.saturating_sub(24))
        );
        let fitted = theme::fit(&text, inner_width);
        if is_selected {
            lines.push(
                Style {
                    fg: Some(Tone::Accent.color()),
                    bg: Some(row::SELECTED_BG),
                    ..Style::default()
                }
                .paint(&fitted),
            );
        } else {
            lines.push(fitted);
        }
    }
    lines
}

/// Inspector (Agents tab) or the selected activity entry (Activity tab).
fn detail_rows(app: &App, width: u16, height: usize) -> Vec<String> {
    let lines = if app.hub.tab == crate::tui::nav::Tab::Activity {
        let items = app.filtered_activity();
        let selected = app.hub.activity_selected.min(items.len().saturating_sub(1));
        match items.get(selected) {
            Some(item) => activity_detail(item),
            None => vec![theme::span("No activity yet".to_string(), Tone::Dim)],
        }
    } else {
        let peers = filtered(&app.hub.roster, &app.hub.filter);
        let selected = app.hub.selected.min(peers.len().saturating_sub(1));
        match peers.get(selected) {
            Some(peer) => inspector(peer),
            None => vec![theme::span("No agent selected".to_string(), Tone::Dim)],
        }
    };
    wrap_rows(lines, usize::from(width))
        .into_iter()
        .take(height)
        .collect()
}

fn activity_detail(item: &Activity) -> Vec<String> {
    vec![
        section("Entry"),
        kv("session", &short_id(&item.session_id)),
        kv("title", &item.title),
        kv("kind", &item.kind),
        kv("model", &item.model),
        kv("time", &item.timestamp),
        String::new(),
        section("Text"),
        theme::span(item.text.clone(), Tone::Dim),
    ]
}

/// Ask overlay: compose a message into the selected peer's transcript.
pub fn ask_rows(form: &AskForm, width: u16, height: u16) -> Vec<String> {
    let box_width = width * 3 / 5;
    let box_height = 7u16;
    let x = usize::from(width.saturating_sub(box_width) / 2);
    let y = usize::from(height.saturating_sub(box_height) / 2);
    let inner = usize::from(box_width).saturating_sub(2);

    let mut canvas = vec![" ".repeat(usize::from(width)); usize::from(height)];
    let title = theme::truncate(&form.peer_title, inner.saturating_sub(4));
    let blank = " ".repeat(inner);
    let to_title = theme::truncate(&form.peer_title, inner.saturating_sub(4));
    let to_pad = inner.saturating_sub(4 + theme::visible_width(&to_title));
    let buffer = theme::span(form.buffer.clone(), Tone::Dim);
    let buffer_pad = inner.saturating_sub(1 + theme::visible_width(&buffer));
    let hints = crate::tui::nav::ask_hints();
    let footer_label = theme::span(" ⌨ ".to_string(), Tone::Muted);
    let footer_hints = theme::span(hints.to_string(), Tone::Dim);
    let footer_pad =
        inner.saturating_sub(1 + theme::visible_width(&footer_label) + theme::visible_width(&footer_hints));

    let box_rows = vec![
        top_box_border(box_width, &format!(" Ask {title} ")),
        format!("│{blank}│"),
        format!(
            "│ {}{}{}{}│",
            theme::span("to ".to_string(), Tone::Muted),
            theme::span(to_title, Tone::Accent),
            " ".repeat(to_pad),
            ""
        ),
        format!("│{blank}│"),
        format!("│ {}{}{}│", buffer, " ".repeat(buffer_pad), ""),
        format!(
            "│ {}{}{}│",
            footer_label,
            footer_hints,
            " ".repeat(footer_pad)
        ),
        bottom_box_border(box_width),
    ];

    for (offset, line) in box_rows.into_iter().enumerate() {
        if offset >= usize::from(box_height) {
            break;
        }
        let row = y + offset;
        if row >= canvas.len() {
            break;
        }
        canvas[row] = splice(&canvas[row], &line, x);
    }
    canvas
}

fn top_box_border(width: u16, title: &str) -> String {
    let inner = usize::from(width).saturating_sub(2);
    let title_width = theme::visible_width(title);
    let dashes = inner.saturating_sub(title_width);
    let left = dashes / 2;
    let right = dashes - left;
    format!("╭{}{}{}╮", "─".repeat(left), title, "─".repeat(right))
}

fn bottom_box_border(width: u16) -> String {
    format!("╰{}╯", "─".repeat(usize::from(width).saturating_sub(2)))
}

fn splice(canvas: &str, line: &str, column: usize) -> String {
    let prefix = truncate_to_width(canvas, column, Ellipsis::Omit);
    let mut out = prefix.clone();
    out.push_str(line);
    let used = theme::visible_width(&prefix) + theme::visible_width(line);
    let total = theme::visible_width(canvas);
    if used < total {
        out.push_str(&" ".repeat(total - used));
    }
    truncate_to_width(&out, total, Ellipsis::Omit)
}

/// Inspector body: identity, task, usage, lineage.
pub fn inspector(peer: &Peer) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "{}{} {}",
        theme::span(
            format!("{} ", row::status_glyph(peer.status)),
            status_tone(peer.status),
        ),
        theme::span(peer.session_id.clone(), Tone::Accent),
        theme::span(format!("· {}", peer.status.label()), Tone::Dim),
    ));
    lines.push(kv("model", &peer.model));
    lines.push(kv("tool", &peer.tool));
    lines.push(kv("age", &age_label(peer.age_ms)));
    lines.push(String::new());
    lines.push(section("Task"));
    if peer.task.is_empty() {
        lines.push(theme::span("—".to_string(), Tone::Muted));
    } else {
        lines.push(theme::span(peer.task.clone(), Tone::Dim));
    }
    lines.push(String::new());
    lines.push(section("Usage"));
    lines.push(theme::span(
        row::compact_metrics(peer, &age_label(peer.age_ms)),
        Tone::Dim,
    ));
    lines.push(theme::span(
        row::context_gauge(peer.tokens, 200_000),
        Tone::Muted,
    ));
    lines.push(String::new());
    lines.push(section("Lineage"));
    lines.push(kv("Parent", peer.parent.as_deref().unwrap_or("—")));
    lines.push(kv(
        "Registered",
        peer.registered_at.get(..19).unwrap_or(&peer.registered_at),
    ));
    lines.push(kv("cwd", &peer.cwd));
    lines
}

fn status_tone(status: super::roster::Status) -> Tone {
    match status {
        super::roster::Status::Active => Tone::Success,
        super::roster::Status::Busy => Tone::Accent,
        super::roster::Status::Idle => Tone::Muted,
        super::roster::Status::Error => Tone::Error,
    }
}

fn kv(label: &str, value: &str) -> String {
    format!(
        "{}{}",
        theme::span(format!("{label:<11}"), Tone::Muted),
        theme::span(value.to_string(), Tone::Dim)
    )
}

fn section(title: &str) -> String {
    theme::span(title.to_string(), Tone::Accent)
}

fn wrap_rows(lines: Vec<String>, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        for wrapped in wrap_text_with_ansi(&line, width.max(1)) {
            out.push(wrapped);
        }
    }
    out
}
