//! Recover AltGr text that Windows console hosts drop under the kitty keyboard
//! Reference: `l/src/windows-altgr.ts`.
//! protocol (port of `l/src/windows-altgr.ts`).
//!
//! Pure translation logic is platform-independent and unit-tested with an
//! injected [`AltGrHost`]. Building the layout table needs Win32 `user32.dll`
//! and is compiled only on Windows.

use std::cell::RefCell;

const KITTY_MOD_SHIFT: u32 = 1;
const KITTY_MOD_ALT: u32 = 2;
const KITTY_MOD_CTRL: u32 = 4;
const KITTY_LOCK_MASK: u32 = 64 | 128;

const VK_SHIFT: u32 = 0x10;
const VK_LSHIFT: u32 = 0xa0;
const VK_RMENU: u32 = 0xa5;

/// How long an observed Right Alt press keeps counting as held.
///
/// Key state is sampled at processing time, and one stdin read can carry
/// several queued AltGr repeats that are dispatched synchronously after the
/// key is released; the latch keeps that tail typing text.
pub const RIGHT_ALT_LATCH_MS: u64 = 30;

/// AltGr layer of one keyboard layout: `"<baseCodepoint>:<shift 0|1>"` → text.
pub type AltGrTable = std::collections::BTreeMap<String, String>;

/// Table key for a lowercased base codepoint and shift state.
pub fn table_key(base_codepoint: u32, shift: bool) -> String {
    format!("{}:{}", base_codepoint, if shift { 1 } else { 0 })
}

/// Whether every character is printable text (no C0/C1 controls).
pub fn is_printable_text(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|ch| {
            let cp = ch as u32;
            cp >= 0x20 && cp != 0x7f && !(0x80..=0x9f).contains(&cp)
        })
}

/// Local keyboard state the translator reads; injectable so tests need no live
/// desktop.
pub trait AltGrHost {
    /// AltGr layer of the layout that produced the keystroke.
    fn active_layer(&self) -> AltGrTable;
    /// Whether Right Alt (AltGr) is held, or was within the last
    /// [`RIGHT_ALT_LATCH_MS`].
    fn is_right_alt_down(&self) -> bool;
}

/// A host whose layout has no AltGr layer (US, UK, …): Alt chords stay
/// shortcuts.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyAltGrHost;

impl AltGrHost for EmptyAltGrHost {
    fn active_layer(&self) -> AltGrTable {
        AltGrTable::new()
    }

    fn is_right_alt_down(&self) -> bool {
        false
    }
}

/// Wrap a raw Right Alt probe so a press stays observed for
/// [`RIGHT_ALT_LATCH_MS`].
pub fn create_right_alt_latch(
    is_down_now: impl Fn() -> bool + 'static,
    now: impl Fn() -> u64 + 'static,
) -> impl Fn() -> bool {
    let last_seen_down = RefCell::new(None::<u64>);
    move || {
        let at = now();
        if is_down_now() {
            *last_seen_down.borrow_mut() = Some(at);
            return true;
        }
        last_seen_down
            .borrow()
            .map(|seen| at.saturating_sub(seen) <= RIGHT_ALT_LATCH_MS)
            .unwrap_or(false)
    }
}

/// One parsed kitty CSI-u sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KittySequence {
    /// Base codepoint.
    pub codepoint: u32,
    /// Raw kitty modifier bitfield.
    pub modifier: u32,
    /// Event type (1 = press, 2 = repeat, 3 = release); `None` means press.
    pub event_type: Option<u32>,
}

/// Parse `ESC [ <codepoint> ; <modifier> [; <event-type>] u`.
///
/// Only the shape the AltGr translator needs; kernel keys, functional keys and
/// mouse reports are rejected by the caller's range checks.
pub fn parse_kitty_sequence(data: &str) -> Option<KittySequence> {
    let body = data.strip_prefix("\x1b[")?.strip_suffix('u')?;
    let mut parts = body.split(';');
    let codepoint = parts.next()?.parse().ok()?;
    let modifier = parts.next()?.parse().ok()?;
    let event_type = match parts.next() {
        Some("") | None => None,
        Some(raw) => Some(raw.parse().ok()?),
    };
    Some(KittySequence {
        codepoint,
        modifier,
        event_type,
    })
}

