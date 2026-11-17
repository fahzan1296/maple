//! Port of `l/test/grep-render.test.ts`.
//!
//! The crate has no terminal grep renderer; it ports the native describe view
//! (`grep::describe_result`) and the plain call/head helpers. Cases that assert
//! terminal layout (tool column, tree branches, syntax colors, OSC-8 links,
//! grouped-file cutting, wrapping) stay ignored.

use maple_tui::native::node::{NativeNode, Text as NodeText};
use maple_tui::tools::grep::{
	describe_result, format_count, grep_native_head_meta, grep_subject_plain, GrepRenderArgs,
	GrepToolDetails,
};
use maple_tui::tools::native_view::{NativeToolHead, NativeToolView};

fn head_meta(head: &NativeToolHead) -> Vec<String> {
	head.meta
		.as_ref()
		.map(|meta| {
			meta.iter()
				.map(|part| match part {
					NodeText::Plain(text) => text.clone(),
					NodeText::Spans(spans) => {
						spans.iter().map(|span| span.t.clone()).collect::<Vec<_>>().join("")
					}
				})
				.collect()
		})
		.unwrap_or_default()
}

fn node_text(node: &NativeNode) -> String {
	let mut out = String::new();
	if let Some(text) = node.props.get_str("text") {
		out.push_str(text);
	}
	if let Some(spans) = node.props.get("spans").and_then(|value| value.as_array()) {
		for span in spans {
			if let Some(text) = span.get("t").and_then(|value| value.as_str()) {
				out.push_str(text);
			}
		}
	}
	for child in &node.children {
		out.push_str(&node_text(child));
	}
	out
}

fn body_text(view: &NativeToolView) -> String {
	view.body
		.as_ref()
		.map(|nodes| nodes.iter().map(node_text).collect::<Vec<_>>().join("\n"))
		.unwrap_or_default()
}

#[test]
fn keeps_binary_match_warnings_in_empty_and_matching_previews() {
	for (match_count, display) in [
		(0usize, "".to_string()),
		(1, "## src/a.ts\n*1│needle".to_string()),
	] {
		let details = GrepToolDetails {
			match_count: Some(match_count),
			file_count: Some(match_count),
			binary_matches: vec!["a.bin".to_string(), "b.bin".to_string()],
			display_content: if display.is_empty() { None } else { Some(display) },
			..GrepToolDetails::default()
		};
		let view = describe_result("", Some(&details), false);
		let body = body_text(&view);
		assert!(
			body.contains("binary matches: 2 files"),
			"match_count {match_count}: {body}"
		);
	}
}

#[test]
fn names_the_search_scope_right_after_the_pattern_before_the_counts() {
	let details = GrepToolDetails {
		match_count: Some(1),
		file_count: Some(1),
		scope_path: Some("src".to_string()),
		display_content: Some("## src/a.ts\n*1│needle".to_string()),
		..GrepToolDetails::default()
	};
	let view = describe_result("", Some(&details), false);
	assert_eq!(
		head_meta(&view.tool.expect("head")),
		vec!["in src", "1 match · 1 file"]
	);
}

#[test]
fn omits_the_file_count_only_when_the_scope_names_the_single_file() {
	let single = GrepToolDetails {
		match_count: Some(2),
		file_count: Some(1),
		display_content: Some("*1│needle\n*5│needle".to_string()),
		..GrepToolDetails::default()
	};
	let with_scope = GrepToolDetails {
		scope_path: Some("src/a.ts".to_string()),
		..single.clone()
	};
	assert_eq!(
		head_meta(&describe_result("", Some(&with_scope), false).tool.expect("head")),
		vec!["in src/a.ts", "2 matches"]
	);
	assert_eq!(
		head_meta(&describe_result("", Some(&single), false).tool.expect("head")),
		vec!["2 matches · 1 file"]
	);
}

#[test]
fn renders_zero_matches_with_a_warning_tone_and_keeps_the_missing_path_note() {
	let details = GrepToolDetails {
		match_count: Some(0),
		file_count: Some(0),
		scope_path: Some("src".to_string()),
		missing_paths: vec!["gone/".to_string()],
		..GrepToolDetails::default()
	};
	let view = describe_result("", Some(&details), false);
	assert_eq!(head_meta(&view.tool.expect("head")), vec!["in src", "0 matches"]);
	assert_eq!(
		view.tone.map(|tone| tone.as_str().to_string()),
		Some("warning".to_string())
	);
	let body = body_text(&view);
	assert!(body.contains("skipped missing: gone/"), "{body}");
}

#[test]
fn keeps_truncation_status_in_the_header() {
	let details = GrepToolDetails {
		match_count: Some(6),
		file_count: Some(3),
		file_limit_reached: Some(3),
		per_file_limit_reached: Some(20),
		truncated: true,
		display_content: Some("alpha:1\nalpha:2\n\nbeta:1\nbeta:2\n\ngamma:1\ngamma:2".to_string()),
		..GrepToolDetails::default()
	};
	let head = describe_result("", Some(&details), false).tool.expect("head");
	let badges = head
		.badges
		.as_ref()
		.expect("truncation badge")
		.iter()
		.map(|badge| badge.text.clone())
		.collect::<Vec<_>>();
	assert_eq!(badges, vec!["truncated"]);
}

#[test]
fn call_subject_then_native_scope_and_flag_meta() {
	let args = GrepRenderArgs {
		pattern: Some("needle".to_string()),
		path: vec!["lib".to_string()],
		case_sensitive: Some(false),
		..GrepRenderArgs::default()
	};
	assert_eq!(
		grep_subject_plain(Some("needle"), Some(&args), None).expect("subject"),
		"needle in lib"
	);
	assert_eq!(
		grep_native_head_meta(Some(&args), None),
		vec!["in lib", "case:insensitive"]
	);
	assert_eq!(format_count("file", 2), "2 files");
}

#[test]
#[ignore = "terminal-only: tool column alignment and accent-free header color"]
fn starts_grep_output_at_the_tool_column() {}

#[test]
#[ignore = "terminal-only: tree branch rows per directory file"]
fn gives_each_file_of_a_directory_its_own_branch() {}

#[test]
#[ignore = "terminal-only: regex syntax coloring in the call header"]
fn colors_partial_regex_syntax() {}

#[test]
#[ignore = "terminal-only: error title/subject rows"]
fn shows_the_title_and_subject_on_an_error() {}

#[test]
#[ignore = "terminal-only: grouped-file collapsed cutting with a match summary"]
fn cuts_the_collapsed_list_between_files() {}

#[test]
#[ignore = "terminal-only: OSC-8 links on grouped headers and code-frame lines"]
fn links_grouped_file_headers_and_code_frame_lines() {}

#[test]
#[ignore = "terminal-only: OSC-8 links on sanitized control-byte filenames"]
fn links_sanitized_grouped_headers_to_the_original_filename() {}

#[test]
#[ignore = "terminal-only: sanitized single-file and URL display targets"]
fn retains_display_targets_after_detail_sanitization() {}

#[test]
#[ignore = "terminal-only: OSC-8 links on single-file code-frame lines"]
fn links_single_file_code_frame_lines() {}

#[test]
#[ignore = "terminal-only: expanded single-file match bound"]
fn bounds_the_expanded_single_file_view() {}

#[test]
#[ignore = "terminal-only: long-pattern header wrapping"]
fn wraps_a_long_pattern_header() {}
