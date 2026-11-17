//! Shared pieces of the tool renderers' TSP describe hooks: result text,
//! card heads, error views, notes and notices. No layout: every helper returns
//! semantic nodes/spans only, with ANSI-free, sanitized text.
//!
//! Ported from `l/src/tools/native-view.ts`.
//!
//! The reference imports its describe contract (`NativeToolHead`,
//! `NativeToolView`, `ToolRenderResult`, `RenderResultOptions`) from
//! `l/src/tools/renderer.ts`; that module is outside this slice, so the
//! contract types live here and the other tool modules import them from
//! `crate::tools::native_view`. `NativeToolView.preview` collapses the
//! TypeScript `TspPreview | { tail } | "none" | "children"` union into
//! [`NativePreview`]. `l/src/tools/lsp.ts` contributes
//! [`FileDiagnosticsResult`]/[`FileFormatResult`] and `l/src/render/render-utils.ts`
//! contributes [`ParsedDiagnostic`]/[`parse_diagnostic_message`]; both are
//! ported here because no crate module owns them yet.
//!
//! Deviations: the reference reads `process.cwd()` and the home directory from
//! the JS runtime; this port reads them from `std::env`. `display_path` keeps
//! the reference's "relative when inside cwd, else `~`-shortened" rule.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::native::describe::{
    ansi, code, compact, elapsed, keyed, node, row, span, text,
};
use crate::native::node::{
    NativeNode, NodeKind, NodeText, Props, Span, Tone,
};
use crate::native::spans::plain_text;
use crate::render::hyperlink::{try_file_uri, FilePosition};
use crate::render::render_utils::{path_name_start, replace_tabs};
use crate::status_line::segments::shorten_path;
use crate::tools::output_meta::{
    format_artifact_error_notice, format_truncation_meta_notice, OutputMeta,
};

/// One tool-result content block: `{ type, text? }`.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentBlock {
    /// Block type (`"text"`, `"image"`, …).
    pub type_: String,
    /// Text payload of a text block.
    pub text: Option<String>,
}

impl ContentBlock {
    /// A text block.
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock {
            type_: "text".to_string(),
            text: Some(text.into()),
        }
    }
}

/// Display state handed to `render_call`/`render_result`.
#[derive(Clone, Debug, Default)]
pub struct RenderResultOptions {
    /// Whether the result view is expanded.
    pub expanded: bool,
    /// Whether this is a partial/streaming result.
    pub is_partial: bool,
    /// Current spinner frame index for animated elements (0-9).
    pub spinner_frame: Option<usize>,
    /// True once arguments are final.
    pub args_complete: Option<bool>,
    /// True once this specific call has begun executing.
    pub execution_started: Option<bool>,
    /// Milliseconds since this call began executing, or its total once settled.
    pub elapsed_ms: Option<f64>,
    /// The call was abandoned without a final result.
    pub cancelled: Option<bool>,
    /// The native host shows files by path.
    pub media: Option<bool>,
    /// The native host draws raw JSON values.
    pub json: Option<bool>,
    /// The native host draws a live shell trace.
    pub shell: Option<bool>,
    /// Native semantic presentation, decoded and cached by the tool component.
    pub shell_presentation: Option<serde_json::Value>,
    /// Validated immutable trace associated with `shell_presentation`.
    pub shell_trace: Option<serde_json::Value>,
    /// Explicit seeks/resets change this epoch; ordinary playback ticks do not.
    pub shell_replay_epoch: Option<u64>,
    /// Tool-specific context the transcript threads through.
    pub render_context: Option<HashMap<String, serde_json::Value>>,
}

/// Render options for a result, plus the tool-specific context the transcript
/// threads through.
pub type RenderResultContextOptions = RenderResultOptions;

/// Tool result shape a renderer receives: content blocks plus the tool's typed
/// `details`.
#[derive(Clone, Debug, Default)]
pub struct ToolRenderResult<TDetails = serde_json::Value> {
    /// Result content blocks.
    pub content: Vec<ContentBlock>,
    /// Tool-specific details payload.
    pub details: Option<TDetails>,
    /// Whether the tool reported a failure.
    pub is_error: Option<bool>,
}

/// Semantic activity text consumed by the transcript's generic compact card.
#[derive(Clone, Debug, Default)]
pub struct ToolActivitySummary {
    /// Short label.
    pub label: String,
    /// Optional detail.
    pub detail: Option<String>,
}

/// A fence a tool call draws under its card, as assistant Markdown draws that
/// fence.
#[derive(Clone, Debug, Default)]
pub struct ToolFigure {
    /// Fence language.
    pub lang: String,
    /// The body so far.
    pub source: String,
    /// No more of the body arrives.
    pub closed: bool,
}

/// A small labelled chip in a tool head.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolBadge {
    /// Badge text, without brackets.
    pub text: String,
    /// Badge tone token.
    pub tone: String,
    /// Optional hover title.
    pub title: Option<String>,
}