/// Translate a kitty CSI-u Alt/Ctrl+Alt chord into the text AltGr produced, or
/// `None` when the sequence is not an AltGr keystroke on the active layout.
///
/// Only call this on native Windows ConPTY hosts with the kitty protocol
/// active.
pub fn translate_windows_altgr_sequence(data: &str, host: &dyn AltGrHost) -> Option<String> {
    let parsed = parse_kitty_sequence(data)?;
    if parsed.event_type == Some(3) {
        return None;
    }
    let modifier = parsed.modifier & !KITTY_LOCK_MASK;
    // AltGr surfaces as Alt (fake LeftCtrl filtered) or Ctrl+Alt, optionally
    // with Shift.
    if modifier & KITTY_MOD_ALT == 0 {
        return None;
    }
    if modifier & !(KITTY_MOD_ALT | KITTY_MOD_CTRL | KITTY_MOD_SHIFT) != 0 {
        return None;
    }
    // Functional keys (kitty PUA range) and C0 controls never carry AltGr text.
    if parsed.codepoint < 0x20 || (0xe000..=0xf8ff).contains(&parsed.codepoint) {
        return None;
    }
    // Layouts without an AltGr layer stop here: Alt chords stay shortcuts.
    let layer = host.active_layer();
    if layer.is_empty() {
        return None;
    }
    let base = char::from_u32(parsed.codepoint)?.to_lowercase().next()?;
    let key = table_key(base as u32, modifier & KITTY_MOD_SHIFT != 0);
    let text = layer.get(&key)?;
    // Left Alt+key shares the exact bytes; only a held Right Alt means AltGr.
    host.is_right_alt_down().then(|| text.clone())
}


/// AltGr layer of the active foreground window's keyboard layout.
#[cfg(windows)]
pub fn read_active_alt_gr_layer() -> AltGrTable {
    windows_altgr_win::active_layout_table().unwrap_or_default()
}

/// AltGr layer lookup is only available on Windows; other platforms return an
/// empty layer so Alt chords stay shortcuts.
#[cfg(not(windows))]
pub fn read_active_alt_gr_layer() -> AltGrTable {
    AltGrTable::new()
}

