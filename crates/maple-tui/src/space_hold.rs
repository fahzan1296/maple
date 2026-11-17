//! Push-to-talk key-hold gesture (port of `l/src/space-hold.ts`).
//!
//! Reference: `l/src/space-hold.ts`.
//!
//! Deviations forced by Rust:
//!
//! - The reference is driven by `setTimeout`/`clearTimeout` and
//!   `performance.now()`. The crate has no timer queue, so the release timer
//!   is a deadline ([`std::time::Instant`]); the host event loop must call
//!   [`SpaceHoldGesture::tick`] each iteration to fire it. `tick` reproduces
//!   the reference's late-firing (event-loop stall) re-arm.
//! - The reference constructor's `allowed` parameter defaults to `() => true`;
//!   Rust has no default arguments, so [`SpaceHoldGesture::new`] omits it and
//!   [`SpaceHoldGesture::with_allowed`] takes it.
//! - `parseKey`/`extractPrintableText` in `l/src/keys.ts` shell out to a native
//!   parser. The crate has no raw-input key parser, so this module carries a
//!   small decoder for the sequences a push-to-talk key can arrive as: literal
//!   text, C0 control bytes, kitty CSI-u (including keypad operators/numpad),
//!   and xterm `modifyOtherKeys`. Unrecognized escape sequences return `None`,
//!   matching the reference's "not a recognized key" result.

use crate::keys::{decode_kitty_keypad_text, decode_modify_other_keys_printable, extract_printable_text};
use crate::keybindings::canonical_key_id;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Max gap (ms) between two configured-key presses for a later one to count as
/// OS key auto-repeat rather than a deliberate press.
pub const SPACE_REPEAT_MAX_GAP_MS: f64 = 120.0;
/// Absolute jitter floor for two consecutive gaps to look machine-driven.
pub const SPACE_REPEAT_JITTER_MS: f64 = 18.0;
/// Proportional jitter allowance for slower repeat rates.
pub const SPACE_REPEAT_JITTER_RATIO: f64 = 0.35;
/// Consecutive mechanical (fast + steady) deltas that confirm a held key.
pub const SPACE_HOLD_MECHANICAL_RUN: usize = 2;
/// Idle gap (ms) after the last repeated configured-key press that ends the hold.
pub const SPACE_HOLD_RELEASE_MS: u64 = 250;
/// How far past its deadline a release may fire before it reads as a stall.
pub const SPACE_HOLD_STALL_SLACK_MS: u64 = 100;


/// Printable text a push-to-talk candidate key types (plain or shifted single
/// characters and Space), or `None` for non-printable keys and chords.
pub fn get_space_hold_text(data: &str, canonical: Option<&str>) -> Option<String> {
    let canonical = canonical?;
    let shifted = canonical.starts_with("shift+");
    let base = if shifted {
        &canonical["shift+".len()..]
    } else {
        canonical
    };
    if base != "space" && base.chars().count() != 1 {
        return None;
    }
    if base == "space" && canonical != "space" && !shifted {
        return None;
    }
    let text = extract_printable_text(data)?;
    if base != "space" || text == " " {
        Some(text)
    } else {
        None
    }
}

/// Whether two consecutive inter-key gaps look machine-driven.
fn gaps_are_mechanical(gap: f64, prev_gap: f64) -> bool {
    if gap > SPACE_REPEAT_MAX_GAP_MS || prev_gap > SPACE_REPEAT_MAX_GAP_MS {
        return false;
    }
    let tolerance =
        SPACE_REPEAT_JITTER_MS.max(gap.min(prev_gap) * SPACE_REPEAT_JITTER_RATIO);
    (gap - prev_gap).abs() <= tolerance
}

/// What a recognized push-to-talk hold drives.
pub trait SpaceHoldHandler {
    /// Gate for the gesture; `false` lets matching keys behave normally.
    fn enabled(&self) -> bool;
    /// A sustained hold was recognized; typed candidate text is already deleted.
    fn on_start(&mut self);
    /// The held key was released.
    fn on_end(&mut self);
}

/// What the host must do with a keypress after [`SpaceHoldGesture::process`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceHoldStep {
    /// Handle it normally.
    Pass,
    /// Insert its printable text now.
    Type,
    /// Drop it; the gesture consumed it.
    Swallow,
}

impl SpaceHoldStep {
    /// The reference's string union value.
    pub fn as_str(self) -> &'static str {
        match self {
            SpaceHoldStep::Pass => "pass",
            SpaceHoldStep::Type => "type",
            SpaceHoldStep::Swallow => "swallow",
        }
    }
}

