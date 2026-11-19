//! The collapsed `Read` group: several file reads share one summary card.
//!
//! Reference: `l/src/chat/read-tool-group.ts`.
//!
//! Collapsed reads are output with leading spaces; the tree rows below the
//! header list one target per row, grouped per file, with the pending/error
//! glyph only on non-success rows. A single read that has content prints as
//! `Read <path>` plus its content rows.
//!
//! Deviations:
//! - No `Container`/`Text` children: the group is one [`Component`] whose
//!   render returns the header, the tree rows and the previews.
//! - Content previews are plain code rows (no code-cell syntax highlighting,
//!   Markdown cells, or hyperlink spans); the preview cap is the shared
//!   collapsed-line limit.
//! - Extension transcript presenters are not consulted: a group always draws
//!   its own card.
//! - Usage rows ride along as preformatted text, since the crate's usage
//!   block is a component of its own.

use std::collections::HashMap;

use serde_json::Value;

use crate::chat::messages::{AssistantContent, AssistantMessage, Usage};
use crate::chat::thinking_display::canonicalize_message;
use crate::chat::tool_execution::ToolExecutionHandle;
use crate::component::Component;
use crate::native::describe::{card, keyed, span, text as ntext};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span, Tone};
use crate::overlays::usage_row::format_usage_row;
use crate::render::render_utils::{format_status_icon, replace_tabs, ToolUiStatus, PREVIEW_LIMITS};
use crate::text_shape::{truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};
use crate::theme::theme_class::Theme;
use crate::tools::line_ranges::{parse_line_ranges, selector_line_ranges, BareLineMode};
use crate::tools::read::{read_result_is_markdown, split_path_and_sel, ReadRenderArgs, ReadToolDetails};
use crate::tools::url_scheme_host::{internal_url_scheme_spec, split_url_scheme};

/// Preview rows shown per read while collapsed.
pub const COLLAPSED_PREVIEW_LINES: usize = PREVIEW_LIMITS.OUTPUT_COLLAPSED;

/// The outer group must not cut later files off; code is trimmed by source
/// line, so its cap is far higher than the collapsed preview.
pub const TRIMMED_BODY_PREVIEW_LINES: usize = 1000;

/// Read status ranks, worst last.
pub fn read_status_rank(status: ReadStatus) -> u8 {
	match status {
		ReadStatus::Success => 0,
		ReadStatus::Pending => 1,
		ReadStatus::Warning => 2,
		ReadStatus::Error => 3,
	}
}

/// One read's display status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadStatus {
	/// The read finished.
	#[default]
	Success,
	/// The read has not returned.
	Pending,
	/// Finished with a warning.
	Warning,
	/// Failed.
	Error,
}

impl ReadStatus {
	/// Native card tone; successful reads stay neutral.
	pub fn tone(self) -> Option<Tone> {
		match self {
			ReadStatus::Success => None,
			ReadStatus::Pending => Some(Tone::Pending),
			ReadStatus::Warning => Some(Tone::Warning),
			ReadStatus::Error => Some(Tone::Error),
		}
	}

	/// Glyph state.
	pub fn ui_status(self) -> ToolUiStatus {
		match self {
			ReadStatus::Success => ToolUiStatus::Success,
			ReadStatus::Pending => ToolUiStatus::Pending,
			ReadStatus::Warning => ToolUiStatus::Warning,
			ReadStatus::Error => ToolUiStatus::Error,
		}
	}
}

/// Extract the read call's target path (`path`, or the legacy `file_path`).
pub fn read_args_target(args: Option<&Value>) -> Option<String> {
	let args = args?.as_object()?;
	args.get("path")
		.or_else(|| args.get("file_path"))
		.and_then(Value::as_str)
		.map(str::to_string)
}

/// Whether a read call names a target at all.
pub fn read_args_have_target(args: Option<&Value>) -> bool {
	read_args_target(args).is_some()
}

/// Whether a read collapses into the compact group: filesystem and external
/// targets always do, internal-URL schemes only when their spec says
/// `compactTranscript`.
pub fn read_args_collapse_into_group(args: Option<&Value>) -> bool {
	let Some(target) = read_args_target(args) else {
		return false;
	};
	match split_url_scheme(&target) {
		None => true,
		Some((scheme, _)) => internal_url_scheme_spec(&scheme)
			.map(|spec| spec.compact_transcript)
			.unwrap_or(true),
	}
}

/// The collapsed read calls that can own a turn's usage row; `None` when the
/// turn mixes them with anything visible.
pub fn grouped_read_usage_call_ids(message: &AssistantMessage) -> Option<Vec<String>> {
	let mut tool_call_ids = Vec::new();
	let mut saw_tool_call = false;
	for content in &message.content {
		match content {
			AssistantContent::ToolCall(call) => {
				if call.name != "read" || !read_args_collapse_into_group(Some(&call.arguments)) {
					return None;
				}
				saw_tool_call = true;
				tool_call_ids.push(call.id.clone());
			}
			AssistantContent::Image(_) => {
				if saw_tool_call {
					return None;
				}
			}
			AssistantContent::Text(text) => {
				if saw_tool_call && !canonicalize_message(Some(&text.text)).is_empty() {
					return None;
				}
			}
			AssistantContent::Thinking(thinking) => {
				if saw_tool_call && !canonicalize_message(Some(&thinking.thinking)).is_empty() {
					return None;
				}
			}
			_ => {}
		}
	}
	if tool_call_ids.is_empty() {
		None
	} else {
		Some(tool_call_ids)
	}
}

