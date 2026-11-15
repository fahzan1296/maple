//! Session picker overlay: filterable list of transcripts on disk.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

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
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<PathBuf> {
        match key.code {
            KeyCode::Esc => self.close(),
            KeyCode::Enter => {
                let path = self
                    .filtered()
                    .get(self.selected)
                    .map(|item| item.path.clone());
                if path.is_some() {
                    self.close();
                }
                return path;
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.filtered().len();
                if len > 0 {
                    self.selected = (self.selected + 1).min(len - 1);
                }
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.selected = 0;
            }
            _ => {}
        }
        None
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let width = area.width * 3 / 5;
        let height = area.height * 3 / 4;
        let rect = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Resume session ")
            .border_style(Style::default().fg(Tone::Border.color()));
        let inner = block.inner(rect);
        f.render_widget(block, rect);

        let list_height = usize::from(inner.height).saturating_sub(1);
        let items = self.filtered();
        let scroll = self.selected.saturating_sub(list_height.saturating_sub(1));
        let list_area = Rect::new(inner.x, inner.y, inner.width, list_height as u16);
        let selected_path = items.get(self.selected).map(|item| item.path.clone()).unwrap_or_default();
        let mut lines: Vec<Line<'static>> = Vec::new();
        for item in items.iter().skip(scroll).take(list_height) {
            let is_selected = item.path == selected_path;
            let text = format!(
                "{} {}  {}  {}",
                if is_selected { super::glyph::CURSOR } else { " " },
                theme::truncate(&item.title, usize::from(inner.width).saturating_sub(28)),
                theme::truncate(&item.cwd, 18),
                item.timestamp.get(..16).unwrap_or(""),
            );
            if is_selected {
                lines.push(Line::styled(
                    theme::fit(&text, usize::from(inner.width)),
                    Style::default().bg(SELECTED_BG).fg(Tone::Accent.color()),
                ));
            } else {
                lines.push(Line::from(text));
            }
        }
        f.render_widget(Paragraph::new(lines), list_area);

        let footer = Line::from(vec![
            theme::span(format!(" /{}", self.query), Tone::Accent),
            theme::span("  ·  ".to_string(), Tone::Muted),
            theme::span(nav::picker_hints().to_string(), Tone::Dim),
        ]);
        let footer_area = Rect::new(
            inner.x,
            inner.y + list_height as u16,
            inner.width,
            1,
        );
        f.render_widget(Paragraph::new(footer), footer_area);
    }
}

