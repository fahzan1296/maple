//! `cfg://` settings read/write transcript render-model helpers.
//!
//! Reference: `l/src/tools/cfg-render.ts`.
//!
//! Deviations:
//! - TS renders TUI components (highlighted YAML, badges, cards, theme styling,
//!   native describe nodes). Rust ports data types and plain/native describe
//!   helpers. Highlighted code/themed card rendering is host concern for
//!   renderer-completion pass.
//! - `parse_cfg_url` is a lightweight local parser (full URL parsing not
//!   assumed); callers may supply pre-parsed details.

use crate::native::describe::code;
use crate::native::node::{NodeText, Props};
use crate::tools::native_view::{error_view, tool_head, NativeToolView, ToolHeadDetail};

/// Summary of a `cfg://` read, attached to read tool details.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CfgReadDetails {
	/// Dotted setting path or namespace that was read; empty for the whole tree.
	pub path: String,
	/// Settings in the listing.
	pub count: usize,
	/// Listed settings whose effective value differs from the schema default.
	pub modified: usize,
}

/// How a `cfg://` write ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgWriteOutcome {
	/// Applied.
	Applied,
	/// Declined.
	Declined,
	/// Unchanged.
	Unchanged,
}

impl CfgWriteOutcome {
	/// Label token.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Applied => "applied",
			Self::Declined => "declined",
			Self::Unchanged => "unchanged",
		}
	}
}

/// A `cfg://` write. Values are display-formatted (credentials redacted).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CfgWriteDetails {
	/// Path.
	pub path: String,
	/// Persisted to config.yml rather than scoped to the session.
	pub save: bool,
	/// Previous display value.
	pub previous: String,
	/// New display value.
	pub value: String,
	/// Outcome.
	pub outcome: Option<CfgWriteOutcome>,
	/// Effective value after saving when higher-precedence layer shadows.
	pub effective: Option<String>,
}

/// Minimal parsed `cfg://` target info used by render helpers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CfgUrlTarget {
	/// Dotted segments.
	pub segments: Vec<String>,
	/// Save flag.
	pub save: bool,
}

/// Best-effort parse of `cfg://path[/save]`.
pub fn parse_cfg_url(url: &str) -> Option<CfgUrlTarget> {
	let rest = url.strip_prefix("cfg://")?;
	let mut parts: Vec<String> = rest
		.split('/')
		.filter(|s| !s.is_empty())
		.map(|s| s.to_string())
		.collect();
	let mut save = false;
	if parts.last().map(|s| s.as_str()) == Some("save") {
		save = true;
		parts.pop();
	}
	Some(CfgUrlTarget { segments: parts, save })
}

/// Description of a cfg read view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CfgReadSummary {
	/// Description/path label.
	pub description: String,
	/// Meta labels.
	pub meta: Vec<String>,
	/// Error, if any.
	pub error: Option<String>,
	/// Listing text.
	pub listing: Option<String>,
}

/// Summarize cfg read.
pub fn describe_cfg_read_plain(
	url: &str,
	is_error: bool,
	first_text: &str,
	details: Option<&CfgReadDetails>,
) -> CfgReadSummary {
	let parsed = parse_cfg_url(url);
	let description = details
		.map(|d| d.path.clone())
		.filter(|s| !s.is_empty())
		.or_else(|| parsed.as_ref().map(|p| p.segments.join(".")))
		.filter(|s| !s.is_empty())
		.unwrap_or_else(|| "all settings".to_string());
	let mut meta = Vec::new();
	if let Some(d) = details {
		meta.push(if d.count == 1 { "1 setting".to_string() } else { format!("{} settings", d.count) });
		if d.modified > 0 {
			meta.push(format!("{} modified", d.modified));
		}
	}
	if is_error {
		return CfgReadSummary {
			description,
			meta,
			error: Some(if first_text.is_empty() { "Settings read failed.".to_string() } else { first_text.to_string() }),
			listing: None,
		};
	}
	let listing = {
		let t = first_text.trim_end().to_string();
		if t.is_empty() { None } else { Some(t) }
	};
	CfgReadSummary { description, meta, error: None, listing }
}

/// Description of a cfg write view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CfgWriteSummary {
	/// Path.
	pub path: Option<String>,
	/// Save/persist scope.
	pub save: bool,
	/// Outcome.
	pub outcome: Option<CfgWriteOutcome>,
	/// Previous value.
	pub previous: Option<String>,
	/// Next/value.
	pub value: String,
	/// Effective shadow warning value.
	pub effective: Option<String>,
	/// Error.
	pub error: Option<String>,
}

