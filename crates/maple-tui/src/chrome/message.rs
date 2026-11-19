//! Reference: `l/src/chrome/message-frame.ts`, `l/src/chrome/message-divider.ts`, `l/src/chrome/message-notice.ts`.
//!
//! Ported from `l/src/chrome/message-frame.ts`, `message-divider.ts`, and
//! `message-notice.ts`. pi-tui renders user/assistant bubbles with Markdown and
//! themed backgrounds; this module keeps the bounded row contract (avatar
//! gutter plus wrapped text, every row clamped to the viewport width) and the
//! presentation policy (role colors, labels, notice tone).

use crate::chrome::transcript_container::CompactTranscriptBlock;
use crate::component::Component;
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{card, col, describe_child, md, node, row, span, text, with_hidden};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text as NodeText};
use crate::native::state::Memo;
use crate::native::tone::color_tone_token;
use crate::style::{Color, Style};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};
use crate::theme::theme_class::Theme;

/// Default maximum rule length of a labelled divider.
pub const DEFAULT_RULE_WIDTH: usize = 10;

/// Default semantic role of a divider's native node.
pub const DEFAULT_DIVIDER_ROLE: &str = "omp.divider";

/// Semantic role of a transcript message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageRole {
    /// Input from the user.
    User,
    /// Output from the assistant.
    Assistant,
    /// Harness/system text.
    System,
    /// Failure text.
    Error,
    /// Caution text.
    Warning,
    /// Confirmation text.
    Success,
    /// Neutral informational text.
    Info,
}

impl MessageRole {
    /// Lower-case role label used in tests and accessibility text.
    pub fn label(self) -> &'static str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Error => "error",
            MessageRole::Warning => "warning",
            MessageRole::Success => "success",
            MessageRole::Info => "info",
        }
    }
}

/// Theme for message chrome: per-role avatar paints plus shared text styles.
#[derive(Clone, Debug)]
pub struct MessageTheme {
    /// Avatar column width in cells.
    pub avatar_width: usize,
    /// User avatar paint.
    pub user: Style,
    /// Assistant avatar paint.
    pub assistant: Style,
    /// System avatar paint.
    pub system: Style,
    /// Error avatar paint.
    pub error: Style,
    /// Warning avatar paint.
    pub warning: Style,
    /// Success avatar paint.
    pub success: Style,
    /// Info avatar paint.
    pub info: Style,
    /// Message body paint.
    pub text: Style,
    /// Secondary label paint.
    pub meta: Style,
    /// Copy affordance paint.
    pub copy_hint: Style,
    /// Divider rule paint.
    pub divider: Style,
}

impl Default for MessageTheme {
    fn default() -> Self {
        MessageTheme {
            avatar_width: 2,
            user: Style::fg(Color::White),
            assistant: Style::fg(Color::Cyan),
            system: Style::fg(Color::DarkGray).italic(),
            error: Style::fg(Color::Red),
            warning: Style::fg(Color::Yellow),
            success: Style::fg(Color::Green),
            info: Style::fg(Color::Blue),
            text: Style::default(),
            meta: Style::fg(Color::DarkGray),
            copy_hint: Style::fg(Color::DarkGray),
            divider: Style::fg(Color::DarkGray),
        }
    }
}

impl MessageTheme {
    /// The avatar paint for `role`.
    pub fn style_for(&self, role: MessageRole) -> Style {
        match role {
            MessageRole::User => self.user,
            MessageRole::Assistant => self.assistant,
            MessageRole::System => self.system,
            MessageRole::Error => self.error,
            MessageRole::Warning => self.warning,
            MessageRole::Success => self.success,
            MessageRole::Info => self.info,
        }
    }
}

/// A role-tagged transcript message with an avatar gutter and wrapped text.
pub struct Message {
    role: MessageRole,
    avatar: String,
    text: String,
    copy_text: Option<String>,
    on_copy: Option<Box<dyn FnMut() + 'static>>,
    theme: MessageTheme,
    memo: Option<(u16, Vec<String>)>,
}

impl Message {
    /// A message with `role`, empty avatar, and empty body.
    pub fn new(role: MessageRole) -> Self {
        Message {
            role,
            avatar: String::new(),
            text: String::new(),
            copy_text: None,
            on_copy: None,
            theme: MessageTheme::default(),
            memo: None,
        }
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: MessageTheme) {
        self.theme = theme;
        self.memo = None;
    }

    /// The message theme.
    pub fn theme(&self) -> &MessageTheme {
        &self.theme
    }

    /// The message role.
    pub fn role(&self) -> MessageRole {
        self.role
    }

    /// Set the avatar glyph (one or two cells).
    pub fn set_avatar(&mut self, avatar: impl Into<String>) {
        self.avatar = avatar.into();
        self.memo = None;
    }

    /// The avatar glyph.
    pub fn avatar(&self) -> &str {
        &self.avatar
    }

