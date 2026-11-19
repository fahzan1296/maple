//! Component for a custom message entry from hooks.
//!
//! Reference: `l/src/chat/hook-message.ts`.
//!
//! Uses distinct styling to differentiate from user messages.
//!
//! Deviations: the crate has no `FramedMessageComponent`, so this is a small
//! standalone component over [`Message`] — it keeps the five-line default fold
//! and the extension renderer hook, and draws the collapsed body as plain rows.
//! The theme is passed in per call instead of read globally.

use crate::chat::extension_types::{HookMessageRenderOptions, HookMessageRenderer};
use crate::chat::messages::{CustomMessageContent, HookMessage};
use crate::chrome::message::Message;
use crate::component::Component;
use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, Props, Text as NodeText};
use crate::theme::theme_class::Theme;

/// Lines of default markdown body shown before the "…" fold when collapsed.
const HOOK_COLLAPSED_LINES: usize = 5;

/// The transcript row for a hook message.
pub struct HookMessageComponent {
    message: HookMessage,
    custom_renderer: Option<HookMessageRenderer>,
    expanded: bool,
    body: Message,
    lines: Vec<String>,
}

impl HookMessageComponent {
    /// Build the row for `message`, deferring to `custom_renderer` when given.
    pub fn new(message: HookMessage, custom_renderer: Option<HookMessageRenderer>) -> Self {
        let mut body = Message::new(crate::chrome::message::MessageRole::Info);
        let lines = message_body_lines(&message);
        body.set_text(lines.join("\n"));
        body.set_copy_text(Some(lines.join("\n")));
        HookMessageComponent {
            message,
            custom_renderer,
            expanded: false,
            body,
            lines,
        }
    }

    /// The hook message.
    pub fn message(&self) -> &HookMessage {
        &self.message
    }

    /// Whether the body is expanded.
    pub fn expanded(&self) -> bool {
        self.expanded
    }

    /// Set the expanded state.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    /// Ask the extension renderer for a replacement component.
    pub fn render_custom(&self, theme: &Theme) -> Option<Box<dyn Component>> {
        let renderer = self.custom_renderer.as_ref()?;
        renderer(
            &self.message,
            HookMessageRenderOptions {
                expanded: self.expanded,
            },
            theme,
        )
    }

    /// The native transcript description.
    pub fn describe(&self) -> NativeNode {
        let mut props = Props::new();
        props.set_str("role", "omp.hook");
        props.set_str("title", "Hook message");
        let mut body_props = Props::new();
        body_props.set_str("wrap", "word");
        let body = crate::native::describe::text(NodeText::Plain(self.body_text()), body_props);
        node(NodeKind::Col, props, vec![body], None)
    }

    fn body_text(&self) -> String {
        if self.expanded || self.lines.len() <= HOOK_COLLAPSED_LINES {
            return self.lines.join("\n");
        }
        let mut shown: Vec<String> = self.lines[..HOOK_COLLAPSED_LINES].to_vec();
        shown.push("…".to_string());
        shown.join("\n")
    }
}

/// The message body as rows.
fn message_body_lines(message: &HookMessage) -> Vec<String> {
    match &message.content {
        CustomMessageContent::Text(text) => text.lines().map(str::to_string).collect(),
        CustomMessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                crate::chat::messages::MessageBlock::Text(text) => Some(text.text.clone()),
                crate::chat::messages::MessageBlock::Image(_) => None,
            })
            .collect(),
    }
}

impl Component for HookMessageComponent {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.body.set_text(self.body_text());
        self.body.render(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::messages::{CustomMessageContent, MessageAttribution};

    fn hook(content: &str) -> HookMessage {
        HookMessage {
            custom_type: "pre-tool".into(),
            content: CustomMessageContent::Text(content.into()),
            display: true,
            details: None,
            attribution: MessageAttribution::Agent,
            timestamp: 0,
        }
    }

    #[test]
    fn short_bodies_render_whole() {
        let mut component = HookMessageComponent::new(hook("one\ntwo"), None);
        let rows = component.render(40);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], "one");
        assert_eq!(component.describe().props.get_str("role"), Some("omp.hook"));
    }

    #[test]
    fn long_bodies_fold_after_five_lines() {
        let body = (0..8).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let mut component = HookMessageComponent::new(hook(&body), None);
        let rows = component.render(40);
        assert_eq!(rows.len(), HOOK_COLLAPSED_LINES + 1);
        assert_eq!(rows[HOOK_COLLAPSED_LINES], "…");
        component.set_expanded(true);
        let rows = component.render(40);
        assert_eq!(rows.len(), 8);
    }

    #[test]
    fn renderer_is_optional() {
        let component = HookMessageComponent::new(hook("x"), None);
        let theme = crate::theme::loader::create_theme(
            crate::theme::builtin::builtin_theme_json("dark").unwrap(),
            crate::theme::loader::CreateThemeOptions {
                mode: Some(crate::theme::theme_class::ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(component.render_custom(&theme).is_none());
    }
}