/// Push-to-talk key-hold state machine.
pub struct SpaceHoldGesture {
    keys: Vec<String>,
    key_aliases: HashSet<String>,
    handler: Option<Box<dyn SpaceHoldHandler>>,
    delete_typed: Box<dyn Fn(usize)>,
    allowed: Box<dyn Fn() -> bool>,
    typed: usize,
    last_typed_length: usize,
    run_key: Option<String>,
    mechanical_run: usize,
    prev_gap: Option<f64>,
    last_key_at: Option<Instant>,
    active_key: Option<String>,
    active: bool,
    release_armed_at: Option<Instant>,
    release_deadline: Option<Instant>,
}

impl SpaceHoldGesture {
    /// Create a gesture; every host state allows it.
    pub fn new(delete_typed: impl Fn(usize) + 'static) -> Self {
        Self::with_allowed(delete_typed, || true)
    }

    /// Create a gesture gated by `allowed` (the reference's second parameter).
    pub fn with_allowed(
        delete_typed: impl Fn(usize) + 'static,
        allowed: impl Fn() -> bool + 'static,
    ) -> Self {
        let mut gesture = SpaceHoldGesture {
            keys: vec!["space".to_string()],
            key_aliases: HashSet::new(),
            handler: None,
            delete_typed: Box::new(delete_typed),
            allowed: Box::new(allowed),
            typed: 0,
            last_typed_length: 0,
            run_key: None,
            mechanical_run: 0,
            prev_gap: None,
            last_key_at: None,
            active_key: None,
            active: false,
            release_armed_at: None,
            release_deadline: None,
        };
        gesture.rebuild_aliases();
        gesture
    }

    /// Configured alternative keys.
    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Replace the configured keys; aliases rebuild with them.
    pub fn set_keys(&mut self, keys: &[String]) {
        self.keys = keys.to_vec();
        self.rebuild_aliases();
    }

    /// The current handler, if any.
    pub fn handler(&self) -> Option<&dyn SpaceHoldHandler> {
        self.handler.as_deref()
    }

    /// Install or clear the handler.
    pub fn set_handler(&mut self, handler: Option<Box<dyn SpaceHoldHandler>>) {
        self.handler = handler;
    }

    /// Whether a recognized push-to-talk recording is in progress.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Fire a due release timer. The host event loop calls this each iteration;
    /// the reference armed a `setTimeout` instead.
    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    fn tick_at(&mut self, now: Instant) {
        let Some(armed_at) = self.release_armed_at else {
            return;
        };
        let deadline = self.release_deadline.unwrap_or(armed_at);
        if now < deadline {
            return;
        }
        let late = now.duration_since(armed_at).as_millis() as u64
            > SPACE_HOLD_RELEASE_MS + SPACE_HOLD_STALL_SLACK_MS;
        if late {
            self.arm_release_timer_at(now);
        } else {
            self.end();
        }
    }

    /// Pure focused-owner preflight on raw input: whether the key must reach the
    /// host before TUI-wide input listeners.
    pub fn should_route(&self, data: &str) -> bool {
        let handler_enabled = self.handler.as_ref().is_some_and(|handler| handler.enabled());
        if !(handler_enabled && (self.allowed)()) {
            return false;
        }
        let Some(parsed) = parse_key(data) else {
            return false;
        };
        let key = canonicalize_key_id(&parsed);
        self.key_aliases.contains(&key) && get_space_hold_text(data, Some(&key)).is_none()
    }

    /// Feed one canonical keypress and, when safely printable, its decoded text
    /// length.
    pub fn process(&mut self, key: Option<&str>, inserted_character_length: usize) -> SpaceHoldStep {
        self.process_at(key, inserted_character_length, Instant::now())
    }

