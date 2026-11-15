use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::session::store::{new_entry_id, now_rfc3339, SessionFile, Store};
use crate::session::types::SessionEntry;
use crate::tui::hub::roster::{self, Peer};
use crate::tui::picker::Picker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input,
}

/// Agent-hub overlay state (roster, selection, filter, tree).
pub struct HubState {
    pub visible: bool,
    pub selected: usize,
    pub filter: String,
    pub filter_editing: bool,
    pub tree: bool,
    pub roster: Vec<Peer>,
}

impl HubState {
    fn new() -> Self {
        Self {
            visible: false,
            selected: 0,
            filter: String::new(),
            filter_editing: false,
            tree: false,
            roster: Vec::new(),
        }
    }
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
    pub hub: HubState,
    pub picker: Picker,
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
            hub: HubState::new(),
            picker: Picker::new(),
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

    fn fork_session(&mut self) {
        let parent = self.file.header.id.clone();
        match self.store.branch(&parent, None) {
            Ok(file) => {
                self.file = file;
                self.scroll = 0;
                self.reload();
            }
            Err(err) => self.status = format!("fork error: {err}"),
        }
    }

    fn refresh_roster(&mut self) {
        self.hub.roster = roster::scan(&self.store);
        let len = self.hub.roster.len();
        self.hub.selected = self.hub.selected.min(len.saturating_sub(1));
        let current = self.file.header.id.clone();
        if let Some(peer) = self.hub.roster.iter_mut().find(|peer| peer.session_id == current) {
            peer.model = self.model.clone();
        }
    }
    fn resume(&mut self, path: std::path::PathBuf) {
        match self.store.load(&path) {
            Ok((header, entries)) => {
                self.file = SessionFile { header, path };
                self.entries = entries;
                self.scroll = 0;
                self.status = format!("resumed {}", &self.file.header.id[..8.min(self.file.header.id.len())]);
            }
            Err(err) => self.status = format!("resume error: {err}"),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        if self.picker.visible {
            if let Some(path) = self.picker.handle_key(key) {
                self.resume(path);
            }
            return;
        }
        if self.hub.filter_editing {
            self.handle_filter_key(key);
            return;
        }
        if self.hub.visible {
            self.handle_hub_key(key);
            return;
        }
        match self.mode {
            Mode::Input => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    if self.input.starts_with('/') {
                        self.run_command();
                    } else {
                        self.send();
                    }
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
                KeyCode::Char('f') => self.fork_session(),
                KeyCode::Char('h') => self.toggle_hub(),
                KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
                KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
                _ => {}
            },
        }
    }

    pub fn toggle_hub(&mut self) {
        self.hub.visible = !self.hub.visible;
        if self.hub.visible {
            self.refresh_roster();
        }
    }

    fn handle_hub_key(&mut self, key: KeyEvent) {
        let len = self.hub.roster.len();
        match key.code {
            KeyCode::Esc => self.hub.visible = false,
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => {
                self.hub.selected = (self.hub.selected + 1).min(len.saturating_sub(1));
                self.refresh_roster();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.hub.selected = self.hub.selected.saturating_sub(1);
                self.refresh_roster();
            }
            KeyCode::Char('t') => self.hub.tree = !self.hub.tree,
            KeyCode::Char('/') => {
                self.hub.filter_editing = true;
                self.hub.filter.clear();
            }
            KeyCode::Enter => self.hub.filter_editing = false,
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.hub.filter_editing = false;
                self.hub.filter.clear();
            }
            KeyCode::Enter => self.hub.filter_editing = false,
            KeyCode::Backspace => {
                self.hub.filter.pop();
                self.hub.selected = 0;
            }
            KeyCode::Char(c) => {
                self.hub.filter.push(c);
                self.hub.selected = 0;
            }
            _ => {}
        }
    }

    fn run_command(&mut self) {
        let command = self.input.trim().to_string();
        self.input.clear();
        match command.as_str() {
            "/resume" => self.picker.open(&self.store),
            "/new" => self.new_session(),
            "/fork" => self.fork_session(),
            "/hub" => self.toggle_hub(),
            other => self.status = format!("unknown command: {other}"),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.file.header.id
    }

    pub fn cwd(&self) -> &str {
        &self.file.header.cwd
    }
}
