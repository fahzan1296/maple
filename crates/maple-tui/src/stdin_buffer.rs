//! Stdin input buffer: reassembles split escape sequences and classifies
//! pastes (port of `l/src/stdin-buffer.ts`).
//!
//! Reference: `l/src/stdin-buffer.ts`.
//!
//! Deviations forced by Rust:
//!
//! - The reference extends `EventEmitter` and emits `data` / `paste` events.
//!   The Rust port returns [`StdinBufferEvent`] values from [`StdinBuffer::process`]
//!   and [`StdinBuffer::tick`] instead.
//! - The reference schedules `setTimeout` timers (flush, paste watchdog, raw
//!   paste window, string abandon, string-discard watchdog). The crate has no
//!   timer queue, so each is a deadline and the host loop calls
//!   [`StdinBuffer::tick`] once per iteration to fire due timers. The
//!   zero-delay flush deferral is unnecessary under polling: the host feeds
//!   stdin before ticking.
//! - The reference reads a process-global `isKittyProtocolActive` flag and
//!   accepts `Buffer` input with a high-byte escape conversion. The port has a
//!   per-instance [`StdinBuffer::set_kitty_protocol_active`] flag (default
//!   `false`) and takes `&str` only.

use crate::native::encode::TSP_OSC_PREFIX;
use std::time::{Duration, Instant};

const ESC: u8 = 0x1b;
/// Terminal bracketed-paste open marker.
pub const BRACKETED_PASTE_START: &str = "\x1b[200~";
/// Terminal bracketed-paste close marker.
pub const BRACKETED_PASTE_END: &str = "\x1b[201~";

const PASTE_INACTIVITY_TIMEOUT_MS: u64 = 1000;
const PASTE_MAX_BYTES: usize = 64 * 1024 * 1024;
const KITTY_PRINTABLE_DEDUP_WINDOW_MS: u128 = 25;
const PARTIAL_HOLD_MAX_MS: u128 = 150;
const MAX_CSI_BYTES: usize = 4096;
const MAX_STRING_SEQ_BYTES: usize = 16 * 1024 * 1024;
const STRING_DISCARD_MAX_BYTES: usize = 2 * MAX_STRING_SEQ_BYTES;
const STRING_DISCARD_INACTIVITY_MS: u64 = 1000;
const STRING_ABANDON_MS: u64 = 5000;
const STRING_DORMANT_MS: u64 = 10_000;
const RAW_PASTE_CLASSIFICATION_TIMEOUT_MS: u64 = 10;
const MAX_OSC5522_METADATA: usize = 1024;
const STRING_ABORTED: i64 = -3;
const OSC5522_PREFIX: &str = "\x1b]5522;";

/// One buffered input event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StdinBufferEvent {
    /// One complete key/escape sequence.
    Data(String),
    /// A completed bracketed paste; `enter` carries an Enter keypress that
    /// shared the paste's stdin read.
    Paste {
        /// The pasted text.
        content: String,
        /// A trailing Enter typed in the same read.
        enter: Option<String>,
    },
}

/// Timing and bound overrides for [`StdinBuffer`].
#[derive(Clone, Debug)]
pub struct StdinBufferOptions {
    /// Maximum time to wait for sequence completion (default 75ms).
    pub timeout: u64,
    /// Extra time an unambiguous partial is held past `timeout` (default 150ms).
    pub partial_hold_timeout: u64,
    /// Paste-mode inactivity watchdog (default 1000ms).
    pub paste_timeout: u64,
    /// Paste-mode byte cap (default 64 MiB).
    pub paste_byte_limit: usize,
}

impl Default for StdinBufferOptions {
    fn default() -> Self {
        StdinBufferOptions {
            timeout: 75,
            partial_hold_timeout: PARTIAL_HOLD_MAX_MS as u64,
            paste_timeout: PASTE_INACTIVITY_TIMEOUT_MS,
            paste_byte_limit: PASTE_MAX_BYTES,
        }
    }
}

/// Whether `text` has two completed logical line breaks.
fn is_raw_multiline_burst(text: &str) -> bool {
    let mut breaks = 0;
    let mut index = 0;
    let bytes = text.as_bytes();
    while index < bytes.len() {
        let code = bytes[index];
        if code == 0x0d {
            breaks += 1;
            if bytes.get(index + 1) == Some(&0x0a) {
                index += 1;
            }
            index += 1;
            continue;
        }
        if code == 0x0a {
            breaks += 1;
            index += 1;
            continue;
        }
        if breaks >= 2 {
            return true;
        }
        let ch = text[index..].chars().next().unwrap_or('\0');
        index += ch.len_utf8();
    }
    false
}

/// Whether a classified raw burst ends with an Enter typed after pasted text.
fn ends_with_typed_enter(text: &str) -> bool {
    let bytes = text.as_bytes();
    let Some(last) = bytes.len().checked_sub(1) else {
        return false;
    };
    if bytes[last] != 0x0d {
        return false;
    }
    !text[..last].contains('\r')
}

fn is_paste_report(buffer: &str, pos: usize) -> bool {
    buffer[pos..].starts_with("\x1b_") || buffer[pos..].starts_with(TSP_OSC_PREFIX)
}

fn is_base64_code(code: u8) -> bool {
    code.is_ascii_uppercase() || code.is_ascii_lowercase() || code.is_ascii_digit() || code == b'+' || code == b'/' || code == b'='
}

/// Index where an OSC 5522 packet's base64 payload starts, or -1.
fn osc5522_payload_start(bytes: &[u8], pos: usize, scan_limit: usize) -> i64 {
    let prefix = OSC5522_PREFIX.as_bytes();
    if !bytes[pos..].starts_with(prefix) {
        return -1;
    }
    let metadata_end = scan_limit.min(pos + prefix.len() + MAX_OSC5522_METADATA);
    for i in pos + prefix.len()..metadata_end {
        if bytes[i] == 0x3b {
            return i as i64 + 1;
        }
    }
    -1
}

