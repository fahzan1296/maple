//! Reference: `l/src/fuzzy.ts`.
//! Ports `l/src/fuzzy.ts`. Matching is deliberately word-local for normal words:
//! this keeps a query like "image provider" from matching a long setting
//! description only because the letters i-m-a-g-e appear somewhere in order
//! across unrelated words.
//!
//! Lower score = better match. A query is normalized, split on spaces into
//! tokens, and every token must match; the summed token scores produce the
//! ranking. The TypeScript original memoizes per-text search indexes in a
//! bounded LRU; this port rebuilds the index per call, which keeps the module
//! free of interior mutability. Callers filtering a stable candidate list
//! repeatedly can keep a [`SearchIndex`] alive and pass it to
//! [`fuzzy_match_index`].

use std::cmp::Ordering;

/// Penalty added when a token only matches through an alphanumeric-swap
/// variant (`a1` matching `1a`).
const ALPHANUMERIC_SWAP_PENALTY: f64 = 5.0;
/// Bonus for matching the whole normalized query as a whole-word phrase.
const COMPACT_PHRASE_BONUS: f64 = 1200.0;
/// Bonus for matching the whole normalized query as a phrase.
const PHRASE_BONUS: f64 = 1000.0;

/// Shortest needle worth scanning past its leading occurrence for.
///
/// One or two characters sit mid-word in nearly every candidate — "im" is
/// inside "experimental" — so rescanning that short would hand the word-start
/// bonus to the whole corpus at once and reshuffle the list on the opening
/// keystrokes of a search instead of narrowing it. Shadowing only misleads for
/// real phrases.
const MIN_SHADOW_RESCAN_LENGTH: usize = 3;

/// Result of matching one query against one text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FuzzyMatch {
    /// Whether the query matched.
    pub matches: bool,
    /// Match quality; lower is better.
    pub score: f64,
}

impl FuzzyMatch {
    /// A successful match with `score`.
    pub fn matched(score: f64) -> Self {
        FuzzyMatch { matches: true, score }
    }

    /// A failed match.
    pub fn no_match() -> Self {
        FuzzyMatch {
            matches: false,
            score: 0.0,
        }
    }
}

/// A single candidate's rank: its original index and score.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RankedIndex {
    /// Position of the item in the input slice.
    pub index: usize,
    /// Match quality; lower is better.
    pub score: f64,
}

struct CharacterMatch {
    matches: bool,
    score: f64,
    span: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]

struct SearchWord {
    text: Vec<char>,
    /// Offset of the word within `SearchIndex::normalized`.
    index: usize,
    /// Zero-based word position.
    ordinal: usize,
}

/// Pre-split, pre-normalized form of a candidate text.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchIndex {
    /// Lowercased text with separators collapsed to single spaces.
    normalized: Vec<char>,
    /// `normalized` with spaces removed.
    compact: Vec<char>,
    /// Start offsets of each word within `compact` (cumulative word lengths).
    compact_word_starts: Vec<usize>,
    /// Words with their offsets and ordinals.
    words: Vec<SearchWord>,
}

impl SearchIndex {
    /// Normalize and index `text` for repeated matching.
    pub fn new(text: &str) -> Self {
        build_search_index(text)
    }


    /// The normalized text as chars (allocation-free view is not possible
    /// because the source is owned as `Vec<char>`).
    pub fn normalized_chars(&self) -> &[char] {
        &self.normalized
    }
}

/// Normalize `value` for matching: split camelCase, lowercase, and collapse
/// every run that is not a letter, mark, or number into a single space.
fn normalize_for_search(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut spaced = String::with_capacity(value.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if prev.is_ascii_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_ascii_uppercase() && next_is_lower)
            {
                spaced.push(' ');
            }
        }
        spaced.push(c);
    }

    let mut out = String::with_capacity(spaced.len());
    let mut pending_space = false;
    for c in spaced.to_lowercase().chars() {
        if is_word_char(c) {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(c);
        } else {
            pending_space = true;
        }
    }
    out
}

/// Whether `c` is a letter, mark, or number — the classes `\p{Letter}`,
/// `\p{Mark}`, and `\p{Number}` keep in the normalized form.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_mark(c)
}

