//! Transcript message vocabulary: custom, hook, execution, and assistant
//! messages, their custom-type tags, and the abort-reason helpers.
//!
//! Reference: `l/src/chat/messages.ts`.
//!
//! The reference imports the model-side message shapes (`AssistantMessage`,
//! `TextContent`, `ImageContent`) from `@oh-my-pi/pi-ai`, the compaction
//! messages from `@oh-my-pi/pi-agent-core`, and `OutputMeta` from
//! `l/src/tools/output-meta.ts`. None of those packages exist in this crate,
//! so the shapes the chat renderers read live here as plain structs:
//!
//! - [`AssistantMessage`] and its content blocks ([`AssistantContent`],
//!   [`TextContent`], [`ThinkingContent`], [`ImageContent`], [`ToolCall`]).
//! - [`Usage`], [`StopReason`], [`AssistantRetryRecovery`] — the reply fields
//!   the renderers count and label.
//! - [`OutputMeta`] and friends — the truncation/source metadata the render
//!   helpers print. The message side keeps only the fields a transcript
//!   renderer reads; the notice rows built from them live in
//!   [`crate::chat::transcript_render_helpers`].
//!
//! `AIError` (`@oh-my-pi/pi-ai` `error/flags.ts`) is likewise absent: the same
//! flag bits are reproduced by [`AiErrorFlag`] plus [`ai_error_create`] and
//! [`ai_error_is`], so persisted `errorId` values keep their meaning.
//!
//! Deviations: TypeScript generics default to `unknown` and every message is a
//! structural type; Rust messages are monomorphic structs with
//! `Option<serde_json::Value>` details, and the normalize helpers take the
//! reference's `unknown` payload as [`CustomMessagePayload`].

use serde_json::Value;

/// Custom message type of a `/skill:<name>` prompt.
pub const SKILL_PROMPT_MESSAGE_TYPE: &str = "skill-prompt";
/// Custom message type for late LSP diagnostics.
pub const LSP_LATE_DIAGNOSTIC_MESSAGE_TYPE: &str = "lsp-late-diagnostic";
/// Custom message type for a `/tan` background-dispatch breadcrumb.
pub const BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE: &str = "background-tan-dispatch";
/// Custom message type for a prewalk plan.
pub const PREWALK_PLAN_MESSAGE_TYPE: &str = "prewalk-plan";
/// Custom message type for the transient Vibe mode directive.
pub const VIBE_MODE_CONTEXT_MESSAGE_TYPE: &str = "vibe-mode-context";
/// Fallback type for extension-injected messages that omit a custom type.
pub const DEFAULT_CUSTOM_MESSAGE_TYPE: &str = "custom-message";
/// Custom message carrying a coding request delegated by the live voice model.
pub const LIVE_DELEGATION_MESSAGE_TYPE: &str = "live-delegation";
/// Custom message carrying a writable-collab peer's prompt (`l/src/wire`).
pub const COLLAB_PROMPT_MESSAGE_TYPE: &str = "collab-prompt";
/// Custom message type for supervised process completions.
pub const LAUNCH_COMPLETION_MESSAGE_TYPE: &str = "launch-completion";

/// Sentinel `AssistantMessage::error_message` marking an expected internal
/// abort (plan-mode → execution compaction) that must not surface as an error
/// line. Persisted, so the bytes are part of the session format.
pub const SILENT_ABORT_MARKER: &str = "__omp.silent_abort__";

/// Reason threaded through an abort when the user presses Esc.
pub const USER_INTERRUPT_LABEL: &str = "Interrupted by user";

/// Sentinel `error_message` stamped on an abort that carried no custom reason.
pub const GENERIC_ABORT_SENTINEL: &str = "Request was aborted";

/// Advisor note severities, weakest first.
pub const ADVISOR_SEVERITIES: [AdvisorSeverity; 3] = [
    AdvisorSeverity::Nit,
    AdvisorSeverity::Concern,
    AdvisorSeverity::Blocker,
];

/// Advisor note severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdvisorSeverity {
    /// Weakest: a style remark.
    Nit,
    /// A correctness or design concern.
    Concern,
    /// Blocks the work.
    Blocker,
}

impl AdvisorSeverity {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            AdvisorSeverity::Nit => "nit",
            AdvisorSeverity::Concern => "concern",
            AdvisorSeverity::Blocker => "blocker",
        }
    }

    /// Parse a severity; unknown ranks read as [`AdvisorSeverity::Nit`]'s slot
    /// only when `none_if_unknown` is false.
    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "nit" => Some(AdvisorSeverity::Nit),
            "concern" => Some(AdvisorSeverity::Concern),
            "blocker" => Some(AdvisorSeverity::Blocker),
            _ => None,
        }
    }
}

