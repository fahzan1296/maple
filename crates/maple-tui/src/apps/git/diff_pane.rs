//! Diff pane of the git TUI.
//!
//! Reference: `l/src/apps/git/diff-pane.ts`.
//!
//! Deviations:
//! - TS depends on `@oh-my-pi/pi-natives` diff/highlight, TUI scroll/image,
//!   theme, and complex ANSI/minimap rendering. No new deps and no native
//!   diff engine in this crate.
//! - This port preserves core public data contracts and pure-ish logic:
//!   [`DiffDocument`], [`HunkBlock`], [`DiffBuildOptions`], [`WhitespaceMode`],
//!   [`ViewMode`], [`build_diff_document`], [`build_line_selection_patch`],
//!   [`DiffPaneModel`] (scroll/cursor/selection/hunk navigation state).
//! - `build_diff_document` performs a simple line-based diff (LCS over lines)
//!   to produce aligned [`DiffRow`]s and tight hunks with context; intraline
//!   word marks, syntax highlighting, minimap rendering, and streaming are
//!   omitted/documented as host/native responsibilities. Hunk patches are
//!   generated in unified form when `can_patch` is true.
//! - Image/asset rendering and mouse hit-testing are omitted; click/asset APIs
//!   represented as data hooks.

use std::cmp::max;

/// Column ranges (inclusive start, exclusive end) carrying intraline emphasis.
pub type MarkRanges = Vec<(usize, usize)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
	Context,
	Change,
	Add,
	Del,
}

/// One aligned diff row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
	pub kind: RowKind,
	pub old_num: Option<usize>,
	pub new_num: Option<usize>,
	pub old_text: String,
	pub new_text: String,
	pub old_width: usize,
	pub new_width: usize,
	pub old_marks: Option<MarkRanges>,
	pub new_marks: Option<MarkRanges>,
	pub old_raw: Option<String>,
	pub new_raw: Option<String>,
}

/// One changed region with its applyable patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkBlock {
	/// `@@ -a,b +c,d @@` header shown above the block.
	pub header: String,
	/// Standalone patch text for this hunk, or empty when not applyable.
	pub patch: String,
	pub rows: Vec<DiffRow>,
}

/// Aligned diff document produced by [`build_diff_document`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffDocument {
	pub file_path: String,
	pub rows: Vec<DiffRow>,
	pub hunks: Vec<HunkBlock>,
	pub file_lines: Vec<FileLine>,
	pub old_display_lines: Vec<String>,
	pub new_display_lines: Vec<String>,
	pub additions: usize,
	pub deletions: usize,
	pub gutter_width: usize,
	pub max_line_width: usize,
	/// False when built with whitespace-ignore: hunk patches would not apply.
	pub can_patch: bool,
	pub raw_old: String,
	pub raw_new: String,
	pub old_ends_newline: bool,
	pub new_ends_newline: bool,
	pub row_index_by_new_line: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileLine {
	pub text: String,
	pub width: usize,
}

/// How the pane presents the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
	Split,
	Inline,
	Hunk,
	File,
}

/// Hunk-button actions raised to the root component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkAction {
	Stage,
	Unstage,
	Discard,
}

/// Whitespace handling for [`build_diff_document`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WhitespaceMode {
	#[default]
	Off,
	Whitespace,
	Formatting,
}

/// Options for [`build_diff_document`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffBuildOptions {
	pub whitespace: WhitespaceMode,
}

fn display_width(s: &str) -> usize {
	s.chars().count()
}

fn file_patch_header(file_path: &str) -> String {
	format!("diff --git a/{file_path} b/{file_path}\n--- a/{file_path}\n+++ b/{file_path}\n")
}

fn split_lines(raw: &str) -> Vec<String> {
	if raw.is_empty() {
		Vec::new()
	} else {
		raw.trim_end_matches('\n').split('\n').map(|s| s.to_string()).collect()
	}
}

