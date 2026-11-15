//! Single-line text input with horizontal scrolling.

use crate::component::Component;
use crate::keys::Key;
use crate::text_shape::{truncate_to_width, Ellipsis};

pub struct Input {
    value: String,
    cursor: usize,
    scroll: usize,
    placeholder: String,
    password: bool,
    disabled_submit: bool,
    max_length: Option<usize>,
    on_submit: Option<Box<dyn FnMut(&str)>>,
    on_change: Option<Box<dyn FnMut(&str)>>,
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

impl Input {
    pub fn new() -> Self {
        Input {
            value: String::new(),
            cursor: 0,
            scroll: 0,
            placeholder: String::new(),
            password: false,
            disabled_submit: false,
            max_length: None,
            on_submit: None,
            on_change: None,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        self.cursor = value.len();
        self.scroll = 0;
        self.value = value;
        self.emit_change();
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn set_placeholder(&mut self, text: impl Into<String>) {
        self.placeholder = text.into();
    }

    pub fn set_password(&mut self, enabled: bool) {
        self.password = enabled;
    }

    pub fn set_disabled_submit(&mut self, disabled: bool) {
        self.disabled_submit = disabled;
    }

    pub fn set_max_length(&mut self, limit: Option<usize>) {
        self.max_length = limit;
    }

    pub fn on_submit(&mut self, callback: impl FnMut(&str) + 'static) {
        self.on_submit = Some(Box::new(callback));
    }

    pub fn on_change(&mut self, callback: impl FnMut(&str) + 'static) {
        self.on_change = Some(Box::new(callback));
    }

    /// Column of the cursor within the rendered viewport.
    pub fn cursor_viewport_column(&self, width: u16) -> usize {
        let cursor_column = self.char_index(self.cursor);
        cursor_column.saturating_sub(self.scroll).min(width as usize)
    }

    fn char_index(&self, byte_index: usize) -> usize {
        self.value[..byte_index.min(self.value.len())].chars().count()
    }


    fn emit_change(&mut self) {
        if let Some(callback) = self.on_change.as_mut() {
            callback(&self.value);
        }
    }

    fn insert(&mut self, text: &str) {
        if self.max_length.is_some_and(|limit| self.value.chars().count() >= limit) {
            return;
        }
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.emit_change();
    }

    fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.value.replace_range(previous..self.cursor, "");
        self.cursor = previous;
        self.emit_change();
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let next = self.value[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(offset, _)| self.cursor + offset)
            .unwrap_or(self.value.len());
        self.value.replace_range(self.cursor..next, "");
        self.emit_change();
    }

    fn move_left(&mut self) {
        if self.cursor > 0 {
            let previous = self.value[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(index, _)| index)
                .unwrap_or(0);
            self.cursor = previous;
        }
    }

    fn move_right(&mut self) {
        if self.cursor < self.value.len() {
            let next = self.value[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| self.cursor + offset)
                .unwrap_or(self.value.len());
            self.cursor = next;
        }
    }

    fn move_word_left(&mut self) {
        while self.cursor > 0 {
            let previous = self.value[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(index, _)| index)
                .unwrap_or(0);
            if !self.value[previous..self.cursor]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = previous;
        }
        while self.cursor > 0 {
            let previous = self.value[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(index, _)| index)
                .unwrap_or(0);
            if self.value[previous..self.cursor]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = previous;
        }
    }

    fn move_word_right(&mut self) {
        while self.cursor < self.value.len() {
            let next = self.value[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| self.cursor + offset)
                .unwrap_or(self.value.len());
            if self.value[self.cursor..next]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = next;
        }
        while self.cursor < self.value.len() {
            let next = self.value[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| self.cursor + offset)
                .unwrap_or(self.value.len());
            if !self.value[self.cursor..next]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                break;
            }
            self.cursor = next;
        }
    }

    fn delete_word_backward(&mut self) {
        let target = {
            let before = &self.value[..self.cursor];
            let mut end = before.trim_end().len();
            while end > 0 {
                let previous = self.value[..end]
                    .char_indices()
                    .next_back()
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                if self.value[previous..end].chars().next().is_some_and(char::is_whitespace) {
                    end = previous;
                } else {
                    break;
                }
            }
            end
        };
        if target != self.cursor {
            self.value.replace_range(target..self.cursor, "");
            self.cursor = target;
            self.emit_change();
        }
    }

