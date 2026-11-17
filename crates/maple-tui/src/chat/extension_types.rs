//! Extension-facing types: the render hooks extensions register, the
//! transcript block vocabulary presenters see, and the presentation decision
//! they return.
//!
//! Reference: `l/src/chat/extension-types.ts`.
//!
//! Deviations forced by Rust:
//! - Every renderer is a boxed closure ([`MessageRenderer`],
//!   [`HookMessageRenderer`], [`AssistantThinkingRenderer`],
//!   [`AssistantTextRenderer`], [`TranscriptPresenter`]); the reference's
//!   generic `T = unknown` details become `serde_json::Value`, so the type
//!   parameters are gone.
//! - `AssistantThinkingRenderContext.requestRender()` is a
//!   `&dyn Fn()` callback instead of a method (the reference closes over the
//!   owning component).
//! - `TranscriptPresentation`'s `undefined` arm is `Option`'s `None`, and
//!   `readonly unknown[]` becomes `serde_json::Value`.
//! - `ExtensionUiComponent`'s optional `dispose` is
//!   [`crate::component::Component::dispose`], which already has a default.

use serde_json::Value;

use crate::component::Component;
use crate::native::node::Text;
use crate::theme::Theme;
use crate::tui::Tui;

use crate::chat::messages::{
    AssistantMessage, BashExecutionMessage, CustomMessage, HookMessage, PythonExecutionMessage,
};
use crate::chat::todo_reminder::TodoItem;
use crate::chat::ttsr_notification::NotificationRule;

/// A component an extension mounts into the UI.
pub type ExtensionUiComponent = Box<dyn Component>;

/// Builds one extension UI component.
pub type ExtensionUiComponentFactory = Box<dyn FnMut(&mut Tui, &Theme) -> ExtensionUiComponent>;

/// What an extension widget slot holds: plain rows, a component factory, or
/// nothing.
pub enum ExtensionWidgetContent {
    /// Rendered rows.
    Lines(Vec<String>),
    /// A component built on demand.
    Factory(ExtensionUiComponentFactory),
    /// The slot is empty (`undefined`).
    Empty,
}

/// Options passed to a message renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageRenderOptions {
    /// Whether the view is expanded.
    pub expanded: bool,
}

/// Renders a custom message, or defers with `None`.
pub type MessageRenderer =
    Box<dyn Fn(&CustomMessage, MessageRenderOptions, &Theme) -> Option<Box<dyn Component>>>;

/// Everything a thinking renderer may read about one thinking block.
pub struct AssistantThinkingRenderContext<'a> {
    /// The block's index in the reply's `content`.
    pub content_index: usize,
    /// The block's index among the reply's thinking blocks.
    pub thinking_index: usize,
    /// The thinking text drawn so far.
    pub text: &'a str,
    /// Ask the transcript to redraw.
    pub request_render: &'a dyn Fn(),
}

/// Renders one thinking block, or defers with `None`.
pub type AssistantThinkingRenderer = Box<
    dyn Fn(&AssistantThinkingRenderContext<'_>, &Theme) -> Option<Box<dyn Component>>,
>;

/// Everything an assistant-text renderer may read about one text block.
pub struct AssistantTextRenderContext<'a> {
    /// The message being drawn; `message.timestamp` identifies the reply.
    ///
    /// After a tool call this is only the segment drawn there, so its `content`
    /// does not hold the whole reply. Editing it cannot change the stored reply.
    pub message: &'a AssistantMessage,
    /// The text block's index in the full reply's `content`, also when the
    /// reply is drawn in segments around tool calls.
    pub content_index: usize,
    /// The block's markdown as drawn so far, after any earlier renderer.
    pub text: &'a str,
    /// True while the block may still grow.
    pub streaming: bool,
}

/// Display-only transform of one assistant text block: the markdown to draw,
/// or `None` to draw `context.text` unchanged.
pub type AssistantTextRenderer = Box<dyn Fn(&AssistantTextRenderContext<'_>) -> Option<String>>;

/// Extension text renderers for one assistant component, and where its content
/// starts in the full reply.
pub struct AssistantTextRendering {
    /// Renderers, applied in registration order.
    pub renderers: Vec<AssistantTextRenderer>,
    /// Index in the full reply's `content` of this component's first block.
    pub content_offset: usize,
}

/// A session message a transcript block may draw.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionMessage {
    /// An extension-injected custom message.
    Custom(CustomMessage),
    /// A `!` shell execution.
    BashExecution(BashExecutionMessage),
    /// A `$` Python execution.
    PythonExecution(PythonExecutionMessage),
}

/// The result a tool call produced, as far as a presenter may see it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranscriptToolResult {
    /// Result content blocks.
    pub content: Vec<Value>,
    /// Structured details.
    pub details: Option<Value>,
}

/// An event a transcript block stands for.
#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptEvent {
    /// A TTSR rule hit rewind.
    TtsrTriggered {
        /// The rules that matched.
        rules: Vec<NotificationRule>,
    },
    /// An incomplete-todo reminder.
    TodoReminder {
        /// The outstanding todos.
        todos: Vec<TodoItem>,
        /// One-based reminder attempt.
        attempt: u32,
        /// Attempts allowed before the turn is forced closed.
        max_attempts: u32,
    },
}

