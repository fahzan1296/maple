//! Multi-line text editor with wrapping, scrolling, and autocomplete.
//! Reference: `l/src/components/editor.ts`.
//!
//! Bounded clean-room port of the upstream `Editor`: plain buffer editing,
//! word motion matching [`crate::components::input`], single-level undo, and a
//! fake cursor. IME composition, host-side edits, native cursor placement, and
//! vim mode are intentionally out of scope.

use crate::component::Component;
use crate::keys::Key;
use crate::style::{Color, Style};
use crate::symbols::{LineSymbols, SymbolTheme};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};

/// Marker emitted immediately before the cursor cell, matching the upstream
/// `CURSOR_MARKER` (`"\x1b_pi:c\x07"`). `component.rs` does not define the
/// constant yet, so the editor keeps a local copy; the integrator should hoist
/// it next to [`Component`] and delete this one.
pub const CURSOR_MARKER: &str = "\x1b_pi:c\x07";

/// Completion source used by [`Editor::set_autocomplete_provider`].
pub trait AutocompleteProvider {
    /// Candidate replacements for the word ending at byte `cursor` of `input`.
    ///
    /// `input` is the logical line the cursor is on and `cursor` is a byte
    /// offset into it. Returning an empty vector makes `Tab` fall back to
    /// inserting a tab.
    fn complete(&self, input: &str, cursor: usize) -> Vec<String>;
}

/// Colors and glyphs for an [`Editor`].
#[derive(Clone)]
pub struct EditorTheme {
    /// Border glyph style.
    pub border: Style,
    /// Body text style.
    pub text: Style,
    /// Ghost hint style.
    pub hint: Style,
    /// Placeholder style.
    pub placeholder: Style,
    /// Cursor cell style.
    pub cursor: Style,
    /// Border glyphs.
    pub symbols: LineSymbols,
    /// Horizontal padding inside the border.
    pub padding_x: u16,
    /// Vertical padding around the bordered block.
    pub padding_y: u16,
}

impl Default for EditorTheme {
    fn default() -> Self {
        EditorTheme {
            border: Style::fg(Color::DarkGray),
            text: Style::default(),
            hint: Style::fg(Color::DarkGray).dim(),
            placeholder: Style::fg(Color::DarkGray).dim(),
            cursor: Style::default().reverse(),
            symbols: SymbolTheme::unicode().box_round,
            padding_x: 1,
            padding_y: 0,
        }
    }
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<String>,
    line: usize,
    col: usize,
}

struct VisualLine {
    line: usize,
    start: usize,
    text: String,
}

enum BorderRow {
    Top,
    Bottom,
}

