use maple_tui::terminal::kitty_keyboard::{
    is_da1_reply, is_kitty_keyboard_reply, parse_kitty_keyboard_reply, KittyKeyboardProbe,
    KITTY_KEYBOARD_QUERY,
};
use maple_tui::terminal_capabilities::EnvReader;
use std::collections::HashMap;

struct Env {
    vars: HashMap<&'static str, String>,
}

impl Env {
    fn new(vars: &[(&'static str, &str)]) -> Self {
        Env {
            vars: vars
                .iter()
                .map(|(key, value)| (*key, (*value).to_string()))
                .collect(),
        }
    }
}

impl EnvReader for Env {
    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn is_tty(&self) -> bool {
        true
    }
}

fn writes(probe: &KittyKeyboardProbe) -> String {
    probe.writes().join("")
}

#[test]
fn query_asks_kitty_flags_then_da1_sentinel() {
    let mut probe = KittyKeyboardProbe::new();
    let out = probe.begin();
    assert_eq!(out, vec![KITTY_KEYBOARD_QUERY.to_string()]);
    assert!(writes(&probe).contains("\x1b[?u\x1b[c"));
    assert!(!probe.kitty_protocol_active());
    assert!(!probe.modify_other_keys_enabled());
}

#[test]
fn kitty_reply_before_da1_enables_alternate_keys() {
    let env = Env::new(&[]);
    let mut probe = KittyKeyboardProbe::new();
    probe.begin();
    probe.take_writes();

    assert!(probe.feed("\x1b[?0u", &env));
    assert!(probe.feed("\x1b[?1;2c", &env));

    let out = writes(&probe);
    assert!(probe.kitty_protocol_active());
    assert!(out.contains("\x1b[>5u"));
    assert!(!out.contains("\x1b[>4;2m"));
    assert!(!probe.modify_other_keys_enabled());
}

#[test]
fn da1_before_kitty_disables_the_fallback_it_armed() {
    let env = Env::new(&[]);
    let mut probe = KittyKeyboardProbe::new();
    probe.begin();
    probe.take_writes();

    assert!(probe.feed("\x1b[?1;2c", &env));
    assert!(probe.modify_other_keys_enabled());
    assert!(probe.feed("\x1b[?0u", &env));

    let out = writes(&probe);
    let enable = out.find("\x1b[>4;2m").expect("fallback enabled");
    let disable = out.find("\x1b[>4;0m").expect("fallback disabled");
    let kitty = out.find("\x1b[>5u").expect("kitty push");
    assert!(probe.kitty_protocol_active());
    assert!(disable > enable, "kitty must retire the fallback");
    assert!(kitty > enable);
    assert!(!probe.modify_other_keys_enabled());
}

#[test]
fn da1_only_keeps_modify_other_keys_fallback() {
    let env = Env::new(&[]);
    let mut probe = KittyKeyboardProbe::new();
    probe.begin();
    probe.take_writes();

    assert!(probe.feed("\x1b[?1;2c", &env));
    let out = writes(&probe);
    assert!(!probe.kitty_protocol_active());
    assert!(probe.modify_other_keys_enabled());
    assert!(out.contains("\x1b[>4;2m"));
    assert!(!out.contains("\x1b[>5u"));
}

#[test]
fn ssh_connection_only_unknown_terminal_skips_fallback() {
    let env = Env::new(&[("SSH_CONNECTION", "192.0.2.10 54321 192.0.2.20 22")]);
    let mut probe = KittyKeyboardProbe::new();
    probe.begin();
    probe.take_writes();

    assert!(probe.feed("\x1b[?1;2c", &env));
    let out = writes(&probe);
    assert!(!probe.kitty_protocol_active());
    assert!(!probe.modify_other_keys_enabled());
    assert!(!out.contains("\x1b[>4;2m"));
}

#[test]
fn tmux_allows_modify_other_keys_fallback() {
    let env = Env::new(&[("TMUX", "/tmp/tmux-501/default,1234,0")]);
    let mut probe = KittyKeyboardProbe::new();
    probe.begin();
    probe.take_writes();

    assert!(probe.feed("\x1b[?1;2c", &env));
    let out = writes(&probe);
    assert!(probe.modify_other_keys_enabled());
    assert!(out.contains("\x1b[>4;2m"));
    assert_eq!(probe.overlay_enter().as_deref(), Some("\x1b[>4;2m"));
    assert_eq!(probe.overlay_exit(), None);
}

#[test]
fn parent_event_types_are_preserved_in_kitty_flags() {
    let env = Env::new(&[]);
    let mut probe = KittyKeyboardProbe::new();
    probe.begin();
    probe.take_writes();

    assert!(probe.feed("\x1b[?3u", &env));
    assert!(probe.kitty_protocol_active());
    assert!(probe.event_types_enabled());
    assert!(writes(&probe).contains("\x1b[>7u"));
    assert_eq!(probe.overlay_enter().as_deref(), Some("\x1b[=7;1u"));
    assert_eq!(probe.overlay_exit().as_deref(), Some("\x1b[<u"));
}

#[test]
fn conpty_reports_limited_flags_and_pushes_them_per_screen() {
    let env = Env::new(&[]);
    let mut probe = KittyKeyboardProbe::new();
    probe.set_conpty(true);
    probe.begin();
    probe.take_writes();

    assert!(probe.feed("\x1b[?2u", &env));
    assert!(probe.kitty_protocol_active());
    assert!(writes(&probe).contains("\x1b[>3u"));
    assert_eq!(probe.overlay_enter().as_deref(), Some("\x1b[=3;1u"));
}

#[test]
fn reply_parsing_distinguishes_kitty_and_da1() {
    assert_eq!(parse_kitty_keyboard_reply("\x1b[?0u"), Some(0));
    assert_eq!(parse_kitty_keyboard_reply("\x1b[?5u"), Some(5));
    assert_eq!(parse_kitty_keyboard_reply("\x1b[?1;2c"), None);
    assert!(is_kitty_keyboard_reply("\x1b[?0u"));
    assert!(is_da1_reply("\x1b[?1;2c"));
    assert!(!is_da1_reply("\x1b[?0u"));
}
