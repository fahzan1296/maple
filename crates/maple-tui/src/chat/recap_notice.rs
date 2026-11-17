//! The idle recap notice row.
//!
//! Reference: `l/src/chat/recap-notice.ts`.
//!
//! The idle recap (`recap.*`) says where the session stands, written while the
//! user was away. It stays in the transcript like a thought: a dim italic
//! `※ recap:` line in ANSI, a history icon beside the italic thought-coloured
//! text in a native terminal (`omp.recap`).
//!
//! Deviation: the crate has no global theme, so the constructor takes `&Theme`
//! and bakes the dim-italic styling into the text once.

use crate::components::container::Container;
use crate::components::spacer::Spacer;
use crate::components::text::Text;
use crate::component::Component;
use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, Props, Text as NodeText, Tone};
use crate::theme::theme_class::Theme;

/// The idle recap line.
pub struct RecapNotice {
    recap: String,
    container: Container,
}

impl RecapNotice {
    /// Build the notice for `recap`.
    pub fn new(recap: impl Into<String>, theme: &Theme) -> Self {
        let recap = recap.into();
        let line = format!("※ recap: {recap}");
        let styled = theme
            .fg("dim", &theme.italic(&line))
            .unwrap_or_else(|_| line.clone());
        let mut container = Container::new();
        container.add_child(Spacer::new(1));
        container.add_child(Text::padded(styled, 1, 0));
        RecapNotice { recap, container }
    }

    /// The recap text.
    pub fn recap(&self) -> &str {
        &self.recap
    }

    /// The native transcript description.
    pub fn describe(&self) -> NativeNode {
        let mut row_props = Props::new();
        row_props.set_str("gap", "sm");
        row_props.set_str("align", "start");
        row_props.set_str("role", "omp.recap");
        row_props.set_str("title", "Recap while you were away");
        let mut icon_props = Props::new();
        icon_props.set_str("name", "history");
        let icon = node(NodeKind::Icon, icon_props, Vec::new(), None);
        let mut body_props = Props::new();
        body_props.set_str("wrap", "word");
        body_props.set_tone(Tone::Muted);
        let body = crate::native::describe::text(NodeText::Plain(self.recap.clone()), body_props);
        node(NodeKind::Row, row_props, vec![icon, body], None)
    }
}

impl Component for RecapNotice {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.container.render(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin::builtin_theme_json;
    use crate::theme::loader::{create_theme, CreateThemeOptions};
    use crate::theme::theme_class::ColorMode;

    fn theme() -> Theme {
        create_theme(
            builtin_theme_json("dark").unwrap(),
            CreateThemeOptions {
                mode: Some(ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn renders_a_blank_row_then_the_dim_recap_line() {
        let theme = theme();
        let mut notice = RecapNotice::new("two files changed", &theme);
        let rows = notice.render(80);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_empty());
        let visible = crate::ansi_scan::strip_ansi(&rows[1]);
        assert_eq!(visible, " ※ recap: two files changed");
        assert!(rows[1].contains("\u{1b}["));
        assert_eq!(notice.recap(), "two files changed");
    }

    #[test]
    fn describe_marks_the_hook_role_and_history_icon() {
        let theme = theme();
        let notice = RecapNotice::new("state", &theme);
        let described = notice.describe();
        assert_eq!(described.props.get_str("role"), Some("omp.recap"));
        assert_eq!(
            described.props.get_str("title"),
            Some("Recap while you were away")
        );
        let children = described.children.as_slice();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].kind, NodeKind::Icon);
        assert_eq!(children[0].props.get_str("name"), Some("history"));
        assert_eq!(children[1].kind, NodeKind::Text);
    }
}