/// One queued advisor advice note.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdvisorNote {
    /// The advice text.
    pub note: String,
    /// Severity rank; absent on notes queued before severities existed.
    pub severity: Option<AdvisorSeverity>,
    /// Which configured advisor produced this note.
    pub advisor: Option<String>,
    /// Primary turns completed when the note was emitted.
    pub turn: Option<u64>,
    /// Primary turns elapsed before merged-batch delivery.
    pub turns_ago: Option<u64>,
}

/// Details payload on the batched `advisor` custom message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdvisorMessageDetails {
    /// The batched notes.
    pub notes: Vec<AdvisorNote>,
}

/// Details persisted on a `/tan` background-dispatch breadcrumb.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackgroundTanDispatchDetails {
    /// Background job id.
    pub job_id: String,
    /// The dispatched work.
    pub work: String,
    /// Forked clone session file, named `<agentId>.jsonl`.
    pub session_file: String,
}

/// A compaction point in the transcript.
///
/// Reference: `CompactionSummaryMessage` from
/// `@oh-my-pi/pi-agent-core/compaction/messages`; that package is not vendored
/// in this tree, so the fields the divider renderer reads live here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompactionSummaryMessage {
    /// The summary that replaced the compacted turns.
    pub summary: String,
    /// Context tokens before compaction.
    pub tokens_before: usize,
    /// Context tokens after compaction; absent on entries predating the field.
    pub tokens_after: Option<usize>,
    /// The maintenance method that fired (`remote`, `soft`, `handoff`,
    /// `snapcompact`, `shake`).
    pub method: Option<String>,
    /// A dead-end warning stamped by the progress guard.
    pub warning: Option<String>,
    /// Snapcompact frame ids attached to the summary.
    pub images: Vec<String>,
}

/// A side-branch collapse folded back into the main line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchSummaryMessage {
    /// The branch summary.
    pub summary: String,
    /// Context tokens before the branch was folded.
    pub tokens_before: Option<usize>,
    /// Context tokens after the fold.
    pub tokens_after: Option<usize>,
}

/// Details persisted on a `/skill:<name>` prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillPromptDetails {
    /// Skill name.
    pub name: String,
    /// Skill file path.
    pub path: String,
    /// Arguments after the skill token.
    pub args: Option<String>,
    /// The draft as submitted with its `/skill:<name>` token in place.
    pub prompt: Option<String>,
    /// Lines the skill body contributes.
    pub line_count: usize,
    /// Internal compact label for a queued custom message; never persisted.
    pub queue_chip_text: Option<String>,
}

/// Who initiated a message, for billing/attribution semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageAttribution {
    /// The user.
    User,
    /// The agent (the default).
    #[default]
    Agent,
}

impl MessageAttribution {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            MessageAttribution::User => "user",
            MessageAttribution::Agent => "agent",
        }
    }
}

/// Why a reply stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished.
    Stop,
    /// The output-token budget ran out.
    Length,
    /// The model asked for tools.
    ToolUse,
    /// The provider reported an error.
    Error,
    /// The turn was aborted.
    Aborted,
}

impl StopReason {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::Stop => "stop",
            StopReason::Length => "length",
            StopReason::ToolUse => "toolUse",
            StopReason::Error => "error",
            StopReason::Aborted => "aborted",
        }
    }
}

/// Token counts for one turn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// Non-cached conversation input tokens.
    pub input: u64,
    /// Total conversation output tokens, thinking included.
    pub output: u64,
    /// Conversation tokens read from the prompt cache.
    pub cache_read: u64,
    /// Conversation tokens written to the prompt cache.
    pub cache_write: u64,
    /// Sum of every bucket, plus provider orchestration tokens when reported.
    pub total_tokens: u64,
    /// Reasoning tokens included in `output`, when the provider reports them.
    pub reasoning_tokens: Option<u64>,
}

/// How a turn recovered from a transient failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistantRetryRecovery {
    /// `"recovered"` or `"superseded"`.
    pub status: String,
    /// One-based attempt number.
    pub attempt: u64,
    /// The recovery class, e.g. `"retry"`.
    pub recovery: String,
    /// Human-readable note shown beside the reminder.
    pub note: String,
    /// For a recovered turn: the reply that replaced it.
    pub superseded_by: Option<SupersededReply>,
}

/// Identity of the reply that replaced a recovered one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupersededReply {
    /// The replacing reply's timestamp.
    pub timestamp: i64,
    /// The replacing reply's response id.
    pub response_id: Option<String>,
    /// The replacing reply's provider.
    pub provider: String,
    /// The replacing reply's model.
    pub model: String,
}

/// A text block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextContent {
    /// The text.
    pub text: String,
    /// Provider metadata id (legacy string or a JSON signature object).
    pub text_signature: Option<String>,
}

