//! Settings panel with value cycling, section headings, and type-to-filter.
//!
//! Ports `l/src/components/settings-list.ts`. Rows carry a label, optional
//! description/warning, a current value, and an optional `values` cycle;
//! Enter/Space cycles the value or fires `on_select`, and typing filters the
//! rows.
//!
//! Divergences from the TypeScript original:
//! - The split sidebar layout and submenu editors are out of scope; the list
//!   always renders the flat layout and documents that choice.
//! - `cursor` is a [`Style`] rather than a glyph string; the glyph is
//!   [`DEFAULT_CURSOR`](crate::components::menu_selection::DEFAULT_CURSOR).
//! - The footer hint is a fixed string rather than being assembled from host
//!   keybinding hints.
//! - `update_value` mutates the row but does not fire `on_toggle`, matching
//!   TypeScript; `apply_value` fires it.

use crate::component::Component;
use crate::keys::Key;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::components::menu_selection::{
    fuzzy_filter, MenuSource, MenuState, DEFAULT_CURSOR,
};
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{
    pad_to_width, truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis,
};

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
    expanded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One settings row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingItem {
    /// Unique identifier.
    pub id: String,
    /// Display label on the left.
    pub label: String,
    /// Optional description shown when the row is selected.
    pub description: Option<String>,
    /// Optional risk note shown above the description with a marker.
    pub warning: Option<String>,
    /// Current value shown on the right.
    pub value: String,
    /// When set, Enter/Space cycles through these values.
    pub values: Option<Vec<String>>,
    /// Render as a non-interactive section heading.
    pub heading: bool,
    /// Disabled rows stay visible but are skipped by navigation.
    pub disabled: bool,
    /// True when the value differs from the default.
    pub changed: bool,
}

impl SettingItem {
    /// Build an enabled row with `id`, `label`, and `value`.
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: impl Into<String>) -> Self {
        SettingItem {
            id: id.into(),
            label: label.into(),
            description: None,
            warning: None,
            value: value.into(),
            values: None,
            heading: false,
            disabled: false,
            changed: false,
        }
    }

    /// Attach a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Attach a risk note.
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warning = Some(warning.into());
        self
    }

    /// Set the cycle values.
    pub fn with_values(mut self, values: Vec<String>) -> Self {
        self.values = Some(values);
        self
    }

    /// Mark the row disabled.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Render as a section heading.
    pub fn heading(mut self) -> Self {
        self.heading = true;
        self
    }

    /// Mark the value as changed from its default.
    pub fn changed(mut self) -> Self {
        self.changed = true;
        self
    }

    fn search_text(&self) -> String {
        let mut text = format!("{} {} {}", self.label, self.id, self.value);
        if let Some(description) = &self.description {
            text.push(' ');
            text.push_str(description);
        }
        if let Some(warning) = &self.warning {
            text.push(' ');
            text.push_str(warning);
        }
        if let Some(values) = &self.values {
            text.push(' ');
            text.push_str(&values.join(" "));
        }
        sanitize_single_line(&text)
    }
}

struct SettingSource;

impl MenuSource<SettingItem> for SettingSource {
    fn key(&self, item: &SettingItem) -> String {
        item.id.clone()
    }

    fn search_text(&self, item: &SettingItem) -> String {
        item.search_text()
    }

    fn disabled(&self, item: &SettingItem) -> bool {
        item.heading || item.disabled
    }

    fn filter(&self, items: &[SettingItem], query: &str) -> Vec<SettingItem> {
        let candidates: Vec<SettingItem> = items
            .iter()
            .filter(|item| !item.heading)
            .cloned()
            .collect();
        fuzzy_filter(&candidates, query, |item| item.search_text())
    }
}

