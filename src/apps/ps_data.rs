//! `omp ps` table data: broker scopes, daemon rows and their display cells.
//!
//! Reference: `l/src/apps/ps-data.ts`. The reference imports the daemon types
//! from `l/src/tools/daemon` and durations from `@oh-my-pi/pi-utils`; neither is
//! ported yet, so [`DaemonState`], [`DaemonSpec`] and [`DaemonSnapshot`] are
//! declared here and will move to the daemon module when that port lands.
//!
//! Deviations:
//! - [`uptime_cell`] takes the elapsed milliseconds instead of reading the wall
//!   clock, so the helper stays deterministic and testable.
//! - chalk's SGR resets become maple-tui's `\x1b[0m` reset; the visible colors
//!   and attributes match.

use std::collections::BTreeMap;
use std::path::Path;

use maple_tui::status_line::segments::format_duration;
use maple_tui::style::{Color, Style};

/// Stable lifecycle states exposed by the launch broker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonState {
    Starting,
    Running,
    Ready,
    Restarting,
    Stopping,
    Exited,
    Failed,
}

impl DaemonState {
    /// Wire/display name, matching the broker's serialized state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Restarting => "restarting",
            Self::Stopping => "stopping",
            Self::Exited => "exited",
            Self::Failed => "failed",
        }
    }

    /// Process states without a running process or live uptime.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Exited | Self::Failed)
    }
}

/// Restart behavior applied after an unexpected daemon exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonRestartPolicy {
    No,
    OnFailure,
    Always,
}

impl DaemonRestartPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::OnFailure => "on-failure",
            Self::Always => "always",
        }
    }
}

/// One readiness condition that must pass before `ready`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonReadyCondition {
    Log,
    Port,
}

/// Readiness conditions; every configured condition must pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonReadySpec {
    pub log: Option<String>,
    pub port: Option<u16>,
    pub host: Option<String>,
    pub timeout_ms: u64,
}

/// Immutable launch specification retained for restart and inspection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonSpec {
    pub name: String,
    pub application: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub pty: bool,
    pub ready: Option<DaemonReadySpec>,
    pub restart: DaemonRestartPolicy,
    pub persist: bool,
    pub detached: bool,
}

/// Serializable daemon state visible to every client in one broker scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonSnapshot {
    pub name: String,
    pub id: String,
    pub state: DaemonState,
    pub pid: Option<u32>,
    pub created_at: i64,
    pub started_at: i64,
    pub ready_at: Option<i64>,
    pub exited_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub exit_reason: Option<String>,
    pub restart_count: u32,
    pub output_bytes: u64,
    pub owner: Option<String>,
    pub ready_match: Option<String>,
    pub ready_pending: Option<Vec<DaemonReadyCondition>>,
    pub persist: bool,
    pub detached: bool,
}

/// One broker scope: a project runtime dir or a machine-global service dir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsScope {
    pub kind: PsScopeKind,
    pub runtime_dir: String,
    /// Canonical project dir when known; used to connect and displayed as the scope label.
    pub project_dir: Option<String>,
    /// Global service name (`kind == Global`).
    pub service: Option<String>,
    /// Live broker PID; `None` when no broker owns the scope.
    pub broker_pid: Option<u32>,
}

/// Which kind of broker scope a row group belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsScopeKind {
    Project,
    Global,
}

/// One process snapshot and its table display metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsDaemonRow {
    pub snapshot: DaemonSnapshot,
    /// Launch command from the persisted spec, when readable.
    pub command: Option<String>,
    pub cwd: Option<String>,
    /// False when the snapshot came from disk with no live broker supervising it.
    pub supervised: bool,
}

/// Process rows grouped under their owning broker scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsScopeReport {
    pub scope: PsScope,
    pub daemons: Vec<PsDaemonRow>,
}

/// Scope selector shared by every ps action: current project, `--dir`, or `--global`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PsTarget {
    pub dir: Option<String>,
    pub global: Option<String>,
}

/// Combine an executable and its arguments for display.
pub fn format_command(spec: Option<&DaemonSpec>) -> Option<String> {
    spec.map(|spec| {
        let mut parts = Vec::with_capacity(spec.args.len() + 1);
        parts.push(spec.application.clone());
        parts.extend(spec.args.iter().cloned());
        parts.join(" ")
    })
}

/// Collapse a launch command to one display line (inline scripts embed newlines/tabs).
pub fn collapse_command(command: Option<&str>) -> String {
    match command {
        Some(command) => {
            let mut collapsed = String::with_capacity(command.len());
            let mut pending_space = false;
            for ch in command.chars() {
                if ch.is_whitespace() {
                    pending_space = !collapsed.is_empty();
                    continue;
                }
                if pending_space {
                    collapsed.push(' ');
                    pending_space = false;
                }
                collapsed.push(ch);
            }
            collapsed
        }
        None => String::new(),
    }
}

