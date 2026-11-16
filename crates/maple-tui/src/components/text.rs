//! Multi-line padded text.
//! Reference: `l/src/components/text.ts`.

use crate::component::Component;
use crate::style::Style;
use crate::text_shape::{pad_to_width, truncate_to_width, wrap_text_with_ansi, Ellipsis};

/// Word-wrapping (or truncating) text block with padding and optional style.
pub struct Text {
    text: String,
    padding_x: u16,
    padding_y: u16,
    wrap: bool,
    style: Option<Style>,
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Text {
            text: text.into(),
            padding_x: 0,
            padding_y: 0,
            wrap: true,
            style: None,
        }
    }

    pub fn padded(text: impl Into<String>, padding_x: u16, padding_y: u16) -> Self {
        Text {
            padding_x,
            padding_y,
            ..Text::new(text)
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_padding(&mut self, x: u16, y: u16) {
        self.padding_x = x;
        self.padding_y = y;
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = Some(style);
    }

    pub fn clear_style(&mut self) {
        self.style = None;
    }

    fn styled(&self, line: &str, width: u16) -> String {
        let padded = pad_to_width(line, width as usize);
        match self.style {
            Some(style) => style.paint(&padded),
            None => padded,
        }
    }
}

impl Component for Text {
    fn render(&mut self, width: u16) -> Vec<String> {
        let padding_x = self.padding_x.min(width / 2);
        let content_width = width.saturating_sub(2 * padding_x).max(1) as usize;
        let mut body: Vec<String> = Vec::new();
        for paragraph in self.text.split('\n') {
            if self.wrap {
                body.extend(wrap_text_with_ansi(paragraph, content_width));
            } else {
                body.push(truncate_to_width(
                    paragraph,
                    content_width,
                    Ellipsis::Omit,
                ));
            }
        }
        let mut rows = Vec::new();
        for _ in 0..self.padding_y {
            rows.push(self.styled("", width));
        }
        for line in body {
            let mut padded = String::new();
            padded.extend(std::iter::repeat_n(' ', padding_x as usize));
            padded.push_str(&line);
            padded.extend(std::iter::repeat_n(' ', padding_x as usize));
            rows.push(self.styled(&padded, width));
        }
        for _ in 0..self.padding_y {
            rows.push(self.styled("", width));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_pads() {
        let mut text = Text::padded("hello world", 1, 0);
        assert_eq!(text.render(9), vec![" hello   ", " world   "]);
        assert_eq!(
            text.render(5),
            vec![" hel ", " lo  ", " wor ", " ld  "]
        );
    }

    #[test]
    fn honors_vertical_padding() {
        let mut text = Text::padded("a", 0, 2);
        assert_eq!(
            text.render(3),
            vec!["   ", "   ", "a  ", "   ", "   "]
        );
    }
}
