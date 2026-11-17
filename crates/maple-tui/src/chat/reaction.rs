//! Agent reactions: an opening emoji on a reply becomes a badge on the
//! transcript block before it.
//!
//! Reference: `l/src/chat/reaction.ts`.
//!
//! Reactions are derived from the persisted assistant text, never stored, so a
//! rebuilt transcript reproduces them exactly. [`split_reaction`] lifts the
//! opening emoji out of the prose and reports whether a still-streaming run
//! could still become one.
//!
//! Deviations:
//! - The reference detects the opening emoji with the `v`-flag `\p{RGI_Emoji}`
//!   and `\p{Extended_Pictographic}` properties, which the `regex` crate does
//!   not expose. [`split_reaction`] takes the first extended grapheme cluster
//!   from `unicode-segmentation` and accepts it when every character is an
//!   emoji code point (pictograph, modifier, regional indicator, tag, VS16,
//!   ZWJ or keycap), which is the same set of sequences.
//! - `isReactionTarget` duck-types a `setReaction` method; Rust cannot probe a
//!   `dyn Component` for a method, so the capability is the [`ReactionTarget`]
//!   trait and the transcript builder records which blocks accept a badge.

use unicode_segmentation::UnicodeSegmentation;

/// Longest emoji grapheme (UTF-16 units) still worth withholding for.
pub const MAX_REACTION_UNITS: usize = 32;

/// A transcript block that can display an agent reaction badge.
pub trait ReactionTarget {
    /// Attach the agent's reaction emoji.
    fn set_reaction(&mut self, emoji: &str);
}

/// The result of splitting an opening reaction emoji off assistant text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReactionSplit {
    /// The reaction emoji when the text opens with one.
    pub emoji: Option<String>,
    /// Text with the opening emoji and any following whitespace removed; the
    /// input when there is none.
    pub body: String,
    /// True while the text could still grow into an emoji.
    pub pending: bool,
}

/// Split an opening reaction emoji off the front of assistant text, consuming
/// the next whitespace if any. Leading whitespace is tolerated.
pub fn split_reaction(text: &str) -> ReactionSplit {
    let head = text.trim_start();
    if head.is_empty() {
        return ReactionSplit {
            body: text.to_string(),
            pending: true,
            ..ReactionSplit::default()
        };
    }

    if let Some((emoji, rest)) = emoji_head(head) {
        if utf16_len(head) <= MAX_REACTION_UNITS && starts_with_emoji_extension(rest) {
            return ReactionSplit {
                body: text.to_string(),
                pending: true,
                ..ReactionSplit::default()
            };
        }
        let next = next_whitespace_len(rest);
        return ReactionSplit {
            emoji: Some(emoji.to_string()),
            body: rest[next..].to_string(),
            pending: false,
        };
    }

    ReactionSplit {
        pending: utf16_len(head) <= MAX_REACTION_UNITS && is_reaction_prefix(head),
        body: text.to_string(),
        ..ReactionSplit::default()
    }
}

/// The first grapheme of `head` when it is one emoji sequence, plus the rest.
fn emoji_head(head: &str) -> Option<(&str, &str)> {
    let grapheme = head.graphemes(true).next()?;
    if is_emoji_sequence(grapheme) {
        Some((grapheme, &head[grapheme.len()..]))
    } else {
        None
    }
}

/// Whether `grapheme` is a complete emoji sequence.
fn is_emoji_sequence(grapheme: &str) -> bool {
    let mut chars = grapheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !is_emoji_base(first) {
        return false;
    }
    if is_regional_indicator(first) {
        // A flag is exactly two regional indicators.
        let rest: Vec<char> = chars.collect();
        return rest.len() == 1 && is_regional_indicator(rest[0]);
    }
    chars.all(is_emoji_joiner)
}

/// Code points that may start an emoji: pictographs and regional indicators.
fn is_emoji_base(ch: char) -> bool {
    is_extended_pictographic(ch) || is_regional_indicator(ch)
}

/// Code points that continue an emoji: modifiers, tags, VS16, ZWJ, keycap.
fn is_emoji_joiner(ch: char) -> bool {
    is_emoji_modifier(ch)
        || matches!(ch as u32, 0xE0020..=0xE007F)
        || ch == '\u{FE0F}'
        || ch == '\u{200D}'
        || ch == '\u{20E3}'
        || is_extended_pictographic(ch)
        || is_regional_indicator(ch)
}

/// Whether `rest` starts with a character that can extend an emoji into a
/// longer grapheme.
fn starts_with_emoji_extension(rest: &str) -> bool {
    rest.chars().next().is_some_and(|ch| {
        is_emoji_modifier(ch)
            || matches!(ch as u32, 0xE0020..=0xE007F)
            || ch == '\u{FE0F}'
            || ch == '\u{200D}'
            || ch == '\u{20E3}'
    })
}

/// Whether `head` can only ever be an emoji grapheme plus trailing blanks.
fn is_reaction_prefix(head: &str) -> bool {
    let trimmed = head.trim_end_matches([' ', '\t']);
    !trimmed.is_empty()
        && trimmed.chars().all(|ch| {
            is_extended_pictographic(ch)
                || is_emoji_modifier(ch)
                || is_regional_indicator(ch)
                || matches!(ch as u32, 0xE0020..=0xE007F)
                || ch == '\u{FE0F}'
                || ch == '\u{200D}'
                || ch == '\u{20E3}'
        })
}

