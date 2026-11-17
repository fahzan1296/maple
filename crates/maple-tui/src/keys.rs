//! Decoded keyboard events plus pi-tui-style predicates.
//! Reference: `l/src/keys.ts`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Normalized key identifier (port of the TS `KeyId` string union).
pub type KeyId = String;

/// A normalized key press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Alt(char),
    Shift(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Paste(String),
    CtrlUp,
    CtrlDown,
    CtrlLeft,
    CtrlRight,
    Up,
    Down,
    Left,
    Right,
    F(u8),
    Unknown,
}

impl Key {
    pub fn ctrl(ch: char) -> Key {
        Key::Ctrl(ch.to_ascii_lowercase())
    }

    pub fn alt(ch: char) -> Key {
        Key::Alt(ch)
    }

    /// Printable character produced by this key, ignoring modifiers.
    pub fn as_char(&self) -> Option<char> {
        match self {
            Key::Char(c) | Key::Shift(c) => Some(*c),
            _ => None,
        }
    }

    pub fn is_ctrl(&self) -> bool {
        matches!(self, Key::Ctrl(_))
    }

    pub fn is_alt(&self) -> bool {
        matches!(self, Key::Alt(_))
    }
}

/// Decode a crossterm key event. Release/repeat events are filtered upstream.
pub fn from_crossterm(event: KeyEvent) -> Key {
    let modifiers = event.modifiers;
    let code = event.code;
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(ch) = code {
            return Key::Ctrl(if modifiers.contains(KeyModifiers::SHIFT) {
                ch
            } else {
                ch.to_ascii_lowercase()
            });
        }
        return match code {
            KeyCode::Up => Key::CtrlUp,
            KeyCode::Down => Key::CtrlDown,
            KeyCode::Left => Key::CtrlLeft,
            KeyCode::Right => Key::CtrlRight,
            _ => decode_code(code, modifiers),
        };
    }
    if modifiers.contains(KeyModifiers::ALT) {
        return match code {
            KeyCode::Enter => Key::Alt('\r'),
            KeyCode::Char(ch) => Key::Alt(ch),
            KeyCode::Backspace => Key::Alt('\x7f'),
            _ => decode_code(code, modifiers),
        };
    }
    decode_code(code, modifiers)
}

fn decode_code(code: KeyCode, modifiers: KeyModifiers) -> Key {
    match code {
        KeyCode::Char(ch) => {
            if modifiers.contains(KeyModifiers::SHIFT) && ch.is_ascii_lowercase() {
                Key::Shift(ch.to_ascii_uppercase())
            } else {
                Key::Char(ch)
            }
        }
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::F(number) => Key::F(number),
        KeyCode::Null => Key::Unknown,
        _ => Key::Unknown,
    }
}

pub fn is_enter(key: &Key) -> bool {
    matches!(key, Key::Enter | Key::Alt('\r'))
}
pub fn is_escape(key: &Key) -> bool {
    matches!(key, Key::Esc)
}
pub fn is_tab(key: &Key) -> bool {
    matches!(key, Key::Tab)
}
pub fn is_shift_tab(key: &Key) -> bool {
    matches!(key, Key::BackTab)
}
pub fn is_backspace(key: &Key) -> bool {
    matches!(key, Key::Backspace)
}
pub fn is_delete(key: &Key) -> bool {
    matches!(key, Key::Delete)
}
pub fn is_arrow_up(key: &Key) -> bool {
    matches!(key, Key::Up)
}
pub fn is_arrow_down(key: &Key) -> bool {
    matches!(key, Key::Down)
}
pub fn is_arrow_left(key: &Key) -> bool {
    matches!(key, Key::Left)
}
pub fn is_arrow_right(key: &Key) -> bool {
    matches!(key, Key::Right)
}
pub fn is_home(key: &Key) -> bool {
    matches!(key, Key::Home)
}
pub fn is_end(key: &Key) -> bool {
    matches!(key, Key::End)
}
pub fn is_page_up(key: &Key) -> bool {
    matches!(key, Key::PageUp)
}
pub fn is_page_down(key: &Key) -> bool {
    matches!(key, Key::PageDown)
}
pub fn is_ctrl_a(key: &Key) -> bool {
    matches!(key, Key::Ctrl('a'))
}
pub fn is_ctrl_c(key: &Key) -> bool {
    matches!(key, Key::Ctrl('c'))
}
pub fn is_ctrl_d(key: &Key) -> bool {
    matches!(key, Key::Ctrl('d'))
}
pub fn is_ctrl_e(key: &Key) -> bool {
    matches!(key, Key::Ctrl('e'))
}
pub fn is_ctrl_k(key: &Key) -> bool {
    matches!(key, Key::Ctrl('k'))
}
pub fn is_ctrl_o(key: &Key) -> bool {
    matches!(key, Key::Ctrl('o'))
}
pub fn is_ctrl_p(key: &Key) -> bool {
    matches!(key, Key::Ctrl('p'))
}
pub fn is_ctrl_u(key: &Key) -> bool {
    matches!(key, Key::Ctrl('u'))
}
pub fn is_ctrl_w(key: &Key) -> bool {
    matches!(key, Key::Ctrl('w'))
}
pub fn is_ctrl_left(key: &Key) -> bool {
    matches!(key, Key::CtrlLeft | Key::Ctrl('h'))
}
pub fn is_ctrl_right(key: &Key) -> bool {
    matches!(key, Key::CtrlRight | Key::Ctrl('l'))
}
pub fn is_alt_left(key: &Key) -> bool {
    matches!(key, Key::Alt('h'))
}
pub fn is_alt_right(key: &Key) -> bool {
    matches!(key, Key::Alt('l'))
}
pub fn is_shift_enter(key: &Key) -> bool {
    matches!(key, Key::Shift('\r'))
}
pub fn is_alt_enter(key: &Key) -> bool {
    matches!(key, Key::Alt('\r'))
}
pub fn is_f(key: &Key, number: u8) -> bool {
    matches!(key, Key::F(n) if *n == number)
}