/// A thinking block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThinkingContent {
    /// The reasoning text.
    pub thinking: String,
    /// Provider signature for the block.
    pub thinking_signature: Option<String>,
    /// `output_item.added` item id.
    pub item_id: Option<String>,
    /// `Some(true)`: a provider-written summary; `Some(false)`: the reasoning
    /// itself; `None`: provenance unknown.
    pub summary: Option<bool>,
    /// Prose the provider returned as a signed thinking block.
    pub narration: Option<bool>,
}

/// An image block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageContent {
    /// Base64-encoded image bytes.
    pub data: String,
    /// MIME type, e.g. `image/png`.
    pub mime_type: String,
    /// OpenAI-only resolution hint.
    pub detail: Option<String>,
    /// Optional https mirror of `data`.
    pub url: Option<String>,
}

/// A model tool call.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolCall {
    /// Provider call id, echoed by the tool result.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Parsed arguments.
    pub arguments: Value,
    /// Google-specific opaque signature.
    pub thought_signature: Option<String>,
    /// Harness-level intent metadata.
    pub intent: Option<String>,
    /// Verbatim in-band syntax block that produced a synthetic call.
    pub raw_block: Option<String>,
    /// Original wire name for OpenAI custom tools.
    pub custom_wire_name: Option<String>,
    /// Responses API namespace.
    pub namespace: Option<String>,
}

/// One block of an assistant reply.
#[derive(Clone, Debug, PartialEq)]
pub enum AssistantContent {
    /// Text.
    Text(TextContent),
    /// Reasoning.
    Thinking(ThinkingContent),
    /// Encrypted reasoning the provider must echo back.
    RedactedThinking(String),
    /// Anthropic server-side fallback boundary.
    Fallback {
        /// Model the response switched away from.
        from: String,
        /// Model the response switched to.
        to: String,
    },
    /// Verbatim Anthropic web-search or tool-search call/result.
    ServerTool(Value),
    /// An image the model produced.
    Image(ImageContent),
    /// A tool call.
    ToolCall(ToolCall),
}

impl AssistantContent {
    /// The text of a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            AssistantContent::Text(text) => Some(&text.text),
            _ => None,
        }
    }

    /// The tool call of a call block.
    pub fn as_tool_call(&self) -> Option<&ToolCall> {
        match self {
            AssistantContent::ToolCall(call) => Some(call),
            _ => None,
        }
    }
}

/// One assistant reply.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistantMessage {
    /// Content blocks, in wire order.
    pub content: Vec<AssistantContent>,
    /// API id of the provider client that produced the turn.
    pub api: String,
    /// Configured gateway that was called.
    pub provider: String,
    /// Requested model id.
    pub model: String,
    /// Token counts.
    pub usage: Usage,
    /// Why the turn stopped.
    pub stop_reason: StopReason,
    /// Failure text; also carries a custom abort reason.
    pub error_message: Option<String>,
    /// Structured error classifier bits; see [`AiErrorFlag`].
    pub error_id: Option<u32>,
    /// Wall-clock timestamp in milliseconds.
    pub timestamp: i64,
    /// Provider response id.
    pub response_id: Option<String>,
    /// Upstream provider an aggregator routed to.
    pub upstream_provider: Option<String>,
    /// Concrete model that produced the turn, when reported.
    pub upstream_model: Option<String>,
    /// Retry bookkeeping for a transient failure.
    pub retry_recovery: Option<AssistantRetryRecovery>,
    /// Visible output tokens a history rewrite removed.
    pub elided_output_tokens: Option<u64>,
}

impl Default for AssistantMessage {
    fn default() -> Self {
        AssistantMessage {
            content: Vec::new(),
            api: String::new(),
            provider: String::new(),
            model: String::new(),
            usage: Usage::default(),
            stop_reason: StopReason::Stop,
            error_message: None,
            error_id: None,
            timestamp: 0,
            response_id: None,
            upstream_provider: None,
            upstream_model: None,
            retry_recovery: None,
            elided_output_tokens: None,
        }
    }
}

/// Content accepted for extension-injected messages.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomMessageContent {
    /// A bare string.
    Text(String),
    /// Text and image blocks.
    Blocks(Vec<MessageBlock>),
}

impl Default for CustomMessageContent {
    fn default() -> Self {
        CustomMessageContent::Text(String::new())
    }
}

impl CustomMessageContent {
    /// The content as plain text: the string itself, or every text block joined.
    pub fn plain_text(&self) -> String {
        match self {
            CustomMessageContent::Text(text) => text.clone(),
            CustomMessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|block| match block {
                    MessageBlock::Text(text) => Some(text.text.as_str()),
                    MessageBlock::Image(_) => None,
                })
                .collect(),
        }
    }

    /// Whether the content carries nothing.
    pub fn is_empty(&self) -> bool {
        match self {
            CustomMessageContent::Text(text) => text.is_empty(),
            CustomMessageContent::Blocks(blocks) => blocks.is_empty(),
        }
    }
}

/// One block of custom-message content.
#[derive(Clone, Debug, PartialEq)]
pub enum MessageBlock {
    /// Text block.
    Text(TextContent),
    /// Image block.
    Image(ImageContent),
}