/// One transcript block a [`TranscriptPresenter`] decides the display of.
///
/// Later reference versions add block types; answer `None` for a type you do
/// not know.
#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptBlock {
    /// A model tool call.
    ToolCall {
        /// The call id.
        id: String,
        /// Tool name.
        tool_name: String,
        /// Call arguments.
        args: Value,
        /// The call's result, once its first result frame arrived.
        result: Option<TranscriptToolResult>,
        /// The call failed.
        is_error: bool,
        /// The call still streams progress.
        is_partial: bool,
    },
    /// A session message drawn as its own block.
    Message {
        /// The decimal `message.timestamp` (shared by messages recorded in the
        /// same millisecond).
        id: String,
        /// The message.
        message: SessionMessage,
    },
    /// A TTSR rule hit or an incomplete-todo reminder.
    Event {
        /// A per-process unique id.
        id: String,
        /// The event.
        event: TranscriptEvent,
    },
}

impl TranscriptBlock {
    /// The block's id: the call id, the message timestamp, or the event id.
    pub fn id(&self) -> &str {
        match self {
            TranscriptBlock::ToolCall { id, .. }
            | TranscriptBlock::Message { id, .. }
            | TranscriptBlock::Event { id, .. } => id,
        }
    }

    /// The wire name of the block type.
    pub fn kind(&self) -> &'static str {
        match self {
            TranscriptBlock::ToolCall { .. } => "toolCall",
            TranscriptBlock::Message { .. } => "message",
            TranscriptBlock::Event { .. } => "event",
        }
    }
}

/// One compact row's replacement: a label and extra rows below it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompactPresentation {
    /// Replaces the summary row's text.
    pub label: Option<Text>,
    /// One extra row per entry.
    pub rows: Vec<Text>,
}

/// How a transcript block is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TranscriptPresentation {
    /// Defer to the next presenter (and, after the last, to `"full"`).
    #[default]
    Undefined,
    /// The built-in card.
    Full,
    /// Draw nothing.
    Hidden,
    /// One row from the built-in summary.
    Compact,
    /// Replace the summary row's text and add rows.
    CompactWith(CompactPresentation),
}

/// Display-only choice of how one transcript block is drawn.
///
/// Presenters are asked in extension load order; the first answer other than
/// [`TranscriptPresentation::Undefined`] wins. A block the user expanded is
/// always drawn in full.
pub type TranscriptPresenter = Box<dyn Fn(&TranscriptBlock) -> TranscriptPresentation>;

/// Options passed to a hook message renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookMessageRenderOptions {
    /// Whether the view is expanded.
    pub expanded: bool,
}

/// Renders a hook message, or defers with `None`.
pub type HookMessageRenderer =
    Box<dyn Fn(&HookMessage, HookMessageRenderOptions, &Theme) -> Option<Box<dyn Component>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::messages::{CustomMessageContent, MessageAttribution};
    use crate::native::node::Text as NodeText;
    use serde_json::json;

    fn custom(custom_type: &str) -> CustomMessage {
        CustomMessage {
            custom_type: custom_type.to_string(),
            content: CustomMessageContent::Text("body".into()),
            display: true,
            details: None,
            attribution: MessageAttribution::Agent,
            timestamp: 7,
        }
    }

    #[test]
    fn block_ids_and_kinds_follow_the_wire_names() {
        let tool = TranscriptBlock::ToolCall {
            id: "call_1".into(),
            tool_name: "read".into(),
            args: json!({"path": "a"}),
            result: None,
            is_error: false,
            is_partial: true,
        };
        assert_eq!(tool.id(), "call_1");
        assert_eq!(tool.kind(), "toolCall");

        let message = TranscriptBlock::Message {
            id: "1700".into(),
            message: SessionMessage::Custom(custom("advisor")),
        };
        assert_eq!(message.id(), "1700");
        assert_eq!(message.kind(), "message");

        let event = TranscriptBlock::Event {
            id: "e1".into(),
            event: TranscriptEvent::TtsrTriggered {
                rules: vec![NotificationRule {
                    name: "no-unwrap".into(),
                    description: None,
                    content: None,
                }],
            },
        };
        assert_eq!(event.id(), "e1");
        assert_eq!(event.kind(), "event");
    }

    #[test]
    fn presentation_default_defers_to_the_next_presenter() {
        assert_eq!(
            TranscriptPresentation::default(),
            TranscriptPresentation::Undefined
        );
        let compact = TranscriptPresentation::CompactWith(CompactPresentation {
            label: Some(NodeText::plain("2 files")),
            rows: vec![NodeText::plain("a.rs")],
        });
        match compact {
            TranscriptPresentation::CompactWith(inner) => {
                assert_eq!(inner.label.unwrap().plain_text(), "2 files");
                assert_eq!(inner.rows.len(), 1);
            }
            _ => panic!("expected a compact presentation"),
        }
    }

    #[test]
    fn presenters_are_asked_in_order_and_the_first_answer_wins() {
        let presenters: Vec<TranscriptPresenter> = vec![
            Box::new(|_block| TranscriptPresentation::Undefined),
            Box::new(|block| {
                if block.kind() == "toolCall" {
                    TranscriptPresentation::Hidden
                } else {
                    TranscriptPresentation::Undefined
                }
            }),
            Box::new(|_block| TranscriptPresentation::Full),
        ];
        let block = TranscriptBlock::ToolCall {
            id: "c".into(),
            tool_name: "grep".into(),
            args: json!({}),
            result: None,
            is_error: false,
            is_partial: false,
        };
        let decided = presenters
            .iter()
            .map(|presenter| presenter(&block))
            .find(|presentation| *presentation != TranscriptPresentation::Undefined)
            .unwrap_or(TranscriptPresentation::Full);
        assert_eq!(decided, TranscriptPresentation::Hidden);
    }
}