/// The data head of a tool call: what the terminal draws in the `tool` kind's
/// head.
#[derive(Clone, Debug, Default)]
pub struct NativeToolHead {
    /// The verb ("Bash", "Edit"); defaults to the tool's label.
    pub title: Option<NodeText>,
    /// The primary argument, shown once.
    pub target: Option<NodeText>,
    /// Target kind (`"text"`, `"command"`, `"path"`, …).
    pub target_kind: Option<String>,
    /// Language for `command` targets.
    pub lang: Option<String>,
    /// The raw command line the head's Copy command button copies.
    pub command: Option<String>,
    /// `file://` link for path targets.
    pub href: Option<String>,
    /// Short facts after the target.
    pub meta: Option<Vec<NodeText>>,
    /// Head badges.
    pub badges: Option<Vec<ToolBadge>>,
    /// A one-word state note ("timed out", "partial").
    pub note: Option<NodeText>,
    /// A non-zero exit code (error chip).
    pub exit: Option<i64>,
}

/// Body clamp while collapsed.
#[derive(Clone, Debug, PartialEq)]
pub enum NativePreview {
    /// The transcript's default preview size.
    Default,
    /// `{ lines: N }`.
    Lines(usize),
    /// `{ tail: N }` keeps the end.
    Tail(usize),
    /// `"none"`.
    None,
    /// `"children"`: the body stays whole while collapsed.
    Children,
}

/// A tool's semantic presentation for Tern Surface Protocol terminals.
#[derive(Clone, Debug, Default)]
pub struct NativeToolView {
    /// The data head.
    pub tool: Option<NativeToolHead>,
    /// Header spans for the fallback card.
    pub head: Option<NodeText>,
    /// Body: sections, never frames.
    pub body: Option<Vec<NativeNode>>,
    /// Tone override.
    pub tone: Option<Tone>,
    /// Body clamp while collapsed.
    pub preview: Option<NativePreview>,
    /// Starts expanded whatever the transcript's expand state.
    pub open: Option<bool>,
    /// Render frameless.
    pub inline: Option<bool>,
    /// Head actions offered on hover.
    pub tools: Option<Vec<String>>,
}

/// A path for native heads: relative to the working directory when inside it,
/// else `~`-shortened.
pub fn display_path(file_path: &str) -> String {
    if Path::new(file_path).is_absolute() {
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rel) = Path::new(file_path).strip_prefix(&cwd) {
                let rel = rel.to_string_lossy();
                if !rel.is_empty() && !rel.starts_with("..") {
                    return rel.into_owned();
                }
            }
        }
    }
    let home = home_dir();
    if home.is_empty() {
        return file_path.to_string();
    }
    shorten_path(file_path, &home)
}

/// Configured file link for a head's path target, when the path is absolute.
pub fn file_href(file_path: Option<&str>) -> Option<String> {
    let path = file_path?;
    if !Path::new(path).is_absolute() {
        return None;
    }
    try_file_uri(path, FilePosition::default(), false)
}

/// Link for a head target that is a web URL (`www.` gains `https://`), else
/// `None`.
pub fn url_href(target: Option<&str>) -> Option<String> {
    let target = target?;
    let candidate = if target.len() >= 4 && target[..4].eq_ignore_ascii_case("www.") {
        format!("https://{target}")
    } else {
        target.to_string()
    };
    let rest = if candidate.len() >= 8 && candidate[..8].eq_ignore_ascii_case("https://") {
        Some(&candidate[8..])
    } else if candidate.len() >= 7 && candidate[..7].eq_ignore_ascii_case("http://") {
        Some(&candidate[7..])
    } else {
        None
    }?;
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return None;
    }
    Some(candidate)
}

