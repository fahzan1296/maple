//! Reference: `l/src/overlays/omfg-panel.ts`.
//!
//! Streaming panel for `/omfg <complaint>`: a candidate TTSR rule is drafted,
//! validated and saved, and the panel reports each step.
//!
//! Deviations:
//! - The reference extends `OverlayPanel` and mounts a `StreamingPanelContent`
//!   child; the crate renders the same panel chrome itself (top border, status
//!   row, streamed body, footer, bottom border) so the streamed markdown can be
//!   re-rendered at the panel's content width without a shared row buffer.
//! - The process-global theme and keybindings are injected ([`Theme`],
//!   [`KeybindingsManager`]); status glyphs come from [`Theme::status`] and the
//!   interrupt key from [`interrupt_key`].
//! - `TUI.requestRender` has no crate counterpart; [`OmfgPanelComponent::append_draft`]
//!   and friends only rebuild state, and the host repaints after calling them.
//! - The reference is dismissed by the composer's `app.interrupt` handler; here
//!   [`Component::handle_input`] maps Escape to [`OmfgPanelComponent::close`].

use crate::chrome::keybinding_hints::{bound_keys, interrupt_key};
use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::component::Component;
use crate::components::markdown::Markdown;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::describe::{col, md, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span};
use crate::native::overlay::{NativeHint, hints_row, status_hints_row};
use crate::render::render_utils::replace_tabs;
use crate::theme::Theme;

/// Lifecycle of the `/omfg` panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OmfgPanelState {
	/// A rule is being drafted.
	#[default]
	Generating,
	/// The draft is being validated.
	Validating,
	/// The operator is confirming the rule.
	Confirming,
	/// The rule is being written.
	Saving,
	/// The rule was registered.
	Saved,
	/// The operator declined the rule.
	Rejected,
	/// The request was cancelled.
	Aborted,
	/// The rule could not be produced.
	Error,
}

impl OmfgPanelState {
	/// Whether work is still in flight (the sheet head carries a spinner).
	pub fn is_live(self) -> bool {
		matches!(
			self,
			OmfgPanelState::Generating
				| OmfgPanelState::Validating
				| OmfgPanelState::Confirming
				| OmfgPanelState::Saving
		)
	}

	/// The state id as the wire spells it.
	pub fn as_str(self) -> &'static str {
		match self {
			OmfgPanelState::Generating => "generating",
			OmfgPanelState::Validating => "validating",
			OmfgPanelState::Confirming => "confirming",
			OmfgPanelState::Saving => "saving",
			OmfgPanelState::Saved => "saved",
			OmfgPanelState::Rejected => "rejected",
			OmfgPanelState::Aborted => "aborted",
			OmfgPanelState::Error => "error",
		}
	}
}

/// Construction options of [`OmfgPanelComponent`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OmfgPanelOptions {
	/// The rule complaint typed after `/omfg`.
	pub complaint: String,
}

/// Role of the panel's native root.
pub const OMFG_PANEL_ROLE: &str = "omp.overlay.omfg";

/// Streaming `/omfg` rule panel.
pub struct OmfgPanelComponent {
	complaint: String,
	state: OmfgPanelState,
	status: String,
	preview: String,
	saved_path: Option<String>,
	error_message: Option<String>,
	closed: bool,
	theme: Theme,
	bindings: KeybindingsManager,
	panel_theme: OverlayTheme,
}

impl OmfgPanelComponent {
	/// A panel for `options.complaint`, in the generating state.
	pub fn new(theme: Theme, bindings: KeybindingsManager, options: OmfgPanelOptions) -> Self {
		let complaint = replace_tabs(&options.complaint);
		OmfgPanelComponent {
			complaint,
			state: OmfgPanelState::Generating,
			status: "Generating TTSR rule…".to_string(),
			preview: String::new(),
			saved_path: None,
			error_message: None,
			closed: false,
			theme,
			bindings,
			panel_theme: OverlayTheme::default(),
		}
	}

	/// Append streamed rule text.
	pub fn append_draft(&mut self, delta: &str) {
		if delta.is_empty() || self.closed {
			return;
		}
		self.preview.push_str(delta);
	}

