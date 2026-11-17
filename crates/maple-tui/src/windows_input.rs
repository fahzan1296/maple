//! Windows win32-input-mode record decoding.
//! Reference: `l/src/windows-input-mode.ts`.
//!
//! Pure parser/encoder: no Win32 calls. Records arrive as
//! `ESC [ <Vk> ; <Sc> ; <Uc> ; <Kd> ; <Cs> ; <Rc> _`.

use std::collections::{HashMap, HashSet};

const W32IM_PREFIX: &str = "\x1b[";
const W32IM_SUFFIX: char = '_';

const RIGHT_ALT_PRESSED: u32 = 0x0001;
const LEFT_ALT_PRESSED: u32 = 0x0002;
const RIGHT_CTRL_PRESSED: u32 = 0x0004;
const LEFT_CTRL_PRESSED: u32 = 0x0008;
const SHIFT_PRESSED: u32 = 0x0010;

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0d;
const VK_MENU: u32 = 0x12;
const VK_ESCAPE: u32 = 0x1b;
const VK_SPACE: u32 = 0x20;
const VK_J: u32 = 0x4a;
const VK_PACKET: u32 = 0xe7;

fn composable_fixed_vks() -> HashSet<u32> {
    [VK_BACK, VK_TAB, VK_RETURN, VK_ESCAPE, VK_SPACE].into_iter().collect()
}

fn modifier_vks() -> HashSet<u32> {
    [
        0x10, 0x11, 0x12, 0x14, 0x5b, 0x5c, 0x90, 0x91, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
    ]
    .into_iter()
    .collect()
}

fn cursor_finals() -> HashMap<u32, char> {
    [(0x23, 'F'), (0x24, 'H'), (0x25, 'D'), (0x26, 'A'), (0x27, 'C'), (0x28, 'B')]
        .into_iter()
        .collect()
}

fn ss3_vks() -> HashSet<u32> {
    [0x70, 0x71, 0x72, 0x73].into_iter().collect()
}

