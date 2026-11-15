//! Bash tool execution renderer.
//!
//! Ported from the command-preview helpers and run-state vocabulary of
//! `l/src/tools/bash.ts`, bounded to what this crate's dependency set supports:
//! syntax highlighting, output-pane minimisation and the native run box are not
//! ported here, so the command renders as styled lines and the output as a
//! plain [`crate::render::tool_card::ToolCard`] body.

use crate::component::Component;
use crate::render::tool_card::{StatusLine, ToolCard, ToolCardPhase};
use crate::render::render_utils::ToolUiStatus;
use crate::text_shape::visible_width;
use crate::theme::Theme;
use crate::tools::tool_execution::{ToolExecution, ToolExecutionState};

/// Default collapsed shell output preview height.
pub const BASH_DEFAULT_PREVIEW_LINES: usize = 8;

/// Minimum duration for a model-facing wall-time notice.
pub const WALL_TIME_NOTICE_MIN_MS: u64 = 1000;

/// LLM-facing footer appended when a tool call becomes a background job.
pub fn format_background_notice(job_id: &str, timeout_sec: Option<u64>) -> String {
    let deadline = match timeout_sec {
        Some(seconds) => format!(
            " (killed once it has run {seconds}s in total; `timeout: 0` disables the deadline)"
        ),
        None => " (no deadline)".to_string(),
    };
    format!(
        "Backgrounded as job {job_id}{deadline}; its output is injected into the conversation as a follow-up the moment it finishes. Do NOT poll for it (no `sleep`, `ps`, `pgrep`, `top`, `pidwait`, log tailing): every poll is a wasted turn. Do other work, or end your reply and wait to be woken."
    )
}

/// Whether `line` is the background notice for `job_id`.
pub fn is_background_notice(line: &str, job_id: &str) -> bool {
    let prefix = format!("Backgrounded as job {job_id}");
    line.starts_with(&prefix)
        && line[prefix.len()..]
            .chars()
            .next()
            .is_some_and(|ch| ch == ';' || ch == ' ')
}

fn format_wall_time_seconds(wall_time_ms: u64) -> String {
    format!("{:.2}", wall_time_ms as f64 / 1000.0)
}

/// The model-facing command duration notice.
pub fn format_wall_time_notice(wall_time_ms: u64) -> String {
    format!(
        "Wall time: {} seconds",
        format_wall_time_seconds(wall_time_ms)
    )
}

/// The model-facing command exit status notice.
pub fn format_exit_code_notice(exit_code: i64) -> String {
    format!("Command exited with code {exit_code}")
}

fn escape_bash_env_value_for_display(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            other => out.push(other),
        }
    }
    out
}