/// Approximate `\p{Mark}`: the combining-mark block ranges used by scripts
/// that attach diacritics to a base letter. Rust's standard library does not
/// expose general categories.
fn is_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x05BF
        | 0x05C1..=0x05C2
        | 0x05C4..=0x05C5
        | 0x05C7
        | 0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x0670
        | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4
        | 0x06E7..=0x06E8
        | 0x06EA..=0x06ED
        | 0x0711
        | 0x0730..=0x074A
        | 0x07A6..=0x07B0
        | 0x07EB..=0x07F3
        | 0x0816..=0x0819
        | 0x081B..=0x0823
        | 0x0825..=0x0827
        | 0x0829..=0x082D
        | 0x0859..=0x085B
        | 0x08D3..=0x08E1
        | 0x08E3..=0x0903
        | 0x093A..=0x093C
        | 0x093E..=0x094F
        | 0x0951..=0x0957
        | 0x0962..=0x0963
        | 0x0981..=0x0983
        | 0x09BC
        | 0x09BE..=0x09C4
        | 0x09C7..=0x09C8
        | 0x09CB..=0x09CD
        | 0x09D7
        | 0x09E2..=0x09E3
        | 0x0A01..=0x0A03
        | 0x0A3C
        | 0x0A3E..=0x0A42
        | 0x0A47..=0x0A48
        | 0x0A4B..=0x0A4D
        | 0x0A51
        | 0x0A70..=0x0A71
        | 0x0A75
        | 0x0A81..=0x0A83
        | 0x0ABC
        | 0x0ABE..=0x0AC5
        | 0x0AC7..=0x0AC9
        | 0x0ACB..=0x0ACD
        | 0x0AE2..=0x0AE3
        | 0x0B01..=0x0B03
        | 0x0B3C
        | 0x0B3E..=0x0B44
        | 0x0B47..=0x0B48
        | 0x0B4B..=0x0B4D
        | 0x0B56..=0x0B57
        | 0x0B62..=0x0B63
        | 0x0B82
        | 0x0BBE..=0x0BC2
        | 0x0BC6..=0x0BC8
        | 0x0BCA..=0x0BCD
        | 0x0BD7
        | 0x0C00..=0x0C04
        | 0x0C3E..=0x0C44
        | 0x0C46..=0x0C48
        | 0x0C4A..=0x0C4D
        | 0x0C55..=0x0C56
        | 0x0C62..=0x0C63
        | 0x0C81..=0x0C83
        | 0x0CBC
        | 0x0CBE..=0x0CC4
        | 0x0CC6..=0x0CC8
        | 0x0CCA..=0x0CCD
        | 0x0CD5..=0x0CD6
        | 0x0CE2..=0x0CE3
        | 0x0D00..=0x0D03
        | 0x0D3B..=0x0D3C
        | 0x0D3E..=0x0D44
        | 0x0D46..=0x0D48
        | 0x0D4A..=0x0D4D
        | 0x0D57
        | 0x0D62..=0x0D63
        | 0x0D81..=0x0D83
        | 0x0DCA
        | 0x0DCF..=0x0DD4
        | 0x0DD6
        | 0x0DD8..=0x0DDF
        | 0x0DF2..=0x0DF3
        | 0x0E31
        | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E
        | 0x0EB1
        | 0x0EB4..=0x0EBC
        | 0x0EC8..=0x0ECD
        | 0x0F18..=0x0F19
        | 0x0F35
        | 0x0F37
        | 0x0F39
        | 0x0F3E..=0x0F3F
        | 0x0F71..=0x0F84
        | 0x0F86..=0x0F87
        | 0x0F8D..=0x0F97
        | 0x0F99..=0x0FBC
        | 0x0FC6
        | 0x102B..=0x103E
        | 0x1056..=0x1059
        | 0x105E..=0x1060
        | 0x1062..=0x1064
        | 0x1067..=0x106D
        | 0x1071..=0x1074
        | 0x1082..=0x108D
        | 0x108F
        | 0x109A..=0x109D
        | 0x135D..=0x135F
        | 0x1712..=0x1714
        | 0x1732..=0x1734
        | 0x1752..=0x1753
        | 0x1772..=0x1773
        | 0x17B4..=0x17D3
        | 0x17DD
        | 0x180B..=0x180D
        | 0x1885..=0x1886
        | 0x18A9
        | 0x1920..=0x192B
        | 0x1930..=0x193B
        | 0x1A17..=0x1A1B
        | 0x1A55..=0x1A5E
        | 0x1A60..=0x1A7C
        | 0x1A7F
        | 0x1AB0..=0x1AFF
        | 0x1B00..=0x1B04
        | 0x1B34..=0x1B44
        | 0x1B6B..=0x1B73
        | 0x1B80..=0x1B82
        | 0x1BA1..=0x1BAD
        | 0x1BE6..=0x1BF3
        | 0x1C24..=0x1C37
        | 0x1CD0..=0x1CD2
        | 0x1CD4..=0x1CE8
        | 0x1CED
        | 0x1CF4
        | 0x1CF7..=0x1CF9
        | 0x1DC0..=0x1DF9
        | 0x1DFB..=0x1DFF
        | 0x20D0..=0x20F0
        | 0x2CEF..=0x2CF1
        | 0x2D7F
        | 0x2DE0..=0x2DFF
        | 0x302A..=0x302F
        | 0x3099..=0x309A
        | 0xA66F..=0xA672
        | 0xA674..=0xA67D
        | 0xA69E..=0xA69F
        | 0xA6F0..=0xA6F1
        | 0xA802
        | 0xA806
        | 0xA80B
        | 0xA823..=0xA827
        | 0xA880..=0xA881
        | 0xA8B4..=0xA8C5
        | 0xA8E0..=0xA8F1
        | 0xA926..=0xA92D
        | 0xA947..=0xA953
        | 0xA980..=0xA983
        | 0xA9B3..=0xA9C0
        | 0xA9E5
        | 0xAA29..=0xAA36
        | 0xAA43
        | 0xAA4C..=0xAA4D
        | 0xAA7B..=0xAA7D
        | 0xAAB0
        | 0xAAB2..=0xAAB4
        | 0xAAB7..=0xAAB8
        | 0xAABE..=0xAABF
        | 0xAAC1
        | 0xAAEB..=0xAAEF
        | 0xAAF5..=0xAAF6
        | 0xABE3..=0xABEA
        | 0xABEC..=0xABED
        | 0xFB1E
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F
    )
}