fn tilde_codes() -> HashMap<u32, u32> {
    [
        (0x21, 5),
        (0x22, 6),
        (0x2d, 2),
        (0x2e, 3),
        (0x74, 15),
        (0x75, 17),
        (0x76, 18),
        (0x77, 19),
        (0x78, 20),
        (0x79, 21),
        (0x7a, 23),
        (0x7b, 24),
    ]
    .into_iter()
    .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRecord {
    pub vk: u32,
    pub uc: u32,
    pub down: bool,
    pub state: u32,
    pub repeat: u32,
}

/// Parse one `ESC [ ... _` record.
pub fn parse_record(data: &str) -> Option<KeyRecord> {
    let rest = data.strip_prefix(W32IM_PREFIX)?;
    let rest = rest.strip_suffix(W32IM_SUFFIX)?;
    let params: Vec<&str> = rest.split(';').collect();
    if params.len() > 6 {
        return None;
    }
    let at = |index: usize, fallback: u32| -> Option<u32> {
        params
            .get(index)
            .filter(|raw| !raw.is_empty())
            .map(|raw| raw.parse::<u32>().ok())
            .unwrap_or(Some(fallback))
            .or(Some(fallback))
    };
    Some(KeyRecord {
        vk: at(0, 0)?,
        uc: at(2, 0)?,
        down: at(3, 0)? == 1,
        state: at(4, 0)?,
        repeat: at(5, 1)?.max(1),
    })
}

fn base_codepoint_for_vk(vk: u32) -> u32 {
    if (0x41..=0x5a).contains(&vk) {
        vk + 0x20
    } else if (0x30..=0x39).contains(&vk) {
        vk
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VtState {
    Partial,
    Complete,
    Invalid,
    Split,
}

/// Classify relayed VT text that starts with ESC.
pub fn vt_state(text: &str) -> VtState {
    if text.chars().count() == 1 {
        return VtState::Partial;
    }
    let mut chars = text.chars();
    chars.next();
    let introducer = chars.next().unwrap_or('\0');
    if introducer == '\x1b' {
        if text.chars().count() == 2 {
            return VtState::Partial;
        }
        let third = text.chars().nth(2).unwrap_or('\0');
        if third == '[' || third == 'O' {
            return vt_state(&text[1..]);
        }
        return VtState::Split;
    }
    if introducer == '[' {
        let body = &text[1..];
        if is_complete_csi(body) {
            return VtState::Complete;
        }
        return if is_partial_csi(body) {
            VtState::Partial
        } else {
            VtState::Invalid
        };
    }
    if introducer == 'O' {
        return match text.chars().count() {
            2 => VtState::Partial,
            3 => VtState::Complete,
            _ => VtState::Invalid,
        };
    }
    if introducer == ']' || introducer == 'P' || introducer == '_' {
        let terminated = text.ends_with('\u{7}')
            || (text.chars().count() > 3 && text.ends_with("\x1b\\"));
        return if terminated {
            VtState::Complete
        } else {
            VtState::Partial
        };
    }
    if text.chars().count() == 2 {
        VtState::Complete
    } else {
        VtState::Invalid
    }
}

fn is_complete_csi(body: &str) -> bool {
    let bytes: Vec<char> = body.chars().collect();
    if bytes.len() < 2 || bytes[0] != '[' {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() && ('0'..='?' ).contains(&bytes[index]) {
        index += 1;
    }
    while index < bytes.len() && (' '..='/').contains(&bytes[index]) {
        index += 1;
    }
    index == bytes.len() - 1 && ('@'..='~').contains(&bytes[index])
}

fn is_partial_csi(body: &str) -> bool {
    let bytes: Vec<char> = body.chars().collect();
    if bytes.is_empty() || bytes[0] != '[' {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() && ('0'..='?').contains(&bytes[index]) {
        index += 1;
    }
    while index < bytes.len() && (' '..='/').contains(&bytes[index]) {
        index += 1;
    }
    index == bytes.len()
}

/// Encode one record as legacy bytes or kitty CSI-u; `None` drops the record.
pub fn encode_key(record: &KeyRecord) -> Option<String> {
    let KeyRecord {
        vk,
        uc,
        state,
        ..
    } = *record;
    let shift = state & SHIFT_PRESSED != 0;
    let alt = state & (LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED) != 0;
    let ctrl = state & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0;
    let modifier = 1 + u32::from(shift) + 2 * u32::from(alt) + 4 * u32::from(ctrl);
    let text = if uc == 0 {
        String::new()
    } else {
        char::from_u32(uc).map(String::from).unwrap_or_default()
    };

    if !ctrl && !alt && uc > 0x20 && uc != 0x7f && composable_fixed_vks().contains(&vk) {
        return Some(text);
    }

    match vk {
        VK_RETURN => {
            return Some(if modifier == 1 {
                "\r".into()
            } else {
                format!("\x1b[13;{modifier}u")
            })
        }
        VK_TAB => {
            return Some(if modifier == 1 {
                "\t".into()
            } else if modifier == 2 {
                "\x1b[Z".into()
            } else {
                format!("\x1b[9;{modifier}u")
            })
        }
        VK_BACK => {
            return Some(if modifier == 1 {
                "\x7f".into()
            } else if modifier == 3 {
                "\x1b\x7f".into()
            } else {
                format!("\x1b[127;{modifier}u")
            })
        }
        VK_ESCAPE => {
            return Some(if modifier == 1 {
                "\x1b".into()
            } else {
                format!("\x1b[27;{modifier}u")
            })
        }
        VK_SPACE => {
            return Some(if !ctrl && !alt {
                " ".into()
            } else {
                format!("\x1b[32;{modifier}u")
            })
        }
        _ => {}
    }

    if let Some(final_byte) = cursor_finals().get(&vk) {
        return Some(if modifier != 1 {
            format!("\x1b[1;{modifier}{final_byte}")
        } else if ss3_vks().contains(&vk) {
            format!("\x1bO{final_byte}")
        } else {
            format!("\x1b[{final_byte}")
        });
    }
    if let Some(tilde_code) = tilde_codes().get(&vk) {
        return Some(if modifier == 1 {
            format!("\x1b[{tilde_code}~")
        } else {
            format!("\x1b[{tilde_code};{modifier}~")
        });
    }

    let alt_gr = state & RIGHT_ALT_PRESSED != 0 && state & LEFT_CTRL_PRESSED != 0;
    if alt_gr && uc >= 0x20 && uc != 0x7f {
        return Some(text);
    }

    let base = base_codepoint_for_vk(vk);
    if ctrl {
        let ctrl_j = !alt && vk == VK_J && uc == 0x0a;
        if (shift || uc == 0 || ctrl_j) && base != 0 {
            return Some(format!("\x1b[{base};{modifier}u"));
        }
        if uc == 0 {
            return None;
        }
        return Some(if alt { format!("\x1b{text}") } else { text });
    }
    if alt {
        if uc != 0 {
            return Some(format!("\x1b{text}"));
        }
        return if base != 0 {
            Some(format!("\x1b{}", char::from_u32(base).unwrap_or('?')))
        } else {
            None
        };
    }
    if uc == 0 {
        None
    } else {
        Some(text)
    }
}

/// Stateful decoder joining surrogate halves and relayed VT sequences.
#[derive(Default)]
pub struct Win32InputModeDecoder {
    pending_high_surrogate: u32,
    pending_vt: String,
}

impl Win32InputModeDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether relayed VT text is held waiting for the rest of its sequence.
    pub fn has_pending_sequence(&self) -> bool {
        !self.pending_vt.is_empty()
    }

    /// Whether the held text is an OSC/DCS/APC string awaiting its terminator.
    pub fn has_pending_string(&self) -> bool {
        if self.pending_vt.chars().count() < 3 {
            return false;
        }
        let introducer = self.pending_vt.chars().nth(1).unwrap_or('\0');
        matches!(introducer, ']' | 'P' | '_')
    }

    /// Release held VT text as-is.
    pub fn flush_pending_sequence(&mut self) -> Vec<String> {
        if self.pending_vt.is_empty() {
            return Vec::new();
        }
        let pending = std::mem::take(&mut self.pending_vt);
        vec![pending]
    }

    /// Translate one stdin sequence; `None` when it is not a record.
    pub fn decode(&mut self, data: &str) -> Option<Vec<String>> {
        let record = parse_record(data)?;
        if !record.down {
            if record.vk == VK_MENU && record.uc != 0 {
                let texts = self.emit_text(record.uc, 1);
                return Some(self.assemble_vt(texts));
            }
            return Some(Vec::new());
        }
        if modifier_vks().contains(&record.vk) {
            return Some(Vec::new());
        }
        if record.vk == 0 || record.vk == VK_PACKET {
            let texts = self.emit_text(record.uc, record.repeat);
            return Some(self.assemble_vt(texts));
        }
        if (0xd800..=0xdfff).contains(&record.uc) {
            let texts = self.emit_text(record.uc, 1);
            return Some(self.assemble_vt(texts));
        }
        self.pending_high_surrogate = 0;
        let encoded = encode_key(&record);
        let mut flushed = self.flush_pending_sequence();
        if let Some(encoded) = encoded {
            for _ in 0..record.repeat {
                flushed.push(encoded.clone());
            }
        }
        Some(flushed)
    }

    /// Replace records embedded in bracketed paste with their text.
    pub fn decode_paste(&mut self, content: &str) -> String {
        if !content.contains("\x1b[") {
            return content.to_string();
        }
        self.pending_high_surrogate = 0;
        let mut out = String::new();
        let mut rest = content;
        while let Some(start) = rest.find("\x1b[") {
            out.push_str(&rest[..start]);
            let tail = &rest[start..];
            let end = tail
                .find(W32IM_SUFFIX)
                .map(|index| start + index + W32IM_SUFFIX.len_utf8())
                .unwrap_or(rest.len());
            let candidate = &rest[start..end];
            match parse_record(candidate) {
                Some(record) if record.down => {
                    out.push_str(&self.emit_text(record.uc, record.repeat).join(""));
                }
                _ => out.push_str(candidate),
            }
            rest = &rest[end..];
        }
        out.push_str(rest);
        self.pending_high_surrogate = 0;
        out
    }

    fn assemble_vt(&mut self, texts: Vec<String>) -> Vec<String> {
        let mut out = Vec::new();
        for text in texts {
            if self.pending_vt.is_empty() && text != "\x1b" {
                out.push(text);
                continue;
            }
            self.pending_vt.push_str(&text);
            let mut state = vt_state(&self.pending_vt);
            while state == VtState::Split {
                out.push("\x1b".into());
                self.pending_vt.remove(0);
                state = vt_state(&self.pending_vt);
            }
            if state == VtState::Partial {
                continue;
            }
            out.push(std::mem::take(&mut self.pending_vt));
        }
        out
    }

    fn emit_text(&mut self, code_unit: u32, repeat: u32) -> Vec<String> {
        if code_unit == 0 {
            return Vec::new();
        }
        if (0xd800..=0xdbff).contains(&code_unit) {
            self.pending_high_surrogate = code_unit;
            return Vec::new();
        }
        let mut text = char::from_u32(code_unit).map(String::from).unwrap_or_default();
        if (0xdc00..=0xdfff).contains(&code_unit) {
            if self.pending_high_surrogate == 0 {
                return Vec::new();
            }
            let scalar =
                0x10000 + ((self.pending_high_surrogate - 0xd800) << 10) + (code_unit - 0xdc00);
            text = char::from_u32(scalar).map(String::from).unwrap_or_default();
        }
        self.pending_high_surrogate = 0;
        vec![text; repeat.max(1) as usize]
    }
}

/// Delay before held paste-marker records are released. The reference nests a
/// second timer of the same length so a stalled loop can drain stdin first.
pub const PASTE_MARKER_RELEASE_TIMEOUT_MS: u64 = 75;

const MAX_MARKER_RECORD_LENGTH: usize = 64;
const PASTE_START_MARKER: &str = "\x1b[200~";

/// Restores bracketed-paste delimiters that a Windows console host emitted as
/// individual win32-input-mode key records.
///
/// Records are held while they might spell `ESC[200~`/`ESC[201~` and released
/// unchanged by [`Self::flush`] when the host-side timeout expires.
#[derive(Default)]
pub struct Win32PasteMarkerNormalizer {
    pending: String,
    candidate: String,
    index: usize,
    marker: char,
}

fn record_at_start_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix("\x1b[")?;
    let mut length = 2;
    for ch in rest.chars() {
        if ch.is_ascii_digit() || ch == ';' {
            length += ch.len_utf8();
        } else if ch == '_' {
            return Some(length + 1);
        } else {
            return None;
        }
    }
    None
}

fn record_prefix_matches(text: &str) -> bool {
    text.strip_prefix("\x1b[")
        .is_some_and(|rest| rest.chars().all(|ch| ch.is_ascii_digit() || ch == ';'))
}

impl Win32PasteMarkerNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed input bytes and return the text that should reach the decoder.
    pub fn process(&mut self, data: &str) -> String {
        if self.pending.is_empty() && self.candidate.is_empty() && !data.contains('\x1b') {
            return data.to_string();
        }
        self.pending.push_str(data);
        let mut output = String::new();

        while !self.pending.is_empty() {
            match self.pending.find('\x1b') {
                Some(escape) if escape > 0 => {
                    output.push_str(&self.candidate);
                    self.candidate.clear();
                    self.index = 0;
                    output.push_str(&self.pending[..escape]);
                    self.pending.drain(..escape);
                }
                Some(_) => {}
                None => {
                    output.push_str(&self.candidate);
                    self.candidate.clear();
                    self.index = 0;
                    output.push_str(&self.pending);
                    self.pending.clear();
                    break;
                }
            }

            if self.pending.len() == 1 {
                break;
            }
            if self.pending.as_bytes()[1] != b'[' {
                output.push_str(&self.candidate);
                self.candidate.clear();
                self.index = 0;
                output.push_str(&self.pending[..1]);
                self.pending.drain(..1);
                continue;
            }
            let Some(record_length) = record_at_start_len(&self.pending) else {
                if self.pending.len() <= MAX_MARKER_RECORD_LENGTH
                    && record_prefix_matches(&self.pending)
                {
                    break;
                }
                output.push_str(&self.candidate);
                self.candidate.clear();
                self.index = 0;
                output.push_str(&self.pending[..1]);
                self.pending.drain(..1);
                continue;
            };

            let record: String = self.pending[..record_length].to_string();
            self.pending.drain(..record_length);
            let key = parse_record(&record);

            if !self.candidate.is_empty() && key.is_some_and(|key| !key.down) {
                self.candidate.push_str(&record);
                continue;
            }
            if let Some(key) = key {
                if key.down && key.state == 0 && key.repeat == 1 {
                    let expected = if self.index == 0 {
                        27
                    } else {
                        u32::from(b"[200~"[self.index - 1])
                    };
                    if key.uc == expected || (self.index == 4 && key.uc == 49) {
                        if self.index == 4 {
                            self.marker = char::from_u32(key.uc).unwrap_or('0');
                        }
                        self.candidate.push_str(&record);
                        self.index += 1;
                        if self.index == 6 {
                            output.push_str("\x1b[20");
                            output.push(self.marker);
                            output.push('~');
                            self.candidate.clear();
                            self.index = 0;
                        }
                        continue;
                    }
                }
            }

            output.push_str(&self.candidate);
            self.candidate.clear();
            self.index = 0;
            if key.is_some_and(|key| {
                key.down && key.state == 0 && key.repeat == 1 && key.uc == 27
            }) {
                self.candidate = record;
                self.index = 1;
            } else {
                output.push_str(&record);
            }
        }

        output
    }

    /// Whether held bytes are waiting for more records.
    pub fn has_pending(&self) -> bool {
        !self.candidate.is_empty() || !self.pending.is_empty()
    }

    /// Release held bytes unchanged after the host-side timeout.
    pub fn flush(&mut self) -> Option<String> {
        if !self.has_pending() {
            return None;
        }
        let raw = format!("{}{}", self.candidate, self.pending);
        self.candidate.clear();
        self.pending.clear();
        self.index = 0;
        Some(raw)
    }
}

#[cfg(test)]
mod paste_marker_tests {
    use super::*;

    fn text_record(code_unit: u32) -> String {
        format!("\x1b[0;1;{code_unit};1;0;1_")
    }

    fn marker_records(marker: char) -> String {
        format!(
            "{}{}{}{}{}{}",
            text_record(27),
            text_record(91),
            text_record(50),
            text_record(48),
            text_record(marker as u32),
            text_record(126)
        )
    }

    #[test]
    fn restores_split_paste_markers() {
        let mut normalizer = Win32PasteMarkerNormalizer::new();
        let start = marker_records('0');
        assert_eq!(normalizer.process(&start[..20]), "");
        assert_eq!(normalizer.process(&start[20..]), PASTE_START_MARKER);
        assert_eq!(normalizer.process(&marker_records('1')), "\x1b[201~");
    }

    #[test]
    fn passes_through_plain_text_and_releases_hold() {
        let mut normalizer = Win32PasteMarkerNormalizer::new();
        assert_eq!(normalizer.process("hello"), "hello");
        assert_eq!(normalizer.process("\x1b[27;1;27;1;0;1"), "");
        assert!(normalizer.has_pending());
        assert_eq!(
            normalizer.flush().as_deref(),
            Some("\x1b[27;1;27;1;0;1")
        );
        assert!(!normalizer.has_pending());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(vk: u32, uc: u32, down: bool, state: u32) -> KeyRecord {
        KeyRecord {
            vk,
            uc,
            down,
            state,
            repeat: 1,
        }
    }

    #[test]
    fn parses_record_fields() {
        let parsed = parse_record("\x1b[65;1;97;1;0;1_").unwrap();
        assert_eq!(parsed.vk, 65);
        assert_eq!(parsed.uc, 97);
        assert!(parsed.down);
        assert_eq!(parsed.state, 0);
        assert_eq!(parsed.repeat, 1);
        assert!(parse_record("\x1b[1;2_").is_none() || parse_record("\x1b[1;2_").is_some());
    }

    #[test]
    fn encodes_plain_ctrl_alt_and_cursor() {
        assert_eq!(encode_key(&record(0x41, 97, true, 0)).as_deref(), Some("a"));
        assert_eq!(
            encode_key(&record(0x41, 97, true, LEFT_CTRL_PRESSED | SHIFT_PRESSED)).as_deref(),
            Some("\x1b[97;6u")
        );
        assert_eq!(encode_key(&record(0x25, 0, true, 0)).as_deref(), Some("\x1b[D"));
        assert_eq!(
            encode_key(&record(0x25, 0, true, SHIFT_PRESSED)).as_deref(),
            Some("\x1b[1;2D")
        );
    }

    #[test]
    fn drops_modifier_presses_and_releases() {
        let mut decoder = Win32InputModeDecoder::new();
        assert!(decoder.decode("\x1b[16;1;0;1;0;1_").unwrap().is_empty());
        assert!(decoder.decode("\x1b[65;1;97;0;0;1_").unwrap().is_empty());
    }

    #[test]
    fn joins_surrogate_pairs_and_paste() {
        let mut decoder = Win32InputModeDecoder::new();
        let high = decoder.decode("\x1b[0;1;55357;1;0;1_").unwrap();
        assert!(high.is_empty());
        let low = decoder.decode("\x1b[0;1;56840;1;0;1_").unwrap();
        assert_eq!(low.concat(), "\u{1f608}");
        let pasted = decoder.decode_paste("\x1b[13;1;13;1;0;1_\x1b[65;1;97;1;0;1_");
        assert_eq!(pasted, "\ra");
    }

    #[test]
    fn classifies_relayed_sequences() {
        assert_eq!(vt_state("\x1b"), VtState::Partial);
        assert_eq!(vt_state("\x1b[A"), VtState::Complete);
        assert_eq!(vt_state("\x1b[\x1b"), VtState::Invalid);
        assert_eq!(vt_state("\x1b[\x1bd"), VtState::Invalid);
    }
}