/// Theme for [`SettingsList`].
#[derive(Clone)]
pub struct SettingsListTheme {
    /// Label of an unselected row.
    pub label: Style,
    /// Label of the selected row.
    pub selected_label: Style,
    /// Value of an unselected row.
    pub value: Style,
    /// Value of the selected row.
    pub selected_value: Style,
    /// Description text.
    pub description: Style,
    /// Warning text and marker.
    pub warning: Style,
    /// Selection cursor glyph.
    pub cursor: Style,
    /// Search status, hints, and the scrollbar track.
    pub hint: Style,
    /// Section heading text.
    pub heading: Style,
    /// Band painted across the selected row.
    pub selected_row: Style,
    /// Glyph marking a row that carries a warning.
    pub warning_mark: String,
    /// Glyph set; retained for hosts that draw their own chrome.
    pub symbols: SymbolTheme,
}

impl Default for SettingsListTheme {
    fn default() -> Self {
        SettingsListTheme {
            label: Style::default(),
            selected_label: Style::fg(Color::White).bold(),
            value: Style::fg(Color::DarkGray),
            selected_value: Style::fg(Color::White),
            description: Style::fg(Color::DarkGray),
            warning: Style::fg(Color::Yellow),
            cursor: Style::fg(Color::White).bold(),
            hint: Style::fg(Color::DarkGray).dim(),
            heading: Style::fg(Color::DarkGray).bold(),
            selected_row: Style::default(),
            warning_mark: "!".to_string(),
            symbols: SymbolTheme::unicode(),
        }
    }
}

/// A contiguous run of rows under one heading.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SettingSection {
    name: String,
    first_item_index: Option<usize>,
}

/// A settings panel.
pub struct SettingsList {
    state: MenuState<SettingItem>,
    theme: SettingsListTheme,
    max_visible: usize,
    type_to_search: bool,
    hint: Option<String>,
    empty_text: String,
    section_focus: bool,
    hovered_id: Option<String>,
    on_toggle: Option<Box<dyn FnMut(&str, &str)>>,
    on_select: Option<Box<dyn FnMut(&SettingItem)>>,
    on_cancel: Option<Box<dyn FnMut()>>,
    on_selection_change: Option<Box<dyn FnMut(&SettingItem)>>,
    hit_rows: Vec<Option<String>>,
}

impl Default for SettingsList {
    fn default() -> Self {
        SettingsList::new(Vec::new(), 5, SettingsListTheme::default())
    }
}

impl SettingsList {
    /// Build a settings list over `items`.
    pub fn new(items: Vec<SettingItem>, max_visible: usize, theme: SettingsListTheme) -> Self {
        SettingsList {
            state: MenuState::new(items, Box::new(SettingSource)),
            theme,
            max_visible: max_visible.max(3),
            type_to_search: true,
            hint: None,
            empty_text: "No settings available".to_string(),
            section_focus: false,
            hovered_id: None,
            on_toggle: None,
            on_select: None,
            on_cancel: None,
            on_selection_change: None,
            hit_rows: Vec::new(),
        }
    }

    /// Replace the rows, preserving the selected id when it survives filtering.
    pub fn set_items(&mut self, items: Vec<SettingItem>) {
        let selected_id = self.state.selected_key();
        let previous_index = self.state.selected_index();
        self.state.set_items(items);
        let survives = self
            .state
            .visible_items()
            .iter()
            .any(|item| selected_id.as_deref() == Some(item.id.as_str()));
        if selected_id.is_some() && !survives {
            let length = self.state.visible_items().len();
            let target = previous_index.max(0).min(length.saturating_sub(1) as isize);
            self.state.set_selected_index(target, 1);
        }
        if self.section_focus && !self.has_section_focus_targets() {
            self.section_focus = false;
        }
        self.notify_selection_change();
    }

    /// Every row before filtering.
    pub fn items(&self) -> &[SettingItem] {
        self.state.items()
    }

    /// Rows surviving the current filter.
    pub fn filtered_items(&self) -> &[SettingItem] {
        self.state.visible_items()
    }

    /// Selected row, or `None` when the selection sits on a heading.
    pub fn get_selected_item(&self) -> Option<&SettingItem> {
        let item = self.state.selected_item()?;
        if item.heading {
            None
        } else {
            Some(item)
        }
    }

    /// Selected index into [`Self::filtered_items`], or `0` when empty.
    pub fn selected_index(&self) -> usize {
        self.state.selected_index().max(0) as usize
    }

