//! Aligned label/value list with wrapped continuations and selection.

use crate::component::Component;
use crate::keys::{is_arrow_down, is_arrow_up, is_end, is_home, Key};
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::style::{Color, Style};
use crate::text_shape::{truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};

/// One label/value entry.
pub struct KeyValueRow {
    /// Text in the label column.
    pub label: String,
    /// Text in the value column; wraps onto continuation lines.
    pub value: String,
    /// Style for the label cell.
    pub label_style: Option<Style>,
    /// Style for the value text.
    pub value_style: Option<Style>,
}

impl KeyValueRow {
    /// A row with default styling.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        KeyValueRow {
            label: label.into(),
            value: value.into(),
            label_style: None,
            value_style: None,
        }
    }

    /// Style the label cell.
    pub fn with_label_style(mut self, style: Style) -> Self {
        self.label_style = Some(style);
        self
    }

    /// Style the value text.
    pub fn with_value_style(mut self, style: Style) -> Self {
        self.value_style = Some(style);
        self
    }
}

/// Label overflow policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyValueOverflow {
    /// Keep the label; the line is clipped to the viewport.
    Allow,
    /// Clip the label to the rendered label column.
    Truncate,
}

/// Styles and sizing for a [`KeyValueList`].
#[derive(Clone, Debug, PartialEq)]
pub struct KeyValueListTheme {
    /// Label cell style.
    pub label: Style,
    /// Value style.
    pub value: Style,
    /// Style layered over the selected row.
    pub selected: Style,
    /// Text between the label cell and the value.
    pub gap: String,
    /// Text before the label column.
    pub indent: String,
    /// Requested label column width.
    pub label_width: u16,
    /// Smallest value column width.
    pub min_value_width: u16,
    /// Label overflow policy.
    pub label_overflow: KeyValueOverflow,
}

impl Default for KeyValueListTheme {
    fn default() -> Self {
        KeyValueListTheme {
            label: Style::fg(Color::DarkGray),
            value: Style::default(),
            selected: Style::default().reverse(),
            gap: " ".to_string(),
            indent: String::new(),
            label_width: 12,
            min_value_width: 1,
            label_overflow: KeyValueOverflow::Truncate,
        }
    }
}

/// A bounded label/value list with keyboard selection.
pub struct KeyValueList {
    rows: Vec<KeyValueRow>,
    theme: KeyValueListTheme,
    selected_index: usize,
    on_select: Option<Box<dyn FnMut(usize) + 'static>>,
    line_starts: Vec<usize>,
}

impl Default for KeyValueList {
    fn default() -> Self {
        KeyValueList {
            rows: Vec::new(),
            theme: KeyValueListTheme::default(),
            selected_index: 0,
            on_select: None,
            line_starts: Vec::new(),
        }
    }
}

impl KeyValueList {
    /// A list over `rows`.
    pub fn new(rows: Vec<KeyValueRow>) -> Self {
        KeyValueList {
            rows,
            theme: KeyValueListTheme::default(),
            selected_index: 0,
            on_select: None,
            line_starts: Vec::new(),
        }
    }

    /// Replace the rows.
    pub fn set_items(&mut self, rows: Vec<KeyValueRow>) {
        self.rows = rows;
        self.selected_index = self.selected_index.min(self.rows.len().saturating_sub(1));
    }

    /// The current rows.
    pub fn items(&self) -> &[KeyValueRow] {
        &self.rows
    }

    /// The selected row index.
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Select `index`, clamped to the rows; fires `on_select` on change.
    pub fn set_selected_index(&mut self, index: usize) {
        let clamped = index.min(self.rows.len().saturating_sub(1));
        if clamped == self.selected_index {
            return;
        }
        self.selected_index = clamped;
        if let Some(callback) = self.on_select.as_mut() {
            callback(self.selected_index);
        }
    }

    /// Select the next row.
    pub fn select_next(&mut self) {
        self.set_selected_index(self.selected_index.saturating_add(1));
    }

