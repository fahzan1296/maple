//! Render a code or markdown cell with an optional output section.
//!
//! Reference: `l/src/render/code-cell.ts`.
//!
//! The native half of the output block (`describeOutputBlock`,
//! `NativeOutputBlockSection`, `outputStateTone`, `outputStateStatus`) is
//! defined here rather than in `render/output_block.rs`, which ports only the
//! ANSI `renderOutputBlock` path; the reference defines them in
//! `l/src/render/output-block.ts`.
//!
//! Deviations forced by Rust:
//! - `highlightCode` (`l/src/theme/tui-adapters.ts`) depends on the native
//!   tree-sitter highlighter, which this crate does not ship. Code previews
//!   fall back to plain lines; the terminal applies its own highlighting from
//!   the `code` node's `lang`/`start` props.
//! - `getMarkdownTheme` (`l/src/theme/tui-adapters.ts`) derives a theme-bound
//!   markdown palette from the active theme. The crate's `Markdown` component
//!   only offers `MarkdownTheme::default()`, so that is used.
//! - `CodeCellStatus` is an enum rather than a string union, and the optional
//!   numeric/boolean fields are `Option`s so callers can omit them the way the
//!   TypeScript interfaces allow.

use crate::component::Component;
use crate::components::markdown::Markdown;
use crate::native::describe::{code, md, node, span};
use crate::native::node::{NativeNode, NodeKind, Props, Span, Text as NodeText, Tone};
use crate::render::output_pane::{
	describe_output_lines, format_output_pane_lines, split_terminal_output_lines, NativeOutputOptions,
	OutputPaneFormatOptions,
};
use crate::render::render_utils::{
	format_expand_hint, format_more_items, format_status_icon, paint, replace_tabs,
	style_tool_output_line, State, ToolUiStatus, PaneEdge,
};
use crate::render::width_aware_text::wrap_prefixed_line;
use crate::status_line::segments::format_duration;
use crate::theme::Theme;
use crate::native::styled_spans;
use crate::render::output_block::{
	output_block_content_width, render_output_block, OutputBlockOptions, OutputSection,
};

/// Lifecycle status of a code or markdown cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeCellStatus {
	Pending,
	Running,
	Warning,
	Complete,
	Error,
}

impl CodeCellStatus {
	/// The wire label used in the header (`pending`, `running`, …).
	pub fn as_str(self) -> &'static str {
		match self {
			CodeCellStatus::Pending => "pending",
			CodeCellStatus::Running => "running",
			CodeCellStatus::Warning => "warning",
			CodeCellStatus::Complete => "complete",
			CodeCellStatus::Error => "error",
		}
	}
}

/// Content and display limits for a code preview with optional output.
#[derive(Clone, Debug)]
pub struct CodeCellOptions {
	/// Source text; tabs are expanded before splitting.
	pub code: String,
	/// Highlight language id.
	pub language: Option<String>,
	/// Zero-based cell index.
	pub index: Option<usize>,
	/// Cell count, for the `[i/n]` header badge.
	pub total: Option<usize>,
	/// Header title.
	pub title: Option<String>,
	/// Lifecycle status.
	pub status: Option<CodeCellStatus>,
	/// Spinner frame for a running cell.
	pub spinner_frame: Option<usize>,
	/// Elapsed milliseconds, shown in the header meta.
	pub duration: Option<u64>,
	/// Optional output stream.
	pub output: Option<String>,
	/// Row cap for the output section while collapsed.
	pub output_max_lines: Option<usize>,
	/// Row cap for the code preview while collapsed.
	pub code_max_lines: Option<usize>,
	/// Show the LAST `code_max_lines` rows (the live streaming edge) instead of
	/// the first, with a "… N earlier lines" marker on top. Ignored when
	/// expanded.
	pub code_tail: Option<bool>,
	/// Whether the cell is expanded.
	pub expanded: Option<bool>,
	/// Prefix the header with the cell's language icon.
	pub show_language: Option<bool>,
	/// Outer block width.
	pub width: usize,
	/// First line number, when the caller does not supply explicit numbers.
	pub code_start_line: Option<usize>,
	/// Per-row line numbers; `None` entries draw a blank gutter.
	pub code_line_numbers: Option<Vec<Option<usize>>>,
}

/// [`CodeCellOptions`] without the render-only `width`, for
/// [`describe_code_cell`].
#[derive(Clone, Debug)]
pub struct CodeCellDescribeOptions {
	/// Source text.
	pub code: String,
	/// Highlight language id.
	pub language: Option<String>,
	/// Zero-based cell index.
	pub index: Option<usize>,
	/// Cell count.
	pub total: Option<usize>,
	/// Header title.
	pub title: Option<String>,
	/// Lifecycle status.
	pub status: Option<CodeCellStatus>,
	/// Spinner frame.
	pub spinner_frame: Option<usize>,
	/// Elapsed milliseconds.
	pub duration: Option<u64>,
	/// Optional output stream.
	pub output: Option<String>,
	/// Row cap for the output section while collapsed.
	pub output_max_lines: Option<usize>,
	/// Row cap for the code preview while collapsed.
	pub code_max_lines: Option<usize>,
	/// Show the streaming tail instead of the head.
	pub code_tail: Option<bool>,
	/// Whether the cell is expanded.
	pub expanded: Option<bool>,
	/// Prefix the header with the language icon.
	pub show_language: Option<bool>,
	/// First line number.
	pub code_start_line: Option<usize>,
	/// Per-row line numbers.
	pub code_line_numbers: Option<Vec<Option<usize>>>,
}

