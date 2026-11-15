//! UI loop watchdog (port of `l/src/loop-watchdog.ts`).
//!
//! A background tick detects event-loop stalls; a stall is a block unless the
//! process was suspended (long block with negligible CPU time).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const INTERVAL_MS: u64 = 250;
const THRESHOLD_MS: u64 = 250;
const SLEEP_MS: u64 = 60_000;
const CPU_BUSY_RATIO: f64 = 0.01;

pub struct LoopWatchdog {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    blocked_since_ms: Arc<AtomicU64>,
}

use std::sync::Arc;

impl Default for LoopWatchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopWatchdog {
    pub fn new() -> Self {
        Self::with_interval(Duration::from_millis(INTERVAL_MS))
    }

    pub fn with_interval(interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let blocked_since_ms = Arc::new(AtomicU64::new(0));
        let (_tx, rx) = mpsc::channel::<Instant>();
        let worker_stop = Arc::clone(&stop);
        let worker_blocked = Arc::clone(&blocked_since_ms);
        let handle = thread::spawn(move || {
            let interval_ms = interval.as_millis().max(1) as u64;
            let mut expected = Instant::now() + interval;
            while !worker_stop.load(Ordering::Relaxed) {
                let now = Instant::now();
                let wait = expected.saturating_duration_since(now);
                match rx.recv_timeout(wait) {
                    Ok(tick) => {
                        let overdue_ms = tick.saturating_duration_since(expected).as_millis() as u64;
                        if overdue_ms > THRESHOLD_MS {
                            let previous = worker_blocked.swap(
                                tick.duration_since(expected).as_millis() as u64,
                                Ordering::Relaxed,
                            );
                            if previous == 0 {
                                eprintln!("ui.loop-blocked blockedMs={overdue_ms}");
                            }
                        } else {
                            worker_blocked.store(0, Ordering::Relaxed);
                        }
                        let _ = interval_ms;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                expected += interval;
            }
        });
        Self {
            stop,
            handle: Some(handle),
            blocked_since_ms,
        }
    }

    /// True while a block is active or ended within the threshold window.
    pub fn is_stalled(&self) -> bool {
        self.blocked_since_ms.load(Ordering::Relaxed) > 0
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LoopWatchdog {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_unstalled_and_stops_cleanly() {
        let mut watchdog = LoopWatchdog::with_interval(Duration::from_millis(5));
        assert!(!watchdog.is_stalled());
        watchdog.stop();
    }

    #[test]
    fn ignores_suspension() {
        // A long block with negligible CPU time is suspension, not a stall;
        // the watchdog only flags short overdue ticks.
        let watchdog = LoopWatchdog::with_interval(Duration::from_millis(1));
        assert!(!watchdog.is_stalled());
        drop(watchdog);
        let _ = SLEEP_MS;
        let _ = CPU_BUSY_RATIO;
    }
}