    /// Move selection to the row with `id`. Returns false when it is not visible.
    pub fn select_item(&mut self, id: &str) -> bool {
        let found = self
            .state
            .visible_items()
            .iter()
            .any(|item| !item.heading && item.id == id);
        if !found {
            return false;
        }
        self.state.set_selected_key(id);
        self.section_focus = false;
        self.notify_selection_change();
        true
    }

    /// Visible row budget (minimum 3).
    pub fn max_visible(&self) -> usize {
        self.max_visible
    }

    /// Resize the visible viewport (minimum 3).
    pub fn set_max_visible(&mut self, rows: usize) {
        self.max_visible = rows.max(3);
    }

    /// Whether printable input filters the rows.
    pub fn type_to_search(&self) -> bool {
        self.type_to_search
    }

    /// Set whether printable input filters the rows.
    pub fn set_type_to_search(&mut self, enabled: bool) {
        self.type_to_search = enabled;
    }

    /// Footer hint override (`None` uses the default, `Some("")` hides it).
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// Override the footer hint.
    pub fn set_hint(&mut self, hint: Option<String>) {
        self.hint = hint;
    }

    /// Message shown when there are no rows at all.
    pub fn empty_text(&self) -> &str {
        &self.empty_text
    }

    /// Set the empty-list message.
    pub fn set_empty_text(&mut self, text: impl Into<String>) {
        self.empty_text = text.into();
    }

