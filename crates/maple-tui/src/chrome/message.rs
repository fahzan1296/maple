//! Transcript message chrome: role-tagged messages with an avatar gutter, a
//! labelled divider, and a toned notice shell.
//!
//! Ported from `l/src/chrome/message-frame.ts`, `message-divider.ts`, and
//! `message-notice.ts`. pi-tui renders user/assistant bubbles with Markdown and
//! themed backgrounds; this module keeps the bounded row contract (avatar
//! gutter plus wrapped text, every row clamped to the viewport width) and the
//! presentation policy (role colors, labels, notice tone).

use crate::component::Component;
use crate::keys::Key;
use crate::style::{Color, Style};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};

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
pub struct MessageDivider {
    label: String,
    label_style: Style,
    rule_style: Style,
    rule_width: usize,
    memo: Option<(u16, Vec<String>)>,
}

impl MessageDivider {
    /// A divider labelled `label` with the default rule width of 10.
    pub fn new(label: impl Into<String>) -> Self {
        MessageDivider {
            label: label.into(),
            label_style: Style::fg(Color::DarkGray),
            rule_style: Style::fg(Color::DarkGray),
            rule_width: 10,
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

    /// The configured label.
    pub fn label(&self) -> &str {
        &self.label
    }

    fn divider_row(&self, width: u16) -> String {
        let width = width as usize;
        let label = self.label_style.paint(&self.label);
        let label_width = visible_width(&label);
        let rule_width = self.rule_width.min(width.saturating_sub(label_width + 1));
        if rule_width < 1 {
            // The row is always clamped: an oversized label would overflow the
            // viewport, which the component contract forbids.
            return truncate_to_width(&label, width, Ellipsis::Omit);
        }
        let rule = self.rule_style.paint(&"─".repeat(rule_width));
        truncate_to_width(&format!("{rule} {label}"), width, Ellipsis::Omit)
    }
}

impl Component for MessageDivider {
    fn render(&mut self, width: u16) -> Vec<String> {
        if let Some((memo_width, rows)) = &self.memo {
            if *memo_width == width {
                return rows.clone();
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
pub struct MessageNotice {
    header: String,
    icon: Option<String>,
    body: Vec<String>,
    leading_space: bool,
    theme: NoticeTheme,
    memo: Option<(u16, Vec<String>)>,
}

impl MessageNotice {
    /// A notice with `header` and no body.
    pub fn new(header: impl Into<String>) -> Self {
        MessageNotice {
            header: header.into(),
            icon: None,
            body: Vec::new(),
            leading_space: true,
            theme: NoticeTheme::default(),
            memo: None,
        }
    }

    /// Replace the notice theme.
    pub fn set_theme(&mut self, theme: NoticeTheme) {
        self.theme = theme;
        self.memo = None;
    }

    /// The notice theme.
    pub fn theme(&self) -> &NoticeTheme {
        &self.theme
    }

    /// Prefix the header with an icon glyph.
    pub fn set_icon(&mut self, icon: Option<impl Into<String>>) {
        self.icon = icon.map(Into::into);
        self.memo = None;
    }

    /// Replace the body rows.
    pub fn set_body(&mut self, body: &[String]) {
        self.body = body.to_vec();
        self.memo = None;
    }

    /// Drop the leading blank row.
    pub fn set_leading_space(&mut self, leading: bool) {
        self.leading_space = leading;
        self.memo = None;
    }

    /// The header text.
    pub fn header(&self) -> &str {
        &self.header
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
}
