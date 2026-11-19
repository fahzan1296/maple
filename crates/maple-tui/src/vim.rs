//! Modal editing state machine (port of `l/src/vim.ts`).
//!
//! Reference: `l/src/vim.ts`.
//!
//! Deliberately a *pure* state machine: it reads a snapshot of the buffer and returns the edits it
//! wants applied, so motions can be unit-tested without a terminal and the editor keeps sole
//! ownership of undo, atomic placeholder tokens, the kill ring, and `onChange`.
//!
//! This is a usable subset of Vim, not a reimplementation of it: Normal and Visual modes, the
//! common motions, and operators built from those motions.
//!
//! Deviations forced by Rust:
//!
//! - Columns are Unicode scalar (char) indices, not JavaScript UTF-16 code units. `line.length`
//!   becomes `line.chars().count()` and `text.charAt(i)` becomes `text.chars().nth(i)`. This
//!   matches the Rust editor's column convention; astral text therefore occupies one column here
//!   where the reference counts two.
//! - `Intl.Segmenter` becomes `unicode_segmentation::UnicodeSegmentation::graphemes`.
//! - `getSegmenter`, `moveWordLeft`, `moveWordRight`, `getWordNavKind`, and `isWordNavJoiner` come
//!   from `l/src/utils.ts`, which has no crate module; they are reimplemented here. Word classes
//!   use `regex` Unicode properties (`\p{White_Space}`, `\p{P}`, `\p{S}`, `\p{L}`, `\p{N}`, and
//!   the Han/Hiragana/Katakana/Hangul scripts), the same classes the reference tests.
//! - `#desiredCol`'s `Infinity` ("sticky end-of-line") is the [`DesiredCol::End`] variant.
//! - Counts are parsed like `Number.parseInt` (as `f64`, then clamped to at least 1).

use regex::Regex;
use std::sync::LazyLock;
use unicode_segmentation::UnicodeSegmentation;

/// Editing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VimMode {
    Insert,
    Normal,
    Visual,
    VisualLine,
    Replace,
}

impl VimMode {
    /// The selector string used by the reference.
    pub fn as_str(self) -> &'static str {
        match self {
            VimMode::Insert => "insert",
            VimMode::Normal => "normal",
            VimMode::Visual => "visual",
            VimMode::VisualLine => "visual-line",
            VimMode::Replace => "replace",
        }
    }
}

/// Operator awaiting a motion or text object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VimOperator {
    Delete,
    Yank,
    Change,
    Indent,
    Outdent,
}

struct NavRegexes {
    whitespace: Regex,
    punct: Regex,
    symbol: Regex,
    letter: Regex,
    number: Regex,
    han: Regex,
    hiragana: Regex,
    katakana: Regex,
    hangul: Regex,
}

static NAV_REGEXES: LazyLock<NavRegexes> = LazyLock::new(|| NavRegexes {
    whitespace: Regex::new(r"^\p{White_Space}$").unwrap(),
    punct: Regex::new(r"^\p{P}$").unwrap(),
    symbol: Regex::new(r"^\p{S}$").unwrap(),
    letter: Regex::new(r"^\p{L}$").unwrap(),
    number: Regex::new(r"^\p{N}$").unwrap(),
    han: Regex::new(r"^\p{Script=Han}$").unwrap(),
    hiragana: Regex::new(r"^\p{Script=Hiragana}$").unwrap(),
    katakana: Regex::new(r"^\p{Script=Katakana}$").unwrap(),
    hangul: Regex::new(r"^\p{Script=Hangul}$").unwrap(),
});

fn nav_regexes() -> &'static NavRegexes {
    &NAV_REGEXES
}
impl VimOperator {
    /// The selector string used by the reference.
    pub fn as_str(self) -> &'static str {
        match self {
            VimOperator::Delete => "d",
            VimOperator::Yank => "y",
            VimOperator::Change => "c",
            VimOperator::Indent => ">",
            VimOperator::Outdent => "<",
        }
    }
}

/// A buffer position; `col` counts Unicode scalars.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VimPosition {
    /// Zero-based line index.
    pub line: usize,
    /// Zero-based column (char index).
    pub col: usize,
}

/// Read-only view of the editor buffer that motions resolve against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VimBuffer {
    /// Buffer lines.
    pub lines: Vec<String>,
    /// Cursor line.
    pub cursor_line: usize,
    /// Cursor column (char index).
    pub cursor_col: usize,
}

impl VimBuffer {
    /// A snapshot of `lines` with the cursor at `cursor_line`/`cursor_col`.
    pub fn new(lines: Vec<String>, cursor_line: usize, cursor_col: usize) -> Self {
        VimBuffer {
            lines,
            cursor_line,
            cursor_col,
        }
    }
}

/// An edit the editor should apply. `to` is always an *exclusive* end position, so ranges compose
/// the same way regardless of whether the originating motion was inclusive (`e`) or exclusive
/// (`w`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VimCommand {
    /// Move the cursor.
    Move {
        /// Destination.
        to: VimPosition,
    },
    /// Switch mode.
    Mode {
        /// Target mode.
        mode: VimMode,
    },
    /// Yank a range into the kill ring.
    Yank {
        /// Inclusive start.
        from: VimPosition,
        /// Exclusive end.
        to: VimPosition,
        /// Whole lines.
        linewise: bool,
    },
    /// Delete a range.
    Delete {
        /// Inclusive start.
        from: VimPosition,
        /// Exclusive end.
        to: VimPosition,
        /// Whole lines.
        linewise: bool,
        /// Delete as part of a change (enters Insert).
        insert: bool,
    },
    /// Open a new line below/above and enter Insert.
    OpenLine {
        /// `o` (below) when true, `O` (above) when false.
        below: bool,
    },
    /// Paste the kill ring.
    Paste {
        /// `p` (after) when true, `P` (before) when false.
        after: bool,
        /// Repeat count.
        count: usize,
    },
    /// Undo.
    Undo,
    /// Replace a range with text.
    Replace {
        /// Inclusive start.
        from: VimPosition,
        /// Exclusive end.
        to: VimPosition,
        /// Replacement text.
        text: String,
    },
    /// Join lines.
    Join {
        /// First line to join.
        from_line: usize,
        /// Number of lines to join.
        lines: usize,
    },
    /// Indent or outdent lines.
    Indent {
        /// First line.
        from_line: usize,
        /// Last line.
        to_line: usize,
        /// Outdent when true.
        out: bool,
    },
    /// Replay an Insert/Replace session.
    Retype {
        /// What the session typed.
        record: VimInsertRecord,
        /// Repeat count.
        times: usize,
        /// The session opened a line (`o`/`O`).
        newline: bool,
        /// The session was Replace mode.
        overwrite: bool,
    },
}

/// What an Insert/Replace session typed, relative to where it started; `.` replays it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VimInsertRecord {
    /// Graphemes removed before the session's start cursor (Backspace past the insert point).
    pub delete_before: usize,
    /// Graphemes removed after it (forward delete).
    pub delete_after: usize,
    /// Inserted text.
    pub text: String,
}

/// Coarse character classification for word navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordNavKind {
    Whitespace,
    Delimiter,
    Cjk,
    Word,
    Other,
}


/// Coarse Unicode-aware character classification for word navigation (Option/Alt + Left/Right).
/// This intentionally avoids language-specific word segmentation for predictability across
/// scripts.
pub fn word_nav_kind(grapheme: &str) -> WordNavKind {
    let ch = match grapheme.chars().next() {
        Some(ch) => ch,
        None => return WordNavKind::Other,
    };
    let mut buf = [0u8; 4];
    let ch_str = ch.encode_utf8(&mut buf);
    let regexes = nav_regexes();
    if regexes.whitespace.is_match(ch_str) {
        return WordNavKind::Whitespace;
    }
    if ch == '_' {
        return WordNavKind::Word;
    }
    if regexes.punct.is_match(ch_str) || regexes.symbol.is_match(ch_str) {
        return WordNavKind::Delimiter;
    }
    if regexes.han.is_match(ch_str)
        || regexes.hiragana.is_match(ch_str)
        || regexes.katakana.is_match(ch_str)
        || regexes.hangul.is_match(ch_str)
    {
        return WordNavKind::Cjk;
    }
    if regexes.letter.is_match(ch_str) || regexes.number.is_match(ch_str) {
        return WordNavKind::Word;
    }
    WordNavKind::Other
}

/// Joiners kept inside a word run by word navigation.
pub fn is_word_nav_joiner(grapheme: &str) -> bool {
    matches!(
        grapheme.chars().next(),
        Some('\'') | Some('\u{2019}') | Some('-') | Some('\u{2010}') | Some('\u{2011}')
    )
}

/// Number of Unicode scalars in `text`.
fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// The Unicode scalar at char index `index`.
fn char_at(text: &str, index: usize) -> Option<char> {
    text.chars().nth(index)
}

/// `text` restricted to char indices `from..to`.
fn char_slice(text: &str, from: usize, to: usize) -> String {
    text.chars()
        .skip(from)
        .take(to.saturating_sub(from))
        .collect()
}

/// Grapheme spans with their char-index start offsets.
fn grapheme_spans(text: &str) -> Vec<(usize, &str)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for grapheme in text.graphemes(true) {
        spans.push((start, grapheme));
        start += grapheme.chars().count();
    }
    spans
}

/// Grapheme spans starting at or after char index `from`.
fn grapheme_spans_from(text: &str, from: usize) -> Vec<(usize, &str)> {
    grapheme_spans(text)
        .into_iter()
        .filter(|(start, _)| *start >= from)
        .collect()
}

/// Inclusive start offsets plus the char length, one entry per grapheme boundary.
fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut bounds = vec![0];
    for (_, grapheme) in grapheme_spans(text) {
        let end = bounds[bounds.len() - 1] + grapheme.chars().count();
        if end != bounds[bounds.len() - 1] {
            bounds.push(end);
        }
    }
    let len = char_count(text);
    if bounds[bounds.len() - 1] != len {
        bounds.push(len);
    }
    bounds
}

/// Start offset of the grapheme after `col`, clamped to the char length.
pub fn next_grapheme_start(text: &str, col: usize) -> usize {
    let len = char_count(text);
    if col >= len {
        return len;
    }
    for (start, grapheme) in grapheme_spans(text) {
        if start >= col {
            return (start + grapheme.chars().count()).min(len);
        }
    }
    len
}