	/// Replace the whole preview.
	pub fn set_rule(&mut self, text: impl Into<String>) {
		if self.closed {
			return;
		}
		self.preview = text.into();
	}

	/// Move to `state` with a fresh status line.
	pub fn set_status(&mut self, state: OmfgPanelState, status: impl Into<String>) {
		if self.closed {
			return;
		}
		self.state = state;
		self.status = status.into();
		self.error_message = None;
	}

	/// Report the rule as registered at `path`.
	pub fn mark_saved(&mut self, path: impl Into<String>) {
		if self.closed {
			return;
		}
		let path = path.into();
		self.state = OmfgPanelState::Saved;
		self.status = format!("Saved {path}");
		self.saved_path = Some(path);
		self.error_message = None;
	}

	/// Report the rule as declined.
	pub fn mark_rejected(&mut self) {
		if self.closed {
			return;
		}
		self.state = OmfgPanelState::Rejected;
		self.status = "Rule was not saved.".to_string();
		self.error_message = None;
	}

	/// Report the request as cancelled.
	pub fn mark_aborted(&mut self) {
		if self.closed {
			return;
		}
		self.state = OmfgPanelState::Aborted;
		self.status = "Cancelled.".to_string();
		self.error_message = None;
	}

	/// Report a failure with `message`.
	pub fn mark_error(&mut self, message: impl Into<String>) {
		if self.closed {
			return;
		}
		self.state = OmfgPanelState::Error;
		self.status = "Could not create rule.".to_string();
		self.error_message = Some(message.into());
	}

	/// Stop accepting updates.
	pub fn close(&mut self) {
		self.closed = true;
	}

	/// Whether the panel was closed.
	pub fn is_closed(&self) -> bool {
		self.closed
	}

	/// The current state.
	pub fn state(&self) -> OmfgPanelState {
		self.state
	}

	/// The current status line (unstyled).
	pub fn status(&self) -> &str {
		&self.status
	}

	/// The candidate rule as streamed so far.
	pub fn preview(&self) -> &str {
		&self.preview
	}

	/// The complaint the panel was opened for.
	pub fn complaint(&self) -> &str {
		&self.complaint
	}

	/// The path a saved rule was registered at.
	pub fn saved_path(&self) -> Option<&str> {
		self.saved_path.as_deref()
	}

	/// The failure message, when the state is [`OmfgPanelState::Error`].
	pub fn error_message(&self) -> Option<&str> {
		self.error_message.as_deref()
	}

	/// Panel title: `/omfg <complaint>`.
	pub fn title(&self) -> String {
		format!("/omfg {}", self.complaint)
	}

	/// The status glyph plus status line, tone by state.
	fn status_text(&self) -> (String, &'static str) {
		match self.state {
			OmfgPanelState::Saved => (self.theme.status().success.clone(), "success"),
			OmfgPanelState::Rejected | OmfgPanelState::Aborted => (self.theme.status().warning.clone(), "warning"),
			OmfgPanelState::Error => (self.theme.status().error.clone(), "error"),
			_ => (String::new(), "muted"),
		}
	}

	/// The streamed body: the error, the candidate rule, or the waiting note.
	fn body_rows(&self, width: u16) -> Vec<String> {
		if self.state == OmfgPanelState::Error {
			let message = replace_tabs(self.error_message.as_deref().unwrap_or("Unknown error"));
			return self.theme.fg("error", &message).map(|row| vec![row]).unwrap_or_else(|_| vec![message]);
		}
		let preview = replace_tabs(&self.preview);
		let preview = preview.trim();
		if preview.is_empty() {
			let waiting = format!("{} Waiting for candidate rule…", self.theme.status().pending);
			return self.theme.fg("dim", &waiting).map(|row| vec![row]).unwrap_or_else(|_| vec![waiting]);
		}
		let mut markdown = Markdown::new();
		markdown.set_text(preview);
		markdown.render(width)
	}

