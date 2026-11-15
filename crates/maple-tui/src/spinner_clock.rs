//! Frame sequencing for animated loaders.

use std::time::{Duration, Instant};

pub struct SpinnerClock {
    frames: Vec<String>,
    interval: Duration,
    started: Instant,
}

impl SpinnerClock {
    pub fn new(frames: Vec<String>, interval: Duration) -> Self {
        SpinnerClock {
            frames,
            interval,
            started: Instant::now(),
        }
    }

    pub fn from_symbols(interval_ms: u64) -> Self {
        SpinnerClock::new(
            crate::symbols::default_spinner_frames(),
            Duration::from_millis(interval_ms),
        )
    }

    pub fn tick(&self) -> usize {
        if self.interval.is_zero() || self.frames.is_empty() {
            return 0;
        }
        let ticks = self.started.elapsed().as_millis() / self.interval.as_millis();
        (ticks % self.frames.len() as u128) as usize
    }

    pub fn frame(&self) -> &str {
        self.frames.get(self.tick()).map(String::as_str).unwrap_or("")
    }

    pub fn reset(&mut self) {
        self.started = Instant::now();
    }
}