    /// Current theme.
    pub fn theme(&self) -> &SettingsListTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: SettingsListTheme) {
        self.theme = theme;
    }

    /// Current filter text.
    pub fn search_query(&self) -> &str {
        self.state.query()
    }

    /// Replace the filter text (the selection is not retained).
    pub fn set_search_query(&mut self, query: &str) {
        self.state.set_query(query, false);
        self.notify_selection_change();
    }

    /// Clear the filter text.
    pub fn clear_search(&mut self) {
        if self.state.query().is_empty() {
            return;
        }
        self.set_search_query("");
    }

    /// True while keyboard focus is on the section headings.
    pub fn section_focused(&self) -> bool {
        self.section_focus
    }

    /// Whether section focus has anywhere to go.
    pub fn has_section_focus_targets(&self) -> bool {
        self.sections().len() >= 2
    }

    /// Toggle focus between section headings and rows.
    pub fn toggle_section_focus(&mut self) -> bool {
        self.section_focus = !self.section_focus && self.has_section_focus_targets();
        self.section_focus
    }

    /// Highlight the row under the pointer (`None` clears).
    pub fn set_hovered_id(&mut self, id: Option<String>) {
        self.hovered_id = id;
    }

    /// Resolve a rendered line to its row id.
    pub fn hit_test(&self, line: usize) -> Option<&SettingItem> {
        let id = self.hit_rows.get(line).cloned().flatten()?;
        self.state.visible_items().iter().find(|item| item.id == id)
    }

    /// Update a row's value without firing `on_toggle`.
    pub fn update_value(&mut self, id: &str, new_value: &str) {
        let new_value = new_value.to_string();
        self.state
            .update_by_key(id, |item| item.value = new_value.clone());
        if !self.state.query().trim().is_empty() {
            self.state.refilter();
        }
    }

    /// Apply `value` to a cycling row the way Enter does, firing `on_toggle`.
    pub fn apply_value(&mut self, id: &str, value: &str) {
        if !self.select_item(id) {
            return;
        }
        let Some(item) = self.state.selected_item() else {
            return;
        };
        if item.heading {
            return;
        }
        match &item.values {
            Some(values) if values.iter().any(|candidate| candidate == value) => {}
            _ => return,
        }
        self.state
            .update_by_key(id, |item| item.value = value.to_string());
        if let Some(callback) = self.on_toggle.as_mut() {
            callback(id, value);
        }
    }

    /// Fire [`Self::on_toggle`] when a row's value changes.
    pub fn on_toggle(&mut self, callback: impl FnMut(&str, &str) + 'static) -> &mut Self {
        self.on_toggle = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_select`] when a row is activated.
    pub fn on_select(&mut self, callback: impl FnMut(&SettingItem) + 'static) -> &mut Self {
        self.on_select = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_cancel`] on cancel.
    pub fn on_cancel(&mut self, callback: impl FnMut() + 'static) -> &mut Self {
        self.on_cancel = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_selection_change`] whenever the highlighted row changes.
    pub fn on_selection_change(&mut self, callback: impl FnMut(&SettingItem) + 'static) -> &mut Self {
        self.on_selection_change = Some(Box::new(callback));
        self
    }

    /// Move the selection one step for a wheel notch.
    pub fn handle_wheel(&mut self, delta: isize) {
        if delta != 0 {
            self.move_selection(delta, false);
        }
    }

    /// Height every render is padded to.
    fn stable_height(&self) -> usize {
        let mut height = self.max_visible + 4;
        if self.type_to_search {
            height += 1;
        }
        if self.hint.as_deref() != Some("") {
            height += 2;
        }
        height
    }

    fn sections(&self) -> Vec<SettingSection> {
        let mut sections: Vec<SettingSection> = Vec::new();
        for (index, item) in self.state.visible_items().iter().enumerate() {
            if item.heading {
                sections.push(SettingSection {
                    name: item.label.clone(),
                    first_item_index: None,
                });
                continue;
            }
            match sections.last_mut() {
                Some(section) if section.first_item_index.is_none() => {
                    section.first_item_index = Some(index);
                }
                Some(_) => {}
                None => sections.push(SettingSection {
                    name: String::new(),
                    first_item_index: Some(index),
                }),
            }
        }
        sections.retain(|section| section.first_item_index.is_some());
        sections
    }

    fn active_section_index(&self, sections: &[SettingSection]) -> usize {
        let selected = self.selected_index();
        for (index, section) in sections.iter().enumerate().rev() {
            if section.first_item_index.is_some_and(|first| first <= selected) {
                return index;
            }
        }
        0
    }

    fn move_selection(&mut self, delta: isize, wrap: bool) -> bool {
        let changed = self.state.move_selection(delta, wrap);
        if changed {
            self.notify_selection_change();
        }
        changed
    }

    fn jump_section(&mut self, delta: isize) {
        let sections = self.sections();
        if sections.len() < 2 {
            let length = self.state.visible_items().len();
            if length == 0 {
                return;
            }
            let current = self.selected_index() as isize;
            let target = (current + delta * self.max_visible as isize)
                .clamp(0, length as isize - 1);
            if self
                .state
                .set_selected_index(target, if delta < 0 { -1 } else { 1 })
            {
                self.notify_selection_change();
            }
            return;
        }
        let active = self.active_section_index(&sections);
        let next = (active as isize + delta).rem_euclid(sections.len() as isize) as usize;
        if let Some(first) = sections[next].first_item_index {
            if self.state.set_selected_index(first as isize, 1) {
                self.notify_selection_change();
            }
        }
    }

    fn notify_selection_change(&mut self) {
        let item = self.get_selected_item().cloned();
        if let Some(item) = item {
            if let Some(callback) = self.on_selection_change.as_mut() {
                callback(&item);
            }
        }
    }

    fn activate_selected(&mut self) {
        let Some(item) = self.state.selected_item() else {
            return;
        };
        if item.heading {
            return;
        }
        let id = item.id.clone();
        let values = item.values.clone();
        if let Some(callback) = self.on_select.as_mut() {
            if let Some(current) = self.state.selected_item() {
                callback(current);
            }
        }
        let Some(values) = values.filter(|values| !values.is_empty()) else {
            return;
        };
        let current = self
            .state
            .selected_item()
            .map(|item| item.value.clone())
            .unwrap_or_default();
        let next_index = values
            .iter()
            .position(|value| *value == current)
            .map(|index| (index + 1) % values.len())
            .unwrap_or(0);
        let next = values[next_index].clone();
        if next == current {
            return;
        }
        self.state
            .update_by_key(&id, |item| item.value = next.clone());
        if let Some(callback) = self.on_toggle.as_mut() {
            callback(&id, &next);
        }
    }

    fn warning_mark(&self, item: &SettingItem) -> String {
        if item.warning.is_some() && !self.theme.warning_mark.is_empty() {
            format!(" {}", self.theme.warning_mark)
        } else {
            String::new()
        }
    }

    fn hint_text(&self) -> String {
        self.hint
            .clone()
            .unwrap_or_else(|| "Enter/Space to change · Type to search · Esc to cancel".to_string())
    }

    fn search_status(&self) -> String {
        let query = sanitize_single_line(self.state.query().trim());
        if query.is_empty() {
            "  Type to search".to_string()
        } else {
            format!("  Search: {}", query)
        }
    }

    fn render_item_row(
        &self,
        item: &SettingItem,
        index: usize,
        max_label_width: usize,
        row_width: usize,
        section_cursor: bool,
    ) -> String {
        if item.heading {
            let prefix = if section_cursor {
                format!("{} ", DEFAULT_CURSOR)
            } else {
                "  ".to_string()
            };
            let text = format!("{}{}", prefix, self.theme.heading.paint(&item.label));
            return pad_to_width(&fit_row(&text, row_width), row_width);
        }
        let is_selected = index == self.selected_index() && !self.section_focus;
        let prefix = if is_selected {
            format!("{} ", DEFAULT_CURSOR)
        } else {
            "  ".to_string()
        };
        let prefix_width = visible_width(&prefix);
        let mark = self.warning_mark(item);
        let label_plain = format!("{}{}", item.label, mark);
        let label_pad = " ".repeat(max_label_width.saturating_sub(visible_width(&label_plain)));
        let value_max = row_width
            .saturating_sub(prefix_width + max_label_width + 4);
        let value_plain = fit_row(&item.value, value_max);
        let label_style = if is_selected {
            &self.theme.selected_label
        } else {
            &self.theme.label
        };
        let value_style = if is_selected {
            &self.theme.selected_value
        } else {
            &self.theme.value
        };
        let label_text = format!(
            "{}{}{}",
            label_style.paint(&item.label),
            self.theme.warning.paint(&mark),
            label_pad
        );
        let value_text = value_style.paint(&value_plain);
        let text = format!("{}{}  {}", prefix, label_text, value_text);
        let clipped = fit_row(&text, row_width);
        let hovered = !is_selected && self.hovered_id.as_deref() == Some(item.id.as_str());
        let styled = if is_selected || hovered {
            self.theme.selected_row.paint(&clipped)
        } else {
            clipped
        };
        pad_to_width(&styled, row_width)
    }

    fn scrollbar_cell(&self, row: usize, offset: usize, total: usize) -> String {
        let height = self.max_visible;
        if total <= height {
            return self.theme.hint.paint("│");
        }
        let thumb = (height * height / total).max(1).min(height);
        let max_offset = total - height;
        let start = if max_offset == 0 {
            0
        } else {
            offset * (height - thumb) / max_offset
        };
        let end = (start + thumb).min(height);
        if row >= start && row < end {
            self.theme.cursor.paint("█")
        } else {
            self.theme.hint.paint("│")
        }
    }

    fn description_rows(&self, width: usize) -> Vec<String> {
        let mut rows: Vec<String> = Vec::new();
        let Some(item) = self.state.visible_items().get(self.selected_index()) else {
            return rows;
        };
        if item.heading {
            return rows;
        }
        let wrap_width = width.saturating_sub(4);
        if wrap_width == 0 {
            return rows;
        }
        if let Some(warning) = &item.warning {
            for line in wrap_text_with_ansi(warning, wrap_width) {
                rows.push(self.theme.warning.paint(&format!(
                    "  {}{}",
                    self.theme.warning_mark, line
                )));
            }
        }
        if let Some(description) = &item.description {
            for line in wrap_text_with_ansi(description, wrap_width) {
                rows.push(self.theme.description.paint(&format!("  {}", line)));
            }
        }
        if rows.len() > 3 {
            rows.truncate(3);
            let last = rows.pop().unwrap_or_default();
            rows.push(fit_row(&format!("{}…", last), width));
        }
        while rows.len() < 3 {
            rows.push(String::new());
        }
        rows.into_iter()
            .map(|row| pad_to_width(&fit_row(&row, width), width))
            .collect()
    }
}

fn fit_row(row: &str, width: usize) -> String {
    if width == 0 {
        String::new()
    } else {
        truncate_to_width(row, width, Ellipsis::Omit)
    }
}

impl Component for SettingsList {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.hit_rows.clear();
        if width == 0 {
            return vec![String::new()];
        }
        let width = width as usize;
        let mut hit_rows: Vec<Option<String>> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        {
            let visible = self.state.visible_items();
            let selected = self.selected_index();
            if self.state.items().is_empty() {
                let message = format!("  {}", self.empty_text);
                lines.push(pad_to_width(&self.theme.hint.paint(&message), width));
            } else if visible.is_empty() {
                if self.type_to_search {
                    let status = self.theme.hint.paint(&self.search_status());
                    lines.push(pad_to_width(&fit_row(&status, width), width));
                }
                lines.push(pad_to_width(
                    &self.theme.hint.paint("  No matching settings"),
                    width,
                ));
                lines.push(String::new());
                lines.push(pad_to_width(
                    &self.theme.hint.paint("  Backspace to edit search · Esc to cancel"),
                    width,
                ));
            } else {
                let viewport = self.max_visible.min(visible.len());
                let start = selected
                    .saturating_sub(viewport / 2)
                    .min(visible.len().saturating_sub(viewport));
                let max_label_width = {
                    let widest = visible
                        .iter()
                        .filter(|item| !item.heading)
                        .map(|item| visible_width(&format!("{}{}", item.label, self.warning_mark(item))))
                        .max()
                        .unwrap_or(0);
                    widest.min(30)
                };
                let overflow = visible.len() > viewport;
                let row_width = width.saturating_sub(if overflow { 1 } else { 0 });
                let sections = self.sections();
                let focused_heading = if self.section_focus {
                    let active = sections.get(self.active_section_index(&sections));
                    match active {
                        Some(section) if !section.name.is_empty() => section
                            .first_item_index
                            .and_then(|first| first.checked_sub(1)),
                        _ => None,
                    }
                } else {
                    None
                };
                for row in 0..viewport {
                    let index = start + row;
                    let Some(item) = visible.get(index) else {
                        break;
                    };
                    let rendered = self.render_item_row(
                        item,
                        index,
                        max_label_width,
                        row_width,
                        focused_heading == Some(index),
                    );
                    let mut line = rendered;
                    if overflow {
                        line.push_str(&self.scrollbar_cell(row, start, visible.len()));
                    }
                    hit_rows.push(if item.heading {
                        None
                    } else {
                        Some(item.id.clone())
                    });
                    lines.push(line);
                }
                lines.push(String::new());
                lines.extend(self.description_rows(width));
                if self.type_to_search {
                    let status = self.theme.hint.paint(&self.search_status());
                    lines.push(pad_to_width(&fit_row(&status, width), width));
                }
                if self.hint.as_deref() != Some("") {
                    lines.push(String::new());
                    let hint = self.theme.hint.paint(&format!("  {}", self.hint_text()));
                    lines.push(pad_to_width(&fit_row(&hint, width), width));
                }
            }
        }
        let stable = self.stable_height();
        while lines.len() < stable {
            lines.push(pad_to_width("", width));
        }
        self.hit_rows = hit_rows;
        lines
    }

    fn handle_input(&mut self, key: &Key) {
        if crate::keys::is_escape(key) {
            if !self.state.query().is_empty() {
                self.clear_search();
            } else if self.section_focus {
                self.section_focus = false;
            } else if let Some(callback) = self.on_cancel.as_mut() {
                callback();
            }
            return;
        }
        if self.type_to_search && !self.state.items().is_empty() {
            if crate::keys::is_backspace(key) {
                if !self.state.query().is_empty() {
                    let mut chars = self.state.query().chars();
                    chars.next_back();
                    let query: String = chars.collect();
                    self.set_search_query(&query);
                }
                return;
            }
            if let Some(character) = key.as_char() {
                if !(self.state.query().is_empty() && character.is_whitespace()) {
                    let mut query = self.state.query().to_string();
                    query.push(character);
                    self.set_search_query(&query);
                    return;
                }
            }
        }
        if crate::keys::is_arrow_up(key) || matches!(key, Key::Ctrl('p')) {
            if self.section_focus {
                self.jump_section(-1);
            } else {
                self.move_selection(-1, true);
            }
            return;
        }
        if crate::keys::is_arrow_down(key) || matches!(key, Key::Ctrl('n')) {
            if self.section_focus {
                self.jump_section(1);
            } else {
                self.move_selection(1, true);
            }
            return;
        }
        if crate::keys::is_page_up(key) {
            self.jump_section(-1);
            return;
        }
        if crate::keys::is_page_down(key) {
            self.jump_section(1);
            return;
        }
        if crate::keys::is_enter(key) || matches!(key, Key::Char(' ')) {
            if self.section_focus {
                self.section_focus = false;
            } else {
                self.activate_selected();
            }
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        match event.kind {
            MouseKind::Down(MouseButtonKind::Left) => {
                let Some(id) = self.hit_rows.get(event.row as usize).cloned().flatten() else {
                    return;
                };
                if self.select_item(&id) {
                    self.activate_selected();
                }
            }
            MouseKind::ScrollUp => self.handle_wheel(-1),
            MouseKind::ScrollDown => self.handle_wheel(1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_theme() -> SettingsListTheme {
        SettingsListTheme {
            label: Style::default(),
            selected_label: Style::default(),
            selected_value: Style::default(),
            value: Style::default(),
            description: Style::default(),
            warning: Style::default(),
            cursor: Style::default(),
            hint: Style::default(),
            heading: Style::default(),
            selected_row: Style::default(),
            warning_mark: "!".to_string(),
            symbols: SymbolTheme::unicode(),
        }
    }

    fn rows() -> Vec<SettingItem> {
        vec![
            SettingItem::new("theme", "Theme", "dark").with_values(vec![
                "dark".to_string(),
                "light".to_string(),
            ]),
            SettingItem::new("model", "Model", "gpt-4").with_description("Model id"),
            SettingItem::new("verbose", "Verbose", "off"),
        ]
    }

    fn trimmed(lines: Vec<String>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect()
    }

    #[test]
    fn renders_exact_rows_at_fixed_width() {
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        let lines = trimmed(list.render(30));
        assert_eq!(&lines[0], "> Theme    dark");
        assert_eq!(&lines[1], "  Model    gpt-4");
        assert_eq!(&lines[2], "  Verbose  off");
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "");
        assert_eq!(lines[5], "");
        assert_eq!(lines[6], "");
    }

    #[test]
    fn description_rows_follow_selection() {
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.handle_input(&Key::Down);
        let lines = trimmed(list.render(30));
        assert_eq!(&lines[1], "> Model    gpt-4");
        assert_eq!(lines[4], "  Model id");
    }

    #[test]
    fn navigation_skips_headings() {
        let mut list = SettingsList::new(
            vec![
                SettingItem::new("h", "General", "").heading(),
                SettingItem::new("a", "Alpha", "one"),
                SettingItem::new("b", "Beta", "two"),
            ],
            5,
            plain_theme(),
        );
        assert_eq!(list.get_selected_item().unwrap().id, "a");
        list.handle_input(&Key::Down);
        assert_eq!(list.get_selected_item().unwrap().id, "b");
        list.handle_input(&Key::Down);
        assert_eq!(list.get_selected_item().unwrap().id, "a");
    }

    #[test]
    fn enter_cycles_value_and_fires_toggle() {
        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.on_toggle(move |id, value| sink.lock().unwrap().push((id.to_string(), value.to_string())));
        list.handle_input(&Key::Enter);
        assert_eq!(
            changes.lock().unwrap().to_vec(),
            [("theme".to_string(), "light".to_string())]
        );
        assert_eq!(list.items()[0].value, "light");
        list.handle_input(&Key::Enter);
        assert_eq!(list.items()[0].value, "dark");
    }

    #[test]
    fn space_activates_without_filtering() {
        let selected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&selected);
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.on_select(move |item| sink.lock().unwrap().push(item.id.clone()));
        list.handle_input(&Key::Char(' '));
        assert_eq!(selected.lock().unwrap().to_vec(), ["theme".to_string()]);
    }

    #[test]
    fn typing_filters_and_escape_clears() {
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.handle_input(&Key::Char('v'));
        assert_eq!(list.search_query(), "v");
        assert_eq!(list.filtered_items().len(), 1);
        assert_eq!(list.get_selected_item().unwrap().id, "verbose");
        list.handle_input(&Key::Esc);
        assert_eq!(list.search_query(), "");
        assert_eq!(list.filtered_items().len(), 3);
    }

    #[test]
    fn escape_without_query_cancels() {
        let cancels = std::sync::Arc::new(std::sync::Mutex::new(0));
        let sink = std::sync::Arc::clone(&cancels);
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.on_cancel(move || *sink.lock().unwrap() += 1);
        list.handle_input(&Key::Esc);
        assert_eq!(*cancels.lock().unwrap(), 1);
    }

    #[test]
    fn update_value_is_silent_and_apply_value_toggles() {
        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.on_toggle(move |id, value| sink.lock().unwrap().push((id.to_string(), value.to_string())));
        list.update_value("theme", "light");
        assert!(changes.lock().unwrap().is_empty());
        assert_eq!(list.items()[0].value, "light");
        list.apply_value("theme", "dark");
        assert_eq!(
            changes.lock().unwrap().to_vec(),
            [("theme".to_string(), "dark".to_string())]
        );
    }

    #[test]
    fn sections_and_focus() {
        let mut list = SettingsList::new(
            vec![
                SettingItem::new("h1", "General", "").heading(),
                SettingItem::new("a", "Alpha", "one"),
                SettingItem::new("h2", "Advanced", "").heading(),
                SettingItem::new("b", "Beta", "two"),
            ],
            5,
            plain_theme(),
        );
        assert_eq!(list.sections().len(), 2);
        assert!(!list.section_focused());
        assert!(list.toggle_section_focus());
        assert!(list.section_focused());
        list.handle_input(&Key::Down);
        assert_eq!(list.get_selected_item().unwrap().id, "b");
        list.handle_input(&Key::Enter);
        assert!(!list.section_focused());
    }

    #[test]
    fn empty_state_renders_hint() {
        let mut list = SettingsList::new(Vec::new(), 3, plain_theme());
        let lines = trimmed(list.render(30));
        assert_eq!(lines[0], "  No settings available");
    }

    #[test]
    fn no_match_state_renders_guidance() {
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.set_search_query("zzz");
        let lines = trimmed(list.render(40));
        assert!(lines.iter().any(|line| line == "  No matching settings"));
        assert!(lines.iter().any(|line| line.contains("to edit search")));
    }

    #[test]
    fn narrow_widths_never_exceed() {
        let mut list = SettingsList::new(rows(), 2, plain_theme());
        for width in [1u16, 2, 3, 9, 20] {
            for line in list.render(width) {
                assert!(visible_width(&line) <= width as usize);
            }
        }
        assert_eq!(list.render(0), vec![""]);
    }

    #[test]
    fn mouse_click_activates_row() {
        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        list.on_toggle(move |id, value| sink.lock().unwrap().push((id.to_string(), value.to_string())));
        list.render(30);
        list.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 1,
            row: 0,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(
            changes.lock().unwrap().to_vec(),
            [("theme".to_string(), "light".to_string())]
        );
    }

    #[test]
    fn stable_height_is_constant() {
        let mut list = SettingsList::new(rows(), 5, plain_theme());
        let first = list.render(30).len();
        list.handle_input(&Key::Down);
        let second = list.render(30).len();
        list.handle_input(&Key::Char('v'));
        let third = list.render(30).len();
        assert_eq!(first, second);
        assert_eq!(second, third);
    }
}