/// A message injected by an extension through `sendMessage()`.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomMessage {
    /// Custom type tag; see the `*_MESSAGE_TYPE` constants.
    pub custom_type: String,
    /// Message content.
    pub content: CustomMessageContent,
    /// Whether the transcript draws the message.
    pub display: bool,
    /// Type-specific details.
    pub details: Option<Value>,
    /// Who initiated the message.
    pub attribution: MessageAttribution,
    /// Wall-clock timestamp in milliseconds.
    pub timestamp: i64,
}

/// Input accepted by `sendMessage()` and `sendCustomMessage`.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomMessagePayload {
    /// A bare string is the content of a default-typed message.
    Text(String),
    /// The reference's partial-object overload.
    Object(CustomMessageFields),
    /// Any other value: `None` is `undefined`/`null`, `Some` is converted with
    /// JavaScript `String(value)`.
    Other(Option<Value>),
}

/// The object overload of [`CustomMessagePayload`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomMessageFields {
    /// Custom type tag.
    pub custom_type: Option<String>,
    /// Content; non-string, non-array values normalize to empty.
    pub content: Option<CustomMessageContent>,
    /// Whether to display the message.
    pub display: Option<bool>,
    /// Type-specific details.
    pub details: Option<Value>,
    /// Who initiated the message.
    pub attribution: Option<String>,
}

/// A payload after runtime defaults are applied.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedCustomMessagePayload {
    /// Custom type tag, never empty.
    pub custom_type: String,
    /// Message content, never absent.
    pub content: CustomMessageContent,
    /// Whether to display the message.
    pub display: bool,
    /// Type-specific details.
    pub details: Option<Value>,
    /// Who initiated the message.
    pub attribution: MessageAttribution,
}

/// A legacy (pre-extensions) hook message. Kept for session migration.
#[derive(Clone, Debug, PartialEq)]
pub struct HookMessage {
    /// Custom type tag.
    pub custom_type: String,
    /// Message content.
    pub content: CustomMessageContent,
    /// Whether the transcript draws the message.
    pub display: bool,
    /// Type-specific details.
    pub details: Option<Value>,
    /// Who initiated the message.
    pub attribution: MessageAttribution,
    /// Wall-clock timestamp in milliseconds.
    pub timestamp: i64,
}

/// A `!` shell execution recorded in the transcript.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BashExecutionMessage {
    /// The command line.
    pub command: String,
    /// Captured output.
    pub output: String,
    /// Exit code; absent when the run did not finish.
    pub exit_code: Option<i32>,
    /// The run was cancelled.
    pub cancelled: bool,
    /// The captured output was truncated.
    pub truncated: bool,
    /// Images extracted from terminal graphics in the output.
    pub images: Vec<ImageContent>,
    /// Output metadata.
    pub meta: Option<OutputMeta>,
    /// Wall-clock timestamp in milliseconds.
    pub timestamp: i64,
    /// Excluded from model context (`!!` prefix).
    pub exclude_from_context: bool,
}

/// A `$` Python execution recorded in the transcript.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PythonExecutionMessage {
    /// The code.
    pub code: String,
    /// Captured output.
    pub output: String,
    /// Exit code; absent when the run did not finish.
    pub exit_code: Option<i32>,
    /// The run was cancelled.
    pub cancelled: bool,
    /// The captured output was truncated.
    pub truncated: bool,
    /// Output metadata.
    pub meta: Option<OutputMeta>,
    /// Wall-clock timestamp in milliseconds.
    pub timestamp: i64,
    /// Excluded from model context (`$$` prefix).
    pub exclude_from_context: bool,
}

/// One file auto-read by an `@filepath` mention.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MentionedFile {
    /// Path as typed.
    pub path: String,
    /// File contents.
    pub content: String,
    /// Line count, when known.
    pub line_count: Option<usize>,
    /// File size in bytes, when known.
    pub byte_size: Option<u64>,
    /// Why the contents were omitted.
    pub skipped_reason: Option<String>,
    /// An image read.
    pub image: Option<ImageContent>,
}

/// A message recording auto-read file mentions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileMentionMessage {
    /// The mentioned files.
    pub files: Vec<MentionedFile>,
    /// Wall-clock timestamp in milliseconds.
    pub timestamp: i64,
}

/// A half-open 1-indexed line range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    /// First line.
    pub start: u64,
    /// Last line, inclusive.
    pub end: u64,
}