    /// Replace the message body.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.memo = None;
    }

    /// The message body.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text a copy action would place on the clipboard.
    pub fn copy_text(&self) -> Option<&str> {
        self.copy_text.as_deref()
    }

    /// Set the text a copy action would place on the clipboard.
    pub fn set_copy_text(&mut self, text: Option<String>) {
        self.copy_text = text;
    }

    /// Register the copy callback, fired by [`Message::request_copy`] and by
    /// `Ctrl+C` while the message has focus.
    pub fn on_copy(&mut self, callback: impl FnMut() + 'static) {
        self.on_copy = Some(Box::new(callback));
    }

    /// Fire the copy callback when a copy payload exists.
    pub fn request_copy(&mut self) -> bool {
        if self.copy_text.is_none() {
            return false;
        }
        if let Some(callback) = self.on_copy.as_mut() {
            callback();
        }
        true
    }

    /// The copy affordance suffix, or `None` when the message has no payload.
    pub fn copy_hint(&self) -> Option<String> {
        self.copy_text
            .as_ref()
            .map(|_| self.theme.copy_hint.paint("copy"))
    }

    fn rows(&self, width: u16) -> Vec<String> {
        let width = width as usize;
        let gutter = self.theme.avatar_width.min(width / 2);
        let content_width = width.saturating_sub(gutter + 1).max(1);
        let avatar = pad_to_width(
            &truncate_to_width(&self.avatar, gutter, Ellipsis::Omit),
            gutter,
        );
        let wrapped = crate::text_shape::wrap_text_with_ansi(&self.text, content_width);
        let mut rows = Vec::with_capacity(wrapped.len());
        for (index, line) in wrapped.iter().enumerate() {
            let mut row = String::new();
            if index == 0 {
                row.push_str(&self.theme.style_for(self.role).paint(&avatar));
                row.push(' ');
            } else {
                row.push_str(&" ".repeat(gutter + 1));
            }
            row.push_str(&self.theme.text.paint(line));
            rows.push(row);
        }
        rows
    }
}

impl Component for Message {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return Vec::new();
        }
        if let Some((memo_width, rows)) = &self.memo {
            if *memo_width == width {
                return rows.clone();
            }
        }
        let rows: Vec<String> = self
            .rows(width)
            .into_iter()
            .map(|row| truncate_to_width(&row, width as usize, Ellipsis::Omit))
            .collect();
        self.memo = Some((width, rows.clone()));
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        if crate::keys::is_ctrl_c(key) {
            self.request_copy();
        }
    }

    fn invalidate(&mut self) {
        self.memo = None;
    }
}

/// A short left-aligned rule and label surrounded by blank transcript rows.
///
/// Reference: `MessageDividerComponent` in `l/src/chrome/message-divider.ts`.
/// The theme-aware `label` closure of the reference is a plain string plus
/// [`MessageDivider::set_label`], or a live label through
/// [`MessageDivider::set_label_fn`].
pub struct MessageDivider {
    label: String,
    label_style: Style,
    rule_style: Style,
    rule_width: usize,
    truncate_when_narrow: bool,
    inline_icon: Option<String>,
    role: String,
    memo: Option<(u16, Vec<String>)>,
    label_fn: Option<Box<dyn Fn() -> String>>,
    /// Theme colour token for the native label span (`labelColor`).
    label_color: Option<String>,
}

impl MessageDivider {
    /// A divider labelled `label` with the default rule width of 10.
    pub fn new(label: impl Into<String>) -> Self {
        MessageDivider {
            label: label.into(),
            label_style: Style::fg(Color::DarkGray),
            rule_style: Style::fg(Color::DarkGray),
            rule_width: DEFAULT_RULE_WIDTH,
            truncate_when_narrow: true,
            inline_icon: None,
            label_fn: None,
            label_color: None,
            role: DEFAULT_DIVIDER_ROLE.to_string(),
            memo: None,
        }
    }

    /// Set the label paint.
    pub fn set_label_style(&mut self, style: Style) {
        self.label_style = style;
        self.memo = None;
    }

    /// Set the rule paint.
    pub fn set_rule_style(&mut self, style: Style) {
        self.rule_style = style;
        self.memo = None;
    }

    /// Set the maximum rule length.
    pub fn set_rule_width(&mut self, width: usize) {
        self.rule_width = width;
        self.memo = None;
    }

    /// Replace the label.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
        self.memo = None;
    }

    /// The configured label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Preserve an intentionally untruncated legacy label below one rule cell
    /// (`false`), or clamp it to the viewport (`true`, the default).
    pub fn set_truncate_when_narrow(&mut self, truncate: bool) {
        self.truncate_when_narrow = truncate;
        self.memo = None;
    }

    /// Whether a narrow row keeps an untruncated label.
    pub fn truncate_when_narrow(&self) -> bool {
        self.truncate_when_narrow
    }

    /// Draw as an inline notice with this icon instead of a labelled rule.
    pub fn set_inline_icon(&mut self, icon: Option<impl Into<String>>) {
        self.inline_icon = icon.map(Into::into);
        self.memo = None;
    }

    /// The semantic role of the native node.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Set the semantic role of the native node.
    pub fn set_role(&mut self, role: impl Into<String>) {
        self.role = role.into();
        self.memo = None;
    }

    /// Draw a live label, re-read on every render (e.g. a spinner).
    pub fn set_label_fn(&mut self, label: Option<Box<dyn Fn() -> String>>) {
        self.label_fn = label;
        self.memo = None;
    }

    /// Theme colour token of the native label span (`labelColor`).
    pub fn set_label_color(&mut self, color: Option<impl Into<String>>) {
        self.label_color = color.map(Into::into);
        self.memo = None;
    }

    /// The label as drawn: the live closure when set, else [`Self::label`].
    fn resolved_label(&self) -> String {
        match &self.label_fn {
            Some(label) => label(),
            None => self.label.clone(),
        }
    }

    fn divider_row(&self, width: u16) -> String {
        let width = width as usize;
        let label = self.label_style.paint(&self.resolved_label());
        let label_width = visible_width(&label);
        let rule_width = self.rule_width.min(width.saturating_sub(label_width + 1));
        if rule_width < 1 {
            // `truncate_when_narrow: false` preserves an intentionally
            // untruncated legacy label, as the reference allows; the component
            // contract otherwise forbids overflowing the viewport.
            return if self.truncate_when_narrow {
                truncate_to_width(&label, width, Ellipsis::Omit)
            } else {
                label
            };
        }
        let rule = self.rule_style.paint(&"─".repeat(rule_width));
        truncate_to_width(&format!("{rule} {label}"), width, Ellipsis::Omit)
    }

    /// The native form: an inline `row` (icon and one-line label) or a
    /// labelled `rule`.
    pub fn describe(&self) -> NativeNode {
        let label = self.resolved_label();
        let style = self.label_color.as_deref();
        if let Some(icon) = &self.inline_icon {
            let mut row_props = Props::new();
            row_props.set_str("gap", "sm");
            row_props.set_str("role", self.role.as_str());
            let mut icon_props = Props::new();
            icon_props.set_str("name", icon);
            let icon_node = node(NodeKind::Icon, icon_props, Vec::new(), None);
            let mut text_props = Props::new();
            text_props.set_str("truncate", "end");
            let label_node = text(NodeText::Spans(vec![span(label, style)]), text_props);
            return row(vec![icon_node, label_node], row_props);
        }
        let mut props = Props::new();
        props.set_str("role", self.role.as_str());
        props.insert(
            "label",
            serde_json::Value::Array(vec![span(label, style).to_value()]),
        );
        node(NodeKind::Rule, props, Vec::new(), None)
    }
}