/// Resolve an OSC/DCS/APC string sequence by its grammar. See the reference.
fn resolve_string_end(
    bytes: &[u8],
    pos: usize,
    length: usize,
    resume_search_from: usize,
    bel_terminates: bool,
) -> i64 {
    let search_from = (pos + 2).max(resume_search_from.saturating_sub(1));
    let scan_limit = length.min(pos + MAX_STRING_SEQ_BYTES);
    let base64_from = if bel_terminates {
        osc5522_payload_start(bytes, pos, scan_limit)
    } else {
        -1
    };
    let dcs = bytes.get(pos + 1) == Some(&0x50);
    let mut i = search_from;
    while i < scan_limit {
        let code = bytes[i];
        if code == 0x1b {
            if i + 1 == scan_limit {
                break;
            }
            if bytes[i + 1] == 0x5c {
                return i as i64 + 2;
            }
            return STRING_ABORTED - i as i64;
        }
        if code == 0x07 && bel_terminates {
            return i as i64 + 1;
        }
        if (code < 0x20 || code == 0x7f) && !(dcs && (0x08..=0x0d).contains(&code)) {
            return STRING_ABORTED - i as i64;
        }
        if base64_from != -1 && i as i64 >= base64_from && !is_base64_code(code) {
            return STRING_ABORTED - i as i64;
        }
        i += 1;
    }
    if length - pos >= MAX_STRING_SEQ_BYTES {
        -2
    } else {
        -1
    }
}

fn sgr_mouse_complete(payload: &[u8]) -> bool {
    // `^<\d+;\d+;\d+[Mm]$`
    let Some(rest) = payload.first().filter(|&&b| b == b'<') else {
        return false;
    };
    let rest = &payload[1..];
    let Some((last, body)) = rest.split_last() else {
        return false;
    };
    if *last != b'M' && *last != b'm' {
        return false;
    }
    let parts: Vec<&[u8]> = body.split(|&b| b == b';').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.iter().all(u8::is_ascii_digit))
}

fn sgr_mouse_partial(buffer: &str) -> bool {
    let bytes = buffer.as_bytes();
    let Some(rest) = bytes.strip_prefix(b"\x1b[<") else {
        return false;
    };
    rest.iter().all(|b| b.is_ascii_digit() || *b == b';')
}

fn string_seq_partial(buffer: &str) -> bool {
    let bytes = buffer.as_bytes();
    bytes.first() == Some(&ESC) && matches!(bytes.get(1), Some(&b']') | Some(&b'P') | Some(&b'_'))
}

/// Prefix match for the legacy string-reply openers omp receives.
fn legacy_string_reply_opener(buffer: &str) -> bool {
    let b = buffer.as_bytes();
    if b.first() != Some(&ESC) {
        return false;
    }
    match b.get(1) {
        Some(&b']') => {
            let mut i = 2;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            i > 2 && (i == b.len() || b[i] == b';')
        }
        Some(&b'P') => {
            let Some(&c) = b.get(2) else { return false };
            if c.is_ascii_digit() {
                match b.get(3) {
                    None => true,
                    Some(&d) if d == b'$' || d == b'+' => {
                        matches!(b.get(4), None | Some(&b'r'))
                    }
                    Some(_) => false,
                }
            } else if c == b'>' || c == b'!' {
                matches!(b.get(3), None | Some(&b'|'))
            } else {
                false
            }
        }
        Some(&b'_') => {
            let Some(&c) = b.get(2) else { return false };
            match c {
                b'G' => true,
                b't' => match b.get(3) {
                    None => true,
                    Some(&b's') => match b.get(4) {
                        None => true,
                        Some(&b'p') => matches!(b.get(5), None | Some(&b';')),
                        Some(_) => false,
                    },
                    Some(_) => false,
                },
                b'2' => match b.get(3) {
                    None => true,
                    Some(&b'5') => match b.get(4) {
                        None => true,
                        Some(&b'a') => match b.get(5) {
                            None => true,
                            Some(&b'1') => matches!(b.get(6), None | Some(&b';')),
                            Some(_) => false,
                        },
                        Some(_) => false,
                    },
                    Some(_) => false,
                },
                _ => false,
            }
        }
        _ => false,
    }
}

/// Resolve the exclusive-end index of the escape sequence starting at `pos`.
fn resolve_escape_end(bytes: &[u8], pos: usize, length: usize, resume_search_from: usize) -> i64 {
    if pos + 1 >= length {
        return -1;
    }
    let next = bytes[pos + 1];
    match next {
        0x1b => -1,
        0x5b => {
            if pos + 2 >= length {
                return -1;
            }
            if bytes[pos + 2] == 0x4d {
                return if pos + 6 <= length {
                    pos as i64 + 6
                } else {
                    -1
                };
            }
            let cap_end = length.min(pos + MAX_CSI_BYTES);
            let is_sgr_mouse = bytes[pos + 2] == 0x3c;
            let mut i = pos + 2;
            while i < cap_end {
                let code = bytes[i];
                if (0x40..=0x7e).contains(&code) {
                    if is_sgr_mouse {
                        if code != 0x4d && code != 0x6d {
                            i += 1;
                            continue;
                        }
                        if sgr_mouse_complete(&bytes[pos + 2..=i]) {
                            return i as i64 + 1;
                        }
                        i += 1;
                        continue;
                    }
                    return i as i64 + 1;
                }
                i += 1;
            }
            if length - pos >= MAX_CSI_BYTES {
                -2
            } else {
                -1
            }
        }
        0x5d => resolve_string_end(bytes, pos, length, resume_search_from, true),
        0x50 | 0x5f => resolve_string_end(bytes, pos, length, resume_search_from, false),
        0x4f => {
            if pos + 3 <= length {
                pos as i64 + 3
            } else {
                -1
            }
        }
        _ => pos as i64 + 2,
    }
}