/// Multi-line editor state and rendering.
pub struct Editor {
    lines: Vec<String>,
    line: usize,
    col: usize,
    scroll: usize,
    preferred_col: Option<usize>,
    focused: bool,
    read_only: bool,
    placeholder: String,
    hint: String,
    autocomplete: Option<Box<dyn AutocompleteProvider>>,
    disabled_submit: bool,
    undo: Option<Snapshot>,
    max_height: Option<usize>,
    theme: EditorTheme,
    on_submit: Option<Box<dyn FnMut(&str)>>,
    on_change: Option<Box<dyn FnMut(&str)>>,
    on_escape: Option<Box<dyn FnMut(&str)>>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

fn floor_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn prev_char_boundary(text: &str, index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    floor_boundary(text, index - 1)
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut index = index + 1;
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn split_at_char(text: &str, index: usize) -> (&str, &str) {
    if index == 0 {
        return ("", text);
    }
    let mut boundary = text.len();
    for (char_index, (byte_index, _)) in text.char_indices().enumerate() {
        if char_index == index {
            boundary = byte_index;
            break;
        }
    }
    (&text[..boundary], &text[boundary..])
}

fn char_index_to_byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(text.len())
}

fn split_lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

/// Word motion shared with `input.rs`: skip whitespace, then skip the word.
fn word_left_bound(text: &str, mut cursor: usize) -> usize {
    while cursor > 0 {
        let previous = prev_char_boundary(text, cursor);
        let whitespace = text[previous..cursor]
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(false);
        if !whitespace {
            break;
        }
        cursor = previous;
    }
    while cursor > 0 {
        let previous = prev_char_boundary(text, cursor);
        let whitespace = text[previous..cursor]
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(false);
        if whitespace {
            break;
        }
        cursor = previous;
    }
    cursor
}

/// Word motion shared with `input.rs`: skip the word, then skip whitespace.
fn word_right_bound(text: &str, mut cursor: usize) -> usize {
    while cursor < text.len() {
        let next = next_char_boundary(text, cursor);
        let whitespace = text[cursor..next]
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(false);
        if whitespace {
            break;
        }
        cursor = next;
    }
    while cursor < text.len() {
        let next = next_char_boundary(text, cursor);
        let whitespace = text[cursor..next]
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(false);
        if !whitespace {
            break;
        }
        cursor = next;
    }
    cursor
}

fn word_start(text: &str, cursor: usize) -> usize {
    let mut start = cursor;
    while start > 0 {
        let previous = prev_char_boundary(text, start);
        let whitespace = text[previous..start]
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(true);
        if whitespace {
            break;
        }
        start = previous;
    }
    start
}

impl Editor {
    /// Create an empty, focused editor with default chrome.
    pub fn new() -> Self {
        Editor {
            lines: vec![String::new()],
            line: 0,
            col: 0,
            scroll: 0,
            preferred_col: None,
            focused: true,
            read_only: false,
            placeholder: String::new(),
            hint: String::new(),
            autocomplete: None,
            disabled_submit: false,
            undo: None,
            max_height: None,
            theme: EditorTheme::default(),
            on_submit: None,
            on_change: None,
            on_escape: None,
        }
    }

    /// Replace the whole buffer and move the cursor to the end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.lines = split_lines(&text.into());
        self.line = self.lines.len().saturating_sub(1);
        self.col = self.lines.get(self.line).map(String::len).unwrap_or(0);
        self.scroll = 0;
        self.preferred_col = None;
        self.undo = None;
        self.emit_change();
    }

    /// The buffer as a single string joined with `\n`.
    pub fn get_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Whether editing keys are ignored.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    /// Whether editing keys are ignored.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// Text painted on the empty cursor row.
    pub fn set_placeholder(&mut self, text: impl Into<String>) {
        self.placeholder = text.into();
    }

    /// The placeholder text.
    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// Override the border glyph color.
    pub fn set_border_color(&mut self, color: Color) {
        self.theme.border = Style::fg(color);
    }

    /// Ghost text painted after the cursor at the end of a line.
    pub fn set_hint_text(&mut self, text: impl Into<String>) {
        self.hint = text.into();
    }

    /// The ghost hint text.
    pub fn hint_text(&self) -> &str {
        &self.hint
    }

    /// Install the completion source used by `Tab`.
    pub fn set_autocomplete_provider(&mut self, provider: Box<dyn AutocompleteProvider>) {
        self.autocomplete = Some(provider);
    }

    /// Fire `callback` with the trimmed buffer when `Enter` submits.
    pub fn on_submit(&mut self, callback: impl FnMut(&str) + 'static) {
        self.on_submit = Some(Box::new(callback));
    }

    /// Fire `callback` with the buffer after every edit.
    pub fn on_change(&mut self, callback: impl FnMut(&str) + 'static) {
        self.on_change = Some(Box::new(callback));
    }

    /// Fire `callback` with the buffer when `Escape` is pressed.
    pub fn on_escape(&mut self, callback: impl FnMut(&str) + 'static) {
        self.on_escape = Some(Box::new(callback));
    }

    /// Enable or disable `Enter` submission.
    pub fn disable_submit(&mut self, disabled: bool) {
        self.disabled_submit = disabled;
    }

    /// Whether `Enter` submission is disabled.
    pub fn submit_disabled(&self) -> bool {
        self.disabled_submit
    }

    /// Set focus; focused editors emit [`CURSOR_MARKER`].
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Whether the editor is focused.
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Cap the number of visible content rows; `None` renders everything.
    pub fn set_max_height(&mut self, height: Option<usize>) {
        self.max_height = height;
        self.scroll = 0;
    }

    /// The visible content-row cap.
    pub fn max_height(&self) -> Option<usize> {
        self.max_height
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: EditorTheme) {
        self.theme = theme;
    }

