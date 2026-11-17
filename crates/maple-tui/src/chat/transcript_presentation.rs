//! Extension transcript presenters (`pi.registerTranscriptPresenter`). The
//! host installs them with [`set_transcript_presenters`]; each presentable
//! block draws from its [`TranscriptPresentationState`].
//!
//! Reference: `l/src/chat/transcript-presentation.ts`.
//!
//! Deviations:
//! - Presenters are Rust closures ([`TranscriptPresenter`]), so there is no
//!   runtime answer validation: [`read_transcript_presentation`] and the
//!   `Proxy`-safe copying in the reference have no counterpart.
//! - Listener registries return numeric subscription ids, because closures
//!   cannot be compared for identity here.
//! - `PresentedBlock` takes an optional describe callback for its inner
//!   component: the crate's `Component` trait has no `describe`, and
//!   `NativeDescribe` cannot be discovered through a generic bound at runtime.
//! - `isTranscriptPresentable` is a trait ([`TranscriptPresentable`]) rather
//!   than a runtime check.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use crate::chat::extension_types::{
    CompactPresentation, TranscriptBlock, TranscriptPresentation, TranscriptPresenter,
};
use crate::chat::messages::{
    BashExecutionMessage, CustomMessage, CustomMessageContent, LAUNCH_COMPLETION_MESSAGE_TYPE,
    LSP_LATE_DIAGNOSTIC_MESSAGE_TYPE, PythonExecutionMessage, SKILL_PROMPT_MESSAGE_TYPE,
};
use crate::component::Component;
use crate::native::describe::{card, col, span, text, with_hidden, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text as NodeTextValue};
use crate::native::spans::{compact_text, plain_line, plain_text, spans_line};
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::theme::Theme;

/// A block's presentation once every presenter has answered.
#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedPresentation {
    /// The built-in card.
    Full,
    /// Draw nothing.
    Hidden,
    /// One head row plus the presenter's rows.
    Compact {
        /// Replaces the head row's text.
        label: Option<NodeTextValue>,
        /// One row per entry.
        rows: Vec<NodeTextValue>,
    },
}

impl ResolvedPresentation {
    /// The answer's mode name.
    pub fn mode(&self) -> &'static str {
        match self {
            ResolvedPresentation::Full => "full",
            ResolvedPresentation::Hidden => "hidden",
            ResolvedPresentation::Compact { .. } => "compact",
        }
    }

    /// The compact rows, when this is a compact answer.
    pub fn compact(&self) -> Option<CompactPresentation> {
        match self {
            ResolvedPresentation::Compact { label, rows } => Some(CompactPresentation {
                label: label.clone(),
                rows: rows.clone(),
            }),
            _ => None,
        }
    }
}

thread_local! {
    static PRESENTERS: RefCell<Vec<TranscriptPresenter>> = const { RefCell::new(Vec::new()) };
    static CHANGE_LISTENERS: RefCell<Vec<(u64, Rc<dyn Fn()>)>> = const { RefCell::new(Vec::new()) };
    static REDRAW_LISTENERS: RefCell<Vec<(u64, Rc<dyn Fn(u64)>)>> = const { RefCell::new(Vec::new()) };
    static NEXT_LISTENER_ID: Cell<u64> = const { Cell::new(1) };
}

/// Install the presenters the transcript draws with, in extension load order;
/// true when the count changed.
pub fn set_transcript_presenters(next: Vec<TranscriptPresenter>) -> bool {
    PRESENTERS.with(|presenters| {
        let mut presenters = presenters.borrow_mut();
        let changed = presenters.len() != next.len();
        *presenters = next;
        changed
    })
}

/// An extension registered or disposed a presenter; the host installs the set
/// again and refreshes.
pub fn notify_transcript_presenters_changed() {
    let listeners = CHANGE_LISTENERS.with(|listeners| listeners.borrow().clone());
    for (_, listener) in listeners {
        listener();
    }
}

/// Call `listener` on [`notify_transcript_presenters_changed`]; returns the
/// subscription id for [`off_transcript_presenters_changed`].
pub fn on_transcript_presenters_changed(listener: Rc<dyn Fn()>) -> u64 {
    let id = NEXT_LISTENER_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    CHANGE_LISTENERS.with(|listeners| listeners.borrow_mut().push((id, listener)));
    id
}

/// Drop a presenter-change subscription.
pub fn off_transcript_presenters_changed(id: u64) {
    CHANGE_LISTENERS.with(|listeners| listeners.borrow_mut().retain(|(other, _)| *other != id));
}

/// Call `listener` when a block draws a presentation other than the one it
/// last drew, outside a refresh (a late result, say): the host replays the
/// scrollback when that block has rows in it. Returns the subscription id.
pub fn on_transcript_presentation_redrawn(listener: Rc<dyn Fn(u64)>) -> u64 {
    let id = NEXT_LISTENER_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    REDRAW_LISTENERS.with(|listeners| listeners.borrow_mut().push((id, listener)));
    id
}

