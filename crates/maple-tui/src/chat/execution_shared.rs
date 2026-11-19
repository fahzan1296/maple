//! Shared rendering primitives for bash/eval execution components.
//!
//! Reference: `l/src/chat/execution-shared.ts`.
//!
//! Both user execution components build the same frame (a dimmed rule around a
//! content column) and the same post-run status rows, so those pieces live
//! here and the components keep only their header and output preparation.
//!
//! Deviations:
//! - The live loader (`Loader`, a timer-driven component) does not exist in
//!   this crate: [`build_execution_frame`] returns the border colour key and
//!   the caller animates its own label.
//! - `interruptKey()` needs the keybinding manager, so the cancel hint is a
//!   process-global string set by the host ([`set_interrupt_hint`]).
//! - Sixel passthrough is not modelled; clamps apply to every line.
//! - `performance.now()` is passed in as `now_ms`, keeping the helpers pure.

use crate::native::describe::{ansi, card, elapsed, node, span, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props, Span, Tone};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::Theme;
use crate::tools::native_view::{
    run_box, run_foot, run_input, run_output, RunBoxProps, RunFootInput, RunInputProps,
    RunOutputProps, RunState,
};
use crate::tools::output_meta::{
    format_artifact_error_notice, format_bytes, format_truncation_meta_notice, MinimizedMeta,
    OutputMeta, TruncationMeta,
};

/// Output rows shown while an execution is collapsed.
pub const PREVIEW_LINES: usize = 20;

/// Maximum visible columns retained from an execution output line.
pub const MAX_DISPLAY_LINE_CHARS: usize = 4000;

/// Terminal rows kept from a `run` output in its collapsed preview.
pub const DEFAULT_TERMINAL_PREVIEW_LINES: usize = 8;

static INTERRUPT_HINT: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Set the key hint shown while a run is cancellable (defaults to `esc`).
pub fn set_interrupt_hint(hint: impl Into<String>) {
    if let Ok(mut guard) = INTERRUPT_HINT.write() {
        *guard = hint.into();
    }
}

/// The key hint shown while a run is cancellable.
pub fn interrupt_hint() -> String {
    INTERRUPT_HINT
        .read()
        .map(|guard| {
            if guard.is_empty() {
                "esc".to_string()
            } else {
                guard.clone()
            }
        })
        .unwrap_or_else(|_| "esc".to_string())
}

/// Clamp execution output by visible width without splitting ANSI sequences.
pub fn clamp_display_line(line: &str) -> String {
    let visible = visible_width(line);
    if visible <= MAX_DISPLAY_LINE_CHARS {
        return line.to_string();
    }
    let omitted = visible - MAX_DISPLAY_LINE_CHARS;
    format!(
        "{}… [{omitted} visible columns omitted]",
        truncate_to_width(line, MAX_DISPLAY_LINE_CHARS, Ellipsis::Omit)
    )
}

/// Lifecycle of a user-initiated run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// Still streaming.
    #[default]
    Running,
    /// Finished successfully.
    Complete,
    /// Cancelled by the user.
    Cancelled,
    /// Finished with a failure.
    Error,
}

impl ExecutionStatus {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionStatus::Running => "running",
            ExecutionStatus::Complete => "complete",
            ExecutionStatus::Cancelled => "cancelled",
            ExecutionStatus::Error => "error",
        }
    }

    /// The card status this run maps to.
    pub fn card_status(self) -> &'static str {
        match self {
            ExecutionStatus::Running => "running",
            ExecutionStatus::Complete => "done",
            ExecutionStatus::Cancelled => "cancelled",
            ExecutionStatus::Error => "error",
        }
    }

    /// The run-box foot state this run maps to.
    pub fn run_state(self) -> RunState {
        match self {
            ExecutionStatus::Running => RunState::Running,
            ExecutionStatus::Complete => RunState::Done,
            ExecutionStatus::Cancelled => RunState::Cancelled,
            ExecutionStatus::Error => RunState::Failed,
        }
    }
}

/// Theme colour keys valid for an execution frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionColorKey {
    /// Dimmed: excluded from context (`!!` / `$$`).
    #[default]
    Dim,
    /// A bash run.
    BashMode,
    /// An eval run.
    PythonMode,
}

impl ExecutionColorKey {
    /// Theme token of this frame colour.
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionColorKey::Dim => "dim",
            ExecutionColorKey::BashMode => "bashMode",
            ExecutionColorKey::PythonMode => "pythonMode",
        }
    }
}