/// Content and display limits for a Markdown preview with optional output.
#[derive(Clone, Debug)]
pub struct MarkdownCellOptions {
	/// Markdown source.
	pub content: String,
	/// Zero-based cell index.
	pub index: Option<usize>,
	/// Cell count.
	pub total: Option<usize>,
	/// Header title.
	pub title: Option<String>,
	/// Lifecycle status.
	pub status: Option<CodeCellStatus>,
	/// Spinner frame.
	pub spinner_frame: Option<usize>,
	/// Elapsed milliseconds.
	pub duration: Option<u64>,
	/// Optional output stream.
	pub output: Option<String>,
	/// Row cap for the output section while collapsed.
	pub output_max_lines: Option<usize>,
	/// Row cap for the markdown body while collapsed.
	pub content_max_lines: Option<usize>,
	/// Whether the cell is expanded.
	pub expanded: Option<bool>,
	/// Outer block width.
	pub width: usize,
}

/// [`MarkdownCellOptions`] without the render-only `width`, for
/// [`describe_markdown_cell`].
#[derive(Clone, Debug)]
pub struct MarkdownCellDescribeOptions {
	/// Markdown source.
	pub content: String,
	/// Zero-based cell index.
	pub index: Option<usize>,
	/// Cell count.
	pub total: Option<usize>,
	/// Header title.
	pub title: Option<String>,
	/// Lifecycle status.
	pub status: Option<CodeCellStatus>,
	/// Spinner frame.
	pub spinner_frame: Option<usize>,
	/// Elapsed milliseconds.
	pub duration: Option<u64>,
	/// Optional output stream.
	pub output: Option<String>,
	/// Row cap for the output section while collapsed.
	pub output_max_lines: Option<usize>,
	/// Row cap for the markdown body while collapsed.
	pub content_max_lines: Option<usize>,
	/// Whether the cell is expanded.
	pub expanded: Option<bool>,
}

/// The header fields shared by the ANSI and native cell forms.
#[derive(Clone, Debug, Default)]
struct CellHeaderOptions {
	index: Option<usize>,
	total: Option<usize>,
	title: Option<String>,
	duration: Option<u64>,
	language: Option<String>,
	show_language: Option<bool>,
}

/// A collapsible card preview clamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeOutputBlockPreview {
	/// Maximum body rows while collapsed.
	pub lines: usize,
}


/// One body section of a native output block.
#[derive(Clone, Debug, Default)]
pub struct NativeOutputBlockSection {
	/// Titled sub-section (the ANSI path's labelled separator bar).
	pub label: Option<NodeText>,
	/// Body nodes.
	pub body: Vec<NativeNode>,
	/// Divide from the previous section with a rule.
	pub separator: bool,
	/// Stable identity when sections come and go.
	pub key: Option<String>,
}

/// Semantic inputs for [`describe_output_block`].
#[derive(Clone, Debug, Default)]
pub struct NativeOutputBlockOptions {
	/// Header text.
	pub head: Option<NodeText>,
	/// Header meta, joined to the head with a dim `·`.
	pub meta: Option<NodeText>,
	/// Lifecycle state driving tone and status.
	pub state: Option<State>,
	/// Card role; defaults to `omp.output`.
	pub role: Option<String>,
	/// Override the state-derived tone.
	pub tone: Option<Tone>,
	/// Body sections.
	pub sections: Vec<NativeOutputBlockSection>,
	/// Whether the card can collapse.
	pub collapsible: Option<bool>,
	/// Whether the card is collapsed.
	pub collapsed: Option<bool>,
	/// Collapsed preview clamp.
	pub preview: Option<NativeOutputBlockPreview>,
	/// Flat card without a ring.
	pub inset: Option<bool>,
	/// Stable sibling identity.
	pub key: Option<String>,
}

/// Card tone for an output state.
pub fn output_state_tone(state: Option<State>) -> Tone {
	match state {
		Some(State::Pending) | Some(State::Running) => Tone::Pending,
		Some(State::Success) => Tone::Success,
		Some(State::Error) => Tone::Error,
		Some(State::Warning) => Tone::Warning,
		None => Tone::Neutral,
	}
}

/// Card status chip for an output state.
pub fn output_state_status(state: Option<State>) -> Option<&'static str> {
	match state {
		Some(State::Pending) => Some("pending"),
		Some(State::Running) => Some("running"),
		Some(State::Success) | Some(State::Warning) => Some("done"),
		Some(State::Error) => Some("error"),
		None => None,
	}
}