#[cfg(windows)]
pub(crate)
mod windows_altgr_win {
    use super::{is_printable_text, table_key, AltGrTable};
    use std::collections::BTreeMap;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyExW, ToUnicodeEx, MAPVK_VK_TO_VSC, TOUNICODE_NO_STATE_CHANGE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetAsyncKeyState, GetForegroundWindow, GetKeyboardLayout, GetWindowThreadProcessId,
    };

    const VK_CONTROL: u32 = 0x11;
    const VK_MENU: u32 = 0x12;
    const VK_LCONTROL: u32 = 0xa2;
    const VK_RMENU: u32 = 0xa5;
    const VK_LSHIFT: u32 = 0xa0;

    const ALT_GR: [u32; 4] = [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU];
    const ALT_GR_SHIFT: [u32; 6] = [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU, 0x10, VK_LSHIFT];

    fn translate(vk: u32, scan: u32, modifiers: &[u32], hkl: isize) -> Option<String> {
        let mut key_state = [0u8; 256];
        for modifier in modifiers {
            key_state[*modifier as usize] = 0x80;
        }
        let mut buffer = [0u16; 8];
        let count = unsafe {
            ToUnicodeEx(
                vk,
                scan,
                key_state.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as i32,
                TOUNICODE_NO_STATE_CHANGE,
                hkl,
            )
        };
        // Negative = dead key; zero = no translation.
        if count <= 0 {
            return None;
        }
        let text: String = buffer[..count as usize]
            .iter()
            .filter_map(|unit| char::from_u32(*unit as u32))
            .collect();
        is_printable_text(&text).then_some(text)
    }

    fn build_alt_gr_table(hkl: isize) -> AltGrTable {
        let mut table = BTreeMap::new();
        for vk in 0x20..=0xfe {
            // Skip Windows/menu keys and the sided modifiers: no text layer.
            if (0x5b..=0x5f).contains(&vk) || (VK_LSHIFT..=VK_RMENU).contains(&vk) {
                continue;
            }
            let scan = unsafe { MapVirtualKeyExW(vk, MAPVK_VK_TO_VSC, hkl) };
            if scan == 0 {
                continue;
            }
            let Some(base) = translate(vk, scan, &[], hkl) else {
                continue;
            };
            let base_codepoint = base.to_lowercase().next().map(|ch| ch as u32).unwrap_or(0);
            for shift in [false, true] {
                let key = table_key(base_codepoint, shift);
                if table.contains_key(&key) {
                    continue;
                }
                if let Some(produced) = translate(vk, scan, if shift { &ALT_GR_SHIFT } else { &ALT_GR }, hkl) {
                    table.insert(key, produced);
                }
            }
        }
        table
    }

    pub fn active_layout_table() -> Option<AltGrTable> {
        unsafe {
            let hwnd = GetForegroundWindow();
            let thread = if hwnd != 0 {
                GetWindowThreadProcessId(hwnd, std::ptr::null_mut())
            } else {
                0
            };
            let hkl = GetKeyboardLayout(thread);
            (hkl != 0).then(|| build_alt_gr_table(hkl))
        }
    }

    /// Whether Right Alt is currently held.
    pub fn is_right_alt_down() -> bool {
        unsafe { GetAsyncKeyState(VK_RMENU as i32) & 0x8000 != 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeHost {
        layer: AltGrTable,
        right_alt: bool,
    }

    impl AltGrHost for FakeHost {
        fn active_layer(&self) -> AltGrTable {
            self.layer.clone()
        }

        fn is_right_alt_down(&self) -> bool {
            self.right_alt
        }
    }

    fn host(layer: AltGrTable, right_alt: bool) -> FakeHost {
        FakeHost { layer, right_alt }
    }

    #[test]
    fn latch_keeps_a_press_alive_briefly() {
        let down = std::rc::Rc::new(Cell::new(true));
        let now = std::rc::Rc::new(Cell::new(0u64));
        let held_down = down.clone();
        let held_now = now.clone();
        let is_down = move || held_down.get();
        let at = move || held_now.get();
        let held = create_right_alt_latch(is_down, at);
        assert!(held());
        down.set(false);
        now.set(29);
        assert!(held());
        now.set(31);
        assert!(!held());
    }

    #[test]
    fn latch_restarts_on_a_new_press() {
        let down = std::rc::Rc::new(Cell::new(false));
        let now = std::rc::Rc::new(Cell::new(0u64));
        let held_down = down.clone();
        let held_now = now.clone();
        let held = create_right_alt_latch(move || held_down.get(), move || held_now.get());
        assert!(!held());
        down.set(true);
        assert!(held());
        down.set(false);
        now.set(100);
        assert!(!held());
        down.set(true);
        now.set(129);
        assert!(held());
    }

    #[test]
    fn parses_csi_u_shapes() {
        assert_eq!(
            parse_kitty_sequence("\x1b[97;6u"),
            Some(KittySequence { codepoint: 97, modifier: 6, event_type: None })
        );
        assert_eq!(
            parse_kitty_sequence("\x1b[97;6;3u"),
            Some(KittySequence { codepoint: 97, modifier: 6, event_type: Some(3) })
        );
        assert_eq!(parse_kitty_sequence("\x1b[97u"), None);
        assert_eq!(parse_kitty_sequence("\x1b[97;6"), None);
    }

    #[test]
    fn translates_only_altgr_chords() {
        let mut layer = AltGrTable::new();
        layer.insert(table_key('a' as u32, false), "@".into());
        let right = host(layer, true);
        assert_eq!(
            translate_windows_altgr_sequence("\x1b[97;6u", &right).as_deref(),
            Some("@")
        );
        // Ctrl+Alt without Right Alt held is a shortcut, not AltGr.
        let left = host(right.layer.clone(), false);
        assert_eq!(translate_windows_altgr_sequence("\x1b[97;6u", &left), None);
        // Release events never carry text.
        assert_eq!(translate_windows_altgr_sequence("\x1b[97;6;3u", &right), None);
        // Plain Alt chords stay shortcuts.
        assert_eq!(translate_windows_altgr_sequence("\x1b[97;3u", &right), None);
        // Unknown base key.
        assert_eq!(translate_windows_altgr_sequence("\x1b[122;6u", &right), None);
    }

    #[test]
    fn empty_layer_stops_translation() {
        assert_eq!(
            translate_windows_altgr_sequence("\x1b[97;6u", &EmptyAltGrHost),
            None
        );
    }

    #[test]
    fn printable_text_rejects_controls() {
        assert!(is_printable_text("@"));
        assert!(!is_printable_text(""));
        assert!(!is_printable_text("\u{1b}"));
        assert!(!is_printable_text("\u{9f}"));
    }
}