/// A path recovered from a delimited read, with the resolved link target for
/// its row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadDisplayPathSpec {
	/// The path as displayed.
	pub path: String,
	/// The filesystem target the row links to.
	pub link_path: Option<String>,
}

/// One read call in the group.
#[derive(Clone, Debug)]
pub struct ReadEntry {
	/// Provider call id.
	pub tool_call_id: String,
	/// The display path (selectors included).
	pub path: String,
	/// Extra display paths from a delimited read.
	pub display_paths: Option<Vec<ReadDisplayPathSpec>>,
	/// The resolved link target for the whole entry.
	pub link_path: Option<String>,
	/// Status.
	pub status: ReadStatus,
	/// A suffix the tool corrected.
	pub corrected_from: Option<String>,
	/// The result's text, when it arrived.
	pub content_text: Option<String>,
	/// Result content type.
	pub content_type: Option<String>,
	/// The call's `raw` argument.
	pub raw: bool,
	/// Conflict markers seen.
	pub conflict_count: Option<usize>,
	/// First source line for code previews.
	pub code_start_line: Option<usize>,
	/// Per-row source line numbers.
	pub code_line_numbers: Option<Vec<Option<usize>>>,
	/// The expanded card open for this read.
	pub expanded: bool,
}

impl Default for ReadEntry {
	fn default() -> Self {
		ReadEntry {
			tool_call_id: String::new(),
			path: String::new(),
			display_paths: None,
			link_path: None,
			status: ReadStatus::Success,
			corrected_from: None,
			content_text: None,
			content_type: None,
			raw: false,
			conflict_count: None,
			code_start_line: None,
			code_line_numbers: None,
			expanded: false,
		}
	}
}

impl ReadEntry {
	/// Whether the preview renders as Markdown instead of numbered source.
	pub fn previews_markdown(&self) -> bool {
		read_result_is_markdown(
			Some(&ReadToolDetails {
				content_type: self.content_type.clone(),
				..ReadToolDetails::default()
			}),
			Some(&ReadRenderArgs {
				path: Some(self.path.clone()),
				raw: self.raw,
				..ReadRenderArgs::default()
			}),
		)
	}
}

/// Usage attached to the group.
#[derive(Clone, Debug)]
pub struct ReadUsageRow {
	/// Calls this row covers.
	pub tool_call_ids: Vec<String>,
	/// Token counts.
	pub usage: Usage,
	/// Request duration.
	pub duration_ms: Option<i64>,
	/// Time to first token.
	pub ttft_ms: Option<i64>,
	/// Request timestamp.
	pub timestamp: Option<i64>,
	/// Turn elapsed.
	pub turn_elapsed_ms: Option<i64>,
}

/// One file's display targets, already split into selector parts.
#[derive(Clone, Debug)]
pub struct ReadDisplayTarget {
	/// The owning entry.
	pub tool_call_id: String,
	/// `path:selector`, or the raw path when it has no selector.
	pub target_path: String,
	/// The path without its selector.
	pub base_path: String,
	/// Link target.
	pub link_path: Option<String>,
	/// The selector part.
	pub selector: Option<String>,
}

/// One summary row: a file, plus every target that lands on it.
#[derive(Clone, Debug)]
pub struct ReadSummaryRow {
	/// `path:merged-selectors`.
	pub target_path: String,
	/// The path without selectors.
	pub base_path: String,
	/// The targets this row shows.
	pub targets: Vec<ReadDisplayTarget>,
}

/// Group options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadToolGroupOptions {
	/// Show content previews under rows.
	pub show_content_preview: bool,
	/// Ask extension presenters; the crate never does.
	pub transcript_presenters: bool,
}

impl Default for ReadToolGroupOptions {
	fn default() -> Self {
		ReadToolGroupOptions {
			show_content_preview: true,
			transcript_presenters: true,
		}
	}
}

/// Whether a top-level token is a bare line-range list.
pub fn selector_chunk_is_line_range_list(chunk: &str) -> bool {
	let trimmed = chunk.trim();
	if trimmed.is_empty() {
		return false;
	}
	matches!(parse_line_ranges(trimmed, BareLineMode::Single), Ok(Some(_)))
}

/// The next top-level `,`/`;`-delimited token starting at `start`, honouring
/// backslash escapes and `{}` groups.
pub fn next_top_level_token(input: &str, start: usize) -> String {
	let bytes = input.as_bytes();
	let mut brace_depth = 0usize;
	let mut index = start;
	while index < bytes.len() {
		let ch = bytes[index] as char;
		if ch == '\\' && index + 1 < bytes.len() {
			index += 2;
			continue;
		}
		if ch == '{' {
			brace_depth += 1;
			index += 1;
			continue;
		}
		if ch == '}' {
			if brace_depth > 0 {
				brace_depth -= 1;
			}
			index += 1;
			continue;
		}
		if brace_depth == 0 && (ch == ',' || ch == ';') {
			return input[start..index].to_string();
		}
		index += 1;
	}
	input[start..].to_string()
}