/// Simple LCS edit script over line slices (equal by string/basis).
fn lcs_diff<'a>(old: &'a [String], new: &'a [String]) -> Vec<DiffOp> {
	let n = old.len();
	let m = new.len();
	let mut dp = vec![vec![0u32; m + 1]; n + 1];
	for i in (0..n).rev() {
		for j in (0..m).rev() {
			dp[i][j] = if old[i] == new[j] {
				dp[i + 1][j + 1] + 1
			} else {
				dp[i + 1][j].max(dp[i][j + 1])
			};
		}
	}
	let mut ops = Vec::new();
	let (mut i, mut j) = (0usize, 0usize);
	while i < n && j < m {
		if old[i] == new[j] {
			ops.push(DiffOp::Equal(i, j));
			i += 1;
			j += 1;
		} else if dp[i + 1][j] >= dp[i][j + 1] {
			ops.push(DiffOp::Del(i));
			i += 1;
		} else {
			ops.push(DiffOp::Add(j));
			j += 1;
		}
	}
	while i < n {
		ops.push(DiffOp::Del(i));
		i += 1;
	}
	while j < m {
		ops.push(DiffOp::Add(j));
		j += 1;
	}
	ops
}

#[derive(Clone, Copy, Debug)]
enum DiffOp {
	Equal(usize, usize),
	Del(usize),
	Add(usize),
}

/// Build the aligned document for one file from its raw old/new texts.
pub fn build_diff_document(
	old_raw: &str,
	new_raw: &str,
	file_path: &str,
	options: DiffBuildOptions,
) -> DiffDocument {
	let ignore_ws = matches!(options.whitespace, WhitespaceMode::Whitespace);
	let old_lines = split_lines(old_raw);
	let new_lines = split_lines(new_raw);

	let old_basis: Vec<String> = if ignore_ws {
		old_lines.iter().map(|l| l.trim().to_string()).collect()
	} else {
		old_lines.clone()
	};
	let new_basis: Vec<String> = if ignore_ws {
		new_lines.iter().map(|l| l.trim().to_string()).collect()
	} else {
		new_lines.clone()
	};

	let ops = lcs_diff(&old_basis, &new_basis);

	let mut rows: Vec<DiffRow> = Vec::new();
	let mut additions = 0usize;
	let mut deletions = 0usize;
	let mut max_line_width = 0usize;

	// pair pending dels/adds into change blocks
	let mut pending_del: Vec<usize> = Vec::new();
	let mut pending_add: Vec<usize> = Vec::new();

	let flush = |rows: &mut Vec<DiffRow>, pd: &mut Vec<usize>, pa: &mut Vec<usize>, additions: &mut usize, deletions: &mut usize| {
		if pd.is_empty() && pa.is_empty() { return; }
		let paired = pd.len().min(pa.len());
		*deletions += pd.len();
		*additions += pa.len();
		for k in 0..paired {
			let oi = pd[k];
			let ni = pa[k];
			rows.push(DiffRow {
				kind: RowKind::Change,
				old_num: Some(oi + 1),
				new_num: Some(ni + 1),
				old_text: old_lines[oi].clone(),
				new_text: new_lines[ni].clone(),
				old_width: display_width(&old_lines[oi]),
				new_width: display_width(&new_lines[ni]),
				old_marks: None,
				new_marks: None,
				old_raw: Some(old_lines[oi].clone()),
				new_raw: Some(new_lines[ni].clone()),
			});
		}
		for oi in pd.iter().skip(paired).copied() {
			rows.push(DiffRow {
				kind: RowKind::Del,
				old_num: Some(oi + 1),
				new_num: None,
				old_text: old_lines[oi].clone(),
				new_text: String::new(),
				old_width: display_width(&old_lines[oi]),
				new_width: 0,
				old_marks: None,
				new_marks: None,
				old_raw: Some(old_lines[oi].clone()),
				new_raw: None,
			});
		}
		for ni in pa.iter().skip(paired).copied() {
			rows.push(DiffRow {
				kind: RowKind::Add,
				old_num: None,
				new_num: Some(ni + 1),
				old_text: String::new(),
				new_text: new_lines[ni].clone(),
				old_width: 0,
				new_width: display_width(&new_lines[ni]),
				old_marks: None,
				new_marks: None,
				old_raw: None,
				new_raw: Some(new_lines[ni].clone()),
			});
		}
		pd.clear();
		pa.clear();
	};

	for op in ops {
		match op {
			DiffOp::Equal(oi, ni) => {
				flush(&mut rows, &mut pending_del, &mut pending_add, &mut additions, &mut deletions);
				max_line_width = max(max_line_width, display_width(&old_lines[oi]));
				max_line_width = max(max_line_width, display_width(&new_lines[ni]));
				rows.push(DiffRow {
					kind: RowKind::Context,
					old_num: Some(oi + 1),
					new_num: Some(ni + 1),
					old_text: old_lines[oi].clone(),
					new_text: new_lines[ni].clone(),
					old_width: display_width(&old_lines[oi]),
					new_width: display_width(&new_lines[ni]),
					old_marks: None,
					new_marks: None,
					old_raw: Some(old_lines[oi].clone()),
					new_raw: Some(new_lines[ni].clone()),
				});
			}
			DiffOp::Del(oi) => {
				max_line_width = max(max_line_width, display_width(&old_lines[oi]));
				pending_del.push(oi);
			}
			DiffOp::Add(ni) => {
				max_line_width = max(max_line_width, display_width(&new_lines[ni]));
				pending_add.push(ni);
			}
		}
	}
	flush(&mut rows, &mut pending_del, &mut pending_add, &mut additions, &mut deletions);

	let can_patch = !ignore_ws;
	let hunks = build_hunks(file_path, &rows, 3, can_patch);

	let file_lines = new_lines
		.iter()
		.map(|l| FileLine { width: display_width(l), text: l.clone() })
		.collect();
	let gutter_width = max(3, format!("{}", old_lines.len().max(new_lines.len())).len());
	let mut row_index_by_new_line = vec![-1i32; new_lines.len() + 1];
	for (idx, row) in rows.iter().enumerate() {
		if let Some(n) = row.new_num {
			if n < row_index_by_new_line.len() && row_index_by_new_line[n] == -1 {
				row_index_by_new_line[n] = idx as i32;
			}
		}
	}

	DiffDocument {
		file_path: file_path.to_string(),
		rows,
		hunks,
		file_lines,
		old_display_lines: old_lines,
		new_display_lines: new_lines,
		additions,
		deletions,
		gutter_width,
		max_line_width,
		can_patch,
		raw_old: old_raw.to_string(),
		raw_new: new_raw.to_string(),
		old_ends_newline: old_raw.ends_with('\n'),
		new_ends_newline: new_raw.ends_with('\n'),
		row_index_by_new_line,
	}
}