    fn process_at(
        &mut self,
        key: Option<&str>,
        inserted_character_length: usize,
        now: Instant,
    ) -> SpaceHoldStep {
        let handler_enabled = self.handler.as_ref().is_some_and(|handler| handler.enabled());
        if !(handler_enabled && (self.allowed)()) {
            if self.active {
                self.end();
            } else {
                self.reset_run();
            }
            return SpaceHoldStep::Pass;
        }

        let candidate_key = key
            .filter(|key| self.key_aliases.contains(*key))
            .map(str::to_string);

        if self.active {
            if candidate_key.as_deref() == self.active_key.as_deref() {
                // Auto-repeat while the same configured key is held.
                self.arm_release_timer_at(now);
                return SpaceHoldStep::Swallow;
            }
            // Any other key means the active key was released.
            self.end();
        }

        let Some(candidate_key) = candidate_key else {
            self.reset_run();
            return SpaceHoldStep::Pass;
        };

        if self.run_key.as_deref().is_some_and(|run| run != candidate_key) {
            self.reset_run();
        }
        let gap = self.last_key_at.map(|at| duration_ms(now - at));
        let has_previous_key = self.last_key_at.is_some();
        let prev_gap = self.prev_gap;
        self.last_key_at = Some(now);
        self.run_key = Some(candidate_key.clone());
        let printable = inserted_character_length > 0;

        if !has_previous_key {
            self.mechanical_run = 0;
            self.typed = 0;
            self.last_typed_length = 0;
            return if printable {
                SpaceHoldStep::Type
            } else {
                SpaceHoldStep::Swallow
            };
        }

        let gap = gap.unwrap_or(0.0);
        self.prev_gap = Some(gap);
        if prev_gap.is_none() {
            // The second event establishes the first interval.
            self.mechanical_run = 0;
            if gap > SPACE_REPEAT_MAX_GAP_MS {
                self.typed = 0;
                self.last_typed_length = 0;
            }
            return if printable {
                SpaceHoldStep::Type
            } else {
                SpaceHoldStep::Swallow
            };
        }
        let prev_gap = prev_gap.unwrap_or(0.0);

        if !gaps_are_mechanical(gap, prev_gap) {
            self.mechanical_run = 0;
            if gap <= SPACE_REPEAT_MAX_GAP_MS && prev_gap > SPACE_REPEAT_MAX_GAP_MS {
                self.typed = self.last_typed_length;
            } else {
                self.typed = 0;
                self.last_typed_length = 0;
            }
            return if printable {
                SpaceHoldStep::Type
            } else {
                SpaceHoldStep::Swallow
            };
        }

        // Steady fast repeat: swallow it.
        self.mechanical_run += 1;
        if self.mechanical_run >= SPACE_HOLD_MECHANICAL_RUN {
            let typed = self.typed;
            (self.delete_typed)(typed);
            self.reset_run();
            self.active = true;
            self.active_key = Some(candidate_key);
            self.arm_release_timer_at(now);
            if let Some(handler) = &mut self.handler {
                handler.on_start();
            }
        }
        SpaceHoldStep::Swallow
    }

    /// Account for text the host actually inserted for the preceding `type` step.
    pub fn record_typed(&mut self, inserted_character_length: usize) {
        self.typed += inserted_character_length;
        self.last_typed_length = inserted_character_length;
    }

    fn rebuild_aliases(&mut self) {
        let mut aliases = HashSet::new();
        for key in &self.keys {
            add_key_aliases(&mut aliases, key);
        }
        self.key_aliases = aliases;
    }

    fn reset_run(&mut self) {
        self.last_typed_length = 0;
        self.typed = 0;
        self.run_key = None;
        self.mechanical_run = 0;
        self.prev_gap = None;
        self.last_key_at = None;
    }

    fn arm_release_timer_at(&mut self, now: Instant) {
        self.release_armed_at = Some(now);
        self.release_deadline = Some(now + Duration::from_millis(SPACE_HOLD_RELEASE_MS));
    }

