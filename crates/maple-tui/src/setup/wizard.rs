//! Reference: `l/src/setup/wizard.ts`.
//!
//! Ordered onboarding scenes, the environment gates that decide whether setup
//! runs, and the blocking driver that owns the fullscreen overlay until its
//! scenes and outro finish.
//!
//! Deviations from the TypeScript original:
//! - `ALL_SCENES` is a fixed-size array, not `as const` over module singletons.
//! - `selectSetupScenes` is synchronous; the crate has no async runtime.
//! - `runSetupWizard` blocks and pumps the wizard with
//!   [`SetupUiHost::poll_input`] / [`SetupUiHost::now_ms`] instead of awaiting a
//!   promise and `setInterval` timers.
//! - `ctx` is an `Rc<dyn SetupHost>` so the wizard component (shown in an
//!   overlay) and the driver can share it.
//! - `Bun.env.OMP_SKIP_SETUP` becomes `std::env`, and `process.stdin/stdout`
//!   TTY probes become [`std::io::IsTerminal`].

use std::rc::Rc;

use crate::setup::scenes::composer::COMPOSER_SETUP_SCENE;
use crate::setup::scenes::glyph::GLYPH_SETUP_SCENE;
use crate::setup::scenes::model::MODEL_SETUP_SCENE;
use crate::setup::scenes::sign_in::PROVIDERS_SETUP_SCENE;
use crate::setup::scenes::theme::THEME_SETUP_SCENE;
use crate::setup::scenes::types::{
	SetupHost, SetupInput, SetupScene,
};
use crate::setup::scenes::updates::UPDATES_SETUP_SCENE;
use crate::setup::wizard_overlay::{setup_scene_applies, SetupWizardSession};
use crate::setup::scenes::splash::SETUP_TICK_MS;

pub use crate::setup::setup_version::CURRENT_SETUP_VERSION;
pub use crate::setup::startup_splash::run_startup_splash;

/// Ordered onboarding scenes with independent version gates.
pub const ALL_SCENES: [SetupScene; 6] = [
	PROVIDERS_SETUP_SCENE,
	MODEL_SETUP_SCENE,
	GLYPH_SETUP_SCENE,
	COMPOSER_SETUP_SCENE,
	THEME_SETUP_SCENE,
	UPDATES_SETUP_SCENE,
];

/// Environment and invocation gates for onboarding scene selection.
#[derive(Clone, Debug, Default)]
pub struct SetupSceneSelectionOptions {
	pub resuming: bool,
	/// `None` probes stdin/stdout.
	pub is_tty: Option<bool>,
	/// `None` falls back to `OMP_SKIP_SETUP`.
	pub skip_env: Option<String>,
	/// `None` treats the wizard as enabled.
	pub setup_wizard_enabled: Option<bool>,
	pub force: bool,
}

fn setup_skip_env_enabled(value: Option<&str>) -> bool {
	let Some(value) = value else {
		return false;
	};
	let normalized = value.trim().to_ascii_lowercase();
	!(normalized.is_empty()
		|| normalized == "0"
		|| normalized == "false"
		|| normalized == "no")
}

/// Whether this launch may show onboarding at all: a TTY and, unless forced,
/// not resuming, not skipped by `OMP_SKIP_SETUP` or `startup.setupWizard`.
pub fn setup_wizard_allowed(options: &SetupSceneSelectionOptions) -> bool {
	let is_tty = options.is_tty.unwrap_or_else(|| {
		use std::io::IsTerminal;
		std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
	});
	if !is_tty {
		return false;
	}
	if options.force {
		return true;
	}
	let env_skip = std::env::var("OMP_SKIP_SETUP").ok();
	let skip = options.skip_env.as_deref().or(env_skip.as_deref());
	!options.resuming && !setup_skip_env_enabled(skip) && options.setup_wizard_enabled != Some(false)
}

/// Select scenes newer than the stored version, honoring hard environment gates
/// ([`setup_wizard_allowed`]).
pub fn select_setup_scenes(
	stored_version: u32,
	scenes: &[SetupScene],
	ctx: Option<&dyn SetupHost>,
	options: &SetupSceneSelectionOptions,
) -> Vec<SetupScene> {
	if !setup_wizard_allowed(options) {
		return Vec::new();
	}
	let mut selected = Vec::new();
	for scene in scenes {
		if !options.force && scene.min_version <= stored_version {
			continue;
		}
		if let Some(should_run) = scene.should_run {
			let Some(ctx) = ctx else {
				continue;
			};
			if !should_run(ctx) {
				continue;
			}
		}
		selected.push(*scene);
	}
	selected
}

/// Control completion persistence and the post-setup welcome animation.
#[derive(Clone, Debug)]
pub struct RunSetupWizardOptions {
	pub mark_complete: bool,
	pub play_welcome_intro: bool,
}

