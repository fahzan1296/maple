//! Searchable menu state, viewport windowing, and a renderable menu component.
//! Reference: `l/src/components/menu-selection.ts`.
//!
//! This module ports `l/src/components/menu-selection.ts`: the `MenuSelection`
//! state engine (filtering, selection, disabled-row navigation, confirmation)
//! and `getMenuWindow` (the centered/nearest viewport over variable-height
//! rows), plus the searchable menu surface the TypeScript `SelectList` and
//! `SettingsList` build on top of that state.
//!
//! Divergences from the TypeScript original, kept deliberately small:
//! - `MenuSource` is a trait rather than a bag of closures so the shared fuzzy
//!   filter has one home; `filter` is only consulted for non-empty queries,
//!   exactly as in TypeScript.
//! - `selected_index` is `isize` with `-1` meaning "nothing selected", matching
//!   `MenuSelection`'s public contract. Renderers convert to `usize`.
//! - The `MenuSelection` component owns its search query and renders a status
//!   line; the TypeScript `MenuSelection` class is state-only and never renders.
//! - `SymbolTheme` has no `cursor` field, so the selection glyph is the
//!   module-level [`DEFAULT_CURSOR`] constant.

use crate::component::Component;
use crate::keys::Key;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};

/// Glyph drawn before the selected row.
pub const DEFAULT_CURSOR: &str = ">";

const TRACK_GLYPH: &str = "│";
const THUMB_GLYPH: &str = "█";

/// Score `query` as a case-insensitive subsequence of `text`.
///
/// Returns `None` when the query is not a subsequence. Consecutive matches and
/// matches at a word boundary score higher; ties are broken by the caller using
/// the original item order so filtering stays deterministic.
fn fuzzy_score(text: &str, query: &str) -> Option<i64> {
    let haystack: Vec<char> = text.to_lowercase().chars().collect();
    let needles: Vec<char> = query.to_lowercase().chars().collect();
    if needles.is_empty() {
        return Some(0);
    }
    let mut score = 0i64;
    let mut cursor = 0usize;
    let mut previous: Option<usize> = None;
    for &needle in &needles {
        let mut found = None;
        while cursor < haystack.len() {
            if haystack[cursor] == needle {
                found = Some(cursor);
                break;
            }
            cursor += 1;
        }
        let at = found?;
        score += if previous.is_some_and(|earlier| at == earlier + 1) {
            2
        } else {
            1
        };
        if at == 0 || matches!(haystack[at - 1], ' ' | '-' | '_' | '/') {
            score += 2;
        }
        previous = Some(at);
        cursor = at + 1;
    }
    Some(score)
}

/// Keep the items whose search text matches `query`, best score first.
///
/// Items that do not match are dropped; equal scores keep their input order.
pub fn fuzzy_filter<T: Clone>(items: &[T], query: &str, search_text: impl Fn(&T) -> String) -> Vec<T> {
    let query = query.trim();
    if query.is_empty() {
        return items.to_vec();
    }
    let mut scored: Vec<(i64, usize, T)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(score) = fuzzy_score(&search_text(item), query) {
            scored.push((score, index, item.clone()));
        }
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    scored.into_iter().map(|(_, _, item)| item).collect()
}

/// A contiguous viewport over variable-height menu rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuWindow {
    /// First item index included in the window.
    pub start_index: usize,
    /// One past the last item index included in the window.
    pub end_index: usize,
    /// Visual rows before `start_index`, used to position the scrollbar thumb.
    pub row_offset: usize,
    /// Total visual rows across every item.
    pub total_rows: usize,
}

/// Window alignment policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuAlignment {
    /// Center the selected item in the viewport.
    #[default]
    Center,
    /// Preserve the current window until the selection leaves it.
    Nearest,
}

/// Options for [`get_menu_window`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuWindowOptions {
    /// Alignment policy; defaults to [`MenuAlignment::Center`].
    pub alignment: MenuAlignment,
    /// Prior window start used by [`MenuAlignment::Nearest`].
    pub current_start_index: Option<usize>,
}

