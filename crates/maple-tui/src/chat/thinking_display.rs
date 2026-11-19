//! Thinking text prepared for display.
//!
//! Reference: `l/src/chat/thinking-display.ts`.
//!
//! Both modes drop empty `<!-- -->` sentinel lines outside code fences
//! (gpt-5.x reasoning summaries pad their parts with them); prose-only mode
//! additionally elides fenced code down to a trailing ellipsis.
//!
//! A small per-mode MRU memo collapses the two or three formats of the same
//! growing text within one streaming tick, and resumes an incremental fold when
//! the new text is a verbatim extension of the cached one (see the reference's
//! cache commentary). The memo is process-wide like the reference's module
//! globals.
//!
//! Deviations: the slot vectors live behind a `Mutex`, `AgentMessage` comes
//! from [`crate::chat::transcript_entry`], and `message.content` is the typed
//! [`crate::chat::messages::AssistantContent`] list.

use std::sync::{LazyLock, Mutex};

use crate::chat::messages::AssistantContent;
use crate::chat::transcript_entry::AgentMessage;
use crate::render::render_utils::FENCE_RE;

/// Fold accumulator for the output produced so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FoldState {
    /// Joined output lines before the last non-blank line, fully finalized.
    committed: String,
    /// Whether a non-blank output line exists (kept in [`Self::tail_line`]).
    has_tail: bool,
    /// The last non-blank output line, rewritten in place by the prose
    /// ellipsis.
    tail_line: String,
    /// Raw blank output lines after `tail_line`, each prefixed by its
    /// separator.
    tail_blank_sep: String,
    /// Whether any output line precedes `tail_line`.
    tail_pred: bool,
    /// Output lines emitted so far.
    emitted: usize,
    /// Inside a fenced code block.
    in_fence: bool,
    /// Fence character.
    fence_char: String,
    /// Fence marker length.
    fence_len: usize,
}

/// One memo slot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DisplayCache {
    /// Last formatted text.
    text: String,
    /// Formatted output for `text`.
    value: String,
    /// Whether `text` contains `<!--`.
    had_comment: bool,
    /// Byte offset of the start of the last (possibly partial) line.
    start_line_byte: usize,
    /// Fold state entering that last line.
    state: FoldState,
    /// Whether the checkpoint is valid to resume from.
    resumable: bool,
}

/// Slots per mode.
const DISPLAY_CACHE_SLOTS: usize = 4;

/// Resume cap for a trailing partial line.
const MAX_RESUME_PARTIAL_BYTES: usize = 8192;

#[derive(Default)]
struct Caches {
    prose: Vec<DisplayCache>,
    raw: Vec<DisplayCache>,
}

static CACHES: LazyLock<Mutex<Caches>> = LazyLock::new(|| Mutex::new(Caches::default()));

fn fresh_display_cache() -> DisplayCache {
    DisplayCache {
        resumable: true,
        ..DisplayCache::default()
    }
}

/// Drop every memo slot so the next call recomputes from scratch (tests only).
pub fn reset_thinking_display_cache_for_tests() {
    let mut caches = CACHES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    caches.prose.clear();
    caches.raw.clear();
}

/// The match kind of an acquired slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheMatch {
    Hit,
    Append,
    Miss,
}

/// Whether `text` is an append of the cached text.
fn is_append(cache: &DisplayCache, text: &str) -> bool {
    !cache.text.is_empty() && text.starts_with(&cache.text)
}

/// Pick the slot to format `text` into and move it to the front; returns the
/// slot index and the match kind.
fn acquire_display_cache(slots: &mut Vec<DisplayCache>, text: &str) -> (usize, CacheMatch) {
    let mut index = slots.iter().position(|slot| slot.text == text);
    let mut kind = if index.is_some() {
        CacheMatch::Hit
    } else {
        CacheMatch::Miss
    };
    if index.is_none() {
        let mut append_length = 0;
        let mut retired: Option<usize> = None;
        let mut retired_length = 0;
        for (i, slot) in slots.iter().enumerate() {
            let length = slot.text.len();
            let threshold = if slot.resumable {
                append_length
            } else {
                retired_length
            };
            if length <= threshold || !is_append(slot, text) {
                continue;
            }
            if slot.resumable {
                index = Some(i);
                kind = CacheMatch::Append;
                append_length = length;
            } else {
                retired = Some(i);
                retired_length = length;
            }
        }
        if index.is_none() {
            index = retired;
        }
    }
    let mut index = match index {
        Some(index) => index,
        None => {
            if slots.len() < DISPLAY_CACHE_SLOTS {
                slots.insert(0, fresh_display_cache());
                return (0, kind);
            }
            slots.len() - 1
        }
    };
    if index > 0 {
        let cache = slots.remove(index);
        slots.insert(0, cache);
        index = 0;
    }
    (index, kind)
}