/// Start offset of the grapheme before `col`, clamped to 0.
pub fn prev_grapheme_start(text: &str, col: usize) -> usize {
    if col <= 0 {
        return 0;
    }
    let mut last = 0;
    for (start, _) in grapheme_spans(text) {
        if start >= col {
            break;
        }
        last = start;
    }
    last
}

/// Offset of the final grapheme — where a Normal-mode cursor rests on a non-empty line.
pub fn last_grapheme_start(text: &str) -> usize {
    prev_grapheme_start(text, char_count(text))
}

/// Number of graphemes in `text`.
pub fn count_graphemes(text: &str) -> usize {
    grapheme_spans(text).len()
}

/// Offset `count` graphemes forward or backward from `offset`, clamped to the text.
pub fn step_graphemes(text: &str, offset: usize, count: usize, forward: bool) -> usize {
    if count <= 0 {
        return offset;
    }
    let mut boundaries: Vec<usize> = grapheme_spans(text).into_iter().map(|(s, _)| s).collect();
    boundaries.push(char_count(text));
    let mut i = 0;
    while i + 1 < boundaries.len() && boundaries[i + 1] <= offset {
        i += 1;
    }
    let last = boundaries.len() - 1;
    if forward {
        boundaries[(i + count).min(last)]
    } else {
        boundaries[((i + usize::from(boundaries[i] < offset)).saturating_sub(count)).min(last)]
    }
}

/// What a session turned `before` into, anchored at its start `cursor`: the prefix match stops at
/// the cursor and the suffix match never crosses it, so typing `aa` before an existing `a` records
/// `aa` at the cursor rather than after the old one. Comparisons are whole graphemes, so a shared
/// high surrogate cannot split an emoji into the replayed text.
pub fn diff_insert_session(before: &str, cursor: usize, after: &str) -> VimInsertRecord {
    let before_bounds = grapheme_boundaries(before);
    let after_bounds = grapheme_boundaries(after);
    let mut cursor_bound = 0;
    while cursor_bound + 1 < before_bounds.len() && before_bounds[cursor_bound + 1] <= cursor {
        cursor_bound += 1;
    }

    let mut prefix = 0;
    while prefix < cursor_bound
        && prefix + 1 < after_bounds.len()
        && char_slice(before, before_bounds[prefix], before_bounds[prefix + 1])
            == char_slice(after, after_bounds[prefix], after_bounds[prefix + 1])
    {
        prefix += 1;
    }

    let before_end = before_bounds.len() - 1;
    let after_end = after_bounds.len() - 1;
    let mut suffix = 0;
    while before_end - suffix > cursor_bound
        && after_end - suffix > prefix
        && char_slice(
            before,
            before_bounds[before_end - suffix - 1],
            before_bounds[before_end - suffix],
        ) == char_slice(
            after,
            after_bounds[after_end - suffix - 1],
            after_bounds[after_end - suffix],
        )
    {
        suffix += 1;
    }

    VimInsertRecord {
        delete_before: cursor_bound - prefix,
        delete_after: before_end - suffix - cursor_bound,
        text: char_slice(after, after_bounds[prefix], after_bounds[after_end - suffix]),
    }
}

fn first_non_blank(text: &str) -> usize {
    for (start, grapheme) in grapheme_spans(text) {
        if !grapheme.trim().is_empty() {
            return start;
        }
    }
    0
}

fn find_pending_key(pending: Option<FindPending>) -> &'static str {
    let Some(pending) = pending else {
        return "";
    };
    if pending.till {
        if pending.forward {
            "t"
        } else {
            "T"
        }
    } else if pending.forward {
        "f"
    } else {
        "F"
    }
}

/// Column of the `count`th grapheme equal to `target`, searching strictly after `from_col` when
/// `forward` and strictly before it otherwise. Line-local, like Vim's `f`/`F`.
fn find_grapheme(
    text: &str,
    from_col: usize,
    target: &str,
    forward: bool,
    count: usize,
) -> Option<usize> {
    if count < 1 || target.is_empty() {
        return None;
    }
    let spans = grapheme_spans(text);
    let mut here = usize::MAX;
    for (i, (start, _)) in spans.iter().enumerate() {
        if *start <= from_col {
            here = i;
        } else {
            break;
        }
    }
    let mut seen = 0;
    if forward {
        let start = if here == usize::MAX { 0 } else { here + 1 };
        for grapheme in spans.iter().skip(start) {
            if grapheme.1 == target {
                seen += 1;
                if seen == count {
                    return Some(grapheme.0);
                }
            }
        }
    } else {
        if here == usize::MAX {
            return None;
        }
        for (start, grapheme) in spans.iter().take(here).rev() {
            if *grapheme == target {
                seen += 1;
                if seen == count {
                    return Some(*start);
                }
            }
        }
    }
    None
}

/// Vim's `w`: `move_word_right` stops at the end of the current word, so skip the gap that follows.
fn word_forward(text: &str, col: usize) -> usize {
    let mut next = move_word_right(text, col);
    let len = char_count(text);
    while next < len && char_at(text, next).is_some_and(|ch| ch.is_whitespace()) {
        next += 1;
    }
    next
}

/// Vim's `e`: the last grapheme of the word the cursor is about to run into.
fn word_end(text: &str, col: usize) -> usize {
    let mut from = next_grapheme_start(text, col);
    let len = char_count(text);
    while from < len && char_at(text, from).is_some_and(|ch| ch.is_whitespace()) {
        from += 1;
    }
    let end = move_word_right(text, from);
    col.max(prev_grapheme_start(text, end))
}

/// Move the cursor one "word" to the left using Unicode-aware coarse navigation.
pub fn move_word_left(text: &str, cursor: usize) -> usize {
    let len = char_count(text);
    if len == 0 {
        return 0;
    }
    let mut i = cursor.clamp(0, len);
    if i == 0 {
        return 0;
    }
    let mut graphemes: Vec<(usize, &str)> = grapheme_spans(text)
        .into_iter()
        .take_while(|&(start, grapheme)| start + grapheme.chars().count() <= i)
        .collect();
    if graphemes.is_empty() {
        return 0;
    }

    while let Some((_, grapheme)) = graphemes.last() {
        if word_nav_kind(grapheme) == WordNavKind::Whitespace {
            i -= grapheme.chars().count();
            graphemes.pop();
        } else {
            break;
        }
    }
    if i == 0 || graphemes.is_empty() {
        return i;
    }

    let kind = word_nav_kind(graphemes.last().map(|(_, g)| *g).unwrap_or(""));
    if kind == WordNavKind::Delimiter || kind == WordNavKind::Cjk {
        while let Some((_, grapheme)) = graphemes.last() {
            if word_nav_kind(grapheme) == kind {
                i -= grapheme.chars().count();
                graphemes.pop();
            } else {
                break;
            }
        }
        return i;
    }

    if kind == WordNavKind::Word {
        let mut has_right_word = false;
        while let Some((_, grapheme)) = graphemes.last().copied() {
            let k = word_nav_kind(grapheme);
            if k == WordNavKind::Word {
                has_right_word = true;
                i -= grapheme.chars().count();
                graphemes.pop();
                continue;
            }
            if has_right_word && k == WordNavKind::Delimiter && is_word_nav_joiner(grapheme) {
                let left = graphemes
                    .len()
                    .checked_sub(2)
                    .and_then(|idx| graphemes.get(idx))
                    .map(|(_, g)| *g)
                    .unwrap_or("");
                if word_nav_kind(left) == WordNavKind::Word {
                    i -= grapheme.chars().count();
                    graphemes.pop();
                    continue;
                }
            }
            break;
        }
        return i;
    }

    if let Some((_, grapheme)) = graphemes.pop() {
        i -= grapheme.chars().count();
    }
    i.max(0)
}

/// Move the cursor one "word" to the right using Unicode-aware coarse navigation.
pub fn move_word_right(text: &str, cursor: usize) -> usize {
    let len = char_count(text);
    if len == 0 {
        return 0;
    }
    let mut i = cursor.clamp(0, len);
    if i == len {
        return len;
    }

    let spans = grapheme_spans_from(text, i);
    let mut pos = 0;
    while pos < spans.len() && word_nav_kind(spans[pos].1) == WordNavKind::Whitespace {
        i += spans[pos].1.chars().count();
        pos += 1;
    }
    if pos >= spans.len() {
        return i;
    }

    let first_kind = word_nav_kind(spans[pos].1);
    if first_kind == WordNavKind::Delimiter || first_kind == WordNavKind::Cjk {
        while pos < spans.len() && word_nav_kind(spans[pos].1) == first_kind {
            i += spans[pos].1.chars().count();
            pos += 1;
        }
        return i;
    }

    if first_kind == WordNavKind::Word {
        let mut has_left_word = false;
        while pos < spans.len() {
            let grapheme = spans[pos].1;
            let k = word_nav_kind(grapheme);
            if k == WordNavKind::Word {
                has_left_word = true;
                i += grapheme.chars().count();
                pos += 1;
                continue;
            }
            if has_left_word && k == WordNavKind::Delimiter && is_word_nav_joiner(grapheme) {
                let lookahead = pos + 1;
                if lookahead < spans.len() && word_nav_kind(spans[lookahead].1) == WordNavKind::Word
                {
                    i += grapheme.chars().count();
                    pos = lookahead;
                    continue;
                }
            }
            break;
        }
        return i;
    }

    i + spans[pos].1.chars().count()
}

/// `0` whitespace, `1` keyword, `2` punctuation — the classes `w` groups by (`W` folds 2 into 1).
fn char_class(ch: char, big: bool) -> u8 {
    if ch.is_whitespace() {
        return 0;
    }
    if big || ch.is_alphanumeric() || ch == '_' {
        return 1;
    }
    2
}

struct Chunk {
    start: usize,
    end: usize,
    space: bool,
}

/// Split a line into maximal same-class runs — the atoms a counted `iw`/`aw` walks over.
fn chunk_line(text: &str, big: bool) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let cls = char_class(chars[i], big);
        let mut j = i + 1;
        while j < chars.len() && char_class(chars[j], big) == cls {
            j += 1;
        }
        chunks.push(Chunk {
            start: i,
            end: j,
            space: cls == 0,
        });
        i = j;
    }
    chunks
}