impl Default for RunSetupWizardOptions {
	fn default() -> Self {
		RunSetupWizardOptions {
			mark_complete: true,
			play_welcome_intro: true,
		}
	}
}

/// Own the fullscreen setup overlay until its scenes and outro finish. When none
/// of `scenes` applies to the current renderer nothing is shown, but completion
/// is still persisted.
pub fn run_setup_wizard(
	ctx: Rc<dyn SetupHost>,
	scenes: Vec<SetupScene>,
	options: RunSetupWizardOptions,
) {
	if scenes.is_empty() {
		return;
	}
	if !scenes.iter().any(setup_scene_applies) {
		if options.mark_complete {
			ctx.mark_complete(CURRENT_SETUP_VERSION);
		}
		if options.play_welcome_intro {
			ctx.play_welcome_intro();
		}
		return;
	}
	let mut session = SetupWizardSession::new(ctx.clone(), scenes);
	let tick_ms = (SETUP_TICK_MS as u64).max(1);
	while !session.pump(ctx.now_ms()) {
		while let Some(input) = ctx.poll_input() {
			match input {
				SetupInput::Key(key) => session.handle_input(&key),
				SetupInput::Sgr(data) => session.handle_sgr(&data),
				SetupInput::Native(event) => session.handle_native_event(&event),
			}
		}
		let width = ctx.cols().max(1) as u16;
		let _ = session.render(width);
		ctx.request_render();
		std::thread::sleep(std::time::Duration::from_millis(tick_ms));
	}
	drop(session);
	if options.mark_complete {
		ctx.mark_complete(CURRENT_SETUP_VERSION);
	}
	if options.play_welcome_intro {
		ctx.play_welcome_intro();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn scene(id: &'static str, min_version: u32) -> SetupScene {
		SetupScene {
			id,
			title: id,
			step: id,
			min_version,
			ansi_only: false,
			should_run: None,
			mount: |_| unreachable!(),
		}
	}

	fn options() -> SetupSceneSelectionOptions {
		SetupSceneSelectionOptions {
			is_tty: Some(true),
			..SetupSceneSelectionOptions::default()
		}
	}

	#[test]
	fn all_scenes_are_ordered_and_versioned() {
		let ids: Vec<&str> = ALL_SCENES.iter().map(|scene| scene.id).collect();
		assert_eq!(
			ids,
			vec!["providers", "model", "glyph", "composer", "theme", "updates"]
		);
		let max = ALL_SCENES.iter().map(|scene| scene.min_version).max().unwrap();
		assert_eq!(max, CURRENT_SETUP_VERSION);
	}

	#[test]
	fn skip_env_truthiness() {
		let cases = [
			(None, false),
			(Some(""), false),
			(Some("0"), false),
			(Some("false"), false),
			(Some("no"), false),
			(Some("  FALSE "), false),
			(Some("1"), true),
			(Some("yes"), true),
		];
		for (value, expected) in cases {
			assert_eq!(setup_skip_env_enabled(value), expected, "{value:?}");
		}
	}

	#[test]
	fn wizard_allowed_needs_a_tty_unless_forced() {
		let mut opts = options();
		assert!(setup_wizard_allowed(&opts));
		opts.is_tty = Some(false);
		assert!(!setup_wizard_allowed(&opts));
		opts.force = true;
		assert!(setup_wizard_allowed(&opts));
	}

	#[test]
	fn wizard_allowed_honors_resume_skip_and_config() {
		let mut opts = options();
		opts.resuming = true;
		assert!(!setup_wizard_allowed(&opts));
		let mut opts = options();
		opts.skip_env = Some("1".to_string());
		assert!(!setup_wizard_allowed(&opts));
		let mut opts = options();
		opts.setup_wizard_enabled = Some(false);
		assert!(!setup_wizard_allowed(&opts));
	}

	#[test]
	fn selection_filters_by_stored_version_and_should_run() {
		let scenes = [scene("providers", 1), scene("model", 2), scene("theme", 3)];
		let selected = select_setup_scenes(1, &scenes, None, &options());
		assert_eq!(selected.iter().map(|scene| scene.id).collect::<Vec<_>>(), vec!["model", "theme"]);

		let mut opts = options();
		opts.force = true;
		let selected = select_setup_scenes(99, &scenes, None, &opts);
		assert_eq!(selected.len(), 3);

		let mut opts = options();
		opts.is_tty = Some(false);
		assert!(select_setup_scenes(0, &scenes, None, &opts).is_empty());
	}

	#[test]
	fn selection_requires_ctx_when_should_run_is_set() {
		let mut gated = scene("gated", 1);
		gated.should_run = Some(|_| true);
		let scenes = [gated];
		assert!(select_setup_scenes(0, &scenes, None, &options()).is_empty());
	}
}