/// Truncation metadata for an output notice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TruncationMeta {
    /// `"head"`, `"tail"` or `"middle"`.
    pub direction: String,
    /// `"lines"`, `"bytes"` or `"middle"`.
    pub truncated_by: String,
    /// Lines in the full output.
    pub total_lines: u64,
    /// Bytes in the full output.
    pub total_bytes: u64,
    /// Lines shown.
    pub output_lines: u64,
    /// Bytes shown.
    pub output_bytes: u64,
    /// Byte cap that caused the cut.
    pub max_bytes: Option<u64>,
    /// Line range shown, 1-indexed and inclusive.
    pub shown_range: Option<LineRange>,
    /// Head range shown when `direction` is `middle`.
    pub head_range: Option<LineRange>,
    /// Tail range shown when `direction` is `middle`.
    pub tail_range: Option<LineRange>,
    /// Bytes elided from the middle.
    pub elided_bytes: Option<u64>,
    /// Lines elided from the middle.
    pub elided_lines: Option<u64>,
    /// Artifact holding the full output.
    pub artifact_id: Option<String>,
    /// Bytes the artifact cap dropped.
    pub artifact_elided_bytes: Option<u64>,
    /// Next pagination offset (head truncation only).
    pub next_offset: Option<u64>,
    /// The single shown line is a byte-capped preview of an oversized line.
    pub partial_line: bool,
}

/// Where an output's contents came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceMeta {
    /// A file path.
    Path(String),
    /// A URL.
    Url(String),
    /// An internal resource.
    Internal(String),
    /// A complete aggregate report.
    Report(String),
}

impl SourceMeta {
    /// `"path"`, `"url"`, `"internal"` or `"report"`.
    pub fn kind(&self) -> &'static str {
        match self {
            SourceMeta::Path(_) => "path",
            SourceMeta::Url(_) => "url",
            SourceMeta::Internal(_) => "internal",
            SourceMeta::Report(_) => "report",
        }
    }

    /// The source value.
    pub fn value(&self) -> &str {
        match self {
            SourceMeta::Path(value)
            | SourceMeta::Url(value)
            | SourceMeta::Internal(value)
            | SourceMeta::Report(value) => value,
        }
    }
}

/// LSP diagnostics attached to an edit/write output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticMeta {
    /// One-line summary.
    pub summary: String,
    /// Pre-formatted diagnostic messages.
    pub messages: Vec<String>,
}

/// A reached limit with the value the tool would accept next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitReached {
    /// The limit that was hit.
    pub reached: u64,
    /// A larger usable limit, when one exists.
    pub suggestion: Option<u64>,
}

/// Limit-specific notices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LimitsMeta {
    /// The match limit was reached.
    pub match_limit: Option<LimitReached>,
    /// The result limit was reached.
    pub result_limit: Option<LimitReached>,
    /// The head limit was reached.
    pub head_limit: Option<LimitReached>,
    /// A column was truncated.
    pub column_truncated: Option<ColumnTruncated>,
}

/// The lines cut off by a column cap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnTruncated {
    /// Widest column kept.
    pub max_column: u64,
    /// `"bytes"` or `"chars"`.
    pub unit: Option<String>,
    /// Artifact holding the full lines.
    pub artifact_id: Option<String>,
    /// Bytes the artifact cap dropped.
    pub artifact_elided_bytes: Option<u64>,
    /// First-to-last range of cut lines.
    pub lines: Option<LineRange>,
    /// Target a `:raw:<line>-<line>` selector reads from.
    pub selector_base: Option<String>,
    /// Byte cap of one `:raw` read.
    pub raw_byte_cap: Option<u64>,
}

/// The bash minimizer rewrote this output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MinimizedMeta {
    /// Native filter label (`chain` for a minimized `&&`/`;` chain).
    pub filter: String,
    /// Raw capture bytes.
    pub input_bytes: u64,
    /// Minimized text bytes.
    pub output_bytes: u64,
    /// Artifact holding the raw capture.
    pub artifact_id: String,
}

/// Structured metadata for tool outputs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputMeta {
    /// How the output was cut.
    pub truncation: Option<TruncationMeta>,
    /// Capture failure of this output itself.
    pub artifact_error: Option<String>,
    /// Where the output came from.
    pub source: Option<SourceMeta>,
    /// The output is a bounded page of the artifact `source` names.
    pub paged_source: bool,
    /// LSP diagnostics.
    pub diagnostics: Option<DiagnosticMeta>,
    /// Reached limits.
    pub limits: Option<LimitsMeta>,
    /// Bash-minimizer rewrite.
    pub minimized: Option<MinimizedMeta>,
}

/// Structured error classifier bits, from `@oh-my-pi/pi-ai` `error/flags.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiErrorFlag {
    /// The id carries a classified kind at all.
    Class,
    /// The model repeated one reasoning thought.
    ThinkingLoop,
    /// A retriable provider failure.
    Transient,
    /// A request timed out.
    Timeout,
    /// A usage limit was hit.
    UsageLimit,
    /// A stale Responses item.
    StaleResponsesItem,
    /// A malformed function call.
    MalformedFunctionCall,
    /// The provider reported a finish error.
    ProviderFinishError,
    /// The provider returned no content.
    EmptyResponse,
    /// Content was blocked by policy.
    ContentBlocked,
    /// Account-scoped provider policy denial.
    AccountPolicy,
    /// The context overflowed.
    ContextOverflow,
    /// Authentication failed.
    AuthFailed,
    /// An expected internal abort: keep the reason quiet.
    SilentAbort,
    /// The user interrupted the turn.
    UserInterrupt,
    /// The turn was aborted.
    Abort,
    /// A strict-tool rejection (grammar or schema).
    Grammar,
    /// The model or account does not support fast mode.
    FastModeUnsupported,
    /// OAuth refresh failed definitively.
    OAuthExpiry,
}

