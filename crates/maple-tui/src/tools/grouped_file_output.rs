//! Grouped file output data structures and pure formatting helpers.
//!
//! Reference: `l/src/tools/grouped-file-output.ts`.
//!
//! Deviations:
//! - TS builds prefix-folded path trees via `@oh-my-pi/pi-utils`; this port
//!   reproduces `buildPathTree`/`walkPathTree` (documented single-child chain
//!   folding, first-seen order) locally.
//! - TS renders syntax-highlighted/file-hyperlinked rich frames; those remain
//!   host-side.
//! - Rust ports section/output types, classifiers, flat/simple-grouped
//!   assembly, and themed string helpers (`*_themed` take `&Theme`); native
//!   describe for grouped results is host-side (token-styled nodes elsewhere).

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::tools::hashline_format::HASHLINE_TAG_SUFFIX_RE;

/// A file leaf of a [`PathTreeNode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathTreeFile {
	/// Basename of the file within its directory.
	pub name: String,
	/// Opaque key carried onto walk events for section lookup.
	pub key: String,
}

/// One directory child of a [`PathTreeNode`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathTreeSubdir {
	/// Directory basename.
	pub name: String,
	/// Child node.
	pub node: PathTreeNode,
}

/// Directory tree node: direct file leaves plus ordered child directories.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathTreeNode {
	/// Direct file leaves in first-seen order.
	pub files: Vec<PathTreeFile>,
	/// Dedup set for `files`.
	pub file_names: HashSet<String>,
	/// Child directories in first-seen order.
	pub subdirs: Vec<PathTreeSubdir>,
	/// Dedup set for `subdirs`.
	pub dir_index: HashSet<String>,
}

/// One entry handed to [`build_path_tree`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathTreeInput {
	/// Path string; absolute, cwd-relative, or url-like. Backslashes are normalized.
	pub path: String,
	/// Whether the leaf itself is a directory.
	pub is_dir: bool,
	/// Opaque key carried onto file events; defaults to `path`.
	pub key: Option<String>,
}

/// One event emitted by [`walk_path_tree`]: a folded directory or a file leaf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupedTreeEvent {
	/// Folded directory chain (`a/b/c`, no trailing slash).
	Dir {
		/// 0-based nesting depth.
		depth: usize,
		/// Folded chain, `/`-joined.
		name: String,
	},
	/// File leaf.
	File {
		/// 0-based nesting depth.
		depth: usize,
		/// Basename.
		name: String,
		/// Opaque key from [`PathTreeInput::key`].
		key: String,
	},
}

/// Whether `file_path` is a `scheme://…` URL-like entry or a Windows drive path.
pub fn is_url_like_path(file_path: &str) -> bool {
	if file_path.contains("://") {
		return true;
	}
	let bytes = file_path.as_bytes();
	bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\') && bytes[0].is_ascii_alphabetic()
}

fn create_path_tree_node() -> PathTreeNode {
	PathTreeNode::default()
}

fn add_path_tree_file(node: &mut PathTreeNode, name: String, key: String) {
	if node.file_names.contains(&name) {
		return;
	}
	node.file_names.insert(name.clone());
	node.files.push(PathTreeFile { name, key });
}

/// Build a directory tree from a flat list of paths. URL-like entries stay whole
/// as root-level file leaves; absolute paths carry a leading empty segment so
/// they share a common `/` root and fold like any other prefix.
pub fn build_path_tree(entries: &[PathTreeInput]) -> PathTreeNode {
	let mut root = create_path_tree_node();
	for entry in entries {
		let normalized = entry.path.replace('\\', "/");
		let file_key = entry.key.clone().unwrap_or_else(|| entry.path.clone());
		if is_url_like_path(&normalized) {
			add_path_tree_file(&mut root, normalized, file_key);
			continue;
		}
		let trimmed = normalized.strip_suffix('/').unwrap_or(&normalized);
		if trimmed.is_empty() {
			continue;
		}
		let segments: Vec<&str> = trimmed.split('/').collect();
		let dir_count = if entry.is_dir { segments.len() } else { segments.len() - 1 };
		let mut node = &mut root;
		for segment in segments.iter().take(dir_count) {
			if node.dir_index.insert(segment.to_string()) {
				node.subdirs.push(PathTreeSubdir {
					name: (*segment).to_string(),
					node: create_path_tree_node(),
				});
			}
			let position = node.subdirs.iter().position(|sub| sub.name == *segment).expect("pushed above");
			node = &mut node.subdirs[position].node;
		}
		if !entry.is_dir {
			let name = segments[segments.len() - 1].to_string();
			add_path_tree_file(node, name, file_key);
		}
	}
	root
}