/// Drop a redraw subscription.
pub fn off_transcript_presentation_redrawn(id: u64) {
    REDRAW_LISTENERS.with(|listeners| listeners.borrow_mut().retain(|(other, _)| *other != id));
}

fn report_redrawn(owner: u64) {
    let listeners = REDRAW_LISTENERS.with(|listeners| listeners.borrow().clone());
    for (_, listener) in listeners {
        listener(owner);
    }
}

/// Presenter text as one sanitized line: ANSI and control characters stripped,
/// breaks as spaces.
fn single_line(value: &NodeTextValue) -> NodeTextValue {
    match value {
        NodeTextValue::Plain(text) => NodeTextValue::Plain(plain_text(text).replace(['\r', '\n'], " ")),
        NodeTextValue::Spans(spans) => {
            let mut out = Vec::new();
            for part in spans {
                let text = plain_text(&part.t).replace(['\r', '\n'], " ");
                if text.is_empty() {
                    continue;
                }
                let mut part = part.clone();
                part.t = text;
                out.push(part);
            }
            NodeTextValue::Spans(out)
        }
    }
}

/// A presenter's answer as a resolved presentation; `None` when it defers.
fn normalize(answer: TranscriptPresentation) -> Option<ResolvedPresentation> {
    match answer {
        TranscriptPresentation::Undefined => None,
        TranscriptPresentation::Full => Some(ResolvedPresentation::Full),
        TranscriptPresentation::Hidden => Some(ResolvedPresentation::Hidden),
        TranscriptPresentation::Compact => Some(ResolvedPresentation::Compact {
            label: None,
            rows: Vec::new(),
        }),
        TranscriptPresentation::CompactWith(compact) => Some(ResolvedPresentation::Compact {
            label: compact.label.as_ref().map(single_line),
            rows: compact.rows.iter().map(single_line).collect(),
        }),
    }
}

/// The first answer other than a deferral wins.
fn resolve(block: &TranscriptBlock) -> ResolvedPresentation {
    let presenters = PRESENTERS.with(|presenters| {
        presenters
            .borrow()
            .iter()
            .map(|presenter| presenter(block))
            .collect::<Vec<_>>()
    });
    for answer in presenters {
        if let Some(presentation) = normalize(answer) {
            return presentation;
        }
    }
    ResolvedPresentation::Full
}

/// Whether `block` is a tool call that still streams progress.
fn is_partial(block: &TranscriptBlock) -> bool {
    matches!(
        block,
        TranscriptBlock::ToolCall {
            is_partial: true,
            ..
        }
    )
}

/// One block's presentation, resolved lazily after
/// [`TranscriptPresentationState::mark_stale`]. The resolved value keeps its
/// identity while the answer is the same, so it can sit in render memo keys.
pub struct TranscriptPresentationState {
    current: ResolvedPresentation,
    stale: bool,
    /// What the block's last render or description drew; `None` before the
    /// first.
    drawn: Option<ResolvedPresentation>,
    /// Whether that drawing was the user's expansion rather than the
    /// presenters' answer.
    drawn_expanded: bool,
    /// The answer changed while the block was still partial; reported once it
    /// settles.
    changed_while_partial: bool,
    /// The transcript block this draws; [`report_redrawn`] names it.
    owner: Option<u64>,
}

impl Default for TranscriptPresentationState {
    fn default() -> Self {
        TranscriptPresentationState {
            current: ResolvedPresentation::Full,
            stale: true,
            drawn: None,
            drawn_expanded: false,
            changed_while_partial: false,
            owner: None,
        }
    }
}

impl TranscriptPresentationState {
    /// A state that names `owner` in its redraw reports.
    pub fn new(owner: Option<u64>) -> Self {
        TranscriptPresentationState {
            owner,
            ..TranscriptPresentationState::default()
        }
    }

    /// The block's data changed: ask again before the next draw.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// What the block draws now.
    pub fn shown(&mut self, block: Option<&TranscriptBlock>, expanded: bool) -> &ResolvedPresentation {
        if expanded {
            return &ResolvedPresentation::Full;
        }
        if self.stale {
            self.stale = false;
            let has_presenters = PRESENTERS.with(|presenters| !presenters.borrow().is_empty());
            let next = match block {
                Some(block) if has_presenters => resolve(block),
                _ => ResolvedPresentation::Full,
            };
            if next != self.current {
                self.current = next;
            }
        }
        &self.current
    }

    /// The last answer [`shown`](Self::shown) resolved, without asking the
    /// presenters again.
    pub fn resolved(&self, expanded: bool) -> &ResolvedPresentation {
        if expanded {
            return &ResolvedPresentation::Full;
        }
        &self.current
    }

    /// [`shown`](Self::shown), for the block's render or description: what
    /// [`refresh`](Self::refresh) compares against.
    pub fn draw(
        &mut self,
        block: Option<&TranscriptBlock>,
        expanded: bool,
    ) -> ResolvedPresentation {
        let previous = self.drawn.clone();
        let next = self.shown(block, expanded).clone();
        let changed = previous
            .as_ref()
            .map(|previous| previous != &next)
            .unwrap_or(false);
        self.report(block, changed && !expanded && !self.drawn_expanded);
        self.drawn = Some(next.clone());
        self.drawn_expanded = expanded;
        next
    }

