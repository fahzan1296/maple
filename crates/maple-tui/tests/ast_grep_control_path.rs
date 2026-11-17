//! Port of `l/test/ast-grep-control-path.test.ts`.
//!
//! Deviations: the reference builds a `ToolExecutionComponent` and reads its
//! rendered rows. The crate's ast-grep renderer is the `describe_result` native
//! view, so the test inspects that view's body nodes for the link target and
//! the visible label.

use maple_tui::native::node::NativeNode;
use maple_tui::tools::ast_grep::{AstGrepToolDetails, describe_result, render_result};

const PRIVATE_MODE: &str = "\x1b[?1049h\x1b[?1003h";

fn collect(node: &NativeNode, hrefs: &mut Vec<String>, text: &mut String) {
	if let Some(spans) = node.props.spans() {
		for span in spans {
			text.push_str(&span.t);
			if let Some(href) = span.href {
				hrefs.push(href);
			}
		}
	} else if let Some(plain) = node.text() {
		text.push_str(plain);
	}
	for child in &node.children {
		collect(child, hrefs, text);
	}
}

#[test]
fn links_the_original_file_without_exposing_its_terminal_controls_in_the_label() {
	let dir = std::env::temp_dir().join(format!(
		"omp-ast-grep-link-{}-{}",
		std::process::id(),
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_default()
			.as_nanos()
	));
	std::fs::create_dir_all(&dir).expect("temp dir");
	let filename = format!("fixture{PRIVATE_MODE}.ts");
	let absolute = dir.join(&filename);
	std::fs::write(&absolute, "needle").expect("fixture file");

	let display_content = format!("# {filename}\n*1│needle");
	let details = AstGrepToolDetails {
		match_count: 1,
		file_count: 1,
		files_searched: 1,
		display_content: Some(display_content.clone()),
		cwd: Some(dir.to_string_lossy().into_owned()),
		search_path: Some(dir.to_string_lossy().into_owned()),
		..AstGrepToolDetails::default()
	};

	let view = describe_result(Some(&details), &display_content, false);
	let body = view.body.expect("body nodes");
	let mut hrefs = Vec::new();
	let mut text = String::new();
	for node in &body {
		collect(node, &mut hrefs, &mut text);
	}

	assert!(
		hrefs.iter().any(|href| href.starts_with("file://") && href.contains("fixture")),
		"expected a file link to the fixture, got {hrefs:?}"
	);
	assert!(
		!text.contains(PRIVATE_MODE) && !text.contains('\x1b'),
		"visible label leaked terminal controls: {text:?}"
	);
	assert_eq!(std::fs::read_to_string(&absolute).expect("fixture"), "needle");

	let _ = std::fs::remove_dir_all(&dir);
}


#[test]
fn plain_render_strips_control_bytes_from_the_output() {
	let display = format!("# fixture{PRIVATE_MODE}.ts\n*1│needle");
	let details = AstGrepToolDetails {
		match_count: 1,
		file_count: 1,
		files_searched: 1,
		display_content: Some(display.clone()),
		..AstGrepToolDetails::default()
	};

	let rows = render_result(Some(&details), &display, false);
	let joined = rows.join("\n");
	assert!(!joined.contains('\x1b'), "plain rows leaked controls: {joined:?}");
	assert!(joined.contains("needle"), "rows: {joined:?}");
}

#[test]
fn unresolved_relative_header_keeps_a_sanitized_label_without_a_link() {
	let display = format!("# fixture{PRIVATE_MODE}.ts\n*1│needle");
	let details = AstGrepToolDetails {
		match_count: 1,
		file_count: 1,
		files_searched: 1,
		display_content: Some(display.clone()),
		..AstGrepToolDetails::default()
	};

	let view = describe_result(Some(&details), &display, false);
	let body = view.body.expect("body nodes");
	let mut hrefs = Vec::new();
	let mut text = String::new();
	for node in &body {
		collect(node, &mut hrefs, &mut text);
	}

	assert!(hrefs.is_empty(), "relative header without a base must not link: {hrefs:?}");
	assert!(text.contains("fixture.ts"), "label: {text:?}");
	assert!(!text.contains('\x1b'), "label leaked controls: {text:?}");
}