/// Parse an unmodified kitty CSI-u printable codepoint.
fn parse_unmodified_kitty_printable_codepoint(sequence: &str) -> Option<u32> {
    let body = sequence.strip_prefix("\x1b[")?.strip_suffix('u')?;
    let digits_end = body
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(body.len());
    if digits_end == 0 {
        return None;
    }
    let codepoint: u32 = body[..digits_end].parse().ok()?;
    let rest = &body[digits_end..];
    let mut groups = rest.split(':');
    // `rest` starts with `:` when non-empty, so the first group is empty.
    if groups.next() != Some("") {
        return None;
    }
    let first = groups.next();
    match first {
        None => {}
        Some(digits) => {
            if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            match groups.next() {
                None => {}
                Some(second) => {
                    if second.is_empty() || !second.bytes().all(|byte| byte.is_ascii_digit()) {
                        return None;
                    }
                    if groups.next().is_some() {
                        return None;
                    }
                }
            }
        }
    }
    (codepoint >= 32).then_some(codepoint)
}

struct ExtractResult {
    sequences: Vec<String>,
    remainder: String,
    resume_search_from: usize,
    discard_from: Option<usize>,
    aborted_head: bool,
}

/// Split the accumulated buffer into complete sequences.
fn extract_complete_sequences(buffer: &str, resume_search_from: usize) -> ExtractResult {
    let bytes = buffer.as_bytes();
    let length = bytes.len();
    let mut sequences = Vec::new();
    let mut pos = 0;
    let mut aborted_head = false;
    let mut hint = resume_search_from;

    while pos < length {
        if bytes[pos] != ESC {
            let ch = buffer[pos..].chars().next().unwrap();
            let char_length = ch.len_utf8();
            sequences.push(buffer[pos..pos + char_length].to_string());
            pos += char_length;
            hint = 0;
            continue;
        }

        if pos + 1 < length && bytes[pos + 1] == ESC {
            if pos + 2 >= length {
                return ExtractResult {
                    sequences,
                    remainder: buffer[pos..].to_string(),
                    resume_search_from: 0,
                    discard_from: None,
                    aborted_head,
                };
            }
            let third = bytes[pos + 2];
            if third != 0x5b && third != 0x4f {
                sequences.push("\x1b".to_string());
                pos += 1;
                hint = 0;
                continue;
            }
            let inner_end = resolve_escape_end(bytes, pos + 1, length, 0);
            if inner_end == -1 {
                return ExtractResult {
                    sequences,
                    remainder: buffer[pos..].to_string(),
                    resume_search_from: 0,
                    discard_from: None,
                    aborted_head,
                };
            }
            if inner_end == -2 {
                let flush_end = length.min(pos + MAX_CSI_BYTES);
                sequences.push(buffer[pos..flush_end].to_string());
                pos = flush_end;
                hint = 0;
                continue;
            }
            if third == 0x5b && bytes.get(pos + 3) == Some(&0x3c) {
                sequences.push("\x1b".to_string());
                sequences.push(buffer[pos + 1..inner_end as usize].to_string());
                pos = inner_end as usize;
                hint = 0;
                continue;
            }
            sequences.push(buffer[pos..inner_end as usize].to_string());
            pos = inner_end as usize;
            hint = 0;
            continue;
        }

        let end = resolve_escape_end(bytes, pos, length, if pos == 0 { hint } else { 0 });
        if end <= STRING_ABORTED {
            if pos == 0 {
                aborted_head = true;
            }
            pos = (STRING_ABORTED - end) as usize;
            hint = 0;
            continue;
        }
        if end == -1 {
            let next = if pos + 1 < length { bytes[pos + 1] as i32 } else { -1 };
            let next_hint = if pos == 0 && (next == 0x5d || next == 0x50 || next == 0x5f) {
                length
            } else {
                0
            };
            return ExtractResult {
                sequences,
                remainder: buffer[pos..].to_string(),
                resume_search_from: next_hint,
                discard_from: None,
                aborted_head,
            };
        }
        if end == -2 {
            let next = bytes[pos + 1];
            if next == 0x5d || next == 0x50 || next == 0x5f {
                return ExtractResult {
                    sequences,
                    remainder: String::new(),
                    resume_search_from: 0,
                    discard_from: Some(pos),
                    aborted_head,
                };
            }
            let flush_end = length.min(pos + MAX_CSI_BYTES);
            sequences.push(buffer[pos..flush_end].to_string());
            pos = flush_end;
            hint = 0;
            continue;
        }

        sequences.push(buffer[pos..end as usize].to_string());
        pos = end as usize;
        hint = 0;
    }

    ExtractResult {
        sequences,
        remainder: String::new(),
        resume_search_from: 0,
        discard_from: None,
        aborted_head,
    }
}

/// The Enter keypress immediately following a bracketed-paste end marker.
fn leading_enter(input: &str) -> Option<&str> {
    if input.starts_with('\r') || input.starts_with('\n') {
        return input.chars().next().map(|ch| &input[..ch.len_utf8()]);
    }
    let rest = input.strip_prefix("\x1b[13")?;
    if let Some(after) = rest.strip_prefix(";1") {
        if after.starts_with('u') {
            return Some(&input[.."\x1b[13;1u".len()]);
        }
        return None;
    }
    if rest.starts_with('u') {
        return Some(&input[.."\x1b[13u".len()]);
    }
    None
}

/// Buffers stdin input and yields complete sequences.
pub struct StdinBuffer {
    buffer: String,
    timeout_ms: u64,
    partial_hold_max_ms: u128,
    paste_timeout_ms: u64,
    paste_byte_limit: usize,
    paste_mode: bool,
    paste_chunks: Vec<String>,
    paste_bytes: usize,
    paste_deadline: Option<Instant>,
    pending_kitty_printable_codepoint: Option<u32>,
    pending_kitty_printable_at: Option<Instant>,
    escape_search_offset: usize,
    raw_paste_candidate: String,
    raw_paste_deadline: Option<Instant>,
    raw_paste_stall_probe: Option<Box<dyn Fn() -> bool>>,
    string_discard_active: bool,
    string_discard_bytes: usize,
    string_discard_esc_held: bool,
    string_discard_deadline: Option<Instant>,
    string_progress_at: Option<Instant>,
    string_typed_keys: Vec<String>,
    string_typed_at: Vec<usize>,
    string_abandon_deadline: Option<Instant>,
    dormant_string: Option<String>,
    dormant_until: Option<Instant>,
    string_resumed: bool,
    flush_deadline: Option<Instant>,
    partial_hold_start: Option<Instant>,
    kitty_protocol_active: bool,
}

