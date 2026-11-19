//! Process-wide display preferences applied by the host settings hooks.
//!
//! Reference: `l/src/chat/display-preferences.ts`.
//!
//! The reference keeps one mutable module-global object that settings hooks
//! patch through `Object.assign`. Rust has no module-level mutable value, so
//! the same state lives behind a process-wide [`std::sync::RwLock`] with a
//! getter ([`chat_transcript_display_preferences`]) and a patcher
//! ([`set_chat_transcript_display_preferences`]); the patch semantics and
//! defaults are unchanged.

use std::sync::RwLock;

/// Display preferences for the chat transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatTranscriptDisplayPreferences {
    /// Draw no tool activity at all.
    pub hide_tool_activity: bool,
    /// Show the interactive shell execution visualization instead of the plain
    /// command.
    pub shell_visualization: bool,
    /// Preview the result of read tool calls.
    pub read_tool_result_preview: bool,
    /// Draw images.
    pub show_images: bool,
    /// Mark cache misses.
    pub cache_miss_marker: bool,
    /// Show per-turn token usage.
    pub show_token_usage: bool,
    /// Show per-turn wall time.
    pub show_turn_time: bool,
    /// Max rows a collapsed advisor card shows below its header; 0 = no cap.
    pub advisor_collapsed_lines: u32,
}

impl Default for ChatTranscriptDisplayPreferences {
    fn default() -> Self {
        ChatTranscriptDisplayPreferences {
            hide_tool_activity: false,
            shell_visualization: true,
            read_tool_result_preview: false,
            show_images: true,
            cache_miss_marker: false,
            show_token_usage: false,
            show_turn_time: false,
            advisor_collapsed_lines: 0,
        }
    }
}

/// A partial patch: every field left `None` keeps its current value.
///
/// The reference accepts a `Partial<…>`; `Option` fields are that shape in
/// Rust, and `..Default::default()` reads as the empty patch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatTranscriptDisplayPreferencesPatch {
    /// See [`ChatTranscriptDisplayPreferences::hide_tool_activity`].
    pub hide_tool_activity: Option<bool>,
    /// See [`ChatTranscriptDisplayPreferences::shell_visualization`].
    pub shell_visualization: Option<bool>,
    /// See [`ChatTranscriptDisplayPreferences::read_tool_result_preview`].
    pub read_tool_result_preview: Option<bool>,
    /// See [`ChatTranscriptDisplayPreferences::show_images`].
    pub show_images: Option<bool>,
    /// See [`ChatTranscriptDisplayPreferences::cache_miss_marker`].
    pub cache_miss_marker: Option<bool>,
    /// See [`ChatTranscriptDisplayPreferences::show_token_usage`].
    pub show_token_usage: Option<bool>,
    /// See [`ChatTranscriptDisplayPreferences::show_turn_time`].
    pub show_turn_time: Option<bool>,
    /// See [`ChatTranscriptDisplayPreferences::advisor_collapsed_lines`].
    pub advisor_collapsed_lines: Option<u32>,
}

static DISPLAY_PREFERENCES: RwLock<ChatTranscriptDisplayPreferences> =
    RwLock::new(ChatTranscriptDisplayPreferences {
        hide_tool_activity: false,
        shell_visualization: true,
        read_tool_result_preview: false,
        show_images: true,
        cache_miss_marker: false,
        show_token_usage: false,
        show_turn_time: false,
        advisor_collapsed_lines: 0,
    });

/// Current transcript display preferences.
pub fn chat_transcript_display_preferences() -> ChatTranscriptDisplayPreferences {
    match DISPLAY_PREFERENCES.read() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

/// Apply host display preferences without pulling settings into the renderer.
pub fn set_chat_transcript_display_preferences(patch: ChatTranscriptDisplayPreferencesPatch) {
    let mut guard = match DISPLAY_PREFERENCES.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let mut next = *guard;
    if let Some(value) = patch.hide_tool_activity {
        next.hide_tool_activity = value;
    }
    if let Some(value) = patch.shell_visualization {
        next.shell_visualization = value;
    }
    if let Some(value) = patch.read_tool_result_preview {
        next.read_tool_result_preview = value;
    }
    if let Some(value) = patch.show_images {
        next.show_images = value;
    }
    if let Some(value) = patch.cache_miss_marker {
        next.cache_miss_marker = value;
    }
    if let Some(value) = patch.show_token_usage {
        next.show_token_usage = value;
    }
    if let Some(value) = patch.show_turn_time {
        next.show_turn_time = value;
    }
    if let Some(value) = patch.advisor_collapsed_lines {
        next.advisor_collapsed_lines = value;
    }
    *guard = next;
}

/// RAII reset guard: restores the previous preferences when dropped.
///
/// The reference has no equivalent; tests need to patch the process-wide value
/// without leaking into each other, and the crate's other global-state tests
/// use the same pattern.
#[cfg(test)]
pub(crate) struct DisplayPreferencesGuard(ChatTranscriptDisplayPreferences);

#[cfg(test)]
impl DisplayPreferencesGuard {
    pub(crate) fn set(patch: ChatTranscriptDisplayPreferencesPatch) -> Self {
        let previous = chat_transcript_display_preferences();
        set_chat_transcript_display_preferences(patch);
        DisplayPreferencesGuard(previous)
    }
}

#[cfg(test)]
impl Drop for DisplayPreferencesGuard {
    fn drop(&mut self) {
        let mut guard = match DISPLAY_PREFERENCES.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = self.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_reference() {
        let preferences = ChatTranscriptDisplayPreferences::default();
        assert!(!preferences.hide_tool_activity);
        assert!(preferences.shell_visualization);
        assert!(!preferences.read_tool_result_preview);
        assert!(preferences.show_images);
        assert!(!preferences.cache_miss_marker);
        assert!(!preferences.show_token_usage);
        assert!(!preferences.show_turn_time);
        assert_eq!(preferences.advisor_collapsed_lines, 0);
    }

    #[test]
    fn patch_touches_only_the_named_fields() {
        let _guard = DisplayPreferencesGuard::set(ChatTranscriptDisplayPreferencesPatch {
            hide_tool_activity: Some(true),
            advisor_collapsed_lines: Some(4),
            ..Default::default()
        });
        let preferences = chat_transcript_display_preferences();
        assert!(preferences.hide_tool_activity);
        assert_eq!(preferences.advisor_collapsed_lines, 4);
        // Untouched fields keep their previous value.
        assert!(preferences.shell_visualization);
        assert!(preferences.show_images);
    }

    #[test]
    fn process_wide_value_is_observable_from_every_caller() {
        let _guard = DisplayPreferencesGuard::set(ChatTranscriptDisplayPreferencesPatch {
            show_token_usage: Some(true),
            ..Default::default()
        });
        assert!(chat_transcript_display_preferences().show_token_usage);
    }
}