/// Canonical message text: `None`/whitespace/placeholder-only text is empty.
pub fn canonicalize_message(text: Option<&str>) -> String {
    let Some(text) = text else {
        return String::new();
    };
    if text.is_empty() {
        return String::new();
    }
    let trimmed = text.trim();
    let placeholder_only = trimmed.chars().all(|ch| {
        matches!(
            ch,
            '.' | '\u{2026}' | ' ' | '\t' | '\n' | '\r'
        )
    });
    if placeholder_only {
        String::new()
    } else {
        trimmed.to_string()
    }
}

/// Whether `line` is reasoning-summary comment noise.
fn is_comment_noise(line: &str, is_last_line: bool) -> bool {
    let trimmed = line.trim();
    is_empty_comment(trimmed) || (is_last_line && is_open_comment(trimmed))
}

/// `/^<!--\s*-->$/`.
fn is_empty_comment(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("<!--") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix("-->") else {
        return false;
    };
    rest.chars().all(char::is_whitespace)
}

/// `/^<!--\s*$/`.
fn is_open_comment(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("<!--")
        .is_some_and(|rest| rest.chars().all(char::is_whitespace))
}

/// Fold one input line into the accumulator.
fn push_fold_line(state: &mut FoldState, line: &str) {
    state.emitted += 1;
    if line.trim().is_empty() {
        if state.has_tail {
            state.tail_blank_sep.push('\n');
            state.tail_blank_sep.push_str(line);
        } else {
            if state.emitted > 1 {
                state.committed.push('\n');
            }
            state.committed.push_str(line);
        }
        return;
    }
    if state.has_tail {
        if state.tail_pred {
            state.committed.push('\n');
        }
        state.committed.push_str(&state.tail_line);
        state.committed.push_str(&state.tail_blank_sep);
        state.tail_pred = true;
    } else {
        state.tail_pred = state.emitted > 1;
    }
    state.tail_line = line.to_string();
    state.has_tail = true;
    state.tail_blank_sep.clear();
}

/// Prose-mode ellipsis: rewrite the last non-blank output line in place.
fn append_fold_ellipsis(state: &mut FoldState) {
    if !state.has_tail {
        push_fold_line(state, "...");
        return;
    }
    let trimmed = state.tail_line.trim_end();
    if trimmed.ends_with("...") {
        state.tail_line = trimmed.to_string();
    } else if let Some(head) = trimmed.strip_suffix('.') {
        state.tail_line = format!("{head}...");
    } else {
        state.tail_line = format!("{trimmed}...");
    }
}

/// Materialize the folded output from the committed prefix and the tail region.
fn render_fold(state: &FoldState) -> String {
    if !state.has_tail {
        return state.committed.clone();
    }
    let mut out = state.committed.clone();
    if state.tail_pred {
        out.push('\n');
    }
    out.push_str(&state.tail_line);
    out.push_str(&state.tail_blank_sep);
    out
}