/// Depth-first walk of a path tree. Single-child directory chains collapse into
/// one folded header; a node's direct files are emitted before its subdirectories.
pub fn walk_path_tree(node: &PathTreeNode) -> Vec<GroupedTreeEvent> {
	let mut events = Vec::new();
	collect_path_tree_events(node, 0, &mut events);
	events
}

fn collect_path_tree_events(node: &PathTreeNode, depth: usize, events: &mut Vec<GroupedTreeEvent>) {
	for file in &node.files {
		events.push(GroupedTreeEvent::File {
			depth,
			name: file.name.clone(),
			key: file.key.clone(),
		});
	}
	for subdir in &node.subdirs {
		let mut dir_node = subdir;
		let mut parts = vec![dir_node.name.clone()];
		while dir_node.node.files.is_empty() && dir_node.node.subdirs.len() == 1 {
			dir_node = &dir_node.node.subdirs[0];
			parts.push(dir_node.name.clone());
		}
		events.push(GroupedTreeEvent::Dir {
			depth,
			name: parts.join("/"),
		});
		collect_path_tree_events(&dir_node.node, depth + 1, events);
	}
}

fn is_absolute_path(value: &str) -> bool {
	if value.starts_with('/') {
		return true;
	}
	let bytes = value.as_bytes();
	bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' && bytes[0].is_ascii_alphabetic()
}

fn resolve_path_segments(value: &str) -> String {
	use std::path::{Component, PathBuf};
	let mut out = PathBuf::new();
	for component in std::path::Path::new(value).components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				out.pop();
			}
			other => out.push(other.as_os_str()),
		}
	}
	out.to_string_lossy().replace('\\', "/")
}

fn resolve_grouped_path(parent: Option<&str>, name: &str) -> Option<String> {
	let parent = parent?;
	if name.is_empty() || name == "." {
		return Some(parent.to_string());
	}
	if is_absolute_path(name) {
		return Some(resolve_path_segments(name));
	}
	Some(resolve_path_segments(&format!("{parent}/{name}")))
}

static GROUPED_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#+)\s+(.*)$").expect("valid header regex"));
static HEADER_SUFFIX_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\s+\([^)]*\)\s*$").expect("valid suffix regex"));

/// Classify grouped output lines, reconstructing owning file paths and display
/// paths from the nested header stack. `file_scope` defaults to `header_base`.
pub fn classify_grouped_lines<S: AsRef<str>>(
	lines: &[S],
	header_base: Option<&str>,
) -> Vec<GroupedLineContext> {
	classify_grouped_lines_with_scope(lines, header_base, header_base)
}

