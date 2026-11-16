//! Vertical whitespace.
//! Reference: `l/src/components/spacer.ts`.

use crate::component::Component;

/// Emits `lines` blank rows.
pub struct Spacer {
    lines: usize,
}

impl Spacer {
    pub fn new(lines: usize) -> Self {
        Spacer { lines }
    }

    pub fn set_lines(&mut self, lines: usize) {
        self.lines = lines;
    }

    pub fn lines(&self) -> usize {
        self.lines
    }
}

impl Component for Spacer {
    fn render(&mut self, _width: u16) -> Vec<String> {
        vec![String::new(); self.lines]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_requested_blank_lines() {
        let mut spacer = Spacer::new(3);
        assert_eq!(spacer.render(80), vec!["", "", ""]);
        spacer.set_lines(1);
        assert_eq!(spacer.render(80), vec![""]);
    }
}
