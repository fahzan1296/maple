//! Component for a custom message entry from an extension.
//!
//! Reference: `l/src/chat/custom-message.ts`.
//!
//! Distinct styling separates injected messages from user messages: hooks get
//! the hook glyph, live delegations hide their header and use the accent border,
//! everything else a neutral package icon.
//!
//! Deviations: `FramedMessageComponent` has no port, so the row is a [`Message`]
//! body under a styled header row, and the crate's `CustomMessage` carries no
//! `role` field — the host says whether the entry is a legacy hook message.

use crate::chat::extension_types::{MessageRenderOptions, MessageRenderer};
use crate::chat::messages::{
	CustomMessage, CustomMessageContent, LIVE_DELEGATION_MESSAGE_TYPE, SKILL_PROMPT_MESSAGE_TYPE,
};
use crate::chrome::message::{Message, MessageRole};
use crate::component::Component;
use crate::native::describe::{node, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::theme::Theme;

/// Lines of body shown before the "…" fold when collapsed.
const CUSTOM_COLLAPSED_LINES: usize = 8;

/// Which wire role a custom message arrived on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CustomMessageRole {
	/// An injected extension message.
	#[default]
	Custom,
	/// A legacy `hookMessage` entry.
	Hook,
}

/// The transcript row for an extension custom message.
pub struct CustomMessageComponent {
	theme: Theme,
	message: CustomMessage,
	custom_renderer: Option<MessageRenderer>,
	role: CustomMessageRole,
	expanded: bool,
	body: Message,
	lines: Vec<String>,
}

impl CustomMessageComponent {
	/// Build the row for `message`, deferring to `custom_renderer` when given.
	pub fn new(message: CustomMessage, custom_renderer: Option<MessageRenderer>, theme: Theme) -> Self {
		let lines = message_body_lines(&message);
		let mut body = Message::new(MessageRole::Info);
		body.set_avatar(Self::icon(&message, CustomMessageRole::Custom));
		body.set_text(lines.join("\n"));
		body.set_copy_text(Some(lines.join("\n")));
		CustomMessageComponent {
			theme,
			message,
			custom_renderer,
			role: CustomMessageRole::Custom,
			expanded: false,
			body,
			lines,
		}
	}

	/// Mark this entry as a legacy hook message (hook glyph, `omp.hook` role).
	pub fn with_hook_role(mut self) -> Self {
		self.role = CustomMessageRole::Hook;
		self.body
			.set_avatar(Self::icon(&self.message, CustomMessageRole::Hook));
		self
	}

	/// Whether the entry is a live delegation (`/tan` foreground handoff).
	pub fn is_live_delegation(&self) -> bool {
		self.message.custom_type == LIVE_DELEGATION_MESSAGE_TYPE
	}

	/// The native role this entry reports.
	pub fn native_role(&self) -> &'static str {
		if self.role == CustomMessageRole::Hook {
			"omp.hook"
		} else if self.is_live_delegation() {
			"omp.custom.delegation"
		} else {
			"omp.custom"
		}
	}

	/// The message.
	pub fn message(&self) -> &CustomMessage {
		&self.message
	}

	/// Whether the body is expanded.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// Set the expanded state.
	pub fn set_expanded(&mut self, expanded: bool) {
		self.expanded = expanded;
	}

	/// Ask the extension renderer for a replacement component.
	pub fn render_custom(&self) -> Option<Box<dyn Component>> {
		let renderer = self.custom_renderer.as_ref()?;
		renderer(
			&self.message,
			MessageRenderOptions {
				expanded: self.expanded,
			},
			&self.theme,
		)
	}

	/// The header row for this entry, styled as the frame would.
	pub fn header(&self) -> String {
		if self.is_live_delegation() {
			return String::new();
		}
		let label = if self.role == CustomMessageRole::Hook {
			"Hook message"
		} else if self.message.custom_type == SKILL_PROMPT_MESSAGE_TYPE {
			"Skill prompt"
		} else {
			"Custom message"
		};
		let title = self.theme.bold(label);
		self.theme
			.fg("customMessageLabel", &title)
			.unwrap_or(title)
	}

	fn icon(message: &CustomMessage, role: CustomMessageRole) -> String {
		if role == CustomMessageRole::Hook {
			"⚓".to_string()
		} else if message.custom_type == LIVE_DELEGATION_MESSAGE_TYPE {
			"⇢".to_string()
		} else {
			"◻".to_string()
		}
	}

	fn body_text(&self) -> String {
		let hidden = self.lines.len().saturating_sub(CUSTOM_COLLAPSED_LINES);
		if self.expanded || hidden == 0 {
			return self.lines.join("\n");
		}
		let mut shown: Vec<String> = self.lines[..CUSTOM_COLLAPSED_LINES].to_vec();
		shown.push(format!("… +{hidden} more lines"));
		shown.join("\n")
	}
}

