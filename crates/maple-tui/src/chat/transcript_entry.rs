//! Persisted session entries as transcript messages.
//!
//! Reference: `l/src/chat/transcript-entry.ts`.
//!
//! Session files store two entry shapes: `message` entries carrying an agent
//! message, and `custom_message` entries carrying an extension's message.
//! Rendering, the rewind picker and the title model all restore messages with
//! the helpers here.
//!
//! Deviations:
//! - `pi-agent-core`'s `AgentMessage` union and `pi-ai`'s `UserMessage` have no
//!   crate equivalent, so [`AgentMessage`]/[`UserMessage`]/[`UserContent`] are
//!   defined here, covering the roles transcript rendering consumes.
//! - `createCustomMessage` is [`CustomMessage`] built from the normalized
//!   payload fields.
//! - Entry `content` is a [`serde_json::Value`] (persisted JSON) rather than the
//!   typed union.

use serde_json::Value;

use crate::chat::messages::{
    is_custom_message_content, is_user_turn_initiator, normalize_custom_message_payload,
    AssistantMessage, CustomMessage, CustomMessageContent, CustomMessageFields,
    CustomMessagePayload, ImageContent, MessageAttribution, MessageBlock, TextContent,
};
use crate::chat::skill_title_input::{title_text_from_skill_prompt, SkillPromptTitleMessage};

/// Content of a user message: a bare string or typed blocks.
#[derive(Clone, Debug, PartialEq)]
pub enum UserContent {
    /// A bare prompt string.
    Text(String),
    /// Text and image blocks.
    Blocks(Vec<MessageBlock>),
}

impl UserContent {
    /// The content as text blocks, joined by `separator`.
    pub fn text(&self, separator: &str) -> String {
        match self {
            UserContent::Text(text) => text.clone(),
            UserContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|block| match block {
                    MessageBlock::Text(text) => Some(text.text.as_str()),
                    MessageBlock::Image(_) => None,
                })
                .collect::<Vec<_>>()
                .join(separator),
        }
    }

    /// The well-formed image blocks, in order.
    pub fn images(&self) -> Vec<ImageContent> {
        match self {
            UserContent::Text(_) => Vec::new(),
            UserContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|block| match block {
                    MessageBlock::Image(image) => Some(image.clone()),
                    MessageBlock::Text(_) => None,
                })
                .collect(),
        }
    }
}

impl Default for UserContent {
    fn default() -> Self {
        UserContent::Text(String::new())
    }
}

/// A user prompt as persisted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserMessage {
    /// Message content.
    pub content: UserContent,
    /// Wall-clock timestamp in milliseconds.
    pub timestamp: i64,
}

/// A message a session entry holds.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentMessage {
    /// A user prompt.
    User(UserMessage),
    /// An assistant reply.
    Assistant(AssistantMessage),
    /// An extension-injected message.
    Custom(CustomMessage),
    /// Any other persisted message (tool results, hook messages), kept opaque.
    Other(Value),
}

impl AgentMessage {
    /// The message's role tag.
    pub fn role(&self) -> &'static str {
        match self {
            AgentMessage::User(_) => "user",
            AgentMessage::Assistant(_) => "assistant",
            AgentMessage::Custom(_) => "custom",
            AgentMessage::Other(_) => "other",
        }
    }
}

/// Persisted `message` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionMessageEntryLike {
    /// Entry id.
    pub id: String,
    /// Parent entry id.
    pub parent_id: Option<String>,
    /// ISO timestamp.
    pub timestamp: String,
    /// The message.
    pub message: AgentMessage,
}

/// Persisted `custom_message` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomMessageEntryLike {
    /// Entry id.
    pub id: String,
    /// Parent entry id.
    pub parent_id: Option<String>,
    /// ISO timestamp.
    pub timestamp: String,
    /// Custom type tag.
    pub custom_type: String,
    /// Persisted content (string or block array).
    pub content: Value,
    /// Type-specific details.
    pub details: Option<Value>,
    /// Whether the transcript draws the message.
    pub display: bool,
    /// Who initiated the message.
    pub attribution: Option<MessageAttribution>,
}
/// Entries that replay as visible or hidden transcript messages.
#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptEntryLike {
    /// A `message` entry.
    Message(SessionMessageEntryLike),
    /// A `custom_message` entry.
    CustomMessage(CustomMessageEntryLike),
}