    fn end(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        self.active_key = None;
        self.reset_run();
        self.release_armed_at = None;
        self.release_deadline = None;
        if let Some(handler) = &mut self.handler {
            handler.on_end();
        }
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Add a key's canonical id (plus its shifted-symbol alias) to `keys`.
fn add_key_aliases(keys: &mut HashSet<String>, key: &str) {
    let canonical = canonicalize_key_id(key);
    keys.insert(canonical.clone());
    if SHIFTED_SYMBOL_KEYS.contains(&canonical.as_str()) {
        keys.insert(format!("shift+{canonical}"));
    }
}

/// Keys whose shifted form is a distinct canonical id.
const SHIFTED_SYMBOL_KEYS: [&str; 20] = [
    "!", "@", "#", "$", "%", "^", "&", "*", "(", ")", "_", "+", "{", "}", "|", ":", "<", ">",
    "?", "~",
];

/// Canonicalize a key id string (`Ctrl+D` → `ctrl+d`, `esc` → `escape`).
fn canonicalize_key_id(id: &str) -> String {
    let mut parts: Vec<&str> = id.split('+').collect();
    let base = parts.pop().unwrap_or("");
    canonical_key_id(&parts, base)
}


/// Parse terminal input into a normalized key id.
fn parse_key(data: &str) -> Option<String> {
    if data == "\x7f" {
        return Some("backspace".to_string());
    }
    if data == "\x08" {
        return Some(
            if raw_backspace_is_ctrl() {
                "ctrl+backspace"
            } else {
                "backspace"
            }
            .to_string(),
        );
    }
    if let Some(text) = decode_kitty_keypad_text(data) {
        return Some(text);
    }
    if let Some(text) = extract_printable_text(data) {
        let mut chars = text.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            return Some(canonicalize_key_id(&ch.to_string()));
        }
        return None;
    }
    match data {
        "\x1b" => Some("escape".to_string()),
        "\r" | "\n" => Some("enter".to_string()),
        "\t" => Some("tab".to_string()),
        _ => {
            let bytes = data.as_bytes();
            if bytes.len() == 1 {
                let byte = bytes[0];
                if byte == 0x7f {
                    return Some("backspace".to_string());
                }
                let letter = match byte {
                    1..=8 => Some((b'a' + byte - 1) as char),
                    11 | 12 => Some((b'a' + byte - 1) as char),
                    14..=26 => Some((b'a' + byte - 1) as char),
                    _ => None,
                };
                if let Some(ch) = letter {
                    return Some(canonical_key_id(&["ctrl"], &ch.to_string()));
                }
            }
            None
        }
    }
}

fn raw_backspace_is_ctrl() -> bool {
    std::env::var("PI_TUI_RAW_BACKSPACE_IS_CTRL").is_ok_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct TestHandler {
        enabled: bool,
        starts: Rc<Cell<usize>>,
        ends: Rc<Cell<usize>>,
    }

    impl SpaceHoldHandler for TestHandler {
        fn enabled(&self) -> bool {
            self.enabled
        }
        fn on_start(&mut self) {
            self.starts.set(self.starts.get() + 1);
        }
        fn on_end(&mut self) {
            self.ends.set(self.ends.get() + 1);
        }
    }

    struct Recorder {
        deleted: Rc<Cell<usize>>,
    }

    impl Recorder {
        fn new() -> (Self, Rc<Cell<usize>>) {
            let deleted = Rc::new(Cell::new(0));
            (
                Recorder {
                    deleted: Rc::clone(&deleted),
                },
                deleted,
            )
        }

        fn gesture(&self) -> SpaceHoldGesture {
            let deleted = Rc::clone(&self.deleted);
            SpaceHoldGesture::new(move |count| deleted.set(count))
        }
    }

    #[test]
    fn gaps_mechanical_only_when_fast_and_steady() {
        assert!(gaps_are_mechanical(50.0, 52.0));
        assert!(!gaps_are_mechanical(130.0, 50.0));
        assert!(!gaps_are_mechanical(50.0, 80.0));
        assert!(gaps_are_mechanical(20.0, 25.0));
    }

    #[test]
    fn space_hold_text_decodes_printable_candidates() {
        assert_eq!(get_space_hold_text(" ", Some("space")), Some(" ".to_string()));
        assert_eq!(
            get_space_hold_text(" ", Some("shift+space")),
            Some(" ".to_string())
        );
        assert_eq!(get_space_hold_text("!", Some("!")), Some("!".to_string()));
        assert_eq!(get_space_hold_text("A", Some("shift+a")), Some("A".to_string()));
        assert_eq!(get_space_hold_text(None, Some("space")), None);
        assert_eq!(get_space_hold_text(" ", Some("ctrl+space")), None);
        assert_eq!(get_space_hold_text(" ", Some("up")), None);
        assert_eq!(get_space_hold_text("ab", Some("a")), Some("ab".to_string()));
    }

    #[test]
    fn recognizes_hold_after_steady_repeats_and_ends_on_idle() {
        let (recorder, deleted) = Recorder::new();
        let mut gesture = recorder.gesture();
        let starts = Rc::new(Cell::new(0));
        let ends = Rc::new(Cell::new(0));
        gesture.set_handler(Some(Box::new(TestHandler {
            enabled: true,
            starts: Rc::clone(&starts),
            ends: Rc::clone(&ends),
        })));
        let t0 = Instant::now();
        assert_eq!(gesture.process_at(Some("space"), 1, t0), SpaceHoldStep::Type);
        gesture.record_typed(1);
        assert_eq!(
            gesture.process_at(Some("space"), 1, t0 + Duration::from_millis(50)),
            SpaceHoldStep::Type
        );
        gesture.record_typed(1);
        assert_eq!(
            gesture.process_at(Some("space"), 1, t0 + Duration::from_millis(100)),
            SpaceHoldStep::Swallow
        );
        assert_eq!(
            gesture.process_at(Some("space"), 1, t0 + Duration::from_millis(150)),
            SpaceHoldStep::Swallow
        );
        assert_eq!(deleted.get(), 2);
        assert_eq!(starts.get(), 1);
        assert!(gesture.is_active());
        gesture.tick_at(t0 + Duration::from_millis(400));
        assert_eq!(ends.get(), 1);
        assert!(!gesture.is_active());
        gesture.tick_at(t0 + Duration::from_millis(650));
        assert_eq!(ends.get(), 1);
    }

    #[test]
    fn slow_taps_stay_typed() {
        let (recorder, deleted) = Recorder::new();
        let mut gesture = recorder.gesture();
        let starts = Rc::new(Cell::new(0));
        gesture.set_handler(Some(Box::new(TestHandler {
            enabled: true,
            starts: Rc::clone(&starts),
            ends: Rc::new(Cell::new(0)),
        })));
        let t0 = Instant::now();
        for step in 0..5 {
            let now = t0 + Duration::from_millis(step * 200);
            assert_eq!(gesture.process_at(Some("space"), 1, now), SpaceHoldStep::Type);
            gesture.record_typed(1);
        }
        assert_eq!(deleted.get(), 0);
        assert_eq!(starts.get(), 0);
        assert!(!gesture.is_active());
    }

    #[test]
    fn other_key_ends_hold_without_merging() {
        let (recorder, _) = Recorder::new();
        let mut gesture = recorder.gesture();
        let starts = Rc::new(Cell::new(0));
        let ends = Rc::new(Cell::new(0));
        gesture.set_handler(Some(Box::new(TestHandler {
            enabled: true,
            starts: Rc::clone(&starts),
            ends: Rc::clone(&ends),
        })));
        let t0 = Instant::now();
        for step in 0..4 {
            gesture.process_at(Some("space"), 1, t0 + Duration::from_millis(step * 50));
            gesture.record_typed(1);
        }
        assert_eq!(starts.get(), 1);
        assert_eq!(
            gesture.process_at(Some("x"), 1, t0 + Duration::from_millis(200)),
            SpaceHoldStep::Pass
        );
        assert_eq!(ends.get(), 1);
        assert!(!gesture.is_active());
    }

    #[test]
    fn disabled_handler_passes_and_ends() {
        let (recorder, _) = Recorder::new();
        let mut gesture = recorder.gesture();
        let ends = Rc::new(Cell::new(0));
        gesture.set_handler(Some(Box::new(TestHandler {
            enabled: false,
            starts: Rc::new(Cell::new(0)),
            ends: Rc::clone(&ends),
        })));
        let t0 = Instant::now();
        for step in 0..3 {
            gesture.process_at(Some("space"), 1, t0 + Duration::from_millis(step * 50));
            gesture.record_typed(1);
        }
        assert_eq!(ends.get(), 0);
        assert_eq!(
            gesture.process_at(Some("space"), 1, t0 + Duration::from_millis(200)),
            SpaceHoldStep::Pass
        );
        assert_eq!(ends.get(), 0);
    }

    #[test]
    fn should_route_claims_only_non_printable_bindings() {
        let (recorder, _) = Recorder::new();
        let mut gesture = recorder.gesture();
        gesture.set_keys(&["ctrl+k".to_string(), "space".to_string()]);
        let starts = Rc::new(Cell::new(0));
        gesture.set_handler(Some(Box::new(TestHandler {
            enabled: true,
            starts: Rc::clone(&starts),
            ends: Rc::new(Cell::new(0)),
        })));
        assert!(gesture.should_route("\x0b"));
        assert!(!gesture.should_route(" "));
        assert!(!gesture.should_route("k"));
    }

    #[test]
    fn parses_raw_control_and_kitty_sequences() {
        assert_eq!(parse_key("\x7f"), Some("backspace".to_string()));
        assert_eq!(parse_key("\x0b"), Some("ctrl+k".to_string()));
        assert_eq!(parse_key("\r"), Some("enter".to_string()));
        assert_eq!(parse_key("\x1b"), Some("escape".to_string()));
        assert_eq!(parse_key("a"), Some("a".to_string()));
        assert_eq!(
            parse_key("\x1b[57415u"),
            Some("=".to_string())
        );
        assert_eq!(
            decode_modify_other_keys_printable("\x1b[27;2;65~"),
            Some("A".to_string())
        );
        assert_eq!(
            decode_modify_other_keys_printable("\x1b[27;5;65~"),
            None
        );
        assert_eq!(extract_printable_text("\x1b[A"), None);
    }
}
