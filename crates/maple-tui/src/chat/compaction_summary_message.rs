//! Compaction, handoff, and branch summary dividers.
//!
//! Reference: `l/src/chat/compaction-summary-message.ts`.
//!
//! Every history-collapse point renders as one slim banner — blank row, a rule
//! carrying the label and the expand hint, blank row — backed by a collapsible
//! markdown detail. The conversation itself stays visible; only the model
//! context was reset.
//!
//! Deviations:
//! - The crate's [`Disclosure`] is a title/body fold rather than a slot-based
//!   one, so the banner and the lazily built markdown box are composed here.
//! - `formatNumber` becomes [`format_tokens`] (uppercase `K`/`M`, one decimal).
//! - No `Memo`: the native description is rebuilt from the message on demand.

use crate::chat::messages::{
	BranchSummaryMessage, CompactionSummaryMessage, CustomMessage, CustomMessageContent,
};
use crate::components::markdown::Markdown;
use crate::component::Component;
use crate::native::describe::{md, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span};
use crate::render::render_utils::{expand_key_hint, replace_tabs};
use crate::text_shape::{visible_width, wrap_text_with_ansi};
use crate::theme::theme_class::Theme;
use crate::theme::tui_adapters::get_markdown_theme;

/// Divider labels per compaction method; unknown methods fall back to
/// `compacted`.
pub fn compaction_method_label(method: Option<&str>) -> &'static str {
	match method {
		Some("remote") => "remote-compacted",
		Some("soft") => "soft-compacted",
		Some("handoff") => "handed-off",
		Some("snapcompact") => "snap-compacted",
		Some("shake") => "shaken",
		_ => "compacted",
	}
}

/// `256K→20K` amount badge, or `None` when the entry predates `tokens_after`.
pub fn compaction_amount(message: &CompactionSummaryMessage) -> Option<String> {
	if message.tokens_before == 0 {
		return None;
	}
	let after = message.tokens_after?;
	Some(format!(
		"{}→{}",
		format_tokens(message.tokens_before),
		format_tokens(after)
	))
}

/// Compact token count: `400k`, `1.2m` shaped as `400K`/`1.2M`.
pub fn format_tokens(count: usize) -> String {
	if count >= 1_000_000 {
		let value = count as f64 / 1_000_000.0;
		if (value.fract() * 10.0).round() == 0.0 {
			format!("{}M", value.round() as u64)
		} else {
			format!("{value:.1}M")
		}
	} else if count >= 1_000 {
		let value = count as f64 / 1_000.0;
		if (value.fract() * 10.0).round() == 0.0 {
			format!("{}K", value.round() as u64)
		} else {
			format!("{value:.1}K")
		}
	} else {
		count.to_string()
	}
}

/// The banner label, e.g. `📷 soft-compacted · 256K→20K`.
pub fn compaction_label(message: &CompactionSummaryMessage, theme: &Theme) -> String {
	let camera = theme.symbol("icon.camera");
	let dot = theme.symbol("sep.dot");
	let mut label = format!("{camera} {}", compaction_method_label(message.method.as_deref()));
	if let Some(amount) = compaction_amount(message) {
		label.push_str(dot);
		label.push_str(&amount);
	}
	if message.warning.is_some() {
		let glyph = theme.status().warning.clone();
		let painted = theme.fg("warning", &glyph).unwrap_or(glyph);
		label.push(' ');
		label.push_str(&painted);
	}
	label
}

