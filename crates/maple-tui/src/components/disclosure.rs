//! Collapsible title/body section with a caret and toggle callbacks.
//! Reference: `l/src/components/disclosure.ts`.

use crate::component::Component;
use crate::keys::{is_enter, Key};
use crate::mouse::{MouseButtonKind, MouseKind, MouseEvent};
use crate::style::Style;
use crate::text_shape::{pad_to_width, truncate_to_width, Ellipsis};


/// Caret, color, and style policy for a [`Disclosure`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisclosureTheme {
    /// Caret shown while collapsed.
    pub collapsed_caret: String,
    /// Caret shown while expanded.
    pub expanded_caret: String,
    /// Style applied to the title row.
    pub title: Style,
    /// Style applied to the body rows.
    pub body: Style,
}

impl Default for DisclosureTheme {
    fn default() -> Self {
        DisclosureTheme {
            collapsed_caret: "▶".to_string(),
            expanded_caret: "▼".to_string(),
            title: Style::default().bold(),
            body: Style::default(),
        }
    }
}

/// A controlled summary/detail section.
pub struct Disclosure {
    title: String,
    body: Vec<String>,
    open: bool,
    padding_x: usize,
    theme: DisclosureTheme,
    on_toggle: Option<Box<dyn FnMut(bool) + 'static>>,
}

impl Default for Disclosure {
    fn default() -> Self {
        Disclosure::new(String::new())
    }
}

impl Disclosure {
    /// Create a collapsed disclosure with a title and no body.
    pub fn new(title: impl Into<String>) -> Self {
        Disclosure {
            title: title.into(),
            body: Vec::new(),
            open: false,
            padding_x: 0,
            theme: DisclosureTheme::default(),
            on_toggle: None,
        }
    }

    /// Replace the title text.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// Current title text.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Replace the body from a single text block.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.body = text.into().split('\n').map(str::to_string).collect();
    }

    /// Replace the body rows.
    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.body = lines;
    }

    /// Current body rows.
    pub fn lines(&self) -> &[String] {
        &self.body
    }

    /// Show or hide the body, firing `on_toggle` when the state changes.
    pub fn set_open(&mut self, open: bool) {
        if self.open == open {
            return;
        }
        self.open = open;
        if let Some(callback) = self.on_toggle.as_mut() {
            callback(open);
        }
    }

    /// Current expansion state.
    pub fn open(&self) -> bool {
        self.open
    }

    /// Flip the expansion state and return the new value.
    pub fn toggle(&mut self) -> bool {
        self.set_open(!self.open);
        self.open
    }

    /// Horizontal padding applied outside the title and body.
    pub fn set_padding_x(&mut self, padding_x: usize) {
        self.padding_x = padding_x;
    }

    /// Replace the caret and style policy.
    pub fn set_theme(&mut self, theme: DisclosureTheme) {
        self.theme = theme;
    }

    /// Current theme.
    pub fn theme(&self) -> &DisclosureTheme {
        &self.theme
    }

    /// Register a callback fired with the new state after every toggle.
    pub fn set_on_toggle<F: FnMut(bool) + 'static>(&mut self, callback: F) {
        self.on_toggle = Some(Box::new(callback));
    }
}

impl Component for Disclosure {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if width == 0 {
            return Vec::new();
        }
        let padding_x = self.padding_x.min(width.saturating_sub(1) / 2);
        let inner_width = width.saturating_sub(2 * padding_x);
        let pad = " ".repeat(padding_x);

        let caret = if self.open {
            self.theme.expanded_caret.as_str()
        } else {
            self.theme.collapsed_caret.as_str()
        };
        let title_row = format!("{caret} {}", self.title);
        let clipped = truncate_to_width(&title_row, inner_width, Ellipsis::Omit);
        let padded = pad_to_width(&clipped, inner_width);
        let mut rows = vec![format!(
            "{pad}{}{pad}",
            self.theme.title.paint(&padded)
        )];