impl Component for MessageDivider {
    fn render(&mut self, width: u16) -> Vec<String> {
        // A live label changes without invalidation, so it bypasses the memo.
        if self.label_fn.is_none() {
            if let Some((memo_width, rows)) = &self.memo {
                if *memo_width == width {
                    return rows.clone();
                }
            }
        }
        let rows = vec![
            String::new(),
            self.divider_row(width),
            String::new(),
        ];
        self.memo = Some((width, rows.clone()));
        rows
    }

    fn invalidate(&mut self) {
        self.memo = None;
    }
}

/// Theme for the notice shell.
#[derive(Clone, Debug)]
pub struct NoticeTheme {
    /// Header paint (defaults to warning).
    pub header: Style,
    /// Body paint.
    pub body: Style,
    /// Optional background wash for the notice card.
    pub background: Option<Style>,
}

impl Default for NoticeTheme {
    fn default() -> Self {
        NoticeTheme {
            header: Style::fg(Color::Yellow),
            body: Style::default(),
            background: None,
        }
    }
}

/// A toned notice: a blank row, a header line, a blank row, then body rows.
///
/// Reference: `MessageNoticeComponent` in `l/src/chrome/message-notice.ts`.
/// The presentation closure is the `header`/`icon`/`body` fields plus
/// [`MessageNotice::refresh`]; the reference's component-typed body is rows
/// here, and its `nativePresentation` fields map onto the native `describe`
/// below.
pub struct MessageNotice {
    header: String,
    icon: Option<String>,
    body: Vec<String>,
    leading_space: bool,
    expanded: bool,
    tool_activity_visible: bool,
    preview_lines: Option<usize>,
    inline_icon: Option<String>,
    role: String,
    severity: String,
    theme: NoticeTheme,
    memo: Option<(u16, Vec<String>)>,
    native: Memo<NativeNode>,
}

impl MessageNotice {
    /// A notice with `header` and no body.
    pub fn new(header: impl Into<String>) -> Self {
        MessageNotice {
            header: header.into(),
            icon: None,
            body: Vec::new(),
            leading_space: true,
            expanded: false,
            tool_activity_visible: true,
            preview_lines: None,
            inline_icon: None,
            role: "omp.notice".to_string(),
            severity: "warning".to_string(),
            theme: NoticeTheme::default(),
            memo: None,
            native: Memo::new(),
        }
    }

    /// Replace the notice theme.
    pub fn set_theme(&mut self, theme: NoticeTheme) {
        self.theme = theme;
        self.refresh();
    }

    /// The notice theme.
    pub fn theme(&self) -> &NoticeTheme {
        &self.theme
    }

    /// Prefix the header with an icon glyph.
    pub fn set_icon(&mut self, icon: Option<impl Into<String>>) {
        self.icon = icon.map(Into::into);
        self.refresh();
    }

    /// Replace the body rows.
    pub fn set_body(&mut self, body: &[String]) {
        self.body = body.to_vec();
        self.refresh();
    }

    /// Drop the leading blank row.
    pub fn set_leading_space(&mut self, leading: bool) {
        self.leading_space = leading;
        self.refresh();
    }


    /// The header text.
    pub fn header(&self) -> &str {
        &self.header
    }

    /// Replace the header text.
    pub fn set_header(&mut self, header: impl Into<String>) {
        self.header = header.into();
        self.refresh();
    }

    /// Set the expanded state of the body.
    pub fn set_expanded(&mut self, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.native.clear();
    }

    /// Whether the body is expanded.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Hide the whole notice while tool activity is hidden.
    pub fn set_tool_activity_visible(&mut self, visible: bool) {
        self.tool_activity_visible = visible;
        self.memo = None;
        self.native.clear();
    }

    /// Whether the notice is drawn while tool activity is hidden.
    pub fn tool_activity_visible(&self) -> bool {
        self.tool_activity_visible
    }

    /// Body rows shown while collapsed; `None` keeps the notice uncollapsible
    /// natively.
    pub fn set_preview_lines(&mut self, lines: Option<usize>) {
        self.preview_lines = lines;
        self.native.clear();
    }

    /// The semantic role of the native card.
    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn set_role(&mut self, role: impl Into<String>) {
        self.role = role.into();
        self.native.clear();
    }