impl Default for StdinBuffer {
    fn default() -> Self {
        Self::new(StdinBufferOptions::default())
    }
}

impl StdinBuffer {
    /// Create a buffer with the given timing/bound overrides.
    pub fn new(options: StdinBufferOptions) -> Self {
        StdinBuffer {
            buffer: String::new(),
            timeout_ms: options.timeout,
            partial_hold_max_ms: options.partial_hold_timeout as u128,
            paste_timeout_ms: options.paste_timeout,
            paste_byte_limit: options.paste_byte_limit,
            paste_mode: false,
            paste_chunks: Vec::new(),
            paste_bytes: 0,
            paste_deadline: None,
            pending_kitty_printable_codepoint: None,
            pending_kitty_printable_at: None,
            escape_search_offset: 0,
            raw_paste_candidate: String::new(),
            raw_paste_deadline: None,
            raw_paste_stall_probe: None,
            string_discard_active: false,
            string_discard_bytes: 0,
            string_discard_esc_held: false,
            string_discard_deadline: None,
            string_progress_at: None,
            string_typed_keys: Vec::new(),
            string_typed_at: Vec::new(),
            string_abandon_deadline: None,
            dormant_string: None,
            dormant_until: None,
            string_resumed: false,
            flush_deadline: None,
            partial_hold_start: None,
            kitty_protocol_active: false,
        }
    }

    /// Whether a bracketed paste has started and its end marker has not arrived.
    pub fn pasting(&self) -> bool {
        self.paste_mode
    }

    /// Set the kitty keyboard protocol flag the reference reads globally.
    pub fn set_kitty_protocol_active(&mut self, active: bool) {
        self.kitty_protocol_active = active;
    }

    /// Gate unbracketed raw-paste classification on event-loop responsiveness.
    pub fn set_raw_paste_stall_probe(&mut self, stalled: Option<Box<dyn Fn() -> bool>>) {
        self.raw_paste_stall_probe = stalled;
    }

    /// The raw buffered text not yet delivered.
    pub fn get_buffer(&self) -> String {
        format!("{}{}", self.raw_paste_candidate, self.buffer)
    }

    /// Feed one stdin read; returns the events it completed.
    pub fn process(&mut self, data: &str) -> Vec<StdinBufferEvent> {
        let mut events = Vec::new();
        self.process_into(data, &mut events);
        events
    }

    /// Fire due timers; returns the events they completed.
    pub fn tick(&mut self) -> Vec<StdinBufferEvent> {
        let mut events = Vec::new();
        let now = Instant::now();
        if let Some(deadline) = self.raw_paste_deadline {
            if now >= deadline {
                self.raw_paste_deadline = None;
                self.flush_raw_paste_candidate(&mut events);
            }
        }
        if self.string_discard_active {
            if let Some(deadline) = self.string_discard_deadline {
                if now >= deadline {
                    self.string_discard_deadline = None;
                    self.exit_string_discard();
                }
            }
        }
        if let Some(deadline) = self.string_abandon_deadline {
            if now >= deadline {
                self.string_abandon_deadline = None;
                self.on_string_abandon(now, &mut events);
            }
        }
        if self.paste_mode {
            if let Some(deadline) = self.paste_deadline {
                if now >= deadline {
                    self.paste_deadline = None;
                    self.abort_paste(&mut events);
                }
            }
        }
        if let Some(deadline) = self.flush_deadline {
            if now >= deadline {
                self.flush_deadline = None;
                self.flush_expired(now, &mut events);
            }
        }
        events
    }

    fn process_into(&mut self, data: &str, events: &mut Vec<StdinBufferEvent>) {
        let mut str = data.to_string();
        if self.string_discard_active {
            str = self.consume_string_discard(&str);
            if str.is_empty() {
                return;
            }
        }
        if self.paste_mode {
            self.consume_paste_chunk(&str, events);
            return;
        }
        if self.dormant_string.is_some() && self.buffer.is_empty() {
            if self.dormant_until.is_some_and(|until| Instant::now() < until) {
                self.resume_dormant_string(&str);
            } else {
                self.dormant_string = None;
            }
        }
        if self.buffer_is_string_head() {
            self.track_string_read(&str);
        }

        // New data may complete the buffered partial; a pending flush is stale.
        self.flush_deadline = None;

        if str.is_empty() && self.buffer.is_empty() && self.raw_paste_candidate.is_empty() {
            self.emit_data_sequence("", events);
            return;
        }

        if !self.raw_paste_candidate.is_empty() {
            if str.contains('\x1b') {
                self.flush_raw_paste_candidate(events);
            } else {
                self.raw_paste_candidate.push_str(&str);
                if is_raw_multiline_burst(&self.raw_paste_candidate) {
                    self.classify_raw_paste_candidate(events);
                }
                return;
            }
        }

        if self.buffer.is_empty()
            && !str.contains('\x1b')
            && (str.contains('\r') || str.contains('\n'))
        {
            self.raw_paste_candidate = str;
            if is_raw_multiline_burst(&self.raw_paste_candidate) {
                self.classify_raw_paste_candidate(events);
            } else {
                self.arm_raw_paste_timer();
            }
            return;
        }

        self.buffer.push_str(&str);

        if let Some(start_index) = self.buffer.find(BRACKETED_PASTE_START) {
            self.emit_string_typed_keys(events);
            if start_index > 0 {
                let before_paste = self.buffer[..start_index].to_string();
                let result = extract_complete_sequences(&before_paste, 0);
                for sequence in result.sequences {
                    self.emit_data_sequence(&sequence, events);
                }
            }
            self.escape_search_offset = 0;
            self.pending_kitty_printable_codepoint = None;
            let rest = self.buffer[start_index + BRACKETED_PASTE_START.len()..].to_string();
            self.buffer.clear();
            self.paste_mode = true;
            self.paste_chunks.clear();
            self.paste_bytes = 0;
            self.consume_paste_chunk(&rest, events);
            return;
        }

        let result = extract_complete_sequences(&self.buffer, self.escape_search_offset);
        if let Some(discard_from) = result.discard_from {
            let junk = self.buffer[discard_from..].to_string();
            self.buffer.clear();
            self.escape_search_offset = 0;
            self.take_string_typed_keys();
            for sequence in result.sequences {
                self.emit_data_sequence(&sequence, events);
            }
            self.enter_string_discard();
            let after = self.consume_string_discard(&junk);
            if !after.is_empty() {
                self.process_into(&after, events);
            }
            return;
        }
        self.buffer = result.remainder;
        self.escape_search_offset = result.resume_search_from;

        if result.aborted_head {
            self.emit_string_typed_keys(events);
        }
        for sequence in result.sequences {
            self.emit_data_sequence(&sequence, events);
        }

        if self.buffer_is_string_head() {
            if self.string_progress_at.is_none() {
                self.dormant_string = None;
                self.string_resumed = false;
                self.note_string_progress();
            }
            self.partial_hold_start = None;
        } else {
            self.take_string_typed_keys();
            if !self.buffer.is_empty() {
                self.arm_flush_timer();
            } else {
                self.partial_hold_start = None;
            }
        }
    }