fn build_hunks(file_path: &str, rows: &[DiffRow], context: usize, can_patch: bool) -> Vec<HunkBlock> {
	let mut hunks = Vec::new();
	let mut i = 0usize;
	while i < rows.len() {
		if rows[i].kind == RowKind::Context {
			i += 1;
			continue;
		}
		// find range of change including nearby context merges? simple: group contiguous non-context with context gaps < 2*context
		let mut start = i;
		let mut end = i + 1;
		while end < rows.len() {
			// look ahead for next change within 2*context context rows
			let mut j = end;
			let mut ctx = 0usize;
			while j < rows.len() && rows[j].kind == RowKind::Context && ctx < context * 2 {
				ctx += 1;
				j += 1;
			}
			if j < rows.len() && rows[j].kind != RowKind::Context {
				end = j + 1;
				continue;
			}
			break;
		}
		start = start.saturating_sub(context);
		let mut ctx_after = 0usize;
		let mut real_end = end;
		while real_end < rows.len() && rows[real_end].kind == RowKind::Context && ctx_after < context {
			real_end += 1;
			ctx_after += 1;
		}
		end = real_end;

		let h_rows: Vec<DiffRow> = rows[start..end].to_vec();
		let old_start = h_rows.iter().find_map(|r| r.old_num).unwrap_or(1);
		let new_start = h_rows.iter().find_map(|r| r.new_num).unwrap_or(1);
		let old_lines_count = h_rows.iter().filter(|r| r.old_num.is_some()).count();
		let new_lines_count = h_rows.iter().filter(|r| r.new_num.is_some()).count();
		let header = format!("@@ -{old_start},{old_lines_count} +{new_start},{new_lines_count} @@");
		let patch = if can_patch {
			let mut p = file_patch_header(file_path);
			p.push_str(&header);
			p.push('\n');
			for r in &h_rows {
				match r.kind {
					RowKind::Context => {
						p.push(' ');
						p.push_str(r.old_raw.as_deref().unwrap_or(&r.old_text));
					}
					RowKind::Del => {
						p.push('-');
						p.push_str(r.old_raw.as_deref().unwrap_or(&r.old_text));
					}
					RowKind::Add => {
						p.push('+');
						p.push_str(r.new_raw.as_deref().unwrap_or(&r.new_text));
					}
					RowKind::Change => {
						p.push('-');
						p.push_str(r.old_raw.as_deref().unwrap_or(&r.old_text));
						p.push('\n');
						p.push('+');
						p.push_str(r.new_raw.as_deref().unwrap_or(&r.new_text));
					}
				}
				p.push('\n');
			}
			p
		} else {
			String::new()
		};
		hunks.push(HunkBlock { header, patch, rows: h_rows });
		i = end;
	}
	hunks
}

