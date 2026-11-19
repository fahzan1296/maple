//! Inline transcript notices: one icon-tone row per warning, error, background
//! job completion or prompt file read.
//!
//! Reference: `l/src/chrome/transcript-notice.ts`.
//!
//! Deviations:
//! - The reference builds its native description from the process-global
//!   `theme` through `styledSpans`; maple-tui has no global theme, so
//!   [`TranscriptNotice::line`] takes an explicit `&Theme`.
//! - `TranscriptBlock` (`l/src/chrome/transcript-container.ts`) is not part of
//!   this crate yet, so [`TranscriptNotice`] owns its ANSI rows directly
//!   instead of extending the transcript block base class.

use crate::component::Component;
use crate::components::{Spacer, Text};
use crate::native::describe::{node, text, NativeDescribe};
use crate::native::node::{
    DescribeContext, NativeNode, NodeKind, NodeText, Props, Span, Tone,
};
use crate::native::spans::styled_spans;
use crate::style::{Color, Style};
use crate::theme::color::parse_hex;
use crate::theme::Theme;

/// How loud an inline transcript notice is; a native terminal styles it as
/// `omp.notice.<tone>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeTone {
    /// Neutral information; keeps its icon quiet.
    Info,
    /// Confirmation.
    Success,
    /// Caution; leads with a `Warning` label in a text terminal.
    Warning,
    /// Failure; leads with an `Error` label in a text terminal.
    Error,
}

impl NoticeTone {
    /// The wire name (`"info"`, `"success"`, `"warning"`, `"error"`), which is
    /// also each tone's default icon name.
    pub fn as_str(self) -> &'static str {
        match self {
            NoticeTone::Info => "info",
            NoticeTone::Success => "success",
            NoticeTone::Warning => "warning",
            NoticeTone::Error => "error",
        }
    }

    /// The TSP tone of this notice.
    pub fn to_tone(self) -> Tone {
        match self {
            NoticeTone::Info => Tone::Info,
            NoticeTone::Success => Tone::Success,
            NoticeTone::Warning => Tone::Warning,
            NoticeTone::Error => Tone::Error,
        }
    }

    /// The word a warning or error leads with in a text terminal.
    pub fn label(self) -> Option<&'static str> {
        match self {
            NoticeTone::Warning => Some("Warning"),
            NoticeTone::Error => Some("Error"),
            _ => None,
        }
    }
}

/// Optional parts of a [`notice_row`].
#[derive(Clone, Debug, Default)]
pub struct NoticeRowOptions {
    /// Icon name in place of the tone's own (`read` for a file read,
    /// `download` for an update).
    pub icon: Option<String>,
    /// Sibling identity key.
    pub key: Option<String>,
}

/// One inline transcript notice row for a native terminal
/// (`omp.notice.<tone>`): a leading icon beside the wrapped message.
pub fn notice_row(tone: NoticeTone, message: NodeText, options: NoticeRowOptions) -> NativeNode {
    let mut icon_props = Props::new();
    icon_props.set_str("name", options.icon.unwrap_or_else(|| tone.as_str().to_string()));
    if tone != NoticeTone::Info {
        icon_props.set_tone(tone.to_tone());
    }
    if let Some(label) = tone.label() {
        icon_props.set_str("aria", label);
    }
    let icon = node(NodeKind::Icon, icon_props, Vec::new(), None);

    let mut text_props = Props::new();
    text_props.set_str("wrap", "word");
    text_props.set_i64("grow", 1);
    text_props.set_i64("shrink", 1);
    let body = text(message, text_props);

    let mut row_props = Props::new();
    row_props.set_role(format!("omp.notice.{}", tone.as_str()));
    row_props.set_tone(tone.to_tone());
    row_props.set_str("gap", "sm");
    row_props.set_str("align", "start");
    node(
        NodeKind::Row,
        row_props,
        vec![icon, body],
        options.key.as_deref(),
    )
}

/// The tones [`TranscriptNotice::line`] renders as a labelled text row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptNoticeTone {
    /// A `Warning: …` line.
    Warning,
    /// An `Error: …` line.
    Error,
}

impl TranscriptNoticeTone {
    /// The full notice tone.
    pub fn notice_tone(self) -> NoticeTone {
        match self {
            TranscriptNoticeTone::Warning => NoticeTone::Warning,
            TranscriptNoticeTone::Error => NoticeTone::Error,
        }
    }
}

/// A transcript block that a native terminal draws as one [`notice_row`] while
/// a text terminal draws ANSI rows.
pub struct TranscriptNotice {
    native: NativeNode,
    ansi: Vec<Box<dyn Component>>,
}

impl TranscriptNotice {
    /// A notice from its prepared native node and ANSI rows.
    pub fn new(native: NativeNode, ansi: Vec<Box<dyn Component>>) -> Self {
        TranscriptNotice { native, ansi }
    }

    /// `showWarning` / `showError`: a blank row and the `Warning: …` /
    /// `Error: …` line in its colour in a text terminal; natively the tone's
    /// icon and the message, the label left to the icon.
    pub fn line(theme: &Theme, tone: TranscriptNoticeTone, message: &str) -> Self {
        let notice_tone = tone.notice_tone();
        let mut line = Text::padded(
            format!("{}: {}", notice_tone.label().expect("labelled tone"), message),
            1,
            0,
        );
        line.set_style(tone_style(theme, notice_tone.as_str()));
        let spans = styled_spans(message, theme);
        Self::new(
            notice_row(notice_tone, NodeText::Spans(spans), NoticeRowOptions::default()),
            vec![Box::new(Spacer::new(1)), Box::new(line)],
        )
    }
}