    /// The active theme.
    pub fn theme(&self) -> &EditorTheme {
        &self.theme
    }

    /// Logical line index of the cursor.
    pub fn cursor_row(&self) -> usize {
        self.line
    }

    /// Character column of the cursor within its logical line.
    pub fn cursor_column(&self) -> usize {
        self.char_column()
    }

    fn char_column(&self) -> usize {
        let line = match self.lines.get(self.line) {
            Some(line) => line,
            None => return 0,
        };
        let col = floor_boundary(line, self.col);
        line[..col].chars().count()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            line: self.line,
            col: self.col,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.lines = snapshot.lines;
        self.line = snapshot.line;
        self.col = snapshot.col;
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.line >= self.lines.len() {
            self.line = self.lines.len() - 1;
        }
        let len = self.lines[self.line].len();
        self.col = floor_boundary(&self.lines[self.line], self.col.min(len));
    }

    fn abs_offset(&self) -> usize {
        let mut offset = self.col;
        for line in &self.lines[..self.line] {
            offset += line.len() + 1;
        }
        offset
    }

    fn locate(&self, abs: usize) -> (usize, usize) {
        let mut remaining = abs;
        for (index, line) in self.lines.iter().enumerate() {
            if remaining <= line.len() {
                return (index, remaining);
            }
            remaining -= line.len() + 1;
        }
        let last = self.lines.len().saturating_sub(1);
        let col = self.lines.get(last).map(String::len).unwrap_or(0);
        (last, col)
    }

    fn apply_text(&mut self, text: &str) {
        let mut all = self.get_text();
        let abs = floor_boundary(&all, self.abs_offset());
        all.insert_str(abs, text);
        let new_offset = abs + text.len();
        self.lines = split_lines(&all);
        let (line, col) = self.locate(new_offset);
        self.line = line;
        self.col = col;
    }

    fn emit_change(&mut self) {
        let text = self.get_text();
        if let Some(callback) = self.on_change.as_mut() {
            callback(&text);
        }
    }

    fn record_undo(&mut self) {
        self.undo = Some(self.snapshot());
    }

    fn insert_text(&mut self, text: &str) {
        if self.read_only || text.is_empty() {
            return;
        }
        self.record_undo();
        self.apply_text(text);
        self.emit_change();
    }

    fn backspace(&mut self) {
        if self.read_only {
            return;
        }
        let all = self.get_text();
        let abs = floor_boundary(&all, self.abs_offset());
        if abs == 0 {
            return;
        }
        self.record_undo();
        let start = prev_char_boundary(&all, abs);
        let mut next = all;
        next.replace_range(start..abs, "");
        let (line, col) = self.locate(start);
        self.lines = split_lines(&next);
        self.line = line;
        self.col = col;
        self.emit_change();
    }

    fn delete_forward(&mut self) {
        if self.read_only {
            return;
        }
        let all = self.get_text();
        let abs = floor_boundary(&all, self.abs_offset());
        if abs >= all.len() {
            return;
        }
        self.record_undo();
        let end = next_char_boundary(&all, abs);
        let mut next = all;
        next.replace_range(abs..end, "");
        let (line, col) = self.locate(abs);
        self.lines = split_lines(&next);
        self.line = line;
        self.col = col;
        self.emit_change();
    }

    fn undo(&mut self) {
        if self.read_only {
            return;
        }
        if let Some(snapshot) = self.undo.take() {
            self.restore(snapshot);
            self.preferred_col = None;
            self.emit_change();
        }
    }

    fn move_left(&mut self) {
        let line = match self.lines.get(self.line) {
            Some(line) => line.clone(),
            None => return,
        };
        if self.col > 0 {
            self.col = prev_char_boundary(&line, floor_boundary(&line, self.col));
        } else if self.line > 0 {
            self.line -= 1;
            self.col = self.lines.get(self.line).map(String::len).unwrap_or(0);
        }
        self.preferred_col = None;
    }

    fn move_right(&mut self) {
        let line = match self.lines.get(self.line) {
            Some(line) => line.clone(),
            None => return,
        };
        if self.col < line.len() {
            self.col = next_char_boundary(&line, floor_boundary(&line, self.col));
        } else if self.line + 1 < self.lines.len() {
            self.line += 1;
            self.col = 0;
        }
        self.preferred_col = None;
    }

