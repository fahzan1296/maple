//! Bounded output pane: carriage-return-safe line splitting, the shared
//! capping/styling algorithm, the native `ansi` description, and the stateful
//! [`OutputPane`] viewport.
//!
//! Reference: `l/src/render/output-pane.ts`.
//!
//! Deviations forced by Rust:
//!
//! - `OutputPaneEdge` is the existing [`PaneEdge`] enum from
//!   [`crate::render::render_utils`], re-exported under the reference's name.
//! - `splitTerminalOutputLines` is defined here, matching the reference's
//!   `/\r?\n/` split (the `render_utils` helper splits on `\n` only and drops
//!   the CR, which differs for CRLF input). `styleToolOutputLine` already
//!   lives in [`crate::render::render_utils`] and is re-exported for the
//!   reference's import path.
//! - The reference's `getThemeEpoch()` (`l/src/theme/theme.ts`) and
//!   `getWidthConfigEpoch()` (`l/src/utils.ts`) memo dependencies have no crate
//!   equivalent. The memo keys on the runtime Hangul-Jamo width configuration
//!   (`terminal().hangul_jamo_width`), a constant theme epoch (a pane holds a
//!   fixed [`Theme`]; callers replace the pane to change theme), the runtime
//!   image protocol, and the resolved content width.
//! - `TERMINAL.imageProtocol` is `terminal().image_protocol`.
//! - `Text.releaseRenderCaches()` does not exist; [`OutputPane::release_render_caches`]
//!   clears the text and calls `Component::invalidate`.

use std::borrow::Cow;
use std::cell::RefCell;

use serde_json::Value;

use crate::chrome::visual_truncate::truncate_to_visual_lines;
use crate::component::Component;
use crate::components::text::Text;
use crate::native::describe::{ansi, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::native::state::Memo;
use crate::render::render_utils::{
	default_hidden_label, format_expand_hint, get_padding_x, get_sixel_line_mask, PaneEdge,
};
use crate::terminal_capabilities::{terminal, ImageProtocol};
use crate::theme::Theme;

pub use crate::render::render_utils::style_tool_output_line;

/// Which side of an output stream remains visible when the pane is capped.
pub type OutputPaneEdge = PaneEdge;

/// Split terminal text without allowing carriage-return progress updates to
/// corrupt following TUI rows. The final segment after a bare CR wins,
/// matching a terminal cursor-return overwrite. CRLF and LF both separate
/// rows.
pub fn split_terminal_output_lines(text: &str) -> Vec<String> {
	let mut rows: Vec<String> = Vec::new();
	let mut start = 0usize;
	let bytes = text.as_bytes();
	let mut index = 0usize;
	while index < bytes.len() {
		if bytes[index] == b'\n' {
			let mut end = index;
			if end > start && bytes[end - 1] == b'\r' {
				end -= 1;
			}
			rows.push(segment_after_cr(&text[start..end]));
			start = index + 1;
		}
		index += 1;
}
	rows.push(segment_after_cr(&text[start..]));
	rows
}

/// The substring after the final bare CR, or the whole segment.
fn segment_after_cr(segment: &str) -> String {
	match segment.rfind('\r') {
		Some(index) => segment[index + 1..].to_string(),
		None => segment.to_string(),
	}
}

/// Inputs for the shared output capping and styling algorithm.
#[derive(Default)]
pub struct OutputPaneFormatOptions<'a> {
	/// Logical rows to cap and style.
	pub lines: &'a [String],
	/// Whether the expanded (`expandedMaxLines`) cap applies.
	pub expanded: bool,
	/// Row cap while collapsed.
	pub collapsed_max_lines: usize,
	/// Optional row cap while expanded.
	pub expanded_max_lines: Option<usize>,
	/// Which edge survives capping; defaults to [`PaneEdge::Head`].
	pub edge: Option<PaneEdge>,
	/// Apply the cap after terminal-width wrapping instead of to logical rows.
	pub visual: bool,
	/// Content width used by visual capping.
	pub width: Option<usize>,
	/// Per-row styling hook.
	pub style_line: Option<&'a dyn Fn(&str, usize) -> String>,
	/// Keep sixel payload rows byte-for-byte and show the complete payload.
	pub uncap_sixel: bool,
	/// Set to `Some(false)` to suppress the hidden-row marker.
	pub show_hidden_marker: Option<bool>,
	/// Set to `Some(false)` to suppress the expand hint on the marker.
	pub show_expand_hint: Option<bool>,
	/// Show the expand hint even when nothing was capped.
	pub show_expand_hint_when_uncapped: Option<bool>,
	/// Hidden-row label hook.
	pub format_hidden: Option<&'a dyn Fn(usize, usize, usize, PaneEdge) -> String>,
}

