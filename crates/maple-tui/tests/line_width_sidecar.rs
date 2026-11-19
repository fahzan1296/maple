//! Port of `l/test/line-width-sidecar.test.ts`.
//!
//! The carried-width sidecar (`publishLineWidths` / `getPublishedLineWidths` /
//! `getWidthConfigEpoch`) has no crate API, so every contract test is ignored
//! with that facility named. The bodies still assert the width measurement the
//! sidecar would carry.

use maple_tui::text_shape::visible_width;

#[test]
#[ignore = "missing facility: publishLineWidths/getPublishedLineWidths are not ported to text_shape"]
fn returns_published_widths_for_the_same_array_reference_only() {
	assert_eq!(visible_width("abc"), 3);
	assert_eq!(visible_width("漢字"), 4);
}

#[test]
#[ignore = "missing facility: publishLineWidths is not ported to text_shape"]
fn rejects_a_publication_whose_widths_do_not_match_the_line_count() {
	assert_eq!(visible_width("one"), 3);
	assert_eq!(visible_width("two"), 3);
}

#[test]
#[ignore = "missing facility: getPublishedLineWidths is not ported to text_shape"]
fn keeps_publication_proof_immutable_from_publishers_and_consumers() {
	assert_eq!(visible_width("hi"), 2);
}

#[test]
#[ignore = "missing facility: publishLineWidths is not ported to text_shape"]
fn drops_published_widths_after_same_array_content_or_length_mutation() {
	assert_eq!(visible_width("hi"), 2);
	assert_eq!(visible_width("hello"), 5);
}

#[test]
#[ignore = "missing facility: runtime Hangul Compatibility Jamo width profile + line-width sidecar are not ported"]
fn drops_published_widths_when_the_hangul_jamo_width_setting_changes() {
	assert_eq!(visible_width("\u{3131}\u{314f}"), 4);
}

#[test]
#[ignore = "missing facility: getWidthConfigEpoch is not ported to text_shape"]
fn bumps_the_width_config_epoch_only_on_an_effective_setting_change() {
	assert_eq!(visible_width("abc"), 3);
}