    /// The severity tone token of the native card.
    pub fn severity(&self) -> &str {
        &self.severity
    }

    pub fn set_severity(&mut self, severity: impl Into<String>) {
        self.severity = severity.into();
        self.native.clear();
    }

    /// Rebuild the presentation after controller-owned state changes.
    pub fn refresh(&mut self) {
        self.memo = None;
        self.native.clear();
    }

    /// Apply a native toggle aimed at this notice's card.
    pub fn handle_native_event(&mut self, event: &NativeUiEvent) -> bool {
        match root_toggle_expanded(event) {
            Some(expanded) => {
                self.set_expanded(expanded);
                true
            }
            None => false,
        }
    }

    /// Draw as an inline notice: a named icon and one line, the body a
    /// disclosure below it, instead of a toned card.
    pub fn set_inline_icon(&mut self, icon: Option<impl Into<String>>) {
        self.inline_icon = icon.map(Into::into);
        self.native.clear();
    }

    /// The inline notice icon, if set.
    pub fn inline_icon(&self) -> Option<&str> {
        self.inline_icon.as_deref()
    }

    /// The native form: an inline `section` when [`MessageNotice::inline_icon`]
    /// is set, else a severity-toned `card` (collapsible through
    /// [`MessageNotice::preview_lines`]); both hide with tool activity.
    pub fn describe(&mut self) -> NativeNode {
        let deps = vec![
            serde_json::json!(self.expanded),
            serde_json::json!(self.tool_activity_visible),
            serde_json::json!(self.preview_lines),
            serde_json::json!(self.severity),
            serde_json::json!(self.body.len()),
            serde_json::json!(self.header),
            serde_json::json!(self.inline_icon),
        ];
        let role = self.role.clone();
        let severity = self.severity.clone();
        let tool_activity_visible = self.tool_activity_visible;
        let expanded = self.expanded;
        let preview_lines = self.preview_lines;
        let inline_icon = self.inline_icon.clone();
        let header = self.header.clone();
        let body = self.body.clone();
        self.native
            .get(&deps, || {
                let tone = color_tone_token(&severity);
                if let Some(icon) = inline_icon {
                    let mut head_props = Props::new();
                    head_props.set_str("gap", "sm");
                    let mut label_props = Props::new();
                    label_props.set_str("truncate", "end");
                    let head = node(
                        NodeKind::Row,
                        head_props,
                        vec![
                            node(
                                NodeKind::Icon,
                                Props::new().set("name", icon),
                                Vec::new(),
                                None,
                            ),
                            text(NodeText::Plain(header), label_props),
                        ],
                        Some("head"),
                    );
                    let mut props = Props::new();
                    props.set_str("role", role);
                    if let Some(tone) = tone {
                        props.set_tone(tone);
                    }
                    props.set_bool("collapsible", !body.is_empty());
                    if !body.is_empty() {
                        props.set_bool("collapsed", !expanded);
                    }
                    let mut children = vec![head];
                    if !body.is_empty() {
                        children.push(md(body.join("\n"), Props::new()));
                    }
                    return with_hidden(
                        node(NodeKind::Section, props, children, None),
                        !tool_activity_visible,
                    );
                }
                let mut props = Props::new();
                props.set_str("role", role);
                if let Some(tone) = tone {
                    props.set_tone(tone);
                }
                props.set_head(NodeText::Spans(vec![span(header, Some("customMessageLabel strong"))]));
                if let Some(lines) = preview_lines {
                    props.set_bool("collapsible", true);
                    props.set_bool("collapsed", !expanded);
                    props.insert("preview", serde_json::json!({ "lines": lines }));
                }
                let children: Vec<NativeNode> = if body.is_empty() {
                    Vec::new()
                } else {
                    vec![md(body.join("\n"), Props::new())]
                };
                with_hidden(card(props, children), !tool_activity_visible)
            })
            .clone()
    }

    /// Release the notice's cached presentation.
    pub fn dispose(&mut self) {
        self.memo = None;
        self.native.clear();
    }
}

impl Component for MessageNotice {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return Vec::new();
        }
        if let Some((memo_width, rows)) = &self.memo {
            if *memo_width == width {
                return rows.clone();
            }
        }
        if !self.tool_activity_visible {
            self.memo = None;
            return Vec::new();
        }
        let mut rows = Vec::new();
        if self.leading_space {
            rows.push(String::new());
        }
        let header = match &self.icon {
            Some(icon) => format!("{icon} {}", self.header),
            None => self.header.clone(),
        };
        rows.push(self.theme.header.paint(&truncate_to_width(
            &header,
            width as usize,
            Ellipsis::Omit,
        )));
        if !self.body.is_empty() {
            rows.push(String::new());
            for line in &self.body {
                let line = truncate_to_width(line, width as usize, Ellipsis::Omit);
                rows.push(self.theme.body.paint(&line));
            }
        }
        if let Some(background) = self.theme.background {
            for row in &mut rows {
                *row = background.paint(&pad_to_width(row, width as usize));
            }
        }
        self.memo = Some((width, rows.clone()));
        rows
    }

    fn invalidate(&mut self) {
        self.memo = None;
    }

    fn dispose(&mut self) {
        MessageNotice::dispose(self);
    }
}

/// Expansion a native toggle asks for when it targets a card root: an empty
/// keypath means the root, and the toggle reports the collapsed state.
///
/// The reference's `rootToggleExpanded` helper is not in this crate's native
/// layer, so the rule lives here.
pub fn root_toggle_expanded(event: &NativeUiEvent) -> Option<bool> {
	match event {
		NativeUiEvent::Toggle { key, collapsed } if key.is_empty() => Some(!collapsed),
		_ => None,
	}
}

