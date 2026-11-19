//! Renders a collab guest prompt on every participant's transcript: a
//! user-message-styled bubble prefixed with the author's name.
//!
//! Reference: `l/src/chat/collab-prompt-message.ts`.
//!
//! Deviation: there is no global theme or `getMarkdownTheme()`, so the
//! constructor takes `&Theme` and styles the bubble as it is built.

use crate::chat::messages::{CustomMessage, MessageBlock};
use crate::component::Component;
use crate::components::container::Container;
use crate::components::markdown::Markdown;
use crate::components::text::Text;
use crate::native::describe::{card, md, span};
use crate::native::node::{NativeNode, Props, Text as NodeText};
use crate::theme::theme_class::Theme;

/// A guest prompt drawn in the user-message bubble with its author's name.
pub struct CollabPromptMessage {
    container: Container,
    native: NativeNode,
}

impl CollabPromptMessage {
    /// Build the bubble for `message`.
    pub fn new(message: &CustomMessage, theme: &Theme) -> Self {
        let from = message
            .details
            .as_ref()
            .and_then(|details| details.get("from"))
            .and_then(|from| from.as_str())
            .map(str::trim)
            .filter(|from| !from.is_empty())
            .unwrap_or("guest");
        let author = format!("\u{1b}[1m«{from}»\u{1b}[22m ›");
        let author = theme.fg("accent", &author).unwrap_or(author);
        let text = message_text(message);

        let mut container = Container::new();
        container.add_child(Text::padded(author, 1, 0));
        let mut markdown = Markdown::new();
        markdown.set_padding(1, 1);
        markdown.set_text(text.clone());
        container.add_child(markdown);

        let mut props = Props::new();
        props.set_str("role", "omp.user.collab");
        props.set_str("tone", "user");
        props.set_head(NodeText::Spans(vec![span(
            format!("«{from}» ›"),
            Some("accent strong"),
        )]));
        let native = card(props, vec![md(text, Props::new())]);
        CollabPromptMessage { container, native }
    }

    /// A user-toned card headed by the guest's name, with the prompt as
    /// markdown.
    pub fn describe(&self) -> NativeNode {
        self.native.clone()
    }
}

/// The prompt text: a bare string, else the joined text blocks.
fn message_text(message: &CustomMessage) -> String {
    match &message.content {
        crate::chat::messages::CustomMessageContent::Text(text) => text.clone(),
        crate::chat::messages::CustomMessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                MessageBlock::Text(text) => Some(text.text.as_str()),
                MessageBlock::Image(_) => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

impl Component for CollabPromptMessage {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.container.render(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::messages::{CustomMessageContent, MessageAttribution, TextContent};
    use crate::theme::builtin::builtin_theme_json;
    use crate::theme::loader::{create_theme, CreateThemeOptions};
    use crate::theme::theme_class::ColorMode;
    use serde_json::json;

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

    fn message(content: CustomMessageContent, from: Option<&str>) -> CustomMessage {
        CustomMessage {
            custom_type: crate::chat::messages::COLLAB_PROMPT_MESSAGE_TYPE.to_string(),
            content,
            display: true,
            details: from.map(|from| json!({ "from": from })),
            attribution: MessageAttribution::User,
            timestamp: 0,
        }
    }

    #[test]
    fn author_line_names_the_guest_and_falls_back() {
        let theme = theme();
        let mut component =
            CollabPromptMessage::new(&message(CustomMessageContent::Text("hi".into()), Some("ada")), &theme);
        let rows = component.render(40);
        let visible = crate::ansi_scan::strip_ansi(&rows[0]);
        assert_eq!(visible, " «ada» ›");

        let mut anonymous =
            CollabPromptMessage::new(&message(CustomMessageContent::Text("hi".into()), Some("   ")), &theme);
        let rows = anonymous.render(40);
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).contains("guest"));
    }

    #[test]
    fn text_blocks_join_without_separator() {
        let content = CustomMessageContent::Blocks(vec![
            MessageBlock::Text(TextContent {
                text: "one ".into(),
                ..TextContent::default()
            }),
            MessageBlock::Text(TextContent {
                text: "two".into(),
                ..TextContent::default()
            }),
        ]);
        assert_eq!(message_text(&message(content, None)), "one two");
    }

    #[test]
    fn describe_marks_the_collab_role() {
        let theme = theme();
        let component =
            CollabPromptMessage::new(&message(CustomMessageContent::Text("hi".into()), Some("ada")), &theme);
        let described = component.describe();
        assert_eq!(described.props.get_str("role"), Some("omp.user.collab"));
        assert_eq!(described.props.get_str("tone"), Some("user"));
        assert_eq!(described.children.len(), 1);
    }
}
