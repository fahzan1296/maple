//! Component for a user-initiated eval cell with streaming output.
//!
//! Reference: `l/src/chat/eval-execution.ts`.
//!
//! Shares the frame and status vocabulary of [`crate::chat::execution_shared`].
//! The reference syntax-highlights the cell through `highlightCode`; this crate
//! has only a plain-line fallback, so the cell renders unhighlighted.
//!
//! Deviations:
//! - No loader animation and no `OutputPane`: output rows are assembled here.
//! - `highlightCode` is a plain split, matching the crate's `code-cell` port.

use crate::chat::execution_shared::{
	build_status_footer, clamp_display_line, describe_execution_card, describe_execution_tool,
	execution_status_label, resolve_execution_status, ExecutionCardInput, ExecutionColorKey,
	ExecutionStatus, ExecutionToolInput, StatusFooterOptions, PREVIEW_LINES,
};
use crate::chrome::dynamic_border::{BorderLabel, DynamicBorder};
use crate::component::Component;
use crate::native::describe::{code, span};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use crate::overlays::extensions::display_text::sanitize_display_text;
use crate::theme::Theme;
use crate::tools::output_meta::TruncationMeta;

/// Kernel a cell runs on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EvalExecutionLanguage {
	/// Python.
	#[default]
	Python,
	/// JavaScript.
	Js,
}

impl EvalExecutionLanguage {
	/// The highlighter language id.
	pub fn highlight_lang(self) -> &'static str {
		match self {
			EvalExecutionLanguage::Python => "python",
			EvalExecutionLanguage::Js => "javascript",
		}
	}

	/// Human title.
	pub fn title(self) -> &'static str {
		match self {
			EvalExecutionLanguage::Python => "Python",
			EvalExecutionLanguage::Js => "JavaScript",
		}
	}
}

/// What `set_complete` adds beyond the exit code.
#[derive(Clone, Debug, Default)]
pub struct EvalCompletion {
	/// Final output; replaces the streamed lines when present.
	pub output: Option<String>,
	/// Truncation metadata.
	pub truncation: Option<TruncationMeta>,
	/// Why the output artifact could not be captured.
	pub artifact_error: Option<String>,
}

/// A streaming `$` cell in the transcript.
pub struct EvalExecutionComponent {
	theme: Theme,
	code: String,
	language: EvalExecutionLanguage,
	exclude_from_context: bool,
	status: ExecutionStatus,
	exit_code: Option<i64>,
	lines: Vec<String>,
	expanded: bool,
	truncation: Option<TruncationMeta>,
	artifact_error: Option<String>,
	started_at_ms: f64,
	ended_at_ms: Option<f64>,
	block_version: u64,
	output_version: u64,
}

impl EvalExecutionComponent {
	/// A running cell; `exclude_from_context` dims the frame (`$$`).
	pub fn new(
		code: impl Into<String>,
		theme: Theme,
		exclude_from_context: bool,
		language: EvalExecutionLanguage,
	) -> Self {
		EvalExecutionComponent {
			theme,
			code: code.into(),
			language,
			exclude_from_context,
			status: ExecutionStatus::Running,
			exit_code: None,
			lines: Vec::new(),
			expanded: false,
			truncation: None,
			artifact_error: None,
			started_at_ms: 0.0,
			ended_at_ms: None,
			block_version: 0,
			output_version: 0,
		}
	}

	/// Record the run's start time, in milliseconds.
	pub fn set_started_at(&mut self, now_ms: f64) {
		self.started_at_ms = now_ms;
	}

	/// Whether the streaming preview is done rewriting its tail window.
	pub fn is_transcript_block_finalized(&self) -> bool {
		self.status != ExecutionStatus::Running
	}

	/// Mutations since finalization.
	pub fn transcript_block_version(&self) -> u64 {
		self.block_version
	}

	/// Show the whole output instead of the tail preview.
	pub fn set_expanded(&mut self, expanded: bool) {
		if self.expanded != expanded {
			self.block_version += 1;
		}
		self.expanded = expanded;
	}

	/// Append one output chunk.
	pub fn append_output(&mut self, chunk: &str) {
		self.lines.extend(chunk.split('\n').map(clamp_display_line));
		self.output_version += 1;
	}

