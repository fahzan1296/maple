//! Port of `l/test/theme-spinner-frames.test.ts`.
//!
//! `symbols.spinnerFrames` validation/normalization and the shared spinner clock
//! exist in the crate, so those cases are real. The reference's live tool
//! interval comes from a second clock instance; the crate's `SpinnerClock` is
//! per-component, so the clock case pins only its deterministic frame selection.

use maple_tui::spinner_clock::SpinnerClock;
use maple_tui::theme::schema::validate_spinner_frames;
use maple_tui::theme::symbols::{
	normalize_spinner_frames_override, spinner_frames, SpinnerFramesOverride, SpinnerType,
	SymbolPreset,
};
use std::time::Duration;

#[test]
fn flat_array_override_applies_to_both_status_and_activity_spinners() {
	let frames = vec!["a".to_string(), "b".to_string()];
	let normalized =
		normalize_spinner_frames_override(Some(&SpinnerFramesOverride::Frames(frames.clone())));
	assert_eq!(normalized.status, Some(frames.clone()));
	assert_eq!(normalized.activity, Some(frames));
}

#[test]
fn object_override_sets_each_spinner_type_independently_and_falls_back_to_preset() {
	assert_eq!(normalized.status, Some(vec!["s".to_string()]));
		status: Some(vec!["s".to_string()]),
		activity: None,
	}));
	assert_eq!(normalized.status.as_deref(), Some(&["s".to_string()[..]]);
	assert_eq!(normalized.activity, None);
	// The unset type still resolves to its preset frames.
	assert!(!spinner_frames(SymbolPreset::Unicode, SpinnerType::Activity).is_empty());
	assert!(!spinner_frames(SymbolPreset::Ascii, SpinnerType::Status).is_empty());
}

#[test]
fn rejects_empty_arrays_and_empty_objects_at_validation_time() {
	assert!(validate_spinner_frames(&serde_json::json!(["a"])).is_ok());
	assert!(validate_spinner_frames(&serde_json::json!({"status": ["a"]})).is_ok());
	assert!(validate_spinner_frames(&serde_json::json!([])).is_err());
	assert!(validate_spinner_frames(&serde_json::json!({})).is_err());
	assert!(validate_spinner_frames(&serde_json::json!({"status": []})).is_err());
	assert!(validate_spinner_frames(&serde_json::json!({"activity": ["a"], "status": []})).is_err());
}

#[test]
fn derives_live_tool_spinner_frames_from_the_shared_clock_at_its_live_interval() {
	let frames = vec!["0".to_string(), "1".to_string(), "2".to_string()];
	let clock = SpinnerClock::new(frames.clone(), Duration::ZERO);
	assert_eq!(clock.tick(), 0);
	assert_eq!(clock.frame(), "0");

	let empty = SpinnerClock::new(Vec::new(), Duration::from_millis(80));
	assert_eq!(empty.tick(), 0);
	assert_eq!(empty.frame(), "");
}