	/// The footer line: status glyph plus the dismiss hint.
	fn footer_line(&self) -> String {
		let esc = interrupt_key(&self.bindings, &self.theme);
		match self.state {
			OmfgPanelState::Saved => format!(
				"{} Registered live · {} · {esc} dismiss",
				self.theme.status().success,
				replace_tabs(self.saved_path.as_deref().unwrap_or("saved"))
			),
			OmfgPanelState::Rejected => format!("{} Not saved · {esc} dismiss", self.theme.status().warning),
			OmfgPanelState::Aborted => format!("{} Cancelled · {esc} dismiss", self.theme.status().warning),
			OmfgPanelState::Error => format!("{} Error · {esc} dismiss", self.theme.status().error),
			_ => format!("{esc} cancel /omfg"),
		}
	}

	/// Footer tone matching the state.
	fn footer_tone(&self) -> &'static str {
		match self.state {
			OmfgPanelState::Saved => "success",
			OmfgPanelState::Rejected | OmfgPanelState::Aborted => "warning",
			OmfgPanelState::Error => "error",
			_ => "muted",
		}
	}

	/// The dismiss hint: the interrupt key, falling back to Escape.
	fn dismiss_hint(&self) -> NativeHint {
		NativeHint {
			keys: bound_keys(&self.bindings, "app.interrupt", &["escape".to_string()])
				.into_iter()
				.take(1)
				.collect(),
			label: "dismiss".to_string(),
		}
	}

	/// A borderless sheet column headed by the complaint, styled by role.
	pub fn describe(&self, _cx: &DescribeContext) -> NativeNode {
		let tone = match self.state {
			OmfgPanelState::Error => Some("error"),
			OmfgPanelState::Rejected | OmfgPanelState::Aborted => Some("warning"),
			_ => None,
		};
		let mut head_children: Vec<NativeNode> = Vec::new();
		if self.state.is_live() {
			head_children.push(node(NodeKind::Spinner, Props::new(), Vec::new(), None));
		}
		let mut head_props = Props::new();
		head_props.set_str("truncate", "end");
		head_props.set_i64("lines", 1);
		head_children.push(text(
			NodeText::Spans(vec![
				Span::styled("/omfg", "accent"),
				Span::new(format!(" {}", self.complaint)),
			]),
			head_props,
		));
		let mut row_props = Props::new();
		row_props.set_role("omp.sheet.head");
		row_props.set_str("gap", "sm");
		row_props.set_str("align", "center");
		let head = node(NodeKind::Row, row_props, head_children, None);

		let mut status_props = Props::new();
		status_props.set_str("wrap", "word");
		let status = text(
			NodeText::Spans(vec![Span::styled(replace_tabs(&self.status), "muted")]),
			status_props,
		);
		let body = if self.state == OmfgPanelState::Error {
			let message = replace_tabs(self.error_message.as_deref().unwrap_or("Unknown error"));
			let mut props = Props::new();
			props.set_str("wrap", "word");
			text(NodeText::Spans(vec![Span::styled(message, "error")]), props)
		} else {
			let preview = replace_tabs(&self.preview);
			let preview = preview.trim();
			if preview.is_empty() {
				text(
					NodeText::Spans(vec![Span::styled(
						format!("{} Waiting for candidate rule…", self.theme.status().pending),
						"dim",
					)]),
					Props::new(),
				)
			} else {
				md(preview, Props::new())
			}
		};
		let footer = self.describe_footer();
		let mut props = Props::new();
		props.set_role(OMFG_PANEL_ROLE);
		props.set_str("gap", "sm");
		if let Some(tone) = tone {
			props.set_str("tone", tone);
		}
		col(vec![head, status, body, footer], props)
	}

	fn describe_footer(&self) -> NativeNode {
		let esc = Some(self.dismiss_hint());
		match self.state {
			OmfgPanelState::Saved => status_hints_row(
				NodeText::Spans(vec![
					Span::styled(format!("{} Registered live", self.theme.status().success), "success"),
					Span::styled(" · ", "muted"),
					Span::styled(replace_tabs(self.saved_path.as_deref().unwrap_or("saved")), "path"),
				]),
				&[esc],
			),
			OmfgPanelState::Rejected => status_hints_row(
				NodeText::Spans(vec![Span::styled(
					format!("{} Not saved", self.theme.status().warning),
					"warning",
				)]),
				&[esc],
			),
			OmfgPanelState::Aborted => status_hints_row(
				NodeText::Spans(vec![Span::styled(
					format!("{} Cancelled", self.theme.status().warning),
					"warning",
				)]),
				&[esc],
			),
			OmfgPanelState::Error => status_hints_row(
				NodeText::Spans(vec![Span::styled(format!("{} Error", self.theme.status().error), "error")]),
				&[esc],
			),
			_ => hints_row(
				&[Some(NativeHint {
					keys: self.dismiss_hint().keys,
					label: "cancel /omfg".to_string(),
				})],
				"hints",
			),
		}
	}
}

