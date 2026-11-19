//! Two-pane horizontal split with a fixed left column.
//! Reference: `l/src/components/layout/split-pane.ts`.

use crate::component::Component;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
}

pub struct SplitPane {
    left: Option<Box<dyn Component>>,
    right: Option<Box<dyn Component>>,
    left_width: u16,
    right_min_width: u16,
    narrow_pane: Pane,
}

impl Default for SplitPane {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitPane {
    pub fn new() -> Self {
        SplitPane {
            left: None,
            right: None,
            left_width: 24,
            right_min_width: 0,
            narrow_pane: Pane::Right,
        }
    }

    pub fn with_children(
        left: Box<dyn Component>,
        right: Box<dyn Component>,
        left_width: u16,
    ) -> Self {
        SplitPane {
            left: Some(left),
            right: Some(right),
            left_width,
            ..SplitPane::new()
        }
    }

    pub fn set_left_width(&mut self, width: u16) {
        self.left_width = width;
    }

    pub fn set_right_min_width(&mut self, width: u16) {
        self.right_min_width = width;
    }

    pub fn set_narrow_pane(&mut self, pane: Pane) {
        self.narrow_pane = pane;
    }

    pub fn set_height(&mut self, _height: u16) {
        // Height is applied by the caller; reserved for viewport integration.
    }

    fn columns(&self, width: u16) -> (u16, u16) {
        let right = width.saturating_sub(self.left_width);
        if right >= self.right_min_width || self.right_min_width == 0 {
            (self.left_width.min(width), right)
        } else if self.narrow_pane == Pane::Right {
            (0, width)
        } else {
            (width, 0)
        }
    }
}

impl Component for SplitPane {
    fn render(&mut self, width: u16) -> Vec<String> {
        let (left_width, right_width) = self.columns(width);
        let left_rows = match (&mut self.left, left_width) {
            (Some(child), width) if width > 0 => child.render(width),
            _ => Vec::new(),
        };
        let right_rows = match (&mut self.right, right_width) {
            (Some(child), width) if width > 0 => child.render(width),
            _ => Vec::new(),
        };
        let height = left_rows.len().max(right_rows.len());
        let mut out = Vec::with_capacity(height);
        for index in 0..height {
            let mut row = String::new();
            row.push_str(left_rows.get(index).map(String::as_str).unwrap_or_default());
            row.push_str(right_rows.get(index).map(String::as_str).unwrap_or_default());
            out.push(row);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text;

    #[test]
    fn splits_at_fixed_left_width() {
        let mut pane = SplitPane::with_children(
            Box::new(Text::new("L")),
            Box::new(Text::new("R")),
            3,
        );
        assert_eq!(pane.render(8), vec!["L  R    "]);
    }

    #[test]
    fn collapses_to_narrow_pane_when_cramped() {
        let mut pane = SplitPane::with_children(
            Box::new(Text::new("L")),
            Box::new(Text::new("R")),
            3,
        );
        pane.set_right_min_width(10);
        assert_eq!(pane.render(6), vec!["R     "]);
    }
}
