//! Horizontal tab bar with keyboard and pointer navigation.
//!
//! Ports `l/src/components/tab-bar.ts`: a label prefix, ` id ` -wrapped tab
//! buttons, a cycle hint, collapse to `short` labels when the bar overflows,
//! muted tabs that navigation skips, and per-render hit zones for pointer
//! clicks.
//!
//! Divergences from the TypeScript original:
//! - `Ctrl+Left`/`Ctrl+Right` also move between tabs (the crate's global
//!   bindings use them for the same gesture).
//! - Theme entries are [`Style`] values rather than closures.

use crate::component::Component;
use crate::keys::Key;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

/// One tab button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    /// Unique identifier.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Compact label used when the bar must shrink.
    pub short: Option<String>,
    /// Muted tabs are skipped by navigation and never highlighted.
    pub muted: bool,
}

impl Tab {
    /// Build an unmuted tab.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Tab {
            id: id.into(),
            label: label.into(),
            short: None,
            muted: false,
        }
    }

    /// Set the compact label.
    pub fn with_short(mut self, short: impl Into<String>) -> Self {
        self.short = Some(short.into());
        self
    }

    /// Mark the tab muted.
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }
}

/// Theme for [`TabBar`].
#[derive(Clone)]
pub struct TabBarTheme {
    /// Bar label prefix.
    pub label: Style,
    /// Active tab.
    pub active_tab: Style,
    /// Inactive tab.
    pub inactive_tab: Style,
    /// Cycle hint.
    pub hint: Style,
    /// Muted tab; defaults to `inactive_tab` when unset.
    pub muted_tab: Option<Style>,
    /// Hovered inactive tab; defaults to `inactive_tab` when unset.
    pub hover_tab: Option<Style>,
    /// Glyph set; retained for hosts that draw their own chrome.
    pub symbols: SymbolTheme,
}

impl Default for TabBarTheme {
    fn default() -> Self {
        TabBarTheme {
            label: Style::fg(Color::White).bold(),
            active_tab: Style::fg(Color::White).bold(),
            inactive_tab: Style::fg(Color::DarkGray),
            hint: Style::fg(Color::DarkGray).dim(),
            muted_tab: None,
            hover_tab: None,
            symbols: SymbolTheme::unicode(),
        }
    }
}

struct Chunk {
    text: String,
    tab_index: Option<usize>,
}

/// A horizontal tab bar.
pub struct TabBar {
    label: String,
    tabs: Vec<Tab>,
    active_index: usize,
    theme: TabBarTheme,
    show_hint: bool,
    hover_id: Option<String>,
    hit_zones: Vec<(usize, usize, usize, usize)>,
    on_select: Option<Box<dyn FnMut(usize, &Tab)>>,
}

impl Default for TabBar {
    fn default() -> Self {
        TabBar::new(String::new(), Vec::new(), TabBarTheme::default())
    }
}

impl TabBar {
    /// Build a tab bar with `label`, `tabs`, and `theme`.
    pub fn new(label: impl Into<String>, tabs: Vec<Tab>, theme: TabBarTheme) -> Self {
        TabBar {
            label: label.into(),
            tabs,
            active_index: 0,
            theme,
            show_hint: true,
            hover_id: None,
            hit_zones: Vec::new(),
            on_select: None,
        }
    }

    /// Bar label prefix.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Set the bar label prefix.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    /// Every tab in render order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Replace the tab set, preserving the active id when it survives.
    pub fn set_tabs(&mut self, tabs: Vec<Tab>) {
        let target = self
            .tabs
            .get(self.active_index)
            .map(|tab| tab.id.clone())
            .or_else(|| tabs.first().map(|tab| tab.id.clone()));
        self.tabs = tabs;
        self.active_index = match &target {
            Some(id) => self
                .tabs
                .iter()
                .position(|tab| &tab.id == id)
                .unwrap_or(0),
            None => 0,
        };
    }

    /// Replace the tab set and activate `active_id` without firing a callback.
    pub fn set_tabs_with_active(&mut self, tabs: Vec<Tab>, active_id: Option<&str>) {
        self.tabs = tabs;
        self.active_index = match active_id {
            Some(id) => self
                .tabs
                .iter()
                .position(|tab| tab.id == id)
                .unwrap_or(0),
            None => 0,
        };
    }

