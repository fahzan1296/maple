//! Tool renderers and execution state.
//!
//! Entry point for the `tools/` slice. `tool_execution` carries the lifecycle
//! state machine, `shell_viz` decodes and renders native shell presentations,
//! and `bash_execution`/`eval_execution` are the bounded tool cards. Cards
//! reuse [`crate::render::tool_card::ToolCard`] and
//! [`crate::render::output_block`].

pub mod bash_execution;
pub mod eval_execution;
pub mod shell_viz;
pub mod tool_execution;

pub use bash_execution::{
    format_background_notice, format_bash_command_lines, format_bash_command_text,
    format_bash_env_assignments, format_exit_code_notice, format_wall_time_notice,
    is_background_notice, BashExecution, BashRenderArgs, BashToolDetails, ShellRunState,
    BASH_DEFAULT_PREVIEW_LINES, WALL_TIME_NOTICE_MIN_MS,
};
pub use eval_execution::{
    format_eval_code_for_display, EvalExecution, EvalLanguage, EVAL_DISPLAY_MAX_LINES,
};
pub use shell_viz::{
    decode_shell_presentation, default_shell_viz_preferences, is_shell_viz_node, render_shell_visualization,
    sanitize_text, shell_viz_text, ShellVizEffect, ShellVizEffectKind, ShellVizLayers, ShellVizNode,
    ShellVizPresentation, ShellVizPreferences, ShellVizRange, ShellVizRole, ShellVizRow, ShellVizSegment,
    ShellVizState, ShellVizTimeline, ShellVizView, ShellVizWatch,
};
pub use tool_execution::{ToolExecution, ToolExecutionState};