/// Diff stats as one head `meta` string, `+8 −1` (U+2212 minus), or `None` when
/// nothing changed.
pub fn diff_stats_meta(added: usize, removed: usize) -> Option<String> {
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("+{added}"));
    }
    if removed > 0 {
        parts.push(format!("\u{2212}{removed}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// One file of a multi-file edit: a borderless `section` headed by the path
/// (→ rename) and its `+a −d`, over the file's body (its diff, diagnostics).
pub fn file_diff_section(
    file: &FileDiffSectionFile,
    body: Vec<NativeNode>,
    p: &FileDiffSectionProps,
) -> NativeNode {
    let mut head: Vec<Span> = vec![span(display_path(&file.path), Some("path"))];
    if let Some(rename) = &file.rename {
        head.push(span(" → ", Some("muted")));
        head.push(span(display_path(rename), Some("path")));
    }
    if let Some(added) = file.added {
        head.push(span(format!(" +{added}"), Some("ins")));
    }
    if let Some(removed) = file.removed {
        head.push(span(format!(" \u{2212}{removed}"), Some("del")));
    }
    let mut props = Props::new();
    props.set_head(NodeText::Spans(head));
    props.set_str("role", &p.role);
    if let Some(tone) = p.tone {
        props.set_tone(tone);
    }
    node(NodeKind::Section, props, body, Some(&file.path))
}

/// Inputs of [`file_diff_section`].
#[derive(Clone, Debug, Default)]
pub struct FileDiffSectionFile {
    /// File path.
    pub path: String,
    /// Rename target, when the edit renamed the file.
    pub rename: Option<String>,
    /// Added line count.
    pub added: Option<usize>,
    /// Removed line count.
    pub removed: Option<usize>,
}

/// Props of [`file_diff_section`].
#[derive(Clone, Debug, Default)]
pub struct FileDiffSectionProps {
    /// Accessible role.
    pub role: String,
    /// Optional tone.
    pub tone: Option<Tone>,
}

/// Inputs of [`file_row`].
#[derive(Clone, Debug, Default)]
pub struct FileRowProps {
    /// Quiet `range` suffix (`:13-36`).
    pub range: Option<String>,
    /// Count chip.
    pub chip: Option<ToolBadge>,
    /// Muted detail line.
    pub detail: Option<NodeText>,
    /// Explicit link target.
    pub href: Option<String>,
    /// Leading glyph replacing the file icon.
    pub lead: Option<NativeNode>,
    /// Icon name override.
    pub icon: Option<String>,
    /// Tone override.
    pub tone: Option<Tone>,
    /// Hover title.
    pub title: Option<String>,
    /// Sibling identity key.
    pub key: Option<String>,
}

/// One file row (role `omp.tool.file`) for read groups, grep files, glob lists
/// and find hits.
pub fn file_row(file_path: &str, p: &FileRowProps) -> NativeNode {
    let is_dir = file_path.ends_with('/');
    let url = url_href(Some(file_path));
    let cut = if url.is_none() {
        path_name_start(file_path)
    } else {
        0
    };
    let href = p.href.clone().or(url);
    let mut spans: Vec<Span> = Vec::new();
    if cut > 0 {
        spans.push(span(file_path[..cut].to_string(), Some("dim")));
    }
    let mut name = span(file_path[cut..].to_string(), Some("strong"));
    name.href = href.clone();
    spans.push(name);
    if let Some(range) = &p.range {
        spans.push(span(range.clone(), Some("muted")));
    }
    let mut text_props = Props::new();
    text_props.set_i64("lines", 1);
    text_props.set_str("truncate", "middle");
    text_props.set_i64("shrink", 1);
    text_props.set_str("title", p.title.as_deref().unwrap_or(file_path));
    let mut children: Vec<NativeNode> = vec![
        p.lead.clone().unwrap_or_else(|| {
            let mut props = Props::new();
            props.set_str("name", p.icon.as_deref().unwrap_or(if is_dir { "folder" } else { "file" }));
            if let Some(tone) = p.tone {
                props.set_tone(tone);
            }
            node(NodeKind::Icon, props, Vec::new(), None)
        }),
        text(NodeText::Spans(spans), text_props),
    ];
    if let Some(chip) = &p.chip {
        let mut props = Props::new();
        props.set_str("text", &chip.text);
        if let Some(title) = &chip.title {
            props.set_str("title", title);
        }
        children.push(node(NodeKind::Badge, props, Vec::new(), None));
    }
    if let Some(detail) = &p.detail {
        let spans = match detail {
            NodeText::Plain(text) => vec![span(plain_text(text), Some("muted"))],
            NodeText::Spans(spans) => spans.clone(),
        };
        let mut props = Props::new();
        props.set_i64("lines", 1);
        props.set_str("truncate", "end");
        props.set_i64("shrink", 1);
        children.push(text(NodeText::Spans(spans), props));
    }
    let mut row_props = Props::new();
    row_props.set_str("role", "omp.tool.file");
    if let Some(tone) = p.tone {
        row_props.set_tone(tone);
    }
    keyed(row(children, row_props), p.key.as_deref().unwrap_or(file_path))
}


/// Longest error excerpt an inline head carries before the body takes the full
/// message.
const ERROR_GIST_CHARS: usize = 120;

/// Inline error view: error tone, the message's first line after `—` in the
/// head's meta, and the full message as the body only when the head had to cut
/// it short.
pub fn inline_error_view(head: NativeToolHead, message: &str) -> NativeToolView {
    let full = plain_text(message).trim().to_string();
    let full = if full.is_empty() { "failed".to_string() } else { full };
    let first = full.split('\n').next().unwrap_or("").trim().to_string();
    let gist = if first.chars().count() > ERROR_GIST_CHARS {
        let mut cut: String = first.chars().take(ERROR_GIST_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        first
    };
    let mut meta = head.meta.clone().unwrap_or_default();
    meta.push(NodeText::Spans(vec![span(format!("— {gist}"), Some("muted"))]));
    let body = if gist == full {
        None
    } else {
        Some(vec![error_text(&full)])
    };
    NativeToolView {
        tool: Some(NativeToolHead {
            meta: Some(meta),
            ..head
        }),
        tone: Some(Tone::Error),
        inline: Some(true),
        body,
        ..NativeToolView::default()
    }
}

/// Text blocks of a tool result joined by newlines, or `""`.
pub fn result_text(result: &ToolRenderResult<serde_json::Value>) -> String {
    let mut out = String::new();
    for block in &result.content {
        if block.type_ == "text" {
            if let Some(text) = &block.text {
                if out.is_empty() {
                    out.push_str(text);
                } else {
                    out.push('\n');
                    out.push_str(text);
                }
            }
        }
    }
    out
}

/// One detail of a [`tool_head`]: a pre-styled span or plain text.
#[derive(Clone, Debug)]
pub enum ToolHeadDetail {
    /// A pre-styled span.
    Span(Span),
    /// Plain text, rendered muted.
    Text(String),
}

impl From<Span> for ToolHeadDetail {
    fn from(value: Span) -> Self {
        ToolHeadDetail::Span(value)
    }
}

impl From<String> for ToolHeadDetail {
    fn from(value: String) -> Self {
        ToolHeadDetail::Text(value)
    }
}

impl From<&str> for ToolHeadDetail {
    fn from(value: &str) -> Self {
        ToolHeadDetail::Text(value.to_string())
    }
}

/// Card head: the bold `toolTitle` title, then detail spans; string details
/// become muted plain text, empty details are skipped.
pub fn tool_head(title: &str, details: &[ToolHeadDetail]) -> Vec<Span> {
    let mut spans: Vec<Span> = vec![span(title, Some("toolTitle strong"))];
    for detail in details {
        let value = match detail {
            ToolHeadDetail::Span(span) => span.clone(),
            ToolHeadDetail::Text(text) => span(plain_text(text), Some("muted")),
        };
        if value.t.is_empty() {
            continue;
        }
        spans.push(span(" ", None));
        spans.push(value);
    }
    spans
}

/// Error body: wrapped error text, or a JSON `code` block when the message is
/// structured.
pub fn error_text(message: &str) -> NativeNode {
    let body = plain_text(message).trim().to_string();
    if body.contains('\n') && (body.starts_with('{') || body.starts_with('[')) {
        let mut props = Props::new();
        props.set_str("lang", "json");
        props.set_tone(Tone::Error);
        props.set_str("role", "omp.tool.error");
        code(body, props)
    } else {
        let mut props = Props::new();
        props.set_str("wrap", "word");
        props.set_str("role", "omp.tool.error");
        text(NodeText::Spans(vec![span(body, Some("error"))]), props)
    }
}

/// Error card view: error tone, the tool head and the error body
/// (`<title> failed` when empty).
pub fn error_view(title: &str, message: &str, meta: &[Option<String>]) -> NativeToolView {
    let details: Vec<ToolHeadDetail> = meta
        .iter()
        .filter_map(|entry| entry.clone().map(ToolHeadDetail::Text))
        .collect();
    let body = plain_text(message).trim().to_string();
    let body = if body.is_empty() {
        format!("{title} failed")
    } else {
        body
    };
    NativeToolView {
        head: Some(NodeText::Spans(tool_head(title, &details))),
        tone: Some(Tone::Error),
        body: Some(vec![error_text(&body)]),
        ..NativeToolView::default()
    }
}

/// Wrapped placeholder or secondary line (`(no output)`, `No matches`, …);
/// `lines` clamps it.
pub fn note_text(message: &str, token: &str, lines: Option<usize>) -> NativeNode {
    let mut props = Props::new();
    props.set_str("wrap", "word");
    if let Some(lines) = lines {
        props.set_i64("lines", lines as i64);
    }
    text(
        NodeText::Spans(vec![span(plain_text(message), Some(token))]),
        props,
    )
}

/// Muted `a · b · c` metadata line, or `None` when there are no parts.
pub fn stats_text(parts: &[String]) -> Option<NativeNode> {
    if parts.is_empty() {
        return None;
    }
    let mut props = Props::new();
    props.set_str("wrap", "word");
    props.set_str("role", "omp.tool.stats");
    Some(text(
        NodeText::Spans(vec![span(parts.join(" · "), Some("muted"))]),
        props,
    ))
}

/// Warning line for truncation / artifact capture failures, or `None`.
pub fn truncation_notice(meta: Option<&OutputMeta>) -> Option<NativeNode> {
    let meta = meta?;
    if meta.truncation.is_none() && meta.artifact_error.is_none() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(truncation) = &meta.truncation {
        parts.push(format_truncation_meta_notice(truncation, meta.source.as_ref()));
    }
    if let Some(error) = meta.artifact_error.as_deref() {
        parts.push(format_artifact_error_notice(error));
    }
    let mut props = Props::new();
    props.set_str("wrap", "word");
    props.set_str("role", "omp.tool.notice");
    Some(text(
        NodeText::Spans(vec![span(parts.join(". "), Some("warning"))]),
        props,
    ))
}

/// The one final quiet line of a tool body: `a · b` facts, then any truncation
/// / artifact-capture notice from `meta`; `None` when empty.
pub fn footnote_text(parts: &[String], meta: Option<&OutputMeta>) -> Option<NativeNode> {
    let mut all: Vec<String> = parts
        .iter()
        .map(|part| plain_text(part))
        .filter(|part| !part.is_empty())
        .collect();
    if let Some(truncation) = meta.and_then(|meta| meta.truncation.as_ref()) {
        all.push(format_truncation_meta_notice(truncation, meta.and_then(|meta| meta.source.as_ref())));
    }
    if let Some(error) = meta.and_then(|meta| meta.artifact_error.as_ref()) {
        all.push(format_artifact_error_notice(error));
    }
    if all.is_empty() {
        return None;
    }
    let notice = meta.map_or(false, |meta| {
        meta.truncation.is_some() || meta.artifact_error.is_some()
    });
    let mut props = Props::new();
    props.set_str("wrap", "word");
    props.set_str(
        "role",
        if notice {
            "omp.tool.notice"
        } else {
            "omp.tool.stats"
        },
    );
    let mut node = text(
        NodeText::Spans(vec![span(all.join(" · "), Some("muted"))]),
        props,
    );
    node.key = Some("foot".to_string());
    Some(node)
}

/// Drawn lines of a run's input (command, cell code) before its "N more lines"
/// button.
pub const RUN_INPUT_PREVIEW_LINES: usize = 6;

/// What a run's foot names: `running` (live clock), `background` (an async job
/// outlives the call), and the settled outcomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// Still running.
    Running,
    /// Running in the background.
    Background,
    /// Finished successfully.
    Done,
    /// Failed.
    Failed,
    /// Killed before finishing.
    Cancelled,
    /// Killed by the deadline.
    TimedOut,
}

impl RunState {
    /// Wire name of the state.
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Running => "running",
            RunState::Background => "background",
            RunState::Done => "done",
            RunState::Failed => "failed",
            RunState::Cancelled => "cancelled",
            RunState::TimedOut => "timed-out",
        }
    }
}

/// Inputs of [`run_foot`].
#[derive(Clone, Debug, Default)]
pub struct RunFootInput {
    /// Settled or in-flight state.
    pub state: Option<RunState>,
    /// Exit code of a `failed` run; a non-zero code names the state `exit N`.
    pub exit_code: Option<i64>,
    /// Milliseconds run so far or in total.
    pub elapsed_ms: Option<f64>,
    /// Quiet facts (job id, service state, artifact).
    pub facts: Option<Vec<String>>,
    /// Truncation / artifact-capture notices.
    pub meta: Option<OutputMeta>,
}

/// The foot line of a run box (`omp.run.foot`).
pub fn run_foot(input: &RunFootInput) -> NativeNode {
    let state = input.state.unwrap_or(RunState::Done);
    let failed_with_code = state == RunState::Failed
        && input.exit_code.map_or(false, |code| code != 0);
    let (word, tone) = if failed_with_code {
        (format!("exit {}", input.exit_code.unwrap_or(0)), Tone::Error)
    } else {
        (
            match state {
                RunState::Running => "Running",
                RunState::Background => "In background",
                RunState::Done => "Done",
                RunState::Failed => "Failed",
                RunState::Cancelled => "Cancelled",
                RunState::TimedOut => "Timed out",
            }
            .to_string(),
            match state {
                RunState::Running | RunState::Background => Tone::Pending,
                RunState::Done => Tone::Success,
                RunState::Failed => Tone::Error,
                RunState::Cancelled => Tone::Muted,
                RunState::TimedOut => Tone::Warning,
            },
        )
    };
    let facts: Vec<String> = input
        .facts
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|fact| plain_text(fact).trim().to_string())
        .filter(|fact| !fact.is_empty())
        .collect();
    let mut notices = Vec::new();
    if let Some(meta) = &input.meta {
        if let Some(truncation) = &meta.truncation {
            notices.push(format_truncation_meta_notice(truncation, meta.source.as_ref()));
        }
        if let Some(error) = meta.artifact_error.as_deref() {
            notices.push(format_artifact_error_notice(error));
        }
    }
    let mut fact_spans: Vec<Span> = Vec::new();
    if !facts.is_empty() {
        fact_spans.push(span(facts.join(" · "), Some("muted")));
    }
    for notice in notices {
        if !fact_spans.is_empty() {
            fact_spans.push(span(" · ", Some("muted")));
        }
        fact_spans.push(span(notice, Some("warning")));
    }
    let ms = input.elapsed_ms.map(|ms| ms.max(0.0).round() as u64);
    let time = ms.map(|ms| {
        let stopped = !matches!(state, RunState::Running | RunState::Background);
        let mut node = elapsed(ms as f64, stopped);
        node.key = Some("time".to_string());
        node
    });
    let mut state_props = Props::new();
    state_props.set_str("role", "omp.run.state");
    state_props.set_tone(tone);
    let state_node = keyed(
        text(NodeText::Spans(vec![span(word, None)]), state_props),
        "state",
    );
    let facts_node = if fact_spans.is_empty() {
        None
    } else {
        let mut props = Props::new();
        props.set_str("role", "omp.run.facts");
        props.set_str("wrap", "word");
        Some(keyed(
            text(NodeText::Spans(fact_spans), props),
            "facts",
        ))
    };
    let mut props = Props::new();
    props.set_str("role", "omp.run.foot");
    props.set_str("gap", "sm");
    props.set_str("align", "center");
    node(
        NodeKind::Row,
        props,
        compact(vec![Some(state_node), time, facts_node]),
        Some("foot"),
    )
}