/// Whether the comma at `comma_index` continues the current part's line
/// selector (`a.ts:1-5,10-20`).
pub fn comma_continues_line_range_selector(input: &str, part_start: usize, comma_index: usize) -> bool {
	let next = next_top_level_token(input, comma_index + 1);
	let end = comma_index + 1 + next.len();
	let (path, selector) = split_path_and_sel(&input[part_start..end]);
	let Some(selector) = selector else {
		return false;
	};
	// Only a selector that reads as a line-range list keeps the comma.
	if !matches!(selector_line_ranges(Some(&selector)), Ok(Some(_))) {
		return false;
	}
	path.len() < comma_index - part_start
}

/// Split a read path into its comma/semicolon-delimited display specs, keeping
/// brace groups and line-range commas together.
pub fn split_read_display_path_specs(raw_path: &str) -> Vec<String> {
	let normalized = raw_path.trim();
	if normalized.is_empty() || split_url_scheme(normalized).is_some() {
		return vec![raw_path.to_string()];
	}
	let bytes = normalized.as_bytes();
	let mut parts: Vec<String> = Vec::new();
	let mut brace_depth = 0usize;
	let mut part_start = 0usize;
	let mut index = 0usize;
	while index < bytes.len() {
		let ch = bytes[index] as char;
		if ch == '\\' && index + 1 < bytes.len() {
			index += 2;
			continue;
		}
		if ch == '{' {
			brace_depth += 1;
			index += 1;
			continue;
		}
		if ch == '}' {
			if brace_depth > 0 {
				brace_depth -= 1;
			}
			index += 1;
			continue;
		}
		if brace_depth != 0 || (ch != ',' && ch != ';') {
			index += 1;
			continue;
		}
		if ch == ',' && comma_continues_line_range_selector(normalized, part_start, index) {
			index += 1;
			continue;
		}
		parts.push(normalized[part_start..index].trim().to_string());
		part_start = index + 1;
		index += 1;
	}
	parts.push(normalized[part_start..].trim().to_string());
	let clean: Vec<String> = parts.into_iter().filter(|part| !part.is_empty()).collect();
	if clean.len() <= 1 {
		return vec![raw_path.to_string()];
	}
	if clean
		.iter()
		.all(|part| split_path_and_sel(part).1.is_some())
	{
		clean
	} else {
		vec![raw_path.to_string()]
	}
}

/// Split a selector into its display parts: `1-5,10-20` becomes two selectors.
pub fn split_selector_display_parts(selector: Option<&str>) -> Vec<Option<String>> {
	let Some(selector) = selector else {
		return vec![None];
	};
	let chunks: Vec<&str> = selector.split(':').collect();
	if chunks.len() == 1 {
		if !selector_chunk_is_line_range_list(selector) || !selector.contains(',') {
			return vec![Some(selector.to_string())];
		}
		return selector
			.split(',')
			.map(|chunk| chunk.trim())
			.filter(|chunk| !chunk.is_empty())
			.map(|chunk| Some(chunk.to_string()))
			.collect();
	}
	if chunks.len() == 2 {
		let (left, right) = (chunks[0], chunks[1]);
		let left_is_range = selector_chunk_is_line_range_list(left);
		let right_is_range = selector_chunk_is_line_range_list(right);
		if left_is_range && left.contains(',') {
			return left
				.split(',')
				.map(str::trim)
				.filter(|chunk| !chunk.is_empty())
				.map(|chunk| Some(format!("{chunk}:{right}")))
				.collect();
		}
		if right_is_range && right.contains(',') {
			return right
				.split(',')
				.map(str::trim)
				.filter(|chunk| !chunk.is_empty())
				.map(|chunk| Some(format!("{left}:{chunk}")))
				.collect();
		}
	}
	vec![Some(selector.to_string())]
}

/// Join merged selector parts, eliding the middle beyond three.
pub fn format_merged_selector_parts(selectors: &[String]) -> String {
	if selectors.len() <= 3 {
		return selectors.join(",");
	}
	format!("{},{}…,{}", selectors[0], selectors[1], selectors[selectors.len() - 1])
}

/// The first line a selector names.
pub fn first_selector_line(selector: Option<&str>) -> Option<u64> {
	selector_line_ranges(selector).ok().flatten()?.first().map(|range| range.start_line)
}

/// Whether a path reads as a URL.
pub fn is_url_like(path: &str) -> bool {
	path.contains("://")
}

/// The collapsed read group.
pub struct ReadToolGroupComponent {
	theme: Theme,
	options: ReadToolGroupOptions,
	entries: Vec<ReadEntry>,
	entry_index: HashMap<String, usize>,
	usage_rows: Vec<ReadUsageRow>,
	usage_batch_by_tool_call_id: HashMap<String, String>,
	expanded: bool,
	tool_activity_visible: bool,
	block_version: u64,
	sealed: bool,
	last_rows: Vec<ReadSummaryRow>,
}

impl ReadToolGroupComponent {
	/// An empty group.
	pub fn new(options: ReadToolGroupOptions, theme: Theme) -> Self {
		ReadToolGroupComponent {
			theme,
			options,
			entries: Vec::new(),
			entry_index: HashMap::new(),
			usage_rows: Vec::new(),
			usage_batch_by_tool_call_id: HashMap::new(),
			expanded: false,
			tool_activity_visible: true,
			block_version: 0,
			sealed: false,
			last_rows: Vec::new(),
		}
	}

