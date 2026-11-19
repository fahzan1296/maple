//! Dim transcript marker for tool calls stripped from the resolved branch
//! (failed/retried turns, results on sibling branches).
//!
//! Reference: `l/src/chat/stripped-tool-calls-placeholder.ts`.
//!
//! It is tool activity, so it hides and reappears with the
//! `display.hideToolActivity` toggle.
//!
//! Deviations: the reference extends `Text` and caches its native node; here the
//! marker owns a [`crate::components::Text`] and rebuilds its node per describe
//! (the describe contract is `&self`, and the node is one dim text run).

use crate::components::Text as TextComponent;
use crate::component::Component;
use crate::native::describe::{span, text, with_hidden, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, Props, Text};
use crate::theme::Theme;

/// A dim italic note counting the tool calls a branch dropped.
pub struct StrippedToolCallsPlaceholder {
    label: String,
    text: TextComponent,
    tool_activity_visible: bool,
}

impl StrippedToolCallsPlaceholder {
    /// A marker for `stripped_tool_calls` calls.
    pub fn new(stripped_tool_calls: usize, tool_activity_visible: bool, theme: &Theme) -> Self {
        let plural = if stripped_tool_calls == 1 { "" } else { "s" };
        let label =
            format!("{stripped_tool_calls} tool call{plural} elided — no result on this branch");
        StrippedToolCallsPlaceholder {
            text: TextComponent::padded(theme.fg("dim", &theme.italic(&label)).unwrap_or_default(), 1, 0),
            label,
            tool_activity_visible,
        }
    }

    /// The label the marker draws.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Show or hide with tool activity.
    pub fn set_tool_activity_visible(&mut self, visible: bool) {
        self.tool_activity_visible = visible;
    }

    /// Whether the marker is currently hidden.
    pub fn is_tool_activity_visible(&self) -> bool {
        self.tool_activity_visible
    }
}

impl Component for StrippedToolCallsPlaceholder {
    fn render(&mut self, width: u16) -> Vec<String> {
        if !self.tool_activity_visible {
            return Vec::new();
        }
        self.text.render(width)
    }

    fn invalidate(&mut self) {
        self.text.invalidate();
    }
}

impl NativeDescribe for StrippedToolCallsPlaceholder {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let mut props = Props::new();
        props.set_str("wrap", "word");
        props.set_str("role", "omp.tool.elided");
        with_hidden(
            text(Text::spans(vec![span(&self.label, Some("dim em"))]), props),
            !self.tool_activity_visible,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin;
    use crate::theme::color::ColorMode;
    use crate::theme::{create_theme, CreateThemeOptions};

    fn theme() -> Theme {
        create_theme(
            builtin::builtin_theme_json("dark").expect("builtin theme"),
            CreateThemeOptions {
                mode: Some(ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .expect("theme resolves")
    }

    #[test]
    fn the_label_counts_singular_and_plural_calls() {
        let theme = theme();
        let cases = [(1, "1 tool call elided — no result on this branch"), (3, "3 tool calls elided — no result on this branch")];
        for (count, expected) in cases {
            let marker = StrippedToolCallsPlaceholder::new(count, true, &theme);
            assert_eq!(marker.label(), expected);
        }
    }

    #[test]
    fn hidden_markers_draw_nothing_and_describe_a_hidden_node() {
        let theme = theme();
        let mut marker = StrippedToolCallsPlaceholder::new(2, true, &theme);
        assert_eq!(marker.render(60).len(), 1);
        let node = marker.describe_native(&DescribeContext::new(80));
        assert_eq!(node.props.hidden(), None);
        assert_eq!(node.props.role(), Some("omp.tool.elided"));

        marker.set_tool_activity_visible(false);
        assert!(marker.render(60).is_empty());
        assert_eq!(
            marker.describe_native(&DescribeContext::new(80)).props.hidden(),
            Some(true)
        );
    }
}