/// Props of [`run_input`].
#[derive(Clone, Debug, Default)]
pub struct RunInputProps {
    /// Accessible role.
    pub role: String,
    /// Language id.
    pub lang: String,
    /// Whether long lines wrap.
    pub wrap: bool,
}

/// A run's input section: the command or cell source, clamped to
/// [`RUN_INPUT_PREVIEW_LINES`].
pub fn run_input(source: &str, p: &RunInputProps) -> NativeNode {
    let mut props = Props::new();
    props.set_str("role", &p.role);
    props.set_str("lang", &p.lang);
    props.set_bool("wrap", p.wrap);
    let mut preview = Props::new();
    preview.set_i64("lines", RUN_INPUT_PREVIEW_LINES as i64);
    props.insert("preview", preview.to_value());
    keyed(code(source.trim_end(), props), "input")
}

/// Props of [`run_output`].
#[derive(Clone, Debug, Default)]
pub struct RunOutputProps {
    /// Accessible role.
    pub role: String,
    /// Lines kept while folded.
    pub preview_lines: usize,
    /// Sibling identity key.
    pub key: Option<String>,
}

/// A run's terminal output: follows its tail, clamped to the last
/// `preview_lines` lines while folded.
pub fn run_output(output: &str, p: &RunOutputProps) -> NativeNode {
    let mut props = Props::new();
    props.set_str("role", &p.role);
    props.set_bool("follow", true);
    let mut preview = Props::new();
    preview.set_i64("lines", p.preview_lines as i64);
    props.insert("preview", preview.to_value());
    keyed(ansi(output, props), p.key.as_deref().unwrap_or("output"))
}