	/// The group's entries.
	pub fn entries(&self) -> &[ReadEntry] {
		&self.entries
	}

	/// The last summary rows the group built.
	pub fn rows(&self) -> &[ReadSummaryRow] {
		&self.last_rows
	}

	/// The entry for a call.
	pub fn entry(&self, tool_call_id: &str) -> Option<&ReadEntry> {
		self.entry_index
			.get(tool_call_id)
			.and_then(|index| self.entries.get(*index))
	}

	/// Whether the group is expanded.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// Whether the group is sealed.
	pub fn is_sealed(&self) -> bool {
		self.sealed
	}

	/// Post-finalize mutation counter.
	pub fn transcript_block_version(&self) -> u64 {
		self.block_version
	}

	/// Any entry still without a result.
	pub fn has_pending_entries(&self) -> bool {
		self.entries.iter().any(|entry| entry.status == ReadStatus::Pending)
	}

	/// Seal as final history.
	pub fn finalize(&mut self) {
		self.sealed = true;
		self.block_version += 1;
	}

	/// A result arrived for an entry.
	pub fn update_result(
		&mut self,
		tool_call_id: &str,
		text: String,
		details: Option<&ReadToolDetails>,
		is_error: bool,
	) {
		let Some(index) = self.entry_index.get(tool_call_id).copied() else {
			return;
		};
		let entry = &mut self.entries[index];
		if let Some(details) = details {
			if !details.display_read_targets.is_empty() {
				let display = details
					.display_read_targets
					.iter()
					.enumerate()
					.map(|(i, path)| ReadDisplayPathSpec {
						path: path.trim().to_string(),
						link_path: details
							.display_read_target_links
							.get(i)
							.cloned()
							.flatten()
							.filter(|link| !link.is_empty()),
					})
					.filter(|spec| !spec.path.is_empty())
					.collect::<Vec<_>>();
				if !display.is_empty() {
					entry.display_paths = Some(display);
				}
			}
			entry.conflict_count = details.conflict_count.filter(|count| *count > 0);
			entry.content_type = details.content_type.clone();
			entry.link_path = details
				.display_target
				.clone()
				.or_else(|| details.resolved_path.clone());
			if let Some(resolution) = &details.suffix_resolution {
				entry.corrected_from = Some(resolution.from.clone());
			}
			if let Some(content) = &details.display_content {
				entry.code_start_line = Some(content.start_line);
				entry.code_line_numbers = Some(content.line_numbers.clone());
			}
		}
		entry.content_text = Some(text);
		entry.status = if is_error {
			ReadStatus::Error
		} else if entry.conflict_count.is_some() {
			ReadStatus::Warning
		} else {
			ReadStatus::Success
		};
		self.block_version += 1;
	}

	/// Rename an entry's call id.
	pub fn rename_entry(&mut self, old_id: &str, new_id: &str) {
		let Some(index) = self.entry_index.remove(old_id) else {
			return;
		};
		self.entry_index.insert(new_id.to_string(), index);
		self.entries[index].tool_call_id = new_id.to_string();
		self.block_version += 1;
	}

	/// Drop an entry; returns whether it existed.
	pub fn remove_entry(&mut self, tool_call_id: &str) -> bool {
		let Some(index) = self.entry_index.remove(tool_call_id) else {
			return false;
		};
		self.entries.remove(index);
		self.entry_index.clear();
		for (position, entry) in self.entries.iter().enumerate() {
			self.entry_index.insert(entry.tool_call_id.clone(), position);
		}
		self.block_version += 1;
		true
	}

	/// Attach the turn's usage to the group's last rows; false when no row
	/// covers those calls.
	pub fn attach_usage(
		&mut self,
		tool_call_ids: &[String],
		usage: &Usage,
		duration_ms: Option<i64>,
		ttft_ms: Option<i64>,
		timestamp: Option<i64>,
		turn_elapsed_ms: Option<i64>,
	) -> bool {
		if !tool_call_ids
			.iter()
			.any(|id| self.entry_index.contains_key(id))
		{
			return false;
		}
		let batch = format!("batch-{}", self.usage_rows.len());
		for id in tool_call_ids {
			self.usage_batch_by_tool_call_id.insert(id.clone(), batch.clone());
		}
		self.usage_rows.push(ReadUsageRow {
			tool_call_ids: tool_call_ids.to_vec(),
			usage: usage.clone(),
			duration_ms,
			ttft_ms,
			timestamp,
			turn_elapsed_ms,
		});
		self.block_version += 1;
		true
	}

	/// Hide or show the group with transcript-wide tool activity.
	pub fn set_tool_activity_visible(&mut self, visible: bool) {
		self.tool_activity_visible = visible;
	}