    fn move_up(&mut self) {
        if self.line == 0 {
            return;
        }
        let target = self.preferred_col.unwrap_or_else(|| self.char_column());
        self.line -= 1;
        self.clamp_preferred_column(target);
    }

    fn move_down(&mut self) {
        if self.line + 1 >= self.lines.len() {
            return;
        }
        let target = self.preferred_col.unwrap_or_else(|| self.char_column());
        self.line += 1;
        self.clamp_preferred_column(target);
    }

    fn clamp_preferred_column(&mut self, target: usize) {
        let line = match self.lines.get(self.line) {
            Some(line) => line,
            None => return,
        };
        self.col = char_index_to_byte(line, target);
        self.preferred_col = Some(target);
    }

    fn move_home(&mut self) {
        self.col = 0;
        self.preferred_col = None;
    }

    fn move_end(&mut self) {
        self.col = self.lines.get(self.line).map(String::len).unwrap_or(0);
        self.preferred_col = None;
    }

    fn move_word_left(&mut self) {
        let line = match self.lines.get(self.line) {
            Some(line) => line.clone(),
            None => return,
        };
        if self.col == 0 {
            if self.line > 0 {
                self.line -= 1;
                self.col = self.lines.get(self.line).map(String::len).unwrap_or(0);
            }
        } else {
            self.col = word_left_bound(&line, floor_boundary(&line, self.col));
        }
        self.preferred_col = None;
    }

    fn move_word_right(&mut self) {
        let line = match self.lines.get(self.line) {
            Some(line) => line.clone(),
            None => return,
        };
        if self.col >= line.len() {
            if self.line + 1 < self.lines.len() {
                self.line += 1;
                self.col = 0;
            }
        } else {
            self.col = word_right_bound(&line, floor_boundary(&line, self.col));
        }
        self.preferred_col = None;
    }

    fn submit(&mut self) {
        if self.disabled_submit {
            return;
        }
        let trimmed = self.get_text().trim().to_string();
        self.lines = vec![String::new()];
        self.line = 0;
        self.col = 0;
        self.scroll = 0;
        self.preferred_col = None;
        self.undo = None;
        self.emit_change();
        if let Some(callback) = self.on_submit.as_mut() {
            callback(&trimmed);
        }
    }

    fn handle_tab(&mut self) {
        if self.read_only {
            return;
        }
        let line = match self.lines.get(self.line) {
            Some(line) => line.clone(),
            None => return,
        };
        let col = floor_boundary(&line, self.col);
        let candidate = self
            .autocomplete
            .as_ref()
            .and_then(|provider| provider.complete(&line, col).into_iter().next());
        if let Some(candidate) = candidate {
            let start = word_start(&line, col);
            let all = self.get_text();
            let abs = floor_boundary(&all, self.abs_offset());
            let abs_start = abs.saturating_sub(col - start);
            if abs_start <= abs && abs <= all.len() {
                self.record_undo();
                let mut next = all;
                next.replace_range(abs_start..abs, &candidate);
                let new_offset = abs_start + candidate.len();
                self.lines = split_lines(&next);
                let (line_index, col_index) = self.locate(new_offset);
                self.line = line_index;
                self.col = col_index;
                self.emit_change();
                return;
            }
        }
        self.insert_text("\t");
    }

    fn handle_key(&mut self, key: &Key) {
        match key {
            Key::Char(character) | Key::Shift(character) => {
                self.insert_text(character.to_string().as_str());
            }
            Key::Paste(text) => self.insert_text(text),
            Key::Enter => self.submit(),
            Key::Backspace | Key::Ctrl('h') => self.backspace(),
            Key::Delete => self.delete_forward(),
            Key::Left => self.move_left(),
            Key::Right => self.move_right(),
            Key::Up => self.move_up(),
            Key::Down => self.move_down(),
            Key::Home | Key::Ctrl('a') => self.move_home(),
            Key::End | Key::Ctrl('e') => self.move_end(),
            Key::CtrlLeft => self.move_word_left(),
            Key::CtrlRight => self.move_word_right(),
            Key::Tab => self.handle_tab(),
            Key::Esc => {
                if self.on_escape.is_some() {
                    let text = self.get_text();
                    if let Some(callback) = self.on_escape.as_mut() {
                        callback(&text);
                    }
                }
            }
            Key::Shift('\r') | Key::Ctrl('\r') | Key::Alt('\r') => self.insert_text("\n"),
            Key::Ctrl('z') => self.undo(),
            _ => {}
        }
    }