/// The bottom-border marker of a settled run: `exit N` for a failure,
/// `cancelled` for a cancel, nothing for a clean exit.
pub fn execution_status_label(
    status: ExecutionStatus,
    exit_code: Option<i64>,
    theme: &Theme,
) -> Option<String> {
    match status {
        ExecutionStatus::Cancelled => Some(theme.fg("warning", "cancelled").unwrap_or_else(|_| "cancelled".to_string())),
        ExecutionStatus::Error => Some(
            theme
                .fg("error", &format!("exit {}", exit_code.unwrap_or(0)))
                .unwrap_or_else(|_| format!("exit {}", exit_code.unwrap_or(0))),
        ),
        ExecutionStatus::Running | ExecutionStatus::Complete => None,
    }
}

/// Inputs of [`build_status_footer`].
#[derive(Clone, Debug, Default)]
pub struct StatusFooterOptions<'a> {
    /// Truncation metadata, when the output was cut.
    pub truncation: Option<&'a TruncationMeta>,
    /// Why the output artifact could not be captured.
    pub artifact_error: Option<&'a str>,
    /// Bash minimizer metadata.
    pub minimized: Option<&'a MinimizedMeta>,
    /// Output rows the collapsed preview hides.
    pub hidden_line_count: usize,
    /// Suppress the "… N more lines" hint (the full output is already shown).
    pub suppress_hidden_count: bool,
}

/// The post-run status rows (hidden-line hint, truncation notice), empty when
/// there is nothing to show.
pub fn build_status_footer(opts: &StatusFooterOptions<'_>, theme: &Theme) -> Vec<String> {
    let paint = |token: &str, value: String| theme.fg(token, &value).unwrap_or(value);
    let mut rows = Vec::new();
    if opts.hidden_line_count > 0 && !opts.suppress_hidden_count {
        rows.push(paint(
            "dim",
            format!(
                "… {} more lines ({} to expand)",
                opts.hidden_line_count,
                crate::render::render_utils::expand_key_hint()
            ),
        ));
    }
    if let Some(truncation) = opts.truncation {
        rows.push(paint("warning", format_truncation_meta_notice(truncation, None)));
    }
    if let Some(error) = opts.artifact_error {
        rows.push(paint("warning", format_artifact_error_notice(error)));
    }
    if let Some(minimized) = opts.minimized {
        rows.push(paint("dim", format!("Minimized: {}", format_minimized_meta(minimized))));
    }
    rows
}

/// `filter: in → out bytes, artifact://id`.
pub fn format_minimized_meta(minimized: &MinimizedMeta) -> String {
    let mut out = format!(
        "{} ({})",
        minimized.filter,
        format_bytes(minimized.output_bytes)
    );
    if minimized.input_bytes > 0 {
        out.push_str(&format!(" from {}", format_bytes(minimized.input_bytes)));
    }
    if !minimized.artifact_id.is_empty() {
        out.push_str(&format!(" · artifact://{}", minimized.artifact_id));
    }
    out
}

/// Inputs of [`describe_execution_card`].
#[derive(Clone, Debug)]
pub struct ExecutionCardInput<'a> {
    /// Accessible role.
    pub role: &'a str,
    /// The head row's text.
    pub head: Vec<Span>,
    /// Excluded from context (`!!`/`$$`): a muted card.
    pub muted: bool,
    /// Lifecycle state.
    pub status: ExecutionStatus,
    /// Start of the run, in milliseconds.
    pub started_at_ms: f64,
    /// Now, in milliseconds.
    pub now_ms: f64,
    /// Whether the block shows its whole body.
    pub expanded: bool,
    /// Body nodes before the output (the eval cell's code).
    pub lead: Vec<NativeNode>,
    /// Raw terminal output.
    pub output: String,
    /// Rendered images.
    pub images: Vec<NativeNode>,
    /// Exit code of a settled run.
    pub exit_code: Option<i64>,
    /// Truncation metadata.
    pub truncation: Option<TruncationMeta>,
    /// Artifact capture error.
    pub artifact_error: Option<String>,
    /// Bash minimizer metadata.
    pub minimized: Option<MinimizedMeta>,
}