/// The native form of `renderOutputBlock`: a `card` whose tone and status chip
/// come from the state, head from the header plus meta, and body from the
/// sections (labelled ones as `section`s, separators as `rule`s).
pub fn describe_output_block(options: &NativeOutputBlockOptions) -> NativeNode {
	let mut head = options.head.clone();
	if let Some(meta) = &options.meta {
		let meta_empty = matches!(meta, NodeText::Plain(text) if text.is_empty());
		if !meta_empty {
			head = Some(match head {
				Some(head) => {
					let mut spans = text_to_spans(&head);
					spans.push(span(" · ", Some("dim")));
					spans.extend(text_to_spans(meta));
					NodeText::Spans(spans)
				}
				None => meta.clone(),
			});
		}
	}

	let mut children: Vec<NativeNode> = Vec::new();
	for (index, section) in options.sections.iter().enumerate() {
		let key = section.key.clone().unwrap_or_else(|| index.to_string());
		if let Some(label) = &section.label {
			let mut props = Props::new();
			props.set_head(label.clone());
			children.push(node(NodeKind::Section, props, section.body.clone(), Some(key.as_str())));
			continue;
		}
		if section.separator && index > 0 {
			let rule_key = format!("{key}:rule");
			children.push(node(
				NodeKind::Rule,
				Props::new(),
				Vec::new(),
				Some(rule_key.as_str()),
			));
		}
		children.extend(section.body.iter().cloned());
	}

	let mut props = Props::new();
	props.set_str("role", options.role.as_deref().unwrap_or("omp.output"));
	props.set_tone(options.tone.unwrap_or_else(|| output_state_tone(options.state)));
	if let Some(status) = output_state_status(options.state) {
		props.set_str("status", status);
	}
	if let Some(head) = head {
		props.set_head(head);
	}
	if let Some(collapsible) = options.collapsible {
		props.set_bool("collapsible", collapsible);
	}
	if let Some(collapsed) = options.collapsed {
		props.set_bool("collapsed", collapsed);
	}
	if let Some(preview) = options.preview {
		props.insert("preview", serde_json::json!({ "lines": preview.lines }));
	}
	if let Some(inset) = options.inset {
		props.set_bool("inset", inset);
	}
	node(NodeKind::Card, props, children, options.key.as_deref())
}

fn text_to_spans(text: &NodeText) -> Vec<Span> {
	match text {
		NodeText::Plain(text) => vec![span(text.clone(), None)],
		NodeText::Spans(spans) => spans.clone(),
	}
}

fn get_state(status: Option<CodeCellStatus>) -> Option<State> {
	match status {
		Some(CodeCellStatus::Complete) => Some(State::Success),
		Some(CodeCellStatus::Error) => Some(State::Error),
		Some(CodeCellStatus::Warning) => Some(State::Warning),
		Some(CodeCellStatus::Running) => Some(State::Running),
		Some(CodeCellStatus::Pending) => Some(State::Pending),
		None => None,
	}
}

fn status_ui(status: CodeCellStatus) -> ToolUiStatus {
	match status {
		CodeCellStatus::Complete => ToolUiStatus::Success,
		CodeCellStatus::Error => ToolUiStatus::Error,
		CodeCellStatus::Warning => ToolUiStatus::Warning,
		CodeCellStatus::Running => ToolUiStatus::Running,
		CodeCellStatus::Pending => ToolUiStatus::Pending,
	}
}

/// The language icon tinted with its brand color, or `None` when the active
/// symbol preset has no icon at all.
fn lang_icon(theme: &Theme, language: &str) -> Option<String> {
	theme
		.get_lang_icon_styled(Some(language))
		.ok()
		.filter(|icon| !icon.is_empty())
}

struct HeaderText {
	title: String,
	meta: Option<String>,
}

fn format_header(options: &CodeCellOptions, theme: &Theme) -> HeaderText {
	let mut parts: Vec<String> = Vec::new();
	if options.show_language == Some(true) {
		if let Some(language) = options.language.as_deref().filter(|language| !language.is_empty()) {
			if let Some(icon) = lang_icon(theme, language) {
				parts.push(icon);
			}
		}
	}
	if let Some(status) = options.status {
		let icon = format_status_icon(status_ui(status), theme, options.spinner_frame);
		if matches!(status, CodeCellStatus::Pending | CodeCellStatus::Running) {
			parts.push(format!("{icon} {}", paint(theme, "muted", status.as_str())));
		} else {
			parts.push(icon);
		}
	}
	if let (Some(index), Some(total)) = (options.index, options.total) {
		if total > 1 {
			parts.push(paint(theme, "accent", &format!("[{}/{}]", index + 1, total)));
		}
	}
	if let Some(title) = options.title.as_deref().filter(|title| !title.is_empty()) {
		parts.push(paint(theme, "toolTitle", title));
	}
	let title = if parts.is_empty() {
		paint(theme, "toolTitle", "Code")
	} else {
		parts.join(" ")
	};

	let mut meta_parts: Vec<String> = Vec::new();
	if let Some(duration) = options.duration {
		meta_parts.push(paint(theme, "dim", &format!("({})", format_duration(duration))));
	}
	let meta = if meta_parts.is_empty() {
		None
	} else {
		Some(meta_parts.join(&paint(theme, "dim", &theme.sep().dot)))
	};
	HeaderText { title, meta }
}

/// Style and cap the output section. Empty or whitespace-only output produces
/// no rows.
fn render_cell_output(
	output: Option<&str>,
	expanded: bool,
	output_max_lines: usize,
	theme: &Theme,
) -> Vec<String> {
	let Some(output) = output else {
		return Vec::new();
	};
	if output.trim().is_empty() {
		return Vec::new();
	}
	let lines = split_terminal_output_lines(output);
	let style_line = |line: &str, _index: usize| style_tool_output_line(line, theme);
	let format_hidden = |hidden: usize, _shown: usize, _total: usize, _edge: PaneEdge| {
		format_more_items(hidden, "line")
	};
	let options = OutputPaneFormatOptions {
		lines: &lines,
		expanded,
		collapsed_max_lines: output_max_lines,
		edge: Some(PaneEdge::Head),
		style_line: Some(&style_line),
		format_hidden: Some(&format_hidden),
		..OutputPaneFormatOptions::default()
	};
	format_output_pane_lines(&options, theme).lines
}

/// Plain-line fallback for `highlightCode`; see the module doc.
fn highlight_code(code: &str, _language: Option<&str>) -> Vec<String> {
	code.split('\n').map(str::to_string).collect()
}