/// One-line daemon summary used by action results and detail views.
pub fn daemon_label(daemon: &DaemonSnapshot) -> String {
    let pid = match daemon.pid {
        Some(pid) => format!(" pid={pid}"),
        None => String::new(),
    };
    let exit = match daemon.exit_code {
        Some(code) => format!(" exit={code}"),
        None => String::new(),
    };
    format!("{}: {}{}{}", daemon.name, daemon.state.as_str(), pid, exit)
}

/// Colored STATE cell, e.g. `ready`, `exited(143)`.
pub fn state_cell(row: &PsDaemonRow) -> String {
    let snapshot = &row.snapshot;
    let mut text = snapshot.state.as_str().to_string();
    if snapshot.state.is_terminal() {
        if let Some(code) = snapshot.exit_code {
            text.push_str(&format!("({code})"));
        }
    }
    let style = match snapshot.state {
        DaemonState::Ready | DaemonState::Running => Style::fg(Color::Green),
        DaemonState::Failed => Style::fg(Color::Red),
        state if state.is_terminal() => Style::default().dim(),
        _ => Style::fg(Color::Yellow),
    };
    style.paint(&text)
}

/// Display persistence and supervision flags for a process row.
pub fn flags_cell(row: &PsDaemonRow) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(2);
    if row.snapshot.detached {
        parts.push("detached");
    } else if row.snapshot.persist {
        parts.push("persist");
    }
    if !row.supervised && !row.snapshot.state.is_terminal() {
        parts.push("unsupervised");
    }
    parts.join(",")
}

/// Display elapsed runtime, or a dash for a terminal process.
///
/// `elapsed_ms` is measured from [`DaemonSnapshot::started_at`]; the caller
/// supplies it so the cell does not read the clock itself.
pub fn uptime_cell(snapshot: &DaemonSnapshot, elapsed_ms: u64) -> String {
    if snapshot.state.is_terminal() {
        return "-".to_string();
    }
    format_duration(elapsed_ms)
}

/// Labels aligned with the process table's display cells.
pub const TABLE_HEADER: [&str; 7] = [
    "NAME", "STATE", "PID", "UPTIME", "RESTARTS", "FLAGS", "COMMAND",
];

/// Raw (possibly colored) cells for one daemon row, aligned with [`TABLE_HEADER`].
pub fn table_cells(row: &PsDaemonRow, elapsed_ms: u64) -> Vec<String> {
    let snapshot = &row.snapshot;
    let pid = match snapshot.pid {
        Some(pid) if !snapshot.state.is_terminal() => pid.to_string(),
        _ => "-".to_string(),
    };
    vec![
        snapshot.name.clone(),
        state_cell(row),
        pid,
        uptime_cell(snapshot, elapsed_ms),
        snapshot.restart_count.to_string(),
        flags_cell(row),
        collapse_command(row.command.as_deref()),
    ]
}

/// Last path component of a directory, falling back to the input when none.
fn basename(dir: &str) -> &str {
    Path::new(dir)
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or(dir)
}