/// Render `KEY="value"` assignments sorted by key.
pub fn format_bash_env_assignments(env: &[(String, String)]) -> String {
    let mut entries: Vec<&(String, String)> = env.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
        .iter()
        .map(|(key, value)| format!("{key}=\"{}\"", escape_bash_env_value_for_display(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Display args for a bash command preview.
#[derive(Clone, Debug, Default)]
pub struct BashRenderArgs {
    /// Command source.
    pub command: String,
    /// Environment assignments.
    pub env: Vec<(String, String)>,
    /// Working directory override.
    pub cwd: Option<String>,
    /// `;` when the model wrote the hoisted `cd` with a semicolon.
    pub cd_separator: Option<String>,
}

fn cd_prefix(display_workdir: &str, separator: Option<&str>) -> String {
    let join = match separator {
        Some(";") => ";",
        _ => " &&",
    };
    format!("cd {display_workdir}{join}")
}

/// The dim `$ cd <dir> && NAME="…" ` prefix ahead of a command's first line.
pub fn bash_command_prefix(args: &BashRenderArgs, theme: &Theme) -> String {
    let mut parts = vec!["$".to_string()];
    if let Some(cwd) = &args.cwd {
        parts.push(cd_prefix(cwd, args.cd_separator.as_deref()));
    }
    let env = format_bash_env_assignments(&args.env);
    if !env.is_empty() {
        parts.push(env);
    }
    theme
        .fg("dim", &format!("{} ", parts.join(" ")))
        .unwrap_or_else(|_| format!("{} ", parts.join(" ")))
}

/// Plain `cd <dir> && NAME="…" <command>` line for native views.
pub fn format_bash_command_text(args: &BashRenderArgs) -> String {
    let mut parts = Vec::new();
    if let Some(cwd) = &args.cwd {
        parts.push(cd_prefix(cwd, args.cd_separator.as_deref()));
    }
    let env = format_bash_env_assignments(&args.env);
    if !env.is_empty() {
        parts.push(env);
    }
    let command = if args.command.is_empty() {
        "…".to_string()
    } else {
        args.command.replace('\t', "  ")
    };
    parts.push(command);
    parts.join(" ")
}

/// The command formatted for the result body: the dim prefix joined with the
/// command lines, the prefix applied to the first line only.
pub fn format_bash_command_lines(args: &BashRenderArgs, theme: &Theme) -> Vec<String> {
    let command = if args.command.is_empty() {
        "…".to_string()
    } else {
        args.command.replace('\t', "  ")
    };
    let prefix = bash_command_prefix(args, theme);
    let lines: Vec<String> = command.split('\n').map(str::to_string).collect();
    if lines.is_empty() {
        return vec![prefix.trim_end().to_string()];
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{prefix}{line}")
            } else {
                line
            }
        })
        .collect()
}

/// The foot state of a shell call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellRunState {
    /// Finished successfully.
    Done,
    /// Failed.
    Failed,
    /// Killed by the deadline.
    TimedOut,
    /// Still running.
    Running,
    /// Running in the background.
    Background,
    /// Killed before finishing.
    Cancelled,
}

impl ShellRunState {
    /// The card phase this run state maps onto.
    pub fn phase(self) -> ToolCardPhase {
        match self {
            ShellRunState::Done => ToolCardPhase::Success,
            ShellRunState::Failed => ToolCardPhase::Error,
            ShellRunState::TimedOut => ToolCardPhase::Warning,
            ShellRunState::Running => ToolCardPhase::Running,
            ShellRunState::Background => ToolCardPhase::Running,
            ShellRunState::Cancelled => ToolCardPhase::Pending,
        }
    }
}

/// Terminal facts of a shell call.
#[derive(Clone, Debug, Default)]
pub struct BashToolDetails {
    /// Timeout in seconds.
    pub timeout_seconds: Option<u64>,
    /// Requested timeout before clamping.
    pub requested_timeout_seconds: Option<u64>,
    /// Timeout disabled.
    pub timeout_disabled: bool,
    /// Wall time in milliseconds.
    pub wall_time_ms: Option<u64>,
    /// Non-zero exit code of a completed command.
    pub exit_code: Option<i64>,
    /// Killed by its timeout deadline.
    pub timed_out: bool,
    /// Background job state.
    pub background: Option<String>,
    /// Background job id.
    pub job_id: Option<String>,
}

/// A bash execution card: command lines, status head, and bounded output.
pub struct BashExecution {
    args: BashRenderArgs,
    details: BashToolDetails,
    output: String,
    execution: ToolExecution,
    theme: Theme,
    preview_lines: usize,
}

impl BashExecution {
    /// A bash card for `args` rendered with `theme`.
    pub fn new(args: BashRenderArgs, theme: Theme) -> Self {
        BashExecution {
            args,
            details: BashToolDetails::default(),
            output: String::new(),
            execution: ToolExecution::new("bash"),
            theme,
            preview_lines: BASH_DEFAULT_PREVIEW_LINES,
        }
    }

    /// Replace the execution record.
    pub fn set_execution(&mut self, execution: ToolExecution) {
        self.execution = execution;
    }

    /// The execution record.
    pub fn execution(&self) -> &ToolExecution {
        &self.execution
    }

    /// Replace the terminal facts.
    pub fn set_details(&mut self, details: BashToolDetails) {
        self.details = details;
    }

    /// The terminal facts.
    pub fn details(&self) -> &BashToolDetails {
        &self.details
    }

    /// Replace the command output.
    pub fn set_output(&mut self, output: impl Into<String>) {
        self.output = output.into();
    }

    /// The command output.
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Set the collapsed preview height.
    pub fn set_preview_lines(&mut self, lines: usize) {
        self.preview_lines = lines;
    }

    /// The current run state.
    pub fn run_state(&self) -> ShellRunState {
        match self.execution.state() {
            ToolExecutionState::Aborted => ShellRunState::Cancelled,
            ToolExecutionState::Warning => ShellRunState::TimedOut,
            ToolExecutionState::Success => ShellRunState::Done,
            ToolExecutionState::Error => ShellRunState::Failed,
            ToolExecutionState::Running | ToolExecutionState::Pending => {
                if self.details.background.as_deref() == Some("running") {
                    ShellRunState::Background
                } else {
                    ShellRunState::Running
                }
            }
        }
    }

    /// Footer facts shown under the output.
    pub fn footer_parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.details.background.as_deref() == Some("running") {
            if let Some(job_id) = &self.details.job_id {
                parts.push(format!("Job {job_id}"));
            }
        }
        if let Some(wall) = self.details.wall_time_ms {
            parts.push(format!("Wall: {}s", format_wall_time_seconds(wall)));
        }
        if self.details.timeout_disabled {
            parts.push("Timeout: disabled".to_string());
        } else if let Some(timeout) = self.details.timeout_seconds {
            match self.details.requested_timeout_seconds {
                Some(requested) if requested != timeout => parts.push(format!(
                    "Timeout: {timeout}s (requested {requested}s clamped)"
                )),
                _ => parts.push(format!("Timeout: {timeout}s")),
            }
        }
        if let Some(exit) = self.details.exit_code {
            if exit != 0 {
                parts.push(format!("Exit: {exit}"));
            }
        }
        parts
    }

    fn card(&mut self) -> ToolCard {
        let command_lines = format_bash_command_lines(&self.args, &self.theme);
        let mut status = StatusLine::new(self.execution.title().to_string());
        status.icon = Some(status_icon(self.run_state()));
        let mut card = ToolCard::new(self.theme.clone(), status);
        card.set_status(self.run_state().phase());
        card.set_body(command_lines);
        if !self.output.is_empty() {
            card.set_output(&self.output);
        }
        let footer = self.footer_parts();
        if !footer.is_empty() {
            card.set_footer_meta(footer.join(" · "));
        }
        card.set_collapsed(true);
        card.set_collapsed_max_lines(self.preview_lines);
        card
    }
}

