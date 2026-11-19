//! Chat transcript model: message kinds, entry rendering and notices.
//!
//! Ported from `l/src/chat/`.

pub mod advisor_message;
pub mod assistant_message;
pub mod background_tan_message;
pub mod bash_execution;
pub mod cache_invalidation_marker;
pub mod chat_transcript_builder;
pub mod collab_prompt_message;
pub mod compaction_summary_message;
pub mod custom_message;
pub mod display_preferences;
pub mod eval_execution;
pub mod execution_shared;
pub mod extension_types;
pub mod fence_figure;
pub mod figure_markdown;
pub mod hook_message;
pub mod image_loading;
pub mod late_diagnostics_message;
pub mod messages;
pub mod reaction;
pub mod read_tool_group;
pub mod recap_notice;
pub mod reply_write;
pub mod served_model_marker;
pub mod skill_message;
pub mod skill_title_input;
pub mod spotlight;
pub mod stripped_tool_calls_placeholder;
pub mod svg_figure;
pub mod svg_source;
pub mod table_chart;
pub mod thinking_display;
pub mod todo_reminder;
pub mod tool_execution;
pub mod transcript_actions;
pub mod transcript_browser;
pub mod transcript_entry;
pub mod transcript_outline;
pub mod transcript_presentation;
pub mod transcript_render_helpers;
pub mod ttsr_notification;
pub mod user_message;
