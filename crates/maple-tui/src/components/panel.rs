//! Padded container with optional full-width background style.

use crate::component::Component;
use crate::style::Style;
use crate::text_shape::{pad_to_width, truncate_to_width, Ellipsis};

pub struct Panel {
    padding_x: u16,
    padding_y: u16,
    style: Option<Style>,
    children: Vec<Box<dyn Component>>,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            padding_x: 0,
            padding_y: 0,
            style: None,
            children: Vec::new(),
        }
    }
}


impl Panel {
    pub fn new(padding_x: u16, padding_y: u16) -> Self {
        Panel {
            padding_x,
            padding_y,
            style: None,
            children: Vec::new(),
        }
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    pub fn add_child<C: Component + 'static>(&mut self, child: C) -> &mut Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index >= self.children.len() {
            return None;
        }
        Some(self.children.remove(index))
    }

    pub fn set_padding(&mut self, x: u16, y: u16) {
        self.padding_x = x;
        self.padding_y = y;
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = Some(style);
    }

    pub fn clear_style(&mut self) {
        self.style = None;
    }

    fn paint_row(&self, line: &str, width: u16) -> String {
        let clipped = truncate_to_width(line, width as usize, Ellipsis::Omit);
        match self.style {
            Some(style) => style.paint(&pad_to_width(&clipped, width as usize)),
            None => clipped,
        }
    }
}

impl Component for Panel {
    fn render(&mut self, width: u16) -> Vec<String> {
        let padding_x = self.padding_x.min(width / 2);
        let inner_width = width.saturating_sub(2 * padding_x).max(1);
        let mut body = Vec::new();
        for child in &mut self.children {
            body.extend(child.render(inner_width));
        }
        let mut rows = Vec::new();
        for _ in 0..self.padding_y {
            rows.push(self.paint_row("", width));
        }
        for line in body {
            let mut padded = String::new();
            padded.extend(std::iter::repeat_n(' ', padding_x as usize));
            padded.push_str(&line);
            padded.extend(std::iter::repeat_n(' ', padding_x as usize));
            rows.push(self.paint_row(&padded, width));
        }
        for _ in 0..self.padding_y {
            rows.push(self.paint_row("", width));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text;
    use crate::style::Color;

    #[test]
    fn pads_and_paints_background() {
        let mut panel = Panel::new(1, 1).with_style(Style::bg(Color::Blue));
        panel.add_child(Text::new("x"));
        let rows = panel.render(3);
        assert_eq!(rows.len(), 3);
        assert!(rows[0].starts_with("\x1b[44m   \x1b[0m"));
        assert!(rows[1].contains(" x "));
        assert_eq!(rows[2], rows[0]);
    }

    #[test]
    fn clips_child_output_to_panel_width() {
        let mut panel = Panel::new(0, 0);
        let mut child = Text::new("abcdef");
        child.set_wrap(false);
        panel.add_child(child);
        assert_eq!(panel.render(3), vec!["abc"]);
    }
}
