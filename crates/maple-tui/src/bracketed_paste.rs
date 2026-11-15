//! Bracketed paste decoding (port of `l/src/bracketed-paste.ts`).

pub const PASTE_START: &str = "\x1b[200~";
pub const PASTE_END: &str = "\x1b[201~";
pub const DEFAULT_BYTE_LIMIT: usize = 64 * 1024 * 1024;

pub struct PasteOutcome {
    /// True when `data` started with the paste marker.
    pub handled: bool,
    /// Completed paste payload, if the end marker was present.
    pub paste_content: Option<String>,
    /// Bytes after the paste end marker.
    pub remaining: String,
}

/// Split a stream chunk into optional paste payload plus remainder.
pub fn process(data: &str) -> PasteOutcome {
    process_limited(data, DEFAULT_BYTE_LIMIT)
}

pub fn process_limited(data: &str, byte_limit: usize) -> PasteOutcome {
    let Some(content) = data.strip_prefix(PASTE_START) else {
        return PasteOutcome {
            handled: false,
            paste_content: None,
            remaining: data.to_string(),
        };
    };
    let mut body = String::new();
    let mut rest = content;
    loop {
        if body.len() >= byte_limit {
            return PasteOutcome {
                handled: true,
                paste_content: Some(decode_reencoded_paste_controls(&body)),
                remaining: rest.to_string(),
            };
        }
        match rest.find(PASTE_END) {
            Some(index) => {
                body.push_str(&rest[..index]);
                rest = &rest[index + PASTE_END.len()..];
                break;
            }
            None => {
                body.push_str(rest);
                rest = "";
                break;
            }
        }
    }
    PasteOutcome {
        handled: true,
        paste_content: Some(decode_reencoded_paste_controls(&body)),
        remaining: rest.to_string(),
    }
}

/// Reverse tmux re-encodings: `ESC [ <cp>;5u` and `ESC [27;5;<cp>~` where
/// `<cp>` is an ASCII letter become `0x01..=0x1A`.
pub fn decode_reencoded_paste_controls(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '\x1b' {
            let tail = &input[index..];
            if let Some((token, consumed)) = paste_control_token(tail) {
                out.push(token);
                for _ in 0..consumed.saturating_sub(1) {
                    chars.next();
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Recognize `ESC [ <cp>;5u` / `ESC [27;5;<cp>~`; returns the control byte and
/// the number of characters consumed from `tail` (including the ESC).
fn paste_control_token(tail: &str) -> Option<(char, usize)> {
    let rest = tail.strip_prefix("\x1b[")?;
    let (token, consumed) = if let Some(position) = rest.find(";5u") {
        (&rest[..position], 2 + position + 3)
    } else if let Some(position) = rest.strip_prefix("27;5;")?.find('~') {
        (&rest[5..5 + position], 2 + 5 + position + 1)
    } else {
        return None;
    };
    control_byte(token).map(|byte| (byte as char, consumed))
}

fn control_byte(token: &str) -> Option<u8> {
    if token.len() == 1 {
        let letter = token.chars().next()?;
        if letter.is_ascii_alphabetic() {
            return Some((letter.to_ascii_lowercase() as u8 - b'a') + 1);
        }
    }
    let codepoint: u32 = token.parse().ok()?;
    let letter = char::from_u32(codepoint)?;
    if letter.is_ascii_alphabetic() {
        Some((letter.to_ascii_lowercase() as u8 - b'a') + 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_paste_and_remainder() {
        let outcome = process("\x1b[200~hello\x1b[201~\x1b[A");
        assert!(outcome.handled);
        assert_eq!(outcome.paste_content.as_deref(), Some("hello"));
        assert_eq!(outcome.remaining, "\x1b[A");
    }

    #[test]
    fn leaves_non_paste_untouched() {
        let outcome = process("plain");
        assert!(!outcome.handled);
        assert_eq!(outcome.remaining, "plain");
    }

    #[test]
    fn decodes_tmux_control_encoding() {
        let decoded = decode_reencoded_paste_controls("\x1b[a;5u\x1b[27;5;z~");
        assert_eq!(decoded, "\x01\x1a");
    }
}