/// Render a syntax-highlighted code preview and its optional output block.
pub fn render_code_cell(options: &CodeCellOptions, theme: &Theme) -> Vec<String> {
	let expanded = options.expanded.unwrap_or(false);
	let output_max_lines = options.output_max_lines.unwrap_or(6);
	let code_max_lines = options.code_max_lines.unwrap_or(12);
	let width = options.width;

	let header = format_header(options, theme);
	let state = get_state(options.status);

	let normalized_code = replace_tabs(&options.code);
	let raw_code_lines = split_terminal_output_lines(&normalized_code);
	let max_code_lines = if expanded {
		raw_code_lines.len()
	} else {
		raw_code_lines.len().min(code_max_lines)
	};
	let hidden_code_lines = raw_code_lines.len() - max_code_lines;
	let tail = options.code_tail == Some(true) && !expanded && hidden_code_lines > 0;
	let start_index = if tail {
		raw_code_lines.len() - max_code_lines
	} else {
		0
	};
	let visible_code = raw_code_lines[start_index..start_index + max_code_lines].join("\n");
	let code_lines = highlight_code(&visible_code, options.language.as_deref());

	let mut visible_line_numbers: Option<Vec<Option<usize>>> = None;
	if let Some(code_line_numbers) = &options.code_line_numbers {
		let start = start_index.min(code_line_numbers.len());
		let end = (start_index + max_code_lines).min(code_line_numbers.len());
		visible_line_numbers = Some(code_line_numbers[start..end].to_vec());
	} else if let Some(code_start_line) = options.code_start_line {
		visible_line_numbers = Some(
			(0..max_code_lines)
				.map(|offset| Some(code_start_line + start_index + offset))
				.collect(),
		);
	}

	let mut line_number_width = 0usize;
	if let Some(visible) = &visible_line_numbers {
		let max_val = visible.iter().flatten().copied().max().unwrap_or(0);
		if max_val > 0 {
			line_number_width = max_val.to_string().len().max(2);
		}
	}

	let mut numbered_code_lines: Vec<String> = Vec::new();
	if line_number_width > 0 {
		if let Some(visible) = &visible_line_numbers {
			for (index, code_line) in code_lines.iter().enumerate() {
				let line_num = visible.get(index).copied().flatten();
				let gutter = match line_num {
					Some(number) => format!("{number:>width$}", width = line_number_width),
					None => " ".repeat(line_number_width),
				};
				let prefix = paint(theme, "dim", &format!("{gutter} "));
				let continuation = " ".repeat(line_number_width + 1);
				numbered_code_lines.extend(wrap_prefixed_line(
					&prefix,
					&continuation,
					code_line,
					output_block_content_width(width, None, None),
				));
			}
		}
	}

	let mut display_code_lines = if line_number_width > 0 {
		numbered_code_lines
	} else {
		code_lines
	};
	if hidden_code_lines > 0 {
		let hint = format_expand_hint(theme, Some(expanded), Some(hidden_code_lines > 0));
		let gutter_pad = if line_number_width > 0 {
			" ".repeat(line_number_width + 1)
		} else {
			String::new()
		};
		if tail {
			let plural = if hidden_code_lines == 1 { "" } else { "s" };
			let earlier = if hint.is_empty() {
				format!("… {hidden_code_lines} earlier line{plural}")
			} else {
				format!("… {hidden_code_lines} earlier line{plural} {hint}")
			};
			display_code_lines.insert(0, paint(theme, "dim", &format!("{gutter_pad}{earlier}")));
		} else {
			let more_line = if hint.is_empty() {
				format_more_items(hidden_code_lines, "line")
			} else {
				format!("{} {hint}", format_more_items(hidden_code_lines, "line"))
			};
			display_code_lines.push(paint(theme, "dim", &format!("{gutter_pad}{more_line}")));
		}
	}

	let output_lines = render_cell_output(options.output.as_deref(), expanded, output_max_lines, theme);

	let mut sections: Vec<OutputSection> = vec![OutputSection {
		lines: display_code_lines,
		..OutputSection::default()
	}];
	if !output_lines.is_empty() {
		sections.push(OutputSection {
			label: Some(paint(theme, "toolTitle", "Output")),
			lines: output_lines,
			..OutputSection::default()
		});
	}

	let block_options = OutputBlockOptions {
		header: header.title,
		header_meta: header.meta.unwrap_or_default(),
		state,
		sections,
		width,
		..OutputBlockOptions::default()
	};
	render_output_block(&block_options, theme)
}

