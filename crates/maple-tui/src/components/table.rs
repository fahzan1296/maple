//! Bounded data table with a header row, selection highlight, and keyboard
//! navigation.

use crate::component::Component;
use crate::keys::{is_arrow_down, is_arrow_up, is_end, is_home, is_page_down, is_page_up, Key};
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::style::{Color, Style};
use crate::symbols::{SymbolTheme, TableSymbols};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

/// Horizontal alignment of a table cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAlign {
    /// Pad on the right.
    Left,
    /// Pad on the left.
    Right,
}

/// Overflow policy for a table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableOverflow {
    /// Keep the content; the finished row is clipped to the viewport.
    Allow,
    /// Clip this column to its fitted width.
    Truncate,
}

/// One table column: heading, cell accessor, width, and styling.
pub struct TableColumn {
    title: String,
    getter: Option<Box<dyn Fn(&[String]) -> String + 'static>>,
    width: u16,
    min_width: u16,
    align: TableAlign,
    overflow: TableOverflow,
    priority: i32,
    style: Option<Style>,
}

impl TableColumn {
    /// A left-aligned truncating column using the row cell at `index`.
    pub fn new(title: impl Into<String>, width: u16) -> Self {
        TableColumn {
            title: title.into(),
            getter: None,
            width,
            min_width: 1,
            align: TableAlign::Left,
            overflow: TableOverflow::Truncate,
            priority: 0,
            style: None,
        }
    }

    /// Use `getter` to produce the cell text for a row slice.
    pub fn with_getter<F>(mut self, getter: F) -> Self
    where
        F: Fn(&[String]) -> String + 'static,
    {
        self.getter = Some(Box::new(getter));
        self
    }

    /// Style every cell in this column.
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Align cell content to the right edge of the column.
    pub fn right_aligned(mut self) -> Self {
        self.align = TableAlign::Right;
        self
    }

    /// Keep overflowing content instead of clipping the column.
    pub fn allow_overflow(mut self) -> Self {
        self.overflow = TableOverflow::Allow;
        self
    }

    /// Set the width this column shrinks to before it stops giving way.
    pub fn with_min_width(mut self, min_width: u16) -> Self {
        self.min_width = min_width;
        self
    }

    /// Set shrink priority; lower values give way first, rightmost on ties.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// The heading text.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The requested column width.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// The column style, if one was set.
    pub fn style(&self) -> Option<Style> {
        self.style
    }
}

/// Colors, glyphs, and spacing for a [`Table`].
#[derive(Clone, Debug, PartialEq)]
pub struct TableTheme {
    /// Header heading style.
    pub header: Style,
    /// Body cell style.
    pub row: Style,
    /// Style layered over the selected body row.
    pub selected: Style,
    /// Border glyph style.
    pub border: Style,
    /// Border glyphs.
    pub symbols: TableSymbols,
    /// Text placed between columns.
    pub gap: String,
    /// Text placed before the first column.
    pub indent: String,
    /// Draw a border around the table.
    pub show_border: bool,
    /// Draw a rule between the header and the body.
    pub show_header_separator: bool,
}

impl Default for TableTheme {
    fn default() -> Self {
        TableTheme {
            header: Style::default().bold(),
            row: Style::default(),
            selected: Style::default().reverse(),
            border: Style::fg(Color::DarkGray),
            symbols: SymbolTheme::unicode().table,
            gap: " ".to_string(),
            indent: String::new(),
            show_border: false,
            show_header_separator: false,
        }
    }
}

/// Structured rows with a header, selection, and keyboard navigation.
pub struct Table {
    columns: Vec<TableColumn>,
    rows: Vec<Vec<String>>,
    theme: TableTheme,
    selected_index: usize,
    page_size: usize,
    on_select: Option<Box<dyn FnMut(usize) + 'static>>,
    on_change: Option<Box<dyn FnMut(usize) + 'static>>,
}