fn status_icon(state: ShellRunState) -> ToolUiStatus {
    match state {
        ShellRunState::Done => ToolUiStatus::Success,
        ShellRunState::Failed => ToolUiStatus::Error,
        ShellRunState::TimedOut => ToolUiStatus::Warning,
        ShellRunState::Running | ShellRunState::Background => ToolUiStatus::Running,
        ShellRunState::Cancelled => ToolUiStatus::Aborted,
    }
}

impl Component for BashExecution {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return Vec::new();
        }
        let mut card = self.card();
        card.render_at(width)
    }

    fn handle_input(&mut self, key: &crate::keys::Key) {
        let _ = key;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_notice_states_the_deadline() {
        let notice = format_background_notice("job_1", Some(30));
        assert!(notice.starts_with("Backgrounded as job job_1"));
        assert!(notice.contains("30s in total"));
        assert!(is_background_notice(&notice, "job_1"));
        assert!(!is_background_notice(&notice, "job_2"));
        assert!(format_background_notice("job_1", None).contains("no deadline"));
    }

    #[test]
    fn wall_time_and_exit_notices_are_exact() {
        assert_eq!(format_wall_time_notice(1200), "Wall time: 1.20 seconds");
        assert_eq!(format_exit_code_notice(2), "Command exited with code 2");
    }

    #[test]
    fn env_assignments_are_sorted_and_escaped() {
        let env = vec![
            ("B".to_string(), "two".to_string()),
            ("A".to_string(), "a\"b\nc".to_string()),
        ];
        assert_eq!(format_bash_env_assignments(&env), "A=\"a\\\"b\\nc\" B=\"two\"");
    }

    #[test]
    fn command_prefix_applies_to_the_first_line_only() {
        let theme = test_theme();
        let args = BashRenderArgs {
            command: "one\ntwo".to_string(),
            env: vec![("K".to_string(), "v".to_string())],
            cwd: Some("/tmp".to_string()),
            cd_separator: Some(";".to_string()),
        };
        let lines = format_bash_command_lines(&args, &theme);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("cd /tmp;"));
        assert!(lines[0].contains("K=\"v\""));
        assert!(!lines[1].contains("cd /tmp"));
        assert!(visible_width(&lines[0]) > 0);
    }

    #[test]
    fn command_text_is_plain() {
        let args = BashRenderArgs {
            command: "ls\t-la".to_string(),
            env: vec![("K".to_string(), "v".to_string())],
            cwd: Some("/tmp".to_string()),
            cd_separator: None,
        };
        assert_eq!(format_bash_command_text(&args), "cd /tmp && K=\"v\" ls  -la");
    }

    #[test]
    fn run_state_maps_to_phases() {
        assert_eq!(ShellRunState::Done.phase(), ToolCardPhase::Success);
        assert_eq!(ShellRunState::Failed.phase(), ToolCardPhase::Error);
        assert_eq!(ShellRunState::TimedOut.phase(), ToolCardPhase::Warning);
        assert_eq!(ShellRunState::Cancelled.phase(), ToolCardPhase::Pending);
    }

    #[test]
    fn card_renders_bounded_rows() {
        let theme = test_theme();
        let mut execution = BashExecution::new(
            BashRenderArgs {
                command: "echo hello".to_string(),
                ..BashRenderArgs::default()
            },
            theme,
        );
        execution.set_output("hello\nworld\n");
        for width in [10u16, 20, 40] {
            for row in execution.render(width) {
                assert!(visible_width(&row) <= width as usize, "width {width}: {row:?}");
            }
        }
        assert!(execution.render(0).is_empty());
    }

    #[test]
    fn footer_parts_report_wall_timeout_and_exit() {
        let theme = test_theme();
        let mut execution = BashExecution::new(BashRenderArgs::default(), theme);
        execution.set_details(BashToolDetails {
            timeout_seconds: Some(30),
            requested_timeout_seconds: Some(60),
            wall_time_ms: Some(1500),
            exit_code: Some(2),
            ..BashToolDetails::default()
        });
        let footer = execution.footer_parts();
        assert!(footer.iter().any(|part| part.contains("Wall: 1.50s")));
        assert!(footer
            .iter()
            .any(|part| part == "Timeout: 30s (requested 60s clamped)"));
        assert!(footer.iter().any(|part| part == "Exit: 2"));
    }

    fn test_theme() -> Theme {
        crate::theme::create_theme(
            crate::theme::builtin::builtin_theme_json("dark").unwrap(),
            crate::theme::CreateThemeOptions::default(),
        )
        .unwrap()
    }
}
