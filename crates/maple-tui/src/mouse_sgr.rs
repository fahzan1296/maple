//! Reference: `l/src/mouse.ts` (SGR mouse parsing; `l/src/mouse-sgr.ts` does not exist).
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};

/// Parse `ESC [ <btn> ; col ; row (M|m)` into a cell-coordinate event.
pub fn parse_sgr_mouse(data: &str) -> Option<MouseEvent> {
    let rest = data.strip_prefix("\x1b[<")?;
    let suffix = rest.chars().last()?;
    if suffix != 'M' && suffix != 'm' {
        return None;
    }
    let body = &rest[..rest.len() - suffix.len_utf8()];
    let mut parts = body.split(';');
    let button: u32 = parts.next()?.parse().ok()?;
    let column: u16 = parts.next()?.parse().ok()?;
    let row: u16 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let release = suffix == 'm';
    let wheel = (button & 64) != 0 && (button & 2) == 0;
    let wheel_delta = if wheel {
        if button & 1 != 0 {
            1
        } else {
            -1
        }
    } else {
        0
    };
    let motion = (button & 32) != 0 && !wheel;
    let kind = if release {
        MouseKind::Up(button_kind(button))
    } else if wheel {
        if wheel_delta > 0 {
            MouseKind::ScrollDown
        } else {
            MouseKind::ScrollUp
        }
    } else if motion {
        MouseKind::Moved
    } else {
        MouseKind::Down(button_kind(button))
    };
    Some(MouseEvent {
        kind,
        column: column.saturating_sub(1),
        row: row.saturating_sub(1),
        shift: button & 4 != 0,
        ctrl: button & 16 != 0,
        alt: button & 8 != 0,
    })
}

fn button_kind(button: u32) -> MouseButtonKind {
    match button & 3 {
        0 => MouseButtonKind::Left,
        1 => MouseButtonKind::Middle,
        _ => MouseButtonKind::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_press_release_and_wheel() {
        let press = parse_sgr_mouse("\x1b[<0;10;5M").unwrap();
        assert_eq!(press.column, 9);
        assert_eq!(press.row, 4);
        assert_eq!(press.kind, MouseKind::Down(MouseButtonKind::Left));
        let release = parse_sgr_mouse("\x1b[<0;10;5m").unwrap();
        assert_eq!(release.kind, MouseKind::Up(MouseButtonKind::Left));
        let wheel = parse_sgr_mouse("\x1b[<64;3;3M").unwrap();
        assert_eq!(wheel.kind, MouseKind::ScrollUp);
        let wheel_down = parse_sgr_mouse("\x1b[<65;3;3M").unwrap();
        assert_eq!(wheel_down.kind, MouseKind::ScrollDown);
    }

    #[test]
    fn decodes_modifiers_and_drag() {
        let event = parse_sgr_mouse("\x1b[<32;7;2M").unwrap();
        assert_eq!(event.kind, MouseKind::Moved);
        let drag = parse_sgr_mouse("\x1b[<44;7;2M").unwrap();
        assert_eq!(drag.kind, MouseKind::Moved);
        assert!(drag.shift);
        assert!(drag.alt);
    }

    #[test]
    fn rejects_other_reports() {
        assert!(parse_sgr_mouse("\x1b[<0;1;1").is_none());
        assert!(parse_sgr_mouse("\x1b[0;1;1M").is_none());
    }
}