impl Component for OmfgPanelComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner = usize::from(width).saturating_sub(4) as u16;
		let (glyph, tone) = self.status_text();
		let status = if glyph.is_empty() {
			replace_tabs(&self.status)
		} else {
			format!("{glyph} {}", replace_tabs(&self.status))
		};
		let status = self.theme.fg(tone, &status).unwrap_or(status);
		let footer = self.footer_line();
		let footer = self.theme.fg(self.footer_tone(), &footer).unwrap_or(footer);

		let mut out = vec![top_border(width, &self.title(), &self.panel_theme), row(&status, width, &self.panel_theme)];
		for line in self.body_rows(inner) {
			out.push(row(&line, width, &self.panel_theme));
		}
		out.push(row(&footer, width, &self.panel_theme));
		out.push(bottom_border(width, &self.panel_theme));
		out
	}

	fn handle_input(&mut self, key: &Key) {
		if matches!(key, Key::Esc) {
			self.close();
		}
	}

	fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::{CreateThemeOptions, create_theme};

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin dark theme"),
			CreateThemeOptions::default(),
		)
		.expect("valid theme")
	}

	fn panel() -> OmfgPanelComponent {
		OmfgPanelComponent::new(
			theme(),
			KeybindingsManager::new(),
			OmfgPanelOptions { complaint: "stop\tshouting".to_string() },
		)
	}

	#[test]
	fn tabs_are_expanded_in_the_complaint_and_title() {
		let panel = panel();
		assert_eq!(panel.complaint(), "stop   shouting");
		assert_eq!(panel.title(), "/omfg stop   shouting");
	}

	#[test]
	fn live_states_are_the_working_ones() {
		assert!(OmfgPanelState::Generating.is_live());
		assert!(OmfgPanelState::Saving.is_live());
		assert!(!OmfgPanelState::Saved.is_live());
		assert!(!OmfgPanelState::Error.is_live());
	}

	#[test]
	fn saved_state_reports_the_registered_path() {
		let mut panel = panel();
		panel.mark_saved("rules/quiet.md");
		assert_eq!(panel.state(), OmfgPanelState::Saved);
		assert_eq!(panel.saved_path(), Some("rules/quiet.md"));
		assert_eq!(panel.status(), "Saved rules/quiet.md");
		let rows = panel.render(60);
		assert!(rows.iter().any(|row| row.contains("rules/quiet.md")));
	}

	#[test]
	fn error_state_shows_the_message_body() {
		let mut panel = panel();
		panel.mark_error("model refused");
		assert_eq!(panel.state(), OmfgPanelState::Error);
		assert_eq!(panel.error_message(), Some("model refused"));
		let rows = panel.render(60);
		assert!(rows.iter().any(|row| row.contains("model refused")));
		assert!(!rows.iter().any(|row| row.contains("Waiting for candidate rule")));
	}

	#[test]
	fn closed_panels_ignore_updates() {
		let mut panel = panel();
		panel.close();
		panel.append_draft("rule body");
		panel.mark_saved("rules/x.md");
		assert!(panel.is_closed());
		assert_eq!(panel.state(), OmfgPanelState::Generating);
		assert_eq!(panel.preview(), "");
	}

	#[test]
	fn drafts_stream_until_replaced() {
		let mut panel = panel();
		panel.append_draft("# Rule\nnever ");
		panel.append_draft("shout");
		assert_eq!(panel.preview(), "# Rule\nnever shout");
		panel.set_rule("replaced");
		assert_eq!(panel.preview(), "replaced");
	}

	#[test]
	fn escape_closes_the_panel() {
		let mut panel = panel();
		panel.handle_input(&Key::Esc);
		assert!(panel.is_closed());
	}
}