/// The expanded detail: the token line, a warning, then the summary.
pub fn compaction_detail_markdown(message: &CompactionSummaryMessage, theme: &Theme) -> String {
	let token_line = if message.tokens_before > 0 {
		match message.tokens_after {
			Some(after) => format!(
				"Compacted from {} to {} tokens",
				crate::overlays::model_browser::format_number(message.tokens_before as u64),
				crate::overlays::model_browser::format_number(after as u64)
			),
			None => format!(
				"Compacted from {} tokens",
				crate::overlays::model_browser::format_number(message.tokens_before as u64)
			),
		}
	} else {
		match message.tokens_after {
			Some(after) => format!(
				"Compacted to {} tokens",
				crate::overlays::model_browser::format_number(after as u64)
			),
			None => "Compacted context".to_string(),
		}
	};
	let frames = message.images.len();
	let frame_note = if frames > 0 {
		format!(
			"\n\n_{frames} snapcompact frame{} attached_",
			if frames == 1 { "" } else { "s" }
		)
	} else {
		String::new()
	};
	let warning_note = message
		.warning
		.as_ref()
		.map(|warning| {
			let glyph = theme.status().warning.clone();
			format!("\n\n{glyph} **Warning:** {warning}")
		})
		.unwrap_or_default();
	format!("**{token_line}**{warning_note}\n\n{}{frame_note}", message.summary)
}

/// The divider chip's native spans: `Soft-compacted · 19K → 6.9K tokens`.
pub fn compaction_native_label(message: &CompactionSummaryMessage) -> Vec<Span> {
	let name = compaction_method_label(message.method.as_deref());
	let mut title = name.chars();
	let capitalised = match title.next() {
		Some(first) => first.to_uppercase().collect::<String>() + title.as_str(),
		None => String::new(),
	};
	let mut spans = vec![span(capitalised, Some("muted"))];
	if message.tokens_after.is_some() && message.tokens_before > 0 {
		spans.push(span(
			format!(" · {}", format_tokens(message.tokens_before)),
			Some("muted mono"),
		));
		spans.push(span(" → ", Some("dim")));
		spans.push(span(
			format!("{} tokens", format_tokens(message.tokens_after.unwrap_or(0))),
			Some("muted mono"),
		));
	}
	spans
}

/// The expanded native detail: caption, warning, summary.
pub fn compaction_native_detail(message: &CompactionSummaryMessage, theme: &Theme) -> Vec<NativeNode> {
	let frames = message.images.len();
	let caption = if frames > 0 {
		format!(
			"Summary · {frames} snapcompact frame{} attached",
			if frames == 1 { "" } else { "s" }
		)
	} else {
		"Summary of the earlier conversation".to_string()
	};
	let mut props = Props::new();
	props.set_str("role", "omp.compaction.caption");
	let mut detail = vec![crate::native::describe::keyed(
		text(NodeText::Plain(caption), props),
		"caption",
	)];
	if let Some(warning) = &message.warning {
		let mut notice = Props::new();
		notice.set_tone(crate::native::node::Tone::Warning);
		notice.set_str("role", "omp.notice");
		detail.push(crate::native::describe::keyed(
			text(NodeText::Plain(warning.clone()), notice),
			"warning",
		));
	}
	detail.push(md(message.summary.clone(), Props::new()));
	let _ = theme;
	detail
}

/// A width-aware banner: blank, the labelled rule, blank.
pub struct DividerSummary {
	theme: Theme,
	label: String,
}

impl DividerSummary {
	/// A banner carrying `label`.
	pub fn new(label: impl Into<String>, theme: Theme) -> Self {
		DividerSummary {
			theme,
			label: label.into(),
		}
	}

	/// Replace the banner label.
	pub fn set_label(&mut self, label: impl Into<String>) {
		self.label = label.into();
	}

	/// The banner rows for `width`.
	pub fn rows(&self, width: usize) -> Vec<String> {
		let width = width.max(1);
		vec![
			String::new(),
			self.divider(width),
			String::new(),
		]
	}

	fn divider(&self, width: usize) -> String {
		let rule = {
			let rule = self.theme.symbol("tree.horizontal");
			if rule.is_empty() { "─" } else { rule }.to_string()
		};
		let dot = self.theme.symbol("sep.dot");
		let hint = format!("{} {}", dot.trim(), expand_key_hint());
		let plain_width = visible_width(&self.label) + 1 + visible_width(&hint);
		let remaining = width.saturating_sub(plain_width + 2);
		if remaining < 4 {
			return self
				.theme
				.fg("muted", &self.label)
				.unwrap_or_else(|_| self.label.clone());
		}
		let left = remaining / 2;
		let right = remaining - left;
		let dim = |text: &str| self.theme.fg("dim", text).unwrap_or_else(|_| text.to_string());
		let muted = self
			.theme
			.fg("muted", &self.label)
			.unwrap_or_else(|_| self.label.clone());
		format!("{} {muted} {} {}", dim(&rule.repeat(left)), dim(&hint), dim(&rule.repeat(right)))
	}
}

