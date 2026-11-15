//! Selectable list with keyboard navigation and type-to-filter search.
//!
//! Ports `l/src/components/select-list.ts`. Items carry a stable `key`, a
//! `title`, and optional `label`/`description`/`disabled`/`confirmation`
//! metadata; the selection, filtering, disabled-row skipping, and confirmation
//! state live in [`MenuState`](crate::components::menu_selection::MenuState).
//!
//! Divergences from the TypeScript original:
//! - `SymbolTheme` has no `cursor` glyph, so the selection prefix uses
//!   [`DEFAULT_CURSOR`] (`">"`) unless a host restyles `select_label`.
//! - The primary column is clamped to 32 cells (the TypeScript default) rather
//!   than being derived from a configurable min/max pair.
//! - `label` renders as a secondary tag after `title`; TypeScript has no such
//!   field.
//! - Icons, native detail text, rich row renderers, and wrapped descriptions are
//!   out of scope. Descriptions render inline and truncate with `Ellipsis::Omit`.
//! - Escape cancels immediately (clearing a pending confirmation first), as in
//!   TypeScript; it does not clear the query first.

use crate::component::Component;
use crate::keys::Key;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::components::menu_selection::{
    get_menu_window, MenuActivation, MenuSource, MenuState, SearchPolicy, DEFAULT_CURSOR,
};
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};

/// Default width of the primary (title) column.
pub const DEFAULT_PRIMARY_COLUMN_WIDTH: usize = 32;
/// Gap kept between the primary column and an inline description.
pub const PRIMARY_COLUMN_GAP: usize = 2;
/// Descriptions narrower than this are dropped instead of rendered.
pub const MIN_DESCRIPTION_WIDTH: usize = 10;

const TRACK_GLYPH: &str = "│";
const THUMB_GLYPH: &str = "█";

/// Collapse tabs, line breaks, and whitespace runs into single spaces and trim.
fn sanitize_single_line(text: &str) -> String {
    let mut expanded = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\t' => expanded.push_str("   "),
            '\r' | '\n' => expanded.push(' '),
            _ => expanded.push(character),
        }
    }
    expanded
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One selectable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectItem {
    /// Stable identity used to retain the selection across data changes.
    pub key: String,
    /// Primary display text.
    pub title: String,
    /// Optional secondary tag rendered after the title.
    pub label: Option<String>,
    /// Optional description rendered inline when the width allows.
    pub description: Option<String>,
    /// Disabled rows stay visible but cannot be selected.
    pub disabled: bool,
    /// When set, activation needs a second Enter and this text becomes the
    /// status line while the confirmation is pending.
    pub confirmation: Option<String>,
}

impl SelectItem {
    /// Build an enabled item with `key` and `title`.
    pub fn new(key: impl Into<String>, title: impl Into<String>) -> Self {
        SelectItem {
            key: key.into(),
            title: title.into(),
            label: None,
            description: None,
            disabled: false,
            confirmation: None,
        }
    }

    /// Attach a secondary tag.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attach a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Mark the row disabled.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Require a second activation and show `confirmation` while pending.
    pub fn with_confirmation(mut self, confirmation: impl Into<String>) -> Self {
        self.confirmation = Some(confirmation.into());
        self
    }

    /// Text matched by the default fuzzy filter.
    fn search_text(&self) -> String {
        let mut text = format!("{} {}", self.title, self.key);
        if let Some(label) = &self.label {
            text.push(' ');
            text.push_str(label);
        }
        if let Some(description) = &self.description {
            text.push(' ');
            text.push_str(description);
        }
        sanitize_single_line(&text)
    }
}

struct SelectItemSource;

impl MenuSource<SelectItem> for SelectItemSource {
    fn key(&self, item: &SelectItem) -> String {
        item.key.clone()
    }

    fn search_text(&self, item: &SelectItem) -> String {
        item.search_text()
    }

    fn disabled(&self, item: &SelectItem) -> bool {
        item.disabled
    }

    fn requires_confirmation(&self, item: &SelectItem) -> bool {
        item.confirmation.is_some()
    }
}