/// Message shape consumed by the shared frame.
#[derive(Clone, Debug, PartialEq)]
pub struct FramedMessage {
	/// Type tag shown as the card header.
	pub custom_type: String,
	/// Body content: plain text or typed parts.
	pub content: FramedContent,
}

/// Body content of a framed message.
#[derive(Clone, Debug, PartialEq)]
pub enum FramedContent {
	/// Markdown source.
	Text(String),
	/// Typed content parts; only text parts reach the body.
	Parts(Vec<FramedPart>),
}

/// One message content part.
#[derive(Clone, Debug, PartialEq)]
pub struct FramedPart {
	/// Part type (`"text"`, `"image"`, …).
	pub kind: String,
	/// Text payload of a `"text"` part.
	pub text: Option<String>,
}

impl FramedPart {
	/// A text part.
	pub fn text(text: impl Into<String>) -> Self {
		FramedPart {
			kind: "text".to_string(),
			text: Some(text.into()),
		}
	}
}

impl FramedMessage {
	/// A message with plain text content.
	pub fn text(custom_type: impl Into<String>, text: impl Into<String>) -> Self {
		FramedMessage {
			custom_type: custom_type.into(),
			content: FramedContent::Text(text.into()),
		}
	}

	/// The body: the text itself, or the text parts joined by newlines.
	pub fn body(&self) -> String {
		match &self.content {
			FramedContent::Text(text) => text.clone(),
			FramedContent::Parts(parts) => parts
				.iter()
				.filter(|part| part.kind == "text")
				.filter_map(|part| part.text.clone())
				.collect::<Vec<_>>()
				.join("\n"),
		}
	}
}

/// A renderer's replacement component, plus whether its rows are compact.
///
/// Rust cannot ask a `dyn Component` whether it implements
/// [`CompactTranscriptBlock`], so the renderer states it (the reference calls
/// `isTranscriptBlockCompact()` on the returned component).
pub struct FramedRender {
	/// The replacement component.
	pub component: Box<dyn Component>,
	/// Whether the rows pack against neighbouring compact blocks.
	pub compact: bool,
}

impl FramedRender {
	/// A component whose rows are not compact.
	pub fn new(component: Box<dyn Component>) -> Self {
		FramedRender {
			component,
			compact: false,
		}
	}
}

/// Callable signature shared by extension and hook message renderers.
///
/// The reference passes `{ expanded }` as the second argument; the flag is bare
/// here.
pub type FramedRenderer = Box<dyn Fn(&FramedMessage, bool, &Theme) -> Option<FramedRender>>;

/// Presentation and extension-renderer policy for a framed custom message.
pub struct FramedMessageOptions {
	/// The message to frame.
	pub message: FramedMessage,
	/// Theme-aware icon glyph shown before the custom type. The reference also
	/// accepts a plain string; a constant closure covers that here.
	pub icon: Option<Box<dyn Fn() -> String>>,
	/// Hide the default type header while retaining the body. The reference
	/// also accepts a plain bool; a constant closure covers that here.
	pub hide_header: Option<Box<dyn Fn() -> bool>>,
	/// Semantic tone token for the card, defaulting to the muted border.
	pub border_color: Option<String>,
	/// Collapse the body to this many lines when not expanded; `None` never
	/// collapses.
	pub collapse_after_lines: Option<usize>,
	/// Replacement renderer; its component wins when it returns one.
	pub custom_renderer: Option<FramedRenderer>,
	/// Semantic role of the native card.
	pub role: String,
}

impl FramedMessageOptions {
	/// Options for `message` with no renderer and no collapse.
	pub fn new(role: impl Into<String>, message: FramedMessage) -> Self {
		FramedMessageOptions {
			message,
			icon: None,
			hide_header: None,
			border_color: None,
			collapse_after_lines: None,
			custom_renderer: None,
			role: role.into(),
		}
	}
}

/// Behavior-owning custom message frame.
///
/// Reference: `FramedMessageComponent` in `l/src/chrome/message-frame.ts`. The
/// frame retries the extension renderer whenever expansion or theme state
/// changes and falls back to a labelled body when the renderer returns nothing
/// or panics (a panicking renderer is caught like the reference's `try`). The
/// reference keeps a renderer's component while its identity is stable; here
/// every rebuild re-runs the renderer and replaces the component, so a
/// renderer must be cheap to call again.
pub struct FramedMessageComponent {
	options: FramedMessageOptions,
	theme: Theme,
	custom: Option<Box<dyn Component>>,
	custom_compact: bool,
	expanded: bool,
	disposed: bool,
	version: u64,
	native: Memo<NativeNode>,
}

impl FramedMessageComponent {
	/// Build the frame and run the renderer once.
	pub fn new(options: FramedMessageOptions, theme: &Theme) -> Self {
		let mut component = FramedMessageComponent {
			options,
			theme: theme.clone(),
			custom: None,
			custom_compact: false,
			expanded: false,
			disposed: false,
			version: 0,
			native: Memo::new(),
		};
		component.rebuild();
		component
	}

	/// The framed message.
	pub fn message(&self) -> &FramedMessage {
		&self.options.message
	}

	/// Whether the body is expanded.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// Set the expanded state, retrying the renderer when it changed.
	pub fn set_expanded(&mut self, expanded: bool) {
		if self.expanded == expanded {
			return;
		}
		self.expanded = expanded;
		self.rebuild();
	}

	/// Replace the theme, retrying the renderer.
	pub fn set_theme(&mut self, theme: &Theme) {
		self.theme = theme.clone();
		self.rebuild();
	}