impl<'a> Default for ExecutionCardInput<'a> {
    fn default() -> Self {
        ExecutionCardInput {
            role: "",
            head: Vec::new(),
            muted: false,
            status: ExecutionStatus::Running,
            started_at_ms: 0.0,
            now_ms: 0.0,
            expanded: false,
            lead: Vec::new(),
            output: String::new(),
            images: Vec::new(),
            exit_code: None,
            truncation: None,
            artifact_error: None,
            minimized: None,
        }
    }
}

/// Native card shared by user bash/eval runs: the head with a live `elapsed`
/// while running, the output as an `ansi` mini terminal that follows its tail,
/// a cancel hint, then exit/truncation notes.
pub fn describe_execution_card(input: &ExecutionCardInput<'_>) -> NativeNode {
    let running = input.status == ExecutionStatus::Running;
    let age = (input.now_ms - input.started_at_ms).max(0.0);
    let mut head_children = vec![keyed_props_text(NodeText::Spans(input.head.clone()), |props| {
        props.set_i64("lines", 1);
        props.set_bool("grow", true);
    }, "head-text")];
    if running {
        head_children.push(elapsed(age, false));
    }
    let mut children = vec![node(
        NodeKind::Row,
        props_with(&[("gap", "sm"), ("align", "baseline")]),
        head_children,
        Some("head"),
    )];
    children.extend(input.lead.iter().cloned());
    if !input.output.is_empty() {
        let mut props = Props::new();
        props.set_str("role", "omp.execution.output");
        props.set_bool("follow", running);
        let mut preview = Props::new();
        preview.set_i64("lines", PREVIEW_LINES as i64);
        props.insert("preview", preview.to_value());
        children.push(crate::native::describe::keyed(ansi(input.output.clone(), props), "output"));
    }
    children.extend(input.images.iter().cloned());
    if running {
        let mut props = Props::new();
        props.set_str("role", "omp.execution.running");
        children.push(node(
            NodeKind::Row,
            props,
            vec![text(
                NodeText::Spans(vec![span(
                    format!("Running… ({} to cancel)", interrupt_hint()),
                    Some("muted"),
                )]),
                Props::new(),
            )],
            Some("running"),
        ));
    }
    let mut notes: Vec<Span> = Vec::new();
    let mut note = |value: String, token: &str| {
        if !notes.is_empty() {
            notes.push(span("\n", None));
        }
        notes.push(span(value, Some(token)));
    };
    if input.status == ExecutionStatus::Cancelled {
        note("(cancelled)".to_string(), "warning");
    } else if input.status == ExecutionStatus::Error {
        note(format!("(exit {})", input.exit_code.unwrap_or(0)), "error");
    }
    if let Some(truncation) = &input.truncation {
        note(format_truncation_meta_notice(truncation, None), "warning");
    }
    if let Some(minimized) = &input.minimized {
        note(format!("Minimized: {}", format_minimized_meta(minimized)), "muted");
    }
    if let Some(error) = &input.artifact_error {
        note(format_artifact_error_notice(error), "warning");
    }
    if !notes.is_empty() {
        let mut props = Props::new();
        props.set_str("wrap", "word");
        children.push(crate::native::describe::keyed(
            text(NodeText::Spans(notes), props),
            "notes",
        ));
    }
    let mut props = Props::new();
    props.set_str("role", input.role);
    if input.muted {
        props.set_tone(Tone::Muted);
    }
    props.set_str("status", input.status.card_status());
    props.set_bool("collapsible", true);
    props.set_bool("collapsed", !input.expanded);
    let mut preview = Props::new();
    preview.set_i64("lines", PREVIEW_LINES as i64);
    props.insert("preview", preview.to_value());
    card(props, children)
}

/// Inputs of [`describe_execution_tool`].
#[derive(Clone, Debug)]
pub struct ExecutionToolInput<'a> {
    /// Accessible role.
    pub role: &'a str,
    /// Tool name for the head icon (`bash`, `eval`).
    pub name: &'a str,
    /// Human title of the run.
    pub title: &'a str,
    /// The command or cell source.
    pub command: &'a str,
    /// Language id.
    pub lang: &'a str,
    /// Excluded from context (`!!`/`$$`).
    pub excluded: bool,
    /// Lifecycle state.
    pub status: ExecutionStatus,
    /// Start of the run, in milliseconds.
    pub started_at_ms: f64,
    /// End of the run, in milliseconds.
    pub ended_at_ms: Option<f64>,
    /// Now, in milliseconds.
    pub now_ms: f64,
    /// Whether the block shows its whole body.
    pub expanded: bool,
    /// Raw terminal output.
    pub output: String,
    /// Rendered images.
    pub images: Vec<NativeNode>,
    /// Exit code of a settled run.
    pub exit_code: Option<i64>,
    /// Truncation metadata.
    pub truncation: Option<TruncationMeta>,
    /// Artifact capture error.
    pub artifact_error: Option<String>,
    /// Bash minimizer metadata.
    pub minimized: Option<MinimizedMeta>,
}

