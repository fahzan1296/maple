//! Replies: text the agent sends to a scheme whose writes are `reply`-scoped
//! (`out://`) is its mid-turn message to the user, drawn as assistant prose so
//! it can answer without ending its turn.
//!
//! Reference: `l/src/chat/reply-write.ts`.
//!
//! - A `write` call to a reply URL draws as [`ReplyWriteComponent`], streaming
//!   as the call's arguments arrive, instead of a tool card.
//! - Text a tool's commands write there (`printf … > out://` in bash) arrives in
//!   the tool's result `details.replies` ([`tool_result_replies`]); transcripts
//!   draw each one as a prose block after the call ([`reply_display_message`]).
//!
//! Deviations: `ToolResultFrame` lives in the (not yet ported) tool-execution
//! module, so [`ReplyWriteComponent::update_result`] takes the formatted error
//! text directly; `imageBudget`/`linkTargets` are carried but not consumed
//! until [`AssistantMessageComponent`] is ported.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::chat::messages::{
    AssistantContent, AssistantMessage, MessageBlock, StopReason, TextContent, Usage,
};
use crate::component::Component;
use crate::components::markdown::Markdown;
use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, Props, Text as NodeText};
use crate::tools::url_scheme_host::{internal_url_scheme_spec, split_url_scheme, SchemeWriteScope};

/// Display options of a reply block; the rest of its presentation follows the
/// assistant prose defaults.
#[derive(Clone, Default)]
pub struct ReplyWriteOptions {
    /// Repaint request for figures and images that finish loading after a render.
    pub request_render: Option<Arc<dyn Fn()>>,
    /// Resolved local destinations of Markdown links, as for assistant text.
    pub link_targets: HashMap<String, String>,
}

/// Whether a `tool_name` call writes to a reply URL.
///
/// `None` while streamed args cannot tell yet: no path so far, or a path that
/// could still grow into a reply URL (`ou`, `out:/`). Callers holding final
/// args treat `None` as `false`.
pub fn is_reply_write(tool_name: &str, args: Option<&Value>) -> Option<bool> {
    if tool_name != "write" {
        return Some(false);
    }
    let args = args?.as_object()?;
    let target = match args.get("path") {
        Some(Value::String(path)) => Some(path.as_str()),
        _ => match args.get("file_path") {
            Some(Value::String(path)) => Some(path.as_str()),
            _ => None,
        },
    }?;
    if let Some((scheme, _)) = split_url_scheme(target) {
        return Some(
            internal_url_scheme_spec(&scheme)
                .and_then(|spec| spec.write)
                .is_some_and(|write| write.scope == SchemeWriteScope::Reply),
        );
    }
    let lower = target.to_lowercase();
    for (scheme, spec) in crate::tools::url_scheme_host::internal_url_scheme_specs() {
        let reply = spec
            .write
            .is_some_and(|write| write.scope == SchemeWriteScope::Reply);
        if reply && format!("{scheme}://").starts_with(&lower) {
            return None;
        }
    }
    Some(false)
}