    fn cursor_content(&self, item: &VisualLine, cursor_col: usize) -> String {
        let offset = cursor_col.saturating_sub(item.start);
        let (before, after) = split_at_char(&item.text, offset);
        let (cell, rest) = if after.is_empty() {
            (self.theme.cursor.paint(" "), "")
        } else {
            let first_len = after.chars().next().map(char::len_utf8).unwrap_or(0);
            (self.theme.cursor.paint(&after[..first_len]), &after[first_len..])
        };
        let mut out = String::new();
        out.push_str(before);
        if self.focused {
            out.push_str(CURSOR_MARKER);
        }
        out.push_str(&cell);
        out.push_str(rest);
        out
    }

    fn border_row(&self, kind: BorderRow, inner: usize) -> String {
        let symbols = &self.theme.symbols;
        let (left, right) = match kind {
            BorderRow::Top => (&symbols.top_left, &symbols.top_right),
            BorderRow::Bottom => (&symbols.bottom_left, &symbols.bottom_right),
        };
        let mut row = String::new();
        row.push_str(&self.theme.border.paint(left));
        row.push_str(&self.theme.border.paint(&symbols.horizontal.repeat(inner)));
        row.push_str(&self.theme.border.paint(right));
        row
    }
}

impl Component for Editor {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width.max(1) as usize;
        let bordered = width > 2;
        let inner = if bordered { width - 2 } else { width };
        let padding_x = (self.theme.padding_x as usize).min(inner.saturating_sub(1) / 2);
        let content_width = inner.saturating_sub(2 * padding_x).max(1);
        let padding_y = self.theme.padding_y as usize;

        let mut visual: Vec<VisualLine> = Vec::new();
        for (line_index, line) in self.lines.iter().enumerate() {
            let chunks = wrap_text_with_ansi(line, content_width);
            let mut start = 0usize;
            for chunk in chunks {
                let char_len = chunk.chars().count();
                visual.push(VisualLine {
                    line: line_index,
                    start,
                    text: chunk,
                });
                start += char_len;
            }
        }
        if visual.is_empty() {
            visual.push(VisualLine {
                line: 0,
                start: 0,
                text: String::new(),
            });
        }

        let cursor_line = self.line.min(self.lines.len().saturating_sub(1));
        let cursor_col = self.char_column();
        let mut cursor_visual = 0usize;
        for (index, item) in visual.iter().enumerate() {
            if item.line == cursor_line {
                cursor_visual = index;
                let end = item.start + item.text.chars().count();
                if cursor_col <= end {
                    break;
                }
            }
        }

        let visible_height = self.max_height.unwrap_or(visual.len()).max(1);
        if cursor_visual < self.scroll {
            self.scroll = cursor_visual;
        } else if cursor_visual >= self.scroll + visible_height {
            self.scroll = cursor_visual + 1 - visible_height;
        }
        if self.scroll + visible_height > visual.len() {
            self.scroll = visual.len().saturating_sub(visible_height);
        }

