//! Component for a user-initiated bash run with streaming output.
//!
//! Reference: `l/src/chat/bash-execution.ts`.
//!
//! The reference streams raw PTY bytes through a headless xterm and re-reads
//! its screen; this crate has no terminal emulator, so the component renders
//! the sanitized text lines the caller feeds it.
//!
//! Deviations:
//! - No PTY replay, sixel passthrough, image budget, or chunk throttle: timing
//!   and chunk batching belong to the caller that owns the event loop.
//! - The loader that animates the bottom rule does not exist; the rule carries
//!   the settled [`ExecutionStatus`] label instead.
//! - Images render as links because this crate has no image component.

use crate::chat::execution_shared::{
    build_status_footer, clamp_display_line, describe_execution_card, describe_execution_tool,
    execution_status_label, resolve_execution_status, ExecutionCardInput, ExecutionColorKey,
    ExecutionStatus, ExecutionToolInput, StatusFooterOptions, PREVIEW_LINES,
};
use crate::chrome::dynamic_border::{BorderLabel, DynamicBorder};
use crate::component::Component;
use crate::overlays::extensions::display_text::sanitize_display_text;
use crate::native::describe::{span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::theme::Theme;
use crate::tools::output_meta::{MinimizedMeta, TruncationMeta};

/// Streaming output retained while collapsed: five preview windows.
const STREAMING_LINE_CAP: usize = PREVIEW_LINES * 5;

/// PTY size for `!` commands: the execution frame's inner content area,
/// given the terminal's own size.
pub fn bash_pty_viewport(columns: usize, rows: usize) -> (usize, usize) {
	(columns.saturating_sub(2).max(20), rows.saturating_sub(4).max(5))
}

/// What `set_complete` adds beyond the exit code.
#[derive(Clone, Debug, Default)]
pub struct BashCompletion {
	/// Final output; replaces the streamed lines when present.
	pub output: Option<String>,
	/// Truncation metadata.
	pub truncation: Option<TruncationMeta>,
	/// Why the output artifact could not be captured.
	pub artifact_error: Option<String>,
	/// Bash minimizer metadata.
	pub minimized: Option<MinimizedMeta>,
	/// Image links to render below the output.
	pub images: Vec<String>,
}

/// A streaming `!` command in the transcript.
pub struct BashExecutionComponent {
	theme: Theme,
	command: String,
	color_key: ExecutionColorKey,
	status: ExecutionStatus,
	exit_code: Option<i64>,
	lines: Vec<String>,
	expanded: bool,
	truncation: Option<TruncationMeta>,
	artifact_error: Option<String>,
	minimized: Option<MinimizedMeta>,
	images: Vec<String>,
	started_at_ms: f64,
	ended_at_ms: Option<f64>,
	block_version: u64,
	output_version: u64,
}

impl BashExecutionComponent {
	/// A running `!` command; `exclude_from_context` dims the frame (`!!`).
	pub fn new(command: impl Into<String>, theme: Theme, exclude_from_context: bool) -> Self {
		BashExecutionComponent {
			theme,
			command: command.into(),
			color_key: if exclude_from_context {
				ExecutionColorKey::Dim
			} else {
				ExecutionColorKey::BashMode
			},
			status: ExecutionStatus::Running,
			exit_code: None,
			lines: Vec::new(),
			expanded: false,
			truncation: None,
			artifact_error: None,
			minimized: None,
			images: Vec::new(),
			started_at_ms: 0.0,
			ended_at_ms: None,
			block_version: 0,
			output_version: 0,
		}
	}

	/// Record the run's start time, in milliseconds, for the live timer.
	pub fn set_started_at(&mut self, now_ms: f64) {
		self.started_at_ms = now_ms;
	}

	/// Whether the streaming preview is done rewriting its tail window.
	pub fn is_transcript_block_finalized(&self) -> bool {
		self.status != ExecutionStatus::Running
	}

	/// Mutations since finalization (expansion toggles bump it).
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

	/// Whether the block shows its whole output.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// Append one output chunk.
	pub fn append_output(&mut self, chunk: &str) {
		let incoming: Vec<String> = chunk.split('\n').map(clamp_display_line).collect();
		if let Some(last) = self.lines.last_mut() {
			if let Some(first) = incoming.first() {
				last.push_str(first);
			}
			self.lines.extend(incoming.into_iter().skip(1));
		} else {
			self.lines.extend(incoming);
		}
		if self.lines.len() > STREAMING_LINE_CAP {
			self.lines.drain(..self.lines.len() - STREAMING_LINE_CAP);
		}
		self.output_version += 1;
	}

	/// Settle the run: status from the exit code, final output, notices.
	pub fn set_complete(
		&mut self,
		exit_code: Option<i64>,
		cancelled: bool,
		completion: BashCompletion,
		now_ms: f64,
	) {
		self.exit_code = exit_code;
		self.status = resolve_execution_status(exit_code, cancelled);
		if self.ended_at_ms.is_none() {
			self.ended_at_ms = Some(now_ms);
		}
		self.truncation = completion.truncation;
		self.artifact_error = completion.artifact_error;
		self.minimized = completion.minimized;
		self.images = completion.images;
		if let Some(output) = completion.output {
			self.set_output(&output);
		}
	}

	/// The output as text, for the stored message.
	pub fn output(&self) -> String {
		self.lines.join("\n")
	}

	/// The command that ran.
	pub fn command(&self) -> &str {
		&self.command
	}

	/// Lifecycle state.
	pub fn status(&self) -> ExecutionStatus {
		self.status
	}

	fn set_output(&mut self, output: &str) {
		let clean = sanitize_display_text(output);
		self.lines = if clean.is_empty() {
			Vec::new()
		} else {
			clean.split('\n').map(clamp_display_line).collect()
		};
		self.output_version += 1;
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
}

impl Component for BashExecutionComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width.max(1);
		let color = self.color_key.as_str().to_string();
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
		let header = format!("$ {}", self.command);
		let bold = theme.bold(&header);
		let styled = theme
			.fg(self.color_key.as_str(), &bold)
			.unwrap_or(bold);
		rows.push(format!(" {styled}"));
		for line in self.visible_rows() {
			rows.push(format!(" {line}"));
		}
		for image in &self.images {
			let link = theme
				.fg("muted", &format!("[image {image}]"))
				.unwrap_or_else(|_| format!("[image {image}]"));
			rows.push(format!(" {link}"));
		}
		if self.status != ExecutionStatus::Running {
			let footer = build_status_footer(
				&StatusFooterOptions {
					truncation: self.truncation.as_ref(),
					artifact_error: self.artifact_error.as_deref(),
					minimized: self.minimized.as_ref(),
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

impl crate::native::describe::NativeDescribe for BashExecutionComponent {
	fn describe_native(&self, context: &DescribeContext) -> NativeNode {
		let common = ExecutionCardInput {
			role: "omp.bash",
			head: vec![span(
				format!("$ {}", self.command),
				Some(&format!("{} strong", self.color_key.as_str())),
			)],
			muted: self.color_key == ExecutionColorKey::Dim,
			status: self.status,
			started_at_ms: self.started_at_ms,
			now_ms: self.ended_at_ms.unwrap_or(self.started_at_ms),
			expanded: self.expanded,
			output: self.output(),
			exit_code: self.exit_code,
			truncation: self.truncation.clone(),
			artifact_error: self.artifact_error.clone(),
			minimized: self.minimized.clone(),
			..ExecutionCardInput::default()
		};
		let mut tool = ExecutionToolInput {
			role: "omp.bash",
			name: "bash",
			title: "Bash",
			command: &self.command,
			lang: "bash",
			excluded: self.color_key == ExecutionColorKey::Dim,
			status: self.status,
			started_at_ms: self.started_at_ms,
			ended_at_ms: self.ended_at_ms,
			now_ms: self.ended_at_ms.unwrap_or(self.started_at_ms),
			expanded: self.expanded,
			output: self.output(),
			exit_code: self.exit_code,
			truncation: self.truncation.clone(),
			artifact_error: self.artifact_error.clone(),
			minimized: self.minimized.clone(),
			..ExecutionToolInput::default()
		};
		if context.supports(NodeKind::Tool) {
			tool.images = Vec::new();
			describe_execution_tool(&tool)
		} else {
			describe_execution_card(&common)
		}
	}
}

/// The block's fallback head for terminals without native chrome.
pub fn bash_head(command: &str, theme: &Theme) -> NativeNode {
	let mut props = Props::new();
	props.set_str("role", "omp.bash.command");
	let plain = theme.fg("bashMode", &format!("$ {command}")).unwrap_or_else(|_| format!("$ {command}"));
	text(NodeText::Plain(plain), props)
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
	fn streaming_merges_partial_lines_and_caps_the_tail() {
		let mut bash = BashExecutionComponent::new("seq 1000", theme(), false);
		bash.append_output("one\ntw");
		bash.append_output("o\nthree\n");
		assert_eq!(bash.output(), "one\ntwo\nthree\n");
		for _ in 0..(STREAMING_LINE_CAP + 20) {
			bash.append_output("x\n");
		}
		assert_eq!(bash.lines.len(), STREAMING_LINE_CAP + 1);
	}

	#[test]
	fn collapse_shows_the_tail_preview() {
		let mut bash = BashExecutionComponent::new("ls", theme(), false);
		for index in 0..(PREVIEW_LINES + 7) {
			bash.append_output(&format!("line {index}\n"));
		}
		let rows = bash.render(40);
		let plain: Vec<String> = rows.iter().map(|row| crate::ansi_scan::strip_ansi(row)).collect();
		assert!(plain.iter().any(|row| row.contains(&format!("line {}", PREVIEW_LINES + 6))));
		assert!(!plain.iter().any(|row| row.contains("line 0")), "the head of the output is hidden");
		bash.set_expanded(true);
		assert_eq!(bash.transcript_block_version(), 1);
		assert_eq!(bash.hidden_line_count(), 0);
	}

	#[test]
	fn completion_sets_status_and_exposes_notices() {
		let mut bash = BashExecutionComponent::new("false", theme(), false);
		bash.set_complete(
			Some(1),
			false,
			BashCompletion {
				output: Some("boom".to_string()),
				..BashCompletion::default()
			},
			10.0,
		);
		assert_eq!(bash.status(), ExecutionStatus::Error);
		assert!(bash.is_transcript_block_finalized());
		let rows = bash.render(40);
		let plain: Vec<String> = rows.iter().map(|row| crate::ansi_scan::strip_ansi(row)).collect();
		assert!(plain.iter().any(|row| row.contains("exit 1")));
		assert!(plain.iter().any(|row| row.contains("boom")));
	}

	#[test]
	fn cancellation_wins_over_the_exit_code() {
		let mut bash = BashExecutionComponent::new("sleep 99", theme(), true);
		bash.set_complete(None, true, BashCompletion::default(), 5.0);
		assert_eq!(bash.status(), ExecutionStatus::Cancelled);
		let rows = bash.render(40);
		assert!(crate::ansi_scan::strip_ansi(&rows.concat()).contains("cancelled"));
	}

	#[test]
	fn describe_uses_the_tool_frame_when_supported() {
		use crate::native::describe::NativeDescribe;
		let mut bash = BashExecutionComponent::new("ls", theme(), false);
		bash.append_output("a\n");
		let tool = bash.describe_native(&DescribeContext::new(80));
		assert_eq!(tool.kind, NodeKind::Tool);
		assert_eq!(tool.props.get_str("command"), Some("ls"));
		let no_tool = DescribeContext::with_capabilities(80, &[], &[]);
		let card = bash.describe_native(&no_tool);
		assert_eq!(card.kind, NodeKind::Card);
		assert_eq!(card.props.get_str("role"), Some("omp.bash"));
		let head = bash_head("ls", &theme());
		assert_eq!(head.kind, NodeKind::Text);
	}

	#[test]
	fn pty_viewport_keeps_a_minimum_size() {
		assert_eq!(bash_pty_viewport(80, 24), (78, 20));
		assert_eq!(bash_pty_viewport(10, 3), (20, 5));
	}
}