/// `iw`/`aw`/`iW`/`aW`, line-local like Vim's. `aw` takes the trailing whitespace run, or the
/// leading one when the word ends the line; starting on whitespace instead takes it plus the
/// following word.
fn word_object(text: &str, col: usize, around: bool, big: bool, count: usize) -> Option<(usize, usize)> {
    let chunks = chunk_line(text, big);
    if chunks.is_empty() {
        return None;
    }
    let last = chunks.len() - 1;
    let mut first = chunks.iter().position(|chunk| col < chunk.end).unwrap_or(last);
    let mut end = first;
    if !around {
        end = first.saturating_add(count.saturating_sub(1)).min(last);
    } else if chunks[first].space {
        end = first
            .saturating_add(count.saturating_sub(1).saturating_mul(2))
            .min(last);
    } else {
        for n in 0..count {
            if n > 0 && end < last {
                end += 1;
            }
            if end < last && chunks[end + 1].space {
                end += 1;
            }
        }
        if !chunks[end].space && first > 0 && chunks[first - 1].space {
            first -= 1;
        }
    }
    Some((chunks[first].start, chunks[end].end))
}

/// `i"`/`a"` and friends, line-local like Vim's: quotes pair off left to right, and the cursor
/// selects the first pair that ends at or after it. `a` swallows the trailing whitespace, or the
/// leading run when there is none.
fn quote_object(text: &str, col: usize, quote: char, around: bool) -> Option<(usize, usize)> {
    let marks = quote_marks(text, quote);
    let mut p = 0;
    while p + 1 < marks.len() {
        let open = marks[p];
        let close = marks[p + 1];
        if col > close {
            p += 2;
            continue;
        }
        if !around {
            return Some((open + 1, close));
        }
        let mut end = close + 1;
        let mut start = open;
        let len = char_count(text);
        while end < len && char_at(text, end).is_some_and(|ch| ch.is_whitespace()) {
            end += 1;
        }
        if end == close + 1 {
            while start > 0 && char_at(text, start - 1).is_some_and(|ch| ch.is_whitespace()) {
                start -= 1;
            }
        }
        return Some((start, end));
    }
    None
}

/// Quote columns, skipping a backslash and the character after it, same as `i"`/`a"`.
fn quote_marks(text: &str, quote: char) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut marks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 1;
        } else if chars[i] == quote {
            marks.push(i);
        }
        i += 1;
    }
    marks
}

fn quote_mate(marks: &[usize], col: usize) -> Option<usize> {
    let mut p = 0;
    while p + 1 < marks.len() {
        let open = marks[p];
        let close = marks[p + 1];
        if col == open {
            return Some(close);
        }
        if col == close {
            return Some(open);
        }
        p += 2;
    }
    None
}

/// Buffer flattened to one string plus the cursor's offset in it, so scans can cross lines.
pub fn flatten(buf: &VimBuffer) -> (String, usize) {
    let mut cursor = buf.cursor_col;
    for line in buf.lines.iter().take(buf.cursor_line) {
        cursor += char_count(line) + 1;
    }
    (buf.lines.join("\n"), cursor)
}

/// Position of a flattened char offset.
pub fn offset_to_pos(lines: &[String], offset: usize) -> VimPosition {
    let mut remaining = offset;
    for (line, text) in lines.iter().enumerate() {
        let width = char_count(text);
        if remaining <= width {
            return VimPosition { line, col: remaining };
        }
        remaining -= width + 1;
    }
    let line = lines.len().saturating_sub(1);
    VimPosition {
        line,
        col: char_count(lines.get(line).map_or("", |text| text.as_str())),
    }
}

/// End-exclusive span an `iw`/`a(`-style text object resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextObjectRange {
    from: VimPosition,
    to: VimPosition,
    linewise: bool,
}

/// A resolved motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Motion {
    to: VimPosition,
    /// Inclusive motions cover the grapheme under `to` when used with an operator.
    inclusive: bool,
    linewise: bool,
}