        if self.open {
            for line in &self.body {
                let clipped = truncate_to_width(line, inner_width, Ellipsis::Omit);
                let padded = pad_to_width(&clipped, inner_width);
                rows.push(format!("{pad}{}{pad}", self.theme.body.paint(&padded)));
            }
        }
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        if is_enter(key) || matches!(key, Key::Char(' ') | Key::Shift(' ')) {
            self.toggle();
        } else if matches!(key, Key::Left) {
            self.set_open(false);
        } else if matches!(key, Key::Right) {
            self.set_open(true);
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if event.row == 0 && event.kind == MouseKind::Down(MouseButtonKind::Left) {
            self.toggle();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn plain_theme() -> DisclosureTheme {
        DisclosureTheme {
            title: Style::default(),
            body: Style::default(),
            ..DisclosureTheme::default()
        }
    }

    fn click(row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 0,
            row,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    #[test]
    fn collapsed_shows_title_with_collapsed_caret() {
        let mut disclosure = Disclosure::new("Title");
        disclosure.set_theme(plain_theme());
        disclosure.set_text("body");
        assert!(!disclosure.open());
        assert_eq!(disclosure.render(8), vec!["▶ Title "]);
    }

    #[test]
    fn expanded_shows_body_rows() {
        let mut disclosure = Disclosure::new("Title");
        disclosure.set_theme(plain_theme());
        disclosure.set_text("hello");
        disclosure.set_open(true);
        assert_eq!(disclosure.render(8), vec!["▼ Title ", "hello   "]);
    }

    #[test]
    fn padding_is_applied_symmetrically() {
        let mut disclosure = Disclosure::new("T");
        disclosure.set_theme(plain_theme());
        disclosure.set_padding_x(1);
        assert_eq!(disclosure.render(7), vec![" ▶ T   "]);
    }

    #[test]
    fn toggle_flips_state_and_fires_callback() {
        let mut disclosure = Disclosure::new("Title");
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        disclosure.set_on_toggle(move |open| sink.borrow_mut().push(open));
        assert!(disclosure.toggle());
        assert!(!disclosure.toggle());
        assert_eq!(*seen.borrow(), vec![true, false]);
    }

    #[test]
    fn set_open_fires_only_on_change() {
        let mut disclosure = Disclosure::new("Title");
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        disclosure.set_on_toggle(move |open| sink.borrow_mut().push(open));
        disclosure.set_open(true);
        disclosure.set_open(true);
        disclosure.set_open(false);
        assert_eq!(*seen.borrow(), vec![true, false]);
    }

    #[test]
    fn keyboard_controls_expansion() {
        let mut disclosure = Disclosure::new("Title");
        disclosure.handle_input(&Key::Enter);
        assert!(disclosure.open());
        disclosure.handle_input(&Key::Char(' '));
        assert!(!disclosure.open());
        disclosure.handle_input(&Key::Right);
        assert!(disclosure.open());
        disclosure.handle_input(&Key::Left);
        assert!(!disclosure.open());
    }

    #[test]
    fn mouse_click_on_title_toggles() {
        let mut disclosure = Disclosure::new("Title");
        disclosure.handle_mouse(&click(0));
        assert!(disclosure.open());
        disclosure.handle_mouse(&click(1));
        assert!(disclosure.open());
        disclosure.handle_mouse(&click(0));
        assert!(!disclosure.open());
    }

    #[test]
    fn zero_width_and_empty_content_are_safe() {
        let mut disclosure = Disclosure::new("");
        disclosure.set_theme(plain_theme());
        assert_eq!(disclosure.render(1), vec!["▶"]);
        assert!(disclosure.render(0).is_empty());
        disclosure.set_open(true);
        assert_eq!(disclosure.render(1), vec!["▼"]);
    }

    #[test]
    fn long_title_is_truncated_to_width() {
        let mut disclosure = Disclosure::new("a very long title");
        disclosure.set_theme(plain_theme());
        let rows = disclosure.render(6);
        assert_eq!(rows, vec!["▶ a ve".to_string()]);
        assert!(crate::text_shape::visible_width(&rows[0]) <= 6);
    }
}
