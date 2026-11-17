//! Decorative-motion switch shared by the status line, editor, and native `fx`.
//!
//! Reference: `l/src/motion-effects.ts`.
//!
//! Deviation: the reference keeps `enabled` and the listener set in module-level
//! mutable state. Rust has no module-level mutable singleton, so the state is
//! explicit in [`MotionEffects`]; [`MotionEffects::on_motion_effects_change`]
//! returns a [`MotionEffectsListener`] token whose [`MotionEffects::unsubscribe`]
//! removes the callback (the reference returns an unsubscribe closure).

/// Decorative motion state: whether effects may run and who observes flips.
pub struct MotionEffects {
    enabled: bool,
    listeners: Vec<(u64, Box<dyn FnMut()>)>,
    next_id: u64,
}

impl Default for MotionEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl MotionEffects {
    /// Motion enabled, matching the reference's initial `enabled = true`.
    pub fn new() -> Self {
        Self {
            enabled: true,
            listeners: Vec::new(),
            next_id: 0,
        }
    }

    /// Whether decorative motion may run.
    pub fn motion_effects_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable decorative motion; listeners run only on a flip.
    pub fn set_motion_effects(&mut self, next: bool) {
        if next == self.enabled {
            return;
        }
        self.enabled = next;

        for (_, listener) in self.listeners.iter_mut() {
            listener();
        }
    }

    /// Observe switch flips, in subscription order; returns the unsubscribe token.
    pub fn on_motion_effects_change(
        &mut self,
        listener: impl FnMut() + 'static,
    ) -> MotionEffectsListener {
        let id = self.next_id;
        self.next_id += 1;
        self.listeners.push((id, Box::new(listener)));
        MotionEffectsListener { id }
    }

    /// Remove a listener previously returned by [`Self::on_motion_effects_change`].
    pub fn unsubscribe(&mut self, listener: MotionEffectsListener) {
        self.listeners.retain(|(id, _)| *id != listener.id);
    }

    /// Number of live listeners.
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }
}

/// Token identifying one [`MotionEffects`] subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionEffectsListener {
    id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_enabled_and_flips_once() {
        let mut effects = MotionEffects::new();
        assert!(effects.motion_effects_enabled());
        effects.set_motion_effects(true);
        assert!(effects.motion_effects_enabled());
        effects.set_motion_effects(false);
        assert!(!effects.motion_effects_enabled());
    }

    #[test]
    fn notifies_in_subscription_order_only_on_change() {
        let mut effects = MotionEffects::new();
        let mut calls: Vec<String> = Vec::new();
        let first = effects.on_motion_effects_change(|| calls.push("first".into()));
        effects.on_motion_effects_change(|| calls.push("second".into()));
        effects.set_motion_effects(false);
        assert_eq!(calls, ["first", "second"]);
        effects.set_motion_effects(false);
        assert_eq!(calls, ["first", "second"]);
        effects.set_motion_effects(true);
        assert_eq!(calls, ["first", "second", "first", "second"]);
        effects.unsubscribe(first);
        effects.set_motion_effects(false);
        assert_eq!(
            calls,
            ["first", "second", "first", "second", "second"]
        );
    }
}
