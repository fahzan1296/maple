//! Render helpers shared between the live transcript and the file/remote-backed
//! transcript builder. Both surfaces build the same transcript rows from
//! persisted message entries; holding the row construction here keeps the two
//! byte-for-byte identical.
//!
//! Reference: `l/src/chat/transcript-render-helpers.ts`.
//!
//! Deviations:
//! - The reference resolves colours and glyphs from the process-global `theme`
//!   (`theme.status.success`); maple-tui has no global theme, so the row
//!   builders take a `&Theme`, and tone glyphs are dropped (the notice row's
//!   tone already carries them natively).
//! - `formatArtifactErrorNotice`/`formatMinimizedMeta` are not ported here;
//!   [`OutputMeta`] keeps the artifact failure as plain text and
//!   [`minimized_row`] formats [`MinimizedMeta`] itself.
//! - `buildAsyncResultComponent`, `buildIrcMessageCard` and
//!   `buildParentIrcMessageCard` need the extension-renderer and IRC-card
//!   ports, which this crate does not have yet, so only the built-in block
//!   builders landed.

use crate::chat::messages::{
    resolve_abort_label, should_render_abort_reason, MentionedFile, MinimizedMeta, OutputMeta,
};
use crate::chat::thinking_display::canonicalize_message;
use crate::chat::transcript_entry::AgentMessage;
use crate::chat::messages::{AssistantContent, AssistantMessage, Usage};
use crate::chrome::transcript_notice::NoticeTone;
use crate::chrome::transcript_status::{
    TranscriptStatusBlock, TranscriptStatusNative, TranscriptStatusPart, TranscriptStatusRow,
};
use crate::native::describe::span;
use crate::native::node::Span;
use crate::render::render_utils::format_styled_path;
use crate::status_line::segments::format_duration;
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::theme::Theme;
use crate::tools::daemon::{DaemonSnapshot, DaemonState};
use crate::tools::output_meta::format_bytes;

/// Widest an error note may render, in cells.
const NOTE_WIDTH: usize = 120;

/// One background job in an `async-result` message's details.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AsyncResultJob {
    /// Job id.
    pub job_id: Option<String>,
    /// Job type (`bash`, `task`, …).
    pub job_type: Option<String>,
    /// Short label.
    pub label: Option<String>,
    /// Wall-clock duration.
    pub duration_ms: Option<f64>,
    /// The job's output metadata.
    pub meta: Option<OutputMeta>,
}

/// An `async-result` custom message's details: one job, or a batch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AsyncResultDetails {
    /// Single-job fields.
    pub job_id: Option<String>,
    /// Single-job type.
    pub job_type: Option<String>,
    /// Single-job label.
    pub label: Option<String>,
    /// Single-job duration.
    pub duration_ms: Option<f64>,
    /// A batch of completed jobs.
    pub jobs: Vec<AsyncResultJob>,
    /// Metadata for the whole message.
    pub meta: Option<OutputMeta>,
}

impl AsyncResultDetails {
    /// The jobs to render: the batch when present, else the single-job fields
    /// as one job.
    pub fn jobs(&self) -> Vec<AsyncResultJob> {
        if !self.jobs.is_empty() {
            return self.jobs.clone();
        }
        vec![AsyncResultJob {
            job_id: self.job_id.clone(),
            job_type: self.job_type.clone(),
            label: self.label.clone(),
            duration_ms: self.duration_ms,
            meta: None,
        }]
    }
}

