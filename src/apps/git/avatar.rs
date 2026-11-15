//! Author identity: deterministic 5x5 mirrored identicon.
//!
//! Reference: `l/src/apps/git/avatar.ts`. The reference digests the normalized
//! email with MD5; this port uses SHA-256 so no new crate is added. The exact
//! glyphs therefore differ, while the mirrored 5x5 layout, the half-block
//! packing (3 rows x 10 columns) and the hue derivation are preserved.

use sha2::{Digest, Sha256};

/// Cached author photos supplied by the command host.
pub trait AvatarSource {
    /// Photo for `email` relative to `cwd`, if the host has one.
    fn get(&self, email: &str, cwd: &str) -> Option<String>;
}

/// Deterministic 5x5 mirrored identicon as half-block rows.
///
/// `colorize(hex, row)` paints one row with the derived color, mirroring the
/// reference theme hook. Without color the rows are 10 columns wide.
pub fn identicon_lines(email: &str, colorize: impl Fn(&str, &str) -> String) -> Vec<String> {
    let normalized = email.trim().to_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    let hue = (u16::from(digest[0]) << 8 | u16::from(digest[1])) % 360;
    let hex = hsl_to_hex(hue, 0.55, 0.58);
    let on = |x: usize, y: usize| {
        let column = if x < 3 { x } else { 4 - x };
        digest[3 + column * 5 + y] % 2 == 0
    };
    let mut lines = Vec::with_capacity(3);
    for row in 0..3 {
        let mut line = String::with_capacity(10);
        for x in 0..5 {
            let top = on(x, row * 2);
            let bottom = row * 2 + 1 < 5 && on(x, row * 2 + 1);
            let cell = match (top, bottom) {
                (true, true) => "\u{2588}",
                (true, false) => "\u{2580}",
                (false, true) => "\u{2584}",
                (false, false) => " ",
            };
            line.push_str(cell);
            line.push_str(cell);
        }
        lines.push(colorize(&hex, &line));
    }
    lines
}

/// `hsl(h, s, l)` to lowercase `#rrggbb`; `hue` wraps at 360.
fn hsl_to_hex(hue: u16, sat: f64, light: f64) -> String {
    let h = f64::from(hue % 360) / 360.0;
    let s = sat.clamp(0.0, 1.0);
    let l = light.clamp(0.0, 1.0);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let second = chroma * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let base = l - chroma / 2.0;
    let (r, g, b) = match (h * 6.0) as usize {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let channel = |value: f64| ((value + base) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", channel(r), channel(g), channel(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(_hex: &str, row: &str) -> String {
        row.to_string()
    }

    #[test]
    fn identicon_is_three_mirrored_rows() {
        for row in identicon_lines("fahzan@example.com", plain) {
            let chars: Vec<char> = row.chars().collect();
            assert_eq!(chars.len(), 10);
            for x in 0..5 {
                assert_eq!(chars[x * 2], chars[(4 - x) * 2]);
                assert_eq!(chars[x * 2 + 1], chars[(4 - x) * 2 + 1]);
            }
        }
    }

    #[test]
    fn identicon_normalizes_and_is_deterministic() {
        assert_eq!(
            identicon_lines("fahzan@example.com", plain),
            identicon_lines("  FAHZAN@example.com ", plain)
        );
        assert_ne!(
            identicon_lines("a@example.com", plain),
            identicon_lines("b@example.com", plain)
        );
    }

    #[test]
    fn identicon_reports_derived_hex_color() {
        let seen = std::cell::RefCell::new(String::new());
        let colorize = |hex: &str, row: &str| {
            *seen.borrow_mut() = hex.to_string();
            row.to_string()
        };
        identicon_lines("fahzan@example.com", colorize);
        let hex = seen.into_inner();
        assert!(hex.starts_with('#'));
        assert_eq!(hex.len(), 7);
    }

    #[test]
    fn hsl_primary_colors() {
        assert_eq!(hsl_to_hex(0, 1.0, 0.5), "#ff0000");
        assert_eq!(hsl_to_hex(120, 1.0, 0.5), "#00ff00");
        assert_eq!(hsl_to_hex(240, 1.0, 0.5), "#0000ff");
        assert_eq!(hsl_to_hex(0, 0.0, 1.0), "#ffffff");
        assert_eq!(hsl_to_hex(0, 0.0, 0.0), "#000000");
    }
}
