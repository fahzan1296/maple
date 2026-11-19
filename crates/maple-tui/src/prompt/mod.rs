//! Prompt surface: composer vocabulary and editor state, autocomplete
//! providers, prose gating, the attachment band, and the prompt-side text
//! helpers.
//!
//! Ported from `l/src/prompt/`. The pieces that own terminal frames in the
//! reference (`welcome.ts`, `custom-editor.ts`, `attachment-chips.ts`) are
//! ported as data/rendering layers: this crate's [`crate::tui::Tui`] and
//! [`crate::components::editor`] keep the frame and input concerns.

pub mod attachment_chips;
pub mod composer;
pub mod composer_attachments;
pub mod composer_cache;
pub mod custom_editor;
pub mod composer_hints;
pub mod editor_top_gap;
pub mod emoji_autocomplete;
pub mod github_ref_autocomplete;
pub mod gradient_highlight;
pub mod image_format;
pub mod image_references;
pub mod image_source;
pub mod interactive_context_helpers;
pub mod internal_url_autocomplete;
pub mod macos_spelling;
pub mod magic_keywords;
pub mod markdown_prose;
pub mod model_mention_autocomplete;
pub mod model_mention_syntax;
pub mod prompt_action_autocomplete;
pub mod prose_gate;
pub mod queue_input;
pub mod queued_messages;
pub mod skill_tokens;
pub mod tools_markdown;
pub mod typing_word;
pub mod usage_amounts;
pub mod video;
pub mod welcome;
pub mod word_completion;