/// Render an `async-result` custom message (a completed background bash/task
/// job, or a batch of them) as a transcript block of one "Background job
/// completed" row per job.
pub fn build_async_result_block(theme: &Theme, details: &AsyncResultDetails) -> TranscriptStatusBlock {
    let mut rows = Vec::new();
    for job in details.jobs() {
        let job_id = job.job_id.clone().unwrap_or_else(|| "unknown".to_string());
        let job_type = job.job_type.clone().unwrap_or_else(|| "job".to_string());
        let duration = job
            .duration_ms
            .filter(|ms| ms.is_finite() && *ms >= 0.0)
            .map(|ms| format_duration(ms.round() as u64));
        let mut message = vec![span("Background job completed", None)];
        message.extend(facts(&[Some(&job_type), Some(&job_id), duration.as_deref()]));
        let mut parts = vec![
            Some(part(" Background job completed", "success")),
            Some(part(&format!("[{job_type}]"), "dim")),
            Some(part(&job_id, "accent")),
        ];
        if let Some(duration) = &duration {
            parts.push(Some(part(&format!("({duration})"), "dim")));
        }
        rows.push(TranscriptStatusRow {
            parts,
            indent: None,
            native: TranscriptStatusNative {
                tone: NoticeTone::Success,
                icon: None,
                message,
            },
        });
        if let Some(meta) = &job.meta {
            if let Some(error) = &meta.artifact_error {
                rows.push(artifact_error_row(error));
            }
            if let Some(minimized) = &meta.minimized {
                rows.push(minimized_row(minimized));
            }
        }
    }
    if let Some(error) = details.meta.as_ref().and_then(|meta| meta.artifact_error.as_ref()) {
        rows.push(artifact_error_row(error));
    }
    TranscriptStatusBlock::new(theme, &rows)
}

/// The warning row under a background job whose output artifact could not be
/// saved.
fn artifact_error_row(error: &str) -> TranscriptStatusRow {
    TranscriptStatusRow {
        parts: vec![Some(part(error, "warning"))],
        indent: None,
        native: TranscriptStatusNative {
            tone: NoticeTone::Warning,
            icon: None,
            message: vec![span(error, None)],
        },
    }
}

/// The dim row under a background job whose output the minimizer shortened.
fn minimized_row(minimized: &MinimizedMeta) -> TranscriptStatusRow {
    let fact = format!(
        "Minimized: {} {} → {}",
        minimized.filter,
        format_bytes(minimized.input_bytes as usize),
        format_bytes(minimized.output_bytes as usize)
    );
    TranscriptStatusRow {
        parts: vec![Some(part(&fact, "dim"))],
        indent: None,
        native: TranscriptStatusNative {
            tone: NoticeTone::Info,
            icon: None,
            message: vec![span(&fact, Some("dim"))],
        },
    }
}

/// Native notice spans for a row's trailing facts (kind, id, duration): each
/// after a dim ` · `, absent ones skipped.
fn facts(values: &[Option<&str>]) -> Vec<Span> {
    let mut spans = Vec::new();
    for value in values.iter().flatten() {
        if value.is_empty() {
            continue;
        }
        spans.push(span(" · ", Some("dim")));
        spans.push(span(*value, Some("dim")));
    }
    spans
}

/// Render a `launch-completion` custom message (terminal supervised-process
/// exits from the launch broker) as a transcript block of one compact
/// "Supervised process ..." row per daemon, matching background-job rows.
pub fn build_launch_completion_block(
    theme: &Theme,
    content: Option<&str>,
    daemons: &[DaemonSnapshot],
) -> TranscriptStatusBlock {
    let mut rows = Vec::new();
    if daemons.is_empty() {
        if let Some(content) = content {
            rows.push(TranscriptStatusRow {
                parts: vec![Some(part(content, "dim"))],
                indent: None,
                native: TranscriptStatusNative {
                    tone: NoticeTone::Success,
                    icon: None,
                    message: vec![span(content, None)],
                },
            });
        }
    }
    for daemon in daemons {
        let failed =
            daemon.state == DaemonState::Failed || daemon.exit_code.is_some_and(|code| code != 0);
        let label = if failed {
            "Supervised process failed"
        } else {
            "Supervised process completed"
        };
        let exit = daemon.exit_code.map(|code| format!("exit {code}"));
        let duration = daemon
            .exited_at
            .filter(|exited| *exited >= daemon.started_at)
            .map(|exited| format_duration(((exited - daemon.started_at) * 1000.0).round() as u64));
        let mut parts = vec![Some(part(label, if failed { "error" } else { "success" }))];
        parts.push(Some(part(&daemon.name, "accent")));
        if let Some(exit) = &exit {
            parts.push(Some(part(&format!("({exit})"), "dim")));
        }
        if let Some(duration) = &duration {
            parts.push(Some(part(&format!("({duration})"), "dim")));
        }
        let mut message = vec![span(label, None), span(" ", None), span(&daemon.name, Some("mono"))];
        message.extend(facts(&[exit.as_deref(), duration.as_deref()]));
        rows.push(TranscriptStatusRow {
            parts,
            indent: None,
            native: TranscriptStatusNative {
                tone: if failed {
                    NoticeTone::Error
                } else {
                    NoticeTone::Success
                },
                icon: None,
                message,
            },
        });
    }
    TranscriptStatusBlock::new(theme, &rows)
}