/// Props of [`run_box`].
#[derive(Clone, Debug, Default)]
pub struct RunBoxProps {
    /// Sibling identity key.
    pub key: String,
    /// Optional tone.
    pub tone: Option<Tone>,
}

/// One run as one box (`omp.run`): input, output, status lines and foot in
/// order.
pub fn run_box(children: &[Option<NativeNode>], p: &RunBoxProps) -> NativeNode {
    let mut props = Props::new();
    props.set_str("role", "omp.run");
    props.set_str("gap", "none");
    if let Some(tone) = p.tone {
        props.set_tone(tone);
    }
    node(NodeKind::Col, props, compact(children.to_vec()), Some(&p.key))
}

/// Diagnostic rows shown before `+N more`.
const DIAGNOSTIC_ROWS: usize = 5;

/// Severity of a parsed diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Info.
    Info,
    /// Hint.
    Hint,
}

impl DiagnosticSeverity {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
            DiagnosticSeverity::Info => "info",
            DiagnosticSeverity::Hint => "hint",
        }
    }

    /// Sort rank (most severe first).
    pub fn rank(self) -> u8 {
        match self {
            DiagnosticSeverity::Error => 0,
            DiagnosticSeverity::Warning => 1,
            DiagnosticSeverity::Info => 2,
            DiagnosticSeverity::Hint => 3,
        }
    }
}