/// Thinking text prepared for display.
pub fn format_thinking_for_display(text: &str, prose_only: bool) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let mut caches = CACHES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let slots = if prose_only {
        &mut caches.prose
    } else {
        &mut caches.raw
    };
    let (index, kind) = acquire_display_cache(slots, text);
    if kind == CacheMatch::Hit {
        return slots[index].value.clone();
    }

    let has_comment;
    let from_byte;
    let mut state;
    if kind == CacheMatch::Append {
        let previous = &slots[index];
        let seam = previous.text.len().saturating_sub(3);
        has_comment = previous.had_comment
            || text
                .get(seam..)
                .is_some_and(|suffix| suffix.contains("<!--"));
        from_byte = seam;
        state = previous.state.clone();
    } else {
        has_comment = text.contains("<!--");
        from_byte = 0;
        state = FoldState::default();
    }

    // Raw mode without comments is the identity.
    if !prose_only && !has_comment {
        let cache = &mut slots[index];
        cache.text = text.to_string();
        cache.value = text.to_string();
        cache.had_comment = false;
        cache.start_line_byte = 0;
        cache.state = FoldState::default();
        cache.resumable = false;
        return text.to_string();
    }

    let sliced = text.get(from_byte..).unwrap_or_default();
    let lines: Vec<&str> = sliced.split('\n').collect();
    let last = lines.len().saturating_sub(1);
    for (i, line) in lines.iter().enumerate() {
        if i == last {
            slots[index].state = state.clone();
        }
        if state.in_fence {
            if let Some(caps) = FENCE_RE.captures(line) {
                let marker = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
                let indent_len = caps.get(1).map(|m| m.len()).unwrap_or(0);
                let rest = line.get(indent_len + marker.len()..).unwrap_or_default().trim();
                if marker.starts_with(&state.fence_char)
                    && marker.len() >= state.fence_len
                    && rest.is_empty()
                {
                    state.in_fence = false;
                    state.fence_char.clear();
                    state.fence_len = 0;
                }
            }
            if !prose_only {
                push_fold_line(&mut state, line);
            }
            continue;
        }

        if has_comment && is_comment_noise(line, i == last) {
            continue;
        }

        if let Some(caps) = FENCE_RE.captures(line) {
            let marker = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
            let indent_len = caps.get(1).map(|m| m.len()).unwrap_or(0);
            let ch = marker.chars().next().unwrap_or('`');
            let info = line.get(indent_len + marker.len()..).unwrap_or_default();
            if !(ch == '`' && info.contains('`')) {
                state.in_fence = true;
                state.fence_char = ch.to_string();
                state.fence_len = marker.len();
                if prose_only {
                    append_fold_ellipsis(&mut state);
                } else {
                    push_fold_line(&mut state, line);
                }
                continue;
            }
        }
        push_fold_line(&mut state, line);
    }

    let formatted = render_fold(&state);
    let start_line_byte = text.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let cache = &mut slots[index];
    cache.text = text.to_string();
    cache.value = formatted.clone();
    cache.had_comment = has_comment;
    cache.start_line_byte = start_line_byte;
    cache.resumable = text.len() - start_line_byte <= MAX_RESUME_PARTIAL_BYTES;
    formatted
}

/// Whether a formatted thinking block has non-placeholder content worth
/// rendering.
pub fn has_displayable_thinking(text: Option<&str>, formatted_text: Option<&str>) -> bool {
    let (Some(text), Some(formatted_text)) = (text, formatted_text) else {
        return false;
    };
    if text.is_empty() || formatted_text.is_empty() {
        return false;
    }
    !formatted_text.trim().is_empty() && !canonicalize_message(Some(text)).is_empty()
}

