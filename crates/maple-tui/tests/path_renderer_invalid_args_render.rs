//! Reference: `l/test/path-renderer-invalid-args-render.test.ts`.
//!
//! The reference feeds the read/write/edit renderers provider paths that are
//! arrays and objects and asserts they still render. The crate's render args
//! type the path as `Option<String>`, so the same provider garbage arrives as
//! an absent path (`None`) or an empty one; each renderer is asserted to
//! survive both and still render its header and content.

use std::panic::{catch_unwind, AssertUnwindSafe};

use maple_tui::tools::edit::{EditRenderArgs, EditToolDetails};
use maple_tui::tools::read::{ReadDisplayContent, ReadRenderArgs, ReadToolDetails};
use maple_tui::tools::write::{WriteRenderArgs, WriteToolDetails};

const CASES: [&str; 2] = ["array path", "object path"];

fn invalid_path(case: &str) -> Option<String> {
	// Both provider shapes are non-strings; the crate's typed args can only
	// represent them as a missing path or an empty one.
	match case {
		"array path" => None,
		_ => Some(String::new()),
	}
}

#[test]
fn read_renderer_does_not_throw_for_invalid_path_arguments() {
	for case in CASES {
		let args = ReadRenderArgs { path: invalid_path(case), ..ReadRenderArgs::default() };
		let call = catch_unwind(AssertUnwindSafe(|| maple_tui::tools::read::render_call(Some(&args))))
			.unwrap_or_else(|_| panic!("read render_call panicked for {case}"));
		assert!(call.contains("Read"), "read call header missing for {case}: {call}");

		let details = ReadToolDetails {
			display_content: Some(ReadDisplayContent {
				text: "hello from read".into(),
				start_line: 1,
				line_numbers: Vec::new(),
			}),
			content_type: Some("text/plain".into()),
			..ReadToolDetails::default()
		};
		let result = catch_unwind(AssertUnwindSafe(|| {
			maple_tui::tools::read::render_result("hello from read", Some(&details), Some(&args), false, false)
		}))
		.unwrap_or_else(|_| panic!("read render_result panicked for {case}"));
		let rendered = result.join("\n");
		assert!(rendered.contains("hello from read"), "read result content missing for {case}: {rendered}");
	}
}

#[test]
fn write_renderer_does_not_throw_for_invalid_path_arguments() {
	for case in CASES {
		let args = WriteRenderArgs {
			path: invalid_path(case),
			content: Some("first line\nsecond line".into()),
			..WriteRenderArgs::default()
		};
		let call = catch_unwind(AssertUnwindSafe(|| maple_tui::tools::write::render_call(Some(&args))))
			.unwrap_or_else(|_| panic!("write render_call panicked for {case}"));
		assert!(call.contains("Write"), "write call header missing for {case}: {call}");

		let details = WriteToolDetails { resolved_path: Some("/tmp/example.ts".into()), ..WriteToolDetails::default() };
		let result = catch_unwind(AssertUnwindSafe(|| {
			maple_tui::tools::write::render_result(Some(&args), Some(&details), false, false, "", false)
		}))
		.unwrap_or_else(|_| panic!("write render_result panicked for {case}"));
		let rendered = result.join("\n");
		assert!(rendered.contains("Write"), "write result header missing for {case}: {rendered}");
		assert!(rendered.contains("first line"), "write result content missing for {case}: {rendered}");
	}
}

#[test]
fn edit_renderer_does_not_throw_for_invalid_path_arguments() {
	for case in CASES {
		let args = EditRenderArgs {
			path: invalid_path(case),
			old_text: Some("before".into()),
			new_text: Some("after".into()),
			..EditRenderArgs::default()
		};
		let call = catch_unwind(AssertUnwindSafe(|| maple_tui::tools::edit::render_call(Some(&args), None)))
			.unwrap_or_else(|_| panic!("edit render_call panicked for {case}"));
		assert!(call.contains("Edit"), "edit call header missing for {case}: {call}");

		let details = EditToolDetails { diff: "-before\n+after".into(), ..EditToolDetails::default() };
		let result = catch_unwind(AssertUnwindSafe(|| {
			maple_tui::tools::edit::render_result("updated", Some(&details), Some(&args), false, false)
		}))
		.unwrap_or_else(|_| panic!("edit render_result panicked for {case}"));
		let rendered = result.join("\n");
		assert!(rendered.contains("Edit"), "edit result header missing for {case}: {rendered}");
		assert!(rendered.contains("after"), "edit result diff missing for {case}: {rendered}");
	}
}