impl Component for TranscriptNotice {
    fn render(&mut self, width: u16) -> Vec<String> {
        let mut rows = Vec::new();
        for child in &mut self.ansi {
            rows.extend(child.render(width));
        }
        rows
    }

    fn invalidate(&mut self) {
        for child in &mut self.ansi {
            child.invalidate();
        }
    }

    fn dispose(&mut self) {
        for child in &mut self.ansi {
            child.dispose();
        }
    }
}

impl NativeDescribe for TranscriptNotice {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        self.native.clone()
    }
}

/// Resolve a tone token to the style used by the ANSI line.
fn tone_style(theme: &Theme, token: &str) -> Style {
    match theme.get_color_hex(token).ok().and_then(|hex| parse_hex(&hex)) {
        Some((red, green, blue)) => Style::fg(Color::Rgb(red, green, blue)),
        None => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin;
    use crate::theme::{create_theme, CreateThemeOptions};
    use crate::theme::color::ColorMode;

    fn theme() -> Theme {
        create_theme(
            builtin::builtin_theme_json("dark").expect("builtin theme"),
            CreateThemeOptions {
                mode: Some(ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .expect("theme resolves")
    }

    fn props_of(node: &NativeNode) -> &Props {
        &node.props
    }

    #[test]
    fn notice_row_tags_role_and_sets_the_icon_name() {
        let row = notice_row(
            NoticeTone::Warning,
            NodeText::plain("careful"),
            NoticeRowOptions::default(),
        );
        assert_eq!(row.kind, NodeKind::Row);
        let props = props_of(&row);
        assert_eq!(props.role(), Some("omp.notice.warning"));
        assert_eq!(props.tone(), Some(Tone::Warning));
        assert_eq!(props.get_str("gap"), Some("sm"));
        assert_eq!(props.get_str("align"), Some("start"));

        let children = &row.children;
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].kind, NodeKind::Icon);
        assert_eq!(children[0].props.get_str("name"), Some("warning"));
        assert_eq!(children[0].props.tone(), Some(Tone::Warning));
        assert_eq!(children[0].props.get_str("aria"), Some("Warning"));
        assert_eq!(children[1].kind, NodeKind::Text);
        assert_eq!(children[1].props.get_str("wrap"), Some("word"));
        assert_eq!(children[1].props.get_i64("grow"), Some(1));
        assert_eq!(children[1].props.get_i64("shrink"), Some(1));
    }

    #[test]
    fn info_tone_leaves_the_icon_untoned_and_unlabelled() {
        let row = notice_row(
            NoticeTone::Info,
            NodeText::plain("hello"),
            NoticeRowOptions::default(),
        );
        let icon = &row.children[0];
        assert_eq!(icon.props.get_str("name"), Some("info"));
        assert_eq!(icon.props.tone(), None);
        assert_eq!(icon.props.get_str("aria"), None);
        assert_eq!(row.props.tone(), Some(Tone::Info));
    }

    #[test]
    fn options_override_the_icon_and_key() {
        let row = notice_row(
            NoticeTone::Success,
            NodeText::plain("done"),
            NoticeRowOptions {
                icon: Some("download".to_string()),
                key: Some("update".to_string()),
            },
        );
        assert_eq!(row.children[0].props.get_str("name"), Some("download"));
        assert_eq!(row.key.as_deref(), Some("update"));
    }

    #[test]
    fn notice_tone_labels_match_the_reference() {
        assert_eq!(NoticeTone::Warning.label(), Some("Warning"));
        assert_eq!(NoticeTone::Error.label(), Some("Error"));
        assert_eq!(NoticeTone::Info.label(), None);
        assert_eq!(NoticeTone::Success.label(), None);
    }

    #[test]
    fn line_builds_a_blank_row_and_a_labelled_styled_row() {
        let dark = theme();
        let mut notice = TranscriptNotice::line(&dark, TranscriptNoticeTone::Warning, "low disk");
        let rows = notice.render(30);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], "");
        assert!(rows[1].contains("Warning: low disk"));
        let described = notice.describe_native(&DescribeContext::new(30));
        assert_eq!(described.props.role(), Some("omp.notice.warning"));
    }

    #[test]
    fn line_paints_the_ansi_label_with_the_tone_colour() {
        let dark = theme();
        let mut notice = TranscriptNotice::line(&dark, TranscriptNoticeTone::Error, "boom");
        let expected = tone_style(&dark, "error");
        assert!(expected != Style::default());
        let row = &notice.render(20)[1];
        assert!(row.starts_with("\x1b["));
        assert!(row.contains("Error: boom"));
    }

    #[test]
    fn native_description_is_stable_across_calls() {
        let dark = theme();
        let notice = TranscriptNotice::line(&dark, TranscriptNoticeTone::Warning, "x");
        let cx = DescribeContext::new(20);
        let first = notice.describe_native(&cx);
        let second = notice.describe_native(&cx);
        assert_eq!(first, second);
    }

    #[test]
    fn spans_are_carried_as_the_message() {
        let row = notice_row(
            NoticeTone::Info,
            NodeText::Spans(vec![Span::styled("path".into(), "path")]),
            NoticeRowOptions::default(),
        );
        let body = &row.children[1];
        assert_eq!(
            body.props.spans(),
            Some(vec![Span::styled("path".into(), "path")])
        );
    }
}