	/// Display targets of `entries`, split per selector.
	pub fn display_targets(&self) -> Vec<ReadDisplayTarget> {
		let mut targets = Vec::new();
		for entry in &self.entries {
			let specs: Vec<ReadDisplayPathSpec> = match &entry.display_paths {
				Some(specs) => specs.clone(),
				None => split_read_display_path_specs(&entry.path)
					.into_iter()
					.map(|path| ReadDisplayPathSpec {
						path,
						link_path: None,
					})
					.collect(),
			};
			let single_spec = specs.len() == 1;
			for spec in specs {
				let (base_path, selector) = split_path_and_sel(&spec.path);
				let entry_link = spec
					.link_path
					.clone()
					.or_else(|| if single_spec { entry.link_path.clone() } else { None });
				let link_path = entry_link.or_else(|| {
					if std::path::Path::new(&base_path).is_absolute() {
						Some(base_path.clone())
					} else {
						None
					}
				});
				for part in split_selector_display_parts(selector.as_deref()) {
					targets.push(ReadDisplayTarget {
						tool_call_id: entry.tool_call_id.clone(),
						target_path: match &part {
							Some(part) => format!("{base_path}:{part}"),
							None => spec.path.clone(),
						},
						base_path: base_path.clone(),
						link_path: link_path.clone(),
						selector: part,
					});
				}
			}
		}
		targets
	}

	/// Group targets into summary rows, merging same-batch selectors per file.
	pub fn build_summary_rows(&self, targets: &[ReadDisplayTarget]) -> Vec<ReadSummaryRow> {
		// base path -> batch -> targets
		let mut by_path: HashMap<String, HashMap<Option<String>, Vec<usize>>> = HashMap::new();
		for (index, target) in targets.iter().enumerate() {
			if target.selector.is_none() {
				continue;
			}
			let batch = self
				.usage_batch_by_tool_call_id
				.get(&target.tool_call_id)
				.cloned();
			by_path
				.entry(target.base_path.clone())
				.or_default()
				.entry(batch)
				.or_default()
				.push(index);
		}
		let mut merged_group: HashMap<usize, Vec<usize>> = HashMap::new();
		for groups in by_path.values() {
			for group in groups.values() {
				if group.len() <= 1 {
					continue;
				}
				for index in group {
					merged_group.insert(*index, group.clone());
				}
			}
		}
		let mut emitted: Vec<usize> = Vec::new();
		let mut rows = Vec::new();
		for (index, target) in targets.iter().enumerate() {
			if let Some(group) = merged_group.get(&index) {
				if !emitted.contains(&group[0]) {
					emitted.push(group[0]);
					let selectors: Vec<String> = group
						.iter()
						.filter_map(|member| targets[*member].selector.clone())
						.collect();
					rows.push(ReadSummaryRow {
						target_path: format!(
							"{}:{}",
							target.base_path,
							format_merged_selector_parts(&selectors)
						),
						base_path: target.base_path.clone(),
						targets: group.iter().map(|member| targets[*member].clone()).collect(),
					});
				}
				continue;
			}
			rows.push(ReadSummaryRow {
				target_path: target.target_path.clone(),
				base_path: target.base_path.clone(),
				targets: vec![target.clone()],
			});
		}
		rows
	}

	/// Worst status across a row's targets.
	pub fn status_for_targets(&self, targets: &[ReadDisplayTarget]) -> ReadStatus {
		let mut status = ReadStatus::Success;
		for target in targets {
			if let Some(entry) = self.entry(&target.tool_call_id) {
				if read_status_rank(entry.status) > read_status_rank(status) {
					status = entry.status;
				}
			}
		}
		status
	}

	/// The corrected-from path a row reports.
	pub fn corrected_from_for_targets(&self, targets: &[ReadDisplayTarget]) -> Option<String> {
		targets
			.iter()
			.find_map(|target| self.entry(&target.tool_call_id)?.corrected_from.clone())
	}

	/// The largest conflict count a row reports.
	pub fn conflict_count_for_targets(&self, targets: &[ReadDisplayTarget]) -> Option<usize> {
		targets
			.iter()
			.filter_map(|target| self.entry(&target.tool_call_id)?.conflict_count)
			.max()
			.filter(|count| *count > 0)
	}

	/// Whether an entry draws its content under the rows.
	pub fn should_render_preview(&self, entry: &ReadEntry) -> bool {
		if !self.options.show_content_preview {
			return false;
		}
		if entry.expanded {
			return true;
		}
		entry
			.content_text
			.as_ref()
			.map(|text| !text.trim().is_empty())
			.unwrap_or(false)
	}

	/// The path cell of a row: path, selector, corrected note, conflict badge.
	pub fn format_row_path(&self, row: &ReadSummaryRow) -> String {
		let (base, selector) = if let Some(rest) = row.target_path.strip_prefix(&format!("{}:", row.base_path)) {
			(row.base_path.clone(), Some(rest.to_string()))
		} else {
			split_path_and_sel(&row.target_path)
		};
		let mut painted = if is_url_like(&base) {
			self.theme
				.fg("accent", &base)
				.unwrap_or_else(|_| base.clone())
		} else {
			crate::render::render_utils::format_styled_path(&base, &self.theme)
		};
		if let Some(selector) = &selector {
			let painted_selector = self
				.theme
				.fg("muted", &format!(":{selector}"))
				.unwrap_or_else(|_| format!(":{selector}"));
			painted.push_str(&painted_selector);
		}
		if let Some(corrected) = self.corrected_from_for_targets(&row.targets) {
			let note = format!(" (corrected from {corrected})");
			painted.push_str(&self.theme.fg("dim", &note).unwrap_or(note));
		}
		if let Some(count) = self.conflict_count_for_targets(&row.targets) {
			let badge = format!(
				" (⚠ {count} conflict{})",
				if count == 1 { "" } else { "s" }
			);
			painted.push_str(&self.theme.fg("warning", &badge).unwrap_or(badge));
		}
		painted
	}

