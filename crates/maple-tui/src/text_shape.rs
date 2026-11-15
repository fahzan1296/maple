//! Visible measurement and ANSI-preserving text shaping.

use crate::ansi_scan::{scan, strip_ansi, Token};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Replacement appended when truncating.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ellipsis {
    /// Cut with no marker.
    Omit,
    /// Single `…` character (default).
    #[default]
    Unicode,
    /// Three dots `...`.
    Ascii,
}

impl Ellipsis {
    fn glyph(self) -> &'static str {
        match self {
            Ellipsis::Omit => "",
            Ellipsis::Unicode => "…",
            Ellipsis::Ascii => "...",
        }
    }
}

fn grapheme_width(grapheme: &str) -> usize {
    UnicodeWidthStr::width(grapheme)
}

/// Display width ignoring ANSI escape sequences.
pub fn visible_width(input: &str) -> usize {
    let mut total = 0;
    for token in scan(input) {
        if let Token::Text(text) = token {
            for grapheme in text.graphemes(true) {
                total += grapheme_width(grapheme);
            }
        }
    }
    total
}

/// Plain-text width (escapes removed first).
pub fn plain_width(input: &str) -> usize {
    visible_width(&strip_ansi(input))
}

/// Cut `input` to `width` display columns, preserving escape sequences and
/// appending `ellipsis` when content was dropped.
pub fn truncate_to_width(input: &str, width: usize, ellipsis: Ellipsis) -> String {
    if width == 0 || visible_width(input) <= width {
        return input.to_string();
    }
    let marker = ellipsis.glyph();
    let marker_width = grapheme_width(marker);
    let budget = width.saturating_sub(marker_width);
    let mut out = String::with_capacity(input.len());
    let mut used = 0usize;
    let mut truncated = false;
    let mut styled = false;
    'tokens: for token in scan(input) {
        match token {
            Token::Escape(seq) => {
                if seq.ends_with('m') {
                    styled = true;
                }
                out.push_str(seq);
            }
            Token::Text(text) => {
                for grapheme in text.graphemes(true) {
                    let grapheme_width = grapheme_width(grapheme);
                    if used + grapheme_width > budget {
                        truncated = true;
                        break 'tokens;
                    }
                    out.push_str(grapheme);
                    used += grapheme_width;
                }
            }
        }
    }
    if truncated && marker_width > 0 && width >= marker_width {
        out.push_str(marker);
    }
    if styled {
        out.push_str("\x1b[0m");
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Atom {
    text: String,
    width: usize,
    whitespace: bool,
}

fn push_atom(atoms: &mut Vec<Atom>, pending: &mut String, text: String, width: usize, whitespace: bool) {
    if text.is_empty() && pending.is_empty() {
        return;
    }
    let mut full = std::mem::take(pending);
    full.push_str(&text);
    atoms.push(Atom {
        text: full,
        width,
        whitespace,
    });
}

fn hard_break(atom: &Atom, width: usize, out: &mut Vec<String>) {
    let mut chunk = String::new();
    let mut chunk_width = 0usize;
    let mut styled = false;
    for token in scan(&atom.text) {
        match token {
            Token::Escape(seq) => {
                if seq.ends_with('m') {
                    styled = true;
                }
                chunk.push_str(seq);
            }
            Token::Text(text) => {
                for grapheme in text.graphemes(true) {
                    let grapheme_width = grapheme_width(grapheme);
                    if chunk_width + grapheme_width > width && chunk_width > 0 {
                        out.push(std::mem::take(&mut chunk));
                        chunk_width = 0;
                    }
                    chunk.push_str(grapheme);
                    chunk_width += grapheme_width;
                }
            }
        }
    }
    if !chunk.is_empty() {
        if styled {
            chunk.push_str("\x1b[0m");
        }
        out.push(chunk);
    }
}

/// Word-wrap `input` to `width` columns, preserving ANSI styling, trimming
/// trailing whitespace, and honoring embedded newlines.
pub fn wrap_text_with_ansi(input: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    for physical in input.split('\n') {
        wrap_line(physical, width, &mut lines);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn wrap_line(line: &str, width: usize, out: &mut Vec<String>) {
    let mut atoms: Vec<Atom> = Vec::new();
    let mut pending = String::new();
    let mut pending_style = String::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    let mut in_word = false;

    fn flush_word(
        atoms: &mut Vec<Atom>,
        pending: &mut String,
        current: &mut String,
        current_width: &mut usize,
        in_word: &mut bool,
    ) {
        if *in_word {
            push_atom(atoms, pending, std::mem::take(current), *current_width, false);
            *current_width = 0;
            *in_word = false;
        }
    }

    for token in scan(line) {
        match token {
            Token::Escape(seq) => {
                if is_reset(seq) {
                    pending_style.clear();
                } else if seq.ends_with('m') {
                    pending_style.push_str(seq);
                }
                if in_word {
                    current.push_str(seq);
                } else {
                    pending.push_str(seq);
                }
            }
            Token::Text(text) => {
                for grapheme in text.graphemes(true) {
                    if grapheme == " " || grapheme == "\t" {
                        flush_word(&mut atoms, &mut pending, &mut current, &mut current_width, &mut in_word);
                        push_atom(&mut atoms, &mut pending, " ".into(), 1, true);
                    } else {
                        let grapheme_width = grapheme_width(grapheme);
                        if !in_word {
                            in_word = true;
                        }
                        current.push_str(grapheme);
                        current_width += grapheme_width;
                    }
                }
            }
        }
    }
    flush_word(&mut atoms, &mut pending, &mut current, &mut current_width, &mut in_word);

    let mut rendered = String::new();
    let mut rendered_width = 0usize;
    let mut pending_space = false;
    for atom in atoms {
        if atom.whitespace {
            pending_space = rendered_width > 0;
            continue;
        }
        if atom.width > width {
            if rendered_width > 0 {
                out.push(std::mem::take(&mut rendered));
                rendered_width = 0;
                pending_space = false;
            }
            hard_break(&atom, width, out);
            continue;
        }
        let extra = if pending_space { 1 } else { 0 };
        if rendered_width + extra + atom.width > width && rendered_width > 0 {
            out.push(std::mem::take(&mut rendered));
            rendered_width = 0;
            pending_space = false;
        }
        if rendered_width == 0 && rendered.is_empty() {
            rendered.push_str(&pending_style);
        }
        if pending_space {
            rendered.push(' ');
            rendered_width += 1;
            pending_space = false;
        }
        rendered.push_str(&atom.text);
        rendered_width += atom.width;
    }
    if !rendered.is_empty() || out.is_empty() {
        out.push(rendered);
    }
}

fn is_reset(sequence: &str) -> bool {
    let Some(params) = sequence.strip_prefix("\x1b[").and_then(|rest| rest.strip_suffix('m')) else {
        return false;
    };
    params.is_empty() || params.split(';').all(|part| part.is_empty() || part == "0")
}

/// Pad `input` with spaces to exactly `width` display columns.
pub fn pad_to_width(input: &str, width: usize) -> String {
    let current = visible_width(input);
    if current >= width {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len() + width - current);
    out.push_str(input);
    out.extend(std::iter::repeat_n(' ', width - current));
    out
}