/// Build a patch covering only changed rows inside `[from, to]` (indices into
/// `doc.rows`). `apply`: base old, adopt selected changes; `revert`: base new,
/// undo selected changes.
pub fn build_line_selection_patch(
	doc: &DiffDocument,
	from: usize,
	to: usize,
	intent_apply: bool,
) -> Option<String> {
	if !doc.can_patch { return None; }
	let mut target: Vec<String> = Vec::new();
	let mut touched = false;
	let end_idx = to.min(doc.rows.len().saturating_sub(1));
	for (i, row) in doc.rows.iter().enumerate() {
		let selected = i >= from && i <= end_idx;
		if selected && row.kind != RowKind::Context { touched = true; }
		let use_new = if intent_apply { selected } else { !selected };
		match row.kind {
			RowKind::Context => {
				let line = if intent_apply { &row.old_raw } else { &row.new_raw };
				if let Some(l) = line { target.push(l.clone()); }
			}
			RowKind::Change => {
				target.push(if use_new { row.new_raw.clone()? } else { row.old_raw.clone()? });
			}
			RowKind::Del => {
				if !use_new { target.push(row.old_raw.clone()?); }
			}
			RowKind::Add => {
				if use_new { target.push(row.new_raw.clone()?); }
			}
		}
	}
	if !touched { return None; }
	let (base, base_ends, other_ends) = if intent_apply {
		(&doc.raw_old, doc.old_ends_newline, doc.new_ends_newline)
	} else {
		(&doc.raw_new, doc.new_ends_newline, doc.old_ends_newline)
	};
	let last_selected = to >= doc.rows.len().saturating_sub(1)
		&& doc.rows.last().map(|r| r.kind != RowKind::Context).unwrap_or(false);
	let ends_nl = if last_selected { other_ends } else { base_ends };
	let mut target_text = target.join("\n");
	if ends_nl && !target.is_empty() { target_text.push('\n'); }
	let rebuilt = build_diff_document(base, &target_text, &doc.file_path, DiffBuildOptions::default());
	if rebuilt.hunks.is_empty() { return None; }
	let mut out = file_patch_header(&doc.file_path);
	for h in &rebuilt.hunks {
		out.push_str(&h.header);
		out.push('\n');
		for r in &h.rows {
			match r.kind {
				RowKind::Context => { out.push(' '); out.push_str(r.old_raw.as_deref().unwrap_or(&r.old_text)); }
				RowKind::Del => { out.push('-'); out.push_str(r.old_raw.as_deref().unwrap_or(&r.old_text)); }
				RowKind::Add => { out.push('+'); out.push_str(r.new_raw.as_deref().unwrap_or(&r.new_text)); }
				RowKind::Change => {
					out.push('-'); out.push_str(r.old_raw.as_deref().unwrap_or(&r.old_text)); out.push('\n');
					out.push('+'); out.push_str(r.new_raw.as_deref().unwrap_or(&r.new_text));
				}
			}
			out.push('\n');
		}
	}
	Some(out)
}

