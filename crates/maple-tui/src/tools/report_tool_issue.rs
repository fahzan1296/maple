//! Call preview helpers for the `xd://report_issue` device.
//!
 //! Reference: `l/src/tools/report-tool-issue.ts`.
 //!
 //! Deviations:
//! - JS-runtime-only TUI `Component` renderer wiring dropped. Themed/plain
//!   render helpers are provided (`*_themed` take `&Theme`); native describe
//!   helpers emit token-styled nodes for theme-aware host rendering.
//! - TS delegates rendering to resolve device-preview helpers; Rust ports device
//!   identifiers, preview extraction, and render/describe helpers.
/// Device name for automatic tool issue reports.
pub const REPORT_ISSUE_DEVICE_NAME: &str = "report_issue";

/// Internal device URL for automatic tool issue reports.
pub const REPORT_ISSUE_DEVICE_PATH: &str = "xd://report_issue";

/// Native/plain call preview title for an `xd://report_issue` write.
pub const REPORT_ISSUE_PREVIEW_TITLE: &str = "Report Tool Issue";

/// First line of a device write payload, for call-preview descriptions.
pub fn preview_first_line(content: Option<&str>) -> Option<String> {
	let body = content?.trim();
	if body.is_empty() {
		return None;
	}
	let first = body.split('\n').next().unwrap_or("").trim();
	if first.is_empty() {
		None
	} else {
		Some(first.to_string())
	}
}

/// Plain render of an `xd://report_issue` call preview.
pub fn render_call(content: Option<&str>) -> String {
	match preview_first_line(content) {
		Some(line) => format!("{REPORT_ISSUE_PREVIEW_TITLE}: {line}"),
		None => REPORT_ISSUE_PREVIEW_TITLE.to_string(),
	}
}

/// Themed render of an `xd://report_issue` call preview.
pub fn render_call_themed(content: Option<&str>, theme: &crate::theme::Theme) -> String {
	let title = theme.fg("warning", REPORT_ISSUE_PREVIEW_TITLE).unwrap_or_else(|_| REPORT_ISSUE_PREVIEW_TITLE.to_string());
	match preview_first_line(content) {
		Some(line) => format!("{title}: {}", theme.fg("muted", &line).unwrap_or(line)),
		None => title,
	}
}

/// Themed render of an `xd://report_issue` result.
pub fn render_result_themed(output: &str, is_error: bool, theme: &crate::theme::Theme) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		let head = theme.fg("error", "Report failed").unwrap_or_else(|_| "Report failed".into());
		return vec![if msg.is_empty() { head } else { format!("{head}: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	if output.trim().is_empty() {
		return vec![theme.fg("success", "Issue reported").unwrap_or_else(|_| "Issue reported".into())];
	}
	output.trim_end().lines().take(20).map(|s| theme.fg("toolOutput", s).unwrap_or_else(|_| s.to_string())).collect()
}
/// Plain render of an `xd://report_issue` result.
pub fn render_result(output: &str, is_error: bool) -> Vec<String> {
	if is_error {
		let msg = output.trim();
		return vec![if msg.is_empty() { "Report failed".into() } else { format!("Report failed: {}", msg.lines().next().unwrap_or(msg)) }];
	}
	if output.trim().is_empty() { vec!["Issue reported".into()] } else { output.trim_end().lines().take(20).map(|s| s.to_string()).collect() }
}

/// Native describe of an `xd://report_issue` call preview.
pub fn describe_call(content: Option<&str>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let details = preview_first_line(content)
		.map(|line| vec![ToolHeadDetail::Span(Span::styled(line, "muted"))])
		.unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head(REPORT_ISSUE_PREVIEW_TITLE, &details))), inline: Some(true), ..NativeToolView::default() }
}

/// Native describe of an `xd://report_issue` result.
pub fn describe_result(output: &str, is_error: bool) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	if is_error {
		let msg = output.trim();
		return error_view(REPORT_ISSUE_PREVIEW_TITLE, if msg.is_empty() { "failed" } else { msg }, &[]);
	}
	let head = tool_head(REPORT_ISSUE_PREVIEW_TITLE, &[]);
	let body = output.trim_end();
	if body.is_empty() {
		return NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![note_text("Issue reported", "dim", None)]), tone: Some(Tone::Success), ..NativeToolView::default() };
	}
	let mut p = Props::new();
	p.set_str("wrap", "word");
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![ntext(NodeText::Plain(body.to_string()), p)]), tone: Some(Tone::Success), ..NativeToolView::default() }
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let c = render_call(Some("bad tool\nmore"));
		assert!(c.contains(REPORT_ISSUE_PREVIEW_TITLE));
		let v = describe_call(Some("bad tool"));
		assert!(v.head.is_some());
		assert_eq!(render_result("", false)[0], "Issue reported");
		let r = describe_result("ok", false);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn constants_match_device() {
		assert_eq!(REPORT_ISSUE_DEVICE_NAME, "report_issue");
		assert_eq!(REPORT_ISSUE_DEVICE_PATH, "xd://report_issue");
	}

	#[test]
	fn preview_extracts_first_line() {
		assert_eq!(preview_first_line(Some("hello\nworld")), Some("hello".into()));
		assert_eq!(preview_first_line(Some("  \n  ")), None);
		assert_eq!(preview_first_line(None), None);
	}
}