        let end_index = (self.scroll + visible_height).min(visual.len());
        let empty_text = self.get_text().is_empty();
        let mut rows: Vec<String> = Vec::new();
        for _ in 0..padding_y {
            rows.push(pad_to_width("", width));
        }
        if bordered {
            rows.push(self.border_row(BorderRow::Top, inner));
        }
        for (offset, item) in visual[self.scroll..end_index].iter().enumerate() {
            let is_cursor = self.scroll + offset == cursor_visual;
            let mut content = if is_cursor {
                self.cursor_content(item, cursor_col)
            } else {
                item.text.clone()
            };

            if is_cursor && empty_text && !self.placeholder.is_empty() {
                let budget = content_width.saturating_sub(1).max(1);
                let placeholder = truncate_to_width(&self.placeholder, budget, Ellipsis::Unicode);
                content = String::new();
                if self.focused {
                    content.push_str(CURSOR_MARKER);
                }
                content.push_str(&self.theme.cursor.paint(" "));
                content.push_str(&self.theme.placeholder.paint(&placeholder));
            } else if is_cursor && !self.hint.is_empty() && !empty_text {
                let line_len = self
                    .lines
                    .get(cursor_line)
                    .map(|line| line.chars().count())
                    .unwrap_or(0);
                if cursor_col >= line_len {
                    let used = visible_width(&content);
                    let available = content_width.saturating_sub(used);
                    let hint = truncate_to_width(&self.hint, available, Ellipsis::Unicode);
                    content.push_str(&self.theme.hint.paint(&hint));
                }
            }

            content = truncate_to_width(&content, content_width, Ellipsis::Omit);
            content = pad_to_width(&content, content_width);

            let mut row = String::new();
            if bordered {
                row.push_str(&self.theme.border.paint(&self.theme.symbols.vertical));
            }
            row.push_str(&" ".repeat(padding_x));
            row.push_str(&self.theme.text.paint(&content));
            row.push_str(&" ".repeat(padding_x));
            if bordered {
                row.push_str(&self.theme.border.paint(&self.theme.symbols.vertical));
            }
            rows.push(row);
        }
        if bordered {
            rows.push(self.border_row(BorderRow::Bottom, inner));
        }
        for _ in 0..padding_y {
            rows.push(pad_to_width("", width));
        }
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        self.handle_key(key);
    }

    fn handle_paste(&mut self, text: &str) {
        self.insert_text(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn inserts_and_deletes_characters() {
        let mut editor = Editor::new();
        editor.set_text("abc");
        assert_eq!(editor.get_text(), "abc");
        assert_eq!(editor.cursor_row(), 0);
        assert_eq!(editor.cursor_column(), 3);
        editor.handle_input(&Key::Left);
        assert_eq!(editor.cursor_column(), 2);
        editor.handle_input(&Key::Backspace);
        assert_eq!(editor.get_text(), "ac");
        editor.handle_input(&Key::Delete);
        assert_eq!(editor.get_text(), "a");
        editor.handle_input(&Key::End);
        editor.handle_input(&Key::Char('!'));
        assert_eq!(editor.get_text(), "a!");
    }

    #[test]
    fn inserts_newlines_and_moves_between_lines() {
        let mut editor = Editor::new();
        editor.handle_input(&Key::Char('a'));
        editor.handle_input(&Key::Alt('\r'));
        editor.handle_input(&Key::Char('b'));
        assert_eq!(editor.get_text(), "a\nb");
        assert_eq!(editor.cursor_row(), 1);
        assert_eq!(editor.cursor_column(), 1);
        editor.handle_input(&Key::Up);
        assert_eq!(editor.cursor_row(), 0);
        editor.handle_input(&Key::Backspace);
        assert_eq!(editor.get_text(), "\nb");
    }

    #[test]
    fn undo_restores_previous_state() {
        let mut editor = Editor::new();
        editor.handle_input(&Key::Char('a'));
        editor.handle_input(&Key::Char('b'));
        editor.handle_input(&Key::Ctrl('z'));
        assert_eq!(editor.get_text(), "a");
        // Single-level undo: a second Ctrl+Z has no further state to restore.
        editor.handle_input(&Key::Ctrl('z'));
        assert_eq!(editor.get_text(), "a");
        editor.handle_input(&Key::Char('c'));
        editor.handle_input(&Key::Ctrl('z'));
        assert_eq!(editor.get_text(), "a");
    }

    #[test]
    fn word_motion_matches_input_semantics() {
        let mut editor = Editor::new();
        editor.set_text("hello world");
        editor.handle_input(&Key::Home);
        editor.handle_input(&Key::CtrlRight);
        assert_eq!(editor.cursor_column(), 6);
        editor.handle_input(&Key::CtrlRight);
        assert_eq!(editor.cursor_column(), 11);
        editor.handle_input(&Key::CtrlLeft);
        assert_eq!(editor.cursor_column(), 6);
        editor.handle_input(&Key::CtrlLeft);
        assert_eq!(editor.cursor_column(), 0);
    }

    #[test]
    fn renders_cursor_marker_and_wraps_long_lines() {
        let mut editor = Editor::new();
        editor.set_text("abcdefghijklmnop");
        let rows = editor.render(10);
        assert!(rows.iter().all(|row| visible_width(row) <= 10));
        assert!(rows.iter().any(|row| row.contains("abcdef")));
        assert!(rows.iter().any(|row| row.contains("ghijkl")));
        assert!(rows.iter().any(|row| row.contains("mnop")));

        editor.set_text("abc");
        editor.handle_input(&Key::Left);
        let rows = editor.render(14);
        let row = rows
            .iter()
            .find(|row| row.contains(CURSOR_MARKER))
            .expect("cursor marker");
        let marker = row.find(CURSOR_MARKER).expect("marker index");
        assert!(row[..marker].contains("ab"), "row was {row:?}");
        assert!(row.contains('c'));
    }

    #[test]
    fn scrolls_to_keep_cursor_visible() {
        let mut editor = Editor::new();
        editor.set_text("a\nb\nc\nd");
        editor.set_max_height(Some(2));
        editor.handle_input(&Key::End);
        let rows = editor.render(12);
        assert_eq!(rows.len(), 4);
        assert!(!rows.iter().any(|row| row.contains(" a ")));
        assert!(rows.iter().any(|row| row.contains("d")));
    }

    #[test]
    fn autocomplete_accepts_first_candidate() {
        struct Words;
        impl AutocompleteProvider for Words {
            fn complete(&self, _input: &str, _cursor: usize) -> Vec<String> {
                vec!["hello.rs".to_string()]
            }
        }
        let mut editor = Editor::new();
        editor.set_text("he");
        editor.set_autocomplete_provider(Box::new(Words));
        editor.handle_input(&Key::Tab);
        assert_eq!(editor.get_text(), "hello.rs");
    }

    #[test]
    fn tab_inserts_when_no_completion() {
        let mut editor = Editor::new();
        editor.handle_input(&Key::Char('a'));
        editor.handle_input(&Key::Tab);
        assert_eq!(editor.get_text(), "a\t");
    }

    #[test]
    fn submit_and_disable_submit() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let mut editor = Editor::new();
        editor.on_submit(move |text| sink.borrow_mut().push(text.to_string()));
        editor.set_text("  hi  ");
        editor.handle_input(&Key::Enter);
        assert_eq!(seen.borrow().as_slice(), &["hi".to_string()][..]);
        assert_eq!(editor.get_text(), "");

        editor.set_text("again");
        editor.disable_submit(true);
        editor.handle_input(&Key::Enter);
        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(editor.get_text(), "again");
    }

    #[test]
    fn read_only_blocks_edits_but_not_movement() {
        let mut editor = Editor::new();
        editor.set_text("abc");
        editor.set_read_only(true);
        editor.handle_input(&Key::Char('x'));
        editor.handle_input(&Key::Backspace);
        editor.handle_paste("y");
        assert_eq!(editor.get_text(), "abc");
        editor.handle_input(&Key::Left);
        assert_eq!(editor.cursor_column(), 2);
    }

    #[test]
    fn placeholder_and_narrow_widths_never_panic() {
        let mut editor = Editor::new();
        editor.set_placeholder("Search");
        let rows = editor.render(12);
        assert!(rows.iter().any(|row| row.contains("Search")));
        for width in [0u16, 1, 2, 3, 5] {
            let rows = editor.render(width);
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|row| visible_width(row) <= width.max(1).into()));
        }
    }

    #[test]
    fn hint_is_painted_after_cursor_at_line_end() {
        let mut editor = Editor::new();
        editor.set_text("he");
        editor.set_hint_text("llo");
        let rows = editor.render(20);
        assert!(rows.iter().any(|row| row.contains("llo")));
    }

    #[test]
    fn paste_inserts_multiline_text() {
        let mut editor = Editor::new();
        editor.handle_paste("x\ny");
        assert_eq!(editor.get_text(), "x\ny");
        assert_eq!(editor.cursor_row(), 1);
        assert_eq!(editor.cursor_column(), 1);
    }
}