/// Shared collapse-point banner: the divider plus a lazily built detail box.
pub struct SummaryMessageComponent {
	theme: Theme,
	role: &'static str,
	divider: DividerSummary,
	label_spans: Vec<Span>,
	detail_markdown: String,
	native_detail: Option<Vec<NativeNode>>,
	expanded: bool,
	detail: Option<Markdown>,
}

impl SummaryMessageComponent {
	/// A banner over `label` showing `detail_markdown` when expanded.
	pub fn new(
		role: &'static str,
		label: impl Into<String>,
		label_spans: Vec<Span>,
		detail_markdown: impl Into<String>,
		theme: Theme,
	) -> Self {
		let label = label.into();
		SummaryMessageComponent {
			divider: DividerSummary::new(label, theme.clone()),
			theme,
			role,
			label_spans,
			detail_markdown: detail_markdown.into(),
			native_detail: None,
			expanded: false,
			detail: None,
		}
	}

	/// Replace the native detail body when it differs from the markdown.
	pub fn set_native_detail(&mut self, detail: Vec<NativeNode>) {
		self.native_detail = Some(detail);
	}

	/// Whether the detail is shown.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// Show or hide the detail.
	pub fn set_expanded(&mut self, expanded: bool) {
		self.expanded = expanded;
	}

	/// The markdown detail source.
	pub fn detail_markdown(&self) -> &str {
		&self.detail_markdown
	}

	fn detail(&mut self) -> &mut Markdown {
		let theme = &self.theme;
		self.detail.get_or_insert_with(|| {
			let mut markdown = Markdown::new();
			markdown.set_theme(get_markdown_theme(theme, true));
			markdown
		})
	}
}

impl Component for SummaryMessageComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let mut rows = self.divider.rows(width as usize);
		if self.expanded {
			let text = self.detail_markdown.clone();
			let markdown = self.detail();
			markdown.set_text(text);
			rows.push(String::new());
			rows.extend(markdown.render(width.saturating_sub(2)));
		}
		rows
	}

	fn invalidate(&mut self) {
		self.detail = None;
	}
}

impl crate::native::describe::NativeDescribe for SummaryMessageComponent {
	fn describe_native(&self, _context: &DescribeContext) -> NativeNode {
		let mut props = Props::new();
		props.set_str("role", self.role);
		props.set_head(NodeText::Spans(self.label_spans.clone()));
		props.set_bool("collapsible", true);
		props.set_bool("collapsed", !self.expanded);
		let children = self
			.native_detail
			.clone()
			.unwrap_or_else(|| vec![md(self.detail_markdown.clone(), Props::new())]);
		node(NodeKind::Section, props, children, None)
	}
}

/// The compaction point itself.
pub struct CompactionSummaryMessageComponent {
	inner: SummaryMessageComponent,
}

impl CompactionSummaryMessageComponent {
	/// The banner for `message`.
	pub fn new(message: CompactionSummaryMessage, theme: Theme) -> Self {
		let label = compaction_label(&message, &theme);
		let detail = compaction_detail_markdown(&message, &theme);
		let mut inner = SummaryMessageComponent::new(
			"omp.compaction",
			label,
			compaction_native_label(&message),
			detail,
			theme.clone(),
		);
		inner.set_native_detail(compaction_native_detail(&message, &theme));
		CompactionSummaryMessageComponent { inner }
	}

	/// The banner component.
	pub fn inner(&self) -> &SummaryMessageComponent {
		&self.inner
	}

	/// The banner component, mutably.
	pub fn inner_mut(&mut self) -> &mut SummaryMessageComponent {
		&mut self.inner
	}
}

impl Component for CompactionSummaryMessageComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.inner.render(width)
	}

	fn invalidate(&mut self) {
		self.inner.invalidate();
	}
}