/// Classify grouped output lines with an explicit owning-file scope (used by
/// single-file searches that print no `#` headers).
pub fn classify_grouped_lines_with_scope<S: AsRef<str>>(
	lines: &[S],
	header_base: Option<&str>,
	file_scope: Option<&str>,
) -> Vec<GroupedLineContext> {
	let base = header_base.filter(|value| !is_url_like_path(value));
	let scope_is_url = file_scope.is_some_and(|value| is_url_like_path(value));
	let mut current_file = if scope_is_url { None } else { file_scope.map(str::to_string) };
	let mut current_url = if scope_is_url { file_scope.map(str::to_string) } else { None };
	let mut dir_at_depth: Vec<Option<String>> = Vec::new();
	let mut display_dir_at_depth: Vec<String> = Vec::new();
	let mut result = Vec::with_capacity(lines.len());

	for line in lines {
		let line = line.as_ref();
		let Some(captures) = GROUPED_HEADER_RE.captures(line) else {
			result.push(GroupedLineContext {
				kind: Some(GroupedLineKind::Content),
				depth: 0,
				file_path: current_file.clone(),
				url: current_url.clone(),
				..GroupedLineContext::default()
			});
			continue;
		};
		let hashes = captures.get(1).expect("group 1").as_str().len();
		let depth = hashes;
		let rest = captures.get(2).expect("group 2").as_str().trim_end();
		let parent = if depth > 1 { dir_at_depth.get(depth - 2).cloned().flatten() } else { base.map(str::to_string) };
		let url = is_url_like_path(rest);
		dir_at_depth.resize(depth - 1, None);
		display_dir_at_depth.resize(depth - 1, String::new());
		current_file = None;
		current_url = None;
		if !url && rest.ends_with('/') {
			let name = &rest[..rest.len() - 1];
			dir_at_depth.push(resolve_grouped_path(parent.as_deref(), name));
			display_dir_at_depth.push(format!("{name}/"));
			result.push(GroupedLineContext {
				kind: Some(GroupedLineKind::Dir),
				depth,
				..GroupedLineContext::default()
			});
			continue;
		}
		let suffix = HEADER_SUFFIX_RE.find(rest);
		let target_end = suffix.map(|matched| matched.start()).unwrap_or(rest.len());
		let target = &rest[..target_end];
		let note = suffix.map(|matched| matched.as_str().trim().to_string());
		let name = HASHLINE_TAG_SUFFIX_RE.replacen(target, "", 1).into_owned();
		if url {
			current_url = Some(target.to_string());
			result.push(GroupedLineContext {
				kind: Some(GroupedLineKind::File),
				depth,
				url: current_url.clone(),
				display_path: Some(name),
				suffix: note,
				..GroupedLineContext::default()
			});
			continue;
		}
		current_file = if name.is_empty() { None } else { resolve_grouped_path(parent.as_deref(), &name) };
		let display_path = format!("{}{}", display_dir_at_depth.concat(), name);
		result.push(GroupedLineContext {
			kind: Some(GroupedLineKind::File),
			depth,
			header_path: current_file.clone(),
			display_path: Some(display_path),
			suffix: note,
			..GroupedLineContext::default()
		});
	}
	result
}

/// Split line indices into blank-line-separated groups. When any blank line is
/// present, runs of blanks break groups; otherwise one group of non-empty lines.
pub fn group_line_indices_by_blank<S: AsRef<str>>(raw_lines: &[S]) -> Vec<Vec<usize>> {
	let has_separators = raw_lines.iter().any(|line| line.as_ref().trim().is_empty());
	let mut groups: Vec<Vec<usize>> = Vec::new();
	if has_separators {
		let mut current = Vec::new();
		for (index, line) in raw_lines.iter().enumerate() {
			if line.as_ref().trim().is_empty() {
				if !current.is_empty() {
					groups.push(std::mem::take(&mut current));
				}
				continue;
			}
			current.push(index);
		}
		if !current.is_empty() {
			groups.push(current);
		}
		return groups;
	}
	let current: Vec<usize> = raw_lines
		.iter()
		.enumerate()
		.filter(|(_, line)| !line.as_ref().trim().is_empty())
		.map(|(index, _)| index)
		.collect();
	if !current.is_empty() {
		groups.push(current);
	}
	groups
}

