//! Animated loading indicator.

use crate::component::Component;
use crate::spinner_clock::SpinnerClock;
use crate::style::{Color, Style};
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::trigger::Trigger;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct Loader {
    trigger: Trigger,
    clock: SpinnerClock,
    message: String,
    spinner_style: Style,
    message_style: Style,
    running: bool,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Default for Loader {
    fn default() -> Self {
        Self::new(Trigger::new())
    }
}

impl Loader {
    pub fn new(trigger: Trigger) -> Self {
        Loader {
            trigger,
            clock: SpinnerClock::from_symbols(80),
            message: "Loading...".into(),
            spinner_style: Style::fg(Color::Cyan),
            message_style: Style::fg(Color::Gray),
            running: false,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn set_styles(&mut self, spinner: Style, message: Style) {
        self.spinner_style = spinner;
        self.message_style = message;
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn start(&mut self) {
        if self.running {
            return;
        }
        self.running = true;
        self.stop.store(false, Ordering::Relaxed);
        self.clock.reset();
        let trigger = self.trigger.clone();
        let stop = Arc::clone(&self.stop);
        let interval = self.clock_interval();
        self.worker = Some(thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(interval);
                trigger.request();
            }
        }));
    }

    fn clock_interval(&self) -> Duration {
        Duration::from_millis(80)
    }

    pub fn stop(&mut self) {
        if !self.running {
            return;
        }
        self.running = false;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Component for Loader {
    fn render(&mut self, width: u16) -> Vec<String> {
        let inner = width.max(1) as usize;
        let spinner = self.spinner_style.paint(self.clock.frame());
        let prefix_width = crate::text_shape::visible_width(self.clock.frame()) + 1;
        let remaining = inner.saturating_sub(prefix_width);
        let message = truncate_to_width(&self.message, remaining, Ellipsis::Unicode);
        let message = self.message_style.paint(&message);
        vec![truncate_to_width(
            &format!("{spinner} {message}"),
            inner,
            Ellipsis::Omit,
        )]
    }

    fn dispose(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_spinner_and_message() {
        let mut loader = Loader::new(Trigger::new());
        loader.set_message("Working");
        let rows = loader.render(20);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("Working"));
    }

    #[test]
    fn start_stop_lifecycle() {
        let mut loader = Loader::new(Trigger::new());
        loader.start();
        assert!(loader.running());
        loader.stop();
        assert!(!loader.running());
    }
}