    fn consume_paste_chunk(&mut self, chunk: &str, events: &mut Vec<StdinBufferEvent>) {
        let input = format!("{}{}", self.buffer, chunk);
        let search_offset = self.escape_search_offset;
        self.buffer.clear();
        self.escape_search_offset = 0;
        let previous_bytes = self.paste_bytes;
        let bytes = input.as_bytes();
        let length = bytes.len();
        let mut text_start = 0;
        let mut search_from = 0;

        while search_from < length {
            let Some(relative) = find_byte(&bytes[search_from..], ESC) else {
                break;
            };
            let escape = search_from + relative;

            if input[escape..].starts_with(BRACKETED_PASTE_END) {
                self.append_paste_text(&input[text_start..escape]);
                let remaining = input[escape + BRACKETED_PASTE_END.len()..].to_string();
                self.finish_paste(&remaining, events);
                return;
            }
            if input.len() - escape < TSP_OSC_PREFIX.len() {
                let tail = &input[escape..];
                if BRACKETED_PASTE_END.starts_with(tail) || TSP_OSC_PREFIX.starts_with(tail) {
                    self.append_paste_text(&input[text_start..escape]);
                    self.buffer = tail.to_string();
                    text_start = input.len();
                    break;
                }
            }
            if !is_paste_report(&input, escape) {
                search_from = escape + 1;
                continue;
            }

            self.append_paste_text(&input[text_start..escape]);
            let end = resolve_escape_end(bytes, escape, length, if escape == 0 { search_offset } else { 0 });
            if end == -1 {
                self.buffer = input[escape..].to_string();
                self.escape_search_offset = input.len() - escape;
                text_start = input.len();
                break;
            }
            if end == -2 {
                self.enter_string_discard();
                let rest = self.consume_string_discard(&input[escape..]);
                self.consume_paste_chunk(&rest, events);
                return;
            }
            if end <= STRING_ABORTED {
                search_from = (STRING_ABORTED - end) as usize;
            } else {
                let sequence = input[escape..end as usize].to_string();
                self.emit_data_sequence(&sequence, events);
                search_from = end as usize;
            }
            text_start = search_from;
        }

        self.append_paste_text(&input[text_start..]);
        let pending_bytes = if is_paste_report(&self.buffer, 0) {
            0
        } else {
            self.buffer.len()
        };
        if self.paste_bytes + pending_bytes > self.paste_byte_limit {
            self.abort_paste(events);
        } else if self.paste_deadline.is_none()
            || self.paste_bytes != previous_bytes
            || pending_bytes > 0
        {
            self.arm_paste_watchdog();
        }
    }

