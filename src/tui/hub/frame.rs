//! Hub frame chrome: split top border, divider, bottom border, chip strip.

use crate::tui::nav;
use crate::tui::theme::{self, Tone};
use maple_tui::style::Style;

use super::row::{short_id, SELECTED_BG};
use super::roster::Peer;

pub const TITLE: &str = "Agent Hub";
pub const SIDEBAR_MIN: u16 = 34;
pub const SIDEBAR_MAX: u16 = 48;

pub struct HubFrame {
    title: String,
    sidebar_min: u16,
    sidebar_max: u16,
}

impl HubFrame {
    pub fn new(title: &str, sidebar_min: u16, sidebar_max: u16) -> Self {
        Self {
            title: title.to_string(),
            sidebar_min,
            sidebar_max,
        }
    }

    pub fn sidebar_width(&self, peers: &[Peer]) -> u16 {
        let longest = peers
            .iter()
            .map(|peer| {
                let label = peer
                    .title
                    .clone()
                    .or_else(|| Some(peer.cwd.clone()))
                    .unwrap_or_default();
                theme::visible_width(&format!("{} {}", short_id(&peer.session_id), label)) + 10
            })
            .max()
            .unwrap_or(0) as u16;
        longest.clamp(self.sidebar_min, self.sidebar_max)
    }

    /// `╭─ Agent Hub ──────────────┬──────────────╮`
    pub fn top(&self, width: u16, sidebar: u16) -> String {
        border(width, sidebar, '╭', '┬', '╮', Some(&self.title))
    }

    /// `├──────────────────────────┴──────────────┤`
    pub fn divider(&self, width: u16, sidebar: u16) -> String {
        border(width, sidebar, '├', '┴', '┤', None)
    }

    /// `╰─────────────────────────────────────────╯`
    pub fn bottom(&self, width: u16, sidebar: u16) -> String {
        let _ = sidebar;
        border(width, 0, '╰', '─', '╯', None)
    }

    /// `  [ Agents ]  Activity`
    pub fn chips(&self, width: u16, labels: &[&str], selected: usize) -> String {
        let mut out = String::from("  ");
        let mut used = 2usize;
        for (index, label) in labels.iter().enumerate() {
            let text = nav::chip(label, index == selected);
            let text_width = theme::visible_width(&text);
            if used + text_width > usize::from(width) {
                break;
            }
            if index == selected {
                out.push_str(
                    &Style {
                        fg: Some(Tone::Accent.color()),
                        bg: Some(SELECTED_BG),
                        ..Style::default()
                    }
                    .paint(&text),
                );
            } else {
                out.push_str(&theme::span(text, Tone::Muted));
            }
            used += text_width;
            out.push_str("  ");
            used += 2;
        }
        out
    }
}

/// Compose one border row from character cells, styling the optional title.
fn border(
    width: u16,
    sidebar: u16,
    left: char,
    mid: char,
    right: char,
    title: Option<&str>,
) -> String {
    let width = usize::from(width.max(4));
    let mid_at = (usize::from(sidebar) + 1).min(width - 2);
    let mut cells: Vec<char> = vec!['─'; width];
    cells[0] = left;
    cells[mid_at] = mid;
    cells[width - 1] = right;

    let border_style = Tone::Border.style();
    let mut out = String::new();
    let mut cursor = 0usize;
    if let Some(title) = title {
        let start = 3usize;
        let end = (start + title.chars().count()).min(mid_at.saturating_sub(1));
        if end > start {
            cells[start - 1] = ' ';
            if end < width {
                cells[end] = ' ';
            }
            let title: String = title.chars().take(end - start).collect();
            out.push_str(&border_style.paint(&cells[..start].iter().collect::<String>()));
            out.push_str(&theme::span(title, Tone::Accent));
            cursor = end;
        }
    }
    if cursor < width {
        out.push_str(&border_style.paint(&cells[cursor..].iter().collect::<String>()));
    }
    out
}
