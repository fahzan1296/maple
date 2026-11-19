//! Horizontal composition of equally weighted children.
//! Reference: `l/src/components/layout/row.ts`.

use crate::component::Component;
use crate::components::layout::geometry::distribute_width;

pub struct Row {
    children: Vec<Box<dyn Component>>,
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

impl Row {
    pub fn new() -> Self {
        Row {
            children: Vec::new(),
        }
    }

    pub fn add_child<C: Component + 'static>(&mut self, child: C) -> &mut Self {
        self.children.push(Box::new(child));
        self
    }
}

impl Component for Row {
    fn render(&mut self, width: u16) -> Vec<String> {
        if self.children.is_empty() {
            return Vec::new();
        }
        let widths = distribute_width(width, self.children.len());
        let mut columns = Vec::new();
        let mut tallest = 0usize;
        for (child, column_width) in self.children.iter_mut().zip(widths) {
            let rows = child.render(column_width);
            tallest = tallest.max(rows.len());
            columns.push(rows);
        }
        let mut out = Vec::with_capacity(tallest);
        for line in 0..tallest {
            let mut row = String::new();
            for column in &columns {
                row.push_str(column.get(line).map(String::as_str).unwrap_or(""));
            }
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
    fn places_children_side_by_side() {
        let mut row = Row::new();
        row.add_child(Text::new("aa")).add_child(Text::new("b"));
        assert_eq!(row.render(5), vec!["aab  "]);
    }
}