/// Parsed diagnostic location, severity, and optional source and code metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedDiagnostic {
    /// File path.
    pub file_path: String,
    /// 1-indexed line.
    pub line: u32,
    /// 1-indexed column.
    pub col: u32,
    /// Severity.
    pub severity: DiagnosticSeverity,
    /// Diagnostic source.
    pub source: Option<String>,
    /// Message.
    pub message: String,
    /// Diagnostic code.
    pub code: Option<String>,
}

static DIAGNOSTIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+?):(\d+):(\d+)\s+\[(\w+)\]\s+(?:\[([^\]]+)\]\s+)?(.+?)(?:\s+\(([^)]+)\))?$")
        .expect("valid diagnostic regex")
});

/// Parse a diagnostic location and message, including optional source and code.
pub fn parse_diagnostic_message(msg: &str) -> Option<ParsedDiagnostic> {
    let caps = DIAGNOSTIC_RE.captures(msg)?;
    let severity = match caps.get(4)?.as_str() {
        "error" => DiagnosticSeverity::Error,
        "warning" => DiagnosticSeverity::Warning,
        "info" => DiagnosticSeverity::Info,
        "hint" => DiagnosticSeverity::Hint,
        other => {
            let mut chars = other.chars();
            let first = chars.next()?.to_ascii_lowercase();
            match first {
                'e' => DiagnosticSeverity::Error,
                'w' => DiagnosticSeverity::Warning,
                'i' => DiagnosticSeverity::Info,
                _ => DiagnosticSeverity::Hint,
            }
        }
    };
    Some(ParsedDiagnostic {
        file_path: replace_tabs(caps.get(1)?.as_str()),
        line: caps.get(2)?.as_str().parse().ok()?,
        col: caps.get(3)?.as_str().parse().ok()?,
        severity,
        source: caps.get(5).map(|m| replace_tabs(m.as_str())),
        message: replace_tabs(caps.get(6)?.as_str()),
        code: caps.get(7).map(|m| replace_tabs(m.as_str())),
    })
}

/// Outcome of automatic file formatting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormatResult {
    /// Unchanged.
    Unchanged,
    /// Formatted.
    Formatted,
    /// Failed.
    Failed,
    /// Unsupported.
    Unsupported,
}

/// Diagnostics and formatting status for one file.
#[derive(Clone, Debug, Default)]
pub struct FileDiagnosticsResult {
    /// Name of the LSP server used.
    pub server: Option<String>,
    /// Formatted diagnostic messages.
    pub messages: Vec<String>,
    /// Summary string.
    pub summary: String,
    /// Whether there are any errors.
    pub errored: bool,
    /// Servers that sent no fresh report in time.
    pub unreported: Option<Vec<String>>,
    /// Whether the file was formatted.
    pub formatter: Option<FileFormatResult>,
}