impl TranscriptEntryLike {
    /// The entry's persisted type tag.
    pub fn entry_type(&self) -> &'static str {
        match self {
            TranscriptEntryLike::Message(_) => "message",
            TranscriptEntryLike::CustomMessage(_) => "custom_message",
        }
    }
}

/// Parse persisted content into typed custom-message content; `None` when it is
/// neither a string nor an array.
pub fn custom_content_from_value(content: &Value) -> Option<CustomMessageContent> {
    match content {
        Value::String(text) => Some(CustomMessageContent::Text(text.clone())),
        Value::Array(items) => Some(CustomMessageContent::Blocks(
            items.iter().filter_map(block_from_value).collect(),
        )),
        _ => None,
    }
}

fn block_from_value(value: &Value) -> Option<MessageBlock> {
    let object = value.as_object()?;
    match object.get("type").and_then(Value::as_str)? {
        "text" => Some(MessageBlock::Text(TextContent {
            text: object.get("text").and_then(Value::as_str)?.to_string(),
            ..TextContent::default()
        })),
        "image" => Some(MessageBlock::Image(ImageContent {
            data: object.get("data").and_then(Value::as_str)?.to_string(),
            mime_type: object.get("mimeType").and_then(Value::as_str)?.to_string(),
            ..ImageContent::default()
        })),
        _ => None,
    }
}

/// Restore a persisted custom message, rejecting unsendable content.
pub fn custom_message_entry_message(entry: &CustomMessageEntryLike) -> Option<CustomMessage> {
    let content = custom_content_from_value(&entry.content)?;
    let normalized = normalize_custom_message_payload(CustomMessagePayload::Object(CustomMessageFields {
        custom_type: Some(entry.custom_type.clone()),
        content: Some(content),
        display: Some(entry.display),
        details: entry.details.clone(),
        attribution: entry.attribution.map(|attribution| attribution.as_str().to_string()),
    }));
    Some(CustomMessage {
        custom_type: normalized.custom_type,
        content: normalized.content,
        display: normalized.display,
        details: normalized.details,
        attribution: normalized.attribution,
        timestamp: parse_timestamp(&entry.timestamp),
    })
}
/// Epoch milliseconds from an ISO-8601 timestamp (`YYYY-MM-DDTHH:MM:SS[.mmm]Z`
/// or with a `±HH:MM` offset); 0 when it does not parse.
pub fn parse_timestamp(timestamp: &str) -> i64 {
    let bytes = timestamp.as_bytes();
    let number = |from: usize, len: usize| -> Option<i64> {
        std::str::from_utf8(bytes.get(from..from + len)?)
            .ok()?
            .parse()
            .ok()
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0, 4),
        number(5, 2),
        number(8, 2),
        number(11, 2),
        number(14, 2),
        number(17, 2),
    ) else {
        return 0;
    };
    let mut millis = 0i64;
    let mut cursor = 19;
    if bytes.get(cursor) == Some(&b'.') {
        let digits: String = timestamp[cursor + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let padded = format!("{digits:0<3}");
        millis = padded.get(..3).and_then(|value| value.parse().ok()).unwrap_or(0);
        cursor += 1 + digits.len();
    }
    let mut offset_seconds = 0i64;
    match bytes.get(cursor) {
        Some(b'Z') | None => {}
        Some(sign @ (b'+' | b'-')) => {
            let sign = if *sign == b'+' { 1 } else { -1 };
            let offset_hours: i64 = number(cursor + 1, 2).unwrap_or(0);
            let offset_minutes: i64 = number(cursor + 4, 2).unwrap_or(0);
            offset_seconds = sign * (offset_hours * 3600 + offset_minutes * 60);
        }
        Some(_) => return 0,
    }
    let days = days_from_civil(year, month, day);
    (days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds) * 1000 + millis
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}


/// Restore the message represented by a transcript entry.
pub fn transcript_entry_message(entry: &TranscriptEntryLike) -> Option<AgentMessage> {
    match entry {
        TranscriptEntryLike::Message(message) => Some(message.message.clone()),
        TranscriptEntryLike::CustomMessage(custom) => {
            custom_message_entry_message(custom).map(AgentMessage::Custom)
        }
    }
}