impl Default for Table {
    fn default() -> Self {
        Table::new(Vec::new(), Vec::new())
    }
}

impl Table {
    /// A table over `rows`, each row a slice of cell strings.
    pub fn new(columns: Vec<TableColumn>, rows: Vec<Vec<String>>) -> Self {
        Table {
            columns,
            rows,
            theme: TableTheme::default(),
            selected_index: 0,
            page_size: 10,
            on_select: None,
            on_change: None,
        }
    }

    /// Replace columns and rows, clamping the selection and reporting the new
    /// row count through `on_change`.
    pub fn set_table(&mut self, columns: Vec<TableColumn>, rows: Vec<Vec<String>>) {
        self.columns = columns;
        self.rows = rows;
        self.selected_index = self.selected_index.min(self.rows.len().saturating_sub(1));
        if let Some(callback) = self.on_change.as_mut() {
            callback(self.rows.len());
        }
    }

    /// Replace the body rows, keeping the columns.
    pub fn set_rows(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
        self.selected_index = self.selected_index.min(self.rows.len().saturating_sub(1));
        if let Some(callback) = self.on_change.as_mut() {
            callback(self.rows.len());
        }
    }

    /// Replace the columns, keeping the rows.
    pub fn set_columns(&mut self, columns: Vec<TableColumn>) {
        self.columns = columns;
    }

    /// The current columns.
    pub fn columns(&self) -> &[TableColumn] {
        &self.columns
    }

    /// The current body rows.
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// The selected body row index.
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Select `index`, clamped to the body; fires `on_select` on change.
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

    /// Move the selection down one row.
    pub fn select_next(&mut self) {
        self.set_selected_index(self.selected_index.saturating_add(1));
    }

    /// Move the selection up one row.
    pub fn select_previous(&mut self) {
        self.set_selected_index(self.selected_index.saturating_sub(1));
    }

    /// Move the selection down one page.
    pub fn select_page_next(&mut self) {
        self.set_selected_index(self.selected_index.saturating_add(self.page_size));
    }

    /// Move the selection up one page.
    pub fn select_page_previous(&mut self) {
        self.set_selected_index(self.selected_index.saturating_sub(self.page_size));
    }

    /// Select the first body row.
    pub fn select_first(&mut self) {
        self.set_selected_index(0);
    }

    /// Select the last body row.
    pub fn select_last(&mut self) {
        self.set_selected_index(self.rows.len().saturating_sub(1));
    }

    /// The theme.
    pub fn theme(&self) -> &TableTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: TableTheme) {
        self.theme = theme;
    }

    /// The number of rows a page key jumps.
    pub fn page_size(&self) -> usize {
        self.page_size
    }

    /// Set the page jump size; zero is treated as one row.
    pub fn set_page_size(&mut self, page_size: usize) {
        self.page_size = page_size.max(1);
    }

    /// Register a callback fired with the new index whenever the selection
    /// changes through input or a setter.
    pub fn set_on_select<F>(&mut self, callback: F)
    where
        F: FnMut(usize) + 'static,
    {
        self.on_select = Some(Box::new(callback));
    }

    /// Register a callback fired with the body row count whenever the data is
    /// replaced.
    pub fn set_on_change<F>(&mut self, callback: F)
    where
        F: FnMut(usize) + 'static,
    {
        self.on_change = Some(Box::new(callback));
    }

    /// The number of rows above the body (border, header, header rule).
    fn header_height(&self) -> usize {
        if self.columns.is_empty() {
            return 0;
        }
        let bordered = self.theme.show_border as usize;
        let separator = self.theme.show_header_separator as usize;
        bordered + 1 + separator
    }

    fn cell_text(column: &TableColumn, row: &[String], index: usize) -> String {
        match &column.getter {
            Some(getter) => getter(row),
            None => row.get(index).cloned().unwrap_or_default(),
        }
    }
}