/// Scope heading, e.g. `project /work/pi — broker pid 1234`.
pub fn scope_header(scope: &PsScope) -> String {
    let fallback = basename(&scope.runtime_dir);
    let label = match scope.kind {
        PsScopeKind::Global => {
            let service = scope.service.as_deref().unwrap_or(fallback);
            format!("global {}", Style::default().bold().paint(service))
        }
        PsScopeKind::Project => {
            let project = scope.project_dir.as_deref().unwrap_or(fallback);
            format!("project {}", Style::default().bold().paint(project))
        }
    };
    let broker = match scope.broker_pid {
        Some(pid) => Style::fg(Color::Green).paint(&format!("broker pid {pid}")),
        None => Style::default().dim().paint("broker not running"),
    };
    format!("{label} {}{broker}", Style::default().dim().paint("—"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(state: DaemonState) -> DaemonSnapshot {
        DaemonSnapshot {
            name: "api".into(),
            id: "api-1".into(),
            state,
            pid: Some(4242),
            created_at: 1_000,
            started_at: 1_000,
            ready_at: None,
            exited_at: None,
            exit_code: None,
            exit_reason: None,
            restart_count: 2,
            output_bytes: 0,
            owner: None,
            ready_match: None,
            ready_pending: None,
            persist: false,
            detached: false,
        }
    }

    fn row(state: DaemonState) -> PsDaemonRow {
        PsDaemonRow {
            snapshot: snapshot(state),
            command: Some("bin/server --port\n\t8080".into()),
            cwd: Some("/work/pi".into()),
            supervised: true,
        }
    }

    #[test]
    fn commands_join_application_and_args() {
        let spec = DaemonSpec {
            name: "api".into(),
            application: "bin/server".into(),
            args: vec!["--port".into(), "8080".into()],
            env: BTreeMap::new(),
            cwd: "/work/pi".into(),
            pty: false,
            ready: None,
            restart: DaemonRestartPolicy::No,
            persist: false,
            detached: false,
        };
        assert_eq!(format_command(Some(&spec)).as_deref(), Some("bin/server --port 8080"));
        assert_eq!(format_command(None), None);
    }

    #[test]
    fn collapsed_commands_become_one_line() {
        assert_eq!(collapse_command(Some("a\n\tb  c")), "a b c");
        assert_eq!(collapse_command(Some("   ")), "");
        assert_eq!(collapse_command(None), "");
    }

    #[test]
    fn labels_include_pid_and_exit() {
        let mut daemon = snapshot(DaemonState::Exited);
        daemon.exit_code = Some(143);
        assert_eq!(daemon_label(&daemon), "api: exited pid=4242 exit=143");
        daemon.pid = None;
        assert_eq!(daemon_label(&daemon), "api: exited exit=143");
    }

    #[test]
    fn state_cells_carry_exit_code_and_color() {
        let mut exited = row(DaemonState::Exited);
        exited.snapshot.exit_code = Some(1);
        let cell = state_cell(&exited);
        assert!(cell.contains("exited(1)"));
        assert!(cell.starts_with("\x1b["));

        let ready = state_cell(&row(DaemonState::Ready));
        assert!(ready.contains("ready"));
        assert!(!ready.contains("dim"));

        let failed = state_cell(&row(DaemonState::Failed));
        assert!(failed.contains("failed"));
    }

    #[test]
    fn flags_report_persistence_and_supervision() {
        assert_eq!(flags_cell(&row(DaemonState::Running)), "");
        let mut persisted = row(DaemonState::Running);
        persisted.snapshot.persist = true;
        assert_eq!(flags_cell(&persisted), "persist");
        let mut detached = row(DaemonState::Running);
        detached.snapshot.detached = true;
        assert_eq!(flags_cell(&detached), "detached");
        let mut unsupervised = row(DaemonState::Starting);
        unsupervised.supervised = false;
        assert_eq!(flags_cell(&unsupervised), "unsupervised");
        // A terminal row from disk is not called unsupervised.
        let mut terminal = row(DaemonState::Exited);
        terminal.supervised = false;
        assert_eq!(flags_cell(&terminal), "");
    }

    #[test]
    fn uptime_is_dash_for_terminal_states() {
        assert_eq!(uptime_cell(&snapshot(DaemonState::Exited), 5_000), "-");
        assert_eq!(uptime_cell(&snapshot(DaemonState::Failed), 5_000), "-");
        assert_eq!(uptime_cell(&snapshot(DaemonState::Running), 30_000), "30.0s");
    }

    #[test]
    fn table_cells_align_with_header() {
        let cells = table_cells(&row(DaemonState::Running), 90_000);
        assert_eq!(cells.len(), TABLE_HEADER.len());
        assert_eq!(cells[0], "api");
        assert!(cells[1].contains("running"));
        assert_eq!(cells[2], "4242");
        assert_eq!(cells[3], "1m30s");
        assert_eq!(cells[4], "2");
        assert_eq!(cells[6], "bin/server --port 8080");

        let mut exited = row(DaemonState::Exited);
        exited.snapshot.pid = None;
        let cells = table_cells(&exited, 90_000);
        assert_eq!(cells[2], "-");
        assert_eq!(cells[3], "-");
    }

    #[test]
    fn scope_headers_name_the_scope_and_broker() {
        let mut scope = PsScope {
            kind: PsScopeKind::Project,
            runtime_dir: "/run/maple/proj".into(),
            project_dir: Some("/work/pi".into()),
            service: None,
            broker_pid: Some(1234),
        };
        let header = scope_header(&scope);
        assert!(header.contains("project "));
        assert!(header.contains("/work/pi"));
        assert!(header.contains("broker pid 1234"));

        scope.project_dir = None;
        assert!(scope_header(&scope).contains("proj"));

        scope.kind = PsScopeKind::Global;
        scope.service = Some("builder".into());
        scope.broker_pid = None;
        let header = scope_header(&scope);
        assert!(header.contains("global "));
        assert!(header.contains("builder"));
        assert!(header.contains("broker not running"));
    }
}
