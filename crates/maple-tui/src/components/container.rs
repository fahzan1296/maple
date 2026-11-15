//! Plain vertical composition of children.

use crate::component::Component;
use crate::style::Style;
use crate::text_shape::pad_to_width;

pub struct Container {
    children: Vec<Box<dyn Component>>,
    style: Option<Style>,
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Container {
    pub fn new() -> Self {
        Container {
            children: Vec::new(),
            style: None,
        }
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

    pub fn children(&self) -> &[Box<dyn Component>] {
        &self.children
    }

    pub fn set_style(&mut self, style: Style) {
        self.style = Some(style);
    }

    pub fn clear_style(&mut self) {
        self.style = None;
    }
}

impl Component for Container {
    fn render(&mut self, width: u16) -> Vec<String> {
        let mut rows = Vec::new();
        for child in &mut self.children {
            rows.extend(child.render(width));
        }
        if let Some(style) = self.style {
            for row in &mut rows {
                *row = style.paint(&pad_to_width(row, width as usize));
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text;

    #[test]
    fn concatenates_children() {
        let mut container = Container::new();
        container.add_child(Text::new("a")).add_child(Text::new("b"));
        assert_eq!(container.render(4), vec!["a   ", "b   "]);
    }
}