/// Result of formatting one bounded output pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputPaneFormatResult {
	/// The visible rows, including any hidden-row marker.
	pub lines: Vec<String>,
	/// How many logical rows were hidden.
	pub hidden_count: usize,
	/// Whether any row carried a sixel payload.
	pub has_sixel: bool,
}

/// Style and cap output rows for code cells, tool cards, and live execution
/// panes. Sixel rows are never styled or split; with `uncapSixel`, the complete
/// payload is retained so terminal image protocols remain valid.
pub fn format_output_pane_lines(
	options: &OutputPaneFormatOptions<'_>,
	theme: &Theme,
) -> OutputPaneFormatResult {
	format_output_pane_lines_with_protocol(options, theme, terminal().image_protocol)
}

/// The protocol-injected core of [`format_output_pane_lines`], split out so
/// tests can exercise the sixel branch without touching the process-wide
/// terminal singleton.
fn format_output_pane_lines_with_protocol(
	options: &OutputPaneFormatOptions<'_>,
	theme: &Theme,
	image_protocol: Option<ImageProtocol>,
) -> OutputPaneFormatResult {
	let edge = options.edge.unwrap_or(PaneEdge::Head);
	let raw_lines = options.lines;
	let sixel_mask = if image_protocol == Some(ImageProtocol::Sixel) && !raw_lines.is_empty() {
		Some(get_sixel_line_mask(raw_lines))
	} else {
		None
	};
	let has_sixel = sixel_mask
		.as_ref()
		.is_some_and(|mask| mask.iter().any(|flag| *flag));
	let styled_lines: Vec<String> = raw_lines
		.iter()
		.enumerate()
		.map(|(index, line)| {
			let is_sixel = sixel_mask
				.as_ref()
				.and_then(|mask| mask.get(index))
				.copied()
				.unwrap_or(false);
			if is_sixel {
				line.clone()
			} else {
				options
					.style_line
					.map(|style| style(line, index))
					.unwrap_or_else(|| line.clone())
			}
		})
		.collect();

	let configured_limit = if options.expanded {
		options.expanded_max_lines
	} else {
		Some(options.collapsed_max_lines)
	};
	let limit = if has_sixel && options.uncap_sixel {
		None
	} else {
		configured_limit
	};
	let render_width = options.width.unwrap_or(1).max(1) as u16;
	let mut visible: Cow<'_, [String]> = Cow::Borrowed(&styled_lines);
	let mut hidden_count = 0usize;

	if let Some(bounded_limit) = limit {
		if bounded_limit == 0 {
			visible = Cow::Owned(Vec::new());
			hidden_count = styled_lines.len();
		} else if options.visual && edge == PaneEdge::Tail {
			let visual = truncate_to_visual_lines(
				&styled_lines.join("\n"),
				bounded_limit,
				render_width,
				0,
			);
			hidden_count = visual.skipped_count;
			visible = Cow::Owned(visual.visual_lines);
		} else if options.visual {
			let joined = styled_lines.join("\n");
			let rendered = Text::padded(joined.as_str(), 0, 0).render(render_width);
			let kept = rendered.len().min(bounded_limit);
			hidden_count = rendered.len() - kept;
			visible = Cow::Owned(rendered[..kept].to_vec());
		} else if styled_lines.len() > bounded_limit {
			hidden_count = styled_lines.len() - bounded_limit;
			visible = if edge == PaneEdge::Tail {
				Cow::Borrowed(&styled_lines[styled_lines.len() - bounded_limit..])
			} else {
				Cow::Borrowed(&styled_lines[..bounded_limit])
			};
		}
	}

	let mut lines: Vec<String> = visible.iter().cloned().collect();
	if hidden_count > 0 && options.show_hidden_marker != Some(false) {
		let label = match options.format_hidden {
			Some(format) => format(
				hidden_count,
				visible.len(),
				hidden_count + visible.len(),
				edge,
			),
			None => default_hidden_label(
				hidden_count,
				visible.len(),
				hidden_count + visible.len(),
				edge,
			),
		};
		let hint = if options.show_expand_hint == Some(false) {
			String::new()
		} else {
			format_expand_hint(theme, Some(options.expanded), Some(true))
		};
		let marker = if hint.is_empty() {
			crate::render::render_utils::paint(theme, "dim", &label)
		} else {
			crate::render::render_utils::paint(theme, "dim", &format!("{label} {hint}"))
		};
		if edge == PaneEdge::Tail {
			lines.insert(0, marker);
		} else {
			lines.push(marker);
		}
	} else if !options.expanded && options.show_expand_hint_when_uncapped == Some(true) {
		let hint = format_expand_hint(theme, Some(false), Some(true));
		if !hint.is_empty() {
			lines.push(hint);
		}
	}

	OutputPaneFormatResult {
		lines,
		hidden_count,
		has_sixel,
	}
}