	/// Release the renderer's component; later renders are empty.
	pub fn dispose(&mut self) {
		if self.disposed {
			return;
		}
		self.disposed = true;
		self.custom = None;
		self.custom_compact = false;
		self.native.clear();
	}

	/// Whether the active presentation is compact rows.
	pub fn compact(&self) -> bool {
		self.custom.is_some() && self.custom_compact
	}

	/// Apply a native toggle aimed at this card.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) -> bool {
		match root_toggle_expanded(event) {
			Some(expanded) => {
				self.set_expanded(expanded);
				true
			}
			None => false,
		}
	}

	/// The native form: the renderer's rows, else a tone-tinted `card` with the
	/// type tag as its head and the markdown body.
	pub fn describe(&mut self, cx: &DescribeContext) -> NativeNode {
		let deps = vec![
			serde_json::json!(self.version),
			serde_json::json!(self.expanded),
			serde_json::json!(self.disposed),
		];
		let role = self.options.role.clone();
		let hide_header = self.hide_header();
		let tag = self.options.message.custom_type.clone();
		let border_color = self.options.border_color.clone();
		let collapse_after_lines = self.options.collapse_after_lines;
		let expanded = self.expanded;
		let body = self.body_text();
		let mut custom_node = None;
		if let Some(custom) = self.custom.as_mut() {
			let mut props = Props::new();
			props.set_str("role", role.as_str());
			custom_node = Some(col(vec![describe_child(custom.as_mut(), cx)], props));
		}
		self.native
			.get(&deps, || {
				if let Some(described) = custom_node {
					return described;
				}
				let mut props = Props::new();
				props.set_str("role", role);
				if let Some(tone) = border_color.as_deref().and_then(color_tone_token) {
					props.set_tone(tone);
				}
				if !hide_header {
					// The role icon replaces the nerd glyph natively; only the ANSI
					// header carries it.
					props.set_head(NodeText::Spans(vec![span(tag, Some("customMessageLabel strong"))]));
				}
				if let Some(lines) = collapse_after_lines {
					props.set_bool("collapsible", true);
					props.set_bool("collapsed", !expanded);
					props.insert("preview", serde_json::json!({ "lines": lines }));
				}
				card(props, vec![md(body, Props::new())])
			})
			.clone()
	}

	/// The type header for the ANSI fallback, `None` when hidden.
	fn header_row(&self) -> Option<String> {
		if self.hide_header() {
			return None;
		}
		let tag = match self.icon() {
			Some(icon) => format!("{icon} {}", self.options.message.custom_type),
			None => self.options.message.custom_type.clone(),
		};
		Some(tag)
	}

	fn icon(&self) -> Option<String> {
		self.options.icon.as_ref().map(|icon| icon())
	}

	fn hide_header(&self) -> bool {
		self.options
			.hide_header
			.as_ref()
			.is_some_and(|hide| hide())
	}

	/// The body as drawn: the whole text, or its leading lines plus `…`.
	fn body_text(&self) -> String {
		let text = self.options.message.body();
		match self.options.collapse_after_lines {
			Some(limit) if !self.expanded => {
				let lines: Vec<&str> = text.split('\n').collect();
				if lines.len() > limit {
					format!("{}\n…", lines[..limit].join("\n"))
				} else {
					text
				}
			}
			_ => text,
		}
	}

	fn rebuild(&mut self) {
		if self.disposed {
			return;
		}
		self.version += 1;
		self.native.clear();
		let mut next = None;
		if let Some(renderer) = self.options.custom_renderer.as_ref() {
			let message = &self.options.message;
			let theme = &self.theme;
			let expanded = self.expanded;
			// A broken extension renderer must not hide its persisted message.
			let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
				renderer(message, expanded, theme)
			}))
			.ok()
			.flatten();
			next = rendered;
		}
		match next {
			Some(render) => {
				self.custom = Some(render.component);
				self.custom_compact = render.compact;
			}
			None => {
				self.custom = None;
				self.custom_compact = false;
			}
		}
	}
}

impl Component for FramedMessageComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		if self.disposed || width == 0 {
			return Vec::new();
		}
		if let Some(custom) = self.custom.as_mut() {
			return custom.render(width);
		}
		let mut rows = Vec::new();
		if let Some(header) = self.header_row() {
			rows.push(truncate_to_width(&header, width as usize, Ellipsis::Omit));
			rows.push(String::new());
		}
		for line in self.body_text().split('\n') {
			rows.push(truncate_to_width(line, width as usize, Ellipsis::Omit));
		}
		rows
	}

	fn invalidate(&mut self) {
		self.rebuild();
	}

	fn dispose(&mut self) {
		FramedMessageComponent::dispose(self);
	}
}

impl CompactTranscriptBlock for FramedMessageComponent {
	fn is_transcript_block_compact(&self) -> bool {
		self.compact()
	}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_theme() -> MessageTheme {
        MessageTheme {
            avatar_width: 2,
            user: Style::default(),
            assistant: Style::default(),
            system: Style::default(),
            error: Style::default(),
            warning: Style::default(),
            success: Style::default(),
            info: Style::default(),
            text: Style::default(),
            meta: Style::default(),
            copy_hint: Style::default(),
            divider: Style::default(),
        }
    }

    #[test]
    fn message_wraps_under_the_avatar_gutter() {
        let mut message = Message::new(MessageRole::User);
        message.set_theme(plain_theme());
        message.set_avatar(">");
        message.set_text("one two three four");
        assert_eq!(message.render(14), vec![">  one two", "   three four"]);
    }