/// Replies a tool result carries in `details.replies` (text its commands wrote
/// to a reply URL), oldest first.
pub fn tool_result_replies(details: Option<&Value>) -> Vec<String> {
    details
        .and_then(|details| details.as_object())
        .and_then(|details| details.get("replies"))
        .and_then(|replies| replies.as_array())
        .map(|replies| {
            replies
                .iter()
                .filter_map(|reply| reply.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Assistant message drawing reply text. No provider turn backs it, so its
/// identity fields stay empty.
pub fn reply_display_message(text: &str, error_message: Option<&str>) -> AssistantMessage {
    AssistantMessage {
        content: vec![AssistantContent::Text(TextContent {
            text: text.to_string(),
            ..TextContent::default()
        })],
        api: String::new(),
        provider: String::new(),
        model: String::new(),
        usage: Usage::default(),
        stop_reason: if error_message.is_some() {
            StopReason::Error
        } else {
            StopReason::Stop
        },
        error_message: error_message.map(str::to_string),
        timestamp: 0,
        ..AssistantMessage::default()
    }
}

/// A reply write drawn as assistant prose.
///
/// The call's lifecycle arrives through the `update_*` methods: streamed args
/// grow the text, final args settle it, and the result seals the block, a
/// failure showing its error under the text. It is a reply, not tool activity,
/// so hiding tool activity leaves it on screen.
pub struct ReplyWriteComponent {
    text: String,
    args_complete: bool,
    finalized: bool,
    options: ReplyWriteOptions,
    message: AssistantMessage,
    markdown: Markdown,
    error: Option<String>,
}

impl ReplyWriteComponent {
    /// An empty reply block.
    pub fn new(options: ReplyWriteOptions) -> Self {
        let mut component = ReplyWriteComponent {
            text: String::new(),
            args_complete: false,
            finalized: false,
            options,
            message: reply_display_message("", None),
            markdown: Markdown::new(),
            error: None,
        };
        component.draw();
        component
    }

    /// The reply text drawn so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The assistant message backing this block.
    pub fn message(&self) -> &AssistantMessage {
        &self.message
    }

    /// Whether the transcript block is finalized.
    pub fn is_transcript_block_finalized(&self) -> bool {
        self.finalized
    }

    /// Grow the text from streamed `write` arguments.
    pub fn update_args(&mut self, args: Option<&Value>) {
        let text = args
            .and_then(|args| args.as_object())
            .and_then(|args| args.get("content"))
            .and_then(|content| content.as_str())
            .unwrap_or_default()
            .to_string();
        if text == self.text || self.is_transcript_block_finalized() {
            return;
        }
        self.text = text;
        self.draw();
    }

    /// Mark the arguments final.
    pub fn set_args_complete(&mut self) {
        if self.args_complete || self.is_transcript_block_finalized() {
            return;
        }
        self.args_complete = true;
        self.draw();
    }

    /// The execution started; replies have nothing to draw for it.
    pub fn set_execution_started(&mut self) {}

    /// Seal the block once the call finished; `error` is the failed call's text.
    pub fn update_result(&mut self, error: Option<String>) {
        if self.is_transcript_block_finalized() {
            return;
        }
        self.args_complete = true;
        self.error = error.clone();
        self.message = reply_display_message(&self.text, error.as_deref());
        self.finalized = true;
        self.draw();
    }

    /// Park as background: nothing to do for a reply.
    pub fn park_as_background(&mut self) {}

    /// Mark the block finalized.
    pub fn seal(&mut self) {
        self.finalized = true;
    }

    /// Request a repaint when the block finishes loading something.
    pub fn request_render(&self) {
        if let Some(request_render) = &self.options.request_render {
            request_render();
        }
    }

    /// The native transcript description.
    pub fn describe(&self) -> NativeNode {
        let props = Props::new();
        let mut children = vec![crate::native::describe::md(self.text.clone(), Props::new())];
        if let Some(error) = &self.error {
            let mut error_props = Props::new();
            error_props.set_str("tone", "error");
            children.push(crate::native::describe::text(
                NodeText::Plain(error.clone()),
                error_props,
            ));
        }
        node(NodeKind::Col, props, children, None)
    }

    fn draw(&mut self) {
        self.message = reply_display_message(&self.text, None);
        self.markdown.set_text(self.text.clone());
    }
}

impl Component for ReplyWriteComponent {
    fn render(&mut self, width: u16) -> Vec<String> {
        let mut rows = self.markdown.render(width);
        if let Some(error) = &self.error {
            rows.extend(error.lines().map(str::to_string));
        }
        rows
    }
}

impl std::fmt::Debug for ReplyWriteComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplyWriteComponent")
            .field("text", &self.text)
            .field("args_complete", &self.args_complete)
            .field("finalized", &self.finalized)
            .finish()
    }
}

/// Text blocks of a result, for callers formatting a failure.
pub fn result_text_blocks(content: &[MessageBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            MessageBlock::Text(text) => Some(text.text.as_str()),
            MessageBlock::Image(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_write_calls_can_be_replies() {
        assert_eq!(is_reply_write("read", Some(&json!({"path": "out://x"}))), Some(false));
        assert_eq!(is_reply_write("write", None), None);
        assert_eq!(is_reply_write("write", Some(&json!("str"))), None);
        assert_eq!(is_reply_write("write", Some(&json!({}))), None);
        // A prefix that could still grow into `out://` is undecided.
        assert_eq!(is_reply_write("write", Some(&json!({"path": "ou"}))), None);
        assert_eq!(is_reply_write("write", Some(&json!({"path": "out:/"}))), None);
        // Case-insensitive prefix match.
        assert_eq!(is_reply_write("write", Some(&json!({"path": "OU"}))), None);
        // A plain file is decided.
        assert_eq!(
            is_reply_write("write", Some(&json!({"path": "src/main.rs"}))),
            Some(false)
        );
        assert_eq!(
            is_reply_write("write", Some(&json!({"file_path": "src/main.rs"}))),
            Some(false)
        );
    }

    #[test]
    fn replies_are_collected_from_details_in_order() {
        let details = json!({"replies": ["one", 3, "two"]});
        assert_eq!(
            tool_result_replies(Some(&details)),
            vec!["one".to_string(), "two".to_string()]
        );
        assert!(tool_result_replies(None).is_empty());
        assert!(tool_result_replies(Some(&json!({"replies": "x"}))).is_empty());
        assert!(tool_result_replies(Some(&json!("x"))).is_empty());
    }

    #[test]
    fn display_messages_carry_no_provider_identity() {
        let message = reply_display_message("hi", None);
        assert_eq!(message.stop_reason, StopReason::Stop);
        assert_eq!(message.api, "");
        assert_eq!(message.timestamp, 0);
        assert_eq!(message.content[0].as_text(), Some("hi"));

        let failed = reply_display_message("hi", Some("boom"));
        assert_eq!(failed.stop_reason, StopReason::Error);
        assert_eq!(failed.error_message.as_deref(), Some("boom"));
    }

    #[test]
    fn component_streams_args_and_seals_on_result() {
        let mut component = ReplyWriteComponent::new(ReplyWriteOptions::default());
        component.update_args(Some(&json!({"content": "part"})));
        assert_eq!(component.text(), "part");
        assert!(!component.is_transcript_block_finalized());
        component.set_args_complete();
        component.update_result(Some("failed".into()));
        assert!(component.is_transcript_block_finalized());
        assert_eq!(component.message().error_message.as_deref(), Some("failed"));
        // Late updates after sealing are ignored.
        component.update_args(Some(&json!({"content": "late"})));
        assert_eq!(component.text(), "part");
    }

    #[test]
    fn result_text_blocks_join_text_only() {
        let content = vec![
            MessageBlock::Text(TextContent {
                text: "a".into(),
                ..TextContent::default()
            }),
            MessageBlock::Text(TextContent {
                text: "b".into(),
                ..TextContent::default()
            }),
        ];
        assert_eq!(result_text_blocks(&content), "a\nb");
    }
}
