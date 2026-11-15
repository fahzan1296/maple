//! Decoded keyboard events plus pi-tui-style predicates.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