fn build_search_index(text: &str) -> SearchIndex {
    let normalized: Vec<char> = normalize_for_search(text).chars().collect();
    if normalized.is_empty() {
        return SearchIndex {
            normalized,
            compact: Vec::new(),
            compact_word_starts: Vec::new(),
            words: Vec::new(),
        };
    }

    let mut words = Vec::new();
    let mut compact_word_starts = Vec::new();
    let mut compact = Vec::with_capacity(normalized.len());
    let mut index = 0usize;
    let mut compact_index = 0usize;
    let mut ordinal = 0usize;
    for word in normalized.split(|&c| c == ' ') {
        words.push(SearchWord {
            text: word.to_vec(),
            index,
            ordinal,
        });
        compact_word_starts.push(compact_index);
        index += word.len() + 1;
        compact_index += word.len();
        compact.extend_from_slice(word);
        ordinal += 1;
    }

    SearchIndex {
        normalized,
        compact,
        compact_word_starts,
        words,
    }
}

/// Score a query as a subsequence of a single word, rewarding consecutive and
/// early matches. `query_lower` and `text_lower` must already be lowercased.
fn score_characters(query_lower: &[char], text_lower: &[char]) -> CharacterMatch {
    if query_lower.is_empty() {
        return CharacterMatch {
            matches: true,
            score: 0.0,
            span: 0,
        };
    }
    if query_lower.len() > text_lower.len() {
        return CharacterMatch {
            matches: false,
            score: 0.0,
            span: 0,
        };
    }

    let mut query_index = 0usize;
    let mut score = 0.0f64;
    let mut first_match_index: Option<usize> = None;
    // Mirrors the TypeScript original, whose `lastMatchIndex` starts at -1:
    // a first match at index 0 therefore counts as consecutive.
    let mut last_match_index: isize = -1;
    let mut consecutive_matches = 0usize;

    for (i, &text_char) in text_lower.iter().enumerate() {
        if query_index >= query_lower.len() {
            break;
        }
        if text_char != query_lower[query_index] {
            continue;
        }
        if first_match_index.is_none() {
            first_match_index = Some(i);
        }
        if last_match_index == i as isize - 1 {
            consecutive_matches += 1;
            score -= consecutive_matches as f64 * 5.0;
        } else {
            consecutive_matches = 0;
            if last_match_index >= 0 {
                score += (i as isize - last_match_index - 1) as f64 * 2.0;
            }
        }
        score += i as f64 * 0.1;
        last_match_index = i as isize;
        query_index += 1;
    }

    if query_index < query_lower.len() {
        return CharacterMatch {
            matches: false,
            score: 0.0,
            span: 0,
        };
    }

    let first = first_match_index.unwrap_or(0);

    let last = last_match_index.max(0) as usize;
    CharacterMatch {
        matches: true,
        score,
        span: last - first + 1,
    }
}