/// Render a list of files as a multi-level, prefix-folded directory tree shared
/// by grep, ast-grep, ast-edit, and the LSP diagnostic formatter. A blank line
/// precedes every top-level file and every directory header.
pub fn format_grouped_files(files: &[String], render: impl Fn(&str) -> GroupedFileSection) -> GroupedFilesOutput {
	let mut sections: HashMap<String, GroupedFileSection> = HashMap::new();
	let mut inputs: Vec<PathTreeInput> = Vec::new();
	for file_path in files {
		if sections.contains_key(file_path) {
			continue;
		}
		let section = render(file_path);
		if section.skip {
			continue;
		}
		inputs.push(PathTreeInput {
			path: file_path.clone(),
			is_dir: false,
			key: Some(file_path.clone()),
		});
		sections.insert(file_path.clone(), section);
	}
	let tree = build_path_tree(&inputs);
	let mut model = Vec::new();
	let mut display = Vec::new();
	let mut model_ends = HashMap::new();
	let mut emitted = false;
	for event in walk_path_tree(&tree) {
		let (depth, kind_dir, name, key) = match event {
			GroupedTreeEvent::Dir { depth, name } => (depth, true, name, String::new()),
			GroupedTreeEvent::File { depth, name, key } => (depth, false, name, key),
		};
		let hashes = "#".repeat(depth + 1);
		if emitted && (depth == 0 || kind_dir) {
			model.push(String::new());
			display.push(String::new());
		}
		emitted = true;
		if kind_dir {
			let header = format!("{hashes} {name}/");
			model.push(header.clone());
			display.push(header);
			continue;
		}
		let section = sections.get(&key).cloned().unwrap_or_default();
		let header = format!("{hashes} {name}{}", section.header_suffix.as_deref().unwrap_or(""));
		model.push(header.clone());
		display.push(header);
		model.extend(section.model_lines.iter().cloned());
		match section.display_lines.as_ref() {
			Some(lines) => display.extend(lines.iter().cloned()),
			None => display.extend(section.model_lines.iter().cloned()),
		}
		model_ends.insert(key, model.len());
	}
	GroupedFilesOutput {
		model,
		display,
		model_ends,
	}
}

/// Render per-file search matches: grouped mode nests via [`format_grouped_files`];
/// flat mode blank-separates files and prefixes hashline headers when tagged.
pub fn format_file_matches(
	files: &[String],
	grouped: bool,
	render: impl Fn(&str) -> FileMatchSection,
	format_hashline_header: impl Fn(&str, &str) -> String,
) -> GroupedFilesOutput {
	if grouped {
		let rendered: HashMap<String, GroupedFileSection> = files
			.iter()
			.map(|file_path| {
				let section = render(file_path);
				(
					file_path.clone(),
					GroupedFileSection {
						model_lines: section.model,
						display_lines: Some(section.display),
						header_suffix: section.tag.map(|tag| format!("#{tag}")),
						skip: section.model.is_empty(),
					},
				)
			})
			.collect();
		return format_grouped_files(files, |file_path| {
			rendered
				.get(file_path)
				.cloned()
				.unwrap_or(GroupedFileSection {
					skip: true,
					..GroupedFileSection::default()
				})
		});
	}
	format_file_matches_flat(files, render, format_hashline_header)
}

/// One file's contribution to grouped file output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupedFileSection {
	/// Optional suffix appended to file header.
	pub header_suffix: Option<String>,
	/// Body lines for model output.
	pub model_lines: Vec<String>,
	/// Body lines for display output; `None` falls back to `model_lines`.
	pub display_lines: Option<Vec<String>>,
	/// Omit file/header entirely when true.
	pub skip: bool,
}

/// Parallel model/display lines for grouped files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupedFilesOutput {
	/// Model-facing lines.
	pub model: Vec<String>,
	/// Display-facing lines.
	pub display: Vec<String>,
	/// Per-file end index in `model`.
	pub model_ends: HashMap<String, usize>,
}

/// One file's rendered search matches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMatchSection {
	/// Model lines.
	pub model: Vec<String>,
	/// Display lines.
	pub display: Vec<String>,
	/// Hashline tag when editable.
	pub tag: Option<String>,
}

/// Per-line classification kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupedLineKind {
	Dir,
	File,
	Content,
}

/// Per-line classification context.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupedLineContext {
	/// Kind.
	pub kind: Option<GroupedLineKind>,
	/// Header depth; 0 for content.
	pub depth: usize,
	/// Resolved header path.
	pub header_path: Option<String>,
	/// Full display path for file headers.
	pub display_path: Option<String>,
	/// Header suffix note.
	pub suffix: Option<String>,
	/// Owning file path for content lines.
	pub file_path: Option<String>,
	/// URL/internal target.
	pub url: Option<String>,
}

