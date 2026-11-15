//! Cross-thread "please repaint" signal.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
pub struct Trigger {
    inner: Arc<(Mutex<bool>, Condvar)>,
}

impl Trigger {
    pub fn new() -> Self {
        Trigger::default()
    }

    /// Wake the UI loop and mark a frame pending.
    pub fn request(&self) {
        let (lock, condvar) = &*self.inner;
        if let Ok(mut pending) = lock.lock() {
            *pending = true;
        }
        condvar.notify_all();
    }

    /// Consume the pending-frame flag.
    pub fn take(&self) -> bool {
        let (lock, _) = &*self.inner;
        let mut pending = lock.lock().unwrap_or_else(|error| error.into_inner());
        std::mem::replace(&mut *pending, false)
    }

    /// Wait up to `timeout` for a request; returns true when one arrived.
    pub fn wait_for_request(&self, timeout: Duration) -> bool {
        let (lock, condvar) = &*self.inner;
        let mut pending = lock.lock().unwrap_or_else(|error| error.into_inner());
        if *pending {
            return true;
        }
        let (guard, result) = condvar
            .wait_timeout_while(pending, timeout, |pending| !*pending)
            .unwrap_or_else(|error| error.into_inner());
        pending = guard;
        !result.timed_out() && *pending
    }
}