/// Display policy for [`describe_output_lines`].
#[derive(Clone, Debug, Default)]
pub struct NativeOutputOptions {
	/// Whether the expanded cap applies.
	pub expanded: bool,
	/// Row cap while collapsed.
	pub collapsed_max_lines: usize,
	/// Optional row cap while expanded.
	pub expanded_max_lines: Option<usize>,
	/// Which edge follows the stream; [`PaneEdge::Tail`] sets `follow`.
	pub edge: Option<PaneEdge>,
	/// Stable sibling identity.
	pub key: Option<String>,
}

/// Raw output rows as a native `ansi` mini-terminal: the terminal wraps and
/// styles them. The row cap becomes a `preview` clamp (with its own "N more"
/// affordance); a tail edge follows the stream.
pub fn describe_output_lines(lines: &[String], options: &NativeOutputOptions) -> NativeNode {
	let limit = if options.expanded {
		options.expanded_max_lines
	} else {
		Some(options.collapsed_max_lines)
	};
	let mut props = Props::new();
	if options.edge == Some(PaneEdge::Tail) {
		props.set_bool("follow", true);
	}
	if let Some(limit) = limit {
		props.insert("preview", serde_json::json!({ "lines": limit }));
	}
	let mut described = ansi(lines.join("\n"), props);
	if let Some(key) = &options.key {
		described.key = Some(key.clone());
	}
	described
}

/// Mutable options for a live [`OutputPane`].
#[derive(Default)]
pub struct OutputPaneOptions {
	/// Whether the expanded cap applies.
	pub expanded: bool,
	/// Row cap while collapsed.
	pub collapsed_max_lines: usize,
	/// Optional row cap while expanded.
	pub expanded_max_lines: Option<usize>,
	/// Which edge survives capping.
	pub edge: Option<PaneEdge>,
	/// Apply the cap after terminal-width wrapping.
	pub visual: bool,
	/// Content width used by visual capping.
	pub width: Option<usize>,
	/// Per-row styling hook.
	pub style_line: Option<Box<dyn Fn(&str, usize) -> String>>,
	/// Keep sixel payload rows byte-for-byte and show the complete payload.
	pub uncap_sixel: bool,
	/// Set to `Some(false)` to suppress the hidden-row marker.
	pub show_hidden_marker: Option<bool>,
	/// Set to `Some(false)` to suppress the expand hint on the marker.
	pub show_expand_hint: Option<bool>,
	/// Show the expand hint even when nothing was capped.
	pub show_expand_hint_when_uncapped: Option<bool>,
	/// Hidden-row label hook.
	pub format_hidden: Option<Box<dyn Fn(usize, usize, usize, PaneEdge) -> String>>,
	/// Horizontal padding of the inner [`Text`].
	pub padding_x: Option<usize>,
	/// Prepend a blank row when there is content.
	pub leading_blank: bool,
	/// Retention cap for stored rows.
	pub max_stored_lines: Option<usize>,
	/// Normalize each logical row before it enters the retained stream.
	pub normalize_line: Option<Box<dyn Fn(&str) -> String>>,
}

/// Partial update for [`OutputPane::configure`], the Rust form of the
/// reference's `Partial<OutputPaneOptions>`. `None` leaves a field untouched;
/// `Some(None)` clears an optional field.
#[derive(Default)]
pub struct OutputPaneOptionsPatch {
	pub expanded: Option<bool>,
	pub collapsed_max_lines: Option<usize>,
	pub expanded_max_lines: Option<Option<usize>>,
	pub edge: Option<Option<PaneEdge>>,
	pub visual: Option<bool>,
	pub width: Option<Option<usize>>,
	pub style_line: Option<Box<dyn Fn(&str, usize) -> String>>,
	pub uncap_sixel: Option<bool>,
	pub show_hidden_marker: Option<Option<bool>>,
	pub show_expand_hint: Option<Option<bool>>,
	pub show_expand_hint_when_uncapped: Option<Option<bool>>,
	pub format_hidden: Option<Box<dyn Fn(usize, usize, usize, PaneEdge) -> String>>,
	pub padding_x: Option<Option<usize>>,
	pub leading_blank: Option<bool>,
	pub max_stored_lines: Option<Option<usize>>,
	pub normalize_line: Option<Box<dyn Fn(&str) -> String>>,
}

