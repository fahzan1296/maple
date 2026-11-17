//! Recorded merge-conflict regions and scoped reconstruction.
//!
 //! Reference: `l/src/tools/conflict-detect.ts`.
 //!
//! Deviations:
//! - The reference throws for a `base` scope on a 2-way conflict; Rust returns
//!   `Err(String)` from [`render_conflict_region`] instead.
//! - Themed render/describe helpers take `&Theme` and paint markers/labels via
//!   theme tokens; no JS-runtime-only facilities required.
/// Recorded merge-conflict region.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConflictBlock {
	/// 1-indexed line of the `<<<<<<<` marker.
	pub start_line: usize,
	/// 1-indexed line of the `=======` separator.
	pub separator_line: usize,
	/// 1-indexed line of the `>>>>>>>` marker.
	pub end_line: usize,
	/// 1-indexed line of the `|||||||` base marker (diff3 only).
	pub base_line: Option<usize>,
	/// Branch label on the `<<<<<<<` marker.
	pub ours_label: Option<String>,
	/// Branch label on the `|||||||` marker.
	pub base_label: Option<String>,
	/// Branch label on the `>>>>>>>` marker.
	pub theirs_label: Option<String>,
	/// Lines between `<<<<<<<` and the separator.
	pub ours_lines: Vec<String>,
	/// Lines between `|||||||` and `=======`.
	pub base_lines: Option<Vec<String>>,
	/// Lines between `=======` and `>>>>>>>`.
	pub theirs_lines: Vec<String>,
}

/// Recorded merge-conflict identity and paths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConflictEntry {
	/// Conflict id.
	pub id: usize,
	/// Absolute file path.
	pub absolute_path: String,
	/// Display file path.
	pub display_path: String,
	/// Conflict block.
	pub block: ConflictBlock,
}

impl ConflictEntry {
	/// The block's `start_line`.
	pub fn start_line(&self) -> usize {
		self.block.start_line
	}
}

/// A selectable side of a recorded conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictScope {
	/// Ours side.
	Ours,
	/// Theirs side.
	Theirs,
	/// Base side (diff3 only).
	Base,
}

/// Reconstructed lines and the file line they start at.
#[derive(Clone, Debug, PartialEq)]
pub struct ConflictRegion {
	/// Lines to render, ordered top-to-bottom.
	pub lines: Vec<String>,
	/// 1-indexed file line number `lines[0]` corresponds to.
	pub start_line: usize,
}

/// Reconstruct a marker line with its optional branch label.
fn marker_line(prefix: &str, label: Option<&str>) -> String {
	match label {
		Some(label) if !label.is_empty() => format!("{prefix} {label}"),
		_ => prefix.to_string(),
	}
}

/// Materialise a conflict block for `conflict://<N>` reads (and their
/// `/ours` / `/theirs` / `/base` scopes).
///
/// Bare (no scope) returns the full block including marker lines. A scoped
/// view returns only that side's body; `base` errors when the recorded
/// conflict is a 2-way merge with no base section.
pub fn render_conflict_region(
	entry: &ConflictEntry,
	scope: Option<ConflictScope>,
) -> Result<ConflictRegion, String> {
	let block = &entry.block;
	match scope {
		Some(ConflictScope::Ours) => Ok(ConflictRegion {
			lines: block.ours_lines.clone(),
			start_line: block.start_line + 1,
		}),
		Some(ConflictScope::Theirs) => Ok(ConflictRegion {
			lines: block.theirs_lines.clone(),
			start_line: block.separator_line + 1,
		}),
		Some(ConflictScope::Base) => {
			let (base_lines, base_line) = match (&block.base_lines, block.base_line) {
				(Some(lines), Some(line)) => (lines, line),
				_ => {
					return Err(format!(
						"Conflict #{} has no base section (2-way merge). 'conflict://{}/base' is only valid for diff3 conflicts.",
						entry.id, entry.id
					));
				}
			};
			Ok(ConflictRegion {
				lines: base_lines.clone(),
				start_line: base_line + 1,
			})
		}
		None => {
			let mut out = Vec::new();
			out.push(marker_line("<<<<<<<", block.ours_label.as_deref()));
			out.extend(block.ours_lines.iter().cloned());
			if let Some(base_lines) = &block.base_lines {
				out.push(marker_line("|||||||", block.base_label.as_deref()));
				out.extend(base_lines.iter().cloned());
			}
			out.push("=======".to_string());
			out.extend(block.theirs_lines.iter().cloned());
			out.push(marker_line(">>>>>>>", block.theirs_label.as_deref()));
			Ok(ConflictRegion {
				lines: out,
				start_line: block.start_line,
			})
		}
	}
}

/// Render conflict region lines with theme styling (markers/labels painted).
pub fn render_conflict_region_themed(
	entry: &ConflictEntry,
	scope: Option<ConflictScope>,
	theme: &crate::theme::Theme,
) -> Result<Vec<String>, String> {
	let region = render_conflict_region(entry, scope)?;
	let mut out = Vec::with_capacity(region.lines.len());
	for line in &region.lines {
		let styled = if line.starts_with("<<<<<<<") {
			theme.fg("toolDiffRemoved", line).unwrap_or_else(|_| line.clone())
		} else if line.starts_with(">>>>>>>") {
			theme.fg("toolDiffAdded", line).unwrap_or_else(|_| line.clone())
		} else if line.starts_with("|||||||") || line == "=======" {
			theme.fg("toolDiffContext", line).unwrap_or_else(|_| line.clone())
		} else {
			line.clone()
		};
		out.push(styled);
	}
	Ok(out)
}