impl crate::native::describe::NativeDescribe for CompactionSummaryMessageComponent {
	fn describe_native(&self, context: &DescribeContext) -> NativeNode {
		self.inner.describe_native(context)
	}
}

/// A handoff: compaction persisted as a custom message.
pub struct HandoffSummaryMessageComponent {
	inner: SummaryMessageComponent,
}

/// The handoff message type tag.
pub const HANDOFF_MESSAGE_TYPE: &str = "handoff";

/// Text of a custom message: its string body, or its text blocks joined.
pub fn get_custom_message_text(message: &CustomMessage) -> String {
	match &message.content {
		CustomMessageContent::Text(text) => text.clone(),
		CustomMessageContent::Blocks(blocks) => blocks
			.iter()
			.filter_map(|block| match block {
				crate::chat::messages::MessageBlock::Text(text) => Some(text.text.clone()),
				crate::chat::messages::MessageBlock::Image(_) => None,
			})
			.collect::<Vec<_>>()
			.join("\n"),
	}
}

/// The `<handoff-context>` body, or the whole trimmed text when untagged.
pub fn extract_handoff_document(text: &str) -> String {
	const OPEN: &str = "<handoff-context>";
	const CLOSE: &str = "</handoff-context>";
	match text.find(OPEN) {
		None => text.trim().to_string(),
		Some(open_index) => {
			let content_start = open_index + OPEN.len();
			let document = match text[content_start..].find(CLOSE) {
				Some(close) => text[content_start..content_start + close].to_string(),
				None => text[content_start..].to_string(),
			};
			document.trim().to_string()
		}
	}
}

impl HandoffSummaryMessageComponent {
	/// The banner for a `handoff` custom message.
	pub fn new(message: &CustomMessage, theme: Theme) -> Self {
		let context = theme.symbol("icon.context");
		let label = format!("{context} handed-off");
		let document = extract_handoff_document(&get_custom_message_text(message));
		let detail = if document.is_empty() {
			"**Handoff context**\n\n_No handoff content._".to_string()
		} else {
			format!("**Handoff context**\n\n{document}")
		};
		HandoffSummaryMessageComponent {
			inner: SummaryMessageComponent::new(
				"omp.handoff",
				label,
				vec![span("Handed off", Some("muted"))],
				detail,
				theme,
			),
		}
	}

	/// The banner component.
	pub fn inner(&self) -> &SummaryMessageComponent {
		&self.inner
	}

	/// The banner component, mutably.
	pub fn inner_mut(&mut self) -> &mut SummaryMessageComponent {
		&mut self.inner
	}
}

impl Component for HandoffSummaryMessageComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.inner.render(width)
	}

	fn invalidate(&mut self) {
		self.inner.invalidate();
	}
}

impl crate::native::describe::NativeDescribe for HandoffSummaryMessageComponent {
	fn describe_native(&self, context: &DescribeContext) -> NativeNode {
		self.inner.describe_native(context)
	}
}

/// The handoff banner for `message`, or `None` when it is not a displayed
/// handoff.
pub fn create_handoff_summary_message_component(
	message: &CustomMessage,
	expanded: bool,
	theme: Theme,
) -> Option<HandoffSummaryMessageComponent> {
	if message.custom_type != HANDOFF_MESSAGE_TYPE || !message.display {
		return None;
	}
	let mut component = HandoffSummaryMessageComponent::new(message, theme);
	component.inner_mut().set_expanded(expanded);
	Some(component)
}

/// A side-branch collapse, rendered with the same banner.
pub struct BranchSummaryMessageComponent {
	inner: SummaryMessageComponent,
}

impl BranchSummaryMessageComponent {
	/// The banner for `message`.
	pub fn new(message: BranchSummaryMessage, theme: Theme) -> Self {
		let branch = theme.symbol("icon.branch");
		let label = format!("{branch} branch");
		let detail = format!("**Branch summary**\n\n{}", message.summary);
		BranchSummaryMessageComponent {
			inner: SummaryMessageComponent::new(
				"omp.branch",
				label,
				vec![span("Branch summarized", Some("muted"))],
				detail,
				theme,
			),
		}
	}