    /// The block's data changed where nothing may draw it again (rows retired
    /// to scrollback): ask now, and report an answer other than the drawn one
    /// as [`draw`](Self::draw) would. What was drawn stays the baseline until
    /// the block draws again.
    pub fn recheck(&mut self, block: Option<&TranscriptBlock>, expanded: bool) {
        let Some(drawn) = self.drawn.clone() else {
            return;
        };
        if self.drawn_expanded || expanded {
            return;
        }
        let shown = self.shown(block, expanded).clone();
        self.report(block, shown != drawn);
    }

    /// Tell the redraw listeners about a change, once the block is no longer
    /// partial.
    fn report(&mut self, block: Option<&TranscriptBlock>, changed: bool) {
        let owner = match self.owner {
            Some(owner) => owner,
            None => return,
        };
        if !changed && !self.changed_while_partial {
            return;
        }
        if block.map(is_partial).unwrap_or(false) {
            self.changed_while_partial = true;
            return;
        }
        self.changed_while_partial = false;
        report_redrawn(owner);
    }

    /// Ask again now when `ids` selects this block (every block when `None`);
    /// true when the answer differs from what the block last drew.
    pub fn refresh(&mut self, block: Option<&TranscriptBlock>, ids: Option<&HashSet<String>>) -> bool {
        if let Some(ids) = ids {
            match block {
                Some(block) if ids.contains(block.id()) => {}
                _ => return false,
            }
        }
        self.stale = true;
        let next = self.shown(block, false).clone();
        if self.drawn.as_ref() == Some(&next) || self.drawn.is_none() {
            return false;
        }
        self.drawn = Some(next);
        self.drawn_expanded = false;
        self.changed_while_partial = false;
        true
    }
}

/// A transcript block whose display presenters choose.
pub trait TranscriptPresentable {
    /// Ask again for the parts whose id is in `ids` (all when `None`); true
    /// when the drawing changed. True obliges the caller to redraw the block.
    fn refresh_transcript_presentation(&mut self, ids: Option<&HashSet<String>>) -> bool;
}

/// A compact block's terminal rows: `• <head>` (the presenter's `label`
/// replacing `head`), then one indented row per presenter row.
pub fn compact_rows(
    head: &str,
    presentation: &CompactPresentation,
    width: usize,
    glyph: &str,
    theme: &Theme,
) -> Vec<String> {
    let shown = match &presentation.label {
        None => head.to_string(),
        Some(NodeTextValue::Plain(text)) => plain_line(text),
        Some(label) => spans_line(label, theme),
    };
    let glyph = theme.fg("dim", glyph).unwrap_or_else(|_| glyph.to_string());
    let mut rows = vec![truncate_to_width(
        &format!("{glyph} {shown}"),
        width,
        Ellipsis::Unicode,
    )];
    for row in &presentation.rows {
        let text = match row {
            NodeTextValue::Plain(text) => plain_line(text),
            row => spans_line(row, theme),
        };
        rows.push(truncate_to_width(
            &format!("  {text}"),
            width,
            Ellipsis::Unicode,
        ));
    }
    rows
}

/// omp's own one-row head: the bold title, then a muted detail (raw input,
/// sanitized to one line).
pub fn compact_head(title: &str, detail: Option<&str>, theme: &Theme) -> String {
    let title = plain_line(title);
    let bold = theme.bold(&title);
    let head = theme.fg("toolTitle", &bold).unwrap_or(bold);
    match detail.map(plain_line).filter(|detail| !detail.is_empty()) {
        None => head,
        Some(detail) => {
            let detail = format!(" {detail}");
            let muted = theme.fg("muted", &detail).unwrap_or(detail);
            format!("{head}{muted}")
        }
    }
}

/// The presenter's rows as ONE text node, so a `preview` of that many lines
/// shows all of them.
fn compact_rows_node(rows: &[NodeTextValue]) -> NativeNode {
    let mut spans = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            spans.push(crate::native::node::Span::new("\n"));
        }
        match row {
            NodeTextValue::Plain(text) => spans.push(crate::native::node::Span::new(text.clone())),
            NodeTextValue::Spans(parts) => spans.extend(parts.iter().cloned()),
        }
    }
    let mut props = Props::new();
    props.set_str("wrap", "none");
    text(compact_text(&spans), props)
}

/// How a block names itself in a compact native card when its full drawing is
/// not a `tool` node.
#[derive(Clone, Debug, PartialEq)]
pub struct CompactNativeHead {
    /// Role style.
    pub role: String,
    /// One-word title.
    pub title: String,
    /// What the block acts on.
    pub target: Option<NodeTextValue>,
}

