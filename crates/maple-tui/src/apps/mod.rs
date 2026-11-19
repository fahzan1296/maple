//! Standalone full-screen applications.
//!
//! Ported from `l/src/apps/`. Each submodule owns one app; `debug` and `git`
//! carry their own nested slices.

pub mod debug;
pub mod git;

pub mod autoresearch_dashboard;
pub mod autoresearch_data;
pub mod cleanse_board;
pub mod cleanse_picker;
pub mod if_bench_board;
pub mod live_visualizer;
pub mod ps_data;
pub mod ps_top;
pub mod session_picker;
pub mod setup_model_picker;
pub mod shell_visualizer;
pub mod standalone_picker;