impl<'a> Default for ExecutionToolInput<'a> {
    fn default() -> Self {
        ExecutionToolInput {
            role: "",
            name: "",
            title: "",
            command: "",
            lang: "",
            excluded: false,
            status: ExecutionStatus::Running,
            started_at_ms: 0.0,
            ended_at_ms: None,
            now_ms: 0.0,
            expanded: false,
            output: String::new(),
            images: Vec::new(),
            exit_code: None,
            truncation: None,
            artifact_error: None,
            minimized: None,
        }
    }
}

/// A user `!`/`$` run as the agent's `tool` frame: a head with a `you` badge
/// (plus `not sent` when excluded) and the command for Copy command; the body
/// is one run box with the source, the output terminal, and the foot.
pub fn describe_execution_tool(input: &ExecutionToolInput<'_>) -> NativeNode {
    let running = input.status == ExecutionStatus::Running;
    let shell = input.name == "bash";
    let ms = (input.ended_at_ms.unwrap_or(input.now_ms) - input.started_at_ms).max(0.0);
    let state = input.status.run_state();
    let mut meta = OutputMeta::default();
    meta.truncation = input.truncation.clone();
    meta.artifact_error = input.artifact_error.clone();
    let box_node = run_box(
        &[
            Some(run_input(
                input.command,
                &RunInputProps {
                    role: if shell {
                        "omp.tool.bash.command".to_string()
                    } else {
                        "omp.tool.eval.input".to_string()
                    },
                    lang: input.lang.to_string(),
                    wrap: shell,
                },
            )),
            (!input.output.is_empty()).then(|| {
                run_output(
                    &input.output,
                    &RunOutputProps {
                        role: if shell {
                            "omp.tool.bash.output".to_string()
                        } else {
                            "omp.tool.eval.output".to_string()
                        },
                        preview_lines: DEFAULT_TERMINAL_PREVIEW_LINES,
                        key: None,
                    },
                )
            }),
            Some(run_foot(&RunFootInput {
                state: Some(state),
                exit_code: input.exit_code,
                elapsed_ms: Some(ms),
                facts: input
                    .minimized
                    .as_ref()
                    .map(|minimized| vec![format!("Minimized {}", format_minimized_meta(minimized))]),
                meta: Some(meta),
            })),
        ],
        &RunBoxProps {
            key: "run".to_string(),
            tone: if state == RunState::Failed {
                Some(Tone::Error)
            } else {
                None
            },
        },
    );
    let badges = if input.excluded { "you, not sent" } else { "you" };
    let mut props = Props::new();
    props.set_str("role", input.role);
    props.set_str("name", input.name);
    props.set_str("title", input.title);
    props.set_str("command", input.command);
    props.set_str("badges", badges);
    props.set_str("status", input.status.card_status());
    if running {
        props.set_f64("age", ms);
    } else {
        props.set_f64("took", ms);
    }
    if input.status == ExecutionStatus::Error {
        props.set_i64("exit", input.exit_code.unwrap_or(0));
    }
    if input.status == ExecutionStatus::Cancelled {
        props.set_str("note", "cancelled");
    }
    props.set_str("frame", "card");
    props.set_bool("collapsible", true);
    props.set_bool("collapsed", !input.expanded);
    props.set_str("preview", "children");
    let mut children = vec![box_node];
    children.extend(input.images.iter().cloned());
    node(NodeKind::Tool, props, children, None)
}

/// Derive the post-run status from an exit code + cancellation flag using the
/// same precedence both execution components apply.
pub fn resolve_execution_status(exit_code: Option<i64>, cancelled: bool) -> ExecutionStatus {
    if cancelled {
        return ExecutionStatus::Cancelled;
    }
    match exit_code {
        Some(code) if code != 0 => ExecutionStatus::Error,
        _ => ExecutionStatus::Complete,
    }
}