	fn status_prefix(&self, row: &ReadSummaryRow) -> String {
		let status = self.status_for_targets(&row.targets);
		if status == ReadStatus::Success {
			String::new()
		} else {
			format!(
				"{} ",
				format_status_icon(status.ui_status(), &self.theme, None)
			)
		}
	}

	fn tree_branch(&self) -> String {
		let glyph = self.theme.symbol("tree.branch");
		if glyph.is_empty() { "├─".to_string() } else { glyph.to_string() }
	}

	fn tree_last(&self) -> String {
		let glyph = self.theme.symbol("tree.last");
		if glyph.is_empty() { "└─".to_string() } else { glyph.to_string() }
	}

	fn usage_rows_by_summary_row(&self, rows: &[ReadSummaryRow]) -> HashMap<usize, Vec<&ReadUsageRow>> {
		let mut last_row_by_call: HashMap<&str, usize> = HashMap::new();
		for (index, row) in rows.iter().enumerate() {
			for target in &row.targets {
				last_row_by_call.insert(target.tool_call_id.as_str(), index);
			}
		}
		let mut by_row: HashMap<usize, Vec<&ReadUsageRow>> = HashMap::new();
		for usage in &self.usage_rows {
			let last = usage
				.tool_call_ids
				.iter()
				.filter_map(|id| last_row_by_call.get(id.as_str()).copied())
				.max();
			if let Some(last) = last {
				by_row.entry(last).or_default().push(usage);
			}
		}
		by_row
	}

	fn format_usage(&self, usage: &ReadUsageRow) -> String {
		format_usage_row(
			&crate::overlays::usage_row::Usage::from(&usage.usage),
			usage.duration_ms,
			usage.ttft_ms,
			usage.timestamp,
			usage.turn_elapsed_ms,
		)
	}

	/// The header + tree rows, without previews.
	pub fn summary_lines(&self) -> Vec<String> {
		let mut lines = Vec::new();
		let rows = self.build_summary_rows(&self.display_targets());
		if rows.is_empty() {
			return lines;
		}
		let usage_by_row = self.usage_rows_by_summary_row(&rows);
		let single_preview = rows.len() == 1
			&& rows[0]
				.targets
				.first()
				.and_then(|target| self.entry(&target.tool_call_id))
				.map(|entry| self.should_render_preview(entry))
				.unwrap_or(false);
		if rows.len() == 1 && !single_preview {
			let row = &rows[0];
			let title = self
				.theme
				.fg("toolTitle", &self.theme.bold("Read"))
				.unwrap_or_else(|_| "Read".to_string());
			let head = format!(" {} {} {}", self.status_prefix(row).trim_end(), title, self.format_row_path(row));
			lines.push(head.trim_end().to_string());
			for usage in usage_by_row.get(&0).into_iter().flatten() {
				lines.push(
					self.theme
						.fg("dim", &format!("   {}", self.format_usage(usage)))
						.unwrap_or_else(|_| self.format_usage(usage)),
				);
			}
			return lines;
		}
		let bullet = {
			let bullet = self.theme.symbol("bullet");
			if bullet.is_empty() { "•".to_string() } else { bullet.to_string() }
		};
		let title = self
			.theme
			.fg("toolTitle", &self.theme.bold("Read"))
			.unwrap_or_else(|_| "Read".to_string());
		let count = self
			.theme
			.fg("dim", &format!(" ({})", rows.len()))
			.unwrap_or_else(|_| format!(" ({})", rows.len()));
		lines.push(format!(" {bullet} {title}{count}"));
		let total = rows.len();
		for (index, row) in rows.iter().enumerate() {
			let connector = if index + 1 == total {
				self.tree_last()
			} else {
				self.tree_branch()
			};
			let row_text = format!("{}{}", self.status_prefix(row), self.format_row_path(row));
			lines.push(
				format!(
					"   {} {}",
					self.theme.fg("dim", &connector).unwrap_or_else(|_| connector.clone()),
					row_text
				)
				.trim_end()
				.to_string(),
			);
			let continuation = if index + 1 == total {
				" ".repeat(visible_width(&connector))
			} else {
				let vertical = {
					let glyph = self.theme.symbol("tree.vertical");
					if glyph.is_empty() { "│".to_string() } else { glyph.to_string() }
				};
				format!(
					"{vertical}{}",
					" ".repeat(visible_width(&connector).saturating_sub(visible_width(&vertical)))
				)
			};
			for usage in usage_by_row.get(&index).into_iter().flatten() {
				let text = format!("   {continuation} {}", self.format_usage(usage));
				lines.push(self.theme.fg("dim", &text).unwrap_or(text));
			}
		}
		lines
	}