/// Summarize cfg write.
pub fn describe_cfg_write_plain(
	url: &str,
	content: Option<&str>,
	is_error: bool,
	first_text: &str,
	details: Option<&CfgWriteDetails>,
) -> CfgWriteSummary {
	let target = parse_cfg_url(url);
	let path = details
		.map(|d| d.path.clone())
		.filter(|s| !s.is_empty())
		.or_else(|| target.as_ref().map(|t| t.segments.join(".")));
	let save = details.map(|d| d.save).or_else(|| target.as_ref().map(|t| t.save)).unwrap_or(false);
	if is_error {
		return CfgWriteSummary {
			path,
			save,
			outcome: details.and_then(|d| d.outcome),
			previous: None,
			value: String::new(),
			effective: None,
			error: Some(if first_text.is_empty() { "Settings write failed.".to_string() } else { first_text.to_string() }),
		};
	}
	CfgWriteSummary {
		path,
		save,
		outcome: details.and_then(|d| d.outcome),
		previous: details.map(|d| d.previous.clone()),
		value: details
			.map(|d| d.value.clone())
			.or_else(|| content.map(|c| c.trim().to_string()))
			.unwrap_or_else(|| "…".to_string()),
		effective: details.and_then(|d| d.effective.clone()),
		error: None,
	}
}

/// Native describe for cfg read call/result.
pub fn describe_cfg_read_native(
	url: &str,
	is_error: bool,
	first_text: &str,
	details: Option<&CfgReadDetails>,
) -> NativeToolView {
	let summary = describe_cfg_read_plain(url, is_error, first_text, details);
	if summary.error.is_some() {
		return NativeToolView {
			..error_view("Settings read", summary.error.as_deref().unwrap_or("Settings read failed."), &[])
		};
	}
	let mut head_details = vec![ToolHeadDetail::Text(summary.description)];
	for m in summary.meta {
		head_details.push(ToolHeadDetail::Text(m));
	}
	let mut view = NativeToolView {
		head: Some(NodeText::Spans(tool_head("Settings read", &head_details))),
		..NativeToolView::default()
	};
	if let Some(listing) = summary.listing {
		let mut p = Props::new();
		p.set_str("language", "yaml");
		view.body = Some(vec![code(listing, p)]);
	}
	view
}

/// Native describe for cfg write call/result.
pub fn describe_cfg_write_native(
	url: &str,
	content: Option<&str>,
	is_error: bool,
	first_text: &str,
	details: Option<&CfgWriteDetails>,
) -> NativeToolView {
	let summary = describe_cfg_write_plain(url, content, is_error, first_text, details);
	if summary.error.is_some() {
		return NativeToolView {
			..error_view("Settings write", summary.error.as_deref().unwrap_or("Settings write failed."), &[])
		};
	}
	let mut head_details = Vec::new();
	if let Some(path) = summary.path.clone() {
		head_details.push(ToolHeadDetail::Text(path));
	}
	head_details.push(ToolHeadDetail::Text(if summary.save { "save".to_string() } else { "session".to_string() }));
	if let Some(outcome) = summary.outcome {
		head_details.push(ToolHeadDetail::Text(outcome.as_str().to_string()));
	}
	let mut body_lines = Vec::new();
	if let Some(prev) = summary.previous.as_deref().filter(|s| !s.is_empty()) {
		body_lines.push(format!("previous: {prev}"));
	}
	if !summary.value.is_empty() {
		body_lines.push(format!("value: {}", summary.value));
	}
	if let Some(eff) = summary.effective.as_deref().filter(|s| !s.is_empty()) {
		body_lines.push(format!("effective: {eff}"));
	}
	let mut view = NativeToolView {
		head: Some(NodeText::Spans(tool_head("Settings write", &head_details))),
		..NativeToolView::default()
	};
	if !body_lines.is_empty() {
		let mut p = Props::new();
		p.set_str("language", "yaml");
		view.body = Some(vec![code(body_lines.join("\n"), p)]);
	}
	view
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_cfg_url_basic() {
		let t = parse_cfg_url("cfg://foo/bar").unwrap();
		assert_eq!(t.segments, vec!["foo", "bar"]);
		assert!(!t.save);
		let t2 = parse_cfg_url("cfg://foo/save").unwrap();
		assert_eq!(t2.segments, vec!["foo"]);
		assert!(t2.save);
	}

	#[test]
	fn describe_read_meta_and_error() {
		let d = CfgReadDetails { path: "ui".into(), count: 2, modified: 1 };
		let s = describe_cfg_read_plain("cfg://ui", false, "a: 1\n", Some(&d));
		assert_eq!(s.description, "ui");
		assert!(s.meta.contains(&"2 settings".into()));
		assert!(s.listing.is_some());
		let e = describe_cfg_read_plain("cfg://ui", true, "", None);
		assert!(e.error.is_some());
	}

	#[test]
	fn describe_write_declined() {
		let d = CfgWriteDetails {
			path: "x.y".into(),
			save: true,
			previous: "1".into(),
			value: "2".into(),
			outcome: Some(CfgWriteOutcome::Declined),
			effective: Some("3".into()),
		};
		let s = describe_cfg_write_plain("cfg://x.y/save", None, false, "", Some(&d));
		assert!(s.save);
		assert_eq!(s.outcome, Some(CfgWriteOutcome::Declined));
		assert_eq!(s.effective.as_deref(), Some("3"));
	}

	#[test]
	fn native_describe_helpers() {
		let d = CfgReadDetails { path: "ui".into(), count: 1, modified: 0 };
		let r = describe_cfg_read_native("cfg://ui", false, "x: 1", Some(&d));
		assert!(r.head.is_some());
		let w = describe_cfg_write_native("cfg://x", Some("2"), false, "", None);
		assert!(w.head.is_some());
	}
}