/// The message body as rows: text lines, or the text blocks of a block body.
pub fn message_body_lines(message: &CustomMessage) -> Vec<String> {
	match &message.content {
		CustomMessageContent::Text(text) => text.lines().map(str::to_string).collect(),
		CustomMessageContent::Blocks(blocks) => blocks
			.iter()
			.filter_map(|block| match block {
				crate::chat::messages::MessageBlock::Text(text) => Some(text.text.clone()),
				crate::chat::messages::MessageBlock::Image(_) => None,
			})
			.collect(),
	}
}

impl Component for CustomMessageComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.body.set_text(self.body_text());
		let mut rows = self.body.render(width);
		let header = self.header();
		if !header.is_empty() {
			let icon = Self::icon(&self.message, self.role);
			rows.insert(0, format!("{icon} {header}"));
		}
		rows
	}

	fn invalidate(&mut self) {
		self.body.invalidate();
	}
}

impl crate::native::describe::NativeDescribe for CustomMessageComponent {
	fn describe_native(&self, _context: &DescribeContext) -> NativeNode {
		let mut props = Props::new();
		props.set_str("role", self.native_role());
		props.set_str("title", self.header());
		props.set_str("customType", self.message.custom_type.as_str());
		props.set_bool("hideHeader", self.is_live_delegation());
		let mut body_props = Props::new();
		body_props.set_str("wrap", "word");
		let body = text(NodeText::Plain(self.body_text()), body_props);
		node(NodeKind::Col, props, vec![body], None)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::messages::{CustomMessageContent, MessageAttribution};
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

	fn custom(custom_type: &str, content: &str) -> CustomMessage {
		CustomMessage {
			custom_type: custom_type.to_string(),
			content: CustomMessageContent::Text(content.to_string()),
			display: true,
			details: None,
			attribution: MessageAttribution::Agent,
			timestamp: 0,
		}
	}

	#[test]
	fn headers_name_hooks_and_skills() {
		let hook = CustomMessageComponent::new(custom("pre-tool", "x"), None, theme()).with_hook_role();
		assert_eq!(hook.native_role(), "omp.hook");
		let skill = CustomMessageComponent::new(custom(SKILL_PROMPT_MESSAGE_TYPE, "x"), None, theme());
		assert!(crate::ansi_scan::strip_ansi(&skill.header()).contains("Skill prompt"));
	}

	#[test]
	fn live_delegations_hide_the_header() {
		let delegation = CustomMessageComponent::new(
			custom(LIVE_DELEGATION_MESSAGE_TYPE, "handoff"),
			None,
			theme(),
		);
		assert!(delegation.is_live_delegation());
		assert!(delegation.header().is_empty());
		assert_eq!(delegation.native_role(), "omp.custom.delegation");
		let mut delegation = delegation;
		let rows = delegation.render(40);
		assert!(crate::ansi_scan::strip_ansi(&rows.concat()).contains("handoff"));
	}

	#[test]
	fn long_bodies_fold_and_expand() {
		let body = (0..(CUSTOM_COLLAPSED_LINES + 4))
			.map(|index| format!("line{index}"))
			.collect::<Vec<_>>()
			.join("\n");
		let mut component = CustomMessageComponent::new(custom("note", &body), None, theme());
		let rows = component.render(60);
		assert!(crate::ansi_scan::strip_ansi(&rows.concat()).contains("more lines"));
		component.set_expanded(true);
		let plain = crate::ansi_scan::strip_ansi(&component.render(60).concat());
		assert!(plain.contains(&format!("line{}", CUSTOM_COLLAPSED_LINES + 3)));
	}

	#[test]
	fn renderer_is_optional() {
		let component = CustomMessageComponent::new(custom("note", "x"), None, theme());
		assert!(component.render_custom().is_none());
	}
}
