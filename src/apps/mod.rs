//! App-level modules ported from the `l/src/apps` tree.
//!
//! Each module mirrors one reference app and owns its tests. UI apps build on
//! [`maple_tui::Component`]; pure helpers stay free of TUI state so they can
//! be exercised directly.

pub mod debug;
pub mod autoresearch_data;
pub mod git;
pub mod ps_data;