/// Swap-variant queries for alphanumeric transpositions (`a1` → `1a`).
fn build_alphanumeric_swap_queries(query_lower: &[char]) -> Vec<Vec<char>> {
    let mut variants: Vec<Vec<char>> = Vec::new();
    if query_lower.len() < 2 {
        return variants;
    }
    for i in 0..query_lower.len() - 1 {
        let current = query_lower[i];
        let next = query_lower[i + 1];
        let is_alphanumeric_swap = (current.is_ascii_lowercase() && next.is_ascii_digit())
            || (current.is_ascii_digit() && next.is_ascii_lowercase());
        if !is_alphanumeric_swap {
            continue;
        }
        let mut swapped = Vec::with_capacity(query_lower.len());
        swapped.extend_from_slice(&query_lower[..i]);
        swapped.push(next);
        swapped.push(current);
        swapped.extend_from_slice(&query_lower[i + 2..]);
        if !variants.iter().any(|variant| *variant == swapped) {
            variants.push(swapped);
        }
    }
    variants
}

fn with_position(score: f64, index: usize) -> f64 {
    score + index as f64 * 0.01
}

fn is_word_boundary_phrase(normalized: &[char], index: usize, length: usize) -> bool {
    let before = index == 0 || normalized[index - 1] == ' ';
    let after_index = index + length;
    let after = after_index == normalized.len() || normalized[after_index] == ' ';
    before && after
}

/// Offset of the first whole-word occurrence of `phrase` in `normalized`.
///
/// A bare substring search reports only the leading occurrence, so a
/// qualifying match is dropped whenever an earlier non-qualifying one shadows
/// it — the whole word "image" in "reimage image provider" loses to the
/// "image" inside "reimage". Occurrences are scanned left to right, so the
/// first qualifying hit is also the best-scoring one.
fn find_word_boundary_phrase(normalized: &[char], phrase: &[char]) -> Option<usize> {
    if phrase.is_empty() {
        return None;
    }
    let first = find_subslice(normalized, phrase)?;
    if is_word_boundary_phrase(normalized, first, phrase.len()) {
        return Some(first);
    }
    if first == 0 || normalized[first - 1] == ' ' {
        return None;
    }
    if phrase.len() < MIN_SHADOW_RESCAN_LENGTH {
        return None;
    }
    let mut at = first + 1;
    while let Some(found) = find_subslice(&normalized[at..], phrase) {
        let offset = at + found;
        if is_word_boundary_phrase(normalized, offset, phrase.len()) {
            return Some(offset);
        }
        at = offset + 1;
    }
    None
}

/// Offset of the first occurrence of `needle` that starts a word in
/// `index.compact`.
fn find_compact_word_start(index: &SearchIndex, needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let first = find_subslice(&index.compact, needle)?;
    if index.compact_word_starts.binary_search(&first).is_ok() {
        return Some(first);
    }
    if needle.len() < MIN_SHADOW_RESCAN_LENGTH {
        return None;
    }
    let mut at = first + 1;
    while let Some(found) = find_subslice(&index.compact[at..], needle) {
        let offset = at + found;
        if index.compact_word_starts.binary_search(&offset).is_ok() {
            return Some(offset);
        }
        at = offset + 1;
    }
    None
}