/// Scrollable diff viewport state model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffPaneModel {
	pub mode: ViewMode,
	pub wrap: bool,
	pub patch_target_stage: bool, // true stage, false unstage; None modeled separately
	pub patch_target: Option<PatchTarget>,
	pub selected_hunk: usize,
	pub scroll_top: usize,
	pub scroll_left: usize,
	pub focused: bool,
	pub cursor: usize,
	pub anchor: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchTarget {
	Stage,
	Unstage,
}

impl DiffPaneModel {
	pub fn new() -> Self {
		Self {
			mode: ViewMode::Split,
			wrap: false,
			patch_target_stage: false,
			patch_target: None,
			selected_hunk: 0,
			scroll_top: 0,
			scroll_left: 0,
			focused: false,
			cursor: 0,
			anchor: None,
		}
	}
	pub fn set_mode(&mut self, mode: ViewMode) {
		if self.mode == mode { return; }
		self.mode = mode;
		self.scroll_top = 0;
		self.scroll_left = 0;
		self.cursor = 0;
		self.anchor = None;
		if mode == ViewMode::Hunk { self.selected_hunk = 0; }
	}
	pub fn cycle_mode(&mut self) {
		let order = [ViewMode::Split, ViewMode::Inline, ViewMode::Hunk, ViewMode::File];
		let idx = order.iter().position(|m| *m == self.mode).unwrap_or(0);
		self.set_mode(order[(idx + 1) % order.len()]);
	}
	pub fn toggle_wrap(&mut self) { self.wrap = !self.wrap; self.scroll_left = 0; }
	pub fn clear_selection(&mut self) -> bool {
		if self.anchor.is_none() { false } else { self.anchor = None; true }
	}
	pub fn selection(&self) -> Option<(usize, usize, bool)> {
		match self.anchor {
			None => Some((self.cursor, self.cursor, false)),
			Some(a) => Some((a.min(self.cursor), a.max(self.cursor), true)),
		}
	}
}

impl Default for DiffPaneModel {
	fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn build_document_simple_change() {
		let doc = build_diff_document("a\nb\n", "a\nc\n", "f.txt", DiffBuildOptions::default());
		assert_eq!(doc.file_path, "f.txt");
		assert_eq!(doc.additions, 1);
		assert_eq!(doc.deletions, 1);
		assert!(doc.rows.iter().any(|r| r.kind == RowKind::Context));
		assert!(doc.rows.iter().any(|r| r.kind == RowKind::Change || r.kind == RowKind::Add || r.kind == RowKind::Del));
		assert!(doc.can_patch);
	}

	#[test]
	fn whitespace_mode_disables_patch() {
		let doc = build_diff_document("a\n", " a\n", "f", DiffBuildOptions { whitespace: WhitespaceMode::Whitespace });
		assert!(!doc.can_patch);
		assert!(doc.hunks.is_empty() || doc.hunks.iter().all(|h| h.patch.is_empty()));
	}

	#[test]
	fn build_line_selection_patch_apply_selected() {
		let doc = build_diff_document("a\nb\nc\n", "a\nx\nc\n", "f.txt", DiffBuildOptions::default());
		let idx = doc.rows.iter().position(|r| r.kind != RowKind::Context).unwrap();
		let patch = build_line_selection_patch(&doc, idx, idx, true);
		assert!(patch.is_some());
		let p = patch.unwrap();
		assert!(p.contains("diff --git"));
		assert!(p.contains("@@"));
	}

	#[test]
	fn selection_and_mode_cycle() {
		let mut m = DiffPaneModel::new();
		assert_eq!(m.mode, ViewMode::Split);
		m.cursor = 3;
		m.anchor = Some(1);
		assert_eq!(m.selection(), Some((1, 3, true)));
		assert!(m.clear_selection());
		assert_eq!(m.selection(), Some((3, 3, false)));
		m.cycle_mode();
		assert_eq!(m.mode, ViewMode::Inline);
		m.set_mode(ViewMode::Hunk);
		assert_eq!(m.selected_hunk, 0);
	}
}