    #[test]
    fn message_rows_never_exceed_width() {
        let mut message = Message::new(MessageRole::Assistant);
        message.set_theme(plain_theme());
        message.set_avatar("ai");
        message.set_text("word ".repeat(40));
        for width in [1u16, 2, 3, 5, 8, 20] {
            for row in message.render(width) {
                assert!(visible_width(&row) <= width as usize, "width {width}: {row:?}");
            }
        }
    }

    #[test]
    fn empty_message_renders_one_blank_row() {
        let mut message = Message::new(MessageRole::System);
        message.set_theme(plain_theme());
        assert_eq!(message.render(10), vec!["   "]);
        assert!(message.render(0).is_empty());
    }

    #[test]
    fn roles_pick_their_style() {
        let theme = MessageTheme::default();
        assert_eq!(theme.style_for(MessageRole::Error), Style::fg(Color::Red));
        assert_eq!(theme.style_for(MessageRole::User), Style::fg(Color::White));
    }

    #[test]
    fn copy_action_fires_once_per_request() {
        let mut message = Message::new(MessageRole::Info);
        message.set_text("body");
        assert!(message.copy_text().is_none());
        assert!(!message.request_copy());
        message.set_copy_text(Some("body".to_string()));
        let hits = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = hits.clone();
        message.on_copy(move || counter.set(counter.get() + 1));
        assert!(message.request_copy());
        assert_eq!(hits.get(), 1);
        message.handle_input(&Key::Ctrl('c'));
        assert_eq!(hits.get(), 2);
        assert_eq!(message.copy_hint().as_deref(), Some("\x1b[90mcopy\x1b[0m"));
    }

    #[test]
    fn divider_renders_rule_and_blank_edges() {
        let mut divider = MessageDivider::new("notes");
        divider.set_label_style(Style::default());
        divider.set_rule_style(Style::default());
        assert_eq!(divider.render(20), vec!["", "────────── notes", ""]);
        divider.set_rule_width(3);
        assert_eq!(divider.render(20), vec!["", "─── notes", ""]);
    }

    #[test]
    fn divider_truncates_when_narrow() {
        let mut divider = MessageDivider::new("a-long-label");
        divider.set_label_style(Style::default());
        divider.set_rule_style(Style::default());
        assert_eq!(divider.render(6), vec!["", "a-long", ""]);
    }

    #[test]
    fn notice_renders_header_body_and_tone() {
        let mut notice = MessageNotice::new("Careful");
        notice.set_icon(Some("!"));
        notice.set_body(&["first".to_string(), "second".to_string()]);
        let rows = notice.render(12);
        assert_eq!(rows.len(), 5);
        assert_eq!(visible_width(&rows[1]), 9);
        assert_eq!(rows[2], "");
        assert_eq!(rows[3], "first");
    }

    #[test]
    fn notice_without_body_has_no_gap_row() {
        let mut notice = MessageNotice::new("Hi");
        notice.set_leading_space(false);
        assert_eq!(notice.render(10), vec!["\x1b[33mHi\x1b[0m"]);
    }

    #[test]
    fn divider_keeps_an_untruncated_legacy_label_when_asked() {
        let mut divider = MessageDivider::new("a-long-label");
        divider.set_label_style(Style::default());
        divider.set_rule_style(Style::default());
        divider.set_truncate_when_narrow(false);
        assert_eq!(divider.render(6), vec!["", "a-long-label", ""]);
        assert!(!divider.truncate_when_narrow());
    }

    #[test]
    fn divider_reads_a_live_label_every_render() {
        use std::cell::Cell;
        let tick = std::rc::Rc::new(Cell::new(0));
        let counter = tick.clone();
        let mut divider = MessageDivider::new("idle");
        divider.set_label_style(Style::default());
        divider.set_rule_style(Style::default());
        divider.set_label_fn(Some(Box::new(move || format!("tick {}", counter.get()))));
        assert!(divider.render(20)[1].contains("tick 0"));
        tick.set(3);
        assert!(divider.render(20)[1].contains("tick 3"));
    }

    #[test]
    fn divider_describes_a_rule_or_an_inline_notice() {
        let divider = MessageDivider::new("notes");
        let described = divider.describe();
        assert_eq!(described.kind, NodeKind::Rule);
        assert_eq!(described.props.get_str("role"), Some(DEFAULT_DIVIDER_ROLE));
        let mut divider = MessageDivider::new("copied");
        divider.set_role("omp.toast");
        divider.set_inline_icon(Some("✔"));
        let described = divider.describe();
        assert_eq!(described.kind, NodeKind::Row);
        assert_eq!(described.props.get_str("role"), Some("omp.toast"));
        assert_eq!(described.children.len(), 2);
        assert_eq!(described.children[0].kind, NodeKind::Icon);
    }

    #[test]
    fn notice_activity_visibility_gates_render_and_describe() {
        let mut notice = MessageNotice::new("Working");
        assert!(notice.tool_activity_visible());
        notice.set_tool_activity_visible(false);
        assert!(notice.render(20).is_empty());
        let described = notice.describe();
        assert_eq!(described.props.get_bool("hidden"), Some(true));
        notice.set_tool_activity_visible(true);
        notice.set_body(&["body".to_string()]);
        assert_eq!(notice.render(20).len(), 3);
        assert_eq!(notice.describe().props.get_bool("hidden"), Some(false));
    }

