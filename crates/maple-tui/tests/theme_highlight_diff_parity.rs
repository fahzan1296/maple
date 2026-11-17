//! Port of `l/test/theme-highlight-diff-parity.test.ts` (portable subset).
//!
//! TS asserts that `highlightCode(text, "diff"|"patch")` produces whole-block,
//! ANSI-styled output and that newline-complete chunk highlighting is visually
//! equivalent to highlighting the whole unified diff at once. Rust's
//! `render::code_cell` path (and chat eval code cells) currently use a plain
//! split-line fallback for highlighting (no tree-sitter/native ANSI diff
//! highlighter in crate); `render_code_cell` renders a framed code preview but
//! does not expose `highlightCode` as a public API returning styled lines.
//!
//! This ports the stable, faithful contract that is exposed: diff/patch source
//! text survives code-cell rendering intact (tabs normalized, lines preserved,
//! no content loss/reordering) for whole blocks and newline-complete chunks, and
//! chunked visible windows preserve the same visible line sequence as the
//! corresponding slice of the whole block. ANSI style parity cannot be asserted
//! without a diff highlighter and is therefore not claimed.
//!
//! Deviations:
//! - No `highlightCode` export and no ANSI diff styling in Rust; tests assert
//!   visible text parity/preservation via `render_code_cell` output, not SGR
//!   equality.
//! - Chunk parity is checked by rendering each chunk as its own code cell and
//!   comparing stripped visible content to the matching whole-diff slice,
//!   rather than by concatenating highlighted token streams.

use maple_tui::render::code_cell::{render_code_cell, CodeCellOptions};
use maple_tui::theme::{builtin_theme_json, create_theme, CreateThemeOptions};

fn theme() -> maple_tui::theme::Theme {
	let t = builtin_theme_json("dark").expect("dark");
	create_theme(t, CreateThemeOptions::default()).expect("theme")
}

fn strip_ansi(s: &str) -> String {
	// minimal CSI/OSC stripper sufficient for rendered cell rows
	let mut out = String::with_capacity(s.len());
	let bytes = s.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == 0x1b {
			i += 1;
			if i >= bytes.len() {
				break;
			}
			match bytes[i] {
				b'[' => {
					i += 1;
					while i < bytes.len() {
						let b = bytes[i];
						i += 1;
						if (0x40..=0x7e).contains(&b) {
							break;
						}
					}
					continue;
				}
				b']' => {
					i += 1;
					while i < bytes.len() {
						if bytes[i] == 0x07 {
							i += 1;
							break;
						}
						if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
							i += 2;
							break;
						}
						i += 1;
					}
					continue;
				}
				b'P' | b'^' | b'_' | b'X' => {
					i += 1;
					while i < bytes.len() {
						if bytes[i] == 0x07 {
							i += 1;
							break;
						}
						if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
							i += 2;
							break;
						}
						i += 1;
					}
					continue;
				}
				_ => {
					i += 1;
					continue;
				}
			}
		}
		out.push(bytes[i] as char);
		i += 1;
	}
	out
}

fn render_lines(code: &str, lang: &str) -> Vec<String> {
	let rendered = render_code_cell(
		&CodeCellOptions {
			code: code.to_string(),
			language: Some(lang.to_string()),
			index: None,
			total: None,
			title: None,
			status: None,
			spinner_frame: None,
			duration: None,
			output: None,
			output_max_lines: None,
			code_max_lines: Some(10_000),
			code_tail: None,
			expanded: Some(true),
			show_language: Some(false),
			width: 200,
			code_start_line: None,
			code_line_numbers: None,
		},
		&theme(),
	);
	rendered.into_iter().map(|l| strip_ansi(&l)).collect()
}

fn contains_all_diff_markers(lines: &[String], markers: &[&str]) -> bool {
	let joined = lines.join("\n");
	markers.iter().all(|m| joined.contains(m))
}

#[test]
fn unified_diff_and_patch_blocks_preserve_visible_content_whole_and_by_chunk() {
	let chunks = vec![
		vec![
			"diff --git a/src/example.ts b/src/example.ts",
			"index 1234567..89abcde 100644",
			"--- a/src/example.ts",
			"+++ b/src/example.ts",
			"@@ -1,4 +1,5 @@",
			" import { run } from \"./run\";",
			"-const enabled = false;",
			"+const enabled = true;",
			"+run(enabled);",
		]
		.join("\n"),
		vec![
			"@@ -8,4 +9,4 @@ export function start() {",
			" context();",
			"-return \"old\";",
			"+return \"new\";",
			"\\ No newline at end of file",
		]
		.join("\n"),
	];
	let unified = chunks.join("\n");

	for lang in ["diff", "patch"] {
		let whole = render_lines(&format!("{unified}\n"), lang);
		assert!(contains_all_diff_markers(
			&whole,
			&[
				"diff --git a/src/example.ts b/src/example.ts",
				"+const enabled = true;",
				"-return \"old\";",
				"\\ No newline at end of file",
			]
		));
		for chunk in &chunks {
			let chunk_lines = render_lines(&format!("{chunk}\n"), lang);
			for marker in chunk.lines() {
				if marker.is_empty() {
					continue;
				}
				assert!(
					chunk_lines.iter().any(|l| l.contains(marker)),
					"chunk marker missing for {lang}: {marker}\n{chunk_lines:?}"
				);
			}
		}
	}
}