/// Render a Markdown preview and its optional output block.
pub fn render_markdown_cell(options: &MarkdownCellOptions, theme: &Theme) -> Vec<String> {
	let expanded = options.expanded.unwrap_or(false);
	let output_max_lines = options.output_max_lines.unwrap_or(6);
	let content_max_lines = options.content_max_lines.unwrap_or(12);
	let width = options.width;

	let code_options = CodeCellOptions {
		code: String::new(),
		language: None,
		index: options.index,
		total: options.total,
		title: options.title.clone(),
		status: options.status,
		spinner_frame: options.spinner_frame,
		duration: options.duration,
		output: None,
		output_max_lines: None,
		code_max_lines: None,
		code_tail: None,
		expanded: None,
		show_language: None,
		width,
		code_start_line: None,
		code_line_numbers: None,
	};
	let header = format_header(&code_options, theme);
	let state = get_state(options.status);

	// Markdown manages its own wrapping at the same inner width as
	// `renderOutputBlock`, so collapsed row caps are applied after wrapping.
	let inner_width = output_block_content_width(width, None, None).max(20);
	let all_lines = if options.content.trim().is_empty() {
		Vec::new()
	} else {
		let mut markdown = Markdown::new();
		markdown.set_text(options.content.clone());
		markdown.set_padding(0, 0);
		markdown.render(inner_width as u16)
	};
	let max_content_lines = if expanded {
		all_lines.len()
	} else {
		all_lines.len().min(content_max_lines)
	};
	let mut content_lines = all_lines[..max_content_lines].to_vec();
	let hidden_content_lines = all_lines.len() - max_content_lines;
	if hidden_content_lines > 0 {
		let hint = format_expand_hint(theme, Some(expanded), Some(hidden_content_lines > 0));
		let more_line = if hint.is_empty() {
			format_more_items(hidden_content_lines, "line")
		} else {
			format!("{} {hint}", format_more_items(hidden_content_lines, "line"))
		};
		content_lines.push(paint(theme, "dim", &more_line));
	}

	let output_lines = render_cell_output(options.output.as_deref(), expanded, output_max_lines, theme);

	let mut sections: Vec<OutputSection> = vec![OutputSection {
		lines: content_lines,
		..OutputSection::default()
	}];
	if !output_lines.is_empty() {
		sections.push(OutputSection {
			label: Some(paint(theme, "toolTitle", "Output")),
			lines: output_lines,
			..OutputSection::default()
		});
	}

	let block_options = OutputBlockOptions {
		header: header.title,
		header_meta: header.meta.unwrap_or_default(),
		state,
		sections,
		width,
		..OutputBlockOptions::default()
	};
	render_output_block(&block_options, theme)
}

/// Header spans for a native cell: language icon, `[i/n]`, title. Status is the
/// card's chip.
fn describe_header(
	options: &CellHeaderOptions,
	theme: &Theme,
) -> (Vec<Span>, Option<Vec<Span>>) {
	let mut head: Vec<Span> = Vec::new();
	let mut push = |part: Vec<Span>| {
		if !head.is_empty() {
			head.push(span(" ", None));
		}
		head.extend(part);
	};
	if options.show_language == Some(true) {
		if let Some(language) = options.language.as_deref().filter(|language| !language.is_empty()) {
			if let Some(icon) = lang_icon(theme, language) {
				push(styled_spans(&icon, theme));
			}
		}
	}
	if let (Some(index), Some(total)) = (options.index, options.total) {
		if total > 1 {
			push(vec![span(format!("[{}/{}]", index + 1, total), Some("accent"))]);
		}
	}
	if let Some(title) = options.title.as_deref().filter(|title| !title.is_empty()) {
		push(vec![span(title.to_string(), Some("toolTitle"))]);
	}
	if head.is_empty() {
		head.push(span("Code", Some("toolTitle")));
	}
	let meta = options
		.duration
		.map(|duration| vec![span(format_duration(duration), Some("dim"))]);
	(head, meta)
}

fn describe_cell_output(
	output: Option<&str>,
	expanded: bool,
	output_max_lines: usize,
) -> Option<NativeOutputBlockSection> {
	let output = output?;
	if output.trim().is_empty() {
		return None;
	}
	Some(NativeOutputBlockSection {
		label: Some(NodeText::Spans(vec![span("Output", Some("toolTitle"))])),
		body: vec![describe_output_lines(
			&split_terminal_output_lines(output),
			&NativeOutputOptions {
				expanded,
				collapsed_max_lines: output_max_lines,
				..NativeOutputOptions::default()
			},
		)],
		separator: false,
		key: Some("output".to_string()),
	})
}

/// The native form of [`render_code_cell`]: a collapsible `card` holding a
/// `code` node (numbered from `code_start_line`, or the first explicit line
/// number) and an `ansi` output section. The collapsed code clamp is the card
/// preview; highlighting and gutters are the terminal's.
pub fn describe_code_cell(options: &CodeCellDescribeOptions, theme: &Theme) -> NativeNode {
	let expanded = options.expanded.unwrap_or(false);
	let output_max_lines = options.output_max_lines.unwrap_or(6);
	let code_max_lines = options.code_max_lines.unwrap_or(12);

	let header_options = CellHeaderOptions {
		index: options.index,
		total: options.total,
		title: options.title.clone(),
		duration: options.duration,
		language: options.language.clone(),
		show_language: options.show_language,
	};
	let (head, meta) = describe_header(&header_options, theme);

	let start = options.code_start_line.or_else(|| {
		options
			.code_line_numbers
			.as_ref()
			.and_then(|numbers| numbers.iter().copied().flatten().next())
	});

	let mut code_props = Props::new();
	if let Some(language) = &options.language {
		code_props.set_str("lang", language.as_str());
	}
	if let Some(start) = start {
		code_props.set_i64("start", start as i64);
	}
	if start.is_some() {
		code_props.set_bool("numbers", true);
	}
	code_props.set_bool("wrap", true);

	let mut sections: Vec<NativeOutputBlockSection> = vec![NativeOutputBlockSection {
		key: Some("code".to_string()),
		body: vec![code(options.code.clone(), code_props)],
		label: None,
		separator: false,
	}];
	if let Some(output) = describe_cell_output(options.output.as_deref(), expanded, output_max_lines) {
		sections.push(output);
	}

	let block_options = NativeOutputBlockOptions {
		head: Some(NodeText::Spans(head)),
		meta: meta.map(NodeText::Spans),
		state: get_state(options.status),
		role: Some("omp.cell.code".to_string()),
		sections,
		collapsible: Some(true),
		collapsed: Some(!expanded),
		preview: Some(NativeOutputBlockPreview { lines: code_max_lines }),
		..NativeOutputBlockOptions::default()
	};
	describe_output_block(&block_options)
}

