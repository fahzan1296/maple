//! Debug app: log viewer, raw SSE buffers, protocol probes, terminal state.

pub mod log_formatting;
pub mod viewer_frame;
pub mod log_viewer;

pub mod terminal_info;
pub use log_formatting::{
    format_debug_log_expanded_lines, parse_debug_log_pid, parse_debug_log_timestamp_ms,
};
pub use terminal_info::{
    collect_terminal_state, format_terminal_state, TerminalEnvSignals, TerminalRuntimeState, TerminalStateInfo,
};
pub use viewer_frame::{
    DebugViewerBody, DebugViewerFrame, DebugViewerFrameContent, DebugViewerFrameContext, DebugViewerFrameOptions,
};
pub use log_viewer::{
    build_log_copy_payload, split_log_text, DebugLogViewerModel, DebugLogViewerModelOptions, ViewerRow,
    LOAD_OLDER_LABEL, SESSION_BOUNDARY_WARNING,
};
