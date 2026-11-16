//! Collapsible titled section with a rule and child components.
//! Reference: `l/src/components/section.ts`.

use crate::component::Component;
use crate::keys::{is_enter, Key};
use crate::style::{Color, Style};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

/// Glyphs, styles, and spacing for a [`Section`].
#[derive(Clone, Debug, PartialEq)]
pub struct SectionTheme {
    /// Title text style.
    pub title: Style,
    /// Rule glyph style.
    pub rule: Style,
    /// Glyph shown when collapsed.
    pub collapsed_glyph: String,
    /// Glyph shown when expanded.
    pub expanded_glyph: String,
    /// Character repeated to fill the title bar.
    pub rule_glyph: char,
    /// Text before the glyph.
    pub indent: String,
    /// Append a blank row after the section.
    pub blank_after: bool,
}

impl Default for SectionTheme {
    fn default() -> Self {
        SectionTheme {
            title: Style::default().bold(),
            rule: Style::fg(Color::DarkGray),
            collapsed_glyph: "▸".to_string(),
            expanded_glyph: "▾".to_string(),
            rule_glyph: '─',
            indent: String::new(),
            blank_after: false,
        }
    }
}

/// A titled rule with a collapsible body of lines or child components.
pub struct Section {
    title: String,
    collapsed: bool,
    lines: Vec<String>,
    children: Vec<Box<dyn Component>>,
    theme: SectionTheme,
    on_toggle: Option<Box<dyn FnMut(bool) + 'static>>,
}

impl Default for Section {
    fn default() -> Self {
        Section {
            title: String::new(),
            collapsed: false,
            lines: Vec::new(),
            children: Vec::new(),
            theme: SectionTheme::default(),
            on_toggle: None,
        }
    }
}

impl Section {
    /// A section titled `title`, expanded, with no body.
    pub fn new(title: impl Into<String>) -> Self {
        Section {
            title: title.into(),
            ..Section::default()
        }
    }

    /// Replace the title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// The current title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Collapse or expand the body.
    pub fn set_collapsed(&mut self, collapsed: bool) {
        if self.collapsed == collapsed {
            return;
        }
        self.collapsed = collapsed;
        if let Some(callback) = self.on_toggle.as_mut() {
            callback(self.collapsed);
        }
    }

    /// Whether the body is hidden.
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// Toggle the body, firing `on_toggle`.
    pub fn toggle(&mut self) {
        self.set_collapsed(!self.collapsed);
    }

    /// Replace the body with plain lines, dropping child components.
    pub fn set_children(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.children.clear();
    }

    /// The plain body lines.
    pub fn children(&self) -> &[String] {
        &self.lines
    }

    /// Append a child component, replacing any plain body lines.
    pub fn add_child<C: Component + 'static>(&mut self, child: C) {
        self.lines.clear();
        self.children.push(Box::new(child));
    }

    /// Remove and return the child at `index`.
    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index >= self.children.len() {
            return None;
        }
        Some(self.children.remove(index))
    }

    /// Drop every child component.
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    /// The number of child components.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// The theme.
    pub fn theme(&self) -> &SectionTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: SectionTheme) {
        self.theme = theme;
    }

    /// Register a callback fired with the new collapsed state on toggle.
    pub fn set_on_toggle<F>(&mut self, callback: F)
    where
        F: FnMut(bool) + 'static,
    {
        self.on_toggle = Some(Box::new(callback));
    }

    /// The title bar plus rule, clipped to `width`.
    fn title_bar(&self, width: usize) -> String {
        let glyph = if self.collapsed {
            self.theme.collapsed_glyph.as_str()
        } else {
            self.theme.expanded_glyph.as_str()
        };
        let mut head = String::new();
        head.push_str(&single_line(&self.theme.indent));
        head.push_str(glyph);
        let title = single_line(&self.title);
        if !title.is_empty() {
            head.push(' ');
            head.push_str(&title);
        }
        let styled_head = self.theme.title.paint(&head);
        let rule_width = width.saturating_sub(visible_width(&styled_head));
        let rule: String = std::iter::repeat(self.theme.rule_glyph).take(rule_width).collect();
        let styled_rule = self.theme.rule.paint(&rule);
        truncate_to_width(
            &format!("{styled_head}{styled_rule}"),
            width,
            Ellipsis::Omit,
        )
    }
}

