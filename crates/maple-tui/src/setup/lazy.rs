//! Reference: `l/src/setup/lazy.ts`.
//!
//! Loads and runs provider setup without completing onboarding or replaying the
//! welcome intro.
//!
//! Deviations from the TypeScript original: the dynamic `import("./wizard")`
//! boundary has no Rust equivalent (the wizard is a static module dependency),
//! and the host's async calls are blocking (see `scenes/types.rs`).

use std::rc::Rc;

use crate::setup::scenes::types::SetupHost;
use crate::setup::wizard::{run_setup_wizard, RunSetupWizardOptions, ALL_SCENES};

/// Run only the provider sign-in scene, leaving onboarding incomplete.
pub fn run_provider_setup_wizard(ctx: Rc<dyn SetupHost>) {
	let providers_scene = ALL_SCENES.iter().find(|scene| scene.id == "providers");
	let Some(scene) = providers_scene else {
		ctx.show_error("Provider setup is unavailable.");
		return;
	};
	run_setup_wizard(
		ctx,
		vec![*scene],
		RunSetupWizardOptions {
			mark_complete: false,
			play_welcome_intro: false,
		},
	);
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::setup::wizard::ALL_SCENES;

	#[test]
	fn providers_scene_exists_for_the_lazy_entry() {
		assert!(ALL_SCENES.iter().any(|scene| scene.id == "providers"));
	}
}