/// Length of the whitespace run the reference consumes after the emoji: an
/// inline newline (optionally preceded by spaces/tabs) or a run of spaces/tabs.
fn next_whitespace_len(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let mut index = 0;
    while index < bytes.len() && (bytes[index] == b' ' || bytes[index] == b'\t') {
        index += 1;
    }
    if bytes.get(index) == Some(&b'\n') {
        return index + 1;
    }
    if bytes.get(index) == Some(&b'\r') && bytes.get(index + 1) == Some(&b'\n') {
        return index + 2;
    }
    index
}

/// Length in UTF-16 code units, the unit the reference counts.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Unicode `Extended_Pictographic` membership.
///
/// The ranges are the property's core blocks plus the newer emoji planes.
fn is_extended_pictographic(ch: char) -> bool {
    matches!(ch as u32,
        0x00A9 | 0x00AE | 0x203C | 0x2049 | 0x2122 | 0x2139
        | 0x2194..=0x2199 | 0x21A9..=0x21AA | 0x231A..=0x231B
        | 0x2328 | 0x2388 | 0x23CF | 0x23E9..=0x23F3 | 0x23F8..=0x23FA
        | 0x24C2 | 0x25AA..=0x25AB | 0x25B6 | 0x25C0 | 0x25FB..=0x25FE
        | 0x2600..=0x2605 | 0x2607..=0x2612 | 0x2614..=0x2685
        | 0x2690..=0x2705 | 0x2708..=0x2712 | 0x2714 | 0x2716 | 0x271D
        | 0x2721 | 0x2728 | 0x2733..=0x2734 | 0x2744 | 0x2747
        | 0x274C | 0x274E | 0x2753..=0x2755 | 0x2757 | 0x2763..=0x2767
        | 0x2795..=0x2797 | 0x27A1 | 0x27B0 | 0x27BF | 0x2934..=0x2935
        | 0x2B05..=0x2B07 | 0x2B1B..=0x2B1C | 0x2B50 | 0x2B55
        | 0x3030 | 0x303D | 0x3297 | 0x3299
        | 0x1F000..=0x1F0FF | 0x1F10D..=0x1F10F | 0x1F12F
        | 0x1F16C..=0x1F171 | 0x1F17E..=0x1F17F | 0x1F18E | 0x1F191..=0x1F19A
        | 0x1F1AD..=0x1F1E5 | 0x1F201..=0x1F20F | 0x1F21A | 0x1F22F
        | 0x1F232..=0x1F23A | 0x1F23C..=0x1F23F | 0x1F249..=0x1F3FA
        | 0x1F400..=0x1F53D | 0x1F546..=0x1F64F | 0x1F680..=0x1F6FF
        | 0x1F774..=0x1F77F | 0x1F7D5..=0x1F7FF | 0x1F80C..=0x1F80F
        | 0x1F848..=0x1F84F | 0x1F85A..=0x1F85F | 0x1F888..=0x1F88F
        | 0x1F8AE..=0x1F8FF | 0x1F90C..=0x1F93A | 0x1F93C..=0x1F945
        | 0x1F947..=0x1FAFF | 0x1FC00..=0x1FFFD
    )
}

/// Unicode `Emoji_Modifier` membership (skin tones).
fn is_emoji_modifier(ch: char) -> bool {
    matches!(ch as u32, 0x1F3FB..=0x1F3FF)
}

/// Unicode `Regional_Indicator` membership (flag letters).
fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1F1E6..=0x1F1FF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_never_a_reaction() {
        let split = split_reaction("Here is the summary.\n");
        assert_eq!(split.emoji, None);
        assert_eq!(split.body, "Here is the summary.\n");
        assert!(!split.pending);
    }

    #[test]
    fn opening_emoji_is_lifted_with_the_following_space() {
        let cases: [(&str, &str, &str); 5] = [
            ("✅ done", "✅", "done"),
            ("  🎉\n\nshipped", "🎉", "shipped"),
            ("👍🏽 nice", "👍🏽", "nice"),
            ("🇫🇷 oui", "🇫🇷", "oui"),
            ("👨‍👩‍👧 family", "👨‍👩‍👧", "family"),
        ];
        for (text, emoji, body) in cases {
            let split = split_reaction(text);
            assert_eq!(split.emoji.as_deref(), Some(emoji), "{text:?}");
            assert_eq!(split.body, body, "{text:?}");
            assert!(!split.pending, "{text:?}");
        }
    }

    #[test]
    fn emoji_without_trailing_space_keeps_an_empty_body() {
        let split = split_reaction("✅");
        assert_eq!(split.emoji.as_deref(), Some("✅"));
        assert_eq!(split.body, "");
        assert!(!split.pending);
    }

    #[test]
    fn streaming_prefixes_are_withheld() {
        let cases: [&str; 3] = ["", "  ", "👨‍"];
        for text in cases {
            let split = split_reaction(text);
            assert_eq!(split.emoji, None, "{text:?}");
            assert!(split.pending, "{text:?}");
            assert_eq!(split.body, text, "{text:?}");
        }
    }

    #[test]
    fn an_emoji_head_followed_by_a_modifier_stays_pending() {
        let split = split_reaction("👍🏽");
        assert_eq!(split.emoji, None);
        assert!(split.pending);
    }

    #[test]
    fn long_non_emoji_heads_stop_pending() {
        let long = "a".repeat(MAX_REACTION_UNITS + 1);
        assert!(!split_reaction(&long).pending);
        assert!(!split_reaction("text goes on").pending);
    }

    #[test]
    fn regional_indicator_pairs_and_singles() {
        assert!(is_emoji_sequence("🇫🇷"));
        assert!(!is_emoji_sequence("🇫🇷🇩"));
        assert!(is_extended_pictographic('✅'));
        assert!(!is_extended_pictographic('a'));
        assert!(is_emoji_modifier('🏽'));
        assert_eq!(utf16_len("👨‍👩‍👧"), 8);
    }
}
