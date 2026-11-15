//! Frame pacing (port of `l/src/frame-rate.ts`).

pub const DEFAULT_MAX_FPS: u32 = 30;
pub const MIN_MAX_FPS: u32 = 1;
pub const MAX_MAX_FPS: u32 = 120;

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static MAX_FPS: AtomicU32 = AtomicU32::new(DEFAULT_MAX_FPS);

/// Minimum interval between rendered frames at the current ceiling.
pub fn min_frame_interval() -> Duration {
    let fps = MAX_FPS.load(Ordering::Relaxed).clamp(MIN_MAX_FPS, MAX_MAX_FPS);
    Duration::from_secs_f64(1.0 / f64::from(fps))
}

/// Set the FPS ceiling (clamped to `MIN_MAX_FPS..=MAX_MAX_FPS`).
pub fn set_max_fps(next: u32) {
    MAX_FPS.store(next.clamp(MIN_MAX_FPS, MAX_MAX_FPS), Ordering::Relaxed);
}

/// Current FPS ceiling.
pub fn max_fps() -> u32 {
    MAX_FPS.load(Ordering::Relaxed)
}

/// Parse a user-provided FPS ceiling; numeric strings are accepted.
pub fn parse_max_fps(value: &str) -> Result<u32, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("Invalid FPS value: {value}"));
    }
    let parsed: u32 = trimmed
        .parse()
        .map_err(|_| format!("Invalid FPS value: {value}"))?;
    if (MIN_MAX_FPS..=MAX_MAX_FPS).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!(
            "FPS must be between {MIN_MAX_FPS} and {MAX_MAX_FPS}: {value}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_clamps() {
        assert_eq!(parse_max_fps("60").unwrap(), 60);
        assert_eq!(parse_max_fps(" 30 ").unwrap(), 30);
        assert!(parse_max_fps("abc").is_err());
        assert!(parse_max_fps("0").is_err());
        assert!(parse_max_fps("500").is_err());
        set_max_fps(500);
        assert_eq!(max_fps(), MAX_MAX_FPS);
        set_max_fps(DEFAULT_MAX_FPS);
        assert_eq!(min_frame_interval().as_millis(), 33);
    }
}