/// Theme for [`SelectList`].
#[derive(Clone)]
pub struct SelectListTheme {
    /// Descriptions, disabled rows, and the scrollbar track.
    pub muted: Style,
    /// Title text of the selected row.
    pub selected: Style,
    /// Band painted across the whole selected (or hovered) row.
    pub selected_row: Style,
    /// Border glyphs.
    pub border: Style,
    /// Selection cursor glyph and scrollbar thumb.
    pub select_label: Style,
    /// Search status line.
    pub hint: Style,
    /// Glyph set; retained for hosts that draw their own chrome.
    pub symbols: SymbolTheme,
}

impl Default for SelectListTheme {
    fn default() -> Self {
        SelectListTheme {
            muted: Style::fg(Color::DarkGray),
            selected: Style::fg(Color::White).bold(),
            selected_row: Style::default(),
            border: Style::fg(Color::DarkGray),
            select_label: Style::fg(Color::White).bold(),
            hint: Style::fg(Color::DarkGray).dim(),
            symbols: SymbolTheme::unicode(),
        }
    }
}

/// An interactive selection list.
pub struct SelectList {
    state: MenuState<SelectItem>,
    theme: SelectListTheme,
    policy: SearchPolicy,
    max_visible: usize,
    max_primary_column_width: usize,
    wrap_navigation: bool,
    empty_text: String,
    no_match_text: String,
    on_select: Option<Box<dyn FnMut(&SelectItem)>>,
    on_cancel: Option<Box<dyn FnMut()>>,
    on_selection_change: Option<Box<dyn FnMut(&SelectItem)>>,
    on_filter_changed: Option<Box<dyn FnMut(&str)>>,
    hit_rows: Vec<Option<usize>>,
    hovered_index: Option<usize>,
}

impl SelectList {
    /// Build a list over `items` with a visible budget of `max_visible` rows.
    pub fn new(items: Vec<SelectItem>, max_visible: usize, theme: SelectListTheme) -> Self {
        SelectList {
            state: MenuState::new(items, Box::new(SelectItemSource)),
            theme,
            policy: SearchPolicy::default(),
            max_visible: max_visible.max(1),
            max_primary_column_width: DEFAULT_PRIMARY_COLUMN_WIDTH,
            wrap_navigation: true,
            empty_text: "No items".to_string(),
            no_match_text: "No matching items".to_string(),
            on_select: None,
            on_cancel: None,
            on_selection_change: None,
            on_filter_changed: None,
            hit_rows: Vec::new(),
            hovered_index: None,
        }
    }

    /// Replace the items, retaining the selected key when it survives.
    pub fn set_items(&mut self, items: Vec<SelectItem>) {
        if self.state.set_items(items) {
            self.notify_selection_change();
        }
    }

    /// Every item before filtering.
    pub fn items(&self) -> &[SelectItem] {
        self.state.items()
    }

    /// Items surviving the current filter.
    pub fn visible_items(&self) -> &[SelectItem] {
        self.state.visible_items()
    }

