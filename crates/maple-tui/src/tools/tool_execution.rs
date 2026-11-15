//! Tool execution lifecycle: the state every tool card transitions through.
//!
//! Ported from the `ToolExecutionState`/`ToolUiStatus` vocabulary shared by
//! `l/src/tools/renderer.ts` and `l/src/render/tool-card.ts`.

use crate::render::render_utils::{State, ToolUiStatus};

/// Lifecycle state of one tool call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolExecutionState {
    /// Accepted, not started.
    #[default]
    Pending,
    /// Running.
    Running,
    /// Finished successfully.
    Success,
    /// Finished with a failure.
    Error,
    /// Killed before finishing.
    Aborted,
    /// Finished with a warning.
    Warning,
}

impl ToolExecutionState {
    /// Lower-case label used by status text and tests.
    pub fn label(self) -> &'static str {
        match self {
            ToolExecutionState::Pending => "pending",
            ToolExecutionState::Running => "running",
            ToolExecutionState::Success => "success",
            ToolExecutionState::Error => "error",
            ToolExecutionState::Aborted => "aborted",
            ToolExecutionState::Warning => "warning",
        }
    }

    /// Whether no further transition can occur.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ToolExecutionState::Success
                | ToolExecutionState::Error
                | ToolExecutionState::Aborted
                | ToolExecutionState::Warning
        )
    }

    /// Whether the call is still in flight.
    pub fn is_running(self) -> bool {
        matches!(self, ToolExecutionState::Pending | ToolExecutionState::Running)
    }

    /// The shared status glyph for this state.
    pub fn ui_status(self) -> ToolUiStatus {
        match self {
            ToolExecutionState::Pending => ToolUiStatus::Pending,
            ToolExecutionState::Running => ToolUiStatus::Running,
            ToolExecutionState::Success => ToolUiStatus::Success,
            ToolExecutionState::Error => ToolUiStatus::Error,
            ToolExecutionState::Aborted => ToolUiStatus::Aborted,
            ToolExecutionState::Warning => ToolUiStatus::Warning,
        }
    }

    /// The output-frame state this maps onto.
    pub fn output_state(self) -> Option<State> {
        match self {
            ToolExecutionState::Pending => Some(State::Pending),
            ToolExecutionState::Running => Some(State::Running),
            ToolExecutionState::Success => Some(State::Success),
            ToolExecutionState::Error => Some(State::Error),
            ToolExecutionState::Warning => Some(State::Warning),
            ToolExecutionState::Aborted => None,
        }
    }
}

/// A tool call's execution record: identity, title, state, and terminal facts.
#[derive(Clone, Debug)]
pub struct ToolExecution {
    name: String,
    title: String,
    state: ToolExecutionState,
    exit_code: Option<i64>,
    timed_out: bool,
    transitions: usize,
}

impl Default for ToolExecution {
    fn default() -> Self {
        ToolExecution::new("tool")
    }
}

impl ToolExecution {
    /// A pending call named `name`.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        ToolExecution {
            title: name.clone(),
            name,
            state: ToolExecutionState::Pending,
            exit_code: None,
            timed_out: false,
            transitions: 0,
        }
    }

    /// The tool name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The displayed title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Set the displayed title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// The current state.
    pub fn state(&self) -> ToolExecutionState {
        self.state
    }

    /// Whether the call already finished.
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Whether the call is still running.
    pub fn is_running(&self) -> bool {
        self.state.is_running()
    }

    /// Exit code of a command that ran to completion.
    pub fn exit_code(&self) -> Option<i64> {
        self.exit_code
    }

    /// Whether the call was killed by its timeout deadline.
    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    /// Number of state transitions observed.
    pub fn transitions(&self) -> usize {
        self.transitions
    }

    /// Mark the call running; a no-op once terminal.
    pub fn start(&mut self) -> bool {
        self.transition(ToolExecutionState::Running)
    }

    /// Mark the call succeeded.
    pub fn succeed(&mut self) -> bool {
        self.transition(ToolExecutionState::Success)
    }

    /// Mark the call failed with an optional exit code.
    pub fn fail(&mut self, exit_code: Option<i64>) -> bool {
        self.exit_code = exit_code;
        self.transition(ToolExecutionState::Error)
    }

    /// Mark the call aborted (killed before finishing).
    pub fn abort(&mut self) -> bool {
        self.transition(ToolExecutionState::Aborted)
    }

    /// Mark the call finished with a warning.
    pub fn warn(&mut self) -> bool {
        self.transition(ToolExecutionState::Warning)
    }

    /// Record that the call hit its timeout deadline.
    pub fn set_timed_out(&mut self, timed_out: bool) {
        self.timed_out = timed_out;
    }

    fn transition(&mut self, next: ToolExecutionState) -> bool {
        if self.state.is_terminal() || self.state == next {
            return false;
        }
        self.state = next;
        self.transitions += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_moves_to_running_then_success() {
        let mut execution = ToolExecution::new("bash");
        assert_eq!(execution.state(), ToolExecutionState::Pending);
        assert!(execution.is_running());
        assert!(execution.start());
        assert_eq!(execution.state(), ToolExecutionState::Running);
        assert!(execution.succeed());
        assert!(execution.is_terminal());
        assert_eq!(execution.transitions(), 2);
    }

    #[test]
    fn terminal_states_reject_further_transitions() {
        let mut execution = ToolExecution::new("bash");
        execution.start();
        execution.succeed();
        assert!(!execution.start());
        assert!(!execution.fail(Some(1)));
        assert_eq!(execution.state(), ToolExecutionState::Success);
        assert_eq!(execution.transitions(), 2);
    }

    #[test]
    fn failure_records_exit_code() {
        let mut execution = ToolExecution::new("bash");
        execution.start();
        assert!(execution.fail(Some(2)));
        assert_eq!(execution.exit_code(), Some(2));
        assert_eq!(execution.state().ui_status(), ToolUiStatus::Error);
        assert_eq!(execution.state().output_state(), Some(State::Error));
    }

    #[test]
    fn abort_has_no_output_state() {
        let mut execution = ToolExecution::new("bash");
        execution.start();
        assert!(execution.abort());
        assert_eq!(execution.state(), ToolExecutionState::Aborted);
        assert_eq!(execution.state().output_state(), None);
        assert_eq!(execution.state().label(), "aborted");
    }

    #[test]
    fn repeated_state_is_not_a_transition() {
        let mut execution = ToolExecution::new("bash");
        assert!(execution.succeed());
        assert!(!execution.succeed());
        assert_eq!(execution.transitions(), 1);
    }
}