/// Stateful, cached output viewport. Streaming callers append chunks while
/// settled callers may replace all rows. Unchanged renders preserve array
/// identity through the inner Text cache.
pub struct OutputPane {
	theme: Theme,
	options: OutputPaneOptions,
	lines: Vec<String>,
	pending_carriage_return: bool,
	text: Text,
	/// The content last handed to the inner `Text`, compared by value.
	render_content: Option<String>,
	/// The last native description, kept with the inputs that produced it.
	native: RefCell<Option<NativeNode>>,
	/// The styled-and-joined content of the last render.
	memo: Memo<String>,
}

impl OutputPane {
	/// Create an empty pane bound to `theme`.
	pub fn new(theme: Theme, options: OutputPaneOptions) -> Self {
		let padding_x = options.padding_x.unwrap_or(0) as u16;
		OutputPane {
			theme,
			options,
			lines: Vec::new(),
			pending_carriage_return: false,
			text: Text::padded("", padding_x, 0),
			render_content: None,
			native: RefCell::new(None),
			memo: Memo::new(),
		}
	}

	/// Create a pane seeded with `text` (the reference's default constructor
	/// argument, which Rust cannot default).
	pub fn from_text(theme: Theme, options: OutputPaneOptions, text: impl Into<String>) -> Self {
		let mut pane = Self::new(theme, options);
		let text = text.into();
		if !text.is_empty() {
			pane.set_text(&text);
		}
		pane
	}

	/// Replace the complete output snapshot.
	pub fn set_text(&mut self, text: &str) {
		self.set_lines(&split_terminal_output_lines(text));
	}

	/// Replace the complete output snapshot with caller-owned immutable rows.
	pub fn set_lines(&mut self, lines: &[String]) {
		let normalized: Vec<String> = match self.options.normalize_line.as_ref() {
			Some(normalize) => lines.iter().map(|line| normalize(line)).collect(),
			None => lines.to_vec(),
		};
		self.lines = self.clamp_stored_lines(&normalized);
		self.pending_carriage_return = false;
		self.invalidate();
	}

	/// Append a streaming chunk. Bare CR overwrites the current logical row;
	/// a CRLF split across chunks remains a newline, so chunked and whole-text
	/// ingestion produce the same settled rows.
	pub fn append(&mut self, chunk: &str) {
		if chunk.is_empty() {
			return;
		}
		let bytes = chunk.as_bytes();
		let mut index = 0usize;
		if self.pending_carriage_return {
			self.pending_carriage_return = false;
			if bytes.first() == Some(&b'\n') {
				self.start_line();
				index = 1;
			} else {
				self.replace_tail("");
			}
		}

		let mut segment_start = index;
		while index < bytes.len() {
			let byte = bytes[index];
			if byte != b'\r' && byte != b'\n' {
				index += 1;
				continue;
			}
			self.append_to_tail(&chunk[segment_start..index]);
			if byte == b'\n' {
				self.start_line();
			} else if index + 1 < bytes.len() {
				if bytes[index + 1] == b'\n' {
					self.start_line();
					index += 1;
				} else {
					self.replace_tail("");
				}
			} else {
				self.pending_carriage_return = true;
			}
			index += 1;
			segment_start = index;
		}
		self.append_to_tail(&chunk[segment_start..]);
		self.lines = self.clamp_stored_lines(&self.lines);
		self.invalidate();
	}

	/// Settle a trailing bare carriage return once no later LF can pair with it.
	pub fn finish(&mut self) {
		if !self.pending_carriage_return {
			return;
		}
		self.pending_carriage_return = false;
		self.replace_tail("");
		self.invalidate();
	}