    fn append_paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.paste_chunks.push(text.to_string());
        self.paste_bytes += text.len();
    }

    fn finish_paste(&mut self, remaining: &str, events: &mut Vec<StdinBufferEvent>) {
        let content = self.paste_chunks.join("");
        self.clear_paste_watchdog();
        self.paste_mode = false;
        self.paste_chunks.clear();
        self.paste_bytes = 0;
        self.pending_kitty_printable_codepoint = None;

        let enter = leading_enter(remaining);
        events.push(StdinBufferEvent::Paste {
            content,
            enter: enter.map(str::to_string),
        });
        let rest = match enter {
            Some(enter) => &remaining[enter.len()..],
            None => remaining,
        };
        if !rest.is_empty() {
            self.process_into(rest, events);
        }
    }

    fn arm_paste_watchdog(&mut self) {
        self.paste_deadline = Some(Instant::now() + Duration::from_millis(self.paste_timeout_ms));
    }

    fn clear_paste_watchdog(&mut self) {
        self.paste_deadline = None;
    }

    fn abort_paste(&mut self, events: &mut Vec<StdinBufferEvent>) {
        let pending = std::mem::take(&mut self.buffer);
        self.escape_search_offset = 0;
        let reply = is_paste_report(&pending, 0);
        if !reply {
            self.append_paste_text(&pending);
        }
        self.finish_paste("", events);
        if reply {
            self.process_into(&pending, events);
        }
    }

    fn arm_raw_paste_timer(&mut self) {
        if self.raw_paste_deadline.is_some() {
            return;
        }
        self.raw_paste_deadline =
            Some(Instant::now() + Duration::from_millis(RAW_PASTE_CLASSIFICATION_TIMEOUT_MS));
    }

    fn take_raw_paste_candidate(&mut self) -> String {
        self.raw_paste_deadline = None;
        std::mem::take(&mut self.raw_paste_candidate)
    }

    fn classify_raw_paste_candidate(&mut self, events: &mut Vec<StdinBufferEvent>) {
        if self.raw_paste_stall_probe.as_ref().is_some_and(|probe| probe()) {
            self.flush_raw_paste_candidate(events);
            return;
        }
        let content = self.take_raw_paste_candidate();
        self.pending_kitty_printable_codepoint = None;
        if ends_with_typed_enter(&content) {
            let pasted = content[..content.len() - 1].to_string();
            events.push(StdinBufferEvent::Paste {
                content: pasted,
                enter: Some("\r".to_string()),
            });
        } else {
            events.push(StdinBufferEvent::Paste {
                content,
                enter: None,
            });
        }
    }

    fn flush_raw_paste_candidate(&mut self, events: &mut Vec<StdinBufferEvent>) {
        let content = self.take_raw_paste_candidate();
        if content.is_empty() {
            return;
        }
        let result = extract_complete_sequences(&content, 0);
        for sequence in result.sequences {
            self.emit_data_sequence(&sequence, events);
        }
    }

    fn emit_data_sequence(&mut self, sequence: &str, events: &mut Vec<StdinBufferEvent>) {
        let now = Instant::now();
        let utf16_length = sequence.encode_utf16().count();
        let raw_codepoint = if utf16_length == 1 {
            sequence.chars().next().map(|ch| ch as u32)
        } else {
            None
        };
        if raw_codepoint.is_some_and(|codepoint| {
            Some(codepoint) == self.pending_kitty_printable_codepoint
                && self
                    .pending_kitty_printable_at
                    .is_some_and(|at| now.duration_since(at).as_millis() <= KITTY_PRINTABLE_DEDUP_WINDOW_MS)
        }) {
            self.pending_kitty_printable_codepoint = None;
            return;
        }
        self.pending_kitty_printable_codepoint =
            parse_unmodified_kitty_printable_codepoint(sequence);
        if self.pending_kitty_printable_codepoint.is_some() {
            self.pending_kitty_printable_at = Some(now);
        }
        events.push(StdinBufferEvent::Data(sequence.to_string()));
    }

    fn arm_flush_timer(&mut self) {
        self.flush_deadline = Some(Instant::now() + Duration::from_millis(self.timeout_ms));
    }

    fn should_hold_partial(&self) -> bool {
        sgr_mouse_partial(&self.buffer) || self.kitty_protocol_active
    }

    fn buffer_is_string_head(&self) -> bool {
        if !string_seq_partial(&self.buffer) {
            return false;
        }
        if self.kitty_protocol_active {
            return true;
        }
        self.buffer.len() > 2 && legacy_string_reply_opener(&self.buffer)
    }

    fn track_string_read(&mut self, str: &str) {
        let now = Instant::now();
        let stalled = self
            .string_progress_at
            .map(|at| now.duration_since(at).as_millis() >= self.timeout_ms as u128)
            .unwrap_or(true);
        self.note_string_progress();
        let code = str.chars().next().map(|ch| ch as u32);
        let utf16_length = str.encode_utf16().count();
        if !stalled
            || utf16_length != 1
            || code.is_none_or(|code| code < 0x20 || code == 0x7f)
        {
            return;
        }
        let candidate = format!("{}{}", self.buffer, str);
        let end = resolve_string_end(
            candidate.as_bytes(),
            0,
            candidate.len(),
            self.buffer.len(),
            self.buffer.as_bytes().get(1) == Some(&0x5d),
        );
        if end != -1 {
            return;
        }
        self.string_typed_keys.push(str.to_string());
        self.string_typed_at.push(self.buffer.len());
    }

    fn note_string_progress(&mut self) {
        self.string_progress_at = Some(Instant::now());
        self.string_abandon_deadline =
            Some(Instant::now() + Duration::from_millis(STRING_ABANDON_MS));
    }

    fn take_string_typed_keys(&mut self) -> Vec<String> {
        if self.string_progress_at.is_none() {
            return Vec::new();
        }
        self.string_progress_at = None;
        self.string_abandon_deadline = None;
        self.string_typed_at.clear();
        std::mem::take(&mut self.string_typed_keys)
    }

    fn emit_string_typed_keys(&mut self, events: &mut Vec<StdinBufferEvent>) {
        for keys in self.take_string_typed_keys() {
            for sequence in extract_complete_sequences(&keys, 0).sequences {
                self.emit_data_sequence(&sequence, events);
            }
        }
    }

    fn on_string_abandon(&mut self, now: Instant, events: &mut Vec<StdinBufferEvent>) {
        if !self.buffer_is_string_head() {
            self.take_string_typed_keys();
            return;
        }
        let mut dormant = self.buffer.clone();
        for index in (0..self.string_typed_at.len()).rev() {
            let at = self.string_typed_at[index];
            let key_length = self.string_typed_keys[index].len();
            dormant.replace_range(at..at + key_length, "");
        }
        if self.string_resumed {
            self.dormant_string = None;
        } else {
            self.dormant_string = Some(dormant);
            self.dormant_until = Some(now + Duration::from_millis(STRING_DORMANT_MS));
        }
        self.buffer.clear();
        self.escape_search_offset = 0;
        self.emit_string_typed_keys(events);
    }

    fn resume_dormant_string(&mut self, str: &str) {
        let head = self.dormant_string.clone().unwrap_or_default();
        let candidate = format!("{}{}", head, str);
        let end = resolve_string_end(
            candidate.as_bytes(),
            0,
            candidate.len(),
            head.len(),
            head.as_bytes().get(1) == Some(&0x5d),
        );
        let code = str.chars().next().map(|ch| ch as u32);
        let held_st = str == "\x1b" && self.kitty_protocol_active;
        if end <= 0 && !held_st && str.encode_utf16().count() == 1 {
            if code.is_some_and(|code| code < 0x20 || code == 0x7f) {
                self.dormant_string = None;
            }
            return;
        }
        self.dormant_string = None;
        if end <= STRING_ABORTED {
            return;
        }
        self.buffer = head;
        self.escape_search_offset = self.buffer.len();
        self.string_resumed = true;
    }

    fn flush_expired(&mut self, now: Instant, events: &mut Vec<StdinBufferEvent>) {
        if self.buffer.is_empty() {
            self.partial_hold_start = None;
            return;
        }
        if self.should_hold_partial() {
            let start = self.partial_hold_start.unwrap_or(now);
            self.partial_hold_start = Some(start);
            if now.duration_since(start).as_millis() < self.partial_hold_max_ms {
                self.arm_flush_timer();
                return;
            }
        }
        self.partial_hold_start = None;
        for sequence in self.drain_buffered() {
            self.emit_data_sequence(&sequence, events);
        }
    }

    /// Drain buffered sequences without waiting for timers.
    pub fn flush(&mut self) -> Vec<String> {
        self.drain_buffered()
    }

    fn drain_buffered(&mut self) -> Vec<String> {
        self.flush_deadline = None;
        let raw_candidate = self.take_raw_paste_candidate();
        let mut sequences = if raw_candidate.is_empty() {
            Vec::new()
        } else {
            extract_complete_sequences(&raw_candidate, 0).sequences
        };
        if self.buffer.is_empty() {
            self.pending_kitty_printable_codepoint = None;
            return sequences;
        }
        let string_head = self.buffer_is_string_head();
        let buffered = std::mem::take(&mut self.buffer);
        self.escape_search_offset = 0;
        self.pending_kitty_printable_codepoint = None;
        if buffered == "\x1b\x1b" {
            sequences.push("\x1b".to_string());
            sequences.push("\x1b".to_string());
        } else if string_head {
            self.take_string_typed_keys();
        } else {
            sequences.push(buffered);
        }
        sequences
    }

    fn enter_string_discard(&mut self) {
        self.string_discard_active = true;
        self.string_discard_bytes = 0;
        self.string_discard_esc_held = false;
        self.arm_string_discard_watchdog();
    }

    fn exit_string_discard(&mut self) {
        self.string_discard_active = false;
        self.string_discard_bytes = 0;
        self.string_discard_esc_held = false;
        self.string_discard_deadline = None;
    }

    fn consume_string_discard(&mut self, str: &str) -> String {
        let bytes = str.as_bytes();
        if self.string_discard_esc_held {
            self.string_discard_esc_held = false;
            if bytes.first() == Some(&0x5c) {
                self.exit_string_discard();
                return str[1..].to_string();
            }
        }
        let mut index = 0;
        while index < bytes.len() {
            let code = bytes[index];
            if code == 0x07 {
                self.exit_string_discard();
                return str[index + 1..].to_string();
            }
            if code == 0x1b {
                if index + 1 == bytes.len() {
                    self.string_discard_esc_held = true;
                    break;
                }
                if bytes[index + 1] == 0x5c {
                    self.exit_string_discard();
                    return str[index + 2..].to_string();
                }
            }
            index += 1;
        }
        self.string_discard_bytes += bytes.len();
        if self.string_discard_bytes > STRING_DISCARD_MAX_BYTES {
            self.exit_string_discard();
            return String::new();
        }
        self.arm_string_discard_watchdog();
        String::new()
    }

    fn arm_string_discard_watchdog(&mut self) {
        self.string_discard_deadline =
            Some(Instant::now() + Duration::from_millis(STRING_DISCARD_INACTIVITY_MS));
    }

    /// Reset all buffering and classification state.
    pub fn clear(&mut self) {
        self.flush_deadline = None;
        self.paste_deadline = None;
        self.raw_paste_deadline = None;
        self.exit_string_discard();
        self.take_string_typed_keys();
        self.dormant_string = None;
        self.string_resumed = false;
        self.buffer.clear();
        self.raw_paste_candidate.clear();
        self.paste_mode = false;
        self.paste_chunks.clear();
        self.paste_bytes = 0;
        self.pending_kitty_printable_codepoint = None;
        self.partial_hold_start = None;
        self.escape_search_offset = 0;
    }

    /// Alias of [`StdinBuffer::clear`].
    pub fn destroy(&mut self) {
        self.clear();
    }
}