    fn submit(&mut self) {
        if self.disabled_submit {
            return;
        }
        if let Some(callback) = self.on_submit.as_mut() {
            callback(&self.value);
        }
    }

    fn handle_key(&mut self, key: &Key) {
        match key {
            Key::Char(character) | Key::Shift(character) => self.insert(&character.to_string()),
            Key::Enter => self.submit(),
            Key::Backspace | Key::Ctrl('h') => self.delete_backward(),
            Key::Delete => self.delete_forward(),
            Key::Left => self.move_left(),
            Key::Right => self.move_right(),
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = self.value.len(),
            Key::Ctrl('a') => self.cursor = 0,
            Key::Ctrl('e') => self.cursor = self.value.len(),
            Key::Ctrl('u') => {
                self.value.replace_range(..self.cursor, "");
                self.cursor = 0;
                self.emit_change();
            }
            Key::Ctrl('k') => {
                self.value.truncate(self.cursor);
                self.emit_change();
            }
            Key::Ctrl('w') | Key::Alt('\x7f') => self.delete_word_backward(),
            Key::CtrlLeft => self.move_word_left(),
            Key::CtrlRight => self.move_word_right(),
            _ => {}
        }
    }

    fn normalize_scroll(&self, width: u16) -> usize {
        let inner = width as usize;
        let cursor_column = self.char_index(self.cursor);
        if inner == 0 {
            return 0;
        }
        if cursor_column < self.scroll {
            cursor_column
        } else if cursor_column >= self.scroll + inner {
            cursor_column + 1 - inner
        } else {
            self.scroll
        }
    }
}

impl Component for Input {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width.max(1);
        let inner = width as usize;
        self.scroll = self.normalize_scroll(width);
        if self.value.is_empty() {
            let placeholder = truncate_to_width(&self.placeholder, inner, Ellipsis::Unicode);
            return vec![placeholder];
        }
        let visible: String = if self.password {
            self.value.chars().map(|_| '*').collect()
        } else {
            self.value.clone()
        };
        let row: String = visible
            .chars()
            .skip(self.scroll)
            .take(inner)
            .collect();
        vec![row]
    }

    fn handle_input(&mut self, key: &Key) {
        self.handle_key(key);
    }

    fn handle_paste(&mut self, text: &str) {
        self.insert(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_and_deletes_characters() {
        let mut input = Input::new();
        input.handle_input(&Key::Char('a'));
        input.handle_input(&Key::Char('b'));
        assert_eq!(input.value(), "ab");
        input.handle_input(&Key::Backspace);
        assert_eq!(input.value(), "a");
        input.handle_input(&Key::Home);
        input.handle_input(&Key::Delete);
        assert_eq!(input.value(), "");
    }

    #[test]
    fn moves_by_character_and_word() {
        let mut input = Input::new();
        input.set_value("one two");
        input.handle_input(&Key::CtrlLeft);
        assert_eq!(input.cursor, 4);
        input.handle_input(&Key::CtrlLeft);
        assert_eq!(input.cursor, 0);
        input.handle_input(&Key::CtrlRight);
        assert_eq!(input.cursor, 4);
        input.handle_input(&Key::CtrlRight);
        assert_eq!(input.cursor, 7);
    }

    #[test]
    fn submits_only_when_enabled() {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&submitted);
        let mut input = Input::new();
        input.set_value("hello");
        input.on_submit(move |value| {
            sink.lock().unwrap().push(value.to_string());
        });
        input.handle_input(&Key::Enter);
        let captured = submitted.lock().unwrap();
        assert_eq!(captured.as_slice(), ["hello".to_string()]);
    }

    #[test]
    fn renders_scrolled_window() {
        let mut input = Input::new();
        input.set_value("abcdefghij");
        input.cursor = 0;
        let row = input.render(4).remove(0);
        assert_eq!(row, "abcd");
    }
}