fn props_with(pairs: &[(&str, &str)]) -> Props {
    let mut props = Props::new();
    for (key, value) in pairs {
        props.set_str(key, *value);
    }
    props
}

fn keyed_props_text(
    content: NodeText,
    configure: impl FnOnce(&mut Props),
    key: &str,
) -> NativeNode {
    let mut props = Props::new();
    configure(&mut props);
    crate::native::describe::keyed(text(content, props), key)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn status_priority_prefers_cancellation_then_exit_code() {
        assert_eq!(resolve_execution_status(Some(0), true), ExecutionStatus::Cancelled);
        assert_eq!(resolve_execution_status(Some(2), false), ExecutionStatus::Error);
        assert_eq!(resolve_execution_status(Some(0), false), ExecutionStatus::Complete);
        assert_eq!(resolve_execution_status(None, false), ExecutionStatus::Complete);
    }

    #[test]
    fn status_labels_name_the_outcome() {
        let theme = theme();
        assert!(execution_status_label(ExecutionStatus::Complete, Some(0), &theme).is_none());
        assert!(crate::ansi_scan::strip_ansi(
            &execution_status_label(ExecutionStatus::Cancelled, None, &theme).unwrap()
        )
        .contains("cancelled"));
        assert!(crate::ansi_scan::strip_ansi(
            &execution_status_label(ExecutionStatus::Error, Some(3), &theme).unwrap()
        )
        .contains("exit 3"));
    }

    #[test]
    fn clamping_keeps_short_lines_and_marks_long_ones() {
        assert_eq!(clamp_display_line("short"), "short");
        let long = "x".repeat(MAX_DISPLAY_LINE_CHARS + 25);
        let clamped = clamp_display_line(&long);
        assert!(clamped.contains("25 visible columns omitted"));
    }

    #[test]
    fn footer_rows_report_hidden_lines_and_truncation() {
        let theme = theme();
        assert!(build_status_footer(&StatusFooterOptions::default(), &theme).is_empty());
        let rows = build_status_footer(
            &StatusFooterOptions {
                hidden_line_count: 5,
                ..StatusFooterOptions::default()
            },
            &theme,
        );
        assert_eq!(rows.len(), 1);
        let plain = crate::ansi_scan::strip_ansi(&rows[0]);
        assert!(plain.contains("5 more lines"), "{plain}");
        assert!(plain.contains("to expand"), "{plain}");
        let suppressed = build_status_footer(
            &StatusFooterOptions {
                hidden_line_count: 5,
                suppress_hidden_count: true,
                ..StatusFooterOptions::default()
            },
            &theme,
        );
        assert!(suppressed.is_empty());
    }

    #[test]
    fn cards_carry_status_and_notes() {
        let input = ExecutionCardInput {
            role: "omp.bash",
            head: vec![span("$ ls", Some("bashMode strong"))],
            status: ExecutionStatus::Error,
            exit_code: Some(2),
            output: "boom\n".to_string(),
            ..ExecutionCardInput::default()
        };
        let node = describe_execution_card(&input);
        assert_eq!(node.props.get_str("status"), Some("error"));
        assert_eq!(node.props.get_str("role"), Some("omp.bash"));
        assert!(node.children.len() >= 3, "head, output and notes");
    }

    #[test]
    fn tool_frames_expose_the_command_and_run_box() {
        let input = ExecutionToolInput {
            role: "omp.bash",
            name: "bash",
            title: "Bash",
            command: "ls",
            lang: "bash",
            status: ExecutionStatus::Running,
            started_at_ms: 1000.0,
            now_ms: 1500.0,
            output: "a\n".to_string(),
            ..ExecutionToolInput::default()
        };
        let node = describe_execution_tool(&input);
        assert_eq!(node.kind, NodeKind::Tool);
        assert_eq!(node.props.get_str("command"), Some("ls"));
        assert_eq!(node.props.get_str("status"), Some("running"));
        assert_eq!(node.children.len(), 1, "run box plus no images");
    }

    #[test]
    fn minimized_metadata_is_summarized() {
        let minimized = MinimizedMeta {
            filter: "test-tap".to_string(),
            input_bytes: 2048,
            output_bytes: 64,
            artifact_id: "abc".to_string(),
        };
        let text = format_minimized_meta(&minimized);
        assert!(text.contains("test-tap"), "{text}");
        assert!(text.contains("artifact://abc"), "{text}");
    }
}