/// Whether a row is a match row (`*12│…` etc.).
pub fn is_match_row(line: &str) -> bool {
	let t = line.trim_start();
	if !t.starts_with('*') {
		return false;
	}
	let rest = &t['*'.len_utf8()..];
	let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
	if digits.is_empty() {
		return false;
	}
	matches!(rest.chars().nth(digits.len()), Some('│' | ':' | '|'))
}

/// Whether a row is a `#` grouped header.
pub fn is_grouped_header(line: &str) -> bool {
	let hashes = line.chars().take_while(|&c| c == '#').count();
	if hashes == 0 {
		return false;
	}
	line.chars().nth(hashes) == Some(' ')
}

/// Format a flat (non-grouped) file match output.
pub fn format_file_matches_flat(
	files: &[String],
	render: impl Fn(&str) -> FileMatchSection,
	format_hashline_header: impl Fn(&str, &str) -> String,
) -> GroupedFilesOutput {
	let mut model = Vec::new();
	let mut display = Vec::new();
	let mut model_ends = HashMap::new();
	for file_path in files {
		let rendered = render(file_path);
		if rendered.model.is_empty() {
			continue;
		}
		if !model.is_empty() {
			model.push(String::new());
			display.push(String::new());
		}
		if let Some(tag) = &rendered.tag {
			model.push(format_hashline_header(file_path, tag));
		}
		model.extend(rendered.model.iter().cloned());
		if rendered.display.is_empty() {
			display.extend(rendered.model.iter().cloned());
		} else {
			display.extend(rendered.display.iter().cloned());
		}
		model_ends.insert(file_path.clone(), model.len());
	}
	GroupedFilesOutput {
		model,
		display,
		model_ends,
	}
}

/// Legacy flat fallback: one `# file` header per file, in input order, without
/// prefix folding. Kept for callers that predate [`format_grouped_files`].
pub fn format_grouped_files_simple(
	files: &[String],
	render: impl Fn(&str) -> GroupedFileSection,
) -> GroupedFilesOutput {
	let mut model = Vec::new();
	let mut display = Vec::new();
	let mut model_ends = HashMap::new();
	let mut emitted = false;
	for file_path in files {
		let section = render(file_path);
		if section.skip {
			continue;
		}
		if emitted {
			model.push(String::new());
			display.push(String::new());
		}
		emitted = true;
		let suffix = section.header_suffix.as_deref().unwrap_or("");
		let header = format!("# {file_path}{suffix}");
		model.push(header.clone());
		display.push(header);
		model.extend(section.model_lines.iter().cloned());
		let disp = section.display_lines.as_ref().unwrap_or(&section.model_lines);
		display.extend(disp.iter().cloned());
		model_ends.insert(file_path.clone(), model.len());
	}
	GroupedFilesOutput {
		model,
		display,
		model_ends,
	}
}

/// Themed render of grouped/simple file output lines (headers/match markers painted).
pub fn render_grouped_output_themed(output: &GroupedFilesOutput, theme: &crate::theme::Theme) -> Vec<String> {
	output.display.iter().map(|line| {
		if is_grouped_header(line) {
			theme.fg("accent", line).unwrap_or_else(|_| line.clone())
		} else if is_match_row(line) {
			// paint leading `*`/line-no gutter dim, keep body
			if let Some((g, rest)) = line.split_once('│').or_else(|| line.split_once('|')).or_else(|| line.split_once(':')) {
				format!("{}{}", theme.fg("muted", g).unwrap_or_else(|_| g.to_string()), theme.fg("toolOutput", &format!("│{rest}")).unwrap_or_else(|_| format!("│{rest}")))
			} else { line.clone() }
		} else { line.clone() }
	}).collect()
}

/// Themed code-frame run render (best-effort: highlight match rows, dim gutters/headers).
pub fn render_code_frame_run_themed(lines: &[String], theme: &crate::theme::Theme) -> Vec<String> {
	lines.iter().map(|line| {
		if is_grouped_header(line) { theme.fg("accent", line).unwrap_or_else(|_| line.clone()) }
		else if is_match_row(line) { theme.fg("toolOutput", line).unwrap_or_else(|_| line.clone()) }
		else { theme.fg("muted", line).unwrap_or_else(|_| line.clone()) }
	}).collect()
}