	/// Preview rows for one entry.
	pub fn preview_lines(&self, entry: &ReadEntry, width: usize) -> Vec<String> {
		let Some(text) = &entry.content_text else {
			return Vec::new();
		};
		let cap = if entry.expanded {
			TRIMMED_BODY_PREVIEW_LINES
		} else {
			COLLAPSED_PREVIEW_LINES
		};
		let mut rows = Vec::new();
		let start = entry.code_start_line.unwrap_or(1);
		for (offset, line) in text.lines().take(cap).enumerate() {
			let gutter = match entry.code_line_numbers.as_ref().and_then(|numbers| numbers.get(offset).copied().flatten()) {
				Some(number) => format!("{number:>5} "),
				None => {
					if entry.previews_markdown() {
						String::new()
					} else {
						format!("{:>5} ", start + offset)
					}
				}
			};
			let gutter = self.theme.fg("dim", &gutter).unwrap_or(gutter);
			for wrapped in wrap_text_with_ansi(&replace_tabs(line), width.saturating_sub(visible_width(&gutter)).max(1)) {
				rows.push(truncate_to_width(&format!("{gutter}{wrapped}"), width, Ellipsis::Omit));
			}
		}
		let extra = text.lines().count().saturating_sub(cap);
		if extra > 0 {
			let more = format!("… {extra} more lines");
			rows.push(self.theme.fg("dim", &more).unwrap_or(more));
		}
		rows
	}
}

impl Component for ReadToolGroupComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		if !self.tool_activity_visible {
			return Vec::new();
		}
		let width = width.max(1) as usize;
		self.last_rows = self.build_summary_rows(&self.display_targets());
		let mut rows = self.summary_lines();
		for entry in &self.entries {
			if self.should_render_preview(entry) {
				rows.extend(self.preview_lines(entry, width));
			}
		}
		rows
	}

	fn invalidate(&mut self) {
		self.block_version += 1;
	}
}

impl ToolExecutionHandle for ReadToolGroupComponent {
	fn update_args(&mut self, args: Option<Value>, tool_call_id: Option<&str>) {
		self.update_args_ref(args.as_ref(), tool_call_id);
	}

	fn update_result(
		&mut self,
		_text: crate::chat::tool_execution::ToolResultFrame,
		_is_partial: bool,
		_tool_call_id: Option<&str>,
	) {
		// Read results arrive through `ReadToolGroupComponent::update_result`,
		// which carries the typed `ReadToolDetails`.
	}

	fn set_expanded(&mut self, expanded: bool) {
		if self.expanded != expanded {
			self.block_version += 1;
		}
		self.expanded = expanded;
		for entry in &mut self.entries {
			entry.expanded = expanded;
		}
	}

	fn park_as_background(&mut self) {}

	fn seal(&mut self) {
		self.finalize();
	}
}

impl ReadToolGroupComponent {
	/// Borrowing form of [`ReadToolGroupComponent::update_args`], for callers
	/// that own the args.
	pub fn update_args_ref(&mut self, args: Option<&Value>, tool_call_id: Option<&str>) {
		let Some(tool_call_id) = tool_call_id else {
			return;
		};
		let Some(path) = read_args_target(args) else {
			return;
		};
		let raw = args
			.and_then(|args| args.get("raw"))
			.and_then(Value::as_bool)
			.unwrap_or(false);
		if let Some(index) = self.entry_index.get(tool_call_id).copied() {
			let entry = &mut self.entries[index];
			entry.path = path;
			entry.raw = raw;
			self.block_version += 1;
			return;
		}
		let entry = ReadEntry {
			tool_call_id: tool_call_id.to_string(),
			path,
			raw,
			status: ReadStatus::Pending,
			..ReadEntry::default()
		};
		self.entry_index
			.insert(tool_call_id.to_string(), self.entries.len());
		self.entries.push(entry);
		self.block_version += 1;
	}

	/// Whether the group has any entries.
	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	/// The group's native summary head.
	pub fn native_head_spans(&self) -> Vec<Span> {
		let rows = self.build_summary_rows(&self.display_targets());
		if rows.len() == 1 {
			let row = &rows[0];
			let mut spans = vec![span("Read", Some("toolTitle strong")), span(" ", None)];
			spans.push(span(row.target_path.clone(), Some("muted")));
			return spans;
		}
		vec![
			span("Read", Some("toolTitle strong")),
			span(format!(" ({})", rows.len()), Some("dim")),
		]
	}
}