/// Plain render wrapper (un-themed).
pub fn render_result(entry: &ConflictEntry, scope: Option<ConflictScope>) -> Result<Vec<String>, String> {
	Ok(render_conflict_region(entry, scope)?.lines)
}

/// Native describe call for a conflict resource.
pub fn describe_call(id: usize, scope: Option<ConflictScope>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let mut details = vec![ToolHeadDetail::Span(Span::styled(format!("conflict://{id}"), "accent"))];
	if let Some(s) = scope {
		let name = match s { ConflictScope::Ours => "ours", ConflictScope::Theirs => "theirs", ConflictScope::Base => "base" };
		details.push(ToolHeadDetail::Span(Span::styled(name.to_string(), "muted")));
	}
	NativeToolView { head: Some(NodeText::Spans(tool_head("Conflict", &details))), ..NativeToolView::default() }
}

/// Native describe result for a conflict region.
pub fn describe_result(
	entry: &ConflictEntry,
	scope: Option<ConflictScope>,
	is_error: bool,
) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::code;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, tool_head, NativeToolView, ToolHeadDetail};
	match render_conflict_region(entry, scope) {
		Err(e) => error_view("Conflict", &e, &[]),
		Ok(region) => {
			let head = tool_head("Conflict", &[
				ToolHeadDetail::Span(Span::styled(entry.display_path.clone(), "accent")),
				ToolHeadDetail::Span(Span::styled(format!("#{}", entry.id), "muted")),
			]);
			let mut p = Props::new();
			p.set_str("path", &entry.display_path);
			NativeToolView {
				head: Some(NodeText::Spans(head)),
				body: Some(vec![code(region.lines.join("\n"), p)]),
				tone: Some(if is_error { Tone::Error } else { Tone::Warning }),
				..NativeToolView::default()
			}
		}
	}
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn render_and_describe() {
		let mut e = ConflictEntry::default();
		e.id = 1; e.display_path = "f.txt".into();
		e.start_line = 1; e.separator_line = 3; e.end_line = 5;
		e.ours_lines = vec!["a".into()];
		e.theirs_lines = vec!["b".into()];
		let lines = render_result(&e, None).unwrap();
		assert!(lines[0].starts_with("<<<<<<<"));
		let c = describe_call(1, None);
		assert!(c.head.is_some());
		let r = describe_result(&e, None, false);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	fn entry() -> ConflictEntry {
		ConflictEntry {
			id: 3,
			absolute_path: "/repo/file.txt".into(),
			display_path: "file.txt".into(),
			block: ConflictBlock {
				start_line: 10,
				separator_line: 14,
				end_line: 18,
				base_line: Some(12),
				ours_label: Some("feature".into()),
				base_label: Some("main".into()),
				theirs_label: None,
				ours_lines: vec!["ours-a".into(), "ours-b".into()],
				base_lines: Some(vec!["base-a".into()]),
				theirs_lines: vec!["theirs-a".into()],
			},
		}
	}

	#[test]
	fn bare_region_keeps_every_marker() {
		let region = render_conflict_region(&entry(), None).unwrap();
		assert_eq!(
			region.lines,
			vec![
				"<<<<<<< feature",
				"ours-a",
				"ours-b",
				"||||||| main",
				"base-a",
				"=======",
				"theirs-a",
				">>>>>>>",
			]
		);
		assert_eq!(region.start_line, 10);
	}

	#[test]
	fn ours_and_theirs_scopes_return_their_bodies() {
		let ours = render_conflict_region(&entry(), Some(ConflictScope::Ours)).unwrap();
		assert_eq!(ours.lines, vec!["ours-a", "ours-b"]);
		assert_eq!(ours.start_line, 11);

		let theirs = render_conflict_region(&entry(), Some(ConflictScope::Theirs)).unwrap();
		assert_eq!(theirs.lines, vec!["theirs-a"]);
		assert_eq!(theirs.start_line, 15);
	}

	#[test]
	fn base_scope_returns_the_diff3_base() {
		let base = render_conflict_region(&entry(), Some(ConflictScope::Base)).unwrap();
		assert_eq!(base.lines, vec!["base-a"]);
		assert_eq!(base.start_line, 13);
	}

	#[test]
	fn base_scope_errors_for_two_way_merge() {
		let mut two_way = entry();
		two_way.block.base_lines = None;
		two_way.block.base_line = None;
		let error = render_conflict_region(&two_way, Some(ConflictScope::Base)).unwrap_err();
		assert!(error.contains("no base section"));
		assert!(error.contains("diff3"));
	}

	#[test]
	fn marker_labels_are_optional() {
		let mut bare = entry();
		bare.block.ours_label = None;
		bare.block.base_lines = None;
		bare.block.base_line = None;
		let region = render_conflict_region(&bare, None).unwrap();
		assert_eq!(region.lines[0], "<<<<<<<");
		assert_eq!(region.lines[region.lines.len() - 1], ">>>>>>>");
	}
}
