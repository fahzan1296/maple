//! Vertical stacking of children.
//! Reference: `l/src/components/layout/stack.ts`.

use crate::component::Component;

pub struct Stack {
    children: Vec<Box<dyn Component>>,
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

impl Stack {
    pub fn new() -> Self {
        Stack {
            children: Vec::new(),
        }
    }

    pub fn add_child<C: Component + 'static>(&mut self, child: C) -> &mut Self {
        self.children.push(Box::new(child));
        self
    }
}

impl Component for Stack {
    fn render(&mut self, width: u16) -> Vec<String> {
        let mut rows = Vec::new();
        for child in &mut self.children {
            rows.extend(child.render(width));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text;

    #[test]
    fn stacks_rows() {
        let mut stack = Stack::new();
        stack.add_child(Text::new("a")).add_child(Text::new("b"));
        assert_eq!(stack.render(4), vec!["a   ", "b   "]);
    }
}