impl Component for Section {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 {
            return Vec::new();
        }
        let mut lines = vec![self.title_bar(width)];
        if !self.collapsed {
            if !self.children.is_empty() {
                for child in &mut self.children {
                    for line in child.render(width as u16) {
                        lines.push(truncate_to_width(
                            &single_line(&line),
                            width,
                            Ellipsis::Omit,
                        ));
                    }
                }
            } else {
                for line in &self.lines {
                    lines.push(truncate_to_width(
                        &single_line(line),
                        width,
                        Ellipsis::Omit,
                    ));
                }
            }
        }
        if self.theme.blank_after {
            lines.push(String::new());
        }
        lines
    }

    fn handle_input(&mut self, key: &Key) {
        if is_enter(key) || matches!(key, Key::Char(' ')) {
            self.toggle();
        }
    }

    fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }
    fn dispose(&mut self) {
        for child in &mut self.children {
            child.dispose();
        }
    }
}

/// Replace tabs and line breaks with spaces.
fn single_line(text: &str) -> String {
    text.replace('\t', " ").replace('\r', " ").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text;
    use std::sync::{Arc, Mutex};

    fn plain() -> SectionTheme {
        SectionTheme {
            title: Style::default(),
            rule: Style::default(),
            collapsed_glyph: "▸".to_string(),
            expanded_glyph: "▾".to_string(),
            rule_glyph: '─',
            indent: String::new(),
            blank_after: false,
        }
    }

    #[test]
    fn renders_title_bar_and_body() {
        let mut section = Section::new("Files");
        section.set_children(vec!["a".to_string(), "b".to_string()]);
        section.set_theme(plain());
        assert_eq!(section.render(12), vec!["▾ Files─────", "a", "b"]);
    }

    #[test]
    fn collapses_and_expands() {
        let mut section = Section::new("Files");
        section.set_children(vec!["a".to_string()]);
        section.set_theme(plain());
        section.set_collapsed(true);
        assert_eq!(section.render(12), vec!["▸ Files─────"]);
        section.set_collapsed(false);
        assert_eq!(section.render(12), vec!["▾ Files─────", "a"]);
    }

    #[test]
    fn toggles_with_enter_and_space() {
        let mut section = Section::new("Files");
        section.set_children(vec!["a".to_string()]);
        section.set_theme(plain());
        section.handle_input(&Key::Enter);
        assert!(section.collapsed());
        section.handle_input(&Key::Char(' '));
        assert!(!section.collapsed());
        section.handle_input(&Key::Tab);
        assert!(!section.collapsed());
    }

    #[test]
    fn fires_toggle_callback() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&states);
        let mut section = Section::new("Files");
        section.set_on_toggle(move |collapsed| sink.lock().unwrap().push(collapsed));
        section.handle_input(&Key::Enter);
        section.handle_input(&Key::Enter);
        assert_eq!(*states.lock().unwrap(), vec![true, false]);
    }

    #[test]
    fn accepts_child_components() {
        let mut section = Section::new("T");
        section.add_child(Text::new("hello"));
        section.set_theme(plain());
        assert_eq!(section.child_count(), 1);
        assert_eq!(section.render(8), vec!["▾ T─────", "hello   "]);
    }

    #[test]
    fn forwards_invalidate_and_dispose() {
        let mut section = Section::new("T");
        section.add_child(Text::new("hello"));
        section.set_theme(plain());
        section.invalidate();
        section.dispose();
        assert_eq!(section.render(8), vec!["▾ T─────", "hello   "]);
    }

    #[test]
    fn truncates_children_to_width() {
        let mut section = Section::new("Long Title");
        section.set_children(vec!["abcdefghijklmnop".to_string()]);
        section.set_theme(plain());
        for width in [1, 4, 8, 12] {
            for line in section.render(width) {
                assert!(visible_width(&line) <= width as usize, "row too wide: {line:?}");
            }
        }
    }

    #[test]
    fn handles_empty_state() {
        let mut section = Section::new("");
        section.set_theme(plain());
        assert_eq!(section.render(6), vec!["▾─────"]);
        assert!(section.render(0).is_empty());
    }

    #[test]
    fn supports_blank_line_after() {
        let mut section = Section::new("T");
        section.set_children(vec!["a".to_string()]);
        let mut theme = plain();
        theme.blank_after = true;
        section.set_theme(theme);
        assert_eq!(section.render(6), vec!["▾ T───", "a", ""]);
    }
}