/// Find `needle` in `haystack`, returning the byte-free char offset.
fn find_subslice(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| haystack[i..i + needle.len()] == *needle)
}

fn score_token_against_word(token: &[char], word: &SearchWord) -> Option<FuzzyMatch> {
    if word.text == token {
        return Some(FuzzyMatch::matched(with_position(-200.0, word.index)));
    }
    if word.text.len() >= token.len() && word.text.starts_with(token) {
        let penalty = (word.text.len() - token.len()) as f64 * 0.5;
        return Some(FuzzyMatch::matched(with_position(-170.0 + penalty, word.index)));
    }
    if token.len() > word.text.len()
        && token.starts_with(&word.text)
        && token.len() - word.text.len() <= 2
    {
        return Some(FuzzyMatch::matched(
            with_position(-150.0 + (token.len() - word.text.len()) as f64, word.index),
        ));
    }
    if let Some(substring_index) = find_subslice(&word.text, token) {
        return Some(FuzzyMatch::matched(with_position(
            -20.0 + substring_index as f64,
            word.index,
        )));
    }

    let character_match = score_characters(token, &word.text);
    if !character_match.matches {
        return None;
    }
    let max_span = (token.len() + 2).max((token.len() as f64 * 1.8).ceil() as usize);
    if character_match.span > max_span {
        return None;
    }
    Some(FuzzyMatch::matched(with_position(
        -40.0 + character_match.score,
        word.index,
    )))
}

fn score_acronym(token: &[char], index: &SearchIndex) -> Option<FuzzyMatch> {
    if token.len() < 2 || token.len() > 4 || index.words.is_empty() {
        return None;
    }

    let mut query_index = 0usize;
    let mut first_ordinal: Option<usize> = None;
    let mut last_ordinal: Option<usize> = None;
    let mut first_text_index = 0usize;

    for word in &index.words {
        if word.text.first() != token.get(query_index) {
            continue;
        }
        if first_ordinal.is_none() {
            first_ordinal = Some(word.ordinal);
            first_text_index = word.index;
        }
        last_ordinal = Some(word.ordinal);
        query_index += 1;
        if query_index == token.len() {
            break;
        }
    }

    let first_ordinal = first_ordinal?;
    let last_ordinal = last_ordinal?;
    if query_index < token.len() {
        return None;
    }

    let word_span = last_ordinal - first_ordinal + 1;
    if word_span > token.len() + 2 {
        return None;
    }
    Some(FuzzyMatch::matched(with_position(
        -30.0 + word_span as f64 * 4.0 - token.len() as f64 * 2.0,
        first_text_index,
    )))
}

fn score_token_direct(token: &[char], index: &SearchIndex) -> FuzzyMatch {
    if token.is_empty() {
        return FuzzyMatch::matched(0.0);
    }

    let mut best: Option<FuzzyMatch> = None;
    if let Some(compact_index) = find_compact_word_start(index, token) {
        best = Some(FuzzyMatch::matched(with_position(-140.0, compact_index)));
    }
    for word in &index.words {
        if let Some(match_) = score_token_against_word(token, word) {
            if best.is_none_or(|current| match_.score < current.score) {
                best = Some(match_);
            }
        }
    }
    if let Some(acronym) = score_acronym(token, index) {
        if best.is_none_or(|current| acronym.score < current.score) {
            best = Some(acronym);
        }
    }

    best.unwrap_or_else(FuzzyMatch::no_match)
}

fn score_token(token: &[char], swap_variants: &[Vec<char>], index: &SearchIndex) -> FuzzyMatch {
    let mut best = score_token_direct(token, index);
    if best.matches {
        return best;
    }
    for variant in swap_variants {
        let match_ = score_token_direct(variant, index);
        if !match_.matches {
            continue;
        }
        let score = match_.score + ALPHANUMERIC_SWAP_PENALTY;
        if !best.matches || score < best.score {
            best = FuzzyMatch::matched(score);
        }
    }
    best
}

/// A query normalized and split once, so ranking does not re-normalize the
/// same query for every candidate.
struct PreparedQuery {
    normalized: Vec<char>,
    tokens: Vec<Vec<char>>,
    /// `build_alphanumeric_swap_queries` of each token, index-aligned.
    token_swaps: Vec<Vec<Vec<char>>>,
    compact: Vec<char>,
}

