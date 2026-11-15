//! Byte-level ANSI escape scanning shared by width and shaping routines.
//!
//! Recognizes CSI (`ESC [`), OSC (`ESC ]`), DCS/PM/APC (`ESC P/X/^/_`) and
//! two-character escapes. Strings that are not valid sequences are treated as
//! literal text so measurement never panics on hostile input.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    Text(&'a str),
    Escape(&'a str),
}

/// Length in bytes of the escape sequence starting at `start` (which must point
/// at `ESC`).
fn escape_len(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    if start + 1 >= bytes.len() {
        return bytes.len() - start;
    }
    match bytes[start + 1] {
        b'[' => {
            for (offset, byte) in bytes.iter().enumerate().skip(start + 2) {
                if (0x40..=0x7e).contains(byte) {
                    return offset + 1 - start;
                }
            }
            bytes.len() - start
        }
        b']' | b'P' | b'X' | b'^' | b'_' => {
            let mut index = start + 2;
            while index < bytes.len() {
                if bytes[index] == 0x07 {
                    return index + 1 - start;
                }
                if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
                    return index + 2 - start;
                }
                index += 1;
            }
            bytes.len() - start
        }
        _ => match input[start + 1..].chars().next() {
            Some(ch) => 1 + ch.len_utf8(),
            None => 1,
        },
    }
}

/// Split `input` into alternating text and escape tokens.
pub(crate) fn scan(input: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] == 0x1b {
            let len = escape_len(input, pos);
            tokens.push(Token::Escape(&input[pos..pos + len]));
            pos += len;
        } else {
            let end = input[pos..]
                .find('\x1b')
                .map(|offset| pos + offset)
                .unwrap_or(bytes.len());
            tokens.push(Token::Text(&input[pos..end]));
            pos = end;
        }
    }
    tokens
}

/// Drop every escape sequence, leaving printable characters only.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for token in scan(input) {
        if let Token::Text(text) = token {
            out.push_str(text);
        }
    }
    out
}