	/// Settle the run.
	pub fn set_complete(
		&mut self,
		exit_code: Option<i64>,
		cancelled: bool,
		completion: EvalCompletion,
		now_ms: f64,
	) {
		self.exit_code = exit_code;
		self.status = resolve_execution_status(exit_code, cancelled);
		if self.ended_at_ms.is_none() {
			self.ended_at_ms = Some(now_ms);
		}
		self.truncation = completion.truncation;
		self.artifact_error = completion.artifact_error;
		if let Some(output) = completion.output {
			self.set_output(&output);
		}
		self.output_version += 1;
	}

	/// The output as text, for the stored message.
	pub fn output(&self) -> String {
		self.lines.join("\n")
	}

	/// The cell source.
	pub fn source(&self) -> &str {
		&self.code
	}

	/// Lifecycle state.
	pub fn status(&self) -> ExecutionStatus {
		self.status
	}

	/// The frame colour key.
	fn color_key(&self) -> ExecutionColorKey {
		if self.exclude_from_context {
			ExecutionColorKey::Dim
		} else {
			ExecutionColorKey::PythonMode
		}
	}

	fn set_output(&mut self, output: &str) {
		let clean = sanitize_display_text(output);
		self.lines = if clean.is_empty() {
			Vec::new()
		} else {
			clean.split('\n').map(clamp_display_line).collect()
		};
	}

	fn visible_rows(&self) -> Vec<String> {
		let rows: Vec<String> = if self.expanded {
			self.lines.clone()
		} else {
			self.lines.iter().rev().take(PREVIEW_LINES).rev().cloned().collect()
		};
		rows.into_iter()
			.map(|line| self.theme.fg("muted", &line).unwrap_or(line))
			.collect()
	}

	fn hidden_line_count(&self) -> usize {
		if self.expanded {
			0
		} else {
			self.lines.len().saturating_sub(PREVIEW_LINES)
		}
	}

	fn header_rows(&self) -> Vec<String> {
		let color = self.color_key().as_str();
		let prompt = self
			.theme
			.fg(color, &self.theme.bold(">>>"))
			.unwrap_or_else(|_| ">>>".to_string());
		let continuation = self
			.theme
			.fg(color, "    ")
			.unwrap_or_else(|_| "    ".to_string());
		self.code
			.split('\n')
			.enumerate()
			.map(|(index, line)| {
				let body = self.theme.fg("muted", line).unwrap_or_else(|_| line.to_string());
				if index == 0 {
					format!(" {prompt} {body}")
				} else {
					format!(" {continuation}{body}")
				}
			})
			.collect()
	}
}

impl Component for EvalExecutionComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width.max(1);
		let color = self.color_key().as_str().to_string();
		let theme = &self.theme;
		let top_color = color.clone();
		let mut top = DynamicBorder::with_color(
			theme,
			Box::new(move |text| theme.fg(&top_color, text).unwrap_or_else(|_| text.to_string())),
		);
		let label = execution_status_label(self.status, self.exit_code, theme);
		let bottom_color = color;
		let mut bottom = DynamicBorder::with_color(
			theme,
			Box::new(move |text| theme.fg(&bottom_color, text).unwrap_or_else(|_| text.to_string())),
		);
		bottom.set_label(label.map(|label| BorderLabel::fixed(label)));

		let mut rows = top.render(width);
		rows.extend(self.header_rows());
		for line in self.visible_rows() {
			rows.push(format!(" {line}"));
		}
		if self.status != ExecutionStatus::Running {
			let footer = build_status_footer(
				&StatusFooterOptions {
					truncation: self.truncation.as_ref(),
					artifact_error: self.artifact_error.as_deref(),
					minimized: None,
					hidden_line_count: self.hidden_line_count(),
					suppress_hidden_count: false,
				},
				theme,
			);
			for row in footer {
				rows.push(format!(" {row}"));
			}
		}
		rows.extend(bottom.render(width));
		rows
	}

	fn invalidate(&mut self) {
		self.output_version += 1;
	}
}