    #[test]
    fn notice_preview_and_toggle_follow_expansion() {
        let mut notice = MessageNotice::new("Careful");
        notice.set_body(&["first".to_string(), "second".to_string()]);
        notice.set_preview_lines(Some(1));
        let described = notice.describe();
        assert_eq!(described.props.get_bool("collapsible"), Some(true));
        assert_eq!(described.props.get_bool("collapsed"), Some(true));
        assert!(!notice.is_expanded());
        assert!(notice.handle_native_event(&NativeUiEvent::Toggle {
            key: Vec::new(),
            collapsed: false,
        }));
        assert!(notice.is_expanded());
        assert_eq!(notice.describe().props.get_bool("collapsed"), Some(false));
        // A toggle on a nested node is somebody else's.
        assert!(!notice.handle_native_event(&NativeUiEvent::Toggle {
            key: vec!["inner".to_string()],
            collapsed: true,
        }));
        assert!(notice.is_expanded());
    }

    fn test_theme() -> Theme {
        crate::theme::loader::create_theme(
            crate::theme::builtin::builtin_theme_json("dark").expect("dark theme"),
            crate::theme::loader::CreateThemeOptions::default(),
        )
        .expect("theme")
    }

    #[test]
    fn framed_message_body_joins_text_parts() {
        let message = FramedMessage {
            custom_type: "hook".to_string(),
            content: FramedContent::Parts(vec![
                FramedPart::text("one"),
                FramedPart {
                    kind: "image".to_string(),
                    text: None,
                },
                FramedPart::text("two"),
            ]),
        };
        assert_eq!(message.body(), "one\ntwo");
    }

    #[test]
    fn framed_frame_falls_back_when_the_renderer_returns_nothing() {
        let theme = test_theme();
        let mut options = FramedMessageOptions::new("omp.custom-message", FramedMessage::text("note", "body"));
        options.icon = Some(Box::new(|| "✎".to_string()));
        let mut frame = FramedMessageComponent::new(options, &theme);
        assert_eq!(frame.render(20), vec!["✎ note", "", "body"]);
        assert!(!frame.compact());
        assert_eq!(frame.describe(&DescribeContext::new(40)).kind, NodeKind::Card);
    }

    #[test]
    fn framed_frame_uses_the_renderer_and_ignores_a_panicking_one() {
        struct Fixed;
        impl Component for Fixed {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["custom".to_string()]
            }
        }
        let theme = test_theme();
        let mut options = FramedMessageOptions::new("omp.custom-message", FramedMessage::text("note", "body"));
        options.custom_renderer = Some(Box::new(|_, _, _| {
            Some(FramedRender {
                component: Box::new(Fixed),
                compact: true,
            })
        }));
        let mut frame = FramedMessageComponent::new(options, &theme);
        assert_eq!(frame.render(20), vec!["custom"]);
        assert!(frame.is_transcript_block_compact());
        // The replacement describes through the rows fallback.
        assert_eq!(frame.describe(&DescribeContext::new(40)).kind, NodeKind::Col);

        let mut broken = FramedMessageOptions::new("omp.custom-message", FramedMessage::text("note", "body"));
        broken.custom_renderer = Some(Box::new(|_, _, _| panic!("renderer bug")));
        let mut frame = FramedMessageComponent::new(broken, &theme);
        assert_eq!(frame.render(20), vec!["note", "", "body"]);
    }

    #[test]
    fn framed_frame_folds_when_collapsed_and_toggles_natively() {
        let theme = test_theme();
        let mut options = FramedMessageOptions::new(
            "omp.custom-message",
            FramedMessage::text("note", "one\ntwo\nthree"),
        );
        options.collapse_after_lines = Some(2);
        options.hide_header = Some(Box::new(|| true));
        let mut frame = FramedMessageComponent::new(options, &theme);
        assert_eq!(frame.render(20), vec!["one", "two", "…"]);
        let described = frame.describe(&DescribeContext::new(40));
        assert_eq!(described.props.get_bool("collapsed"), Some(true));
        assert!(frame.handle_native_event(&NativeUiEvent::Toggle {
            key: Vec::new(),
            collapsed: false,
        }));
        assert_eq!(frame.render(20), vec!["one", "two", "three"]);
        assert_eq!(
            frame.describe(&DescribeContext::new(40)).props.get_bool("collapsed"),
            Some(false)
        );
    }

    #[test]
    fn root_toggle_needs_the_root_keypath() {
        assert_eq!(
            root_toggle_expanded(&NativeUiEvent::Toggle {
                key: Vec::new(),
                collapsed: true,
            }),
            Some(false)
        );
        assert_eq!(
            root_toggle_expanded(&NativeUiEvent::Select {
                key: Vec::new(),
                item: "x".to_string(),
            }),
            None
        );
    }

    #[test]
    fn inline_notice_describes_a_disclosable_section() {
        let mut notice = MessageNotice::new("Thinking");
        notice.set_inline_icon(Some("brain"));
        notice.set_body(&["body".to_string()]);
        notice.set_role("omp.notice.thinking");
        let described = notice.describe();
        assert_eq!(described.kind, NodeKind::Section);
        assert_eq!(described.props.get_str("role"), Some("omp.notice.thinking"));
        assert_eq!(described.props.get_bool("collapsible"), Some(true));
        assert_eq!(described.props.get_bool("collapsed"), Some(true));
        // head row: icon then truncated label.
        assert_eq!(described.children[0].kind, NodeKind::Row);
        assert_eq!(described.children[0].children[0].kind, NodeKind::Icon);
        assert!(notice.handle_native_event(&NativeUiEvent::Toggle {
            key: Vec::new(),
            collapsed: false,
        }));
        assert_eq!(notice.describe().props.get_bool("collapsed"), Some(false));
        notice.set_inline_icon(None::<String>);
        assert_eq!(notice.describe().kind, NodeKind::Card);
    }
}
