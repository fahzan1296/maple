use maple_tui::Key;


use crate::session::store::{new_entry_id, now_rfc3339, SessionFile, Store};
use crate::session::types::SessionEntry;
use crate::tui::hub::row::short_id;
use crate::tui::hub::render::filtered;
use crate::tui::hub::roster::{self, Activity, Peer};
use crate::tui::nav::Tab;
use crate::tui::picker::Picker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Input => "input",
        }
    }
}

/// Ask form: compose a message into another peer's transcript.
pub struct AskForm {
    pub peer_id: String,
    pub peer_title: String,
    pub buffer: String,
}

/// Agent-hub overlay state (roster, activity feed, selection, filter, tree, ask).
pub struct HubState {
    pub visible: bool,
    pub tab: Tab,
    pub selected: usize,
    pub activity_selected: usize,
    pub filter: String,
    pub filter_editing: bool,
    pub tree: bool,
    pub roster: Vec<Peer>,
    pub activity: Vec<Activity>,
    pub ask: Option<AskForm>,
}

impl HubState {
    fn new() -> Self {
        Self {
            visible: false,
            tab: Tab::Agents,
            selected: 0,
            activity_selected: 0,
            filter: String::new(),
            filter_editing: false,
            tree: false,
            roster: Vec::new(),
            activity: Vec::new(),
            ask: None,
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
        let roster = roster::scan(&self.store);
        self.hub.roster = roster.peers;
        self.hub.activity = roster.activity;
        let len = self.hub.roster.len();
        self.hub.selected = self.hub.selected.min(len.saturating_sub(1));
        let activity = self.hub.activity.len();
        self.hub.activity_selected = self.hub.activity_selected.min(activity.saturating_sub(1));
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

    pub fn handle_key(&mut self, key: Key) {
        if matches!(key, Key::Ctrl('c')) {
            self.should_quit = true;
            return;
        }
        if self.picker.visible {
            if let Some(path) = self.picker.handle_key(key) {
                self.resume(path);
            }
            return;
        }
        if self.hub.ask.is_some() {
            self.handle_ask_key(key);
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
            Mode::Input => match key {
                Key::Esc => self.mode = Mode::Normal,
                Key::Enter => {
                    if self.input.starts_with('/') {
                        self.run_command();
                    } else {
                        self.send();
                    }
                    self.mode = Mode::Normal;
                }
                Key::Backspace => {
                    self.input.pop();
                }
                Key::Char(c) => self.input.push(c),
                _ => {}
            },
            Mode::Normal => match key {
                Key::Char('q') => self.should_quit = true,
                Key::Char('i') => self.mode = Mode::Input,
                Key::Char('n') => self.new_session(),
                Key::Char('f') => self.fork_session(),
                Key::Char('h') => self.toggle_hub(),
                Key::Char('j') => self.scroll = self.scroll.saturating_add(1),
                Key::Char('k') => self.scroll = self.scroll.saturating_sub(1),
                _ => {}
            },
        }
    }

    /// Bracketed paste: treat as literal text in the focused editor.
    pub fn handle_paste(&mut self, text: &str) {
        if self.picker.visible {
            self.picker.query_push_str(text);
        } else if let Some(form) = self.hub.ask.as_mut() {
            form.buffer.push_str(text);
        } else if self.hub.filter_editing {
            self.hub.filter.push_str(text);
            self.hub.selected = 0;
            self.hub.activity_selected = 0;
        } else if self.mode == Mode::Input {
            self.input.push_str(text);
        }
    }

    pub fn toggle_hub(&mut self) {
        self.hub.visible = !self.hub.visible;
        if self.hub.visible {
            self.refresh_roster();
        }
    }

    fn handle_hub_key(&mut self, key: Key) {
        match key {
            Key::Esc => self.hub.visible = false,
            Key::Char('q') => self.should_quit = true,
            Key::Tab => {
                self.hub.tab = self.hub.tab.other();
                self.refresh_roster();
            }
            Key::Char('j') | Key::Down => self.move_selection(1),
            Key::Char('k') | Key::Up => self.move_selection(-1),
            Key::Char('t') => self.hub.tree = !self.hub.tree,
            Key::Char('a') => self.open_ask(),
            Key::Char('/') => {
                self.hub.filter_editing = true;
                self.hub.filter.clear();
            }
            Key::Enter => {
                if self.hub.filter_editing {
                    self.hub.filter_editing = false;
                } else if self.hub.tab == Tab::Activity {
                    self.open_activity_session();
                }
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: i64) {
        if self.hub.tab == Tab::Activity {
            let len = self.filtered_activity().len();
            let next = self.hub.activity_selected as i64 + delta;
            self.hub.activity_selected = next.clamp(0, len.saturating_sub(1) as i64) as usize;
            return;
        }
        let len = filtered(&self.hub.roster, &self.hub.filter).len();
        let next = self.hub.selected as i64 + delta;
        self.hub.selected = next.clamp(0, len.saturating_sub(1) as i64) as usize;
        self.refresh_roster();
    }

    /// Activity rows visible under the current filter, newest first.
    pub fn filtered_activity(&self) -> Vec<&Activity> {
        let needle = self.hub.filter.trim().to_ascii_lowercase();
        self.hub
            .activity
            .iter()
            .filter(|item| {
                needle.is_empty()
                    || format!("{} {} {}", item.title, item.session_id, item.text)
                        .to_ascii_lowercase()
                        .contains(&needle)
            })
            .collect()
    }

    /// Open the ask form addressed to the selected peer.
    fn open_ask(&mut self) {
        let Some(peer) = filtered(&self.hub.roster, &self.hub.filter)
            .get(self.hub.selected)
            .map(|peer| (*peer).clone())
        else {
            return;
        };
        let title = peer
            .title
            .clone()
            .unwrap_or_else(|| short_id(&peer.session_id).to_string());
        self.hub.ask = Some(AskForm {
            peer_id: peer.session_id,
            peer_title: title,
            buffer: String::new(),
        });
    }

    fn handle_ask_key(&mut self, key: Key) {
        let Some(form) = self.hub.ask.as_mut() else {
            return;
        };
        match key {
            Key::Esc => self.hub.ask = None,
            Key::Enter => {
                let text = form.buffer.trim().to_string();
                if text.is_empty() {
                    return;
                }
                let peer_id = form.peer_id.clone();
                self.deliver_ask(&peer_id, text);
                self.hub.ask = None;
            }
            Key::Backspace => {
                form.buffer.pop();
            }
            Key::Char(c) => form.buffer.push(c),
            _ => {}
        }
    }

    /// Append the message to the peer's transcript and refresh the feed.
    fn deliver_ask(&mut self, peer_id: &str, text: String) {
        let Some(peer) = self.hub.roster.iter().find(|peer| peer.session_id == peer_id) else {
            self.status = "ask error: peer vanished".to_string();
            return;
        };
        let path = peer.path.clone();
        let from = self.file.header.id.clone();
        let entry = SessionEntry::new(
            "message",
            new_entry_id(),
            None,
            now_rfc3339(),
            serde_json::json!({ "text": text, "from": from }),
        );
        match self.store.append(&path, &entry) {
            Ok(()) => {
                self.refresh_roster();
                self.status = format!("asked {}", short_id(peer_id));
            }
            Err(err) => self.status = format!("ask error: {err}"),
        }
    }

    /// Jump to the session that produced the selected activity row.
    fn open_activity_session(&mut self) {
        let Some(item) = self
            .filtered_activity()
            .get(self.hub.activity_selected)
            .map(|item| (*item).clone())
        else {
            return;
        };
        let path = item.path.clone();
        self.hub.visible = false;
        self.hub.filter.clear();
        self.resume(path);
    }

    fn handle_filter_key(&mut self, key: Key) {
        match key {
            Key::Esc => {
                self.hub.filter_editing = false;
                self.hub.filter.clear();
            }
            Key::Enter => self.hub.filter_editing = false,
            Key::Backspace => {
                self.hub.filter.pop();
                self.hub.selected = 0;
                self.hub.activity_selected = 0;
            }
            Key::Char(c) => {
                self.hub.filter.push(c);
                self.hub.selected = 0;
                self.hub.activity_selected = 0;
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