/// Render a `fileMention` message's files as a transcript block of "Read
/// <path>" rows. `indent` sets the left pad: the live chat renders within an
/// outer gutter (0), the transcript viewer renders body rows without one so
/// rows own their pad (1).
pub fn build_file_mention_block(
    theme: &Theme,
    files: &[MentionedFile],
    indent: u16,
) -> TranscriptStatusBlock {
    let mut rows = Vec::with_capacity(files.len());
    for file in files {
        let detail = file_detail(file);
        let path = file.path.as_str();
        let mut message = vec![span("Read", None), span(" ", None), span(path, Some("path"))];
        message.extend(facts(&[Some(&detail)]));
        rows.push(TranscriptStatusRow {
            parts: vec![
                Some(part("└─ ", "dim")),
                Some(part("Read ", "muted")),
                Some(TranscriptStatusPart {
                    text: format_styled_path(path, theme),
                    color: None,
                }),
                Some(part(&format!("({detail})"), "dim")),
            ],
            indent: Some(indent),
            native: TranscriptStatusNative {
                tone: NoticeTone::Info,
                icon: Some("read".to_string()),
                message,
            },
        });
    }
    TranscriptStatusBlock::new(theme, &rows)
}

/// What a read row says about one mentioned file.
fn file_detail(file: &MentionedFile) -> String {
    match file.skipped_reason.as_deref() {
        Some("tooLarge") | Some("binary") => {
            let size = file
                .byte_size
                .map(|bytes| format_bytes(bytes as usize))
                .unwrap_or_else(|| "unknown size".to_string());
            if file.skipped_reason.as_deref() == Some("binary") {
                format!("skipped: binary, {size}")
            } else {
                format!("skipped: {size}")
            }
        }
        _ => {
            if file.image.is_some() {
                "image".to_string()
            } else {
                match file.line_count {
                    Some(lines) => format_count("line", lines),
                    None => "unknown lines".to_string(),
                }
            }
        }
    }
}

/// `n` counted `kind`s, pluralized.
fn format_count(kind: &str, n: usize) -> String {
    if n == 1 {
        format!("1 {kind}")
    } else {
        format!("{n} {kind}s")
    }
}

/// One themed status part.
fn part(text: &str, color: &str) -> TranscriptStatusPart {
    TranscriptStatusPart {
        text: text.to_string(),
        color: Some(color.to_string()),
    }
}

/// The assistant reply of an [`AgentMessage`], when it is one.
pub fn assistant_reply(message: &AgentMessage) -> Option<&AssistantMessage> {
    match message {
        AgentMessage::Assistant(assistant) => Some(assistant),
        _ => None,
    }
}

/// Whether an assistant turn has visible text, thinking, or image content — i.e.
/// content that closes the current read-tool run.
pub fn assistant_has_visible_content(message: &AssistantMessage) -> bool {
    message.content.iter().any(|content| match content {
        AssistantContent::Image(_) => true,
        AssistantContent::Text(text) => !canonicalize_message(Some(&text.text)).is_empty(),
        AssistantContent::Thinking(thinking) => {
            !canonicalize_message(Some(&thinking.thinking)).is_empty()
        }
        _ => false,
    })
}