/// Whether an assistant message contains thinking content the TUI can reveal.
pub fn message_has_displayable_thinking(message: &AgentMessage, prose_only: bool) -> bool {
    let AgentMessage::Assistant(assistant) = message else {
        return false;
    };
    assistant.content.iter().any(|content| match content {
        AssistantContent::Thinking(thinking) => has_displayable_thinking(
            Some(&thinking.thinking),
            Some(&format_thinking_for_display(&thinking.thinking, prose_only)),
        ),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::messages::{AssistantMessage, ThinkingContent};

    fn reset() {
        reset_thinking_display_cache_for_tests();
    }

    #[test]
    fn plain_prose_passes_through_unchanged() {
        reset();
        let cases = [
            ("hello", true, "hello"),
            ("a\nb", true, "a\nb"),
            ("a\n\nb", true, "a\n\nb"),
            ("", true, ""),
        ];
        for (text, prose, expected) in cases {
            assert_eq!(format_thinking_for_display(text, prose), expected);
        }
    }

    #[test]
    fn comment_noise_is_dropped_outside_fences_only() {
        reset();
        assert_eq!(
            format_thinking_for_display("**Headline**\n\n<!-- -->", true),
            "**Headline**"
        );
        assert_eq!(format_thinking_for_display("a\n<!-- -->", false), "a");
        // A real comment with content stays.
        assert_eq!(
            format_thinking_for_display("a\n<!-- note -->", true),
            "a\n<!-- note -->"
        );
        // Inside a fence it is code.
        assert_eq!(
            format_thinking_for_display("```\n<!-- -->\n```", true),
            "..."
        );
        // A still-unterminated prefix on the streaming last line is noise.
        assert_eq!(format_thinking_for_display("a\n<!--", true), "a");
    }

    #[test]
    fn prose_mode_elides_fences_to_an_ellipsis() {
        reset();
        assert_eq!(
            format_thinking_for_display("before\n```rust\nlet x = 1;\n```\nafter", true),
            "before...\nafter"
        );
        assert_eq!(format_thinking_for_display("```\nonly code\n```", true), "...");
        // Raw mode keeps fence lines verbatim.
        assert_eq!(
            format_thinking_for_display("before\n```\ncode\n```", false),
            "before\n```\ncode\n```"
        );
    }

    #[test]
    fn ellipsis_replaces_an_existing_trailing_dot_run() {
        reset();
        assert_eq!(format_thinking_for_display("a.\n```\ncode\n```", true), "a...");
        assert_eq!(format_thinking_for_display("a...\n```\ncode\n```", true), "a...");
        assert_eq!(format_thinking_for_display("```\nx\n```", true), "...");
    }

    #[test]
    fn appending_resumes_the_fold() {
        reset();
        let first = format_thinking_for_display("alpha\nbeta", true);
        assert_eq!(first, "alpha\nbeta");
        let grown = format_thinking_for_display("alpha\nbeta\ngamma", true);
        assert_eq!(grown, "alpha\nbeta\ngamma");
        let more = format_thinking_for_display("alpha\nbeta\ngamma\ndelta", true);
        assert_eq!(more, "alpha\nbeta\ngamma\ndelta");
        reset();
    }

    #[test]
    fn identical_text_hits_the_memo() {
        reset();
        let once = format_thinking_for_display("same", true);
        let twice = format_thinking_for_display("same", true);
        assert_eq!(once, twice);
        reset();
    }

    #[test]
    fn canonical_messages_reject_placeholders() {
        let cases = [
            (None, ""),
            (Some(""), ""),
            (Some("   "), ""),
            (Some("..."), ""),
            (Some("\u{2026}\n\t"), ""),
            (Some("  hello  "), "hello"),
        ];
        for (text, expected) in cases {
            assert_eq!(canonicalize_message(text), expected);
        }
    }

    #[test]
    fn displayability_needs_text_on_both_sides() {
        assert!(has_displayable_thinking(Some("thought"), Some("thought")));
        assert!(!has_displayable_thinking(None, Some("x")));
        assert!(!has_displayable_thinking(Some("x"), None));
        assert!(!has_displayable_thinking(Some(""), Some("x")));
        assert!(!has_displayable_thinking(Some("..."), Some("...")));
        assert!(!has_displayable_thinking(Some("<!-- -->"), Some("  ")));
    }

    #[test]
    fn assistant_messages_are_probed_for_thinking() {
        reset();
        let message = AgentMessage::Assistant(AssistantMessage {
            content: vec![
                AssistantContent::Thinking(ThinkingContent {
                    thinking: "reasoning".into(),
                    ..ThinkingContent::default()
                }),
                AssistantContent::Text(crate::chat::messages::TextContent {
                    text: "answer".into(),
                    ..Default::default()
                }),
            ],
            ..AssistantMessage::default()
        });
        assert!(message_has_displayable_thinking(&message, true));

        let empty = AgentMessage::Assistant(AssistantMessage {
            content: vec![AssistantContent::Thinking(ThinkingContent {
                thinking: "...".into(),
                ..ThinkingContent::default()
            })],
            ..AssistantMessage::default()
        });
        assert!(!message_has_displayable_thinking(&empty, true));

        let user = AgentMessage::User(crate::chat::transcript_entry::UserMessage::default());
        assert!(!message_has_displayable_thinking(&user, true));
        reset();
    }
}