impl AiErrorFlag {
    /// The flag's bit.
    pub fn mask(self) -> u32 {
        match self {
            AiErrorFlag::Class => 0x1000,
            AiErrorFlag::ThinkingLoop => 0x0001_0000,
            AiErrorFlag::Transient => 0x0002_0000,
            AiErrorFlag::Timeout => 0x0004_0000,
            AiErrorFlag::UsageLimit => 0x0008_0000,
            AiErrorFlag::StaleResponsesItem => 0x0010_0000,
            AiErrorFlag::MalformedFunctionCall => 0x0020_0000,
            AiErrorFlag::ProviderFinishError => 0x0040_0000,
            AiErrorFlag::EmptyResponse => 0x0000_2000,
            AiErrorFlag::ContentBlocked => 0x0000_8000,
            AiErrorFlag::AccountPolicy => 0x0000_4000,
            AiErrorFlag::ContextOverflow => 0x0080_0000,
            AiErrorFlag::AuthFailed => 0x0100_0000,
            AiErrorFlag::SilentAbort => 0x0200_0000,
            AiErrorFlag::UserInterrupt => 0x0400_0000,
            AiErrorFlag::Abort => 0x0800_0000,
            AiErrorFlag::Grammar => 0x1000_0000,
            AiErrorFlag::FastModeUnsupported => 0x2000_0000,
            AiErrorFlag::OAuthExpiry => 0x4000_0000,
        }
    }
}

/// Build an error id from `flags`, always including the class bit.
pub fn ai_error_create(flags: &[AiErrorFlag]) -> u32 {
    let mut bits = 0;
    for flag in flags {
        bits |= flag.mask();
    }
    bits | AiErrorFlag::Class.mask()
}

/// Whether `error_id` carries `flag`.
pub fn ai_error_is(error_id: Option<u32>, flag: AiErrorFlag) -> bool {
    (error_id.unwrap_or(0) & flag.mask()) != 0
}

/// The parts of a reply the abort-reason helpers read.
pub trait AbortReasonMessage {
    /// Structured classifier.
    fn error_id(&self) -> Option<u32>;
    /// Failure text, which also carries a custom abort reason.
    fn error_message(&self) -> Option<&str>;
}

impl AbortReasonMessage for AssistantMessage {
    fn error_id(&self) -> Option<u32> {
        self.error_id
    }

    fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }
}

/// Whether the abort was an expected internal transition.
pub fn is_silent_abort(message: &impl AbortReasonMessage) -> bool {
    ai_error_is(message.error_id(), AiErrorFlag::SilentAbort)
        || message.error_message() == Some(SILENT_ABORT_MARKER)
}

/// Whether the user pressed Esc to abort the turn.
pub fn is_user_interrupt_abort(message: &impl AbortReasonMessage) -> bool {
    ai_error_is(message.error_id(), AiErrorFlag::UserInterrupt)
        || message.error_message() == Some(USER_INTERRUPT_LABEL)
}

/// Whether the reply's abort reason deserves a transcript line.
pub fn should_render_abort_reason(message: &impl AbortReasonMessage) -> bool {
    !is_silent_abort(message) && !is_user_interrupt_abort(message)
}

/// The operator-facing label for an aborted turn.
pub fn resolve_abort_label(message: &impl AbortReasonMessage, retry_attempt: u32) -> String {
    let error_message = message.error_message();
    let generic_abort = ai_error_is(message.error_id(), AiErrorFlag::Abort)
        || error_message.is_none_or(str::is_empty)
        || error_message == Some(GENERIC_ABORT_SENTINEL)
        || is_silent_abort(message);
    if !generic_abort {
        return error_message.unwrap_or_default().to_string();
    }
    if retry_attempt > 0 {
        let plural = if retry_attempt > 1 { "s" } else { "" };
        return format!("Aborted after {retry_attempt} retry attempt{plural}");
    }
    "Operation aborted".to_string()
}

/// Whether a persisted or extension-supplied value can be custom-message
/// content.
pub fn is_custom_message_content(value: &Value) -> bool {
    value.is_string() || value.is_array()
}