/// A compact block head: the terminal row after its glyph, and the native row.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockHead {
    /// The terminal row.
    pub terminal: String,
    /// The glyph before it, when the block replaces the bullet.
    pub glyph: Option<String>,
    /// Kind name for the native row's icon and role styles.
    pub name: String,
    /// Role style.
    pub role: String,
    /// One-word title.
    pub title: String,
    /// What the block acts on, sanitized.
    pub target: Option<String>,
}

/// A block's native drawing under `presentation`. Hidden stays mounted through
/// the `hidden` prop. Compact folds a `tool` node to one inline, collapsed row
/// (its toggle asks for the full block); any other drawing becomes the body of
/// a collapsed card headed by `head`.
pub fn present_native(
    full: NativeNode,
    presentation: &ResolvedPresentation,
    head: &CompactNativeHead,
) -> NativeNode {
    match presentation {
        ResolvedPresentation::Full => full,
        ResolvedPresentation::Hidden => with_hidden(full, true),
        ResolvedPresentation::Compact { label, rows } => {
            if full.kind != NodeKind::Tool || full.props.is_empty() {
                let target = match label {
                    Some(NodeTextValue::Plain(text)) => {
                        let text = plain_line(text);
                        if text.is_empty() {
                            None
                        } else {
                            Some(NodeTextValue::Plain(text))
                        }
                    }
                    Some(label) => Some(single_line(label)),
                    None => head.target.as_ref().map(|target| match target {
                        NodeTextValue::Plain(text) => NodeTextValue::Plain(plain_line(text)),
                        target => single_line(target),
                    }),
                };
                let mut spans = vec![span(plain_line(&head.title), Some("toolTitle strong"))];
                if let Some(target) = &target {
                    spans.push(span(" ", None));
                    match target {
                        NodeTextValue::Plain(text) => spans.push(span(text.clone(), Some("muted"))),
                        NodeTextValue::Spans(parts) => spans.extend(parts.iter().cloned()),
                    }
                }
                let mut props = Props::new();
                props.set_str("role", head.role.clone());
                props.set_head(NodeTextValue::Spans(spans));
                props.set_bool("collapsible", true);
                props.set_bool("collapsed", true);
                if !rows.is_empty() {
                    props.set_i64("preview", rows.len() as i64);
                }
                let mut children = Vec::new();
                if !rows.is_empty() {
                    children.push(compact_rows_node(rows));
                } else {
                    children.push(full);
                }
                return card(props, children);
            }
            let mut props = full.props.clone();
            match label {
                Some(label) => {
                    props.set_head(single_line(label));
                    props.set_str("targetKind", "text");
                }
                None => {}
            }
            props.set_str("frame", "inline");
            props.set_bool("collapsible", true);
            props.set_bool("collapsed", true);
            if !rows.is_empty() {
                props.set_i64("preview", rows.len() as i64);
            }
            let children = if rows.is_empty() {
                full.children.clone()
            } else {
                vec![compact_rows_node(rows)]
            };
            NativeNode::with_children(full.kind, props, children)
        }
    }
}

/// The `message` block of a session message: its id is the decimal
/// `timestamp`.
pub fn message_block(
    message: &SessionMessageLike,
    timestamp: i64,
) -> TranscriptBlock {
    TranscriptBlock::Message {
        id: timestamp.to_string(),
        message: message.clone(),
    }
}

/// A session message a transcript block may wrap.
pub type SessionMessageLike = crate::chat::extension_types::SessionMessage;

thread_local! {
    static NEXT_EVENT_BLOCK_ID: Cell<u64> = const { Cell::new(0) };
}

/// A process-unique id for an `event` block.
pub fn event_block_id(kind: &str) -> String {
    let id = NEXT_EVENT_BLOCK_ID.with(|next| {
        let id = next.get() + 1;
        next.set(id);
        id
    });
    format!("{kind}:{id}")
}

