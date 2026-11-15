use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::session::store::{new_entry_id, now_rfc3339, SessionFile, Store};
use crate::session::types::SessionEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input,
}

pub struct App {
    store: Store,
    pub file: SessionFile,
    pub entries: Vec<SessionEntry>,
    pub input: String,
    pub mode: Mode,
    pub status: String,
    pub should_quit: bool,
    pub model: String,
    pub scroll: u16,
}

impl App {
    pub fn new(store: Store, file: SessionFile, model: String) -> Self {
        let mut app = Self {
            store,
            file,
            entries: Vec::new(),
            input: String::new(),
            mode: Mode::Normal,
            status: String::new(),
            should_quit: false,
            model,
            scroll: 0,
        };
        app.reload();
        app
    }

    pub fn reload(&mut self) {
        match self.store.load(&self.file.path) {
            Ok((_header, entries)) => {
                self.entries = entries;
                self.status = format!("{} entries", self.entries.len());
            }
            Err(err) => self.status = format!("load error: {err}"),
        }
    }

    fn send(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let entry = SessionEntry::new(
            "message",
            new_entry_id(),
            None,
            now_rfc3339(),
            serde_json::json!({ "text": text }),
        );
        match self.store.append(&self.file.path, &entry) {
            Ok(()) => {
                self.input.clear();
                self.reload();
            }
            Err(err) => self.status = format!("append error: {err}"),
        }
    }

    fn new_session(&mut self) {
        match self.store.create(None) {
            Ok(file) => {
                self.file = file;
                self.scroll = 0;
                self.reload();
            }
            Err(err) => self.status = format!("create error: {err}"),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match self.mode {
            Mode::Input => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    self.send();
                    self.mode = Mode::Normal;
                }
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Char(c) => self.input.push(c),
                _ => {}
            },
            Mode::Normal => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Char('i') => self.mode = Mode::Input,
                KeyCode::Char('n') => self.new_session(),
                KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
                KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
                _ => {}
            },
        }
    }

    pub fn session_id(&self) -> &str {
        &self.file.header.id
    }

    pub fn cwd(&self) -> &str {
        &self.file.header.cwd
    }
}