	/// Update presentation without replacing streamed content.
	pub fn configure(&mut self, options: OutputPaneOptionsPatch) {
		let previous_padding = self.options.padding_x.unwrap_or(0);
		if let Some(value) = options.expanded {
			self.options.expanded = value;
		}
		if let Some(value) = options.collapsed_max_lines {
			self.options.collapsed_max_lines = value;
		}
		if let Some(value) = options.expanded_max_lines {
			self.options.expanded_max_lines = value;
		}
		if let Some(value) = options.edge {
			self.options.edge = value;
		}
		if let Some(value) = options.visual {
			self.options.visual = value;
		}
		if let Some(value) = options.width {
			self.options.width = value;
		}
		if let Some(value) = options.style_line {
			self.options.style_line = Some(value);
		}
		if let Some(value) = options.uncap_sixel {
			self.options.uncap_sixel = value;
		}
		if let Some(value) = options.show_hidden_marker {
			self.options.show_hidden_marker = value;
		}
		if let Some(value) = options.show_expand_hint {
			self.options.show_expand_hint = value;
		}
		if let Some(value) = options.show_expand_hint_when_uncapped {
			self.options.show_expand_hint_when_uncapped = value;
		}
		if let Some(value) = options.format_hidden {
			self.options.format_hidden = Some(value);
		}
		if let Some(value) = options.padding_x {
			self.options.padding_x = value;
		}
		if let Some(value) = options.leading_blank {
			self.options.leading_blank = value;
		}
		if let Some(value) = options.max_stored_lines {
			self.options.max_stored_lines = value;
		}
		if let Some(value) = options.normalize_line {
			self.options.normalize_line = Some(value);
		}
		let next_padding = self.options.padding_x.unwrap_or(0);
		if previous_padding != next_padding {
			self.text = Text::padded("", next_padding as u16, 0);
		}
		self.lines = self.clamp_stored_lines(&self.lines);
		self.invalidate();
	}

	/// Collapse or expand the pane.
	pub fn set_expanded(&mut self, expanded: bool) {
		if self.options.expanded == expanded {
			return;
		}
		self.options.expanded = expanded;
		self.invalidate();
	}

	/// Number of retained logical rows.
	pub fn line_count(&self) -> usize {
		self.lines.len()
	}

	/// Whether the retained rows carry a sixel payload.
	pub fn has_sixel(&self) -> bool {
		self.has_sixel_with_protocol(terminal().image_protocol)
	}

	fn has_sixel_with_protocol(&self, protocol: Option<ImageProtocol>) -> bool {
		if protocol != Some(ImageProtocol::Sixel) || self.lines.is_empty() {
			return false;
		}
		get_sixel_line_mask(&self.lines).iter().any(|flag| *flag)
	}

	/// The retained rows joined with newlines.
	pub fn get_text(&self) -> String {
		self.lines.join("\n")
	}

	/// Drop derived caches; the next render rebuilds from state.
	pub fn invalidate(&mut self) {
		self.render_content = None;
		self.native.borrow_mut().take();
		self.memo.clear();
		self.text.invalidate();
	}

	/// Release owned render resources. Called when the component leaves the tree.
	pub fn release_render_caches(&mut self) {
		self.render_content = None;
		self.memo.clear();
		self.text.set_text("");
		self.text.invalidate();
	}

	/// The memo key: the width-affecting runtime configuration, a constant
	/// theme epoch, and the image protocol.
	fn render_deps() -> Vec<Value> {
		let info = terminal();
		vec![
			Value::String(format!("{:?}", info.hangul_jamo_width)),
			Value::Null,
			Value::String(format!("{:?}", info.image_protocol)),
		]
	}

	fn append_to_tail(&mut self, text: &str) {
		if text.is_empty() {
			return;
		}
		if self.lines.is_empty() {
			self.lines.push(String::new());
		}
		let last = self.lines.len() - 1;
		let combined = format!("{}{}", self.lines[last], text);
		self.lines[last] = combined;
	}

	fn replace_tail(&mut self, text: &str) {
		let normalized = self
			.options
			.normalize_line
			.as_ref()
			.map(|normalize| normalize(text))
			.unwrap_or_else(|| text.to_string());
		if self.lines.is_empty() {
			self.lines.push(normalized);
		} else {
			let last = self.lines.len() - 1;
			self.lines[last] = normalized;
		}
	}

	fn start_line(&mut self) {
		if self.lines.is_empty() {
			self.lines.push(String::new());
		}
		self.lines.push(String::new());
	}

	fn clamp_stored_lines(&self, lines: &[String]) -> Vec<String> {
		let Some(max_stored) = self.options.max_stored_lines else {
			return lines.to_vec();
		};
		if lines.len() <= max_stored {
			return lines.to_vec();
		}
		if max_stored == 0 {
			return Vec::new();
		}
		lines[lines.len() - max_stored..].to_vec()
	}
}

impl NativeDescribe for OutputPane {
	/// The retained rows as an `ansi` node; streamed appends grow its text, so
	/// the reconciler sends `text append`.
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		if let Some(native) = self.native.borrow().as_ref() {
			return native.clone();
		}
		let options = NativeOutputOptions {
			expanded: self.options.expanded,
			collapsed_max_lines: self.options.collapsed_max_lines,
			expanded_max_lines: self.options.expanded_max_lines,
			edge: self.options.edge,
			key: None,
		};
		let native = describe_output_lines(&self.lines, &options);
		*self.native.borrow_mut() = Some(native.clone());
		native
	}
}