/// One split of a mixed assistant turn.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistantToolTimeline {
    /// The reply with the content before the first tool call.
    pub before_tools: AssistantMessage,
    /// Post-tool segments by the tool call they follow.
    pub after_tool_calls: Vec<(String, AssistantMessage)>,
    /// Index in the original content of each after-tool segment's first block,
    /// by tool-call id.
    pub content_offsets: Vec<(String, usize)>,
    /// Whether the turn called a tool at all.
    pub has_tool_calls: bool,
    /// The last tool call's id.
    pub last_tool_call_id: Option<String>,
}

/// Split mixed assistant turns into visible text before tool execution and
/// visible text segments that must render immediately after the preceding tool.
///
/// Cursor can return intro text, tool calls, progress text, and the final
/// answer in one assistant message; keeping every text block in the leading
/// assistant block buries post-tool text above tool results in the transcript.
pub fn split_assistant_message_tool_timeline(message: &AssistantMessage) -> AssistantToolTimeline {
    let mut before_tools = Vec::new();
    let mut after_tool_calls: Vec<(String, AssistantMessage)> = Vec::new();
    let mut content_offsets = Vec::new();
    let mut pending_after_tool = Vec::new();
    let mut last_tool_call_id: Option<String> = None;
    let mut saw_tool_call = false;

    let display_segment = |content: Vec<AssistantContent>| AssistantMessage {
        content,
        stop_reason: crate::chat::messages::StopReason::Stop,
        error_message: None,
        retry_recovery: None,
        ..message.clone()
    };

    for (index, content) in message.content.iter().enumerate() {
        if let Some(call) = content.as_tool_call() {
            if let Some(id) = last_tool_call_id.clone() {
                if !pending_after_tool.is_empty() {
                    after_tool_calls.push((id, display_segment(std::mem::take(&mut pending_after_tool))));
                }
            }
            saw_tool_call = true;
            last_tool_call_id = Some(call.id.clone());
            content_offsets.push((call.id.clone(), index + 1));
            continue;
        }
        if saw_tool_call {
            pending_after_tool.push(content.clone());
        } else {
            before_tools.push(content.clone());
        }
    }
    if let Some(id) = last_tool_call_id.clone() {
        if !pending_after_tool.is_empty() {
            after_tool_calls.push((id, display_segment(std::mem::take(&mut pending_after_tool))));
        }
    }

    if !saw_tool_call {
        return AssistantToolTimeline {
            before_tools: message.clone(),
            after_tool_calls,
            content_offsets,
            has_tool_calls: false,
            last_tool_call_id,
        };
    }

    AssistantToolTimeline {
        before_tools: display_segment(before_tools),
        after_tool_calls,
        content_offsets,
        has_tool_calls: true,
        last_tool_call_id,
    }
}

/// Normalize raw tool-call arguments to a plain record, collapsing non-object
/// or array values to an empty object.
pub fn normalize_tool_args(args: &serde_json::Value) -> &serde_json::Map<String, serde_json::Value> {
    static EMPTY: std::sync::LazyLock<serde_json::Map<String, serde_json::Value>> =
        std::sync::LazyLock::new(serde_json::Map::new);
    match args {
        serde_json::Value::Object(map) => map,
        _ => &EMPTY,
    }
}

/// How a turn-ending assistant error renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssistantErrorPresentation {
    /// No error row.
    None,
    /// The full error text.
    Full {
        /// The text.
        text: String,
        /// Whether the row reads as an error.
        is_error: bool,
    },
    /// A compact note for a recovered retry.
    CompactRecovered {
        /// The note.
        text: String,
        /// Whether the row reads as an error (never, for a recovery).
        is_error: bool,
    },
}

/// One line, tabs expanded and runs of whitespace collapsed.
fn sanitize_recovered_retry_note(note: &str) -> String {
    let normalized = note.replace('\t', "    ");
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: &str = if collapsed.is_empty() { "retried" } else { &collapsed };
    truncate_to_width(text, NOTE_WIDTH, Ellipsis::Unicode)
}