impl Component for Table {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 || self.columns.is_empty() {
            return Vec::new();
        }
        let bordered = self.theme.show_border && width >= 4;
        let inner_width = if bordered { width - 2 } else { width };
        let indent = single_line(&self.theme.indent);
        let indent_width = visible_width(&indent);
        let content_width = inner_width.saturating_sub(indent_width);
        let gap = single_line(&self.theme.gap);
        let gap_width = visible_width(&gap);
        let widths = fit_widths(&self.columns, content_width, gap_width);

        let mut lines = Vec::new();
        if bordered {
            lines.push(self.theme.border.paint(&border_row(
                &self.theme.symbols.top_left,
                &self.theme.symbols.top_right,
                &self.theme.symbols.horizontal,
                inner_width,
            )));
        }

        let header_cells: Vec<String> = self
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let style = Style::merge(self.theme.header, column.style.unwrap_or_default());
                align_cell(&column.title, column, widths[index], style)
            })
            .collect();
        let header_text = single_line(&format!("{}{}", indent, header_cells.join(&gap)));
        if bordered {
            let inner = truncate_to_width(&header_text, inner_width, Ellipsis::Omit);
            let left = self.theme.border.paint(&self.theme.symbols.vertical);
            let right = self.theme.border.paint(&self.theme.symbols.vertical);
            lines.push(truncate_to_width(
                &format!("{left}{inner}{right}"),
                width,
                Ellipsis::Omit,
            ));
        } else {
            lines.push(truncate_to_width(&header_text, width, Ellipsis::Omit));
        }

        if self.theme.show_header_separator {
            lines.push(self.theme.border.paint(&border_row(
                &self.theme.symbols.tee_left,
                &self.theme.symbols.tee_right,
                &self.theme.symbols.horizontal,
                inner_width,
            )));
        }

        for (index, row) in self.rows.iter().enumerate() {
            let cells: Vec<String> = self
                .columns
                .iter()
                .enumerate()
                .map(|(column_index, column)| {
                    let text = Self::cell_text(column, row, column_index);
                    let style = Style::merge(self.theme.row, column.style.unwrap_or_default());
                    align_cell(&text, column, widths[column_index], style)
                })
                .collect();
            let mut line = format!("{}{}", indent, cells.join(&gap));
            if index == self.selected_index {
                line = self.theme.selected.paint(&line);
            }
            let line = single_line(&line);
            if bordered {
                let inner = truncate_to_width(&line, inner_width, Ellipsis::Omit);
                let left = self.theme.border.paint(&self.theme.symbols.vertical);
                let right = self.theme.border.paint(&self.theme.symbols.vertical);
                lines.push(truncate_to_width(
                    &format!("{left}{inner}{right}"),
                    width,
                    Ellipsis::Omit,
                ));
            } else {
                lines.push(truncate_to_width(&line, width, Ellipsis::Omit));
            }
        }

        if bordered {
            lines.push(self.theme.border.paint(&border_row(
                &self.theme.symbols.bottom_left,
                &self.theme.symbols.bottom_right,
                &self.theme.symbols.horizontal,
                inner_width,
            )));
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
        } else if is_page_down(key) {
            self.select_page_next();
        } else if is_page_up(key) {
            self.select_page_previous();
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if event.kind != MouseKind::Down(MouseButtonKind::Left) {
            return;
        }
        let header = self.header_height();
        let row = match event.row as usize {
            row if row >= header => row - header,
            _ => return,
        };
        if row < self.rows.len() {
            self.set_selected_index(row);
        }
    }
}

/// Replace tabs and line breaks with spaces.
fn single_line(text: &str) -> String {
    text.replace('\t', " ").replace('\r', " ").replace('\n', " ")
}