/// The native form of [`render_markdown_cell`]: a collapsible `card` holding
/// `md` source and optional output.
pub fn describe_markdown_cell(
	options: &MarkdownCellDescribeOptions,
	theme: &Theme,
) -> NativeNode {
	let expanded = options.expanded.unwrap_or(false);
	let output_max_lines = options.output_max_lines.unwrap_or(6);
	let content_max_lines = options.content_max_lines.unwrap_or(12);

	let header_options = CellHeaderOptions {
		index: options.index,
		total: options.total,
		title: options.title.clone(),
		duration: options.duration,
		language: None,
		show_language: None,
	};
	let (head, meta) = describe_header(&header_options, theme);

	let mut sections: Vec<NativeOutputBlockSection> = Vec::new();
	if !options.content.trim().is_empty() {
		sections.push(NativeOutputBlockSection {
			key: Some("content".to_string()),
			body: vec![md(options.content.clone(), Props::new())],
			label: None,
			separator: false,
		});
	}
	if let Some(output) = describe_cell_output(options.output.as_deref(), expanded, output_max_lines) {
		sections.push(output);
	}

	let block_options = NativeOutputBlockOptions {
		head: Some(NodeText::Spans(head)),
		meta: meta.map(NodeText::Spans),
		state: get_state(options.status),
		role: Some("omp.cell.markdown".to_string()),
		sections,
		collapsible: Some(true),
		collapsed: Some(!expanded),
		preview: Some(NativeOutputBlockPreview { lines: content_max_lines }),
		..NativeOutputBlockOptions::default()
	};
	describe_output_block(&block_options)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::{builtin, create_theme, CreateThemeOptions};

	fn test_theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("embedded dark theme exists"),
			CreateThemeOptions::default(),
		)
		.expect("embedded dark theme resolves")
	}

	fn code_options(code: &str) -> CodeCellOptions {
		CodeCellOptions {
			code: code.to_string(),
			language: None,
			index: None,
			total: None,
			title: None,
			status: None,
			spinner_frame: None,
			duration: None,
			output: None,
			output_max_lines: None,
			code_max_lines: None,
			code_tail: None,
			expanded: None,
			show_language: None,
			width: 60,
			code_start_line: None,
			code_line_numbers: None,
		}
	}

	fn markdown_options(content: &str) -> MarkdownCellOptions {
		MarkdownCellOptions {
			content: content.to_string(),
			index: None,
			total: None,
			title: None,
			status: None,
			spinner_frame: None,
			duration: None,
			output: None,
			output_max_lines: None,
			content_max_lines: None,
			expanded: None,
			width: 60,
		}
	}

	#[test]
	fn state_maps_every_status() {
		let cases = [
			(CodeCellStatus::Complete, State::Success),
			(CodeCellStatus::Error, State::Error),
			(CodeCellStatus::Warning, State::Warning),
			(CodeCellStatus::Running, State::Running),
			(CodeCellStatus::Pending, State::Pending),
		];
		for (status, expected) in cases {
			assert_eq!(get_state(Some(status)), Some(expected));
		}
		assert_eq!(get_state(None), None);
	}

	#[test]
	fn output_state_tone_and_status_cover_every_state() {
		let tone_cases = [
			(Some(State::Pending), Tone::Pending),
			(Some(State::Running), Tone::Pending),
			(Some(State::Success), Tone::Success),
			(Some(State::Error), Tone::Error),
			(Some(State::Warning), Tone::Warning),
			(None, Tone::Neutral),
		];
		for (state, expected) in tone_cases {
			assert_eq!(output_state_tone(state), expected);
		}
		let status_cases = [
			(Some(State::Pending), Some("pending")),
			(Some(State::Running), Some("running")),
			(Some(State::Success), Some("done")),
			(Some(State::Warning), Some("done")),
			(Some(State::Error), Some("error")),
			(None, None),
		];
		for (state, expected) in status_cases {
			assert_eq!(output_state_status(state), expected);
		}
	}

	#[test]
	fn header_falls_back_to_code_and_joins_parts() {
		let theme = test_theme();
		let bare = format_header(&code_options(""), &theme);
		assert_eq!(bare.title, paint(&theme, "toolTitle", "Code"));
		assert_eq!(bare.meta, None);

		let mut options = code_options("");
		options.index = Some(1);
		options.total = Some(3);
		options.title = Some("Demo".to_string());
		options.duration = Some(1500);
		options.status = Some(CodeCellStatus::Complete);
		let header = format_header(&options, &theme);
		assert!(header.title.contains("[2/3]"), "title: {}", header.title);
		assert!(header.title.contains("Demo"), "title: {}", header.title);
		assert!(header.meta.is_some());
		assert!(header.meta.unwrap().contains("1.5s"));
	}

	#[test]
	fn header_skips_single_cell_index_and_empty_title() {
		let theme = test_theme();
		let mut options = code_options("");
		options.index = Some(0);
		options.total = Some(1);
		options.title = Some(String::new());
		let header = format_header(&options, &theme);
		assert_eq!(header.title, paint(&theme, "toolTitle", "Code"));
	}

	#[test]
	fn header_labels_pending_and_running_status() {
		let theme = test_theme();
		for (status, label) in [
			(CodeCellStatus::Pending, "pending"),
			(CodeCellStatus::Running, "running"),
		] {
			let mut options = code_options("");
			options.status = Some(status);
			let header = format_header(&options, &theme);
			assert!(header.title.contains(label), "title: {}", header.title);
		}
	}

	#[test]
	fn header_shows_the_language_icon_when_asked() {
		let theme = test_theme();
		let mut options = code_options("");
		options.show_language = Some(true);
		options.language = Some("rust".to_string());
		let icon = lang_icon(&theme, "rust").expect("rust icon resolves");
		let with_icon = format_header(&options, &theme);
		assert!(with_icon.title.contains(&icon), "title: {}", with_icon.title);
		assert_ne!(with_icon.title, paint(&theme, "toolTitle", "Code"));

		let mut without = code_options("");
		without.language = Some("rust".to_string());
		let header = format_header(&without, &theme);
		assert_eq!(header.title, paint(&theme, "toolTitle", "Code"));
	}

	#[test]
	fn render_code_cell_caps_collapsed_and_marks_hidden_rows() {
		let theme = test_theme();
		let mut options = code_options("a\nb\nc");
		options.code_max_lines = Some(2);
		let rows = render_code_cell(&options, &theme);
		assert!(
			rows.iter().any(|row| row.contains("1 more line")),
			"rows: {rows:?}"
		);

		options.expanded = Some(true);
		let rows = render_code_cell(&options, &theme);
		assert!(
			!rows.iter().any(|row| row.contains("more line")),
			"rows: {rows:?}"
		);
	}

	#[test]
	fn render_code_cell_tail_marks_earlier_rows_on_top() {
		let theme = test_theme();
		let mut options = code_options("a\nb\nc");
		options.code_max_lines = Some(2);
		options.code_tail = Some(true);
		let rows = render_code_cell(&options, &theme);
		let marker = rows
			.iter()
			.find(|row| row.contains("earlier line"))
			.expect("tail marker");
		assert!(marker.contains("1 earlier line"), "marker: {marker}");
	}

	#[test]
	fn render_code_cell_draws_line_number_gutters() {
		let theme = test_theme();
		let mut options = code_options("a\nb");
		options.code_start_line = Some(10);
		let rows = render_code_cell(&options, &theme);
		assert!(
			rows.iter().any(|row| row.contains("10") && row.contains('a')),
			"rows: {rows:?}"
		);
		assert!(
			rows.iter().any(|row| row.contains("11") && row.contains('b')),
			"rows: {rows:?}"
		);

		let mut blank_gutter = code_options("a\nb");
		blank_gutter.code_line_numbers = Some(vec![None, Some(5)]);
		let rows = render_code_cell(&blank_gutter, &theme);
		assert!(
			rows.iter().any(|row| row.contains('b') && row.contains('5')),
			"rows: {rows:?}"
		);
	}

	#[test]
	fn render_code_cell_adds_a_labelled_output_section() {
		let theme = test_theme();
		let mut options = code_options("a");
		options.output = Some("x\ny".to_string());
		options.output_max_lines = Some(1);
		let rows = render_code_cell(&options, &theme);
		assert!(rows.iter().any(|row| row.contains("Output")), "rows: {rows:?}");
		assert!(rows.iter().any(|row| row.contains("1 more line")), "rows: {rows:?}");
	}

	#[test]
	fn render_markdown_cell_wraps_and_caps() {
		let theme = test_theme();
		let mut options = markdown_options("# Title\n\nbody");
		options.content_max_lines = Some(1);
		let rows = render_markdown_cell(&options, &theme);
		assert!(rows.iter().any(|row| row.contains("2 more lines")), "rows: {rows:?}");

		options.expanded = Some(true);
		let rows = render_markdown_cell(&options, &theme);
		assert!(
			!rows.iter().any(|row| row.contains("more lines")),
			"rows: {rows:?}"
		);
	}

	#[test]
	fn render_markdown_cell_omits_blank_content() {
		let theme = test_theme();
		let options = markdown_options("   \n  ");
		let rows = render_markdown_cell(&options, &theme);
		assert!(!rows.iter().any(|row| row.contains("more lines")));
	}

	#[test]
	fn describe_code_cell_builds_a_collapsible_card() {
		let theme = test_theme();
		let options = CodeCellDescribeOptions {
			code: "fn main() {}".to_string(),
			language: Some("rust".to_string()),
			index: Some(0),
			total: Some(2),
			title: Some("Demo".to_string()),
			status: Some(CodeCellStatus::Complete),
			spinner_frame: None,
			duration: Some(1500),
			output: Some("hello".to_string()),
			output_max_lines: Some(6),
			code_max_lines: Some(12),
			code_tail: None,
			expanded: Some(false),
			show_language: None,
			code_start_line: Some(3),
			code_line_numbers: None,
		};
		let described = describe_code_cell(&options, &theme);
		assert_eq!(described.kind, NodeKind::Card);
		assert_eq!(described.props.get_str("role"), Some("omp.cell.code"));
		assert_eq!(described.props.get_str("status"), Some("done"));
		assert_eq!(described.props.get_bool("collapsible"), Some(true));
		assert_eq!(described.props.get_bool("collapsed"), Some(true));
		assert_eq!(
			described
				.props
				.get("preview")
				.and_then(|value| value.get("lines"))
				.and_then(serde_json::Value::as_i64),
			Some(12)
		);
		let head = described.props.head().expect("head");
		assert!(head.plain_text().contains("Demo"));
		assert!(head.plain_text().contains("1.5s"));

		let code = described
			.children
			.iter()
			.find(|child| child.kind == NodeKind::Code)
			.expect("code child");
		assert_eq!(code.props.text(), Some("fn main() {}"));
		assert_eq!(code.props.get_str("lang"), Some("rust"));
		assert_eq!(
			code.props.get("start").and_then(serde_json::Value::as_i64),
			Some(3)
		);
		assert_eq!(code.props.get_bool("numbers"), Some(true));
		assert_eq!(code.props.get_bool("wrap"), Some(true));

		let output = described
			.children
			.iter()
			.find(|child| child.kind == NodeKind::Section)
			.expect("output section");
		assert_eq!(output.key.as_deref(), Some("output"));
		assert_eq!(output.props.head().expect("section head").plain_text(), "Output");
		assert_eq!(output.children.len(), 1);
		assert_eq!(output.children[0].kind, NodeKind::Ansi);
		assert_eq!(output.children[0].props.text(), Some("hello"));
	}

	#[test]
	fn describe_code_cell_numbers_from_the_first_explicit_line() {
		let theme = test_theme();
		let options = CodeCellDescribeOptions {
			code: "a\nb".to_string(),
			language: None,
			index: None,
			total: None,
			title: None,
			status: None,
			spinner_frame: None,
			duration: None,
			output: None,
			output_max_lines: None,
			code_max_lines: None,
			code_tail: None,
			expanded: None,
			show_language: None,
			code_start_line: None,
			code_line_numbers: Some(vec![None, Some(7)]),
		};
		let described = describe_code_cell(&options, &theme);
		let code = described
			.children
			.iter()
			.find(|child| child.kind == NodeKind::Code)
			.expect("code child");
		assert_eq!(
			code.props.get("start").and_then(serde_json::Value::as_i64),
			Some(7)
		);
		assert_eq!(code.props.get_bool("numbers"), Some(true));
		assert_eq!(code.props.get_str("lang"), None);
	}

	#[test]
	fn describe_markdown_cell_includes_content_only_when_present() {
		let theme = test_theme();
		let options = MarkdownCellDescribeOptions {
			content: "hello".to_string(),
			index: None,
			total: None,
			title: None,
			status: None,
			spinner_frame: None,
			duration: None,
			output: None,
			output_max_lines: None,
			content_max_lines: Some(5),
			expanded: Some(true),
		};
		let described = describe_markdown_cell(&options, &theme);
		assert_eq!(described.kind, NodeKind::Card);
		assert_eq!(described.props.get_str("role"), Some("omp.cell.markdown"));
		assert_eq!(described.props.get_bool("collapsed"), Some(false));
		assert_eq!(
			described
				.props
				.get("preview")
				.and_then(|value| value.get("lines"))
				.and_then(serde_json::Value::as_i64),
			Some(5)
		);
		let md = described
			.children
			.iter()
			.find(|child| child.kind == NodeKind::Md)
			.expect("md child");
		assert_eq!(md.props.text(), Some("hello"));

		let blank = MarkdownCellDescribeOptions {
			content: "   ".to_string(),
			..options
		};
		let described = describe_markdown_cell(&blank, &theme);
		assert!(
			!described
				.children
				.iter()
				.any(|child| child.kind == NodeKind::Md)
		);
	}

	#[test]
	fn describe_output_block_merges_meta_and_draws_separators() {
		let options = NativeOutputBlockOptions {
			head: Some(NodeText::Plain("Title".to_string())),
			meta: Some(NodeText::Plain("Meta".to_string())),
			state: Some(State::Success),
			sections: vec![
				NativeOutputBlockSection {
					label: Some(NodeText::Plain("Section".to_string())),
					body: vec![md("body", Props::new())],
					separator: false,
					key: Some("s1".to_string()),
				},
				NativeOutputBlockSection {
					label: None,
					body: vec![md("plain", Props::new())],
					separator: true,
					key: Some("s2".to_string()),
				},
			],
			..NativeOutputBlockOptions::default()
		};
		let described = describe_output_block(&options);
		assert_eq!(described.kind, NodeKind::Card);
		assert_eq!(described.props.get_str("role"), Some("omp.output"));
		assert_eq!(described.props.get_str("tone"), Some("success"));
		assert_eq!(described.props.get_str("status"), Some("done"));
		assert_eq!(
			described.props.head().expect("head").plain_text(),
			"Title · Meta"
		);
		assert_eq!(described.children.len(), 3);
		assert_eq!(described.children[0].kind, NodeKind::Section);
		assert_eq!(described.children[1].kind, NodeKind::Rule);
		assert_eq!(described.children[1].key.as_deref(), Some("s2:rule"));
		assert_eq!(described.children[2].kind, NodeKind::Md);
	}

	#[test]
	fn describe_output_block_tone_override_wins() {
		let options = NativeOutputBlockOptions {
			tone: Some(Tone::Accent),
			state: Some(State::Error),
			..NativeOutputBlockOptions::default()
		};
		let described = describe_output_block(&options);
		assert_eq!(described.props.get_str("tone"), Some("accent"));
		assert_eq!(described.props.get_str("status"), Some("error"));
	}

	#[test]
	fn describe_output_block_skips_empty_meta() {
		let options = NativeOutputBlockOptions {
			head: Some(NodeText::Plain("Title".to_string())),
			meta: Some(NodeText::Plain(String::new())),
			..NativeOutputBlockOptions::default()
		};
		let described = describe_output_block(&options);
		assert_eq!(
			described.props.head().expect("head").plain_text(),
			"Title"
		);
	}
}
