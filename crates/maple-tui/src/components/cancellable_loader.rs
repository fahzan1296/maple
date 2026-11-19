//! Loader that can be cancelled with Escape.
//!
//! Reference: `l/src/components/cancellable-loader.ts` (which extends
//! `l/src/components/loader.ts`).
//!
//! Deviations forced by Rust:
//!
//! - The reference extends `Loader` by inheritance; Rust has none, so
//!   [`CancellableLoader`] composes a [`Loader`] and delegates to it.
//! - The reference's `AbortController`/`AbortSignal` are JS runtime objects.
//!   [`CancellableSignal`] reproduces the consumed surface (`aborted`, aborted
//!   on demand) with an `Arc<AtomicBool>` so a clone observes the same state.
//! - `getKeybindings()` is a module-level singleton upstream; maple-tui has no
//!   global keybindings, so the [`KeybindingsManager`] is passed explicitly
//!   ([`CancellableLoader::with_keybindings`], defaulting to
//!   [`KeybindingsManager::new`]).
//! - `onAbort` is a public mutable field upstream; Rust exposes
//!   [`CancellableLoader::set_on_abort`].
//! - `Loader::debugState()` does not exist on the Rust `Loader`, so
//!   [`CancellableLoader::debug_state`] reports the loader fields it does have
//!   (`message`, `running`) plus `cancelled`.

use crate::component::Component;
use crate::components::loader::Loader;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::style::Style;
use crate::trigger::Trigger;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Abort state shared by a [`CancellableLoader`] and the operations watching it.
#[derive(Clone, Debug, Default)]
pub struct CancellableSignal {
    aborted: Arc<AtomicBool>,
}

impl CancellableSignal {
    /// A signal that has not been aborted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether cancellation was requested.
    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    /// Request cancellation.
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }
}

/// Animates a spinner while asynchronous work runs, and cancels on Escape.
pub struct CancellableLoader {
    loader: Loader,
    signal: CancellableSignal,
    on_abort: Option<Arc<dyn Fn() + Send + Sync>>,
    keybindings: KeybindingsManager,
}

impl CancellableLoader {
    /// A loader using the default keybindings.
    pub fn new(trigger: Trigger) -> Self {
        Self::with_keybindings(trigger, KeybindingsManager::new())
    }

    /// A loader matching cancel against an explicit keybinding set.
    pub fn with_keybindings(trigger: Trigger, keybindings: KeybindingsManager) -> Self {
        CancellableLoader {
            loader: Loader::new(trigger),
            signal: CancellableSignal::new(),
            on_abort: None,
            keybindings,
        }
    }

    /// The signal aborted when the user presses the cancel key.
    pub fn signal(&self) -> CancellableSignal {
        self.signal.clone()
    }

    /// Whether the loader was aborted.
    pub fn aborted(&self) -> bool {
        self.signal.aborted()
    }

    /// Register the callback run when the user presses the cancel key.
    pub fn set_on_abort<F>(&mut self, on_abort: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_abort = Some(Arc::new(on_abort));
    }

    /// Remove any cancel callback.
    pub fn clear_on_abort(&mut self) {
        self.on_abort = None;
    }

    /// Loader state including whether cancellation was requested.
    pub fn debug_state(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut state = serde_json::Map::new();
        state.insert(
            "message".to_string(),
            serde_json::Value::String(self.loader.message().to_string()),
        );
        state.insert(
            "running".to_string(),
            serde_json::Value::Bool(self.loader.running()),
        );
        state.insert("cancelled".to_string(), serde_json::Value::Bool(self.aborted()));
        state
    }

    /// The wrapped loader.
    pub fn loader(&self) -> &Loader {
        &self.loader
    }

    /// The wrapped loader, mutably.
    pub fn loader_mut(&mut self) -> &mut Loader {
        &mut self.loader
    }

    /// Delegate to the wrapped loader.
    pub fn start(&mut self) {
        self.loader.start();
    }

    /// Delegate to the wrapped loader.
    pub fn stop(&mut self) {
        self.loader.stop();
    }

    /// Delegate to the wrapped loader.
    pub fn running(&self) -> bool {
        self.loader.running()
    }

    /// Delegate to the wrapped loader.
    pub fn set_message(&mut self, message: impl Into<String>) {
        self.loader.set_message(message);
    }

    /// Delegate to the wrapped loader.
    pub fn message(&self) -> &str {
        self.loader.message()
    }

    /// Delegate to the wrapped loader.
    pub fn set_styles(&mut self, spinner: Style, message: Style) {
        self.loader.set_styles(spinner, message);
    }

    fn cancel(&self) {
        self.signal.abort();
        if let Some(on_abort) = &self.on_abort {
            on_abort();
        }
    }
}

impl Component for CancellableLoader {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.loader.render(width)
    }

    fn handle_input(&mut self, key: &Key) {
        if self.keybindings.matches(key, "tui.select.cancel") {
            self.cancel();
        }
    }

    fn dispose(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, Style};

    #[test]
    fn escape_aborts_and_calls_on_abort() {
        let mut loader = CancellableLoader::new(Trigger::new());
        let aborted = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&aborted);
        loader.set_on_abort(move || seen.store(true, Ordering::Relaxed));
        loader.handle_input(&Key::Esc);
        assert!(loader.aborted());
        assert!(loader.signal().aborted());
        assert!(aborted.load(Ordering::Relaxed));
    }

    #[test]
    fn non_cancel_key_does_not_abort() {
        let mut loader = CancellableLoader::new(Trigger::new());
        loader.handle_input(&Key::Char('a'));
        assert!(!loader.aborted());
    }

    #[test]
    fn ctrl_c_cancel_matches_default_binding() {
        let mut loader = CancellableLoader::new(Trigger::new());
        loader.handle_input(&Key::Ctrl('c'));
        assert!(loader.aborted());
    }

    #[test]
    fn dispose_stops_the_loader() {
        let mut loader = CancellableLoader::new(Trigger::new());
        loader.start();
        assert!(loader.running());
        loader.dispose();
        assert!(!loader.running());
    }

    #[test]
    fn delegates_message_and_styles() {
        let mut loader = CancellableLoader::new(Trigger::new());
        loader.set_message("Working...");
        loader.set_styles(Style::fg(Color::Cyan), Style::fg(Color::Gray));
        assert_eq!(loader.message(), "Working...");
        assert!(loader.render(20)[0].contains("Working..."));
    }

    #[test]
    fn debug_state_reports_cancelled() {
        let mut loader = CancellableLoader::new(Trigger::new());
        loader.set_message("Working...");
        let state = loader.debug_state();
        assert_eq!(state.get("message").and_then(|value| value.as_str()), Some("Working..."));
        assert_eq!(state.get("cancelled"), Some(&serde_json::Value::Bool(false)));
        loader.handle_input(&Key::Esc);
        assert_eq!(
            loader.debug_state().get("cancelled"),
            Some(&serde_json::Value::Bool(true))
        );
    }
}