// =============================================================================
// Raw terminal input parsing (pure parts of `l/src/keys.ts`)
// =============================================================================

use crate::terminal_capabilities::{is_inside_terminal_multiplexer, is_ssh_session, EnvReader};

const KITTY_LOCK_MASK: u32 = 64 + 128;
const KITTY_MOD_SHIFT: u32 = 1;
const KITTY_MOD_ALT: u32 = 2;
const KITTY_MOD_CTRL: u32 = 4;
const KITTY_MOD_SUPER: u32 = 8;

/// Whether the local process runs directly under Windows Terminal.
#[must_use]
pub fn is_windows_terminal_session(env: &impl EnvReader) -> bool {
	env.get("WT_SESSION").is_some_and(|value| !value.is_empty()) && !is_ssh_session(env)
}

/// Match ambiguous legacy Backspace bytes against an expected modifier mask.
///
/// Windows Terminal encodes Ctrl+Backspace as raw `0x08` and plain Backspace as
/// `0x7f`; the automatic heuristic is limited to direct Windows Terminal
/// sessions, and `PI_TUI_RAW_BACKSPACE_IS_CTRL=1` opts in everywhere.
#[must_use]
pub fn matches_raw_backspace(data: &str, expected_modifier: u32, env: &impl EnvReader) -> bool {
	if data == "\x7f" {
		return expected_modifier == 0;
	}
	if data != "\x08" {
		return false;
	}
	let raw_backspace_is_ctrl =
		env.get("PI_TUI_RAW_BACKSPACE_IS_CTRL").as_deref() == Some("1")
			|| (is_windows_terminal_session(env) && !is_inside_terminal_multiplexer(env));
	if raw_backspace_is_ctrl {
		expected_modifier == 4
	} else {
		expected_modifier == 0
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct KittyParts {
	codepoint: u32,
	shifted: Option<u32>,
	modifier: u32,
	event_type: Option<u32>,
	text: Option<Vec<u32>>,
}

/// Parse `\x1b[<codepoint>[:<shifted>[:<base>]][;<modifier>[:<event>]][;<text>]u`.
fn parse_kitty_csi_u(data: &str) -> Option<KittyParts> {
	let body = data.strip_prefix("\x1b[")?.strip_suffix('u')?;
	let fields: Vec<&str> = body.split(';').collect();
	let key_field = fields.first()?;
	let mut key_parts = key_field.split(':');
	let codepoint: u32 = key_parts.next()?.parse().ok()?;
	let shifted = key_parts
		.next()
		.filter(|part| !part.is_empty())
		.and_then(|part| part.parse::<u32>().ok());
	let (modifier, event_type) = match fields.get(1) {
		Some(field) => {
			let mut parts = field.split(':');
			let modifier = parts
				.next()
				.filter(|part| !part.is_empty())
				.and_then(|part| part.parse::<u32>().ok())
				.unwrap_or(1);
			let event_type = parts.next().and_then(|part| part.parse::<u32>().ok());
			(modifier, event_type)
		}
		None => (1, None),
	};
	let text = fields.get(2).and_then(|field| {
		let codepoints: Vec<u32> = field
			.split(':')
			.filter(|part| !part.is_empty())
			.filter_map(|part| part.parse().ok())
			.collect();
		if codepoints.is_empty() {
			None
		} else {
			Some(codepoints)
		}
	});
	Some(KittyParts {
		codepoint,
		shifted,
		modifier,
		event_type,
		text,
	})
}

fn keypad_operator_text(codepoint: u32) -> Option<&'static str> {
	match codepoint {
		57410 => Some("/"),
		57411 => Some("*"),
		57412 => Some("-"),
		57413 => Some("+"),
		57415 => Some("="),
		_ => None,
	}
}

fn numpad_text(codepoint: u32) -> Option<&'static str> {
	match codepoint {
		57399 => Some("0"),
		57400 => Some("1"),
		57401 => Some("2"),
		57402 => Some("3"),
		57403 => Some("4"),
		57404 => Some("5"),
		57405 => Some("6"),
		57406 => Some("7"),
		57407 => Some("8"),
		57408 => Some("9"),
		57409 => Some("."),
		_ => None,
	}
}