    /// Index of the active tab.
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// The active tab.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_index)
    }

    /// Activate the tab at `index` (clamped), firing `on_select` on change.
    pub fn set_selected(&mut self, index: usize) {
        if self.tabs.is_empty() {
            self.active_index = 0;
            return;
        }
        let target = index.min(self.tabs.len() - 1);
        if target == self.active_index {
            return;
        }
        self.active_index = target;
        self.notify();
    }

    /// Activate the tab with `id` without firing a callback.
    pub fn set_selected_by_id(&mut self, id: &str) -> bool {
        match self.tabs.iter().position(|tab| tab.id == id) {
            Some(index) => {
                self.active_index = index;
                true
            }
            None => false,
        }
    }

    /// Activate the tab with `id`, firing `on_select`; muted tabs are ignored.
    pub fn select_tab(&mut self, id: &str) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if self.tabs[index].muted {
            return false;
        }
        self.set_selected(index);
        true
    }

    /// Whether the trailing cycle hint renders.
    pub fn show_hint(&self) -> bool {
        self.show_hint
    }

    /// Show or hide the trailing cycle hint.
    pub fn set_show_hint(&mut self, show: bool) {
        self.show_hint = show;
    }

    /// Highlight the tab under the pointer (`None` clears).
    pub fn set_hovered_id(&mut self, id: Option<String>) {
        self.hover_id = id;
    }

    /// Fire [`Self::on_select`] when the active tab changes.
    pub fn on_select(&mut self, callback: impl FnMut(usize, &Tab) + 'static) -> &mut Self {
        self.on_select = Some(Box::new(callback));
        self
    }

    /// Move to the next non-muted tab (wrapping).
    pub fn next_tab(&mut self) {
        self.step_tab(1);
    }

    /// Move to the previous non-muted tab (wrapping).
    pub fn previous_tab(&mut self) {
        self.step_tab(-1);
    }

    fn step_tab(&mut self, delta: isize) {
        let length = self.tabs.len();
        if length == 0 {
            return;
        }
        for step in 1..=length {
            let index = (self.active_index as isize + delta * step as isize)
                .rem_euclid(length as isize) as usize;
            if !self.tabs[index].muted {
                self.set_selected(index);
                return;
            }
        }
    }

    fn notify(&mut self) {
        if let Some(callback) = self.on_select.as_mut() {
            if let Some(tab) = self.tabs.get(self.active_index) {
                let index = self.active_index;
                callback(index, tab);
            }
        }
    }

    /// Resolve a pointer position against the last rendered frame.
    pub fn tab_at(&self, line: usize, column: usize) -> Option<&Tab> {
        let (_, _, _, index) = self
            .hit_zones
            .iter()
            .find(|(zone_line, start, end, _)| {
                *zone_line == line && column >= *start && column < *end
            })?;
        self.tabs.get(*index)
    }

    fn build_chunks(&self, labels: &[String]) -> Vec<Chunk> {
        let mut chunks: Vec<Chunk> = Vec::new();
        if !self.label.is_empty() {
            chunks.push(Chunk {
                text: self.theme.label.paint(&format!("{}:", self.label)),
                tab_index: None,
            });
            chunks.push(Chunk {
                text: "  ".to_string(),
                tab_index: None,
            });
        }
        for (index, tab) in self.tabs.iter().enumerate() {
            let hovered = self.hover_id.as_deref() == Some(tab.id.as_str())
                && !tab.muted
                && index != self.active_index;
            let style = if tab.muted {
                self.theme
                    .muted_tab
                    .unwrap_or(self.theme.inactive_tab)
            } else if index == self.active_index {
                self.theme.active_tab
            } else if hovered {
                self.theme
                    .hover_tab
                    .unwrap_or(self.theme.inactive_tab)
            } else {
                self.theme.inactive_tab
            };
            chunks.push(Chunk {
                text: style.paint(&format!(" {} ", labels[index])),
                tab_index: Some(index),
            });
            if index + 1 < self.tabs.len() {
                chunks.push(Chunk {
                    text: "  ".to_string(),
                    tab_index: None,
                });
            }
        }
        if self.show_hint {
            chunks.push(Chunk {
                text: "  ".to_string(),
                tab_index: None,
            });
            chunks.push(Chunk {
                text: self.theme.hint.paint("(tab to cycle)"),
                tab_index: None,
            });
        }
        chunks
    }

    fn total_width(chunks: &[Chunk]) -> usize {
        chunks
            .iter()
            .map(|chunk| visible_width(&chunk.text))
            .sum()
    }
}

