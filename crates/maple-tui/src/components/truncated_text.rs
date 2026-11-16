//! Single-line text clipped to the viewport.
//! Reference: `l/src/components/truncated-text.ts`.

use crate::component::Component;
use crate::text_shape::{pad_to_width, truncate_to_width, Ellipsis};

pub struct TruncatedText {
    text: String,
    padding_x: u16,
    padding_y: u16,
    ellipsis: Ellipsis,
}

impl TruncatedText {
    pub fn new(text: impl Into<String>) -> Self {
        TruncatedText {
            text: text.into(),
            padding_x: 0,
            padding_y: 0,
            ellipsis: Ellipsis::default(),
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

    pub fn set_ellipsis(&mut self, ellipsis: Ellipsis) {
        self.ellipsis = ellipsis;
    }
}

impl Component for TruncatedText {
    fn render(&mut self, width: u16) -> Vec<String> {
        let padding_x = self.padding_x.min(width / 2);
        let content_width = width.saturating_sub(2 * padding_x).max(1) as usize;
        let clipped = truncate_to_width(&self.text, content_width, self.ellipsis);
        let mut padded = String::new();
        padded.extend(std::iter::repeat_n(' ', padding_x as usize));
        padded.push_str(&clipped);
        padded.extend(std::iter::repeat_n(' ', padding_x as usize));
        let row = pad_to_width(&padded, width as usize);
        let mut rows = vec![String::new(); self.padding_y as usize];
        rows.push(row);
        rows.extend(vec![String::new(); self.padding_y as usize]);
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_long_text() {
        let mut text = TruncatedText::new("abcdef");
        assert_eq!(text.render(4), vec!["abc…"]);
    }

    #[test]
    fn keeps_short_text_untouched() {
        let mut text = TruncatedText::new("ab");
        assert_eq!(text.render(4), vec!["ab  "]);
    }
}