/// Pick a contiguous window over variable-height items whose rows fit `row_budget`.
///
/// In `Center` mode the selection is centered; in `Nearest` mode the supplied
/// start index is retained until the selection moves past the window edge. A
/// selected item taller than the budget is still included.
pub fn get_menu_window(
    row_counts: &[usize],
    selected_index: usize,
    row_budget: usize,
    options: Option<&MenuWindowOptions>,
) -> MenuWindow {
    let total_rows: usize = row_counts.iter().sum();
    if row_counts.is_empty() {
        return MenuWindow {
            start_index: 0,
            end_index: 0,
            row_offset: 0,
            total_rows: 0,
        };
    }
    let budget = row_budget.max(1);
    let selected = selected_index.min(row_counts.len() - 1);
    let selected_rows = row_counts[selected];
    let alignment = options.map(|option| option.alignment).unwrap_or_default();

    let (mut start_index, mut used) = if alignment == MenuAlignment::Nearest {
        let raw_start = options
            .and_then(|option| option.current_start_index)
            .unwrap_or(0);
        let mut start = raw_start.min(selected);
        let mut rows_needed = 0;
        for index in start..=selected {
            rows_needed += row_counts[index];
        }
        while rows_needed > budget && start < selected {
            rows_needed -= row_counts[start];
            start += 1;
        }
        (start, rows_needed)
    } else {
        let target_rows_before = budget.saturating_sub(selected_rows.min(budget)) / 2;
        let mut start = selected;
        let mut rows_before = 0;
        while start > 0 {
            let cost = row_counts[start - 1];
            if rows_before + cost > target_rows_before {
                break;
            }
            start -= 1;
            rows_before += cost;
        }
        (start, rows_before + selected_rows)
    };

    let mut end_index = selected + 1;

    while end_index < row_counts.len() {
        let cost = row_counts[end_index];
        if used + cost > budget {
            break;
        }
        used += cost;
        end_index += 1;
    }
    while start_index > 0 {
        let cost = row_counts[start_index - 1];
        if used + cost > budget {
            break;
        }
        start_index -= 1;
        used += cost;
    }

    let row_offset: usize = row_counts[..start_index].iter().sum();
    MenuWindow {
        start_index,
        end_index,
        row_offset,
        total_rows,
    }
}

/// Identity, search text, and behavior supplied by a menu's item type.
pub trait MenuSource<T: Clone> {
    /// Stable identity used to retain selection across data and filter changes.
    fn key(&self, item: &T) -> String;
    /// Text the default fuzzy filter matches against.
    fn search_text(&self, item: &T) -> String;
    /// Disabled rows stay visible but are skipped by navigation and activation.
    fn disabled(&self, _item: &T) -> bool {
        false
    }
    /// Rows returning true need a second activation without an intervening change.
    fn requires_confirmation(&self, _item: &T) -> bool {
        false
    }
    /// Domain-specific filtering/ranking; defaults to the shared fuzzy filter.
    fn filter(&self, items: &[T], query: &str) -> Vec<T> {
        fuzzy_filter(items, query, |item| self.search_text(item))
    }
}

/// Outcome of requesting activation of a menu row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuActivation<T> {
    /// No row is selected.
    Empty,
    /// The row is disabled.
    Disabled(T),
    /// The row needs a second activation request.
    Pending(T),
    /// The row is activated.
    Confirmed(T),
}

/// Controlled menu data, filtering, selection, and confirmation state.
pub struct MenuState<T: Clone> {
    items: Vec<T>,
    visible: Vec<T>,
    query: String,
    selected_index: isize,
    pending_key: Option<String>,
    source: Box<dyn MenuSource<T>>,
}

impl<T: Clone> MenuState<T> {
    /// Build a state over `items` using `source` for identity and filtering.
    pub fn new(items: Vec<T>, source: Box<dyn MenuSource<T>>) -> Self {
        let mut state = MenuState {
            items: Vec::new(),
            visible: Vec::new(),
            query: String::new(),
            selected_index: -1,
            pending_key: None,
            source,
        };
        state.set_items_with_key(items, None);
        state
    }