fn find_byte(haystack: &[u8], needle: u8) -> Option<usize> {
    haystack.iter().position(|&byte| byte == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassembles_split_sgr_mouse_report() {
        let mut buffer = StdinBuffer::default();
        assert!(buffer.process("\x1b").is_empty());
        assert!(buffer.process("[<35").is_empty());
        let events = buffer.process(";20;5m");
        assert_eq!(
            events,
            vec![StdinBufferEvent::Data("\x1b[<35;20;5m".to_string())]
        );
    }

    #[test]
    fn delivers_complete_sequences_immediately() {
        let mut buffer = StdinBuffer::default();
        assert_eq!(
            buffer.process("a"),
            vec![StdinBufferEvent::Data("a".to_string())]
        );
        assert_eq!(
            buffer.process("\x1b[A"),
            vec![StdinBufferEvent::Data("\x1b[A".to_string())]
        );
    }

    #[test]
    fn bracketed_paste_emits_content_and_trailing_enter() {
        let mut buffer = StdinBuffer::default();
        let events = buffer.process("\x1b[200~hello\r\x1b[201~\r");
        assert_eq!(
            events,
            vec![StdinBufferEvent::Paste {
                content: "hello\r".to_string(),
                enter: Some("\r".to_string())
            }]
        );
        assert!(!buffer.pasting());
    }

    #[test]
    fn kitty_printable_deduplicates_same_write() {
        let mut buffer = StdinBuffer::default();
        let first = buffer.process("\x1b[97u");
        assert_eq!(first, vec![StdinBufferEvent::Data("\x1b[97u".to_string())]);
        assert_eq!(buffer.process("a"), Vec::new());
        let mut later = StdinBuffer::new(StdinBufferOptions {
            timeout: 0,
            ..Default::default()
        });
        assert_eq!(
            later.process("\x1b[97u"),
            vec![StdinBufferEvent::Data("\x1b[97u".to_string())]
        );
    }

    #[test]
    fn raw_multiline_burst_is_a_paste() {
        let mut buffer = StdinBuffer::default();
        let events = buffer.process("one\ntwo\nthree");
        assert_eq!(
            events,
            vec![StdinBufferEvent::Paste {
                content: "one\ntwo\nthree".to_string(),
                enter: None
            }]
        );
    }

    #[test]
    fn flush_delivers_buffered_partial() {
        let mut buffer = StdinBuffer::default();
        assert!(buffer.process("\x1b[1").is_empty());
        assert_eq!(buffer.flush(), vec!["\x1b[1".to_string()]);
        assert!(buffer.flush().is_empty());
    }

    #[test]
    fn torn_string_is_discarded_until_terminator() {
        let mut buffer = StdinBuffer::default();
        let events = buffer.process("\x1b]5522;read:status=DATA:mime=text/plain;payload=AAAA");
        assert!(events.is_empty());
        let events = buffer.process("BBBB\x1b\\tail");
        assert_eq!(events.len(), 5);
        assert!(matches!(&events[0], StdinBufferEvent::Data(sequence) if sequence.ends_with("payload=AAAABBBB\x1b\\")));
        assert_eq!(
            events[1..],
            vec![
                StdinBufferEvent::Data("t".to_string()),
                StdinBufferEvent::Data("a".to_string()),
                StdinBufferEvent::Data("i".to_string()),
                StdinBufferEvent::Data("l".to_string()),
            ]
        );
    }

    #[test]
    fn tick_flushes_expired_partial() {
        let mut buffer = StdinBuffer::default();
        assert!(buffer.process("\x1b[1").is_empty());
        buffer.flush_deadline = Some(Instant::now() - Duration::from_millis(1));
        let events = buffer.tick();
        assert_eq!(events, vec![StdinBufferEvent::Data("\x1b[1".to_string())]);
    }

    #[test]
    fn double_escape_splits_on_flush() {
        let mut buffer = StdinBuffer::default();
        assert!(buffer.process("\x1b\x1b").is_empty());
        let sequences = buffer.flush();
        assert_eq!(sequences, vec!["\x1b".to_string(), "\x1b".to_string()]);
    }

    #[test]
    fn paste_watchdog_recovers_truncated_paste() {
        let mut buffer = StdinBuffer::default();
        let events = buffer.process("\x1b[200~hello");
        assert!(events.is_empty());
        assert!(buffer.pasting());
        buffer.paste_deadline = Some(Instant::now() - Duration::from_millis(1));
        let events = buffer.tick();
        assert_eq!(
            events,
            vec![StdinBufferEvent::Paste {
                content: "hello".to_string(),
                enter: None
            }]
        );
        assert!(!buffer.pasting());
    }

    #[test]
    fn sgr_prefix_is_held_past_timeout() {
        let mut buffer = StdinBuffer::default();
        assert!(buffer.process("\x1b[<35").is_empty());
        buffer.flush_deadline = Some(Instant::now() - Duration::from_millis(1));
        let events = buffer.tick();
        assert!(events.is_empty());
        assert_eq!(buffer.get_buffer(), "\x1b[<35");
    }

    #[test]
    fn raw_paste_window_flushes_candidate_as_keys() {
        let mut buffer = StdinBuffer::default();
        assert!(buffer.process("a\n").is_empty());
        assert_eq!(buffer.get_buffer(), "a\n");
        buffer.raw_paste_deadline = Some(Instant::now() - Duration::from_millis(1));
        let events = buffer.tick();
        assert_eq!(
            events,
            vec![
                StdinBufferEvent::Data("a".to_string()),
                StdinBufferEvent::Data("\n".to_string()),
            ]
        );
        assert!(buffer.get_buffer().is_empty());
    }

    #[test]
    fn abandoned_string_replays_typed_key_and_resumes_late_tail() {
        let head = "\x1b]5522;read:status=DATA:mime=text/plain;payload=AAAA";
        let mut buffer = StdinBuffer::default();
        assert!(buffer.process(head).is_empty());
        buffer.string_progress_at = Some(Instant::now() - Duration::from_millis(100));
        assert!(buffer.process("x").is_empty());
        buffer.string_abandon_deadline = Some(Instant::now() - Duration::from_millis(1));
        let events = buffer.tick();
        assert_eq!(events, vec![StdinBufferEvent::Data("x".to_string())]);
        let events = buffer.process("\x1b\\");
        assert_eq!(events, vec![StdinBufferEvent::Data(format!("{head}\x1b\\"))]);
    }

    #[test]
    fn torn_string_discard_swallows_until_terminator() {
        let mut buffer = StdinBuffer::default();
        buffer.enter_string_discard();
        assert!(buffer.string_discard_active);
        assert_eq!(buffer.consume_string_discard("garbage"), "");
        assert_eq!(buffer.consume_string_discard("\x1b"), "");
        assert_eq!(buffer.consume_string_discard("\\rest"), "rest");
        assert!(!buffer.string_discard_active);
        buffer.enter_string_discard();
        assert_eq!(buffer.consume_string_discard("\x07rest"), "rest");
        assert!(!buffer.string_discard_active);
    }

    #[test]
    fn clear_resets_buffering_state() {
        let mut buffer = StdinBuffer::default();
        buffer.process("\x1b[1");
        buffer.process("a\n");
        buffer.clear();
        assert!(buffer.get_buffer().is_empty());
        assert!(buffer.flush().is_empty());
        assert!(!buffer.pasting());
    }
}