impl Component for OutputPane {
	fn render(&mut self, width: u16) -> Vec<String> {
		let padding_x = get_padding_x(self.options.padding_x.unwrap_or(0));
		let content_width = (width as usize).saturating_sub(padding_x * 2).max(1);
		let mut deps = Self::render_deps();
		deps.push(Value::from(content_width));
		let style_line = self.options.style_line.as_ref().map(|style| style.as_ref());
		let format_hidden = self
			.options
			.format_hidden
			.as_ref()
			.map(|format| format.as_ref());
		let options = OutputPaneFormatOptions {
			lines: &self.lines,
			expanded: self.options.expanded,
			collapsed_max_lines: self.options.collapsed_max_lines,
			expanded_max_lines: self.options.expanded_max_lines,
			edge: self.options.edge,
			visual: self.options.visual,
			width: Some(content_width),
			style_line,
			uncap_sixel: self.options.uncap_sixel,
			show_hidden_marker: self.options.show_hidden_marker,
			show_expand_hint: self.options.show_expand_hint,
			show_expand_hint_when_uncapped: self.options.show_expand_hint_when_uncapped,
			format_hidden,
		};
		let leading_blank = self.options.leading_blank;
		let theme = &self.theme;
		let content = self
			.memo
			.get(&deps, || {
				let formatted = format_output_pane_lines(&options, theme);
				format!(
					"{}{}",
					if leading_blank && !formatted.lines.is_empty() {
						"\n"
					} else {
						""
					},
					formatted.lines.join("\n")
				)
			})
			.clone();
		if self.render_content.as_deref() != Some(content.as_str()) {
			self.render_content = Some(content.clone());
			self.text.set_text(content);
		}
		self.text.render(width)
	}

	fn invalidate(&mut self) {
		OutputPane::invalidate(self);
	}
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

	fn lines(values: &[&str]) -> Vec<String> {
		values.iter().map(|value| value.to_string()).collect()
	}

	#[test]
	fn split_collapses_carriage_return_overwrites() {
		assert_eq!(split_terminal_output_lines("a\rb\nc"), lines(&["b", "c"]));
		assert_eq!(split_terminal_output_lines("a\r\nb"), lines(&["a", "b"]));
		assert_eq!(split_terminal_output_lines("a\r"), lines(&[""]));
		assert_eq!(split_terminal_output_lines(""), lines(&[""]));
	}

	#[test]
	fn default_hidden_label_marks_head_and_tail() {
		assert_eq!(default_hidden_label(2, 3, 5, PaneEdge::Head), "… 2 more lines");
		assert_eq!(default_hidden_label(1, 3, 4, PaneEdge::Head), "… 1 more line");
		assert_eq!(
			default_hidden_label(2, 3, 5, PaneEdge::Tail),
			"… (2 earlier lines, showing 3 of 5)"
		);
	}