/// Identify user-authored requests among transcript and metadata entries.
pub fn is_user_request_entry(entry: Option<&TranscriptEntryLike>) -> bool {
    match entry {
        None => false,
        Some(TranscriptEntryLike::Message(message)) => match &message.message {
            AgentMessage::User(_) => true,
            AgentMessage::Custom(custom) => is_user_turn_initiator(custom),
            _ => false,
        },
        Some(TranscriptEntryLike::CustomMessage(custom)) => custom_message_entry_message(custom)
            .is_some_and(|message| is_user_turn_initiator(&message)),
    }
}

/// Recent transcript tail starting at a user-request boundary, so tool calls
/// and results stay together.
pub fn recent_transcript_entries(entries: &[TranscriptEntryLike], limit: usize) -> Vec<TranscriptEntryLike> {
    let limit = limit.max(1);
    if entries.len() <= limit {
        return entries.to_vec();
    }
    let mut index = entries.len() - limit;
    while index > 0 {
        if is_user_request_entry(entries.get(index)) {
            return entries[index..].to_vec();
        }
        index -= 1;
    }
    entries.to_vec()
}

/// Editable user request text, preserving skill invocation syntax.
pub fn user_turn_draft(entry: &TranscriptEntryLike) -> Option<String> {
    let message = transcript_entry_message(entry)?;
    match message {
        AgentMessage::User(user) => Some(user.content.text("")),
        AgentMessage::Custom(custom) => {
            if !is_user_turn_initiator(&custom) {
                return None;
            }
            title_text_from_skill_prompt(&SkillPromptTitleMessage {
                role: "custom",
                custom_type: Some(&custom.custom_type),
                attribution: Some(custom.attribution.as_str()),
                details: custom.details.as_ref(),
            })
            .or_else(|| Some(custom.content.plain_text()))
        }
        _ => None,
    }
}

/// Extract text blocks verbatim, optionally separating block boundaries.
pub fn text_content(content: &UserContent, separator: &str) -> String {
    content.text(separator)
}

/// Well-formed image blocks in order: a `[Image #N]` marker in the text names
/// the Nth.
pub fn image_content(content: &UserContent) -> Vec<ImageContent> {
    content.images()
}