impl crate::native::describe::NativeDescribe for EvalExecutionComponent {
	fn describe_native(&self, context: &DescribeContext) -> NativeNode {
		let color = self.color_key().as_str();
		let card_input = ExecutionCardInput {
			role: "omp.eval",
			head: vec![span(self.language.title(), Some(&format!("{color} strong")))],
			muted: self.exclude_from_context,
			status: self.status,
			started_at_ms: self.started_at_ms,
			now_ms: self.ended_at_ms.unwrap_or(self.started_at_ms),
			expanded: self.expanded,
			lead: vec![code(
				self.code.clone(),
				{
					let mut props = Props::new();
					props.set_str("lang", self.language.highlight_lang());
					props
				},
			)],
			output: self.output(),
			exit_code: self.exit_code,
			truncation: self.truncation.clone(),
			artifact_error: self.artifact_error.clone(),
			..ExecutionCardInput::default()
		};
		if context.supports(NodeKind::Tool) {
			describe_execution_tool(&ExecutionToolInput {
				role: "omp.eval",
				name: "eval",
				title: self.language.title(),
				command: &self.code,
				lang: self.language.highlight_lang(),
				excluded: self.exclude_from_context,
				status: self.status,
				started_at_ms: self.started_at_ms,
				ended_at_ms: self.ended_at_ms,
				now_ms: self.ended_at_ms.unwrap_or(self.started_at_ms),
				expanded: self.expanded,
				output: self.output(),
				exit_code: self.exit_code,
				truncation: self.truncation.clone(),
				artifact_error: self.artifact_error.clone(),
				minimized: None,
				..ExecutionToolInput::default()
			})
		} else {
			describe_execution_card(&card_input)
		}
	}
}

/// The cell's header rows for terminals without native chrome.
pub fn eval_header_rows(source: &str, theme: &Theme, color_key: ExecutionColorKey) -> Vec<String> {
	let prompt = theme
		.fg(color_key.as_str(), &theme.bold(">>>"))
		.unwrap_or_else(|_| ">>>".to_string());
	let continuation = theme.fg(color_key.as_str(), "    ").unwrap_or_else(|_| "    ".to_string());
	source
		.split('\n')
		.enumerate()
		.map(|(index, line)| {
			let body = theme.fg("muted", line).unwrap_or_else(|_| line.to_string());
			if index == 0 {
				format!("{prompt} {body}")
			} else {
				format!("{continuation}{body}")
			}
		})
		.collect()
}


#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};
	use crate::theme::theme_class::ColorMode;

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

	#[test]
	fn header_prefixes_the_prompt_and_indents_continuations() {
		let mut eval = EvalExecutionComponent::new(
			"a = 1\nprint(a)",
			theme(),
			false,
			EvalExecutionLanguage::Python,
		);
		eval.set_started_at(0.0);
		let rows = eval.render(40);
		let plain: Vec<String> = rows.iter().map(|row| crate::ansi_scan::strip_ansi(row)).collect();
		assert!(plain[0].trim_start().starts_with(">>>"), "{:?}", plain[0]);
		assert!(plain.iter().any(|row| row.contains("a = 1")));
		assert!(plain.iter().any(|row| row.contains("    print(a)")));
	}

	#[test]
	fn output_uses_the_tail_preview_until_expanded() {
		let mut eval = EvalExecutionComponent::new("x", theme(), false, EvalExecutionLanguage::Js);
		for index in 0..(PREVIEW_LINES + 5) {
			eval.append_output(&format!("row {index}\n"));
		}
		let rows = eval.render(40);
		let plain: Vec<String> = rows.iter().map(|row| crate::ansi_scan::strip_ansi(row)).collect();
		assert!(plain.iter().any(|row| row.contains(&format!("row {}", PREVIEW_LINES + 4))));
		assert!(!plain.iter().any(|row| row.contains("row 0")));
		eval.set_expanded(true);
		assert_eq!(eval.transcript_block_version(), 1);
	}

	#[test]
	fn completion_and_cancellation_set_the_status_label() {
		let mut eval = EvalExecutionComponent::new("1/0", theme(), false, EvalExecutionLanguage::Python);
		eval.set_complete(
			Some(1),
			false,
			EvalCompletion {
				output: Some("ZeroDivisionError".to_string()),
				..EvalCompletion::default()
			},
			4.0,
		);
		assert_eq!(eval.status(), ExecutionStatus::Error);
		assert!(eval.is_transcript_block_finalized());
		let rows = eval.render(40);
		let plain: Vec<String> = rows.iter().map(|row| crate::ansi_scan::strip_ansi(row)).collect();
		assert!(plain.iter().any(|row| row.contains("exit 1")));
		assert!(plain.iter().any(|row| row.contains("ZeroDivisionError")));
	}

	#[test]
	fn excluded_cells_describe_a_muted_card() {
		use crate::native::describe::NativeDescribe;
		let eval = EvalExecutionComponent::new("x", theme(), true, EvalExecutionLanguage::Js);
		let no_tool = DescribeContext::with_capabilities(80, &[], &[]);
		let card = eval.describe_native(&no_tool);
		assert_eq!(card.kind, NodeKind::Card);
		let tool = eval.describe_native(&DescribeContext::new(80));
		assert_eq!(tool.props.get_str("name"), Some("eval"));
	}
}
