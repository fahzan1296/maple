//! Kitty keyboard progressive-enhancement probe.
//!
//! Reference: `l/src/terminal.ts` (`#queryKittyKeyboardSupport`,
//! `#resolveKittyKeyboardSupport`, `#enableKittyKeyboard`,
//! `#enableModifyOtherKeysFallback`).
//!
//! The probe writes `CSI ? u` (kitty flags query) followed by a DA1 sentinel
//! (`CSI c`). The kitty reply authoritatively enables the protocol; DA1 only
//! guarantees a reply from terminals that ignore the query, and arms the
//! xterm `modifyOtherKeys` fallback. Reply ordering is not guaranteed, so the
//! state machine honors a kitty reply that arrives after DA1 and disables the
//! fallback it enabled in the meantime.
//!
//! Pure state machine: the bytes to write are collected in [`Self::writes`]
//! and the raw-stdin seam lives on `ProcessTerminal`, like the program-status
//! probe.

use crate::terminal_capabilities::EnvReader;

/// Query written at startup: kitty flags query plus DA1 sentinel.
pub const KITTY_KEYBOARD_QUERY: &str = "\x1b[?u\x1b[c";

const MODIFY_OTHER_KEYS_ENABLE: &str = "\x1b[>4;2m";
const MODIFY_OTHER_KEYS_DISABLE: &str = "\x1b[>4;0m";

/// Kitty flag bit: report event types.
const KITTY_FLAG_EVENT_TYPES: u32 = 2;
/// Kitty flag bit: disambiguate escape codes.
const KITTY_FLAG_DISAMBIGUATE: u32 = 1;
/// Kitty flag bit: report alternate keys.
const KITTY_FLAG_ALTERNATE_KEYS: u32 = 4;

/// Whether `sequence` is a kitty keyboard flags reply (`CSI ? <flags> u`).
pub fn is_kitty_keyboard_reply(sequence: &str) -> bool {
    parse_kitty_keyboard_reply(sequence).is_some()
}

/// Parse the flags from `CSI ? <flags> u`.
pub fn parse_kitty_keyboard_reply(sequence: &str) -> Option<u32> {
    let rest = sequence.strip_prefix("\x1b[?")?;
    let rest = rest.strip_suffix('u')?;
    rest.parse().ok()
}

/// Whether `sequence` is a DA1 reply (`CSI ? … c`).
pub fn is_da1_reply(sequence: &str) -> bool {
    sequence.starts_with("\x1b[?") && sequence.ends_with('c')
}

/// Kitty keyboard / modifyOtherKeys negotiation state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KittyKeyboardProbe {
    active: bool,
    event_types: bool,
    modify_other_keys: bool,
    da1_seen: bool,
    conpty: bool,
    last_flags: u32,
    writes: Vec<String>,
}

impl KittyKeyboardProbe {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the backend as a Windows ConPTY host, whose kitty flags are
    /// limited to disambiguate (plus event types a parent app enabled).
    pub fn set_conpty(&mut self, conpty: bool) {
        self.conpty = conpty;
    }

    /// Bytes to write at startup.
    pub fn begin(&mut self) -> Vec<String> {
        self.writes.push(KITTY_KEYBOARD_QUERY.to_string());
        vec![KITTY_KEYBOARD_QUERY.to_string()]
    }

    /// Consume one raw stdin sequence. Returns whether it belonged to the
    /// probe (kitty reply or DA1 sentinel).
    pub fn feed(&mut self, sequence: &str, env: &impl EnvReader) -> bool {
        if let Some(flags) = parse_kitty_keyboard_reply(sequence) {
            self.enable_kitty(flags);
            return true;
        }
        if is_da1_reply(sequence) {
            self.da1_seen = true;
            if !self.active && self.fallback_allowed(env) {
                self.modify_other_keys = true;
                self.writes.push(MODIFY_OTHER_KEYS_ENABLE.to_string());
            }
            return true;
        }
        false
    }

    fn enable_kitty(&mut self, flags: u32) {
        self.active = true;
        self.last_flags = flags;
        self.event_types = flags & KITTY_FLAG_EVENT_TYPES != 0;
        if self.modify_other_keys {
            self.modify_other_keys = false;
            self.writes.push(MODIFY_OTHER_KEYS_DISABLE.to_string());
        }
        self.writes.push(format!("\x1b[>{}u", self.push_flags()));
    }

    fn push_flags(&self) -> u32 {
        if self.conpty {
            self.last_flags | KITTY_FLAG_DISAMBIGUATE
        } else {
            KITTY_FLAG_DISAMBIGUATE
                | KITTY_FLAG_ALTERNATE_KEYS
                | (self.last_flags & KITTY_FLAG_EVENT_TYPES)
        }
    }

    fn fallback_allowed(&self, env: &impl EnvReader) -> bool {
        if env.get("TMUX").is_some_and(|value| !value.is_empty()) {
            return true;
        }
        let ssh_connection = env
            .get("SSH_CONNECTION")
            .is_some_and(|value| !value.is_empty());
        let ssh_tty = env.get("SSH_TTY").is_some_and(|value| !value.is_empty());
        let ssh_client = env
            .get("SSH_CLIENT")
            .is_some_and(|value| !value.is_empty());
        if ssh_connection && !ssh_tty && !ssh_client {
            return false;
        }
        true
    }

    /// Whether the kitty reply enabled the protocol.
    #[must_use]
    pub fn kitty_protocol_active(&self) -> bool {
        self.active
    }

    /// Whether the xterm modifyOtherKeys fallback is enabled.
    #[must_use]
    pub fn modify_other_keys_enabled(&self) -> bool {
        self.modify_other_keys
    }

    /// Whether a DA1 sentinel arrived.
    #[must_use]
    pub fn da1_seen(&self) -> bool {
        self.da1_seen
    }

    /// Whether event-type reporting was enabled by a parent app.
    #[must_use]
    pub fn event_types_enabled(&self) -> bool {
        self.event_types
    }

    /// Every byte string this probe has produced, in order.
    #[must_use]
    pub fn writes(&self) -> &[String] {
        &self.writes
    }

    /// Drain the collected writes.
    pub fn take_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.writes)
    }

    /// Flags to push when a fullscreen overlay enters the alternate screen.
    #[must_use]
    pub fn overlay_enter(&self) -> Option<String> {
        if self.active {
            Some(format!("\x1b[={};1u", self.push_flags()))
        } else if self.modify_other_keys {
            Some(MODIFY_OTHER_KEYS_ENABLE.to_string())
        } else {
            None
        }
    }

    /// Flags to pop when a fullscreen overlay leaves the alternate screen.
    /// modifyOtherKeys is a single global flag, so it is never popped.
    #[must_use]
    pub fn overlay_exit(&self) -> Option<String> {
        if self.active {
            Some("\x1b[<u".to_string())
        } else {
            None
        }
    }
}
