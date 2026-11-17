//! Tool renderers and execution state.
//! Reference: `l/src/tools/index.ts`.
//!
//! Entry point for the `tools/` slice. `tool_execution` carries the lifecycle
//! state machine, `shell_viz` decodes and renders native shell presentations,
//! and `bash_execution`/`eval_execution` are the bounded tool cards. Cards
//! reuse [`crate::render::tool_card::ToolCard`] and
//! [`crate::render::output_block`].

pub mod bash_execution;
pub mod eval_execution;
pub mod eval_format;
pub mod shell_viz;
pub mod tool_execution;
pub mod ask;
pub mod autoresearch;

pub mod agent_progress;
pub mod agent_tree;
pub mod ast_edit;
pub mod ast_grep;
pub mod bash_interactive;
pub mod cfg_render;
pub mod cfg_url;
pub mod conflict_detect;
pub mod daemon;
pub mod debug;
pub mod default_renderer;
pub mod edit;
pub mod fetch;
pub mod find;
pub mod gh_format;
pub mod github;
pub mod glob;
pub mod goal;
pub mod grep;
pub mod grouped_file_output;
pub mod hashline_format;
pub mod irc;
pub mod job_widget;
pub mod json_tree;
pub mod line_ranges;
pub mod list_limit;
pub mod lsp;
pub mod match_line_format;
pub mod mcp;
pub mod memory;
pub mod native_view;
pub mod output_meta;
pub mod proc_render;
pub mod read;
pub mod report_tool_issue;
pub mod resolve;
pub mod result_card;
pub mod streaming_output;
pub mod subprocess;
pub mod task;
pub mod task_repair_args;
pub mod task_yield_assembly;
pub mod terminal_output;
pub mod think;
pub mod todo;
pub mod tool_errors;
pub mod url_scheme_host;
pub mod vibe;
pub mod wait;
pub mod web_search;
pub mod web_search_types;
pub mod write;
pub mod xd_url;
pub mod xdev;
pub mod r#yield;


pub use bash_execution::{
    format_background_notice, format_bash_command_lines, format_bash_command_text,
    format_bash_env_assignments, format_exit_code_notice, format_wall_time_notice,
    is_background_notice, BashExecution, BashRenderArgs, BashToolDetails, ShellRunState,
    BASH_DEFAULT_PREVIEW_LINES, WALL_TIME_NOTICE_MIN_MS,
};
pub use eval_execution::{format_eval_code_for_display, EvalExecution, EvalLanguage};
pub use shell_viz::{
    decode_shell_presentation, default_shell_viz_preferences, is_shell_viz_node, render_shell_visualization,
    sanitize_text, shell_viz_text, ShellVizEffect, ShellVizEffectKind, ShellVizLayers, ShellVizNode,
    ShellVizPresentation, ShellVizPreferences, ShellVizRange, ShellVizRole, ShellVizRow, ShellVizSegment,
    ShellVizState, ShellVizTimeline, ShellVizView, ShellVizWatch,
};
pub use tool_execution::{ToolExecution, ToolExecutionState};
