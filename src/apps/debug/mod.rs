//! Debug app: log viewer, raw SSE buffers, protocol probes, terminal state.

pub mod log_formatting;

pub use log_formatting::{
    format_debug_log_expanded_lines, parse_debug_log_pid, parse_debug_log_timestamp_ms,
};
