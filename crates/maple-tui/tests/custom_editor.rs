//! Port of `l/test/custom-editor.test.ts` (paste-path parts).
//!
//! Deviations: the reference also covers placeholder decoration, restored image
//! drafts, queue shorthand, space-hold push-to-talk and magic-keyword shimmer —
//! all host/keybinding/timer concerns absent from the crate's pure
//! [`CustomEditor`] data. The port covers the bracketed/plain paste path
//! recognition, `file://` decoding, shell sigils and segment splitting that the
//! reference asserts through those helpers.

use maple_tui::bracketed_paste::{PASTE_END, PASTE_START};
use maple_tui::prompt::custom_editor::{
	extract_bracketed_image_paste_path, extract_bracketed_image_paste_paths,
	extract_image_path_from_text, extract_paste_paths_from_text, is_previewable_attachment_path,
	normalize_pasted_path, split_pasted_path_segments, strip_shell_sigil,
};

fn bracketed(text: &str) -> String {
	format!("{PASTE_START}{text}{PASTE_END}")
}

#[test]
fn a_single_image_or_video_path_is_recognized_and_bare_names_are_not() {
	assert_eq!(extract_image_path_from_text("/tmp/screenshot.png").as_deref(), Some("/tmp/screenshot.png"));
	assert_eq!(
		extract_image_path_from_text("/Users/me/Pictures/photo.jpeg").as_deref(),
		Some("/Users/me/Pictures/photo.jpeg")
	);
	assert_eq!(
		extract_image_path_from_text("C:\\Users\\me\\img.gif").as_deref(),
		Some("C:\\Users\\me\\img.gif")
	);
	assert_eq!(
		extract_image_path_from_text("/Users/me/Movies/launch.mp4").as_deref(),
		Some("/Users/me/Movies/launch.mp4")
	);
	assert_eq!(extract_image_path_from_text("  /tmp/photo.webp\n").as_deref(), Some("/tmp/photo.webp"));
	assert_eq!(extract_image_path_from_text("icon.png"), None, "a bare filename is project-relative text");
	assert_eq!(extract_image_path_from_text("/tmp/report.csv"), None);
	assert_eq!(extract_image_path_from_text("/tmp/notes.txt"), None);
	assert_eq!(extract_image_path_from_text(""), None);
	assert_eq!(extract_image_path_from_text("   "), None);
}

#[test]
fn extra_words_or_a_second_path_are_rejected_but_one_spaced_path_survives() {
	assert_eq!(extract_image_path_from_text("see /tmp/screenshot.png"), None);
	assert_eq!(extract_image_path_from_text("/tmp/a.png /tmp/b.png"), None);
	assert_eq!(
		extract_image_path_from_text("/Users/me/Desktop/Screenshot 2026-06-25 at 1.23.45 PM.png").as_deref(),
		Some("/Users/me/Desktop/Screenshot 2026-06-25 at 1.23.45 PM.png")
	);
	assert_eq!(
		extract_image_path_from_text("~/Pictures/Cleanshot 2026-06-25 at 12.00.png").as_deref(),
		Some("~/Pictures/Cleanshot 2026-06-25 at 12.00.png")
	);
	assert_eq!(
		extract_image_path_from_text("C:\\Users\\me\\My Pictures\\img with space.jpg").as_deref(),
		Some("C:\\Users\\me\\My Pictures\\img with space.jpg")
	);
	assert_eq!(extract_image_path_from_text("/tmp/a.png /tmp/b shot.png"), None);
	assert_eq!(extract_image_path_from_text("see /Users/me/Desktop/Screenshot 1.png"), None);
}

#[test]
fn plain_and_bracketed_payloads_share_the_same_path_rules() {
	assert_eq!(
		extract_paste_paths_from_text("/tmp/a.png /tmp/b.png"),
		Some(vec!["/tmp/a.png".to_string(), "/tmp/b.png".to_string()])
	);
	assert_eq!(extract_paste_paths_from_text("just text"), None);

	let payload = bracketed("/tmp/icon-photo-default.png");
	assert_eq!(
		extract_bracketed_image_paste_paths(&payload),
		Some(vec!["/tmp/icon-photo-default.png".to_string()])
	);
	assert_eq!(
		extract_bracketed_image_paste_path(&bracketed("./images/icon.png")).as_deref(),
		Some("./images/icon.png")
	);
	assert_eq!(extract_bracketed_image_paste_paths(&bracketed("icon-photo-default.png")), None);
	assert_eq!(extract_bracketed_image_paste_paths(&bracketed("api/file/icon/867d45144217eec6d3c5805fd5a2d548.png")), None);
	assert_eq!(
		extract_bracketed_image_paste_paths(&bracketed("/Users/me/Movies/launch cut.mp4")),
		Some(vec!["/Users/me/Movies/launch cut.mp4".to_string()])
	);
	assert_eq!(extract_bracketed_image_paste_paths("not a paste"), None);
}

#[test]
fn file_urls_unwrap_and_shell_escapes_are_normalized() {
	assert_eq!(
		extract_image_path_from_text("file:///Users/me/Pictures/photo.png").as_deref(),
		Some("/Users/me/Pictures/photo.png")
	);
	assert_eq!(normalize_pasted_path("\"quoted path.png\"").as_str(), "quoted path.png");
	assert_eq!(normalize_pasted_path("/tmp/a\\ b.png").as_str(), "/tmp/a b.png");
	assert_eq!(normalize_pasted_path("\\~/Pictures/image.png").as_str(), "~/Pictures/image.png");
	assert_eq!(normalize_pasted_path("C:\\~\\capture.png").as_str(), "C:\\~\\capture.png");
	assert_eq!(strip_shell_sigil("!!ls -la"), "ls -la");
	assert_eq!(strip_shell_sigil("$ python -V"), "python -V");
	assert_eq!(strip_shell_sigil("plain text"), "plain text");
}

#[test]
fn segments_split_on_unquoted_whitespace_and_quotes_stay_intact() {
	assert_eq!(
		split_pasted_path_segments("/tmp/a.png\t./b.png"),
		Some(vec!["/tmp/a.png".to_string(), "./b.png".to_string()])
	);
	assert_eq!(
		split_pasted_path_segments("\"/tmp/my shots/a.png\""),
		Some(vec!["\"/tmp/my shots/a.png\"".to_string()])
	);
	assert_eq!(
		split_pasted_path_segments("/tmp/a\\ b.png"),
		Some(vec!["/tmp/a\\ b.png".to_string()])
	);
	assert_eq!(split_pasted_path_segments("/tmp/unclosed\""), None);
	assert_eq!(split_pasted_path_segments("/tmp/trailing\\"), None);
	assert_eq!(split_pasted_path_segments("   "), None);
	assert!(is_previewable_attachment_path("/tmp/a.PNG"));
}