	/// The banner component.
	pub fn inner(&self) -> &SummaryMessageComponent {
		&self.inner
	}

	/// The banner component, mutably.
	pub fn inner_mut(&mut self) -> &mut SummaryMessageComponent {
		&mut self.inner
	}
}

impl Component for BranchSummaryMessageComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.inner.render(width)
	}

	fn invalidate(&mut self) {
		self.inner.invalidate();
	}
}

impl crate::native::describe::NativeDescribe for BranchSummaryMessageComponent {
	fn describe_native(&self, context: &DescribeContext) -> NativeNode {
		self.inner.describe_native(context)
	}
}

/// Wrap `label` to `width`, used by hosts that draw the banner themselves.
pub fn wrap_label(label: &str, width: usize) -> Vec<String> {
	wrap_text_with_ansi(&replace_tabs(label), width)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::native::describe::NativeDescribe;
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

	fn compaction() -> CompactionSummaryMessage {
		CompactionSummaryMessage {
			summary: "earlier turns".to_string(),
			tokens_before: 256_000,
			tokens_after: Some(20_000),
			method: Some("soft".to_string()),
			..CompactionSummaryMessage::default()
		}
	}

	#[test]
	fn labels_name_method_and_amounts() {
		assert_eq!(compaction_method_label(Some("soft")), "soft-compacted");
		assert_eq!(compaction_method_label(None), "compacted");
		assert_eq!(format_tokens(256_000), "256K");
		assert_eq!(format_tokens(6_900), "6.9K");
		assert_eq!(compaction_amount(&compaction()).unwrap(), "256K→20K");
		let mut missing = compaction();
		missing.tokens_after = None;
		assert!(compaction_amount(&missing).is_none());
	}

	#[test]
	fn divider_frames_the_label_and_falls_back_when_narrow() {
		let banner = DividerSummary::new("soft-compacted", theme());
		let rows = banner.rows(80);
		assert_eq!(rows.len(), 3);
		assert!(rows[0].is_empty() && rows[2].is_empty());
		let plain = crate::ansi_scan::strip_ansi(&rows[1]);
		assert!(plain.contains("soft-compacted"), "{plain}");
		let narrow = banner.rows(6);
		assert!(crate::ansi_scan::strip_ansi(&narrow[1]).contains("soft-compacted"));
	}

	#[test]
	fn expanded_detail_shows_the_summary() {
		let mut component = CompactionSummaryMessageComponent::new(compaction(), theme());
		let collapsed = component.render(60);
		assert_eq!(collapsed.len(), 3);
		component.inner_mut().set_expanded(true);
		let expanded = component.render(60);
		let plain = crate::ansi_scan::strip_ansi(&expanded.concat());
		assert!(plain.contains("earlier turns"), "{plain}");
		assert!(plain.contains("Compacted from"), "{plain}");
	}

	#[test]
	fn handoff_extracts_the_tagged_document() {
		let text = "pre <handoff-context>\n  carried over\n</handoff-context> post";
		assert_eq!(extract_handoff_document(text), "carried over");
		assert_eq!(extract_handoff_document("plain"), "plain");
		let message = CustomMessage {
			custom_type: HANDOFF_MESSAGE_TYPE.to_string(),
			content: CustomMessageContent::Text(text.to_string()),
			display: true,
			..CustomMessage::default()
		};
		let component = create_handoff_summary_message_component(&message, true, theme()).unwrap();
		assert_eq!(component.inner().expanded(), true);
		let mut hidden = message.clone();
		hidden.display = false;
		assert!(create_handoff_summary_message_component(&hidden, false, theme()).is_none());
	}

	#[test]
	fn native_sections_report_their_role() {
		let component = CompactionSummaryMessageComponent::new(compaction(), theme());
		let described = component.describe_native(&DescribeContext::new(80));
		assert_eq!(described.kind, NodeKind::Section);
		assert_eq!(described.props.get_str("role"), Some("omp.compaction"));
		assert_eq!(
			described.props.get_bool("collapsed"),
			Some(true),
			"the detail starts folded"
		);
	}
}
