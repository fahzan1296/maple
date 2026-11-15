//! Session picker overlay: filterable list of transcripts on disk.

use std::path::PathBuf;

use maple_tui::style::Style;
use maple_tui::text_shape::{truncate_to_width, Ellipsis};
use maple_tui::Key;

use crate::session::store::Store;
use crate::tui::nav;
use crate::tui::theme::{self, Tone};

use super::hub::row::{short_id, SELECTED_BG};

pub struct PickerItem {
    pub title: String,
    pub session_id: String,
    pub cwd: String,
    pub timestamp: String,
    pub path: PathBuf,
}

pub struct Picker {
    pub visible: bool,
    items: Vec<PickerItem>,
    query: String,
    selected: usize,
}

impl Picker {
    pub fn new() -> Self {
        Self {
            visible: false,
            items: Vec::new(),

            query: String::new(),
            selected: 0,
        }
    }

    /// Load every transcript for this directory, newest first.
    pub fn open(&mut self, store: &Store) {
        self.items = store
            .list_files()
            .unwrap_or_default()
            .into_iter()
            .map(|file| PickerItem {
                title: file
                    .header
                    .title
                    .clone()
                    .unwrap_or_else(|| short_id(&file.header.id).to_string()),
                session_id: file.header.id.clone(),
                cwd: file.header.cwd.clone(),
                timestamp: file.header.timestamp.clone(),
                path: file.path,
            })
            .collect();
        self.visible = true;
        self.query.clear();
        self.selected = 0;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    pub fn query_push_str(&mut self, text: &str) {
        self.query.push_str(text);
        self.selected = 0;
    }

    pub fn filtered(&self) -> Vec<&PickerItem> {
        let needle = self.query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return self.items.iter().collect();
        }
        self.items
            .iter()
            .filter(|item| {
                format!("{} {} {}", item.title, item.session_id, item.cwd)
                    .to_ascii_lowercase()
                    .contains(&needle)
            })
            .collect()
    }

    /// Handle a key; returns the chosen session path on Enter.
    pub fn handle_key(&mut self, key: Key) -> Option<PathBuf> {
        match key {
            Key::Esc => self.close(),
            Key::Enter => {
                let path = self
                    .filtered()
                    .get(self.selected)
                    .map(|item| item.path.clone());
                if path.is_some() {
                    self.close();
                }
                return path;
            }
            Key::Backspace => {
                self.query.pop();
                self.selected = 0;
            }
            Key::Up | Key::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            Key::Down | Key::Char('j') => {
                let len = self.filtered().len();
                if len > 0 {
                    self.selected = (self.selected + 1).min(len - 1);
                }
            }
            Key::Char(c) => {
                self.query.push(c);
                self.selected = 0;
            }
            _ => {}
        }
        None
    }

    /// Render the centered resume-session dialog into string rows.
    pub fn rows(&self, width: u16, height: u16) -> Vec<String> {
        let box_width = width * 3 / 5;
        let box_height = height * 3 / 4;
        let x = usize::from(width.saturating_sub(box_width) / 2);
        let y = usize::from(height.saturating_sub(box_height) / 2);
        let inner = usize::from(box_width).saturating_sub(2);

        let mut canvas = vec![" ".repeat(usize::from(width)); usize::from(height)];
        let items = self.filtered();
        let list_height = usize::from(box_height).saturating_sub(2);
        let scroll = self
            .selected
            .saturating_sub(list_height.saturating_sub(1));
        let selected_path = items
            .get(self.selected)
            .map(|item| item.path.clone())
            .unwrap_or_default();

        let mut rows = vec![top_border(box_width)];
        rows.push(format!("│{}│", " ".repeat(inner)));
        for item in items.iter().skip(scroll).take(list_height) {
            let is_selected = item.path == selected_path;
            let text = format!(
                "{} {}  {}  {}",
                if is_selected { super::glyph::CURSOR } else { " " },
                theme::truncate(&item.title, inner.saturating_sub(28)),
                theme::truncate(&item.cwd, 18),
                item.timestamp.get(..16).unwrap_or(""),
            );
            let fitted = theme::fit(&text, inner);
            if is_selected {
                rows.push(format!(
                    "│{}│",
                    Style {
                        fg: Some(Tone::Accent.color()),
                        bg: Some(SELECTED_BG),
                        ..Style::default()
                    }
                    .paint(&fitted)
                ));
            } else {
                rows.push(format!("│{fitted}│"));
            }
        }
        while rows.len() < box_height as usize - 1 {
            rows.push(format!("│{}│", " ".repeat(inner)));
        }
        let footer = format!(
            "{}{}  ·  {}",
            theme::span(format!(" /{}", self.query), Tone::Accent),
            " ".repeat(inner.saturating_sub(
                2 + self.query.chars().count() + 6 + nav::picker_hints().chars().count()
            )),
            theme::span(nav::picker_hints().to_string(), Tone::Dim),
        );
        rows.push(format!("│{}│", theme::fit(&footer, inner)));
        rows.push(bottom_border(box_width));

        for (offset, line) in rows.into_iter().enumerate() {
            let row = y + offset;
            if row >= canvas.len() {
                break;
            }
            let prefix = truncate_to_width(&canvas[row], x, Ellipsis::Omit);
            let mut joined = prefix.clone();
            joined.push_str(&line);
            let used = theme::visible_width(&prefix) + theme::visible_width(&line);
            let total = theme::visible_width(&canvas[row]);
            if used < total {
                joined.push_str(&" ".repeat(total - used));
            }
            canvas[row] = truncate_to_width(&joined, total, Ellipsis::Omit);
        }
        canvas
    }
}

fn top_border(width: u16) -> String {
    format!("╭─ Resume session {}╮", "─".repeat(usize::from(width).saturating_sub(19)))
}

fn bottom_border(width: u16) -> String {
    format!("╰{}╯", "─".repeat(usize::from(width).saturating_sub(2)))
}