/// Shrink truncating columns by priority until the row fits `max_width`.
fn fit_widths(columns: &[TableColumn], max_width: usize, gap_width: usize) -> Vec<usize> {
    let mut widths: Vec<usize> = columns.iter().map(|column| column.width as usize).collect();
    let gaps = gap_width * columns.len().saturating_sub(1);
    let mut overflow = widths.iter().sum::<usize>() + gaps;
    if overflow <= max_width {
        return widths;
    }
    overflow -= max_width;

    let mut order: Vec<usize> = (0..columns.len()).collect();
    order.sort_by(|a, b| {
        columns[*a]
            .priority
            .cmp(&columns[*b].priority)
            .then_with(|| b.cmp(a))
    });
    for index in order {
        if overflow == 0 {
            break;
        }
        let column = &columns[index];
        if column.overflow == TableOverflow::Allow {
            continue;
        }
        let minimum = (column.min_width as usize).min(widths[index]);
        let shrink = overflow.min(widths[index] - minimum);
        widths[index] -= shrink;
        overflow -= shrink;
    }
    widths
}

/// Align one cell, padding it to `width` and styling the padded result.
fn align_cell(text: &str, column: &TableColumn, width: usize, style: Style) -> String {
    if width == 0 {
        return String::new();
    }
    let text = single_line(text);
    let fitted = if visible_width(&text) > width && column.overflow == TableOverflow::Truncate {
        truncate_to_width(&text, width, Ellipsis::Omit)
    } else {
        text
    };
    let padding = width.saturating_sub(visible_width(&fitted));
    let aligned = if column.align == TableAlign::Right {
        format!("{}{}", " ".repeat(padding), fitted)
    } else {
        format!("{}{}", fitted, " ".repeat(padding))
    };
    style.paint(&aligned)
}