fn severity_icon(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "x-circle",
        DiagnosticSeverity::Warning => "warn",
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Hint => "lightbulb",
    }
}

fn severity_tone(severity: DiagnosticSeverity) -> Tone {
    match severity {
        DiagnosticSeverity::Error => Tone::Error,
        DiagnosticSeverity::Warning => Tone::Warning,
        DiagnosticSeverity::Info => Tone::Info,
        DiagnosticSeverity::Hint => Tone::Muted,
    }
}

/// A diagnostic's severity glyph, in the severity tone.
pub fn diagnostic_glyph(severity: DiagnosticSeverity) -> NativeNode {
    let mut props = Props::new();
    props.set_str("name", severity_icon(severity));
    props.set_tone(severity_tone(severity));
    props.set_str("aria", severity.as_str());
    node(NodeKind::Icon, props, Vec::new(), None)
}

/// One diagnostic row: severity icon · `line:col` (with the file when
/// `with_path`) · message and source.
fn diagnostic_row(diagnostic: &ParsedDiagnostic, with_path: bool, key: &str) -> NativeNode {
    let where_text = if with_path {
        format!("{}:{}:{}", display_path(&diagnostic.file_path), diagnostic.line, diagnostic.col)
    } else {
        format!("{}:{}", diagnostic.line, diagnostic.col)
    };
    let mut message: Vec<Span> = vec![span(diagnostic.message.clone(), None)];
    let source = [diagnostic.source.clone(), diagnostic.code.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if !source.is_empty() {
        message.push(span(format!(" {source}"), Some("muted")));
    }
    let mut where_props = Props::new();
    where_props.set_str("wrap", "none");
    let mut message_props = Props::new();
    message_props.set_str("wrap", "word");
    row(
        vec![
            {
                let mut props = Props::new();
                props.set_str("name", severity_icon(diagnostic.severity));
                props.set_tone(severity_tone(diagnostic.severity));
                props.set_str("aria", diagnostic.severity.as_str());
                node(NodeKind::Icon, props, Vec::new(), None)
            },
            text(NodeText::Spans(vec![span(where_text, Some("muted"))]), where_props),
            text(NodeText::Spans(message), message_props),
        ],
        {
            let mut props = Props::new();
            props.set_str("role", "omp.tool.diagnostic");
            props.set_str("key", key);
            props
        },
    )
}

/// Options of [`diagnostics_section`].
#[derive(Clone, Copy, Debug, Default)]
pub struct DiagnosticsSectionOpts {
    /// Prefix each row with its file path.
    pub with_path: bool,
}

/// LSP diagnostics section for write/edit results: rows of severity icon ·
/// `line:col` · message, most severe first, capped at five with `+N more`.
pub fn diagnostics_section(
    diagnostics: Option<&FileDiagnosticsResult>,
    opts: Option<DiagnosticsSectionOpts>,
) -> Option<NativeNode> {
    let diagnostics = diagnostics?;
    if diagnostics.messages.is_empty() {
        return None;
    }
    let mut parsed: Vec<ParsedDiagnostic> = Vec::new();
    let mut unparsed: Vec<String> = Vec::new();
    for message in &diagnostics.messages {
        match parse_diagnostic_message(plain_text(message).trim()) {
            Some(diagnostic) => parsed.push(diagnostic),
            None => {
                let trimmed = plain_text(message);
                let trimmed = trimmed.trim();
                if !trimmed.is_empty() {
                    unparsed.push(trimmed.to_string());
                }
            }
        }
    }
    parsed.sort_by(|a, b| {
        a.severity
            .rank()
            .cmp(&b.severity.rank())
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
    });
    let with_path = opts.map_or(false, |opts| opts.with_path)
        || parsed
            .iter()
            .map(|d| d.file_path.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1;
    let mut rows: Vec<NativeNode> = parsed
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| diagnostic_row(diagnostic, with_path, &format!("d{index}")))
        .collect();
    rows.extend(unparsed.iter().enumerate().map(|(index, message)| {
        let mut props = Props::new();
        props.set_str("wrap", "word");
        props.set_str("role", "omp.tool.diagnostic");
        props.set_str("key", &format!("u{index}"));
        text(
            NodeText::Spans(vec![span(message.clone(), None)]),
            props,
        )
    }));
    let shown = rows.len().min(DIAGNOSTIC_ROWS);
    let hidden = rows.len() - shown;
    let mut shown_rows: Vec<NativeNode> = rows.drain(..shown).collect();
    if hidden > 0 {
        let mut props = Props::new();
        props.set_str("role", "omp.tool.stats");
        props.set_str("key", "more");
        shown_rows.push(text(
            NodeText::Spans(vec![span(format!("+{hidden} more"), Some("muted"))]),
            props,
        ));
    }
    let mut props = Props::new();
    let head = vec![span("Diagnostics", None)];
    props.set_head(NodeText::Spans(head));
    props.set_tone(if diagnostics.errored {
        Tone::Error
    } else {
        Tone::Warning
    });
    props.set_str("role", "omp.tool.diagnostics");
    Some(node(
        NodeKind::Section,
        props,
        shown_rows,
        Some("diagnostics"),
    ))
}

/// Head chip counting LSP diagnostics across files (`2 diagnostics`), or `None`
/// when there are none.
pub fn diagnostics_badge(files: &[Option<FileDiagnosticsResult>]) -> Option<ToolBadge> {
    let mut count = 0usize;
    let mut errored = false;
    for file in files.iter().flatten() {
        count += file.messages.len();
        errored |= file.errored;
    }
    if count == 0 {
        return None;
    }
    Some(ToolBadge {
        text: if count == 1 {
            "1 diagnostic".to_string()
        } else {
            format!("{count} diagnostics")
        },
        tone: if errored { "error" } else { "warning" }.to_string(),
        title: None,
    })
}

fn home_dir() -> String {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_stats_meta_formats_added_and_removed() {
        assert_eq!(diff_stats_meta(8, 1).as_deref(), Some("+8 \u{2212}1"));
        assert_eq!(diff_stats_meta(0, 0), None);
        assert_eq!(diff_stats_meta(3, 0).as_deref(), Some("+3"));
    }

    #[test]
    fn url_href_adds_https_to_www_and_rejects_other_text() {
        assert_eq!(
            url_href(Some("www.example.com")).as_deref(),
            Some("https://www.example.com")
        );
        assert_eq!(
            url_href(Some("https://example.com/a")).as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(url_href(Some("example.com")), None);
        assert_eq!(url_href(Some("https://")), None);
        assert_eq!(url_href(None), None);
    }

    #[test]
    fn result_text_joins_text_blocks() {
        let result = ToolRenderResult {
            content: vec![ContentBlock::text("a"), ContentBlock::text("b")],
            details: None,
            is_error: None,
        };
        assert_eq!(result_text(&result), "a\nb");
    }

    #[test]
    fn tool_head_skips_empty_details() {
        let spans = tool_head("Bash", &[ToolHeadDetail::Text(String::new())]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].t, "Bash");
    }

    #[test]
    fn error_text_uses_json_code_for_structured_multiline() {
        let node = error_text("{\n  \"code\": 1\n}");
        assert_eq!(node.kind, NodeKind::Code);
    }

    #[test]
    fn parse_diagnostic_message_reads_fields() {
        let parsed = parse_diagnostic_message("src/a.ts:12:3 [error] [ts] Type mismatch (ts2322)").unwrap();
        assert_eq!(parsed.file_path, "src/a.ts");
        assert_eq!(parsed.line, 12);
        assert_eq!(parsed.col, 3);
        assert_eq!(parsed.severity, DiagnosticSeverity::Error);
        assert_eq!(parsed.source.as_deref(), Some("ts"));
        assert_eq!(parsed.message, "Type mismatch");
        assert_eq!(parsed.code.as_deref(), Some("ts2322"));
    }

    #[test]
    fn parse_diagnostic_message_rejects_plain_text() {
        assert!(parse_diagnostic_message("not a diagnostic").is_none());
    }

    #[test]
    fn diagnostics_section_caps_rows_and_reports_more() {
        let messages: Vec<String> = (0..7)
            .map(|index| format!("src/a.ts:{index}:1 [warning] message {index}"))
            .collect();
        let diagnostics = FileDiagnosticsResult {
            messages,
            summary: "7 warnings".to_string(),
            ..FileDiagnosticsResult::default()
        };
        let node = diagnostics_section(Some(&diagnostics), None).unwrap();
        let body = node.children.len();
        assert_eq!(body, DIAGNOSTIC_ROWS + 1);
    }

    #[test]
    fn diagnostics_badge_counts_across_files() {
        let files = vec![
            Some(FileDiagnosticsResult {
                messages: vec!["a".to_string()],
                ..FileDiagnosticsResult::default()
            }),
            Some(FileDiagnosticsResult {
                messages: vec!["b".to_string(), "c".to_string()],
                errored: true,
                ..FileDiagnosticsResult::default()
            }),
        ];
        let badge = diagnostics_badge(&files).unwrap();
        assert_eq!(badge.text, "3 diagnostics");
        assert_eq!(badge.tone, "error");
    }

    #[test]
    fn footnote_text_combines_facts_and_notices() {
        let meta = OutputMeta {
            artifact_error: Some(crate::tools::streaming_output::OutputArtifactError::Write),
            ..OutputMeta::default()
        };
        let node = footnote_text(&["Job 4".to_string()], Some(&meta)).unwrap();
        assert_eq!(node.key.as_deref(), Some("foot"));
        let spans = node.props.spans().expect("spans prop");
        let text: String = spans.iter().map(|span| span.t.as_str()).collect();
        assert!(text.contains("Job 4"));
        assert!(text.contains("Full output was not saved completely"));
    }

    #[test]
    fn inline_error_view_truncates_long_gist() {
        let head = NativeToolHead {
            title: Some(NodeText::Plain("Read".to_string())),
            ..NativeToolHead::default()
        };
        let long = "x".repeat(200);
        let view = inline_error_view(head, &long);
        assert_eq!(view.tone, Some(Tone::Error));
        assert!(view.inline.unwrap_or(false));
        assert!(view.body.is_some());
    }
}