    /// Mutate every item whose key equals `key`, in both the full list and the
    /// visible (filtered) list.
    pub fn update_by_key<F: FnMut(&mut T)>(&mut self, key: &str, mut mutate: F) {
        for item in self.items.iter_mut() {
            if self.source.key(item) == key {
                mutate(item);
            }
        }
        for item in self.visible.iter_mut() {
            if self.source.key(item) == key {
                mutate(item);
            }
        }
    }

    /// Re-run the current filter, retaining the selected key.
    pub fn refilter(&mut self) {
        let key = self.selected_key();
        self.pending_key = None;
        self.apply_filter(key.as_deref());
    }

    /// Every controlled item, before filtering.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// Items surviving the current query, in render order.
    pub fn visible_items(&self) -> &[T] {
        &self.visible
    }

    /// Current filter text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Selected index into [`Self::visible_items`], or `-1` when empty.
    pub fn selected_index(&self) -> isize {
        self.selected_index
    }

    /// Selected item, if any.
    pub fn selected_item(&self) -> Option<&T> {
        if self.selected_index < 0 {
            return None;
        }
        self.visible.get(self.selected_index as usize)
    }

    /// Stable key of the selected item, if any.
    pub fn selected_key(&self) -> Option<String> {
        self.selected_item().map(|item| self.source.key(item))
    }

    /// Key of the row awaiting confirmation, if any.
    pub fn pending_key(&self) -> Option<&str> {
        self.pending_key.as_deref()
    }

    /// True when `item` is disabled.
    pub fn is_disabled(&self, item: &T) -> bool {
        self.source.disabled(item)
    }

    /// True when `item` is the row awaiting confirmation.
    pub fn is_pending(&self, item: &T) -> bool {
        self.pending_key.as_deref() == Some(self.source.key(item).as_str())
    }

    /// Replace the items, retaining the current selection key when it survives.
    pub fn set_items(&mut self, items: Vec<T>) -> bool {
        self.set_items_with_key(items, None)
    }

    /// Replace the items, retaining `preferred_key` (`None` keeps the current
    /// selection key). Returns true when the selected key changed.
    pub fn set_items_with_key(&mut self, items: Vec<T>, preferred_key: Option<String>) -> bool {
        let previous_key = self.selected_key();
        self.items = items;
        self.pending_key = None;
        let preferred = preferred_key.or_else(|| previous_key.clone());
        self.apply_filter(preferred.as_deref());
        previous_key != self.selected_key()
    }

    /// Set the query, retaining the selection key when it survives filtering.
    /// Returns true when the selected key changed.
    pub fn set_query(&mut self, query: &str, retain_selection: bool) -> bool {
        if query == self.query {
            return false;
        }
        let previous_key = self.selected_key();
        self.query = query.to_string();
        self.pending_key = None;
        let preferred = if retain_selection {
            previous_key.clone()
        } else {
            None
        };
        self.apply_filter(preferred.as_deref());
        previous_key != self.selected_key()
    }

    /// Select a visible row, coercing disabled rows to the nearest enabled one.
    /// `direction` picks the scan direction (`1` or `-1`). Returns true when the
    /// selection changed.
    pub fn set_selected_index(&mut self, index: isize, direction: isize) -> bool {
        let next = self.coerce_index(index, direction);
        if next == self.selected_index {
            return false;
        }
        self.selected_index = next;
        self.pending_key = None;
        true
    }

    /// Select the visible row whose key equals `key`. Unknown keys do nothing.
    pub fn set_selected_key(&mut self, key: &str) -> bool {
        match self.visible.iter().position(|item| self.source.key(item) == key) {
            Some(index) => self.set_selected_index(index as isize, 1),
            None => false,
        }
    }

    /// Move by `delta` rows, skipping disabled rows and optionally wrapping.
    pub fn move_selection(&mut self, delta: isize, wrap: bool) -> bool {
        let total = self.visible.len() as isize;
        if total == 0 || delta == 0 {
            return false;
        }
        let direction = if delta < 0 { -1 } else { 1 };
        let mut remaining = delta.abs().max(1);
        let mut index = self.selected_index;
        let mut attempts = 0;
        while attempts < total && remaining > 0 {
            let mut next = index + direction;
            if next < 0 || next >= total {
                if !wrap {
                    break;
                }
                next = if direction > 0 { 0 } else { total - 1 };
            }
            if next == index {
                break;
            }
            index = next;
            let item = &self.visible[index as usize];
            if !self.source.disabled(item) {
                remaining -= 1;
            }
            attempts += 1;
        }
        index != self.selected_index && self.set_selected_index(index, direction)
    }