    /// Select the previous row.
    pub fn select_previous(&mut self) {
        self.set_selected_index(self.selected_index.saturating_sub(1));
    }

    /// Select the first row.
    pub fn select_first(&mut self) {
        self.set_selected_index(0);
    }

    /// Select the last row.
    pub fn select_last(&mut self) {
        self.set_selected_index(self.rows.len().saturating_sub(1));
    }

    /// The theme.
    pub fn theme(&self) -> &KeyValueListTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: KeyValueListTheme) {
        self.theme = theme;
    }

    /// Register a callback fired with the new index on selection changes.
    pub fn set_on_select<F>(&mut self, callback: F)
    where
        F: FnMut(usize) + 'static,
    {
        self.on_select = Some(Box::new(callback));
    }

    /// Apply selection styling and clip a rendered line to `width`.
    fn finish_row(&self, line: &str, row_index: usize, width: usize) -> String {
        let styled = if row_index == self.selected_index {
            self.theme.selected.paint(line)
        } else {
            line.to_string()
        };
        truncate_to_width(&styled, width, Ellipsis::Omit)
    }
}

impl Component for KeyValueList {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        self.line_starts.clear();
        if width == 0 {
            return Vec::new();
        }
        let indent = single_line(&self.theme.indent);
        let gap = single_line(&self.theme.gap);
        let label_width = self.theme.label_width as usize;
        let min_value_width = (self.theme.min_value_width as usize).max(1);
        let indent_width = visible_width(&indent);
        let gap_width = visible_width(&gap);
        let constrained = width.saturating_sub(indent_width + gap_width + min_value_width);
        let rendered_label_width = label_width.min(constrained);
        let mut lines = Vec::new();
        for (row_index, row) in self.rows.iter().enumerate() {
            self.line_starts.push(lines.len());
            let mut label = single_line(&row.label);
            if rendered_label_width == 0 {
                label.clear();
            } else if visible_width(&label) > rendered_label_width
                && (self.theme.label_overflow == KeyValueOverflow::Truncate
                    || rendered_label_width < label_width)
            {
                label = truncate_to_width(&label, rendered_label_width, Ellipsis::Omit);
            }
            let label_padding = rendered_label_width.saturating_sub(visible_width(&label));
            let raw_label_cell = format!("{}{}", label, " ".repeat(label_padding));
            let label_cell = row
                .label_style
                .unwrap_or(self.theme.label)
                .paint(&raw_label_cell);
            let prefix_width = indent_width + visible_width(&raw_label_cell) + gap_width;
            let value_width = width.saturating_sub(prefix_width).max(1);
            let raw_value = single_line(&row.value);
            let value = row
                .value_style
                .unwrap_or(self.theme.value)
                .paint(&raw_value);
            let wrapped = wrap_text_with_ansi(&value, value_width);
            let continuation = " ".repeat(prefix_width);
            let first = format!(
                "{}{}{}{}",
                indent,
                label_cell,
                gap,
                wrapped.first().map(|value| value.as_str()).unwrap_or("")
            );
            lines.push(self.finish_row(&first, row_index, width));
            for extra in wrapped.iter().skip(1) {
                let line = format!("{continuation}{extra}");
                lines.push(self.finish_row(&line, row_index, width));
            }
        }
        lines
    }

    fn handle_input(&mut self, key: &Key) {
        if is_arrow_down(key) {
            self.select_next();
        } else if is_arrow_up(key) {
            self.select_previous();
        } else if is_home(key) {
            self.select_first();
        } else if is_end(key) {
            self.select_last();
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if event.kind != MouseKind::Down(MouseButtonKind::Left) || self.line_starts.is_empty() {
            return;
        }
        let row = event.row as usize;
        if let Some(index) = self.line_starts.iter().rposition(|start| *start <= row) {
            self.set_selected_index(index);
        }
    }
}