/// Plain describe helpers (structural) for grouped output.
pub fn describe_grouped_output(output: &GroupedFilesOutput) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let head = tool_head("Files", &[ToolHeadDetail::Span(Span::styled(format!("{} lines", output.display.len()), "muted"))]);
	let mut p = Props::new(); p.set_str("wrap", "none");
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![ntext(NodeText::Plain(output.display.join("\n")), p)]), ..NativeToolView::default() }
}
pub fn describe_call(label: Option<&str>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let details = label.filter(|s| !s.is_empty()).map(|s| vec![ToolHeadDetail::Span(Span::styled(s.to_string(), "accent"))]).unwrap_or_default();
	NativeToolView { head: Some(NodeText::Spans(tool_head("Files", &details))), ..NativeToolView::default() }
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn classifiers() {
		assert!(is_match_row("*12│code"));
		assert!(is_match_row(" 12:code"));
		assert!(!is_match_row(" 12│code"));
		assert!(is_grouped_header("# src/"));
		assert!(is_grouped_header("## file.rs"));
		assert!(!is_grouped_header("nope"));
	}

	#[test]
	fn flat_file_matches_skips_empty() {
		let files = vec!["a.rs".to_string(), "b.rs".to_string()];
		let out = format_file_matches_flat(
			&files,
			|p| {
				if p == "a.rs" {
					FileMatchSection {
						model: vec!["1:hi".into()],
						display: vec![],
						tag: Some("t".into()),
					}
				} else {
					FileMatchSection::default()
				}
			},
			|p, t| format!("{p}#{t}"),
		);
		assert_eq!(out.model, vec!["a.rs#t", "1:hi"]);
		assert_eq!(out.model_ends.get("a.rs"), Some(&2));
		assert!(!out.model_ends.contains_key("b.rs"));
	}

	#[test]
	fn simple_grouped_files_headers() {
		let files = vec!["x.rs".to_string()];
		let out = format_grouped_files_simple(&files, |_| GroupedFileSection {
			model_lines: vec!["body".into()],
			..Default::default()
		});
		assert_eq!(out.model[0], "# x.rs");
		assert_eq!(out.model[1], "body");
	}

	#[test]
	fn format_grouped_files_nests_and_blank_separates_top_groups() {
		let files = vec![
			"pkg/ai/CHANGELOG.md".to_string(),
			"pkg/ai/src/util/x.ts".to_string(),
			"README.md".to_string(),
		];
		let out = format_grouped_files(&files, |file| GroupedFileSection {
			model_lines: vec![format!("  {file}")],
			header_suffix: Some(" (1)".to_string()),
			..GroupedFileSection::default()
		});
		assert_eq!(
			out.model,
			[
				"# README.md (1)",
				"  README.md",
				"",
				"# pkg/ai/",
				"## CHANGELOG.md (1)",
				"  pkg/ai/CHANGELOG.md",
				"",
				"## src/util/",
				"### x.ts (1)",
				"  pkg/ai/src/util/x.ts",
			]
		);
	}

	#[test]
	fn format_grouped_files_omits_skipped_and_empty_dirs() {
		let files = vec!["a/keep.ts".to_string(), "a/drop.ts".to_string()];
		let out = format_grouped_files(&files, |file| GroupedFileSection {
			model_lines: vec![format!("  {file}")],
			skip: file.ends_with("drop.ts"),
			..GroupedFileSection::default()
		});
		assert_eq!(out.model, ["# a/", "## keep.ts", "  a/keep.ts"]);
		assert!(!out.model_ends.contains_key("a/drop.ts"));
	}

	#[test]
	fn classify_reconstructs_nested_paths_and_display_paths() {
		let lines = vec![
			"# pkg/ai/",
			"## CHANGELOG.md",
			"  match",
			"## src/util/",
			"### x.ts",
			"*12│const y = 1;",
		];
		let ctx = classify_grouped_lines(&lines, Some("/repo"));
		assert_eq!(ctx[0].kind, Some(GroupedLineKind::Dir));
		assert_eq!(ctx[1].header_path.as_deref(), Some("/repo/pkg/ai/CHANGELOG.md"));
		assert_eq!(ctx[1].display_path.as_deref(), Some("pkg/ai/CHANGELOG.md"));
		assert_eq!(ctx[2].file_path.as_deref(), Some("/repo/pkg/ai/CHANGELOG.md"));
		assert_eq!(ctx[3].kind, Some(GroupedLineKind::Dir));
		assert_eq!(ctx[4].header_path.as_deref(), Some("/repo/pkg/ai/src/util/x.ts"));
		assert_eq!(ctx[4].display_path.as_deref(), Some("pkg/ai/src/util/x.ts"));
	}

	#[test]
	fn classify_keeps_absolute_folded_prefix_off_the_base() {
		let ctx = classify_grouped_lines(&["# /outside/dir/", "## file.txt"], Some("/repo"));
		assert_eq!(ctx[0].kind, Some(GroupedLineKind::Dir));
		assert_eq!(ctx[1].header_path.as_deref(), Some("/outside/dir/file.txt"));
	}

	#[test]
	fn classify_keeps_parenthesized_directory_name() {
		let ctx = classify_grouped_lines(&["# src (old)/", "## file.ts (2 replacements)"], Some("/repo"));
		assert_eq!(ctx[1].display_path.as_deref(), Some("src (old)/file.ts"));
		assert_eq!(ctx[1].suffix.as_deref(), Some("(2 replacements)"));
		assert_eq!(ctx[1].header_path.as_deref(), Some("/repo/src (old)/file.ts"));
	}

	#[test]
	fn classify_keeps_leading_slash_of_absolute_root() {
		let ctx = classify_grouped_lines(&["# /", "## z.ts", "", "## tmp/a/", "### x.ts"], Some("/repo"));
		assert_eq!(ctx[1].display_path.as_deref(), Some("/z.ts"));
		assert_eq!(ctx[4].display_path.as_deref(), Some("/tmp/a/x.ts"));
	}

	#[test]
	fn classify_strips_only_hashline_tag_suffix() {
		let ctx = classify_grouped_lines(
			&["# a.ts#1A2B (1 replacement)", "# component.ts#dead"],
			Some("/repo"),
		);
		assert_eq!(ctx[0].display_path.as_deref(), Some("a.ts"));
		assert_eq!(ctx[0].suffix.as_deref(), Some("(1 replacement)"));
		assert_eq!(ctx[1].display_path.as_deref(), Some("component.ts#dead"));
		assert_eq!(ctx[1].header_path.as_deref(), Some("/repo/component.ts#dead"));
	}

	#[test]
	fn classify_headerless_lines_use_scope() {
		let ctx = classify_grouped_lines(&["*7│needle();"], Some("/repo/file.ts"));
		assert_eq!(ctx[0].file_path.as_deref(), Some("/repo/file.ts"));

		let url = classify_grouped_lines_with_scope(&["*7│needle();"], Some("/repo"), Some("local://notes/a.ts"));
		assert_eq!(url[0].url.as_deref(), Some("local://notes/a.ts"));
		assert_eq!(url[0].file_path, None);
	}

	#[test]
	fn classify_url_headers_stay_urls() {
		let ctx = classify_grouped_lines(&["# omp://docs/", "  body"], Some("/repo"));
		assert_eq!(ctx[0].kind, Some(GroupedLineKind::File));
		assert_eq!(ctx[0].url.as_deref(), Some("omp://docs/"));
		assert_eq!(ctx[0].header_path, None);
		assert_eq!(ctx[1].url.as_deref(), Some("omp://docs/"));
		assert_eq!(ctx[1].file_path, None);
	}

	#[test]
	fn blank_groups_keep_original_indices() {
		assert_eq!(group_line_indices_by_blank(&["a", "b", "", "", "c"]), vec![vec![0, 1], vec![4]]);
		assert_eq!(group_line_indices_by_blank(&["", "a", "", ""]), vec![vec![1]]);
		assert_eq!(group_line_indices_by_blank(&["a", "b"]), vec![vec![0, 1]]);
		assert!(group_line_indices_by_blank::<&str>(&[]).is_empty());
	}
}