/// A horizontal border row of `width` inner cells.
fn border_row(left: &str, right: &str, horizontal: &str, width: usize) -> String {
    let mut row = String::with_capacity(width + 2);
    row.push_str(left);
    for _ in 0..width {
        row.push_str(horizontal);
    }
    row.push_str(right);
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn plain(show_border: bool, separator: bool) -> TableTheme {
        TableTheme {
            header: Style::default(),
            row: Style::default(),
            selected: Style::default(),
            border: Style::default(),
            symbols: SymbolTheme::unicode().table,
            gap: " ".to_string(),
            indent: String::new(),
            show_border,
            show_header_separator: separator,
        }
    }

    fn columns() -> Vec<TableColumn> {
        vec![
            TableColumn::new("Name", 6),
            TableColumn::new("Age", 3),
            TableColumn::new("City", 8),
        ]
    }

    fn rows() -> Vec<Vec<String>> {
        vec![
            vec!["Alice".to_string(), "30".to_string(), "NYC".to_string()],
            vec!["Bob".to_string(), "7".to_string(), "London".to_string()],
            vec!["Cara".to_string(), "51".to_string(), "Paris".to_string()],
        ]
    }

    #[test]
    fn renders_header_and_body_rows() {
        let mut table = Table::new(columns(), rows());
        table.set_theme(plain(false, false));
        assert_eq!(
            table.render(19),
            vec![
                "Name   Age City    ",
                "Alice  30  NYC     ",
                "Bob    7   London  ",
                "Cara   51  Paris   ",
            ]
        );
    }

    #[test]
    fn renders_border_and_header_rule() {
        let mut table = Table::new(columns(), vec![rows()[0].clone()]);
        let mut theme = plain(true, true);
        theme.symbols = SymbolTheme::ascii().table;
        table.set_theme(theme);
        assert_eq!(
            table.render(21),
            vec![
                "+-------------------+",
                "|Name   Age City    |",
                "+-------------------+",
                "|Alice  30  NYC     |",
                "+-------------------+",
            ]
        );
    }

    #[test]
    fn highlights_the_selected_row() {
        let mut table = Table::new(columns(), rows());
        let mut theme = plain(false, false);
        theme.selected = Style::bg(Color::Blue);
        table.set_theme(theme);
        table.set_selected_index(1);
        let rendered = table.render(19);
        assert_eq!(rendered[0], "Name   Age City    ");
        assert_eq!(rendered[2], "\x1b[44mBob    7   London  \x1b[0m");
        assert_eq!(visible_width(&rendered[2]), 19);
    }

    #[test]
    fn navigates_with_keys() {
        let mut table = Table::new(columns(), rows());
        table.set_page_size(2);
        table.handle_input(&Key::Down);
        assert_eq!(table.selected_index(), 1);
        table.handle_input(&Key::PageDown);
        assert_eq!(table.selected_index(), 2);
        table.handle_input(&Key::PageDown);
        assert_eq!(table.selected_index(), 2);
        table.handle_input(&Key::End);
        assert_eq!(table.selected_index(), 2);
        table.handle_input(&Key::Home);
        assert_eq!(table.selected_index(), 0);
        table.handle_input(&Key::Up);
        assert_eq!(table.selected_index(), 0);
        table.handle_input(&Key::PageUp);
        assert_eq!(table.selected_index(), 0);
        table.handle_input(&Key::End);
        table.handle_input(&Key::Down);
        assert_eq!(table.selected_index(), 2);
    }

    #[test]
    fn selects_on_left_click() {
        let mut table = Table::new(columns(), rows());
        table.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 1,
            row: 2,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(table.selected_index(), 1);
    }

    #[test]
    fn fires_select_and_change_callbacks() {
        let selected = Arc::new(Mutex::new(Vec::new()));
        let changed = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&selected);
        let change_sink = Arc::clone(&changed);
        let mut table = Table::new(columns(), rows());
        table.set_on_select(move |index| sink.lock().unwrap().push(index));
        table.set_on_change(move |count| change_sink.lock().unwrap().push(count));
        table.handle_input(&Key::Down);
        table.set_table(columns(), vec![rows()[0].clone()]);
        assert_eq!(*selected.lock().unwrap(), vec![1]);
        assert_eq!(*changed.lock().unwrap(), vec![1]);
    }

    #[test]
    fn truncates_to_narrow_widths() {
        let mut table = Table::new(columns(), rows());
        table.set_theme(plain(false, false));
        for width in [1, 2, 5, 8, 12] {
            for row in table.render(width) {
                assert!(visible_width(&row) <= width as usize, "row too wide: {row:?}");
            }
        }
    }

    #[test]
    fn shrinks_columns_by_priority() {
        let mut table = Table::new(
            vec![
                TableColumn::new("A", 6).with_min_width(2).with_priority(1),
                TableColumn::new("B", 6).with_min_width(2).with_priority(0),
            ],
            vec![vec!["aaaaaaaaaa".to_string(), "bbbbbbbbbb".to_string()]],
        );
        table.set_theme(plain(false, false));
        let rendered = table.render(8);
        assert_eq!(rendered[0], "A     B ");
        assert_eq!(rendered[1], "aaaaa bb");
    }

    #[test]
    fn handles_empty_data() {
        let mut table = Table::new(Vec::new(), Vec::new());
        table.set_theme(plain(false, false));
        assert!(table.render(10).is_empty());
        assert!(table.render(0).is_empty());

        let mut header_only = Table::new(columns(), Vec::new());
        header_only.set_theme(plain(false, false));
        assert_eq!(header_only.render(19), vec!["Name   Age City    "]);
    }

    #[test]
    fn supports_custom_getters() {
        let mut table = Table::new(
            vec![
                TableColumn::new("Name", 6).with_getter(|row| row.get(0).cloned().unwrap_or_default()),
                TableColumn::new("Doubled", 8).with_getter(|row| {
                    row.get(1)
                        .and_then(|value| value.parse::<u32>().ok())
                        .map(|value| value * 2)
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                }),
            ],
            vec![vec!["Alice".to_string(), "21".to_string()]],
        );
        table.set_theme(plain(false, false));
        assert_eq!(table.render(15), vec!["Name   Doubled ", "Alice  42      "]);
    }
}