/// Decode a Kitty CSI-u sequence into the printable text it produces.
#[must_use]
pub fn decode_kitty_printable(data: &str) -> Option<String> {
	let parts = parse_kitty_csi_u(data)?;
	if parts.event_type == Some(3) {
		return None;
	}
	let modifier = parts.modifier.saturating_sub(1);
	let effective = modifier & !KITTY_LOCK_MASK;
	if effective & !(KITTY_MOD_SHIFT | KITTY_MOD_ALT | KITTY_MOD_CTRL | KITTY_MOD_SUPER) != 0 {
		return None;
	}
	if effective & (KITTY_MOD_ALT | KITTY_MOD_CTRL | KITTY_MOD_SUPER) != 0 {
		return None;
	}
	if let Some(codepoints) = parts.text {
		let text: String = codepoints
			.iter()
			.filter(|&&code| code >= 32 && code != 127)
			.filter_map(|&code| char::from_u32(code))
			.collect();
		if !text.is_empty() {
			return Some(text);
		}
	}
	if let Some(operator) = keypad_operator_text(parts.codepoint) {
		return Some(operator.to_string());
	}
	if effective == 0 {
		if let Some(digit) = numpad_text(parts.codepoint) {
			return Some(digit.to_string());
		}
	}
	let mut codepoint = parts.codepoint;
	if effective & KITTY_MOD_SHIFT != 0 {
		if let Some(shifted) = parts.shifted {
			codepoint = shifted;
		}
	}
	if (0xe000..=0xf8ff).contains(&codepoint) {
		return None;
	}
	if codepoint < 32 || codepoint == 127 {
		return None;
	}
	char::from_u32(codepoint).map(|ch| ch.to_string())
}

/// Decode a Kitty CSI-u keypad sequence (numpad digits / keypad operators) into
/// the text it produces, or `None` for any non-keypad sequence.
#[must_use]
pub fn decode_kitty_keypad_text(data: &str) -> Option<String> {
	let parts = parse_kitty_csi_u(data)?;
	if numpad_text(parts.codepoint).is_none() && keypad_operator_text(parts.codepoint).is_none() {
		return None;
	}
	decode_kitty_printable(data)
}

/// Decode an xterm `modifyOtherKeys` sequence (`CSI 27 ; modifiers ; keycode ~`)
/// into the printable character it represents. Only no-modifier or Shift-only
/// sequences produce text.
#[must_use]
pub fn decode_modify_other_keys_printable(data: &str) -> Option<String> {
	let body = data.strip_prefix("\x1b[27;")?.strip_suffix('~')?;
	let mut parts = body.split(';');
	let mod_value: u32 = parts.next()?.parse().ok()?;
	let codepoint: u32 = parts.next()?.parse().ok()?;
	if parts.next().is_some() {
		return None;
	}
	let modifier = mod_value.checked_sub(1)? & !KITTY_LOCK_MASK;
	if modifier & !KITTY_MOD_SHIFT != 0 {
		return None;
	}
	if codepoint < 32 || codepoint == 127 {
		return None;
	}
	char::from_u32(codepoint).map(|ch| ch.to_string())
}

/// Decode terminal input into the printable character it represents.
#[must_use]
pub fn decode_printable_key(data: &str) -> Option<String> {
	decode_kitty_printable(data).or_else(|| decode_modify_other_keys_printable(data))
}

fn has_control_chars(data: &str) -> bool {
	data.chars().any(|ch| {
		let code = ch as u32;
		code < 32 || code == 0x7f || (0x80..=0x9f).contains(&code)
	})
}

/// Extract printable text from raw terminal input.
#[must_use]
pub fn extract_printable_text(data: &str) -> Option<String> {
	if let Some(printable) = decode_printable_key(data) {
		return Some(printable);
	}
	if data.is_empty() || has_control_chars(data) {
		return None;
	}
	Some(data.to_string())
}
