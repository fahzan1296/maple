//! Reference: `l/src/setup/setup-version.ts`.
//!
//! Setup version the wizard advances a fresh install to. Bump it whenever a new
//! setup scene lands (or an existing scene raises its `min_version`).
//!
//! MUST equal `max(scene.min_version)` across `ALL_SCENES`; the setup scene
//! selection tests guard this invariant.

/// The setup version a fresh install is advanced to.
pub const CURRENT_SETUP_VERSION: u32 = 3;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::setup::wizard::ALL_SCENES;

	#[test]
	fn current_version_is_the_highest_scene_gate() {
		let max = ALL_SCENES
			.iter()
			.map(|scene| scene.min_version)
			.max()
			.unwrap_or(0);
		assert_eq!(CURRENT_SETUP_VERSION, max);
	}
}