/// Resolve the turn-ending assistant error presentation, if any.
///
/// Silent and user-interrupt aborts yield no label. Recovered retry attempts
/// render a compact note; attempts superseded by an exhausted budget are hidden
/// while the final terminal error keeps its full presentation.
pub fn resolve_assistant_error_presentation(
    message: &AssistantMessage,
    retry_attempt: u32,
) -> AssistantErrorPresentation {
    if let Some(recovery) = &message.retry_recovery {
        if recovery.status == "superseded" {
            return AssistantErrorPresentation::None;
        }
        if recovery.status == "recovered" {
            return AssistantErrorPresentation::CompactRecovered {
                text: sanitize_recovered_retry_note(&recovery.note),
                is_error: false,
            };
        }
    }
    if message.stop_reason == crate::chat::messages::StopReason::Aborted {
        if !should_render_abort_reason(message) {
            return AssistantErrorPresentation::None;
        }
        return AssistantErrorPresentation::Full {
            text: resolve_abort_label(message, retry_attempt),
            is_error: true,
        };
    }
    if message.stop_reason == crate::chat::messages::StopReason::Error {
        return AssistantErrorPresentation::Full {
            text: message
                .error_message
                .clone()
                .unwrap_or_else(|| "Error".to_string()),
            is_error: true,
        };
    }
    if let Some(error) = &message.error_message {
        if should_render_abort_reason(message) {
            return AssistantErrorPresentation::Full {
                text: error.clone(),
                is_error: true,
            };
        }
    }
    AssistantErrorPresentation::None
}