/// Normalize extension-provided custom message input.
pub fn normalize_custom_message_payload(
    payload: CustomMessagePayload,
) -> NormalizedCustomMessagePayload {
    match payload {
        CustomMessagePayload::Text(content) => NormalizedCustomMessagePayload {
            custom_type: DEFAULT_CUSTOM_MESSAGE_TYPE.to_string(),
            content: CustomMessageContent::Text(content),
            display: true,
            details: None,
            attribution: MessageAttribution::Agent,
        },
        CustomMessagePayload::Other(value) => {
            let content = match &value {
                None => String::new(),
                Some(value) => js_string(value),
            };
            let empty = content.is_empty();
            NormalizedCustomMessagePayload {
                custom_type: DEFAULT_CUSTOM_MESSAGE_TYPE.to_string(),
                content: CustomMessageContent::Text(content),
                display: !empty,
                details: None,
                attribution: MessageAttribution::Agent,
            }
        }
        CustomMessagePayload::Object(fields) => NormalizedCustomMessagePayload {
            custom_type: fields
                .custom_type
                .filter(|custom_type| !custom_type.is_empty())
                .unwrap_or_else(|| DEFAULT_CUSTOM_MESSAGE_TYPE.to_string()),
            content: fields
                .content
                .unwrap_or_else(|| CustomMessageContent::Text(String::new())),
            display: fields.display.unwrap_or(false),
            details: fields.details,
            attribution: match fields.attribution.as_deref() {
                Some("user") => MessageAttribution::User,
                _ => MessageAttribution::Agent,
            },
        },
    }
}

/// JavaScript `String(value)` for the values a JSON payload can hold.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// True for a `/skill:<name>` prompt the user invoked directly.
pub fn is_user_invoked_skill_prompt(message: &CustomMessage) -> bool {
    message.custom_type == SKILL_PROMPT_MESSAGE_TYPE
        && message.attribution == MessageAttribution::User
}

