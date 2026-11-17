//! Interface for custom editor components.
//!
//! Reference: `l/src/editor-component.ts`.
//!
//! Deviations forced by Rust:
//!
//! - The reference is a TypeScript `interface` whose optional members are
//!   properties. Rust traits cannot have fields, so optional callbacks are
//!   `set_*` methods with default no-op implementations.
//! - `handleInput(data)` takes raw terminal bytes upstream. The crate decodes
//!   input into [`crate::keys::Key`] first, so the raw counterpart is
//!   [`EditorComponent::handle_input_data`] and [`Component::handle_input`]
//!   stays the decoded entry point.

use crate::component::Component;
use crate::components::editor::AutocompleteProvider;

/// A pluggable editor implementation (vim mode, emacs mode, custom bindings)
/// that stays compatible with the core application.
pub trait EditorComponent: Component {
    // Core text access (required).

    /// Current text content.
    fn get_text(&self) -> String;

    /// Replace the text content.
    fn set_text(&mut self, text: &str);

    /// Handle raw terminal input (key presses, paste sequences, etc.).
    fn handle_input_data(&mut self, data: &str);

    // Callbacks (optional upstream, defaulted here).

    /// Called when the user submits (e.g. the Enter key).
    fn set_on_submit(&mut self, _callback: Option<Box<dyn FnMut(&str)>>) {}

    /// Programmatically trigger submission.
    fn submit(&mut self) {}

    /// Called when the text changes.
    fn set_on_change(&mut self, _callback: Option<Box<dyn FnMut(&str)>>) {}

    // History support.

    /// Add text to history for up/down navigation.
    fn add_to_history(&mut self, _text: &str) {}

    // Advanced text manipulation.

    /// Insert text at the current cursor position.
    fn insert_text_at_cursor(&mut self, _text: &str) {}

    /// Text with any markers expanded (e.g. paste markers); `None` falls back
    /// to [`EditorComponent::get_text`].
    fn get_expanded_text(&self) -> Option<String> {
        None
    }

    // Autocomplete support.

    /// Set the autocomplete provider.
    fn set_autocomplete_provider(&mut self, _provider: Box<dyn AutocompleteProvider>) {}

    // Appearance.

    /// Border color function.
    fn set_border_color(&mut self, _color: Box<dyn Fn(&str) -> String + Send + Sync>) {}

    /// Horizontal padding.
    fn set_padding_x(&mut self, _padding: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;

    struct Dummy {
        text: String,
        expanded: Option<String>,
        submitted: Option<String>,
        changed: Option<String>,
        history: Vec<String>,
        padding: Option<usize>,
    }

    impl Dummy {
        fn new() -> Self {
            Self {
                text: String::new(),
                expanded: None,
                submitted: None,
                changed: None,
                history: Vec::new(),
                padding: None,
            }
        }
    }

    impl Component for Dummy {
        fn render(&mut self, _width: u16) -> Vec<String> {
            vec![self.text.clone()]
        }

        fn handle_input(&mut self, _key: &Key) {}
    }

    impl EditorComponent for Dummy {
        fn get_text(&self) -> String {
            self.text.clone()
        }

        fn set_text(&mut self, text: &str) {
            self.text = text.to_string();
        }

        fn handle_input_data(&mut self, data: &str) {
            self.text.push_str(data);
        }

        fn set_on_submit(&mut self, mut callback: Option<Box<dyn FnMut(&str)>>) {
            if let Some(callback) = callback.as_mut() {
                callback(&self.text);
            }
            self.submitted = Some(self.text.clone());
        }

        fn submit(&mut self) {
            self.submitted = Some(self.text.clone());
        }

        fn set_on_change(&mut self, callback: Option<Box<dyn FnMut(&str)>>) {
            if let Some(mut callback) = callback {
                callback(&self.text);
            }
        }

        fn add_to_history(&mut self, text: &str) {
            self.history.push(text.to_string());
        }

        fn insert_text_at_cursor(&mut self, text: &str) {
            self.text.push_str(text);
        }

        fn get_expanded_text(&self) -> Option<String> {
            self.expanded.clone()
        }

        fn set_padding_x(&mut self, padding: usize) {
            self.padding = Some(padding);
        }
    }

    #[test]
    fn required_members_round_trip() {
        let mut dummy = Dummy::new();
        dummy.set_text("abc");
        assert_eq!(dummy.get_text(), "abc");
        dummy.handle_input_data("def");
        assert_eq!(dummy.get_text(), "abcdef");
        dummy.insert_text_at_cursor("!");
        assert_eq!(dummy.get_text(), "abcdef!");
    }

    struct Minimal;

    impl Component for Minimal {
        fn render(&mut self, _width: u16) -> Vec<String> {
            Vec::new()
        }
    }

    impl EditorComponent for Minimal {
        fn get_text(&self) -> String {
            String::new()
        }

        fn set_text(&mut self, _text: &str) {}

        fn handle_input_data(&mut self, _data: &str) {}
    }

    #[test]
    fn optional_members_default_to_noop() {
        let mut minimal = Minimal;
        minimal.set_on_submit(None);
        minimal.submit();
        minimal.set_on_change(None);
        minimal.set_autocomplete_provider(Box::new(NoopProvider));
        minimal.set_border_color(Box::new(|_| "red".into()));
        minimal.add_to_history("x");
        minimal.insert_text_at_cursor("y");
        assert_eq!(minimal.get_expanded_text(), None);
        minimal.set_padding_x(2);
    }

    #[test]
    fn expanded_text_overrides_when_present() {
        let mut dummy = Dummy::new();
        dummy.set_text("raw");
        assert_eq!(dummy.get_expanded_text(), None);
        dummy.expanded = Some("expanded".into());
        assert_eq!(dummy.get_expanded_text(), Some("expanded".into()));
    }

    #[test]
    fn dispatches_through_trait_object() {
        let mut editor: Box<dyn EditorComponent> = Box::new(Dummy::new());
        editor.set_text("hi");
        editor.set_padding_x(2);
        assert_eq!(editor.get_text(), "hi");
        assert_eq!(editor.render(10), vec!["hi".to_string()]);
    }

    struct NoopProvider;

    impl AutocompleteProvider for NoopProvider {
        fn complete(&self, _input: &str, _cursor: usize) -> Vec<String> {
            Vec::new()
        }
    }
}