/// Join text blocks with spaces and collapse whitespace into a single-line
/// label.
pub fn user_message_label(content: &UserContent) -> String {
    let joined = text_content(content, " ");
    joined.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn custom_entry(content: Value) -> CustomMessageEntryLike {
        CustomMessageEntryLike {
            id: "e1".into(),
            parent_id: None,
            timestamp: "2026-01-01T00:00:00.000Z".into(),
            custom_type: "collab-prompt".into(),
            content,
            details: Some(json!({"from": "user"})),
            display: true,
            attribution: Some(MessageAttribution::User),
        }
    }

    #[test]
    fn custom_entries_restore_messages_but_reject_unsendable_content() {
        let entry = custom_entry(json!([{"type": "text", "text": "hi"}]));
        let message = custom_message_entry_message(&entry).expect("restores");
        assert_eq!(message.custom_type, "collab-prompt");
        assert!(message.display);
        assert_eq!(message.attribution, MessageAttribution::User);

        let text_entry = custom_entry(json!("plain"));
        assert!(custom_message_entry_message(&text_entry).is_some());

        let bad = custom_entry(json!({"nope": true}));
        assert!(custom_message_entry_message(&bad).is_none());
        assert!(custom_content_from_value(&json!(42)).is_none());
    }

    #[test]
    fn transcript_messages_come_from_both_entry_shapes() {
        let user = TranscriptEntryLike::Message(SessionMessageEntryLike {
            id: "e2".into(),
            parent_id: None,
            timestamp: String::new(),
            message: AgentMessage::User(UserMessage {
                content: UserContent::Text("hello".into()),
                timestamp: 1,
            }),
        });
        assert!(matches!(
            transcript_entry_message(&user),
            Some(AgentMessage::User(_))
        ));
        let custom = TranscriptEntryLike::CustomMessage(custom_entry(json!("x")));
        assert_eq!(transcript_entry_message(&custom).map(|m| m.role()), Some("custom"));
    }

    #[test]
    fn user_requests_include_user_turns_and_user_invocations() {
        let prompt = TranscriptEntryLike::Message(SessionMessageEntryLike {
            id: "e3".into(),
            parent_id: None,
            timestamp: String::new(),
            message: AgentMessage::User(UserMessage::default()),
        });
        assert!(is_user_request_entry(Some(&prompt)));
        assert!(is_user_request_entry(Some(&TranscriptEntryLike::CustomMessage(custom_entry(
            json!("x")
        )))));
        assert!(!is_user_request_entry(None));
        let agent_custom = TranscriptEntryLike::CustomMessage(CustomMessageEntryLike {
            attribution: Some(MessageAttribution::Agent),
            ..custom_entry(json!("x"))
        });
        assert!(!is_user_request_entry(Some(&agent_custom)));
    }

    #[test]
    fn recent_tails_start_at_a_user_request() {
        let entry = |role: &str| {
            TranscriptEntryLike::Message(SessionMessageEntryLike {
                id: role.into(),
                parent_id: None,
                timestamp: String::new(),
                message: if role == "user" {
                    AgentMessage::User(UserMessage::default())
                } else {
                    AgentMessage::Assistant(AssistantMessage::default())
                },
            })
        };
        let entries = vec![entry("user"), entry("assistant"), entry("assistant"), entry("user")];
        assert_eq!(recent_transcript_entries(&entries, 10).len(), 4);
        assert_eq!(recent_transcript_entries(&entries, 2).len(), 2);
        assert_eq!(recent_transcript_entries(&entries, 3).len(), 3);
        assert_eq!(
            recent_transcript_entries(&entries, 2)[0].entry_type(),
            "message"
        );
        let tail = recent_transcript_entries(&entries, 3);
        assert_eq!(tail.len(), 3);
    }

    #[test]
    fn drafts_keep_skill_syntax_and_user_text() {
        let user = TranscriptEntryLike::Message(SessionMessageEntryLike {
            id: "e4".into(),
            parent_id: None,
            timestamp: String::new(),
            message: AgentMessage::User(UserMessage {
                content: UserContent::Blocks(vec![MessageBlock::Text(TextContent {
                    text: "draft".into(),
                    ..TextContent::default()
                })]),
                timestamp: 0,
            }),
        });
        assert_eq!(user_turn_draft(&user).as_deref(), Some("draft"));

        let skill = TranscriptEntryLike::CustomMessage(CustomMessageEntryLike {
            custom_type: "skill-prompt".into(),
            details: Some(json!({"name": "review", "args": "HEAD"})),
            content: json!([{"type": "text", "text": "expanded body"}]),
            ..custom_entry(json!("x"))
        });
        assert_eq!(
            user_turn_draft(&skill).as_deref(),
            Some("/skill:review HEAD")
        );

        let agent = TranscriptEntryLike::CustomMessage(CustomMessageEntryLike {
            attribution: Some(MessageAttribution::Agent),
            ..custom_entry(json!("x"))
        });
        assert_eq!(user_turn_draft(&agent), None);
    }

    #[test]
    fn text_labels_collapse_whitespace_and_images_list_in_order() {
        let blocks = UserContent::Blocks(vec![
            MessageBlock::Text(TextContent {
                text: "one".into(),
                ..TextContent::default()
            }),
            MessageBlock::Image(ImageContent {
                data: "aGk=".into(),
                mime_type: "image/png".into(),
                ..ImageContent::default()
            }),
            MessageBlock::Text(TextContent {
                text: " two\n".into(),
                ..TextContent::default()
            }),
        ]);
        assert_eq!(text_content(&blocks, " "), "one  two\n");
        assert_eq!(text_content(&blocks, ""), "one two\n");
        assert_eq!(user_message_label(&blocks), "one two");
        assert_eq!(image_content(&blocks).len(), 1);
        assert!(image_content(&UserContent::Text("x".into())).is_empty());
    }

    #[test]
    fn blocks_drop_malformed_entries() {
        let content = json!([
            {"type": "text", "text": "keep"},
            {"type": "image", "data": "aGk=", "mimeType": "image/png"},
            {"type": "image"},
            {"type": "toolCall", "id": "t"},
        ]);
        let parsed = custom_content_from_value(&content).expect("array parses");
        match parsed {
            CustomMessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2);
                assert!(matches!(blocks[1], MessageBlock::Image(_)));
            }
            CustomMessageContent::Text(_) => panic!("expected blocks"),
        }
    }
}