/// True for a custom message that initiates a user-attributed turn.
pub fn is_user_turn_initiator(message: &CustomMessage) -> bool {
    is_user_invoked_skill_prompt(message)
        || (message.custom_type == COLLAB_PROMPT_MESSAGE_TYPE
            && message.attribution == MessageAttribution::User)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assistant(error_id: Option<u32>, error_message: Option<&str>) -> AssistantMessage {
        AssistantMessage {
            error_id,
            error_message: error_message.map(str::to_string),
            ..AssistantMessage::default()
        }
    }

    #[test]
    fn error_flags_match_the_reference_bit_layout() {
        let cases: [(AiErrorFlag, u32); 6] = [
            (AiErrorFlag::Class, 0x1000),
            (AiErrorFlag::SilentAbort, 0x0200_0000),
            (AiErrorFlag::UserInterrupt, 0x0400_0000),
            (AiErrorFlag::Abort, 0x0800_0000),
            (AiErrorFlag::Transient, 0x0002_0000),
            (AiErrorFlag::ContextOverflow, 0x0080_0000),
        ];
        for (flag, mask) in cases {
            assert_eq!(flag.mask(), mask, "{flag:?}");
        }
    }

    #[test]
    fn created_ids_carry_the_class_bit_and_their_flags() {
        let id = ai_error_create(&[AiErrorFlag::SilentAbort]);
        assert_eq!(id & AiErrorFlag::Class.mask(), AiErrorFlag::Class.mask());
        assert!(ai_error_is(Some(id), AiErrorFlag::SilentAbort));
        assert!(!ai_error_is(Some(id), AiErrorFlag::UserInterrupt));
        assert!(!ai_error_is(None, AiErrorFlag::SilentAbort));
    }

    #[test]
    fn abort_predicates_follow_flag_or_marker() {
        let silent = assistant(
            Some(ai_error_create(&[AiErrorFlag::SilentAbort])),
            Some("plan mode handoff"),
        );
        assert!(is_silent_abort(&silent));
        assert!(!should_render_abort_reason(&silent));

        let legacy_marker = assistant(None, Some(SILENT_ABORT_MARKER));
        assert!(is_silent_abort(&legacy_marker));

        let interrupt = assistant(
            Some(ai_error_create(&[AiErrorFlag::UserInterrupt])),
            None,
        );
        assert!(is_user_interrupt_abort(&interrupt));
        assert!(!should_render_abort_reason(&interrupt));

        let plain = assistant(None, Some("network down"));
        assert!(should_render_abort_reason(&plain));
    }

    #[test]
    fn abort_labels_cover_generic_retry_and_custom_reasons() {
        let cases: [(Option<u32>, Option<&str>, u32, &str); 6] = [
            (None, Some("network down"), 0, "network down"),
            (None, Some(GENERIC_ABORT_SENTINEL), 0, "Operation aborted"),
            (None, None, 0, "Operation aborted"),
            (None, None, 1, "Aborted after 1 retry attempt"),
            (None, None, 3, "Aborted after 3 retry attempts"),
            (
                Some(ai_error_create(&[AiErrorFlag::Abort])),
                Some("Request was aborted"),
                0,
                "Operation aborted",
            ),
        ];
        for (error_id, error_message, attempt, expected) in cases {
            let message = assistant(error_id, error_message);
            assert_eq!(
                resolve_abort_label(&message, attempt),
                expected,
                "{error_message:?} attempt {attempt}"
            );
        }
    }

    #[test]
    fn normalize_covers_the_string_object_and_unknown_overloads() {
        assert_eq!(
            normalize_custom_message_payload(CustomMessagePayload::Text("hello".into())),
            NormalizedCustomMessagePayload {
                custom_type: DEFAULT_CUSTOM_MESSAGE_TYPE.to_string(),
                content: CustomMessageContent::Text("hello".into()),
                display: true,
                details: None,
                attribution: MessageAttribution::Agent,
            }
        );

        let normalized = normalize_custom_message_payload(CustomMessagePayload::Object(
            CustomMessageFields {
                custom_type: Some("skill-prompt".into()),
                content: Some(CustomMessageContent::Text("body".into())),
                display: None,
                details: Some(json!({"name": "x"})),
                attribution: Some("user".into()),
            },
        ));
        assert_eq!(normalized.custom_type, SKILL_PROMPT_MESSAGE_TYPE);
        assert!(!normalized.display);
        assert_eq!(normalized.attribution, MessageAttribution::User);
        assert_eq!(normalized.details, Some(json!({"name": "x"})));

        let empty_type = normalize_custom_message_payload(CustomMessagePayload::Object(
            CustomMessageFields {
                custom_type: Some(String::new()),
                ..Default::default()
            },
        ));
        assert_eq!(empty_type.custom_type, DEFAULT_CUSTOM_MESSAGE_TYPE);
        assert!(empty_type.content.is_empty());

        let unknown = normalize_custom_message_payload(CustomMessagePayload::Other(Some(json!(42))));
        assert_eq!(unknown.content, CustomMessageContent::Text("42".into()));
        assert!(unknown.display);

        let null = normalize_custom_message_payload(CustomMessagePayload::Other(None));
        assert_eq!(null.content, CustomMessageContent::Text(String::new()));
        assert!(!null.display);
    }

    #[test]
    fn js_string_joins_arrays_and_flattens_objects() {
        assert_eq!(js_string(&json!(["a", 1, true])), "a,1,true");
        assert_eq!(js_string(&json!({"a": 1})), "[object Object]");
        assert_eq!(js_string(&Value::Null), "null");
    }

    #[test]
    fn user_turn_initiators_need_user_attribution() {
        let skill = CustomMessage {
            custom_type: SKILL_PROMPT_MESSAGE_TYPE.to_string(),
            content: CustomMessageContent::default(),
            display: true,
            details: None,
            attribution: MessageAttribution::User,
            timestamp: 0,
        };
        assert!(is_user_invoked_skill_prompt(&skill));
        assert!(is_user_turn_initiator(&skill));

        let agent_skill = CustomMessage {
            attribution: MessageAttribution::Agent,
            ..skill.clone()
        };
        assert!(!is_user_invoked_skill_prompt(&agent_skill));
        assert!(!is_user_turn_initiator(&agent_skill));

        let collab = CustomMessage {
            custom_type: COLLAB_PROMPT_MESSAGE_TYPE.to_string(),
            attribution: MessageAttribution::User,
            ..skill.clone()
        };
        assert!(is_user_turn_initiator(&collab));

        let reminder = CustomMessage {
            custom_type: "todo-reminder".to_string(),
            attribution: MessageAttribution::User,
            ..skill
        };
        assert!(!is_user_turn_initiator(&reminder));
    }

    #[test]
    fn advisor_severities_rank_weakest_first() {
        assert!(AdvisorSeverity::Nit < AdvisorSeverity::Concern);
        assert!(AdvisorSeverity::Concern < AdvisorSeverity::Blocker);
        assert_eq!(
            ADVISOR_SEVERITIES
                .iter()
                .map(|severity| severity.as_str())
                .collect::<Vec<_>>(),
            ["nit", "concern", "blocker"]
        );
        assert_eq!(AdvisorSeverity::from_str("blocker"), Some(AdvisorSeverity::Blocker));
        assert_eq!(AdvisorSeverity::from_str("fatal"), None);
    }

    #[test]
    fn custom_message_content_plain_text_skips_images() {
        let content = CustomMessageContent::Blocks(vec![
            MessageBlock::Text(TextContent {
                text: "one ".into(),
                text_signature: None,
            }),
            MessageBlock::Image(ImageContent {
                data: "aGk=".into(),
                mime_type: "image/png".into(),
                ..ImageContent::default()
            }),
            MessageBlock::Text(TextContent {
                text: "two".into(),
                text_signature: None,
            }),
        ]);
        assert_eq!(content.plain_text(), "one two");
        assert!(!content.is_empty());
        assert!(CustomMessageContent::Text(String::new()).is_empty());
    }
}