	#[test]
	fn head_cap_keeps_the_first_rows_and_appends_a_marker() {
		let theme = test_theme();
		let rows = lines(&["a", "b", "c"]);
		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 2,
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.hidden_count, 1);
		assert!(!result.has_sixel);
		assert_eq!(result.lines.len(), 3);
		assert_eq!(result.lines[0], "a");
		assert_eq!(result.lines[1], "b");
		assert!(result.lines[2].contains("1 more line"));
	}

	#[test]
	fn tail_cap_keeps_the_last_rows_and_prepends_a_marker() {
		let theme = test_theme();
		let rows = lines(&["a", "b", "c"]);
		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 2,
			edge: Some(PaneEdge::Tail),
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.hidden_count, 1);
		assert!(result.lines[0].contains("… (1 earlier lines, showing 2 of 3)"));
		assert_eq!(result.lines[1], "b");
		assert_eq!(result.lines[2], "c");
	}

	#[test]
	fn zero_limit_hides_every_row() {
		let theme = test_theme();
		let rows = lines(&["a", "b"]);
		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 0,
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.hidden_count, 2);
		assert_eq!(result.lines.len(), 1);
		assert!(result.lines[0].contains("2 more lines"));
	}

	#[test]
	fn expanded_cap_uses_expanded_max_lines() {
		let theme = test_theme();
		let rows = lines(&["a", "b", "c", "d"]);
		let options = OutputPaneFormatOptions {
			lines: &rows,
			expanded: true,
			collapsed_max_lines: 1,
			expanded_max_lines: Some(3),
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.hidden_count, 1);
		assert_eq!(result.lines.len(), 4);
	}

	#[test]
	fn style_line_colours_every_uncapped_row() {
		let theme = test_theme();
		let rows = lines(&["a", "b"]);
		let style = |line: &str, _index: usize| format!("[{line}]");
		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 5,
			style_line: Some(&style),
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.lines, lines(&["[a]", "[b]"]));
	}

	#[test]
	fn hidden_marker_and_hint_can_be_suppressed() {
		let theme = test_theme();
		let rows = lines(&["a", "b", "c"]);
		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 1,
			show_hidden_marker: Some(false),
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.lines, lines(&["a"]));

		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 1,
			show_expand_hint: Some(false),
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.lines.len(), 2);
		assert!(!result.lines[1].contains("Expand"));
	}

	#[test]
	fn uncapped_hint_appears_only_when_collapsed() {
		let theme = test_theme();
		let rows = lines(&["a"]);
		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 5,
			show_expand_hint_when_uncapped: Some(true),
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.lines.len(), 2);
		assert!(result.lines[1].contains("Expand"));

		let options = OutputPaneFormatOptions {
			expanded: true,
			..options
		};
		let result = format_output_pane_lines(&options, &theme);
		assert_eq!(result.lines.len(), 1);
	}

	#[test]
	fn visual_tail_cap_keeps_the_last_wrapped_rows() {
		let theme = test_theme();
		let rows = lines(&["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc"]);
		let options = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 2,
			edge: Some(PaneEdge::Tail),
			visual: true,
			width: Some(10),
			..OutputPaneFormatOptions::default()
		};
		let result = format_output_pane_lines(&options, &theme);
		// Each 10-cell row wraps to one visual line at width 10, so the last
		// two logical rows survive and one is skipped.
		assert_eq!(result.hidden_count, 1);
		assert_eq!(result.lines.len(), 3);
		assert_eq!(result.lines[1], "bbbbbbbbbb");
		assert_eq!(result.lines[2], "cccccccccc");
	}

	#[test]
	fn sixel_rows_are_untouched_and_can_be_uncapped() {
		let theme = test_theme();
		let rows = lines(&["\x1bPq\x1b\\", "plain"]);
		let style = |_line: &str, _index: usize| "<styled>".to_string();
		let base = OutputPaneFormatOptions {
			lines: &rows,
			collapsed_max_lines: 1,
			style_line: Some(&style),
			..OutputPaneFormatOptions::default()
		};
		let capped = format_output_pane_lines_with_protocol(&base, &theme, Some(ImageProtocol::Sixel));
		assert!(capped.has_sixel);
		assert_eq!(capped.hidden_count, 1);
		assert_eq!(capped.lines[0], "\x1bPq\x1b\\");
		assert_eq!(capped.lines[1].contains("<styled>"), false);

		let uncapped_options = OutputPaneFormatOptions {
			uncap_sixel: true,
			..base
		};
		let uncapped =
			format_output_pane_lines_with_protocol(&uncapped_options, &theme, Some(ImageProtocol::Sixel));
		assert_eq!(uncapped.hidden_count, 0);
		assert_eq!(uncapped.lines, lines(&["\x1bPq\x1b\\", "<styled>"]));
	}

	#[test]
	fn describe_output_lines_builds_a_previewed_ansi_node() {
		let rows = lines(&["a", "b"]);
		let node = describe_output_lines(
			&rows,
			&NativeOutputOptions {
				expanded: false,
				collapsed_max_lines: 1,
				..NativeOutputOptions::default()
			},
		);
		assert_eq!(node.kind, NodeKind::Ansi);
		assert_eq!(node.props.text(), Some("a\nb"));
		assert_eq!(
			node.props.get("preview").and_then(|value| value.get("lines")).and_then(Value::as_i64),
			Some(1)
		);
		assert_eq!(node.props.get_bool("follow"), None);

		let node = describe_output_lines(
			&rows,
			&NativeOutputOptions {
				expanded: false,
				collapsed_max_lines: 1,
				edge: Some(PaneEdge::Tail),
				key: Some("out".to_string()),
				..NativeOutputOptions::default()
			},
		);
		assert_eq!(node.props.get_bool("follow"), Some(true));
		assert_eq!(node.key.as_deref(), Some("out"));
	}

	#[test]
	fn pane_streams_chunks_with_cr_semantics() {
		let theme = test_theme();
		let mut pane = OutputPane::new(theme, OutputPaneOptions::default());
		pane.set_text("a\nb");
		assert_eq!(pane.line_count(), 2);
		assert_eq!(pane.get_text(), "a\nb");

		pane.append("c");
		assert_eq!(pane.get_text(), "a\nbc");

		pane.append("\nd");
		assert_eq!(pane.get_text(), "a\nbc\nd");

		pane.append("\re");
		assert_eq!(pane.get_text(), "a\nbc\ne");

		pane.append("f\r");
		assert_eq!(pane.get_text(), "a\nbc\nef");
		pane.append("\ng");
		assert_eq!(pane.get_text(), "a\nbc\nef\ng");

	pane.append("h\r");
	assert_eq!(pane.get_text(), "a\nbc\nef\ngh");
	pane.finish();
	assert_eq!(pane.get_text(), "a\nbc\nef\n");
	}

	#[test]
	fn pane_clamps_and_normalizes_stored_lines() {
		let theme = test_theme();
		let options = OutputPaneOptions {
			max_stored_lines: Some(2),
			normalize_line: Some(Box::new(|line: &str| line.to_uppercase())),
			..OutputPaneOptions::default()
		};
		let mut pane = OutputPane::new(theme, options);
		pane.set_lines(&lines(&["a", "b", "c"]));
		assert_eq!(pane.line_count(), 2);
		assert_eq!(pane.get_text(), "B\nC");
	}

	#[test]
	fn pane_configure_changes_padding_and_expansion() {
		let theme = test_theme();
		let options = OutputPaneOptions {
			collapsed_max_lines: 2,
			..OutputPaneOptions::default()
		};
		let mut pane = OutputPane::from_text(theme, options, "a\nb\nc");
		pane.configure(OutputPaneOptionsPatch {
			padding_x: Some(Some(1)),
			..OutputPaneOptionsPatch::default()
		});
		let rows = pane.render(20);
		assert!(rows.iter().any(|row| row.starts_with(' ')));

		pane.configure(OutputPaneOptionsPatch {
			expanded: Some(true),
			expanded_max_lines: Some(Some(3)),
			..OutputPaneOptionsPatch::default()
		});
		let rows = pane.render(20);
		assert_eq!(rows.len(), 3);
	}

	#[test]
	fn pane_render_is_cached_until_invalidated() {
		let theme = test_theme();
		let options = OutputPaneOptions {
			collapsed_max_lines: 2,
			..OutputPaneOptions::default()
		};
		let mut pane = OutputPane::from_text(theme, options, "a\nb\nc");
		let first = pane.render(30);
		let second = pane.render(30);
		assert_eq!(first, second);
        pane.append("\nd");
		let third = pane.render(30);
		assert_ne!(first, third);
	}

	#[test]
	fn pane_describe_native_is_ansi_and_cached() {
		let theme = test_theme();
		let options = OutputPaneOptions {
			collapsed_max_lines: 2,
			..OutputPaneOptions::default()
		};
		let mut pane = OutputPane::from_text(theme, options, "a\nb\nc");
		let cx = DescribeContext::default();
		let first = pane.describe_native(&cx);
		assert_eq!(first.kind, NodeKind::Ansi);
		assert_eq!(first.props.text(), Some("a\nb\nc"));
		let second = pane.describe_native(&cx);
		assert_eq!(first, second);
		pane.set_expanded(true);
		let third = pane.describe_native(&cx);
		assert!(third.props.get("preview").is_none());
	}

	#[test]
	fn pane_has_sixel_follows_the_runtime_protocol() {
		let theme = test_theme();
		let mut pane = OutputPane::new(theme, OutputPaneOptions::default());
		pane.set_lines(&lines(&["\x1bPq\x1b\\"]));
        assert!(pane.has_sixel_with_protocol(Some(ImageProtocol::Sixel)));
		pane.set_lines(&lines(&["plain"]));
		assert!(!pane.has_sixel_with_protocol(Some(ImageProtocol::Sixel)));
		pane.set_lines(&lines(&["\x1bPq\x1b\\"]));
		assert!(pane.has_sixel_with_protocol(Some(ImageProtocol::Sixel)));
		assert!(!pane.has_sixel_with_protocol(Some(ImageProtocol::Kitty)));
	}

	#[test]
	fn pane_release_render_caches_clears_text() {
		let theme = test_theme();
		let mut pane = OutputPane::from_text(theme, OutputPaneOptions::default(), "a\nb");
		let _ = pane.render(20);
		pane.release_render_caches();
		assert_eq!(pane.line_count(), 2);
	}
}