/// Replace tabs and line breaks with spaces.
fn single_line(text: &str) -> String {
    text.replace('\t', " ").replace('\r', " ").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn plain() -> KeyValueListTheme {
        KeyValueListTheme {
            label: Style::default(),
            value: Style::default(),
            selected: Style::default(),
            gap: " ".to_string(),
            indent: String::new(),
            label_width: 6,
            min_value_width: 1,
            label_overflow: KeyValueOverflow::Truncate,
        }
    }

    fn rows() -> Vec<KeyValueRow> {
        vec![
            KeyValueRow::new("Name", "Alice"),
            KeyValueRow::new("Host", "localhost"),
        ]
    }

    #[test]
    fn aligns_labels_and_values() {
        let mut list = KeyValueList::new(rows());
        list.set_theme(plain());
        assert_eq!(list.render(20), vec!["Name   Alice", "Host   localhost"]);
    }

    #[test]
    fn wraps_values_with_aligned_continuations() {
        let mut list = KeyValueList::new(vec![KeyValueRow::new("Key", "abcdefghij")]);
        let mut theme = plain();
        theme.label_width = 4;
        list.set_theme(theme);
        assert_eq!(
            list.render(14),
            vec!["Key  abcdefghi", "     j"]
        );
    }

    #[test]
    fn truncates_labels_when_the_column_shrinks() {
        let mut list = KeyValueList::new(vec![KeyValueRow::new("Hostname", "x")]);
        let mut theme = plain();
        theme.label_width = 8;
        list.set_theme(theme);
        assert_eq!(list.render(8), vec!["Hostna x"]);
    }

    #[test]
    fn highlights_the_selected_row() {
        let mut list = KeyValueList::new(rows());
        let mut theme = plain();
        theme.selected = Style::bg(Color::Blue);
        list.set_theme(theme);
        list.set_selected_index(1);
        let rendered = list.render(20);
        assert_eq!(rendered[0], "Name   Alice");
        assert_eq!(rendered[1], "\x1b[44mHost   localhost\x1b[0m");
    }

    #[test]
    fn navigates_with_keys() {
        let mut list = KeyValueList::new(rows());
        list.handle_input(&Key::Down);
        assert_eq!(list.selected_index(), 1);
        list.handle_input(&Key::Down);
        assert_eq!(list.selected_index(), 1);
        list.handle_input(&Key::End);
        assert_eq!(list.selected_index(), 1);
        list.handle_input(&Key::Home);
        assert_eq!(list.selected_index(), 0);
        list.handle_input(&Key::Up);
        assert_eq!(list.selected_index(), 0);
    }

    #[test]
    fn selects_on_left_click() {
        let mut list = KeyValueList::new(rows());
        list.set_theme(plain());
        list.render(20);
        list.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 0,
            row: 1,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(list.selected_index(), 1);
    }

    #[test]
    fn fires_select_callback() {
        let selected = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&selected);
        let mut list = KeyValueList::new(rows());
        list.set_on_select(move |index| sink.lock().unwrap().push(index));
        list.handle_input(&Key::Down);
        assert_eq!(*selected.lock().unwrap(), vec![1]);
    }

    #[test]
    fn keeps_rows_within_narrow_widths() {
        let mut list = KeyValueList::new(vec![KeyValueRow::new("LongLabel", "a very long value")]);
        let mut theme = plain();
        theme.label_width = 10;
        theme.label_overflow = KeyValueOverflow::Allow;
        list.set_theme(theme);
        for width in [1, 2, 4, 6, 9] {
            for line in list.render(width) {
                assert!(visible_width(&line) <= width as usize, "row too wide: {line:?}");
            }
        }
    }

    #[test]
    fn handles_empty_rows() {
        let mut list = KeyValueList::new(Vec::new());
        list.set_theme(plain());
        assert!(list.render(10).is_empty());
        assert!(list.render(0).is_empty());
    }

    #[test]
    fn reflects_state_changes() {
        let mut list = KeyValueList::new(rows());
        list.set_theme(plain());
        assert_eq!(list.render(20), vec!["Name   Alice", "Host   localhost"]);
        list.set_items(vec![KeyValueRow::new("Only", "one")]);
        assert_eq!(list.render(20), vec!["Only   one"]);
        assert_eq!(list.selected_index(), 0);
    }
}