    /// Jump to the first or last selectable row.
    pub fn move_to_boundary(&mut self, first: bool) -> bool {
        let target = if first {
            0
        } else {
            self.visible.len() as isize - 1
        };
        let direction = if first { 1 } else { -1 };
        self.set_selected_index(target, direction)
    }

    /// Clear a pending confirmation. Returns true only when one was cleared.
    pub fn cancel_confirmation(&mut self) -> bool {
        if self.pending_key.is_none() {
            return false;
        }
        self.pending_key = None;
        true
    }

    /// Request activation of `index` (defaults to the selected row).
    pub fn request_activation(&mut self, index: isize) -> MenuActivation<T> {
        if index < 0 {
            return MenuActivation::Empty;
        }
        let Some(item) = self.visible.get(index as usize) else {
            return MenuActivation::Empty;
        };
        if self.source.disabled(item) {
            return MenuActivation::Disabled(item.clone());
        }
        let key = self.source.key(item);
        if self.source.requires_confirmation(item) && self.pending_key.as_deref() != Some(key.as_str()) {
            self.pending_key = Some(key);
            return MenuActivation::Pending(item.clone());
        }
        self.pending_key = None;
        MenuActivation::Confirmed(item.clone())
    }

    fn apply_filter(&mut self, preferred_key: Option<&str>) {
        let query = self.query.trim();
        self.visible = if query.is_empty() {
            self.items.clone()
        } else {
            self.source.filter(&self.items, query)
        };
        self.selected_index = self.find_initial_index(preferred_key);
    }

    fn find_initial_index(&self, preferred_key: Option<&str>) -> isize {
        if self.visible.is_empty() {
            return -1;
        }
        if let Some(key) = preferred_key {
            if let Some(index) = self
                .visible
                .iter()
                .position(|item| self.source.key(item) == key)
            {
                if !self.source.disabled(&self.visible[index]) {
                    return index as isize;
                }
            }
        }
        self.coerce_index(0, 1)
    }

    fn coerce_index(&self, index: isize, direction: isize) -> isize {
        if self.visible.is_empty() {
            return -1;
        }
        let length = self.visible.len() as isize;
        let clamped = index.clamp(0, length - 1);
        if !self.source.disabled(&self.visible[clamped as usize]) {
            return clamped;
        }
        let mut next = clamped + direction;
        while next >= 0 && next < length {
            if !self.source.disabled(&self.visible[next as usize]) {
                return next;
            }
            next += direction;
        }
        let mut next = clamped - direction;
        while next >= 0 && next < length {
            if !self.source.disabled(&self.visible[next as usize]) {
                return next;
            }
            next -= direction;
        }
        clamped
    }
}

/// One selectable row of a [`MenuSelection`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuOption {
    /// Stable identity.
    pub id: String,
    /// Display text.
    pub label: String,
    /// Optional secondary text rendered after the label.
    pub description: Option<String>,
    /// Disabled rows stay visible but cannot be activated.
    pub disabled: bool,
}

impl MenuOption {
    /// Build an enabled option with `id` and `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        MenuOption {
            id: id.into(),
            label: label.into(),
            description: None,
            disabled: false,
        }
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
}

struct MenuOptionSource;

impl MenuSource<MenuOption> for MenuOptionSource {
    fn key(&self, item: &MenuOption) -> String {
        item.id.clone()
    }

    fn search_text(&self, item: &MenuOption) -> String {
        match &item.description {
            Some(description) => format!("{} {}", item.label, description),
            None => item.label.clone(),
        }
    }

    fn disabled(&self, item: &MenuOption) -> bool {
        item.disabled
    }
}

/// When a menu accepts typed search input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchPolicy {
    /// Typing always filters.
    Always,
    /// Typing filters only when the item count exceeds the visible budget.
    #[default]
    Overflow,
    /// Typing never filters.
    Never,
}