    /// Selected item, if any.
    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.state.selected_item()
    }

    /// Stable key of the selected item, if any.
    pub fn selected_key(&self) -> Option<String> {
        self.state.selected_key()
    }

    /// Selected index into [`Self::visible_items`], or `-1`.
    pub fn selected_index(&self) -> isize {
        self.state.selected_index()
    }

    /// Select the visible row at `index`.
    pub fn set_selected_index(&mut self, index: isize) {
        if self.state.set_selected_index(index, 1) {
            self.notify_selection_change();
        }
    }

    /// Select the visible row whose key equals `key`.
    pub fn set_selected_key(&mut self, key: &str) -> bool {
        let changed = self.state.set_selected_key(key);
        if changed {
            self.notify_selection_change();
        }
        changed
    }

    /// Move the selection by `delta` rows, skipping disabled rows.
    pub fn move_selection(&mut self, delta: isize, wrap: bool) -> bool {
        let changed = self.state.move_selection(delta, wrap);
        if changed {
            self.notify_selection_change();
        }
        changed
    }

    /// Current filter text.
    pub fn filter(&self) -> &str {
        self.state.query()
    }

    /// Replace the filter text; fires `on_filter_changed` when it differs.
    pub fn set_filter(&mut self, filter: &str) {
        if filter == self.state.query() {
            return;
        }
        let selection_changed = self.state.set_query(filter, true);
        if let Some(callback) = self.on_filter_changed.as_mut() {
            callback(self.state.query());
        }
        if selection_changed {
            self.notify_selection_change();
        }
    }

    /// Clear the filter text.
    pub fn clear_filter(&mut self) {
        self.set_filter("");
    }

    /// Search policy.
    pub fn search_policy(&self) -> SearchPolicy {
        self.policy
    }

    /// Set the search policy.
    pub fn set_search_policy(&mut self, policy: SearchPolicy) {
        self.policy = policy;
    }

    /// True when the policy lets the current item count be searched.
    pub fn can_edit_search(&self) -> bool {
        self.policy.can_edit(self.state.items().len(), self.max_visible)
    }

    /// Visible row budget.
    pub fn max_visible(&self) -> usize {
        self.max_visible
    }

    /// Refit the visible row budget.
    pub fn set_max_visible(&mut self, rows: usize) {
        self.max_visible = rows.max(1);
    }

    /// Upper bound for the primary column.
    pub fn max_primary_column_width(&self) -> usize {
        self.max_primary_column_width
    }

    /// Set the upper bound for the primary column.
    pub fn set_max_primary_column_width(&mut self, width: usize) {
        self.max_primary_column_width = width.max(1);
    }

    /// Whether single-step navigation wraps at the edges.
    pub fn wrap_navigation(&self) -> bool {
        self.wrap_navigation
    }

    /// Set whether single-step navigation wraps at the edges.
    pub fn set_wrap_navigation(&mut self, wrap: bool) {
        self.wrap_navigation = wrap;
    }

    /// Message shown when there are no items at all.
    pub fn empty_text(&self) -> &str {
        &self.empty_text
    }

    /// Set the empty-list message.
    pub fn set_empty_text(&mut self, text: impl Into<String>) {
        self.empty_text = text.into();
    }

    /// Message shown when a filter matches nothing.
    pub fn no_match_text(&self) -> &str {
        &self.no_match_text
    }

    /// Set the no-match message.
    pub fn set_no_match_text(&mut self, text: impl Into<String>) {
        self.no_match_text = text.into();
    }

    /// Current theme.
    pub fn theme(&self) -> &SelectListTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: SelectListTheme) {
        self.theme = theme;
    }

    /// Highlight the row under the pointer (`None` clears).
    pub fn set_hovered_index(&mut self, index: Option<usize>) {
        self.hovered_index = index;
    }

    /// Row under the pointer, if any.
    pub fn hovered_index(&self) -> Option<usize> {
        self.hovered_index
    }

    /// Resolve a rendered line to its item.
    pub fn hit_test(&self, line: usize) -> Option<&SelectItem> {
        let index = self.hit_rows.get(line).copied().flatten()?;
        self.state.visible_items().get(index)
    }

    /// Fire [`Self::on_select`] with the currently selected item.
    pub fn on_select(&mut self, callback: impl FnMut(&SelectItem) + 'static) -> &mut Self {
        self.on_select = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_cancel`] on cancel.
    pub fn on_cancel(&mut self, callback: impl FnMut() + 'static) -> &mut Self {
        self.on_cancel = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_selection_change`] whenever the highlighted row changes.
    pub fn on_selection_change(&mut self, callback: impl FnMut(&SelectItem) + 'static) -> &mut Self {
        self.on_selection_change = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_filter_changed`] when the filter text changes.
    pub fn on_filter_changed(&mut self, callback: impl FnMut(&str) + 'static) -> &mut Self {
        self.on_filter_changed = Some(Box::new(callback));
        self
    }

    /// Move the selection one step for a wheel notch.
    pub fn handle_wheel(&mut self, delta: isize) {
        if delta != 0 {
            self.move_selection(delta, false);
        }
    }

    /// Select the row at `index` and request activation.
    pub fn click_item(&mut self, index: usize) {
        let Some(item) = self.state.visible_items().get(index) else {
            return;
        };
        if item.disabled {
            return;
        }
        if self.state.set_selected_index(index as isize, 1) {
            self.notify_selection_change();
        }
        self.activate();
    }

    fn style_row(&self, row: &str, selected: bool, hovered: bool, disabled: bool) -> String {
        let styled = if disabled {
            self.theme.muted.paint(row)
        } else if selected {
            self.theme.selected.paint(row)
        } else {
            row.to_string()
        };
        if selected || hovered {
            self.theme.selected_row.paint(&styled)
        } else {
            styled
        }
    }

    /// True while the status line applies.
    pub fn should_render_status(&self) -> bool {
        self.state.pending_key().is_some()
            || !self.state.query().is_empty()
            || self.can_edit_search()
    }

    fn pending_confirmation(&self) -> Option<&str> {
        let pending = self.state.pending_key()?;
        self.state
            .visible_items()
            .iter()
            .find(|item| item.key == pending)
            .and_then(|item| item.confirmation.as_deref())
    }

    fn status_text(&self) -> String {
        if let Some(confirmation) = self.pending_confirmation() {
            return format!("  {}", confirmation);
        }
        let query = self.state.query().trim();
        if !query.is_empty() {
            return format!("  Search: {}", query);
        }
        if self.can_edit_search() {
            return "  Type to search".to_string();
        }
        String::new()
    }

    fn notify_selection_change(&mut self) {
        if let Some(item) = self.state.selected_item() {
            if let Some(callback) = self.on_selection_change.as_mut() {
                callback(item);
            }
        }
    }

    fn activate(&mut self) {
        let activation = self.state.request_activation(self.state.selected_index());
        if let MenuActivation::Confirmed(item) = activation {
            if let Some(callback) = self.on_select.as_mut() {
                callback(&item);
            }
        }
    }

    fn primary_column_width(&self, visible: &[SelectItem]) -> usize {
        let widest = visible
            .iter()
            .map(|item| visible_width(&display_title(item)) + PRIMARY_COLUMN_GAP)
            .max()
            .unwrap_or(0);
        widest.clamp(1, self.max_primary_column_width)
    }


    fn scrollbar_cell(&self, row: usize, window: crate::components::menu_selection::MenuWindow) -> String {
        let height = self.max_visible;
        let total = window.total_rows;
        if total <= height {
            return self.theme.muted.paint(TRACK_GLYPH);
        }
        let thumb = (height * height / total).max(1).min(height);
        let max_offset = total - height;
        let start = if max_offset == 0 {
            0
        } else {
            window.row_offset * (height - thumb) / max_offset
        };
        let end = (start + thumb).min(height);
        if row >= start && row < end {
            self.theme.select_label.paint(THUMB_GLYPH)
        } else {
            self.theme.muted.paint(TRACK_GLYPH)
        }
    }
}

/// Title plus the optional secondary tag.
fn display_title(item: &SelectItem) -> String {
    match &item.label {
        Some(label) => format!("{}  {}", item.title, label),
        None => item.title.clone(),
    }
}

fn fit_row(row: &str, width: usize) -> String {
    if width == 0 {
        String::new()
    } else {
        truncate_to_width(row, width, Ellipsis::Omit)
    }
}

impl Component for SelectList {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.hit_rows.clear();
        if width == 0 {
            return vec![String::new()];
        }
        let width = width as usize;
        let mut hit_rows: Vec<Option<usize>> = Vec::new();
        let rows = {
            let visible = self.state.visible_items();
            let selected = self.state.selected_index().max(0) as usize;
            if visible.is_empty() {
                let mut rows = Vec::new();
                if self.should_render_status() {
                    let status = self.theme.hint.paint(&self.status_text());
                    rows.push(pad_to_width(&fit_row(&status, width), width));
                }
                let message = if self.state.query().trim().is_empty() {
                    self.empty_text.as_str()
                } else {
                    self.no_match_text.as_str()
                };
                let painted = self.theme.muted.paint(&format!("  {}", message));
                rows.push(pad_to_width(&fit_row(&painted, width), width));
                return rows;
            }
            let primary = self.primary_column_width(visible);
            let counts = vec![1usize; visible.len()];
            let window = get_menu_window(&counts, selected, self.max_visible, None);
            let overflow = window.total_rows > self.max_visible;
            let row_width = width.saturating_sub(if overflow { 1 } else { 0 });
            let mut rows: Vec<String> = Vec::new();
            for index in window.start_index..window.end_index {
                if rows.len() >= self.max_visible {
                    break;
                }
                let Some(item) = visible.get(index) else {
                    continue;
                };
                let is_selected = index == selected;
                let is_hovered = !is_selected && self.hovered_index == Some(index);
                let prefix = if is_selected {
                    format!("{} ", DEFAULT_CURSOR)
                } else {
                    "  ".to_string()
                };
                let prefix_width = visible_width(&prefix);
                let title = display_title(item);
                let mut row = prefix.clone();
                let description = item
                    .description
                    .as_deref()
                    .map(sanitize_single_line)
                    .filter(|text| !text.is_empty());
                let laid_out = if let Some(description) = description.filter(|_| width > 40) {
                    let effective = primary
                        .clamp(1, width.saturating_sub(prefix_width + 4).max(1));
                    let max_primary = effective.saturating_sub(PRIMARY_COLUMN_GAP).max(1);
                    let truncated_title = truncate_to_width(&title, max_primary, Ellipsis::Omit);
                    let spacing_width = effective.saturating_sub(visible_width(&truncated_title)).max(1);
                    let description_start =
                        prefix_width + visible_width(&truncated_title) + spacing_width;
                    let remaining = width.saturating_sub(description_start + 2);
                    if remaining > MIN_DESCRIPTION_WIDTH {
                        row.push_str(&truncated_title);
                        row.push_str(&" ".repeat(spacing_width));
                        row.push_str(&truncate_to_width(&description, remaining, Ellipsis::Omit));
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !laid_out {
                    let max_title = width.saturating_sub(prefix_width + 2);
                    row.push_str(&fit_row(&title, max_title));
                }
                let styled = self.style_row(&row, is_selected, is_hovered, item.disabled);
                let mut line = pad_to_width(&fit_row(&styled, row_width), row_width);
                if overflow {
                    line.push_str(&self.scrollbar_cell(rows.len(), window));
                }
                hit_rows.push(Some(index));
                rows.push(line);
            }
            while rows.len() < self.max_visible {
                rows.push(pad_to_width("", width));
            }
            if self.should_render_status() {
                let status = self.theme.hint.paint(&self.status_text());
                rows.push(pad_to_width(&fit_row(&status, width), width));
            }
            rows
        };
        self.hit_rows = hit_rows;
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        if crate::keys::is_escape(key) {
            if !self.state.cancel_confirmation() {
                if let Some(callback) = self.on_cancel.as_mut() {
                    callback();
                }
            }
            return;
        }
        if let Some(character) = key.as_char() {
            if self.can_edit_search() {
                if self.state.query().is_empty() && character.is_whitespace() {
                    return;
                }
                let mut query = self.state.query().to_string();
                query.push(character);
                self.set_filter(&query);
                return;
            }
        }
        if crate::keys::is_arrow_up(key) || matches!(key, Key::Ctrl('p')) {
            self.move_selection(-1, self.wrap_navigation);
            return;
        }
        if crate::keys::is_arrow_down(key) || matches!(key, Key::Ctrl('n')) {
            self.move_selection(1, self.wrap_navigation);
            return;
        }
        if crate::keys::is_page_up(key) {
            self.move_selection(-(self.max_visible as isize), false);
            return;
        }
        if crate::keys::is_page_down(key) {
            self.move_selection(self.max_visible as isize, false);
            return;
        }
        if crate::keys::is_home(key) {
            if self.state.move_to_boundary(true) {
                self.notify_selection_change();
            }
            return;
        }
        if crate::keys::is_end(key) {
            if self.state.move_to_boundary(false) {
                self.notify_selection_change();
            }
            return;
        }
        if crate::keys::is_enter(key) {
            self.activate();
            return;
        }
        if crate::keys::is_backspace(key) {
            if self.can_edit_search() && !self.state.query().is_empty() {
                let mut chars = self.state.query().chars();
                chars.next_back();
                let query: String = chars.collect();
                self.set_filter(&query);
            }
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        match event.kind {
            MouseKind::Down(MouseButtonKind::Left) => {
                let Some(index) = self.hit_rows.get(event.row as usize).copied().flatten() else {
                    return;
                };
                self.click_item(index);
            }
            MouseKind::ScrollUp => self.handle_wheel(-1),
            MouseKind::ScrollDown => self.handle_wheel(1),
            _ => {}
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if self.can_edit_search() {
            let mut query = self.state.query().to_string();
            query.push_str(text);
            self.set_filter(&query);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_theme() -> SelectListTheme {
        SelectListTheme {
            muted: Style::default(),
            selected: Style::default(),
            selected_row: Style::default(),
            border: Style::default(),
            select_label: Style::default(),
            hint: Style::default(),
            symbols: SymbolTheme::unicode(),
        }
    }

    fn items() -> Vec<SelectItem> {
        vec![
            SelectItem::new("a", "Alpha").with_description("First option"),
            SelectItem::new("b", "Beta").with_description("Second option"),
            SelectItem::new("c", "Gamma"),
        ]
    }

    fn trimmed(rows: Vec<String>) -> Vec<String> {
        rows.into_iter().map(|row| row.trim_end().to_string()).collect()
    }

    #[test]
    fn renders_exact_rows_at_fixed_width() {
        let mut list = SelectList::new(items(), 3, plain_theme());
        assert_eq!(
            trimmed(list.render(30)),
            vec!["> Alpha", "  Beta", "  Gamma"]
        );
    }

    #[test]
    fn navigation_moves_and_wraps() {
        let mut list = SelectList::new(items(), 3, plain_theme());
        list.handle_input(&Key::Down);
        assert_eq!(list.selected_key().as_deref(), Some("b"));
        list.handle_input(&Key::Ctrl('n'));
        assert_eq!(list.selected_key().as_deref(), Some("c"));
        list.handle_input(&Key::Down);
        assert_eq!(list.selected_key().as_deref(), Some("a"));
        list.handle_input(&Key::Up);
        assert_eq!(list.selected_key().as_deref(), Some("c"));
        list.handle_input(&Key::Ctrl('p'));
        assert_eq!(list.selected_key().as_deref(), Some("b"));
        list.handle_input(&Key::End);
        assert_eq!(list.selected_key().as_deref(), Some("c"));
        list.handle_input(&Key::Home);
        assert_eq!(list.selected_key().as_deref(), Some("a"));
    }

    #[test]
    fn no_wrap_clamps_at_edges() {
        let mut list = SelectList::new(items(), 3, plain_theme());
        list.set_wrap_navigation(false);
        list.handle_input(&Key::Up);
        assert_eq!(list.selected_key().as_deref(), Some("a"));
        list.handle_input(&Key::End);
        list.handle_input(&Key::Down);
        assert_eq!(list.selected_key().as_deref(), Some("c"));
    }

    #[test]
    fn page_keys_move_by_budget() {
        let many: Vec<SelectItem> = (0..9)
            .map(|index| SelectItem::new(format!("k{index}"), format!("Item {index}")))
            .collect();
        let mut list = SelectList::new(many, 3, plain_theme());
        list.handle_input(&Key::PageDown);
        assert_eq!(list.selected_key().as_deref(), Some("k3"));
        list.handle_input(&Key::PageUp);
        assert_eq!(list.selected_key().as_deref(), Some("k0"));
    }

    #[test]
    fn enter_fires_select_and_esc_fires_cancel() {
        let picked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&picked);
        let cancels = std::sync::Arc::new(std::sync::Mutex::new(0));
        let cancel_sink = std::sync::Arc::clone(&cancels);
        let mut list = SelectList::new(items(), 3, plain_theme());
        list.on_select(move |item| sink.lock().unwrap().push(item.key.clone()));
        list.on_cancel(move || *cancel_sink.lock().unwrap() += 1);
        list.handle_input(&Key::Down);
        list.handle_input(&Key::Enter);
        assert_eq!(picked.lock().unwrap().as_slice(), ["b".to_string()]);
        list.handle_input(&Key::Esc);
        assert_eq!(*cancels.lock().unwrap(), 1);
    }

    #[test]
    fn filter_narrows_and_notifies() {
        let filters = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&filters);
        let mut list = SelectList::new(items(), 3, plain_theme());
        list.set_search_policy(SearchPolicy::Always);
        list.on_filter_changed(move |query| sink.lock().unwrap().push(query.to_string()));
        list.handle_input(&Key::Char('g'));
        assert_eq!(list.filter(), "g");
        assert_eq!(list.visible_items().len(), 1);
        assert_eq!(list.selected_key().as_deref(), Some("c"));
        assert_eq!(filters.lock().unwrap().as_slice(), ["g".to_string()]);
        list.handle_input(&Key::Backspace);
        assert_eq!(list.filter(), "");
        assert_eq!(list.visible_items().len(), 3);
    }

    #[test]
    fn disabled_rows_are_skipped() {
        let mut list = SelectList::new(
            vec![
                SelectItem::new("a", "Alpha").disabled(),
                SelectItem::new("b", "Beta"),
            ],
            2,
            plain_theme(),
        );
        assert_eq!(list.selected_key().as_deref(), Some("b"));
        list.handle_input(&Key::Up);
        assert_eq!(list.selected_key().as_deref(), Some("b"));
    }

    #[test]
    fn confirmation_needs_two_enters() {
        let picked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&picked);
        let mut list = SelectList::new(
            vec![SelectItem::new("a", "Alpha").with_confirmation("Really?")],
            1,
            plain_theme(),
        );
        list.on_select(move |item| sink.lock().unwrap().push(item.key.clone()));
        list.handle_input(&Key::Enter);
        assert!(picked.lock().unwrap().is_empty());
        assert_eq!(trimmed(list.render(20)), vec!["> Alpha", "  Really?"]);
        list.handle_input(&Key::Enter);
        assert_eq!(picked.lock().unwrap().as_slice(), ["a".to_string()]);
    }

    #[test]
    fn empty_state_renders_message() {
        let mut list = SelectList::new(Vec::new(), 3, plain_theme());
        assert_eq!(list.selected_index(), -1);
        assert_eq!(trimmed(list.render(20)), vec!["  No items"]);
    }

    #[test]
    fn no_match_state_renders_message() {
        let mut list = SelectList::new(items(), 3, plain_theme());
        list.set_filter("zzz");
        assert_eq!(
            trimmed(list.render(20)),
            vec!["  Search: zzz", "  No matching items"]
        );
    }

    #[test]
    fn narrow_widths_never_exceed() {
        let mut list = SelectList::new(items(), 2, plain_theme());
        for width in [1u16, 2, 3, 9, 20] {
            for row in list.render(width) {
                assert!(visible_width(&row) <= width as usize);
            }
        }
        assert_eq!(list.render(0), vec![""]);
    }

    #[test]
    fn mouse_click_selects_and_activates() {
        let picked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&picked);
        let mut list = SelectList::new(items(), 3, plain_theme());
        list.on_select(move |item| sink.lock().unwrap().push(item.key.clone()));
        list.render(20);
        list.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 1,
            row: 1,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(list.selected_key().as_deref(), Some("b"));
        assert_eq!(picked.lock().unwrap().as_slice(), ["b".to_string()]);
    }

    #[test]
    fn styled_theme_paints_selection() {
        let mut list = SelectList::new(items(), 3, SelectListTheme::default());
        let rows = list.render(20);
        assert!(rows[0].contains("Alpha"));
        assert!(rows[0].contains('\x1b'));
        assert_eq!(trimmed(rows)[1], "  Beta");
    }
}