impl Component for TabBar {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return vec![String::new()];
        }
        let max_width = width as usize;
        let mut labels: Vec<String> = self.tabs.iter().map(|tab| tab.label.clone()).collect();
        let mut chunks = self.build_chunks(&labels);

        if Self::total_width(&chunks) > max_width {
            let mut collapse_order: Vec<usize> = self
                .tabs
                .iter()
                .enumerate()
                .filter(|(index, tab)| *index != self.active_index && tab.short.is_some())
                .map(|(index, _)| index)
                .collect();
            collapse_order.sort_by_key(|index| {
                std::cmp::Reverse((*index as isize - self.active_index as isize).abs())
            });
            for index in collapse_order {
                if let Some(short) = self.tabs[index].short.clone() {
                    labels[index] = short;
                }
                chunks = self.build_chunks(&labels);
                if Self::total_width(&chunks) <= max_width {
                    break;
                }
            }
        }

        self.hit_zones.clear();
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut current_width = 0usize;
        for chunk in chunks {
            let chunk_width = visible_width(&chunk.text);
            if chunk_width == 0 {
                continue;
            }
            if chunk_width > max_width {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                if let Some(index) = chunk.tab_index {
                    self.hit_zones.push((lines.len(), 0, max_width, index));
                }
                lines.push(truncate_to_width(&chunk.text, max_width, Ellipsis::Omit));
                continue;
            }
            if current_width > 0 && current_width + chunk_width > max_width {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            if let Some(index) = chunk.tab_index {
                self.hit_zones.push((lines.len(), current_width, current_width + chunk_width, index));
            }
            current.push_str(&chunk.text);
            current_width += chunk_width;
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }

    fn handle_input(&mut self, key: &Key) {
        if crate::keys::is_tab(key)
            || crate::keys::is_arrow_right(key)
            || crate::keys::is_ctrl_right(key)
        {
            self.next_tab();
            return;
        }
        if crate::keys::is_shift_tab(key)
            || crate::keys::is_arrow_left(key)
            || crate::keys::is_ctrl_left(key)
        {
            self.previous_tab();
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if !matches!(event.kind, MouseKind::Down(MouseButtonKind::Left)) {
            return;
        }
        let column = event.column as usize;
        let row = event.row as usize;
        if let Some(tab) = self.tab_at(row, column) {
            let id = tab.id.clone();
            self.select_tab(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_theme() -> TabBarTheme {
        TabBarTheme {
            label: Style::default(),
            active_tab: Style::default(),
            inactive_tab: Style::default(),
            hint: Style::default(),
            muted_tab: None,
            hover_tab: None,
            symbols: SymbolTheme::unicode(),
        }
    }

    fn tabs() -> Vec<Tab> {
        vec![
            Tab::new("config", "Config"),
            Tab::new("tools", "Tools"),
            Tab::new("help", "Help"),
        ]
    }

    #[test]
    fn renders_single_tab_exactly() {
        let mut bar = TabBar::new("", vec![Tab::new("only", "Only")], plain_theme());
        bar.set_show_hint(false);
        assert_eq!(bar.render(20), vec![" Only "]);
    }

    #[test]
    fn renders_label_and_hint_on_one_line() {
        let mut bar = TabBar::new("Settings", tabs(), plain_theme());
        let lines = bar.render(60);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Settings:"));
        assert!(lines[0].contains("Config"));
        assert!(lines[0].contains("Tools"));
        assert!(lines[0].ends_with("(tab to cycle)"));
        assert_eq!(bar.render(0), vec![""]);
    }

    #[test]
    fn arrow_and_tab_navigation_wraps() {
        let mut bar = TabBar::new(String::new(), tabs(), plain_theme());
        assert_eq!(bar.active_index(), 0);
        bar.handle_input(&Key::Right);
        assert_eq!(bar.active_index(), 1);
        bar.handle_input(&Key::Tab);
        assert_eq!(bar.active_index(), 2);
        bar.handle_input(&Key::Tab);
        assert_eq!(bar.active_index(), 0);
        bar.handle_input(&Key::Left);
        assert_eq!(bar.active_index(), 2);
        bar.handle_input(&Key::CtrlRight);
        assert_eq!(bar.active_index(), 0);
        bar.handle_input(&Key::CtrlLeft);
        assert_eq!(bar.active_index(), 2);
    }

    #[test]
    fn muted_tabs_are_skipped_by_navigation() {
        let mut bar = TabBar::new(
            String::new(),
            vec![
                Tab::new("a", "A"),
                Tab::new("b", "B").muted(),
                Tab::new("c", "C"),
            ],
            plain_theme(),
        );
        bar.next_tab();
        assert_eq!(bar.active_index(), 2);
        bar.next_tab();
        assert_eq!(bar.active_index(), 0);
        assert!(!bar.select_tab("b"));
        assert_eq!(bar.active_index(), 0);
    }

    #[test]
    fn set_selected_fires_callback() {
        let selected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&selected);
        let mut bar = TabBar::new(String::new(), tabs(), plain_theme());
        bar.on_select(move |index, tab| {
            sink.lock().unwrap().push((index, tab.id.clone()));
        });
        bar.set_selected(2);
        assert_eq!(selected.lock().unwrap().to_vec(), [(2usize, "help".to_string())]);
        bar.set_selected(99);
        assert_eq!(bar.active_index(), 2);
    }

    #[test]
    fn set_selected_by_id_is_silent() {
        let selected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&selected);
        let mut bar = TabBar::new(String::new(), tabs(), plain_theme());
        bar.on_select(move |index, tab| {
            sink.lock().unwrap().push((index, tab.id.clone()));
        });
        assert!(bar.set_selected_by_id("tools"));
        assert_eq!(bar.active_index(), 1);
        assert!(selected.lock().unwrap().is_empty());
    }

    #[test]
    fn set_tabs_preserves_active_id() {
        let mut bar = TabBar::new(String::new(), tabs(), plain_theme());
        bar.set_selected(1);
        bar.set_tabs(vec![Tab::new("x", "X"), Tab::new("tools", "Tools")]);
        assert_eq!(bar.active_index(), 1);
        assert_eq!(bar.active_tab().unwrap().id, "tools");
    }

    #[test]
    fn mouse_click_activates_tab() {
        let selected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&selected);
        let mut bar = TabBar::new("Settings", tabs(), plain_theme());
        bar.set_show_hint(false);
        bar.on_select(move |index, tab| {
            sink.lock().unwrap().push((index, tab.id.clone()));
        });
        let lines = bar.render(60);
        assert_eq!(lines.len(), 1);
        // "Settings:" (9) + "  " (2) = 11, " Config " occupies 11..19.
        bar.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 22,
            row: 0,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(bar.active_index(), 1);
        assert_eq!(selected.lock().unwrap().to_vec(), [(1usize, "tools".to_string())]);
    }

    #[test]
    fn overflow_collapses_to_short_labels() {
        let mut bar = TabBar::new(
            String::new(),
            vec![
                Tab::new("a", "Alpha").with_short("A"),
                Tab::new("b", "Beta").with_short("B"),
                Tab::new("c", "Gamma"),
            ],
            plain_theme(),
        );
        bar.set_show_hint(false);
        let lines = bar.render(10);
        let joined = lines.join("");
        assert!(joined.contains("Alpha") || joined.contains("A"));
        assert!(visible_width(&lines[0]) <= 10);
        for line in &lines {
            assert!(visible_width(line) <= 10);
        }
    }

    #[test]
    fn empty_tabs_render_one_blank_line() {
        let mut bar = TabBar::new(String::new(), Vec::new(), plain_theme());
        bar.set_show_hint(false);
        assert_eq!(bar.render(20), vec![""]);
        assert_eq!(bar.active_index(), 0);
        assert!(bar.active_tab().is_none());
        bar.next_tab();
        assert_eq!(bar.active_index(), 0);
    }

    #[test]
    fn narrow_widths_never_exceed() {
        let mut bar = TabBar::new("Settings", tabs(), plain_theme());
        for width in [1u16, 2, 3, 8, 20] {
            for line in bar.render(width) {
                assert!(visible_width(&line) <= width as usize);
            }
        }
    }
}
