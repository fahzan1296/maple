//! Port of `l/test/issue-848-repro.test.ts`.
//!
//! The reference regression is about the JS wrapper forwarding `null` napi
//! arguments; the Rust API has no nullish inputs. The behaviours the wrapper
//! had to preserve (concrete defaults, no throw) are pinned directly, and the
//! nullish-argument cases are ignored.

use maple_tui::text_shape::{truncate_to_width, Ellipsis};

#[test]
fn returns_the_original_text_when_it_fits() {
	assert_eq!(truncate_to_width("hello world", 80, Ellipsis::Unicode), "hello world");
	assert_eq!(truncate_to_width("hello world", 80, Ellipsis::Omit), "hello world");
}

#[test]
fn maps_a_legacy_empty_string_ellipsis_argument_to_omit_ellipsis() {
	assert_eq!(truncate_to_width("hello world", 5, Ellipsis::Omit), "hello");
}

#[test]
fn truncates_without_throwing_on_narrow_widths() {
	assert_eq!(truncate_to_width("hello world", 0, Ellipsis::Omit), "hello world");
	assert!(!truncate_to_width("hello world", 5, Ellipsis::Unicode).is_empty());
}

#[test]
#[ignore = "missing facility: the crate has no napi/JS wrapper, so nullish ellipsis/pad arguments cannot be expressed"]
fn nullish_ellipsis_and_pad_do_not_throw() {
	assert_eq!(truncate_to_width("hello world", 80, Ellipsis::Unicode), "hello world");
}

#[test]
#[ignore = "missing facility: the crate has no napi/JS wrapper, so a nullish maxWidth cannot be expressed"]
fn nullish_max_width_does_not_throw() {
	assert_eq!(truncate_to_width("hello world", 0, Ellipsis::Omit), "hello world");
}