impl SearchPolicy {
    /// True when the policy lets `item_count` items be searched in `max_visible` rows.
    pub fn can_edit(self, item_count: usize, max_visible: usize) -> bool {
        match self {
            SearchPolicy::Always => true,
            SearchPolicy::Overflow => item_count > max_visible,
            SearchPolicy::Never => false,
        }
    }
}

/// Theme for [`MenuSelection`] and the selection rows it renders.
#[derive(Clone)]
pub struct MenuTheme {
    /// Descriptions, disabled rows, and the scrollbar track.
    pub muted: Style,
    /// Label text of the selected row.
    pub selected: Style,
    /// Band painted across the whole selected row.
    pub selected_row: Style,
    /// Border and scrollbar glyphs.
    pub border: Style,
    /// Selection cursor glyph and scrollbar thumb.
    pub select_label: Style,
    /// Search status line.
    pub hint: Style,
    /// Glyph set; retained for hosts that draw their own chrome.
    pub symbols: SymbolTheme,
}

impl Default for MenuTheme {
    fn default() -> Self {
        MenuTheme {
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

/// A searchable, renderable menu built on [`MenuState`].
pub struct MenuSelection {
    state: MenuState<MenuOption>,
    theme: MenuTheme,
    policy: SearchPolicy,
    max_visible: usize,
    on_select: Option<Box<dyn FnMut(&MenuOption)>>,
    on_cancel: Option<Box<dyn FnMut()>>,
    on_selection_change: Option<Box<dyn FnMut(&MenuOption)>>,
    hit_rows: Vec<Option<usize>>,
}

impl MenuSelection {
    /// Build a menu over `options` with a visible budget of `max_visible` rows.
    pub fn new(options: Vec<MenuOption>, max_visible: usize, theme: MenuTheme) -> Self {
        MenuSelection {
            state: MenuState::new(options, Box::new(MenuOptionSource)),
            theme,
            policy: SearchPolicy::default(),
            max_visible: max_visible.max(1),
            on_select: None,
            on_cancel: None,
            on_selection_change: None,
            hit_rows: Vec::new(),
        }
    }

    /// Replace the options, retaining the selected id when it survives.
    pub fn set_menu(&mut self, options: Vec<MenuOption>) {
        if self.state.set_items(options) {
            self.notify_selection_change();
        }
    }

    /// Replace the options, retaining `key` when it survives filtering.
    pub fn set_menu_with_key(&mut self, options: Vec<MenuOption>, key: Option<String>) -> bool {
        let changed = self.state.set_items_with_key(options, key);
        if changed {
            self.notify_selection_change();
        }
        changed
    }

    /// Every option before filtering.
    pub fn options(&self) -> &[MenuOption] {
        self.state.items()
    }

    /// Options surviving the current query.
    pub fn visible_options(&self) -> &[MenuOption] {
        self.state.visible_items()
    }

    /// Selected option, if any.
    pub fn selected(&self) -> Option<&MenuOption> {
        self.state.selected_item()
    }

    /// Stable id of the selected option, if any.
    pub fn selected_key(&self) -> Option<String> {
        self.state.selected_key()
    }

    /// Selected index into [`Self::visible_options`], or `-1`.
    pub fn selected_index(&self) -> isize {
        self.state.selected_index()
    }

    /// Current filter text.
    pub fn query(&self) -> &str {
        self.state.query()
    }

    /// Replace the filter text.
    pub fn set_query(&mut self, query: &str) {
        if self.state.set_query(query, true) {
            self.notify_selection_change();
        }
    }

    /// Clear the filter text.
    pub fn clear_query(&mut self) {
        self.set_query("");
    }

    /// Search policy.
    pub fn search_policy(&self) -> SearchPolicy {
        self.policy
    }

    /// Set the search policy.
    pub fn set_search_policy(&mut self, policy: SearchPolicy) {
        self.policy = policy;
    }

    /// Visible row budget.
    pub fn max_visible(&self) -> usize {
        self.max_visible
    }

    /// Refit the visible row budget.
    pub fn set_max_visible(&mut self, rows: usize) {
        self.max_visible = rows.max(1);
    }

    /// Current theme.
    pub fn theme(&self) -> &MenuTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: MenuTheme) {
        self.theme = theme;
    }

    /// Select the visible row at `index`.
    pub fn set_selected_index(&mut self, index: isize) {
        if self.state.set_selected_index(index, 1) {
            self.notify_selection_change();
        }
    }

    /// Select the visible row whose id equals `key`.
    pub fn set_selected_key(&mut self, key: &str) -> bool {
        let changed = self.state.set_selected_key(key);
        if changed {
            self.notify_selection_change();
        }
        changed
    }

    /// Move the selection, skipping disabled rows.
    pub fn move_selection(&mut self, delta: isize, wrap: bool) -> bool {
        let changed = self.state.move_selection(delta, wrap);
        if changed {
            self.notify_selection_change();
        }
        changed
    }

    /// Resolve a rendered line to its option.
    pub fn hit_test(&self, line: usize) -> Option<&MenuOption> {
        let index = self.hit_rows.get(line).copied().flatten()?;
        self.state.visible_items().get(index)
    }

    /// Fire [`Self::on_select`] with the currently selected option.
    pub fn on_select(&mut self, callback: impl FnMut(&MenuOption) + 'static) -> &mut Self {
        self.on_select = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_cancel`] on cancel.
    pub fn on_cancel(&mut self, callback: impl FnMut() + 'static) -> &mut Self {
        self.on_cancel = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_selection_change`] whenever the highlighted row changes.
    pub fn on_selection_change(&mut self, callback: impl FnMut(&MenuOption) + 'static) -> &mut Self {
        self.on_selection_change = Some(Box::new(callback));
        self
    }

    /// True when the status line applies.
    pub fn should_render_status(&self) -> bool {
        self.state.pending_key().is_some()
            || !self.state.query().is_empty()
            || self.policy.can_edit(self.state.items().len(), self.max_visible)
    }

    fn can_edit_search(&self) -> bool {
        self.policy.can_edit(self.state.items().len(), self.max_visible)
    }

    fn status_text(&self) -> String {
        if let Some(pending) = self.state.pending_key() {
            return format!("  Confirm {}", pending);
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

    fn scrollbar_cell(&self, row: usize, window: MenuWindow) -> String {
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

    fn style_row(&self, row: &str, selected: bool, disabled: bool) -> String {
        let styled = if disabled {
            self.theme.muted.paint(row)
        } else if selected {
            self.theme.selected.paint(row)
        } else {
            row.to_string()
        };
        if selected {
            self.theme.selected_row.paint(&styled)
        } else {
            styled
        }
    }

    fn push_query(&mut self, text: &str) {
        let mut query = self.state.query().to_string();
        query.push_str(text);
        self.set_query(&query);
    }
}

impl Component for MenuSelection {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            self.hit_rows.clear();
            return vec![String::new()];
        }
        let width = width as usize;
        self.hit_rows.clear();
        let mut hit_rows: Vec<Option<usize>> = Vec::new();
        let rows = {
            let visible = self.state.visible_items();
            let selected = self.state.selected_index().max(0) as usize;
            if visible.is_empty() {
                let message = if self.state.query().trim().is_empty() {
                    "  No items"
                } else {
                    "  No matching items"
                };
                let mut rows = vec![pad_to_width(&self.theme.muted.paint(message), width)];
                if self.should_render_status() {
                    let status = self.theme.hint.paint(&self.status_text());
                    rows.push(pad_to_width(&truncate_to_width(&status, width, Ellipsis::Omit), width));
                }
                return rows;
            }
            let counts = vec![1usize; visible.len()];
            let window = get_menu_window(&counts, selected, self.max_visible, None);
            let overflow = window.total_rows > self.max_visible;
            let row_width = width.saturating_sub(if overflow { 1 } else { 0 });
            let mut rows: Vec<String> = Vec::new();
            for index in window.start_index..window.end_index {
                if rows.len() >= self.max_visible {
                    break;
                }
                let Some(option) = visible.get(index) else {
                    continue;
                };
                let is_selected = index == selected;
                let prefix = if is_selected {
                    format!("{} ", DEFAULT_CURSOR)
                } else {
                    "  ".to_string()
                };
                let mut row = prefix + option.label.as_str();
                if let Some(description) = &option.description {
                    if width > 40 {
                        let used = visible_width(&row);
                        let remaining = row_width.saturating_sub(used).saturating_sub(2);
                        if remaining > 10 {
                            row.push_str("  ");
                            row.push_str(&truncate_to_width(description, remaining, Ellipsis::Omit));
                        }
                    }
                }
                let styled = self.style_row(&row, is_selected, option.disabled);
                let clipped = if row_width == 0 {
                    String::new()
                } else {
                    truncate_to_width(&styled, row_width, Ellipsis::Omit)
                };
                let mut line = pad_to_width(&clipped, row_width);
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
                rows.push(pad_to_width(&truncate_to_width(&status, width, Ellipsis::Omit), width));
            }
            rows
        };
        self.hit_rows = hit_rows;
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        if crate::keys::is_escape(key) {
            if self.state.query().is_empty() {
                if let Some(callback) = self.on_cancel.as_mut() {
                    callback();
                }
            } else {
                self.clear_query();
            }
            return;
        }
        if crate::keys::is_arrow_up(key) || matches!(key, Key::Ctrl('p')) {
            self.move_selection(-1, true);
            return;
        }
        if crate::keys::is_arrow_down(key) || matches!(key, Key::Ctrl('n')) {
            self.move_selection(1, true);
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
                self.set_query(&query);
            }
            return;
        }
        if self.can_edit_search() {
            if let Some(character) = key.as_char() {
                if self.state.query().is_empty() && character.is_whitespace() {
                    return;
                }
                let mut query = self.state.query().to_string();
                query.push(character);
                self.set_query(&query);
            }
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        match event.kind {
            MouseKind::Down(MouseButtonKind::Left) => {
                let Some(index) = self.hit_rows.get(event.row as usize).copied().flatten() else {
                    return;
                };
                if self.state.set_selected_index(index as isize, 1) {
                    self.notify_selection_change();
                }
                self.activate();
            }
            MouseKind::ScrollUp => {
                self.move_selection(-1, false);
            }
            MouseKind::ScrollDown => {
                self.move_selection(1, false);
            }
            _ => {}
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if self.can_edit_search() {
            self.push_query(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi_scan::strip_ansi;

    fn theme() -> MenuTheme {
        MenuTheme::default()
    }

    fn options() -> Vec<MenuOption> {
        vec![
            MenuOption::new("a", "Alpha"),
            MenuOption::new("b", "Beta"),
            MenuOption::new("c", "Gamma"),
        ]
    }

    fn trimmed(rows: Vec<String>) -> Vec<String> {
        rows.into_iter()
            .map(|row| strip_ansi(&row).trim_end().to_string())
            .collect()
    }
    #[test]
    fn centers_window_and_moves_selection() {
        let mut menu = MenuSelection::new(options(), 2, theme());
        assert_eq!(menu.selected_index(), 0);
        assert_eq!(
            trimmed(menu.render(12)),
            vec!["> Alpha    █", "  Beta     │", "  Type to se"]
        );
        menu.handle_input(&Key::Down);
        assert_eq!(menu.selected_key().as_deref(), Some("b"));
        assert_eq!(
            trimmed(menu.render(12)),
            vec!["> Beta     │", "  Gamma    █", "  Type to se"]
        );
        menu.handle_input(&Key::Ctrl('p'));
        assert_eq!(menu.selected_key().as_deref(), Some("a"));
        menu.handle_input(&Key::Ctrl('n'));
        assert_eq!(menu.selected_key().as_deref(), Some("b"));
    }

    #[test]
    fn wraps_at_edges() {
        let mut menu = MenuSelection::new(options(), 3, theme());
        menu.handle_input(&Key::Up);
        assert_eq!(menu.selected_key().as_deref(), Some("c"));
        menu.handle_input(&Key::Down);
        assert_eq!(menu.selected_key().as_deref(), Some("a"));
    }

    #[test]
    fn filters_by_subsequence_and_clearing() {
        let mut menu = MenuSelection::new(options(), 3, theme());
        menu.set_query("ga");
        assert_eq!(menu.visible_options().len(), 1);
        assert_eq!(menu.selected_key().as_deref(), Some("c"));
        assert_eq!(
            trimmed(menu.render(14)),
            vec!["> Gamma", "", "", "  Search: ga"]
        );
        menu.handle_input(&Key::Esc);
        assert_eq!(menu.query(), "");
        assert_eq!(menu.visible_options().len(), 3);
    }

    #[test]
    fn backspace_edits_query() {
        let mut menu = MenuSelection::new(options(), 3, theme());
        menu.set_search_policy(SearchPolicy::Always);
        menu.handle_input(&Key::Char('g'));
        assert_eq!(menu.query(), "g");
        menu.handle_input(&Key::Backspace);
        assert_eq!(menu.query(), "");
    }

    #[test]
    fn disabled_rows_are_skipped() {
        let mut menu = MenuSelection::new(
            vec![
                MenuOption::new("a", "Alpha").disabled(),
                MenuOption::new("b", "Beta"),
            ],
            2,
            theme(),
        );
        assert_eq!(menu.selected_key().as_deref(), Some("b"));
        menu.handle_input(&Key::Up);
        assert_eq!(menu.selected_key().as_deref(), Some("b"));
        menu.handle_input(&Key::Enter);
        assert_eq!(menu.selected_key().as_deref(), Some("b"));
    }

    #[test]
    fn enter_fires_select_callback() {
        let picked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&picked);
        let mut menu = MenuSelection::new(options(), 3, theme());
        menu.on_select(move |option| sink.lock().unwrap().push(option.id.clone()));
        menu.handle_input(&Key::Down);
        menu.handle_input(&Key::Enter);
        assert_eq!(picked.lock().unwrap().as_slice(), ["b".to_string()]);
    }

    #[test]
    fn escape_fires_cancel_only_without_query() {
        let cancels = std::sync::Arc::new(std::sync::Mutex::new(0));
        let sink = std::sync::Arc::clone(&cancels);
        let mut menu = MenuSelection::new(options(), 3, theme());
        menu.set_search_policy(SearchPolicy::Always);
        menu.on_cancel(move || *sink.lock().unwrap() += 1);
        menu.handle_input(&Key::Esc);
        assert_eq!(*cancels.lock().unwrap(), 1);
        menu.handle_input(&Key::Char('a'));
        menu.handle_input(&Key::Esc);
        assert_eq!(*cancels.lock().unwrap(), 1);
        assert_eq!(menu.query(), "");
    }

    #[test]
    fn empty_state_renders_without_panic() {
        let mut menu = MenuSelection::new(Vec::new(), 3, theme());
        assert_eq!(menu.selected_index(), -1);
        assert_eq!(trimmed(menu.render(12)), vec!["  No items"]);
        assert_eq!(menu.render(0), vec![""]);
    }

    #[test]
    fn narrow_width_truncates_every_row() {
        let mut menu = MenuSelection::new(options(), 2, theme());
        for width in [1u16, 2, 3, 12] {
            for row in menu.render(width) {
                assert!(visible_width(&row) <= width as usize);
            }
        }
    }

    #[test]
    fn mouse_click_activates_row() {
        let picked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&picked);
        let mut menu = MenuSelection::new(options(), 3, theme());
        menu.on_select(move |option| sink.lock().unwrap().push(option.id.clone()));
        menu.render(12);
        menu.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 1,
            row: 2,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(picked.lock().unwrap().as_slice(), ["c".to_string()]);
        assert_eq!(menu.selected_key().as_deref(), Some("c"));
    }

    #[test]
    fn zero_width_rows_stay_empty() {
        let mut menu = MenuSelection::new(options(), 2, theme());
        assert_eq!(menu.render(0), vec![""]);
    }

    #[test]
    fn fuzzy_ranking_prefers_consecutive_matches() {
        let items = vec!["abc", "axbxc", "zzz"];
        let kept = fuzzy_filter(&items, "abc", |item| item.to_string());
        assert_eq!(kept, vec!["abc".to_string(), "axbxc".to_string()]);
    }
}