/// Whether an assistant turn's `usage` reflects work the operator was billed
/// for.
///
/// Empty automated turns from providers that emit `usage: 0` collapse to
/// `false`, but any input, output or cache token keeps the row so cost
/// transparency survives — the live path and the resume/rebuild path agree
/// turn-by-turn.
pub fn assistant_usage_is_billed(usage: &Usage) -> bool {
    if usage.input > 0 || usage.output > 0 {
        return true;
    }
    if usage.cache_read > 0 || usage.cache_write > 0 {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::messages::{
        AssistantRetryRecovery, StopReason, TextContent, ThinkingContent, ToolCall,
    };
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

    fn text(value: &str) -> AssistantContent {
        AssistantContent::Text(TextContent {
            text: value.to_string(),
            text_signature: None,
        })
    }

    fn call(id: &str) -> AssistantContent {
        AssistantContent::ToolCall(ToolCall {
            id: id.to_string(),
            name: "read".to_string(),
            ..ToolCall::default()
        })
    }

    fn reply(content: Vec<AssistantContent>) -> AssistantMessage {
        AssistantMessage {
            content,
            ..AssistantMessage::default()
        }
    }

    #[test]
    fn visible_content_ignores_empty_text() {
        assert!(!assistant_has_visible_content(&reply(vec![])));
        assert!(!assistant_has_visible_content(&reply(vec![text("   \n")])));
        assert!(assistant_has_visible_content(&reply(vec![text("hi")])));
        let thinking = AssistantContent::Thinking(ThinkingContent {
            thinking: "hmm".to_string(),
            ..ThinkingContent::default()
        });
        assert!(assistant_has_visible_content(&reply(vec![thinking])));
    }

    #[test]
    fn timeline_splits_around_tool_calls() {
        let message = reply(vec![text("intro"), call("a"), text("progress"), call("b"), text("done")]);
        let timeline = split_assistant_message_tool_timeline(&message);
        assert!(timeline.has_tool_calls);
        assert_eq!(timeline.before_tools.content, vec![text("intro")]);
        assert_eq!(timeline.last_tool_call_id.as_deref(), Some("b"));
        assert_eq!(timeline.content_offsets, vec![("a".to_string(), 2), ("b".to_string(), 4)]);
        assert_eq!(timeline.after_tool_calls.len(), 2);
        assert_eq!(timeline.after_tool_calls[0].0, "a");
        assert_eq!(timeline.after_tool_calls[0].1.content, vec![text("progress")]);
        assert_eq!(timeline.after_tool_calls[1].0, "b");
        assert_eq!(timeline.after_tool_calls[1].1.content, vec![text("done")]);
    }

    #[test]
    fn timeline_without_tools_is_the_whole_turn() {
        let message = reply(vec![text("only text")]);
        let timeline = split_assistant_message_tool_timeline(&message);
        assert!(!timeline.has_tool_calls);
        assert_eq!(timeline.before_tools, message);
        assert!(timeline.after_tool_calls.is_empty());
    }

    #[test]
    fn tool_args_normalize_to_objects() {
        let object = serde_json::json!({"a": 1});
        assert_eq!(normalize_tool_args(&object).len(), 1);
        assert!(normalize_tool_args(&serde_json::json!([1, 2])).is_empty());
        assert!(normalize_tool_args(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn error_presentations_follow_the_stop_reason() {
        let mut message = reply(vec![]);
        assert_eq!(
            resolve_assistant_error_presentation(&message, 0),
            AssistantErrorPresentation::None
        );
        message.stop_reason = StopReason::Error;
        assert!(matches!(
            resolve_assistant_error_presentation(&message, 0),
            AssistantErrorPresentation::Full { is_error: true, .. }
        ));
        message.retry_recovery = Some(AssistantRetryRecovery {
            status: "recovered".to_string(),
            attempt: 1,
            recovery: "retry".to_string(),
            note: "tried\tagain".to_string(),
            superseded_by: None,
        });
        assert_eq!(
            resolve_assistant_error_presentation(&message, 0),
            AssistantErrorPresentation::CompactRecovered {
                text: "tried again".to_string(),
                is_error: false,
            }
        );
        message.retry_recovery.as_mut().unwrap().status = "superseded".to_string();
        assert_eq!(
            resolve_assistant_error_presentation(&message, 0),
            AssistantErrorPresentation::None
        );
    }

    #[test]
    fn usage_billing_counts_any_tokens() {
        assert!(!assistant_usage_is_billed(&Usage::default()));
        assert!(assistant_usage_is_billed(&Usage {
            input: 1,
            ..Usage::default()
        }));
        assert!(assistant_usage_is_billed(&Usage {
            cache_read: 1,
            ..Usage::default()
        }));
    }

    #[test]
    fn async_result_blocks_render_one_row_per_job() {
        let details = AsyncResultDetails {
            jobs: vec![
                AsyncResultJob {
                    job_id: Some("bg_1".to_string()),
                    job_type: Some("bash".to_string()),
                    duration_ms: Some(1500.0),
                    ..AsyncResultJob::default()
                },
                AsyncResultJob {
                    job_id: None,
                    job_type: None,
                    ..AsyncResultJob::default()
                },
            ],
            ..AsyncResultDetails::default()
        };
        let mut block = build_async_result_block(&theme(), &details);
        let rows = block.render(80);
        assert_eq!(rows.len(), 2);
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).contains("bg_1"));
        assert!(crate::ansi_scan::strip_ansi(&rows[1]).contains("unknown"));
    }

    #[test]
    fn launch_completion_rows_mark_failures() {
        let mut daemons = vec![DaemonSnapshot {
            name: "web".to_string(),
            state: DaemonState::Exited,
            exit_code: Some(0),
            started_at: 0.0,
            exited_at: Some(2.0),
            ..DaemonSnapshot::default()
        }];
        let mut block = build_launch_completion_block(&theme(), None, &daemons);
        let rows = block.render(80);
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).contains("Supervised process completed"));
        daemons[0].exit_code = Some(3);
        let mut failed = build_launch_completion_block(&theme(), None, &daemons);
        let rows = failed.render(80);
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).contains("Supervised process failed"));
    }

    #[test]
    fn file_mention_details_cover_skips_and_lines() {
        let lines = MentionedFile {
            path: "src/main.rs".to_string(),
            line_count: Some(12),
            ..MentionedFile::default()
        };
        assert_eq!(file_detail(&lines), "12 lines");
        let binary = MentionedFile {
            skipped_reason: Some("binary".to_string()),
            byte_size: Some(2048),
            ..MentionedFile::default()
        };
        assert!(file_detail(&binary).starts_with("skipped: binary"));
        let unknown = MentionedFile::default();
        assert_eq!(file_detail(&unknown), "unknown lines");
    }
}