/// `n` counted `kind`s.
fn count_of(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// A head from a title and an optional target.
fn plain_head(
    name: &str,
    role: &str,
    title: &str,
    target: Option<&str>,
    theme: &Theme,
) -> BlockHead {
    BlockHead {
        terminal: compact_head(title, target, theme),
        glyph: None,
        name: name.to_string(),
        role: role.to_string(),
        title: plain_line(title),
        target: target.map(plain_line).filter(|target| !target.is_empty()),
    }
}

/// omp's own one-row summary of a wrapped block, from the block presenters
/// see.
pub fn block_head(block: &TranscriptBlock, theme: &Theme) -> BlockHead {
    match block {
        TranscriptBlock::Event { event, .. } => match event {
            crate::chat::extension_types::TranscriptEvent::TtsrTriggered { rules } => {
                let names = rules
                    .iter()
                    .map(|rule| rule.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                plain_head("ttsr", "omp.notice.ttsr", "Rule", Some(&names), theme)
            }
            crate::chat::extension_types::TranscriptEvent::TodoReminder {
                todos,
                attempt,
                max_attempts,
            } => {
                let reminder = format!(
                    "{} - reminder {attempt}/{max_attempts}",
                    count_of(todos.len(), "incomplete todo")
                );
                plain_head("todo", "omp.notice.todo", "Todos", Some(&reminder), theme)
            }
        },
        TranscriptBlock::ToolCall { tool_name, .. } => {
            plain_head("tool", "omp.tool", tool_name, None, theme)
        }
        TranscriptBlock::Message { message, .. } => {
            message_head(message, theme)
        }
    }
}

/// The head of a message block.
fn message_head(message: &SessionMessageLike, theme: &Theme) -> BlockHead {
    let (bash, command, cancelled, exit_code, exclude_from_context, custom) = match message {
        SessionMessageLike::BashExecution(BashExecutionMessage {
            command,
            exit_code,
            cancelled,
            exclude_from_context,
            ..
        }) => (
            true,
            Some(command.as_str()),
            *cancelled,
            *exit_code,
            *exclude_from_context,
            None,
        ),
        SessionMessageLike::PythonExecution(PythonExecutionMessage {
            code,
            exit_code,
            cancelled,
            ..
        }) => (false, Some(code.as_str()), *cancelled, *exit_code, false, None),
        SessionMessageLike::Custom(custom) => (false, None, false, None, false, Some(custom)),
    };
    if let Some(source) = command {
        let first = plain_line(source.lines().next().unwrap_or_default());
        let color = if exclude_from_context {
            "dim"
        } else if bash {
            "bashMode"
        } else {
            "pythonMode"
        };
        let prompt = if bash { "$" } else { ">>>" };
        let bold = theme.bold(&format!("{prompt} {first}"));
        let mut terminal = theme.fg(color, &bold).unwrap_or(bold);
        if cancelled {
            terminal.push_str(&theme.fg("muted", " cancelled").unwrap_or_else(|_| " cancelled".to_string()));
        } else if let Some(code) = exit_code.filter(|code| *code != 0) {
            terminal.push_str(&format!(" exit {code}"));
        }
        return BlockHead {
            terminal,
            glyph: None,
            name: if bash { "bash" } else { "eval" }.to_string(),
            role: if bash { "omp.bash" } else { "omp.eval" }.to_string(),
            title: if bash { "Bash" } else { "Python" }.to_string(),
            target: Some(first),
        };
    }
    let Some(custom) = custom else {
        return plain_head("message", "omp.custom", "Message", None, theme);
    };
    custom_head(custom, theme)
}

/// The head of an extension message block.
fn custom_head(message: &CustomMessage, theme: &Theme) -> BlockHead {
    let details = message.details.as_ref();
    match message.custom_type.as_str() {
        "advisor" => {
            let notes = details
                .and_then(|details| details.get("notes"))
                .and_then(|notes| notes.as_array())
                .cloned()
                .unwrap_or_default();
            let blockers = notes
                .iter()
                .filter(|note| note.get("severity").and_then(|s| s.as_str()) == Some("blocker"))
                .count();
            let tally = if blockers > 0 {
                format!("{} · {}", count_of(notes.len(), "note"), count_of(blockers, "blocker"))
            } else {
                count_of(notes.len(), "note")
            };
            plain_head("advisor", "omp.advisor", "Advisor", Some(&tally), theme)
        }
        SKILL_PROMPT_MESSAGE_TYPE => {
            let name = details
                .and_then(|details| details.get("name"))
                .and_then(|name| name.as_str())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "unknown".to_string());
            plain_head("skill", "omp.skill", "Skill", Some(&name), theme)
        }
        LSP_LATE_DIAGNOSTIC_MESSAGE_TYPE => {
            let files = details
                .and_then(|details| details.get("files"))
                .and_then(|files| files.as_array())
                .cloned()
                .unwrap_or_default();
            let count: usize = files
                .iter()
                .map(|file| {
                    file.get("messages")
                        .and_then(|messages| messages.as_array())
                        .map(|messages| messages.len())
                        .unwrap_or(0)
                })
                .sum();
            let summary = files
                .iter()
                .filter_map(|file| file.get("summary").and_then(|s| s.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            let errored = files
                .iter()
                .any(|file| file.get("errored").and_then(|e| e.as_bool()) == Some(true));
            let mut head = plain_head(
                "diagnostics",
                "omp.diagnostics.late",
                "Late diagnostics",
                Some(&format!(
                    "{}{}",
                    count_of(count, "diagnostic"),
                    if summary.is_empty() {
                        String::new()
                    } else {
                        format!(" · {summary}")
                    }
                )),
                theme,
            );
            head.glyph = Some(status_glyph(theme, if errored { "error" } else { "warning" }));
            head
        }
        "async-result" => {
            let jobs = details
                .and_then(|details| details.get("jobs"))
                .and_then(|jobs| jobs.as_array())
                .cloned()
                .unwrap_or_default();
            let (ids, many) = if jobs.is_empty() {
                let id = details
                    .and_then(|details| details.get("jobId"))
                    .and_then(|id| id.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                (id, false)
            } else {
                (
                    jobs.iter()
                        .map(|job| {
                            job.get("jobId")
                                .and_then(|id| id.as_str())
                                .unwrap_or("unknown")
                                .to_string()
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    true,
                )
            };
            let title = if many && jobs.len() > 1 {
                format!("{} background jobs completed", jobs.len())
            } else {
                "Background job completed".to_string()
            };
            let mut head = plain_head("job", "omp.activity", &title, Some(&ids), theme);
            head.glyph = Some(status_glyph(theme, "success"));
            head
        }
        LAUNCH_COMPLETION_MESSAGE_TYPE => {
            let daemons = details
                .and_then(|details| details.get("daemons"))
                .and_then(|daemons| daemons.as_array())
                .cloned()
                .unwrap_or_default();
            let failed = daemons.iter().any(|daemon| {
                daemon.get("state").and_then(|state| state.as_str()) == Some("failed")
                    || daemon
                        .get("exitCode")
                        .and_then(|code| code.as_i64())
                        .is_some_and(|code| code != 0)
            });
            let names = daemons
                .iter()
                .filter_map(|daemon| daemon.get("name").and_then(|name| name.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            let title = if failed {
                "Supervised process failed"
            } else {
                "Supervised process completed"
            };
            let content = match &message.content {
                CustomMessageContent::Text(text) if names.is_empty() => Some(text.as_str()),
                _ => None,
            };
            let mut head = plain_head(
                "process",
                "omp.activity",
                title,
                Some(if names.is_empty() { content.unwrap_or("") } else { &names }),
                theme,
            );
            head.glyph = Some(status_glyph(theme, if failed { "error" } else { "success" }));
            head
        }
        other => plain_head("message", "omp.custom", other, None, theme),
    }
}

/// Hooks a wrapped component may have; the wrapper forwards the ones it has.
pub trait CompactTranscriptBlock: Component {
    /// Expand or collapse the block.
    fn set_expanded(&mut self, _expanded: bool) {}
    /// Whether the user expanded it, including through a nested disclosure.
    fn is_expanded(&self) -> bool {
        false
    }
    /// Whether the block is the user's tool-activity visibility.
    fn set_tool_activity_visible(&mut self, _visible: bool) {}
    /// Whether the block may still change.
    fn is_transcript_block_finalized(&self) -> bool {
        true
    }
    /// The block draws compactly while the transcript squeezes it.
    fn is_transcript_block_compact(&self) -> bool {
        false
    }
    /// Rows the transcript allocated it.
    fn set_transcript_allocation(&mut self, _rows: usize) {}
}

/// A non-tool transcript block (advisor, skill, late diagnostics, `!`/`$` run,
/// background job, supervised process, TTSR, todo reminder) with the
/// presenters' answer applied around the component that draws it.
pub struct PresentedBlock<T: CompactTranscriptBlock> {
    inner: T,
    block: TranscriptBlock,
    head: Option<BlockHead>,
    presentation: TranscriptPresentationState,
    expanded: bool,
    tool_activity_visible: bool,
    allocation: usize,
    theme: Theme,
    describe_inner: Option<Box<dyn Fn(&DescribeContext) -> NativeNode>>,
}

impl<T: CompactTranscriptBlock> PresentedBlock<T> {
    /// Wrap `inner`, whose data is `block`.
    pub fn new(inner: T, block: TranscriptBlock, theme: Theme) -> Self {
        PresentedBlock {
            inner,
            block,
            head: None,
            presentation: TranscriptPresentationState::default(),
            expanded: false,
            tool_activity_visible: true,
            allocation: usize::MAX,
            theme,
            describe_inner: None,
        }
    }

    /// Describe `inner` natively through `describe` instead of rendering rows.
    pub fn with_inner_description(
        mut self,
        describe: Box<dyn Fn(&DescribeContext) -> NativeNode>,
    ) -> Self {
        self.describe_inner = Some(describe);
        self
    }

    /// The block presenters see.
    pub fn block(&self) -> &TranscriptBlock {
        &self.block
    }

    /// The wrapped component.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// The wrapped component, mutably.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// The block's data changed (a `!`/`$` run was recorded, TTSR rules merged
    /// in). A block already in scrollback is not drawn again, so a changed
    /// answer is reported now for a one-time replay.
    pub fn set_block(&mut self, block: TranscriptBlock) {
        self.block = block;
        self.head = None;
        self.presentation.mark_stale();
        let expanded = self.is_expanded();
        let block = self.block.clone();
        self.presentation.recheck(Some(&block), expanded);
    }

    /// Expand or collapse the block.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
        self.inner.set_expanded(expanded);
    }

    /// Whether the user expanded it.
    pub fn is_expanded(&self) -> bool {
        self.expanded || self.inner.is_expanded()
    }

    /// Hide or show the block's tool-activity chrome.
    pub fn set_tool_activity_visible(&mut self, visible: bool) {
        self.tool_activity_visible = visible;
        self.inner.set_tool_activity_visible(visible);
    }

    /// Whether the block may still change.
    pub fn is_transcript_block_finalized(&self) -> bool {
        self.inner.is_transcript_block_finalized()
    }

    /// A squeezed compact block keeps its head and as many rows as fit; in
    /// full, `inner` decides.
    pub fn set_transcript_allocation(&mut self, rows: usize) {
        self.allocation = rows;
        self.inner.set_transcript_allocation(rows);
    }

    /// Compact while drawn as compact rows; in full, as compact as `inner`
    /// says.
    pub fn is_transcript_block_compact(&mut self) -> bool {
        let expanded = self.is_expanded();
        let block = self.block.clone();
        match self.presentation.shown(Some(&block), expanded).mode() {
            "full" => self.inner.is_transcript_block_compact(),
            "compact" => self.tool_activity_visible && self.allocation > 0,
            _ => false,
        }
    }
}

impl<T: CompactTranscriptBlock> TranscriptPresentable for PresentedBlock<T> {
    fn refresh_transcript_presentation(&mut self, ids: Option<&HashSet<String>>) -> bool {
        let block = self.block.clone();
        self.presentation.refresh(Some(&block), ids)
    }
}

impl<T: CompactTranscriptBlock> Component for PresentedBlock<T> {
    fn render(&mut self, width: u16) -> Vec<String> {
        let expanded = self.is_expanded();
        let block = self.block.clone();
        let shown = self.presentation.draw(Some(&block), expanded);
        match shown {
            ResolvedPresentation::Full => self.inner.render(width),
            ResolvedPresentation::Hidden => Vec::new(),
            ResolvedPresentation::Compact { .. } => {
                if !self.tool_activity_visible || self.allocation == 0 {
                    return Vec::new();
                }
                let head = match &self.head {
                    Some(head) => head.clone(),
                    None => {
                        let head = block_head(&block, &self.theme);
                        self.head = Some(head.clone());
                        head
                    }
                };
                let compact = shown.compact().unwrap_or_default();
                let glyph = head.glyph.as_deref().unwrap_or("•");
                let mut rows = compact_rows(&head.terminal, &compact, width as usize, glyph, &self.theme);
                rows.truncate(self.allocation);
                rows
            }
        }
    }

    fn invalidate(&mut self) {
        self.head = None;
        self.inner.invalidate();
    }

    fn dispose(&mut self) {
        self.inner.dispose();
    }
}

impl<T: CompactTranscriptBlock> NativeDescribe for PresentedBlock<T> {
    fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
        // The wrapper describes its full drawing only; a compact answer is
        // rendered as rows by the transcript container.
        match &self.describe_inner {
            Some(describe) => describe(cx),
            None => col(Vec::new(), Props::new()),
        }
    }
}

/// The theme's status glyph for `tone` (`"success"`, `"warning"`, `"error"`), painted.
fn status_glyph(theme: &Theme, tone: &str) -> String {
    let status = theme.status();
    let glyph = match tone {
        "error" => status.error,
        "warning" => status.warning,
        _ => status.success,
    };
    theme.fg(tone, &glyph).unwrap_or(glyph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::extension_types::TranscriptEvent;
    use crate::components::Text as RowText;
    use crate::theme::builtin::builtin_theme_json;
    use crate::theme::loader::{create_theme, CreateThemeOptions};
    use crate::theme::theme_class::ColorMode;
    use std::cell::Cell;

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

    fn message_block_of(custom_type: &str, details: serde_json::Value) -> TranscriptBlock {
        TranscriptBlock::Message {
            id: "7".to_string(),
            message: SessionMessageLike::Custom(CustomMessage {
                custom_type: custom_type.to_string(),
                content: CustomMessageContent::Text("body".to_string()),
                display: true,
                details: Some(details),
                attribution: Default::default(),
                timestamp: 7,
            }),
        }
    }

    #[test]
    fn presenters_resolve_in_order_and_first_answer_wins() {
        set_transcript_presenters(vec![
            Box::new(|_block| TranscriptPresentation::Undefined),
            Box::new(|_block| TranscriptPresentation::Hidden),
            Box::new(|_block| TranscriptPresentation::Full),
        ]);
        let block = message_block_of("advisor", serde_json::json!({}));
        let mut state = TranscriptPresentationState::default();
        assert_eq!(
            state.shown(Some(&block), false),
            &ResolvedPresentation::Hidden
        );
        set_transcript_presenters(Vec::new());
        let mut fresh = TranscriptPresentationState::default();
        assert_eq!(fresh.shown(Some(&block), false), &ResolvedPresentation::Full);
    }

    #[test]
    fn expansion_always_draws_full() {
        set_transcript_presenters(vec![Box::new(|_block| TranscriptPresentation::Hidden)]);
        let block = message_block_of("advisor", serde_json::json!({}));
        let mut state = TranscriptPresentationState::default();
        assert_eq!(
            state.shown(Some(&block), true),
            &ResolvedPresentation::Full
        );
        set_transcript_presenters(Vec::new());
    }

    #[test]
    fn compact_labels_and_rows_are_sanitized() {
        set_transcript_presenters(vec![Box::new(|_block| {
            TranscriptPresentation::CompactWith(CompactPresentation {
                label: Some(NodeTextValue::Plain("line\nbreak".to_string())),
                rows: vec![NodeTextValue::Plain("row one".to_string())],
            })
        })]);
        let block = message_block_of("advisor", serde_json::json!({}));
        let mut state = TranscriptPresentationState::default();
        let shown = state.shown(Some(&block), false).clone();
        match shown {
            ResolvedPresentation::Compact { label, rows } => {
                assert_eq!(label, Some(NodeTextValue::Plain("line break".to_string())));
                assert_eq!(rows, vec![NodeTextValue::Plain("row one".to_string())]);
            }
            other => panic!("expected compact, got {other:?}"),
        }
        set_transcript_presenters(Vec::new());
    }

    #[test]
    fn compact_rows_carry_the_glyph_and_indent() {
        let rows = compact_rows(
            "head",
            &CompactPresentation {
                label: None,
                rows: vec![NodeTextValue::Plain("second".to_string())],
            },
            40,
            "•",
            &theme(),
        );
        assert_eq!(crate::ansi_scan::strip_ansi(&rows[0]), "• head");
        assert_eq!(crate::ansi_scan::strip_ansi(&rows[1]), "  second");
    }

    #[test]
    fn compact_head_bolds_the_title_and_mutes_the_detail() {
        let head = compact_head("Bash", Some("ls -la"), &theme());
        assert!(crate::ansi_scan::strip_ansi(&head).contains("Bash ls -la"));
    }

    #[test]
    fn block_heads_name_each_message_kind() {
        let bash = TranscriptBlock::Message {
            id: "3".to_string(),
            message: SessionMessageLike::BashExecution(BashExecutionMessage {
                command: "printf hi".to_string(),
                exit_code: Some(2),
                ..BashExecutionMessage::default()
            }),
        };
        let head = block_head(&bash, &theme());
        assert_eq!(head.name, "bash");
        assert!(crate::ansi_scan::strip_ansi(&head.terminal).contains("exit 2"));

        let advisor = message_block_of("advisor", serde_json::json!({"notes": [{"severity": "blocker"}]}));
        let head = block_head(&advisor, &theme());
        assert_eq!(head.role, "omp.advisor");
        assert!(crate::ansi_scan::strip_ansi(&head.terminal).contains("1 note · 1 blocker"));

        let skill = message_block_of(SKILL_PROMPT_MESSAGE_TYPE, serde_json::json!({"name": "deploy"}));
        assert_eq!(block_head(&skill, &theme()).title, "Skill");

        let todo = TranscriptBlock::Event {
            id: event_block_id("todo"),
            event: TranscriptEvent::TodoReminder {
                todos: Vec::new(),
                attempt: 2,
                max_attempts: 3,
            },
        };
        let head = block_head(&todo, &theme());
        assert_eq!(head.role, "omp.notice.todo");
        assert!(crate::ansi_scan::strip_ansi(&head.terminal).contains("0 incomplete todos - reminder 2/3"));
    }

    #[test]
    fn event_block_ids_are_unique() {
        assert_ne!(event_block_id("todo"), event_block_id("todo"));
    }

    #[test]
    fn message_blocks_use_the_decimal_timestamp() {
        let block = message_block(
            &SessionMessageLike::BashExecution(BashExecutionMessage {
                timestamp: 42,
                ..BashExecutionMessage::default()
            }),
            42,
        );
        assert_eq!(block.id(), "42");
        assert_eq!(block.kind(), "message");
    }

    #[test]
    fn presented_blocks_follow_the_presenters_answer() {
        set_transcript_presenters(vec![Box::new(|_block| TranscriptPresentation::Hidden)]);
        let block = message_block_of("advisor", serde_json::json!({}));
        let mut presented = PresentedBlock::new(RowText::new("full body"), block, theme());
        assert!(presented.render(40).is_empty());
        presented.set_expanded(true);
        let rows = presented.render(40);
        assert_eq!(rows.len(), 1);
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).trim_end() == "full body");
        set_transcript_presenters(Vec::new());
    }

    #[test]
    fn refreshes_report_only_changed_answers() {
        set_transcript_presenters(vec![Box::new(|_block| TranscriptPresentation::Hidden)]);
        let block = message_block_of("advisor", serde_json::json!({}));
        let mut presented = PresentedBlock::new(RowText::new("body"), block, theme());
        let _ = presented.render(40);
        assert!(!presented.refresh_transcript_presentation(None), "nothing drawn changed");
        set_transcript_presenters(vec![Box::new(|_block| TranscriptPresentation::Full)]);
        assert!(presented.refresh_transcript_presentation(None));
        set_transcript_presenters(Vec::new());
    }

    #[test]
    fn listener_subscriptions_can_be_dropped() {
        let calls = Rc::new(Cell::new(0usize));
        let counted = calls.clone();
        let id = on_transcript_presenters_changed(Rc::new(move || {
            counted.set(counted.get() + 1)
        }));
        notify_transcript_presenters_changed();
        assert_eq!(calls.get(), 1);
        off_transcript_presenters_changed(id);
        notify_transcript_presenters_changed();
        assert_eq!(calls.get(), 1);
    }
}
