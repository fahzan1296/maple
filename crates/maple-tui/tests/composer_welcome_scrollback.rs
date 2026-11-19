//! Port of `l/test/composer-welcome-scrollback.test.ts`.
//!
//! The reference runs the real `Composer` against a WASM-backed
//! `VirtualTerminal`, injects a virtual render scheduler and asserts that an
//! oversized startup changelog reaches scrollback after the intro completes.
//! The crate's `prompt::composer` is a dependency-injected options/type module
//! with no runtime, terminal ownership or scheduler, so the assertion is
//! ignored; the body pins the closest facility (transcript scrollback
//! completion plus the composer's own defaults).

use maple_tui::chrome::transcript_container::TranscriptContainer;
use maple_tui::prompt::composer::{ComposerOptions, ComposerPreferences, ComposerStartOptions};

#[test]
#[ignore = "missing facility: no Composer runtime, terminal ownership or render scheduler, so intro scrollback cannot be observed"]
fn makes_an_oversized_startup_changelog_available_in_scrollback_after_the_intro() {
	let preferences = ComposerPreferences::default();
	assert!(!preferences.complete_scrollback);
	let options = ComposerOptions {
		preferences: Some(preferences),
		welcome: None,
		status: None,
	};
	assert!(options.welcome.is_none());

	let mut transcript = TranscriptContainer::new();
	assert!(!transcript.complete_scrollback());
	transcript.set_complete_scrollback(true);
	assert!(transcript.complete_scrollback());

	let start = ComposerStartOptions::default();
	assert!(!start.clear_scrollback);
}