fn prepare_query(query: &str) -> Option<PreparedQuery> {
    let normalized: Vec<char> = normalize_for_search(query).chars().collect();
    if normalized.is_empty() {
        return None;
    }
    let tokens: Vec<Vec<char>> = normalized
        .split(|&c| c == ' ')
        .map(|token| token.to_vec())
        .collect();
    let token_swaps = tokens
        .iter()
        .map(|token| build_alphanumeric_swap_queries(token))
        .collect();
    let compact: Vec<char> = normalized.iter().copied().filter(|&c| c != ' ').collect();
    Some(PreparedQuery {
        normalized,
        tokens,
        token_swaps,
        compact,
    })
}

fn fuzzy_match_core(prepared: Option<&PreparedQuery>, index: &SearchIndex) -> FuzzyMatch {
    let Some(prepared) = prepared else {
        return FuzzyMatch::matched(0.0);
    };
    if index.words.is_empty() {
        return FuzzyMatch::no_match();
    }

    let mut total_score = 0.0;
    if let Some(phrase_index) = find_word_boundary_phrase(&index.normalized, &prepared.normalized) {
        total_score -= PHRASE_BONUS;
        total_score += phrase_index as f64 * 0.01;
    }
    if let Some(compact_phrase_index) = find_compact_word_start(index, &prepared.compact) {
        total_score -= COMPACT_PHRASE_BONUS;
        total_score += compact_phrase_index as f64 * 0.01;
    }

    for (token, swaps) in prepared.tokens.iter().zip(prepared.token_swaps.iter()) {
        let match_ = score_token(token, swaps, index);
        if !match_.matches {
            return FuzzyMatch::no_match();
        }
        total_score += match_.score;
    }

    FuzzyMatch::matched(total_score)
}

/// Match `query` against `text` (space-separated tokens; all must match).
pub fn fuzzy_match(query: &str, text: &str) -> FuzzyMatch {
    let prepared = prepare_query(query);
    if prepared.is_none() {
        return FuzzyMatch::matched(0.0);
    }
    let index = build_search_index(text);
    fuzzy_match_core(prepared.as_ref(), &index)
}

/// Match `query` against a prebuilt [`SearchIndex`], skipping normalization.
pub fn fuzzy_match_index(query: &str, index: &SearchIndex) -> FuzzyMatch {
    let prepared = prepare_query(query);
    fuzzy_match_core(prepared.as_ref(), index)
}

/// Score `needle` against `haystack`: `Some(score)` when it matches, `None`
/// otherwise. An empty needle matches everything with score `0.0`.
pub fn fuzzy_score(needle: &str, haystack: &str) -> Option<f64> {
    fuzzy_match(needle, haystack).into_score()
}

/// Filter and rank `items` by fuzzy match quality, returning original indices
/// best-first. An empty or whitespace-only needle keeps the input order.
pub fn fuzzy_filter<T: AsRef<str>>(needle: &str, items: &[T]) -> Vec<usize> {
    fuzzy_rank(items, needle, |item| item.as_ref())
        .into_iter()
        .map(|ranked| ranked.index)
        .collect()
}

/// Rank `items` by fuzzy match quality, returning original indices and scores
/// best-first. `get_text` extracts the candidate text from each item.
pub fn fuzzy_rank<T>(
    items: &[T],
    query: &str,
    get_text: impl Fn(&T) -> &str,
) -> Vec<RankedIndex> {
    if query.trim().is_empty() {
        return (0..items.len())
            .map(|index| RankedIndex { index, score: 0.0 })
            .collect();
    }

    let prepared = prepare_query(query);
    let mut results: Vec<RankedIndex> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let search_index = build_search_index(get_text(item));
        let match_ = fuzzy_match_core(prepared.as_ref(), &search_index);
        if match_.matches {
            results.push(RankedIndex {
                index,
                score: match_.score,
            });
        }
    }
    results.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(Ordering::Equal)
    });
    results
}

