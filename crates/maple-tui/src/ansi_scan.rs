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

/// Most parameter tokens one emitted CSI may carry before it is split.
const MAX_SGR_PARAMS: usize = 16;

/// The parameter text of an SGR sequence (`ESC [ … m`), when `seq` is one.
fn sgr_params(seq: &str) -> Option<&str> {
    let inner = seq.strip_prefix("\x1b[")?.strip_suffix('m')?;
    let plain = inner
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b';' || byte == b':');
    plain.then_some(inner)
}

/// Parameter tokens in `param` (`;` and `:` both separate).
fn param_tokens(param: &str) -> usize {
    param.bytes().filter(|byte| *byte == b';' || *byte == b':').count() + 1
}

/// Whether every extended-colour introducer (`38`/`48`/`58`) carries all channels.
fn sgr_complete(params: &str) -> bool {
    let parts: Vec<&str> = params.split(';').collect();
    let mut index = 0;
    while index < parts.len() {
        if matches!(parts[index], "38" | "48" | "58") {
            let Some(mode) = parts.get(index + 1) else {
                return false;
            };
            let channels = match *mode {
                "5" => 1,
                "2" => 3,
                _ => {
                    index += 1;
                    continue;
                }
            };
            if parts.len() < index + 2 + channels {
                return false;
            }
            index += 2 + channels;
        } else {
            index += 1;
        }
    }
    true
}

/// Append `params` as one or more CSIs, each of at most [`MAX_SGR_PARAMS`] tokens.
fn emit_sgr(out: &mut String, params: &[String]) {
    let mut chunk: Vec<&str> = Vec::new();
    let mut tokens = 0usize;
    for param in params {
        let count = param_tokens(param);
        if !chunk.is_empty() && tokens + count > MAX_SGR_PARAMS {
            out.push_str("\x1b[");
            out.push_str(&chunk.join(";"));
            out.push('m');
            chunk.clear();
            tokens = 0;
        }
        chunk.push(param);
        tokens += count;
    }
    if !chunk.is_empty() {
        out.push_str("\x1b[");
        out.push_str(&chunk.join(";"));
        out.push('m');
    }
}

/// Merge byte-adjacent SGR sequences into one CSI, splitting runs that exceed
/// [`MAX_SGR_PARAMS`]. Visible text and non-SGR escapes break the run; a
/// sequence with an incomplete extended colour stays on its own.
pub fn coalesce_adjacent_sgr(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let tokens = scan(input);
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            Token::Text(text) => {
                out.push_str(text);
                index += 1;
            }
            Token::Escape(seq) => {
                let Some(first) = sgr_params(seq) else {
                    out.push_str(seq);
                    index += 1;
                    continue;
                };
                let mut params = vec![if first.is_empty() { "0".to_string() } else { first.to_string() }];
                let mut end = index + 1;
                if sgr_complete(first) {
                    while let Some(Token::Escape(next)) = tokens.get(end) {
                        let Some(next_params) = sgr_params(next) else {
                            break;
                        };
                        if !sgr_complete(next_params) {
                            break;
                        }
                        params.push(if next_params.is_empty() {
                            "0".to_string()
                        } else {
                            next_params.to_string()
                        });
                        end += 1;
                    }
                }
                if params.len() == 1 {
                    out.push_str(seq);
                } else {
                    emit_sgr(&mut out, &params);
                }
                index = end;
            }
        }
    }
    out
}