impl crate::native::describe::NativeDescribe for ReadToolGroupComponent {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		if !self.tool_activity_visible {
			return card(Props::new(), Vec::new());
		}
		let rows = self.build_summary_rows(&self.display_targets());
		let status = rows
			.iter()
			.map(|row| self.status_for_targets(&row.targets))
			.max_by_key(|status| read_status_rank(*status))
			.unwrap_or(ReadStatus::Success);
		let mut props = Props::new();
		props.set_str("role", "omp.read.group");
		props.set_head(NodeText::Spans(self.native_head_spans()));
		props.set_bool("collapsible", true);
		props.set_bool("collapsed", !self.expanded);
		if let Some(tone) = status.tone() {
			props.set_tone(tone);
		}
		let children = rows
			.iter()
			.enumerate()
			.map(|(index, row)| {
				let mut row_props = Props::new();
				row_props.set_str("role", "omp.read.row");
				let mut spans = vec![span(row.target_path.clone(), Some("accent"))];
				if let Some(corrected) = self.corrected_from_for_targets(&row.targets) {
					spans.push(span(
						format!(" (corrected from {corrected})"),
						Some("dim"),
					));
				}
				if let Some(count) = self.conflict_count_for_targets(&row.targets) {
					spans.push(span(
						format!(
							" (⚠ {count} conflict{})",
							if count == 1 { "" } else { "s" }
						),
						Some("warning"),
					));
				}
				if let Some(tone) = self.status_for_targets(&row.targets).tone() {
					row_props.set_tone(tone);
				}
				keyed(ntext(NodeText::Spans(spans), row_props), format!("row{index}"))
			})
			.collect();
		crate::native::describe::card(props, children)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::messages::{AssistantContent, TextContent, ToolCall};
	use crate::native::describe::NativeDescribe;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};
	use crate::theme::theme_class::ColorMode;
	use serde_json::json;

	fn theme() -> Theme {
		create_theme(
			builtin_theme_json("dark").unwrap(),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.unwrap()
	}

	fn group() -> ReadToolGroupComponent {
		ReadToolGroupComponent::new(ReadToolGroupOptions::default(), theme())
	}

	#[test]
	fn targets_split_selectors_but_keep_line_lists() {
		assert_eq!(
			split_read_display_path_specs("a.ts:1-5,10-20,b.ts:2"),
			vec!["a.ts:1-5,10-20", "b.ts:2"]
		);
		// A part without a selector keeps the whole string intact.
		assert_eq!(
			split_read_display_path_specs("a.ts:1-5,10-20,b.ts"),
			vec!["a.ts:1-5,10-20,b.ts"]
		);
		assert_eq!(split_read_display_path_specs("a.ts:1-5,10-20"), vec!["a.ts:1-5,10-20"]);
		assert_eq!(
			split_selector_display_parts(Some("1-5,10-20")),
			vec![Some("1-5".to_string()), Some("10-20".to_string())]
		);
		assert_eq!(
			split_selector_display_parts(Some("raw")),
			vec![Some("raw".to_string())]
		);
		assert_eq!(format_merged_selector_parts(&["1-5".to_string(), "10".to_string()]), "1-5,10");
		assert_eq!(first_selector_line(Some("40-60")), Some(40));
	}

	#[test]
	fn collapse_leaves_internal_terminal_schemes_alone() {
		assert!(read_args_collapse_into_group(Some(&json!({"path": "src/main.rs"}))));
		assert!(read_args_collapse_into_group(Some(&json!({"file_path": "src/main.rs"}))));
		assert!(!read_args_collapse_into_group(Some(&json!({}))));
		assert!(!read_args_collapse_into_group(None));
	}

	#[test]
	fn grouped_usage_ids_stop_at_visible_content() {
		let call = |id: &str| {
			AssistantContent::ToolCall(ToolCall {
				id: id.to_string(),
				name: "read".to_string(),
				arguments: json!({"path": "src/main.rs"}),
				..ToolCall::default()
			})
		};
		let read_only = AssistantMessage {
			content: vec![call("a"), call("b")],
			..AssistantMessage::default()
		};
		assert_eq!(grouped_read_usage_call_ids(&read_only), Some(vec!["a".to_string(), "b".to_string()]));

		let mut mixed = read_only.clone();
		mixed.content.push(AssistantContent::Text(TextContent {
			text: "here is why".to_string(),
			..TextContent::default()
		}));
		assert_eq!(grouped_read_usage_call_ids(&mixed), None);
	}

	#[test]
	fn rows_group_same_file_selectors_and_status_wins() {
		let mut group = group();
		group.update_args_ref(Some(&json!({"path": "a.ts:1-5"})), Some("a"));
		group.update_args_ref(Some(&json!({"path": "a.ts:10-20"})), Some("b"));
		group.update_args_ref(Some(&json!({"path": "c.ts"})), Some("c"));
		group.update_result("a", "one".to_string(), None, false);
		group.update_result("b", "two".to_string(), None, true);

		let targets = group.display_targets();
		let rows = group.build_summary_rows(&targets);
		assert_eq!(rows.len(), 2, "{rows:?}");
		assert_eq!(rows[0].target_path, "a.ts:1-5,10-20");
		assert_eq!(group.status_for_targets(&rows[0].targets), ReadStatus::Error);
		assert_eq!(rows[1].target_path, "c.ts");
	}

	#[test]
	fn summary_and_preview_render_through_the_component() {
		let mut group = group();
		group.update_args_ref(Some(&json!({"path": "src/main.rs"})), Some("a"));
		group.update_result(
			"a",
			"fn main() {}\nfn helper() {}".to_string(),
			None,
			false,
		);
		let rows = group.render(80);
		let plain = rows
			.iter()
			.map(|row| crate::ansi_scan::strip_ansi(row))
			.collect::<Vec<_>>()
			.join("\n");
		assert!(plain.contains("Read"), "{plain}");
		assert!(plain.contains("src/main.rs"), "{plain}");
		assert!(plain.contains("fn main"), "{plain}");
	}

	#[test]
	fn native_group_reports_the_read_role() {
		let mut group = group();
		group.update_args_ref(Some(&json!({"path": "src/main.rs"})), Some("a"));
		let described = group.describe_native(&DescribeContext::new(80));
		assert_eq!(described.props.get_str("role"), Some("omp.read.group"));
		assert_eq!(described.props.get_bool("collapsed"), Some(true));
	}
}