/// `i(`/`a{`… — the innermost pair enclosing the cursor, counting nesting and spanning lines. A
/// cursor sitting on either delimiter counts as being on that pair. Charwise in both variants;
/// Vim's linewise-ish `i{` reshaping is deliberately not reproduced.
fn bracket_object(buf: &VimBuffer, open: &str, close: &str, around: bool) -> Option<TextObjectRange> {
    let (text, cursor) = flatten(buf);
    let len = char_count(&text);
    if len == 0 {
        return None;
    }
    let mut depth = 0;
    let mut open_at = None;
    let start = cursor.min(len - 1);
    for i in (0..=start).rev() {
        let ch = char_at(&text, i)?;
        if ch.to_string() == close && i != cursor {
            depth += 1;
        } else if ch.to_string() == open {
            if depth == 0 {
                open_at = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let open_at = open_at?;
    let mut depth = 0;
    let mut close_at = None;
    for i in open_at + 1..len {
        let ch = char_at(&text, i)?;
        if ch.to_string() == open {
            depth += 1;
        } else if ch.to_string() == close {
            if depth == 0 {
                close_at = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let close_at = close_at?;
    let from = offset_to_pos(&buf.lines, if around { open_at } else { open_at + 1 });
    let to = offset_to_pos(&buf.lines, if around { close_at + 1 } else { close_at });
    Some(TextObjectRange {
        from,
        to,
        linewise: false,
    })
}

/// `ip`/`ap`: the run of lines matching the cursor line's blankness. `ap` also takes the run that
/// follows, or the one before it when the paragraph ends the buffer. Always linewise.
fn paragraph_object(buf: &VimBuffer, around: bool) -> TextObjectRange {
    let last = buf.lines.len().saturating_sub(1);
    let blank = |line: usize| buf.lines.get(line).is_some_and(|text| text.trim().is_empty());
    let target = blank(buf.cursor_line);
    let mut first = buf.cursor_line;
    let mut end = buf.cursor_line;
    while first > 0 && blank(first - 1) == target {
        first -= 1;
    }
    while end < last && blank(end + 1) == target {
        end += 1;
    }
    if around {
        let stop = end;
        while end < last && blank(end + 1) != target {
            end += 1;
        }
        if end == stop {
            while first > 0 && blank(first - 1) != target {
                first -= 1;
            }
        }
    }
    TextObjectRange {
        from: VimPosition { line: first, col: 0 },
        to: VimPosition {
            line: end,
            col: char_count(buf.lines.get(end).map_or("", |text| text.as_str())),
        },
        linewise: true,
    }
}

struct BracketSpec {
    mate: &'static str,
    forward: bool,
}

fn bracket_match(ch: char) -> Option<BracketSpec> {
    match ch {
        '(' => Some(BracketSpec { mate: ")", forward: true }),
        ')' => Some(BracketSpec { mate: "(", forward: false }),
        '[' => Some(BracketSpec { mate: "]", forward: true }),
        ']' => Some(BracketSpec { mate: "[", forward: false }),
        '{' => Some(BracketSpec { mate: "}", forward: true }),
        '}' => Some(BracketSpec { mate: "{", forward: false }),
        _ => None,
    }
}

/// Column of the match for `%`: the next bracket or quote on this line, then its pair.
fn match_delimiter(buf: &VimBuffer) -> Option<VimPosition> {
    let line = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
    let chars: Vec<char> = line.chars().collect();
    let double = quote_marks(line, '"');
    let single = quote_marks(line, '\'');
    let quote_at = |col: usize| -> Option<char> {
        if double.contains(&col) {
            Some('"')
        } else if single.contains(&col) {
            Some('\'')
        } else {
            None
        }
    };
    let mut col = buf.cursor_col;
    while col < chars.len()
        && bracket_match(chars[col]).is_none()
        && quote_at(col).is_none()
    {
        col += 1;
    }
    if col >= chars.len() {
        return None;
    }
    if let Some(quote) = quote_at(col) {
        let marks = if quote == '"' { &double } else { &single };
        let mate = quote_mate(marks, col)?;
        return Some(VimPosition {
            line: buf.cursor_line,
            col: mate,
        });
    }
    let spec = bracket_match(chars[col])?;
    let scan = VimBuffer {
        lines: buf.lines.clone(),
        cursor_line: buf.cursor_line,
        cursor_col: col,
    };
    let (text, cursor) = flatten(&scan);
    let open = if spec.forward {
        chars[col].to_string()
    } else {
        spec.mate.to_string()
    };
    let close = if spec.forward {
        spec.mate.to_string()
    } else {
        chars[col].to_string()
    };
    let mut depth = 0;
    let mut found = None;
    if spec.forward {
        for i in cursor + 1..char_count(&text) {
            let ch = char_at(&text, i)?;
            if ch.to_string() == open {
                depth += 1;
            } else if ch.to_string() == close {
                if depth == 0 {
                    found = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
    } else {
        for i in (0..cursor).rev() {
            let ch = char_at(&text, i)?;
            if ch.to_string() == close {
                depth += 1;
            } else if ch.to_string() == open {
                if depth == 0 {
                    found = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
    }
    found.map(|match_at| offset_to_pos(&buf.lines, match_at))
}

fn is_paragraph_blank(lines: &[String], line: usize) -> bool {
    lines.get(line).is_some_and(|text| text.trim().is_empty())
}

/// Line `{`/`}` land on. A missing boundary stays on the first or last line.
fn paragraph_boundary(lines: &[String], line: usize, forward: bool) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let last = lines.len() - 1;
    let mut i = line.clamp(0, last);
    if forward {
        if i < last && !is_paragraph_blank(lines, i) {
            while i < last && !is_paragraph_blank(lines, i + 1) {
                i += 1;
            }
            return if i < last { i + 1 } else { last };
        }
        while i < last && is_paragraph_blank(lines, i) {
            i += 1;
        }
        while i < last && !is_paragraph_blank(lines, i) {
            i += 1;
        }
        return i;
    }
    if i > 0 && !is_paragraph_blank(lines, i) {
        while i > 0 && !is_paragraph_blank(lines, i - 1) {
            i -= 1;
        }
        return if i > 0 { i - 1 } else { 0 };
    }
    while i > 0 && is_paragraph_blank(lines, i) {
        i -= 1;
    }
    while i > 0 && !is_paragraph_blank(lines, i - 1) {
        i -= 1;
    }
    if i > 0 {
        i - 1
    } else {
        0
    }
}

/// Commands whose count repeats the typed text rather than a motion (`3.` after `ifoo` →
/// `foofoofoo`).
const INSERT_KEYS: [&str; 7] = ["i", "a", "I", "A", "o", "O", "R"];

fn is_edit_kind(command: &VimCommand) -> bool {
    matches!(
        command,
        VimCommand::Delete { .. }
            | VimCommand::Replace { .. }
            | VimCommand::Join { .. }
            | VimCommand::Indent { .. }
            | VimCommand::Paste { .. }
            | VimCommand::OpenLine { .. }
    )
}

fn parse_count(value: &str) -> usize {
    if value.is_empty() {
        return 1;
    }
    value
        .parse::<f64>()
        .map(|count| count.max(1.0) as usize)
        .unwrap_or(1)
}

/// `i` or `a` typed after an operator or in Visual mode — waiting for the object key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextObject {
    Inside,
    Around,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FindPending {
    forward: bool,
    till: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LastFind {
    forward: bool,
    till: bool,
    target: String,
}

/// Vim's "desired column": `j`/`k` remember the column you started from, so descending through a
/// short line and back out returns to it instead of collapsing permanently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DesiredCol {
    Column(usize),
    /// `$`'s sticky end-of-line, which keeps `$j` on the end of each line.
    End,
}

/// The last change `.` repeats. Normal-mode changes replay their keys so motions and text objects
/// re-resolve at the new cursor; Visual changes replay the selection's shape, as Vim does.
#[derive(Clone, Debug, PartialEq, Eq)]
enum VimChange {
    Keys {
        keys: Vec<String>,
        count: Option<usize>,
        insert: Option<VimInsertRecord>,
    },
    Visual {
        key: String,
        linewise: bool,
        lines: usize,
        graphemes: usize,
        end_col: usize,
        insert: Option<VimInsertRecord>,
    },
}

/// Modal editing state machine.
#[derive(Clone, Debug)]
pub struct VimState {
    /// Current mode.
    pub mode: VimMode,
    /// Fixed end of a Visual selection; the cursor is the moving end.
    pub anchor: Option<VimPosition>,
    count: String,
    operator: Option<VimOperator>,
    pending_g: bool,
    text_object: Option<TextObject>,
    find_pending: Option<FindPending>,
    last_find: Option<LastFind>,
    replace_char: bool,
    desired_col: Option<DesiredCol>,
    cmd_keys: Vec<String>,
    cmd_count: String,
    last_change: Option<VimChange>,
    staged_change: Option<VimChange>,
}

impl Default for VimState {
    fn default() -> Self {
        VimState::new()
    }
}

impl VimState {
    /// A Normal-mode state.
    pub fn new() -> Self {
        VimState {
            mode: VimMode::Normal,
            anchor: None,
            count: String::new(),
            operator: None,
            pending_g: false,
            text_object: None,
            find_pending: None,
            last_find: None,
            replace_char: false,
            desired_col: None,
            cmd_keys: Vec::new(),
            cmd_count: String::new(),
            last_change: None,
            staged_change: None,
        }
    }

    /// True while a count, operator, `g`, text-object, find, or `r` prefix is half-typed — Escape
    /// cancels it.
    pub fn pending(&self) -> bool {
        !self.count.is_empty()
            || self.operator.is_some()
            || self.pending_g
            || self.text_object.is_some()
            || self.find_pending.is_some()
            || self.replace_char
    }

    /// The half-typed command as Vim would echo it (`"2"`, `"d"`, `"2d"`, `"di"`, `"df"`, `"r"`).
    pub fn pending_text(&self) -> String {
        let mut out = self.count.clone();
        if let Some(operator) = self.operator {
            out.push_str(operator.as_str());
        }
        if self.pending_g {
            out.push('g');
        }
        if let Some(text_object) = self.text_object {
            out.push(match text_object {
                TextObject::Inside => 'i',
                TextObject::Around => 'a',
            });
        }
        out.push_str(find_pending_key(self.find_pending));
        if self.replace_char {
            out.push('r');
        }
        out
    }

    /// True in either Visual mode.
    pub fn visual(&self) -> bool {
        self.mode == VimMode::Visual || self.mode == VimMode::VisualLine
    }

    /// Return to a clean Normal-mode state.
    pub fn reset(&mut self) {
        self.mode = VimMode::Normal;
        self.anchor = None;
        self.desired_col = None;
        self.clear_pending();
        self.last_find = None;
        self.cmd_keys.clear();
        self.cmd_count.clear();
        self.last_change = None;
        self.staged_change = None;
    }

    fn clear_pending(&mut self) {
        self.count.clear();
        self.operator = None;
        self.pending_g = false;
        self.text_object = None;
        self.find_pending = None;
        self.replace_char = false;
    }

    fn take_count(&mut self) -> usize {
        let count = parse_count(&self.count);
        self.count.clear();
        count
    }

    /// `0` is the line-start motion unless it extends a count already being typed.
    fn is_count_key(&self, key: &str) -> bool {
        if self.find_pending.is_some() || self.replace_char {
            return false;
        }
        let mut chars = key.chars();
        let Some(ch) = chars.next() else {
            return false;
        };
        if chars.next().is_some() {
            return false;
        }
        ('1'..='9').contains(&ch) || (ch == '0' && !self.count.is_empty())
    }

    /// Clamp a position so the Normal-mode cursor rests *on* a grapheme rather than past the last.
    fn clamp_normal(&self, buf: &VimBuffer, pos: VimPosition) -> VimPosition {
        let line = pos.line.clamp(0, buf.lines.len().saturating_sub(1));
        let text = buf.lines.get(line).map_or("", |text| text.as_str());
        VimPosition {
            line,
            col: pos.col.min(last_grapheme_start(text)),
        }
    }

    /// Handle one key. `key` is either the literal `"escape"` or a single grapheme; the editor
    /// normalizes arrow/Home/End keys onto their Vim equivalents before calling.
    ///
    /// Returns the commands to apply, or `None` when the key is not ours — the editor then falls
    /// through to its regular handling (and, for Escape in Normal mode, to the app's interrupt).
    pub fn handle_key(&mut self, key: &str, buf: &VimBuffer) -> Option<Vec<VimCommand>> {
        if key == "escape" {
            self.cmd_keys.clear();
            self.cmd_count.clear();
            return self.handle_escape(buf);
        }
        if self.mode == VimMode::Insert {
            return None;
        }
        let recording = self.mode == VimMode::Normal;
        let dot_is_target = self.find_pending.is_some() || self.replace_char;
        if recording {
            if self.is_count_key(key) {
                self.cmd_count.push_str(key);
            } else {
                self.cmd_keys.push(key.to_string());
            }
        }
        let commands = self.dispatch(key, buf);
        if recording && !self.pending() {
            let mode = self.mode;
            let editing = mode == VimMode::Insert || mode == VimMode::Replace;
            if commands.is_some()
                && (key != "." || dot_is_target)
                && (editing || commands.as_ref().is_some_and(|cmds| cmds.iter().any(is_edit_kind)))
            {
                let insert_key = self
                    .cmd_keys
                    .first()
                    .is_some_and(|key| INSERT_KEYS.contains(&key.as_str()));
                let change = VimChange::Keys {
                    keys: self.cmd_keys.clone(),
                    count: if insert_key || self.cmd_count.is_empty() {
                        None
                    } else {
                        Some(parse_count(&self.cmd_count))
                    },
                    insert: None,
                };
                if editing {
                    self.last_change = Some(change);
                    self.staged_change = None;
                } else {
                    self.staged_change = Some(change);
                }
            }
            self.cmd_keys.clear();
            self.cmd_count.clear();
        }
        commands
    }

    /// Attach what an Insert/Replace session typed to the change that opened it. After an arrow key
    /// split the session, Vim's redo becomes a plain insert of the text typed since.
    pub fn record_insert(&mut self, record: VimInsertRecord, after_arrow: bool) {
        if after_arrow {
            self.last_change = Some(VimChange::Keys {
                keys: vec!["i".to_string()],
                count: None,
                insert: Some(record),
            });
        } else if let Some(change) = self.last_change.take() {
            self.last_change = Some(match change {
                VimChange::Keys {
                    keys,
                    count,
                    insert: _,
                } => VimChange::Keys {
                    keys,
                    count,
                    insert: Some(record),
                },
                visual => visual,
            });
        } else if !record.text.is_empty() || record.delete_before > 0 || record.delete_after > 0
        {
            self.last_change = Some(VimChange::Keys {
                keys: vec!["i".to_string()],
                count: None,
                insert: Some(record),
            });
        }
    }

    /// Commit a staged change when the editor confirms text changed.
    pub fn commit_change(&mut self) {
        if let Some(change) = self.staged_change.take() {
            self.last_change = Some(change);
        }
    }

    /// Discard a staged change when the command was a no-op (e.g. `J` on the last line).
    pub fn discard_change(&mut self) {
        self.staged_change = None;
    }

    fn repeat(&mut self, buf: &VimBuffer, dot_count: Option<usize>) -> Vec<VimCommand> {
        let Some(change) = self.last_change.clone() else {
            return Vec::new();
        };
        let mut commands = Vec::new();
        let mut count = None;
        if let VimChange::Keys {
            keys,
            count: change_count,
            insert,
        } = &change
        {
            let resolved = dot_count.or(*change_count);
            count = resolved;
            let mut sequence: Vec<String> = Vec::new();
            if let Some(resolved) = resolved {
                sequence.extend(resolved.to_string().chars().map(|digit| digit.to_string()));
            }
            sequence.extend(keys.iter().cloned());
            for key in &sequence {
                commands = self.dispatch(key, buf).unwrap_or_default();
            }
            self.last_change = Some(VimChange::Keys {
                keys: keys.clone(),
                count: resolved,
                insert: insert.clone(),
            });
        } else {
            commands = self.repeat_visual(&change, buf);
        }
        if self.mode == VimMode::Insert || self.mode == VimMode::Replace {
            let insert_key = matches!(
                &self.last_change,
                Some(VimChange::Keys { keys, .. }) if keys.first().is_some_and(|key| INSERT_KEYS.contains(&key.as_str()))
            );
            let (times, newline) = match (&self.last_change, count) {
                (Some(VimChange::Keys { keys, .. }), _) if insert_key => (
                    count.unwrap_or(1),
                    keys.first().is_some_and(|key| key == "o" || key == "O"),
                ),
                _ => (1, false),
            };
            commands.push(VimCommand::Retype {
                record: match &self.last_change {
                    Some(VimChange::Keys { insert, .. }) => insert.clone().unwrap_or_default(),
                    _ => VimInsertRecord::default(),
                },
                times,
                newline,
                overwrite: self.mode == VimMode::Replace,
            });
            self.mode = VimMode::Normal;
            commands.push(VimCommand::Mode {
                mode: VimMode::Normal,
            });
        }
        commands
    }

    /// Re-applies a Visual change over the same shape starting at the cursor; `.`'s count is
    /// ignored.
    fn repeat_visual(&mut self, change: &VimChange, buf: &VimBuffer) -> Vec<VimCommand> {
        let VimChange::Visual {
            key,
            linewise,
            lines,
            graphemes,
            end_col,
            ..
        } = change
        else {
            return Vec::new();
        };
        let from_line = buf.cursor_line;
        let to_line = (from_line + lines.saturating_sub(1)).min(buf.lines.len().saturating_sub(1));
        let line_at = |line: usize| buf.lines.get(line).map_or("", |text| text.as_str());
        let cursor = VimPosition {
            line: buf.cursor_line,
            col: buf.cursor_col,
        };
        let (from, to);
        if *linewise {
            from = VimPosition {
                line: from_line,
                col: 0,
            };
            to = VimPosition {
                line: to_line,
                col: char_count(line_at(to_line)),
            };
        } else if *lines == 1 {
            from = cursor;
            to = VimPosition {
                line: from_line,
                col: step_graphemes(line_at(from_line), buf.cursor_col, *graphemes, true),
            };
        } else {
            from = cursor;
            // `end_col` was measured on the original last line, so it can land mid-grapheme on a
            // destination line with longer clusters; snap it up to a boundary or a replayed `d`
            // would leave half an emoji behind.
            let text = line_at(to_line);
            let target = (*end_col).min(char_count(text));
            let mut snapped = char_count(text);
            for (start, grapheme) in grapheme_spans(text) {
                if start >= target {
                    snapped = start;
                    break;
                }
                snapped = start + grapheme.chars().count();
            }
            to = VimPosition {
                line: to_line,
                col: snapped,
            };
        }
        self.visual_edit(key, from, to, *linewise)
    }

    fn dispatch(&mut self, key: &str, buf: &VimBuffer) -> Option<Vec<VimCommand>> {
        // The character after `f`/`t` is the target, including digits. Resolve it before the count
        // prefix.
        if let Some(spec) = self.find_pending.take() {
            self.desired_col = None;
            self.last_find = Some(LastFind {
                forward: spec.forward,
                till: spec.till,
                target: key.to_string(),
            });
            let count = parse_count(&self.count);
            self.count.clear();
            let motion = self.seek(key, buf, spec, count, None);
            if motion.is_none() {
                self.operator = None;
                self.count.clear();
                return Some(Vec::new());
            }
            return Some(self.apply_motion(buf, motion?));
        }
        if self.replace_char {
            self.replace_char = false;
            let count = parse_count(&self.count);
            self.count.clear();
            let line = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
            // One pass: stepping with `next_grapheme_start` would rescan the line per grapheme.
            let mut end = None;
            let mut taken = 0;
            for (start, grapheme) in grapheme_spans(line) {
                if start < buf.cursor_col {
                    continue;
                }
                taken += 1;
                if taken == count {
                    end = Some(start + grapheme.chars().count());
                    break;
                }
            }
            let Some(end) = end else {
                return Some(Vec::new());
            };
            return Some(vec![VimCommand::Replace {
                from: VimPosition {
                    line: buf.cursor_line,
                    col: buf.cursor_col,
                },
                to: VimPosition {
                    line: buf.cursor_line,
                    col: end,
                },
                text: key.repeat(count),
            }]);
        }

        if self.is_count_key(key) {
            self.count.push_str(key);
            return Some(Vec::new());
        }

        if self.pending_g {
            self.pending_g = false;
            if key != "g" {
                self.clear_pending();
                return Some(Vec::new());
            }
            // `gg` goes to the first line; `5gg` to line 5.
            self.desired_col = None;
            let line = self.take_count().saturating_sub(1).min(buf.lines.len().saturating_sub(1));
            return Some(self.apply_motion(
                buf,
                Motion {
                    to: VimPosition { line, col: 0 },
                    inclusive: false,
                    linewise: true,
                },
            ));
        }

        if self.text_object.is_some() {
            self.desired_col = None;
            return Some(self.apply_text_object(key, buf));
        }

        // `i`/`a` only introduce a text object where they cannot mean "insert": after an operator,
        // or in Visual mode. Bare `i` in Normal mode still enters Insert.
        if (key == "i" || key == "a") && (self.operator.is_some() || self.visual()) {
            self.text_object = Some(if key == "i" {
                TextObject::Inside
            } else {
                TextObject::Around
            });
            return Some(Vec::new());
        }
        if key == "f" || key == "F" || key == "t" || key == "T" {
            self.desired_col = None;
            self.find_pending = Some(FindPending {
                forward: key == "f" || key == "t",
                till: key == "t" || key == "T",
            });
            return Some(Vec::new());
        }
        if (key == "r" || key == "R") && self.operator.is_none() && !self.visual() {
            self.desired_col = None;
            if key == "R" {
                self.take_count();
                self.mode = VimMode::Replace;
                return Some(vec![VimCommand::Mode {
                    mode: VimMode::Replace,
                }]);
            }
            self.replace_char = true;
            return Some(Vec::new());
        }

        // Only consecutive `j`/`k` carry the desired column; anything else re-anchors it. Counts
        // and the `g` prefix returned above, so `2j` still continues an established column.
        if key != "j" && key != "k" {
            self.desired_col = None;
        }

        if let Some(motion) = self.resolve_motion(key, buf) {
            return Some(self.apply_motion(buf, motion));
        }

        if self.visual() {
            self.handle_visual_key(key, buf)
        } else {
            self.handle_normal_key(key, buf)
        }
    }

    fn handle_escape(&mut self, buf: &VimBuffer) -> Option<Vec<VimCommand>> {
        if self.pending() {
            self.clear_pending();
            return Some(Vec::new());
        }
        if self.visual() {
            self.mode = VimMode::Normal;
            self.anchor = None;
            return Some(vec![
                VimCommand::Mode {
                    mode: VimMode::Normal,
                },
                VimCommand::Move {
                    to: self.clamp_normal(
                        buf,
                        VimPosition {
                            line: buf.cursor_line,
                            col: buf.cursor_col,
                        },
                    ),
                },
            ]);
        }
        if self.mode == VimMode::Insert || self.mode == VimMode::Replace {
            self.mode = VimMode::Normal;
            let text = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
            return Some(vec![
                VimCommand::Mode {
                    mode: VimMode::Normal,
                },
                VimCommand::Move {
                    to: VimPosition {
                        line: buf.cursor_line,
                        col: prev_grapheme_start(text, buf.cursor_col),
                    },
                },
            ]);
        }
        // Normal mode with nothing pending: leave Escape to the app (interrupt / clear draft).
        None
    }

    fn resolve_motion(&mut self, key: &str, buf: &VimBuffer) -> Option<Motion> {
        let count = parse_count(&self.count);
        let line = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
        let at = |col: usize| VimPosition {
            line: buf.cursor_line,
            col,
        };
        let key = if key == " " { "l" } else { key };

        match key {
            "h" => {
                let mut col = buf.cursor_col;
                for _ in 0..count {
                    col = prev_grapheme_start(line, col);
                }
                Some(Motion {
                    to: at(col),
                    inclusive: false,
                    linewise: false,
                })
            }
            "l" => {
                let mut col = buf.cursor_col;
                for _ in 0..count {
                    col = next_grapheme_start(line, col);
                }
                Some(Motion {
                    to: at(col),
                    inclusive: false,
                    linewise: false,
                })
            }
            "j" | "k" => {
                let delta = if key == "j" { count as isize } else { -(count as isize) };
                let target = (buf.cursor_line as isize + delta)
                    .clamp(0, buf.lines.len().saturating_sub(1) as isize) as usize;
                // Anchor the desired column on the first vertical move, then keep reusing it. The
                // host clamps the target to each line's length, so short lines en route never
                // shrink it.
                if self.desired_col.is_none() {
                    self.desired_col = Some(DesiredCol::Column(buf.cursor_col));
                }
                let col = match self.desired_col.unwrap() {
                    DesiredCol::Column(col) => col,
                    DesiredCol::End => char_count(buf.lines.get(target).map_or("", |text| text.as_str())),
                };
                Some(Motion {
                    to: VimPosition {
                        line: target,
                        col,
                    },
                    inclusive: false,
                    linewise: true,
                })
            }
            "0" => Some(Motion {
                to: at(0),
                inclusive: false,
                linewise: false,
            }),
            "^" => Some(Motion {
                to: at(first_non_blank(line)),
                inclusive: false,
                linewise: false,
            }),
            "$" => {
                // Sticky end-of-line, so `$j` lands on the end of each line rather than a fixed
                // column.
                self.desired_col = Some(DesiredCol::End);
                Some(Motion {
                    to: at(char_count(line)),
                    inclusive: false,
                    linewise: false,
                })
            }
            "w" => {
                let mut col = buf.cursor_col;
                // Vim's `cw` quirk: standing on a non-blank, it changes to the end of the word
                // like `ce` rather than swallowing the whitespace that follows it.
                if self.operator == Some(VimOperator::Change)
                    && !char_at(line, buf.cursor_col).is_some_and(|ch| ch.is_whitespace())
                {
                    for _ in 0..count {
                        col = word_end(line, col);
                    }
                    return Some(Motion {
                        to: at(col),
                        inclusive: true,
                        linewise: false,
                    });
                }
                for _ in 0..count {
                    col = word_forward(line, col);
                }
                Some(Motion {
                    to: at(col),
                    inclusive: false,
                    linewise: false,
                })
            }
            "b" => {
                let mut col = buf.cursor_col;
                for _ in 0..count {
                    col = move_word_left(line, col);
                }
                Some(Motion {
                    to: at(col),
                    inclusive: false,
                    linewise: false,
                })
            }
            "e" => {
                let mut col = buf.cursor_col;
                for _ in 0..count {
                    col = word_end(line, col);
                }
                Some(Motion {
                    to: at(col),
                    inclusive: true,
                    linewise: false,
                })
            }
            "%" => {
                // `{count}%` jumps to that percentage of the buffer, linewise, like Vim.
                if !self.count.is_empty() {
                    if count > 100 {
                        return None;
                    }
                    let target = (count.saturating_mul(buf.lines.len()) + 99) / 100;
                    let target = target.saturating_sub(1);
                    let text = buf.lines.get(target).map_or("", |text| text.as_str());
                    return Some(Motion {
                        to: VimPosition {
                            line: target,
                            col: first_non_blank(text),
                        },
                        inclusive: false,
                        linewise: true,
                    });
                }
                match match_delimiter(buf) {
                    Some(to) => Some(Motion {
                        to,
                        inclusive: true,
                        linewise: false,
                    }),
                    None => None,
                }
            }
            "{" | "}" => {
                let mut line = buf.cursor_line;
                for _ in 0..count {
                    let next = paragraph_boundary(&buf.lines, line, key == "}");
                    if next == line {
                        break;
                    }
                    line = next;
                }
                let text = buf.lines.get(line).map_or("", |text| text.as_str());
                // No blank line ahead: Vim parks at the end of the buffer, not column 0. A bare
                // move clamps back onto the last grapheme; an operator still covers the rest of
                // the line.
                let at_buffer_end =
                    key == "}" && line == buf.lines.len().saturating_sub(1) && !text.trim().is_empty();
                Some(Motion {
                    to: VimPosition {
                        line,
                        col: if at_buffer_end { char_count(text) } else { 0 },
                    },
                    inclusive: false,
                    linewise: false,
                })
            }
            "G" => {
                let target = if self.count.is_empty() {
                    buf.lines.len().saturating_sub(1)
                } else {
                    count.saturating_sub(1)
                };
                Some(Motion {
                    to: VimPosition {
                        line: target.min(buf.lines.len().saturating_sub(1)),
                        col: 0,
                    },
                    inclusive: false,
                    linewise: true,
                })
            }
            ";" | "," => self.repeat_find(key == ";", buf, count),
            _ => None,
        }
    }

    fn repeat_find(&self, same_direction: bool, buf: &VimBuffer, count: usize) -> Option<Motion> {
        let last = self.last_find.as_ref()?;
        let forward = if same_direction { last.forward } else { !last.forward };
        let text = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
        // A till lands beside its target, so repeating from the cursor rediscovers that same
        // target.
        let from_col = if last.till {
            if forward {
                next_grapheme_start(text, buf.cursor_col)
            } else {
                prev_grapheme_start(text, buf.cursor_col)
            }
        } else {
            buf.cursor_col
        };
        self.seek(
            &last.target,
            buf,
            FindPending {
                forward,
                till: last.till,
            },
            count,
            Some(from_col),
        )
    }

    /// `f`/`F` land on the match and include it. `t`/`T` land on the adjacent grapheme and include
    /// that grapheme, so the operator stops short of the target. A till that would not move fails.
    fn seek(
        &self,
        target: &str,
        buf: &VimBuffer,
        spec: FindPending,
        count: usize,
        from_col: Option<usize>,
    ) -> Option<Motion> {
        let text = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
        let from_col = from_col.unwrap_or(buf.cursor_col);
        let match_at = find_grapheme(text, from_col, target, spec.forward, count)?;
        let land = if spec.till {
            if spec.forward {
                prev_grapheme_start(text, match_at)
            } else {
                next_grapheme_start(text, match_at)
            }
        } else {
            match_at
        };
        if land == buf.cursor_col {
            return None;
        }
        Some(Motion {
            to: VimPosition {
                line: buf.cursor_line,
                col: land,
            },
            inclusive: true,
            linewise: false,
        })
    }

    /// Turn a resolved motion into either a cursor move, a selection extension, or an operator
    /// range.
    fn apply_motion(&mut self, buf: &VimBuffer, motion: Motion) -> Vec<VimCommand> {
        let operator = self.operator;
        self.operator = None;
        self.take_count();
        if operator == Some(VimOperator::Indent) || operator == Some(VimOperator::Outdent) {
            let from_line = buf.cursor_line.min(motion.to.line);
            let to_line = buf.cursor_line.max(motion.to.line);
            return vec![VimCommand::Indent {
                from_line,
                to_line,
                out: operator == Some(VimOperator::Outdent),
            }];
        }

        if operator.is_none() {
            // `$` parks on the last grapheme in Normal mode but must still be able to select the
            // final character in Visual mode, where the cursor is allowed one past it. Replace
            // mode may also sit past the end, so typing there appends instead of overwriting the
            // last grapheme.
            let to = if self.visual() || self.mode == VimMode::Replace {
                let length = char_count(buf.lines.get(motion.to.line).map_or("", |text| text.as_str()));
                VimPosition {
                    line: motion.to.line,
                    col: motion.to.col.min(length),
                }
            } else {
                self.clamp_normal(buf, motion.to)
            };
            return vec![VimCommand::Move { to }];
        }

        let from = VimPosition {
            line: buf.cursor_line,
            col: buf.cursor_col,
        };
        let mut start = from;
        let mut end = motion.to;
        if end.line < start.line || (end.line == start.line && end.col < start.col) {
            std::mem::swap(&mut start, &mut end);
        }
        if motion.inclusive {
            let text = buf.lines.get(end.line).map_or("", |text| text.as_str());
            end = VimPosition {
                line: end.line,
                col: next_grapheme_start(text, end.col),
            };
        }
        self.operate(operator.unwrap(), start, end, motion.linewise)
    }

    fn operate(
        &mut self,
        operator: VimOperator,
        from: VimPosition,
        to: VimPosition,
        linewise: bool,
    ) -> Vec<VimCommand> {
        if operator == VimOperator::Indent || operator == VimOperator::Outdent {
            let from_line = from.line.min(to.line);
            let to_line = from.line.max(to.line);
            return vec![VimCommand::Indent {
                from_line,
                to_line,
                out: operator == VimOperator::Outdent,
            }];
        }
        if operator == VimOperator::Yank {
            return vec![
                VimCommand::Yank { from, to, linewise },
                VimCommand::Move { to: from },
            ];
        }
        if operator != VimOperator::Change {
            return vec![VimCommand::Delete {
                from,
                to,
                linewise,
                insert: false,
            }];
        }
        // `c` always lands in Insert mode. The leading move matters when the range is empty
        // (`ci"` between bare quotes): the delete is a no-op, so nothing else would park the
        // cursor. `cc`/`cj` clear the lines but keep them, so a linewise change stays
        // linewise-shaped.
        self.mode = VimMode::Insert;
        vec![
            VimCommand::Move { to: from },
            VimCommand::Delete {
                from,
                to,
                linewise: false,
                insert: true,
            },
            VimCommand::Mode {
                mode: VimMode::Insert,
            },
        ]
    }

    /// `iw`, `a(`, `i"`, `ap`, … — resolved around the cursor rather than from a motion endpoint.
    fn resolve_text_object(
        &self,
        key: &str,
        around: bool,
        buf: &VimBuffer,
        count: usize,
    ) -> Option<TextObjectRange> {
        let line = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
        let on_line = |range: Option<(usize, usize)>| -> Option<TextObjectRange> {
            range.map(|(from, to)| TextObjectRange {
                from: VimPosition {
                    line: buf.cursor_line,
                    col: from,
                },
                to: VimPosition {
                    line: buf.cursor_line,
                    col: to,
                },
                linewise: false,
            })
        };

        match key {
            "w" | "W" => on_line(word_object(line, buf.cursor_col, around, key == "W", count)),
            "\"" | "'" | "`" => {
                let quote = key.chars().next()?;
                on_line(quote_object(line, buf.cursor_col, quote, around))
            }
            "(" | ")" | "b" => bracket_object(buf, "(", ")", around),
            "[" | "]" => bracket_object(buf, "[", "]", around),
            "{" | "}" | "B" => bracket_object(buf, "{", "}", around),
            "<" | ">" => bracket_object(buf, "<", ">", around),
            "p" => Some(paragraph_object(buf, around)),
            _ => None,
        }
    }

    /// Consume the object key that follows a pending `i`/`a`. Under an operator the object becomes
    /// the operated range (`diw`); in Visual mode it becomes the selection instead (`viw`).
    fn apply_text_object(&mut self, key: &str, buf: &VimBuffer) -> Vec<VimCommand> {
        let around = self.text_object == Some(TextObject::Around);
        let operator = self.operator;
        self.text_object = None;
        self.operator = None;
        let count = self.take_count();
        let Some(range) = self.resolve_text_object(key, around, buf, count) else {
            return Vec::new();
        };
        if let Some(operator) = operator {
            return self.operate(operator, range.from, range.to, range.linewise);
        }

        self.anchor = Some(range.from);
        let mut commands = Vec::new();
        // A linewise object in charwise Visual mode promotes the selection, as Vim's `vip` does.
        if range.linewise && self.mode == VimMode::Visual {
            self.mode = VimMode::VisualLine;
            commands.push(VimCommand::Mode {
                mode: self.mode,
            });
        }
        // The selection's moving end sits *on* the object's last grapheme, not one past it.
        let mut to = range.to;
        if to.col > 0 {
            let text = buf.lines.get(to.line).map_or("", |text| text.as_str());
            to = VimPosition {
                line: to.line,
                col: prev_grapheme_start(text, to.col),
            };
        } else if to.line > range.from.line {
            let text = buf.lines.get(to.line - 1).map_or("", |text| text.as_str());
            to = VimPosition {
                line: to.line - 1,
                col: last_grapheme_start(text),
            };
        } else {
            to = range.from;
        }
        commands.push(VimCommand::Move { to });
        commands
    }

    fn handle_normal_key(&mut self, key: &str, buf: &VimBuffer) -> Option<Vec<VimCommand>> {
        let line = buf.lines.get(buf.cursor_line).map_or("", |text| text.as_str());
        let count = parse_count(&self.count);

        match key {
            "g" => {
                self.pending_g = true;
                Some(Vec::new())
            }
            "i" => {
                self.take_count();
                self.mode = VimMode::Insert;
                Some(vec![VimCommand::Mode {
                    mode: VimMode::Insert,
                }])
            }
            "a" => {
                self.take_count();
                self.mode = VimMode::Insert;
                Some(vec![
                    VimCommand::Mode {
                        mode: VimMode::Insert,
                    },
                    VimCommand::Move {
                        to: VimPosition {
                            line: buf.cursor_line,
                            col: next_grapheme_start(line, buf.cursor_col),
                        },
                    },
                ])
            }
            "I" => {
                self.take_count();
                self.mode = VimMode::Insert;
                Some(vec![
                    VimCommand::Mode {
                        mode: VimMode::Insert,
                    },
                    VimCommand::Move {
                        to: VimPosition {
                            line: buf.cursor_line,
                            col: first_non_blank(line),
                        },
                    },
                ])
            }
            "A" => {
                self.take_count();
                self.mode = VimMode::Insert;
                Some(vec![
                    VimCommand::Mode {
                        mode: VimMode::Insert,
                    },
                    VimCommand::Move {
                        to: VimPosition {
                            line: buf.cursor_line,
                            col: char_count(line),
                        },
                    },
                ])
            }
            "o" | "O" => {
                self.take_count();
                self.mode = VimMode::Insert;
                Some(vec![
                    VimCommand::OpenLine {
                        below: key == "o",
                    },
                    VimCommand::Mode {
                        mode: VimMode::Insert,
                    },
                ])
            }
            "v" | "V" => {
                self.take_count();
                self.mode = if key == "v" {
                    VimMode::Visual
                } else {
                    VimMode::VisualLine
                };
                self.anchor = Some(VimPosition {
                    line: buf.cursor_line,
                    col: buf.cursor_col,
                });
                Some(vec![VimCommand::Mode {
                    mode: self.mode,
                }])
            }
            "x" => {
                self.take_count();
                let mut col = buf.cursor_col;
                let length = char_count(line);
                for _ in 0..count {
                    if col >= length {
                        break;
                    }
                    col = next_grapheme_start(line, col);
                }
                if col == buf.cursor_col {
                    return Some(Vec::new());
                }
                Some(vec![VimCommand::Delete {
                    from: VimPosition {
                        line: buf.cursor_line,
                        col: buf.cursor_col,
                    },
                    to: VimPosition {
                        line: buf.cursor_line,
                        col,
                    },
                    linewise: false,
                    insert: false,
                }])
            }
            "D" | "C" => {
                // Like Vim, `D`/`C` take a count: `2D` deletes to the end of the next line, not
                // just this one (`:h D` — "and [count]-1 more lines").
                let span = self.take_count();
                let last = (buf.cursor_line + span.saturating_sub(1))
                    .min(buf.lines.len().saturating_sub(1));
                Some(self.operate(
                    if key == "C" {
                        VimOperator::Change
                    } else {
                        VimOperator::Delete
                    },
                    VimPosition {
                        line: buf.cursor_line,
                        col: buf.cursor_col,
                    },
                    VimPosition {
                        line: last,
                        col: char_count(buf.lines.get(last).map_or("", |text| text.as_str())),
                    },
                    false,
                ))
            }
            "d" | "y" | "c" => {
                let operator = match key {
                    "d" => VimOperator::Delete,
                    "y" => VimOperator::Yank,
                    _ => VimOperator::Change,
                };
                // A doubled operator (`dd`, `yy`, `cc`) is linewise over `count` lines.
                if self.operator == Some(operator) {
                    let span = self.take_count();
                    let last = (buf.cursor_line + span.saturating_sub(1))
                        .min(buf.lines.len().saturating_sub(1));
                    self.operator = None;
                    return Some(self.operate(
                        operator,
                        VimPosition {
                            line: buf.cursor_line,
                            col: 0,
                        },
                        VimPosition {
                            line: last,
                            col: char_count(
                                buf.lines.get(last).map_or("", |text| text.as_str()),
                            ),
                        },
                        true,
                    ));
                }
                self.operator = Some(operator);
                Some(Vec::new())
            }
            "p" | "P" => Some(vec![VimCommand::Paste {
                after: key == "p",
                count: self.take_count(),
            }]),
            "u" => {
                self.take_count();
                Some(vec![VimCommand::Undo])
            }
            "." => {
                if self.operator.is_some() {
                    self.clear_pending();
                    return Some(Vec::new());
                }
                let dot_count = if self.count.is_empty() {
                    None
                } else {
                    Some(self.take_count())
                };
                Some(self.repeat(buf, dot_count))
            }
            "J" => {
                if self.operator.is_some() {
                    self.clear_pending();
                    return Some(Vec::new());
                }
                Some(vec![VimCommand::Join {
                    from_line: buf.cursor_line,
                    lines: self.take_count().max(2),
                }])
            }
            ">" | "<" => {
                let operator = if key == ">" {
                    VimOperator::Indent
                } else {
                    VimOperator::Outdent
                };
                if self.operator == Some(operator) {
                    let span = self.take_count();
                    let last = (buf.cursor_line + span.saturating_sub(1))
                        .min(buf.lines.len().saturating_sub(1));
                    self.operator = None;
                    return Some(vec![VimCommand::Indent {
                        from_line: buf.cursor_line,
                        to_line: last,
                        out: key == "<",
                    }]);
                }
                if self.operator.is_some() {
                    self.clear_pending();
                    return Some(Vec::new());
                }
                self.operator = Some(operator);
                Some(Vec::new())
            }
            _ => {
                // Normal mode swallows unknown printable keys rather than typing them into the
                // buffer.
                self.clear_pending();
                Some(Vec::new())
            }
        }
    }

    fn handle_visual_key(&mut self, key: &str, buf: &VimBuffer) -> Option<Vec<VimCommand>> {
        let anchor = self.anchor.unwrap_or(VimPosition {
            line: buf.cursor_line,
            col: buf.cursor_col,
        });
        let linewise = self.mode == VimMode::VisualLine;

        match key {
            "v" | "V" => {
                let next = if key == "v" {
                    VimMode::Visual
                } else {
                    VimMode::VisualLine
                };
                if self.mode == next {
                    self.mode = VimMode::Normal;
                    self.anchor = None;
                    return Some(vec![
                        VimCommand::Mode {
                            mode: VimMode::Normal,
                        },
                        VimCommand::Move {
                            to: self.clamp_normal(
                                buf,
                                VimPosition {
                                    line: buf.cursor_line,
                                    col: buf.cursor_col,
                                },
                            ),
                        },
                    ]);
                }
                self.mode = next;
                Some(vec![VimCommand::Mode { mode: next }])
            }
            "o" => {
                self.anchor = Some(VimPosition {
                    line: buf.cursor_line,
                    col: buf.cursor_col,
                });
                Some(vec![VimCommand::Move { to: anchor }])
            }
            "y" | "d" | "x" | "c" | "s" | ">" | "<" | "J" => {
                let (from, to) = visual_range(buf, anchor, linewise);
                self.clear_pending();
                self.anchor = None;
                if key != "y" {
                    let graphemes = if from.line == to.line {
                        count_graphemes(&char_slice(
                            buf.lines.get(from.line).map_or("", |text| text.as_str()),
                            from.col,
                            to.col,
                        ))
                    } else {
                        0
                    };
                    let change = VimChange::Visual {
                        key: key.to_string(),
                        linewise,
                        lines: to.line.saturating_sub(from.line) + 1,
                        graphemes,
                        end_col: to.col,
                        insert: None,
                    };
                    if key == "c" || key == "s" {
                        self.last_change = Some(change);
                        self.staged_change = None;
                    } else {
                        self.staged_change = Some(change);
                    }
                }
                Some(self.visual_edit(key, from, to, linewise))
            }
            _ => {
                self.clear_pending();
                Some(Vec::new())
            }
        }
    }

    /// Apply a Visual-mode edit key to the span `from`–`to` (end-exclusive), live or from `.`.
    fn visual_edit(
        &mut self,
        key: &str,
        from: VimPosition,
        to: VimPosition,
        linewise: bool,
    ) -> Vec<VimCommand> {
        if key == ">" || key == "<" || key == "J" {
            self.mode = VimMode::Normal;
            let edit = if key == "J" {
                VimCommand::Join {
                    from_line: from.line,
                    lines: (to.line.saturating_sub(from.line) + 1).max(2),
                }
            } else {
                VimCommand::Indent {
                    from_line: from.line,
                    to_line: to.line,
                    out: key == "<",
                }
            };
            return vec![
                edit,
                VimCommand::Mode {
                    mode: VimMode::Normal,
                },
            ];
        }
        let operator = if key == "y" {
            VimOperator::Yank
        } else if key == "c" || key == "s" {
            VimOperator::Change
        } else {
            VimOperator::Delete
        };
        // `operate` switches to Insert itself for `c`; everything else drops back to Normal.
        if operator != VimOperator::Change {
            self.mode = VimMode::Normal;
        }
        let mut commands = self.operate(operator, from, to, linewise);
        if operator != VimOperator::Change {
            commands.push(VimCommand::Mode {
                mode: VimMode::Normal,
            });
        }
        commands
    }
}

/// Normalized, end-exclusive span covered by a Visual selection.
///
/// Charwise selections include the grapheme under the cursor (Vim semantics); linewise selections
/// span whole lines, with `to.col` at the end of the last line so the caller can decide whether the
/// trailing newline goes too.
pub fn visual_range(
    buf: &VimBuffer,
    anchor: VimPosition,
    linewise: bool,
) -> (VimPosition, VimPosition) {
    let cursor = VimPosition {
        line: buf.cursor_line,
        col: buf.cursor_col,
    };
    let mut from = anchor;
    let mut to = cursor;
    if to.line < from.line || (to.line == from.line && to.col < from.col) {
        std::mem::swap(&mut from, &mut to);
    }

    if linewise {
        let last_line = to.line.min(buf.lines.len().saturating_sub(1));
        return (
            VimPosition {
                line: from.line,
                col: 0,
            },
            VimPosition {
                line: last_line,
                col: char_count(buf.lines.get(last_line).map_or("", |text| text.as_str())),
            },
        );
    }
    let text = buf.lines.get(to.line).map_or("", |text| text.as_str());
    (
        from,
        VimPosition {
            line: to.line,
            col: next_grapheme_start(text, to.col),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(lines: &[&str]) -> VimBuffer {
        VimBuffer {
            lines: lines.iter().map(|line| line.to_string()).collect(),
            cursor_line: 0,
            cursor_col: 0,
        }
    }

    fn pos(line: usize, col: usize) -> VimPosition {
        VimPosition { line, col }
    }

    #[test]
    fn grapheme_helpers_use_char_indices() {
        let text = "a\u{301}\u{1f44b}b";
        assert_eq!(char_count(text), 4);
        assert_eq!(count_graphemes(text), 3);
        assert_eq!(next_grapheme_start(text, 0), 2);
        assert_eq!(next_grapheme_start(text, 1), 3);
        assert_eq!(prev_grapheme_start(text, 2), 0);
        assert_eq!(last_grapheme_start(text), 3);
        assert_eq!(step_graphemes(text, 1, 1, true), 2);
        assert_eq!(step_graphemes(text, 2, 1, false), 0);
        assert_eq!(step_graphemes(text, 3, 5, true), 4);
        assert_eq!(step_graphemes(text, 0, 5, false), 0);
    }

    #[test]
    fn diff_insert_session_anchors_at_cursor() {
        let record = diff_insert_session("ab", 1, "axb");
        assert_eq!(record.delete_before, 0);
        assert_eq!(record.delete_after, 1);
        assert_eq!(record.text, "x");

        let emoji = diff_insert_session("\u{1f44b}", 1, "\u{1f44b}\u{1f44b}");
        assert_eq!(emoji.delete_before, 0);
        assert_eq!(emoji.delete_after, 0);
        assert_eq!(emoji.text, "\u{1f44b}");
    }

    #[test]
    fn word_navigation_classifies_and_moves() {
        assert_eq!(word_nav_kind("a"), WordNavKind::Word);
        assert_eq!(word_nav_kind(" "), WordNavKind::Whitespace);
        assert_eq!(word_nav_kind("\u{4e2d}"), WordNavKind::Cjk);
        assert_eq!(word_nav_kind("-"), WordNavKind::Delimiter);
        assert!(is_word_nav_joiner("-"));
        assert!(!is_word_nav_joiner("."));

        assert_eq!(move_word_right("foo bar", 0), 3);
        assert_eq!(word_forward("foo bar", 0), 4);
        assert_eq!(move_word_left("foo bar", 4), 0);
        assert_eq!(move_word_right("foo bar", 4), 7);
        assert_eq!(move_word_left("foo bar", 7), 4);
    }

    #[test]
    fn word_objects_cover_inner_and_around() {
        assert_eq!(word_object("foo bar", 1, false, false, 1), Some((0, 3)));
        assert_eq!(word_object("foo bar", 1, true, false, 1), Some((0, 4)));
        assert_eq!(word_object("foo bar", 3, true, false, 1), Some((3, 7)));
        assert_eq!(word_object("foo bar", 3, false, false, 1), Some((3, 4)));
        assert_eq!(word_object("foo   ", 5, false, false, 1), Some((3, 6)));
    }

    #[test]
    fn quote_objects_pair_off() {
        assert_eq!(quote_object("say \"hi\" now", 5, '"', false), Some((5, 7)));
        assert_eq!(quote_object("say \"hi\" now", 5, '"', true), Some((4, 9)));
        assert_eq!(quote_object("say \"hi\" now", 0, '"', false), None);
    }

    #[test]
    fn flatten_and_offset_round_trip() {
        let buf = VimBuffer::new(vec!["ab".to_string(), "cde".to_string()], 1, 1);
        let (text, cursor) = flatten(&buf);
        assert_eq!(text, "ab\ncde");
        assert_eq!(cursor, 4);
        assert_eq!(offset_to_pos(&buf.lines, 4), pos(1, 1));
        assert_eq!(offset_to_pos(&buf.lines, 0), pos(0, 0));
        assert_eq!(offset_to_pos(&buf.lines, 20), pos(1, 3));
    }

    #[test]
    fn bracket_objects_span_lines() {
        let buf = VimBuffer::new(
            vec!["a (b".to_string(), "c) d".to_string()],
            1,
            1,
        );
        let range = bracket_object(&buf, "(", ")", false).unwrap();
        assert_eq!(range.from, pos(0, 3));
        assert_eq!(range.to, pos(1, 1));
        let around = bracket_object(&buf, "(", ")", true).unwrap();
        assert_eq!(around.from, pos(0, 2));
        assert_eq!(around.to, pos(1, 2));
    }

    #[test]
    fn paragraph_boundary_moves_between_paragraphs() {
        let lines: Vec<String> = ["a", "", "b", "c", "", "d"]
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert_eq!(paragraph_boundary(&lines, 0, true), 1);
        assert_eq!(paragraph_boundary(&lines, 1, true), 4);
        assert_eq!(paragraph_boundary(&lines, 4, false), 1);
        assert_eq!(paragraph_boundary(&lines, 5, false), 4);
    }

    #[test]
    fn visual_range_is_charwise_inclusive() {
        let buf = VimBuffer::new(vec!["abc".to_string()], 0, 1);
        let (from, to) = visual_range(&buf, pos(0, 0), false);
        assert_eq!(from, pos(0, 0));
        assert_eq!(to, pos(0, 2));
        let (from, to) = visual_range(&buf, pos(0, 0), true);
        assert_eq!(from, pos(0, 0));
        assert_eq!(to, pos(0, 3));
    }

    #[test]
    fn normal_keys_delete_linewise_and_words() {
        let mut vim = VimState::new();
        let buf = buffer(&["foo bar"]);
        vim.handle_key("d", &buf).unwrap();
        let commands = vim.handle_key("w", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Delete {
                from: pos(0, 0),
                to: pos(0, 4),
                linewise: false,
                insert: false,
            }]
        );
        vim.commit_change();

        let mut vim = VimState::new();
        let buf = buffer(&["foo", "bar"]);
        vim.handle_key("d", &buf).unwrap();
        let commands = vim.handle_key("d", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Delete {
                from: pos(0, 0),
                to: pos(0, 3),
                linewise: true,
                insert: false,
            }]
        );
    }

    #[test]
    fn x_deletes_one_grapheme_and_dot_repeats() {
        let mut vim = VimState::new();
        let buf = buffer(&["abc"]);
        let commands = vim.handle_key("x", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Delete {
                from: pos(0, 0),
                to: pos(0, 1),
                linewise: false,
                insert: false,
            }]
        );
        vim.commit_change();

        let buf = buffer(&["bc"]);
        let commands = vim.handle_key(".", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Delete {
                from: pos(0, 0),
                to: pos(0, 1),
                linewise: false,
                insert: false,
            }]
        );
    }

    #[test]
    fn replace_key_overwrites_count_graphemes() {
        let mut vim = VimState::new();
        let buf = buffer(&["abc"]);
        vim.handle_key("r", &buf).unwrap();
        let commands = vim.handle_key("x", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Replace {
                from: pos(0, 0),
                to: pos(0, 1),
                text: "x".to_string(),
            }]
        );

        let mut vim = VimState::new();
        vim.handle_key("2", &buf).unwrap();
        vim.handle_key("r", &buf).unwrap();
        let commands = vim.handle_key("x", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Replace {
                from: pos(0, 0),
                to: pos(0, 2),
                text: "xx".to_string(),
            }]
        );
    }

    #[test]
    fn escape_cancels_a_pending_operator() {
        let mut vim = VimState::new();
        let buf = buffer(&["abc"]);
        assert_eq!(vim.handle_key("d", &buf).unwrap(), Vec::<VimCommand>::new());
        assert!(vim.pending());
        assert_eq!(vim.pending_text(), "d");
        assert_eq!(vim.handle_key("escape", &buf).unwrap(), Vec::<VimCommand>::new());
        assert!(!vim.pending());
        assert_eq!(vim.pending_text(), "");
    }

    #[test]
    fn pending_text_echoes_prefixes() {
        let mut vim = VimState::new();
        let buf = buffer(&["abc"]);
        vim.handle_key("2", &buf).unwrap();
        vim.handle_key("d", &buf).unwrap();
        assert_eq!(vim.pending_text(), "2d");
        vim.handle_key("f", &buf).unwrap();
        assert_eq!(vim.pending_text(), "2df");
        vim.handle_key("escape", &buf).unwrap();
        vim.handle_key("r", &buf).unwrap();
        assert_eq!(vim.pending_text(), "r");
    }

    #[test]
    fn insert_enters_mode_and_records_text() {
        let mut vim = VimState::new();
        let buf = buffer(&["abc"]);
        let commands = vim.handle_key("i", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Mode {
                mode: VimMode::Insert
            }]
        );
        assert_eq!(vim.mode, VimMode::Insert);
        vim.record_insert(
            VimInsertRecord {
                text: "hi".to_string(),
                ..VimInsertRecord::default()
            },
            false,
        );
        assert!(matches!(
            &vim.last_change,
            Some(VimChange::Keys { insert, .. })
                if insert.as_ref().is_some_and(|record| record.text == "hi")
        ));
    }

    #[test]
    fn dollar_then_j_keeps_end_of_line() {
        let mut vim = VimState::new();
        let buf = VimBuffer::new(vec!["abc".to_string(), "de".to_string()], 0, 0);
        let commands = vim.handle_key("$", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Move {
                to: VimPosition { line: 0, col: 2 }
            }]
        );
        vim.handle_key("j", &buf).unwrap();
        assert_eq!(
            vim.handle_key("k", &buf).unwrap(),
            vec![VimCommand::Move {
                to: VimPosition { line: 0, col: 2 }
            }]
        );
    }

    #[test]
    fn visual_delete_covers_selection() {
        let mut vim = VimState::new();
        let mut buf = VimBuffer::new(vec!["abc".to_string()], 0, 0);
        vim.handle_key("v", &buf).unwrap();
        let commands = vim.handle_key("l", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Move {
                to: VimPosition { line: 0, col: 1 }
            }]
        );
        buf.cursor_col = 1;
        let commands = vim.handle_key("d", &buf).unwrap();
        assert_eq!(
            commands,
            vec![
                VimCommand::Delete {
                    from: pos(0, 0),
                    to: pos(0, 2),
                    linewise: false,
                    insert: false,
                },
                VimCommand::Mode {
                    mode: VimMode::Normal
                },
            ]
        );
    }

    #[test]
    fn percent_matches_bracket_pair() {
        let mut vim = VimState::new();
        let buf = buffer(&["(abc)"]);
        let commands = vim.handle_key("%", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Move {
                to: VimPosition { line: 0, col: 4 }
            }]
        );
    }

    #[test]
    fn find_and_till_seek_line_locally() {
        let mut vim = VimState::new();
        let buf = buffer(&["aXbXc"]);
        vim.handle_key("f", &buf).unwrap();
        let commands = vim.handle_key("X", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Move {
                to: VimPosition { line: 0, col: 1 }
            }]
        );
        let buf = VimBuffer::new(vec!["aXbXc".to_string()], 0, 1);
        let commands = vim.handle_key(";", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Move {
                to: VimPosition { line: 0, col: 3 }
            }]
        );
        let buf = VimBuffer::new(vec!["aXbXc".to_string()], 0, 3);
        let commands = vim.handle_key("l", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Move {
                to: VimPosition { line: 0, col: 4 }
            }]
        );
    }

    #[test]
    fn text_objects_delete_inside_word() {
        let mut vim = VimState::new();
        let buf = buffer(&["foo bar"]);
        vim.handle_key("d", &buf).unwrap();
        vim.handle_key("i", &buf).unwrap();
        let commands = vim.handle_key("w", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Delete {
                from: pos(0, 0),
                to: pos(0, 3),
                linewise: false,
                insert: false,
            }]
        );
    }

    #[test]
    fn indent_operator_applies_to_range() {
        let mut vim = VimState::new();
        let buf = buffer(&["a", "b"]);
        vim.handle_key(">", &buf).unwrap();
        let commands = vim.handle_key("j", &buf).unwrap();
        assert_eq!(
            commands,
            vec![VimCommand::Indent {
                from_line: 0,
                to_line: 1,
                out: false,
            }]
        );
    }
}