impl FuzzyMatch {
    /// Score when matched, `None` otherwise.
    pub fn into_score(self) -> Option<f64> {
        if self.matches {
            Some(self.score)
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_needle_matches_everything_at_zero() {
        assert_eq!(fuzzy_match("", "anything"), FuzzyMatch::matched(0.0));
        assert_eq!(fuzzy_score("", ""), Some(0.0));
    }

    #[test]
    fn exact_word_scores_minus_2400() {
        // phrase bonus (-1000) + compact phrase bonus (-1200) + exact word (-200)
        assert_eq!(fuzzy_score("image", "image"), Some(-2400.0));
    }

    #[test]
    fn subsequence_match_rewards_tight_spans() {
        // Reference scores from the TypeScript original.
        assert_eq!(fuzzy_score("img", "image"), Some(-52.6));
        assert_eq!(fuzzy_score("ac", "abc"), Some(-42.8));
    }

    #[test]
    fn consecutive_matches_earn_bonus() {
        assert_eq!(fuzzy_score("abcd", "abxcd"), Some(-57.2));
    }
    #[test]
    fn longer_query_than_text_does_not_match() {
        assert_eq!(fuzzy_match("abcd", "aXbXc"), FuzzyMatch::no_match());
        assert_eq!(fuzzy_score("abcd", "aXbXc"), None);
    }

    #[test]
    fn case_handling_is_case_insensitive() {
        let lower = fuzzy_score("image", "Image Provider");
        let upper = fuzzy_score("IMAGE", "image provider");
        assert_eq!(lower, upper);
    }

    #[test]
    fn camel_case_is_split_into_words() {
        // "HTTPServer" normalizes to "http server", so "http" is a whole word.
        assert!(fuzzy_score("http", "HTTPServer").is_some());
        assert!(fuzzy_score("server", "HTTPServer").is_some());
    }

    #[test]
    fn alphanumeric_swap_variant_applies_penalty() {
        // Direct match fails; the "1a" swap variant matches with +5.
        assert_eq!(fuzzy_score("a1", "1a x"), Some(-195.0));
    }

    #[test]
    fn whole_word_phrase_outranks_buried_occurrence() {
        let direct = fuzzy_score("image", "image provider").unwrap();
        let buried = fuzzy_score("image", "reimage image provider").unwrap();
        assert!(direct < buried, "direct {direct} should beat buried {buried}");
    }

    #[test]
    fn every_token_must_match() {
        assert!(fuzzy_score("image provider", "image provider").is_some());
        assert!(fuzzy_score("image provider", "image only").is_none());
    }

    #[test]
    fn punctuation_only_query_matches_everything() {
        assert_eq!(fuzzy_match("!!", "anything"), FuzzyMatch::matched(0.0));
        let ranked = fuzzy_rank(&["b", "a", "c"], "!!", |item| *item);
        assert_eq!(
            ranked,
            vec![
                RankedIndex { index: 0, score: 0.0 },
                RankedIndex { index: 1, score: 0.0 },
                RankedIndex { index: 2, score: 0.0 },
            ]
        );
    }

    #[test]
    fn filter_orders_best_first_and_drops_misses() {
        let items = ["image provider", "reimage image provider", "unrelated"];
        assert_eq!(fuzzy_filter("image", &items), vec![0, 1]);
    }

    #[test]
    fn empty_needle_keeps_input_order() {
        let items = ["b", "a", "c"];
        assert_eq!(fuzzy_filter("   ", &items), vec![0, 1, 2]);
    }

    #[test]
    fn ranking_is_deterministic() {
        let items = ["alpha", "alpaca", "beta", "alphabet soup"];
        let first = fuzzy_filter("alp", &items);
        let second = fuzzy_filter("alp", &items);
        assert_eq!(first, second);
        assert_eq!(first, vec![0, 1, 3]);
    }

    #[test]
    fn prepared_index_matches_fresh_match() {
        let index = SearchIndex::new("image provider");
        assert_eq!(
            fuzzy_match_index("image", &index),
            fuzzy_match("image", "image provider")
        );
    }

    #[test]
    fn acronym_matches_word_initials() {
        let score = fuzzy_score("ip", "image provider").unwrap();
        assert!(score < 0.0, "acronym should score negatively, got {score}");
    }
}
