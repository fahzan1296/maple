//! Compact transcript status rows: background activity and prompt file reads,
//! one themed line per row in a text terminal, one notice row natively.
//!
//! Reference: `l/src/chrome/transcript-status.ts`.
//!
//! Deviations:
//! - The reference resolves each part's colour from the process-global
//!   `theme`; maple-tui has no global theme, so [`TranscriptStatusBlock::new`]
//!   takes an explicit `&Theme`.
//! - `TranscriptBlock` (`l/src/chrome/transcript-container.ts`) is not ported
//!   yet, so [`TranscriptStatusBlock`] owns its ANSI rows directly instead of
//!   extending that base class.

use crate::component::Component;
use crate::components::Text;
use crate::native::describe::{col, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeText, Props, Span};
use crate::theme::Theme;

use super::transcript_notice::{notice_row, NoticeRowOptions, NoticeTone};

/// One run of a status row's terminal line, in `color` unless the text is
/// already styled.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptStatusPart {
    /// The visible text.
    pub text: String,
    /// The theme colour token painting it, when any.
    pub color: Option<String>,
}

/// A status row in a native terminal: its notice row's tone, icon and message.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptStatusNative {
    /// How loud the row is.
    pub tone: NoticeTone,
    /// Icon name in place of the tone's own.
    pub icon: Option<String>,
    /// The message spans.
    pub message: Vec<Span>,
}

/// One compact transcript status row: the terminal line's themed parts and the
/// same row natively.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptStatusRow {
    /// The themed runs; `None` entries are dropped.
    pub parts: Vec<Option<TranscriptStatusPart>>,
    /// Left padding in cells; defaults to 1.
    pub indent: Option<u16>,
    /// The native description.
    pub native: TranscriptStatusNative,
}

/// Shared compact status block for background activity and prompt file reads.
pub struct TranscriptStatusBlock {
    ansi: Vec<Box<dyn Component>>,
    native: NativeNode,
}

impl TranscriptStatusBlock {
    /// Build the block from its rows: one themed line per row in a text
    /// terminal, one notice row per row natively (`omp.status-block`).
    pub fn new(theme: &Theme, rows: &[TranscriptStatusRow]) -> Self {
        let mut ansi: Vec<Box<dyn Component>> = Vec::with_capacity(rows.len());
        let mut native_rows: Vec<NativeNode> = Vec::with_capacity(rows.len());
        for row in rows {
            let styled: Vec<String> = row
                .parts
                .iter()
                .flatten()
                .map(|part| match &part.color {
                    Some(color) => theme.fg(color, &part.text).unwrap_or_else(|_| part.text.clone()),
                    None => part.text.clone(),
                })
                .collect();
            ansi.push(Box::new(Text::padded(
                styled.join(" "),
                row.indent.unwrap_or(1),
                0,
            )));
            native_rows.push(notice_row(
                row.native.tone,
                NodeText::Spans(row.native.message.clone()),
                NoticeRowOptions {
                    icon: row.native.icon.clone(),
                    key: None,
                },
            ));
        }
        let mut props = Props::new();
        props.set_role("omp.status-block");
        TranscriptStatusBlock {
            ansi,
            native: col(native_rows, props),
        }
    }
}

impl Component for TranscriptStatusBlock {
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

impl NativeDescribe for TranscriptStatusBlock {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        self.native.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::node::{NodeKind, Tone};
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

    fn row(parts: Vec<Option<TranscriptStatusPart>>, tone: NoticeTone) -> TranscriptStatusRow {
        TranscriptStatusRow {
            parts,
            indent: None,
            native: TranscriptStatusNative {
                tone,
                icon: None,
                message: vec![Span::styled("body".into(), "muted")],
            },
        }
    }

    fn part(text: &str, color: Option<&str>) -> Option<TranscriptStatusPart> {
        Some(TranscriptStatusPart {
            text: text.to_string(),
            color: color.map(str::to_string),
        })
    }

    #[test]
    fn ansi_joins_styled_parts_with_single_spaces() {
        let dark = theme();
        let rows = vec![row(
            vec![
                part("read", Some("accent")),
                part("~/.config/rc", None),
                None,
            ],
            NoticeTone::Info,
        )];
        let mut block = TranscriptStatusBlock::new(&dark, &rows);
        let rendered = block.render(40);
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].starts_with(' '));
        assert!(rendered[0].contains("read"));
        assert!(rendered[0].contains(" ~/.config/rc"));
        assert!(rendered[0].contains("\x1b["));
    }

    #[test]
    fn ansi_indent_defaults_to_one_and_can_be_overridden() {
        let dark = theme();
        let plain = row(vec![part("a", None)], NoticeTone::Success);
        let mut indented = row(vec![part("a", None)], NoticeTone::Success);
        indented.indent = Some(3);
        let mut block = TranscriptStatusBlock::new(&dark, &[plain, indented]);
        let rendered = block.render(40);
        assert_eq!(rendered[0], " a");
        assert_eq!(rendered[1], "   a");
    }

    #[test]
    fn native_is_a_status_block_of_notice_rows() {
        let dark = theme();
        let rows = vec![
            row(vec![part("a", None)], NoticeTone::Info),
            row(vec![part("b", None)], NoticeTone::Warning),
        ];
        let block = TranscriptStatusBlock::new(&dark, &rows);
        let described = block.describe_native(&DescribeContext::new(40));
        assert_eq!(described.kind, NodeKind::Col);
        assert_eq!(described.props.role(), Some("omp.status-block"));
        assert_eq!(described.children.len(), 2);
        assert_eq!(
            described.children[0].props.role(),
            Some("omp.notice.info")
        );
        assert_eq!(
            described.children[1].props.role(),
            Some("omp.notice.warning")
        );
        assert_eq!(described.children[1].props.tone(), Some(Tone::Warning));
    }

    #[test]
    fn native_message_spans_and_icon_are_carried() {
        let dark = theme();
        let mut rows = vec![row(vec![part("a", None)], NoticeTone::Success)];
        rows[0].native.icon = Some("download".to_string());
        rows[0].native.message = vec![Span::styled("saved".into(), "path")];
        let block = TranscriptStatusBlock::new(&dark, &rows);
        let described = block.describe_native(&DescribeContext::new(40));
        let row_node = &described.children[0];
        assert_eq!(row_node.children[0].props.get_str("name"), Some("download"));
        assert_eq!(
            row_node.children[1].props.spans(),
            Some(vec![Span::styled("saved".into(), "path")])
        );
    }

    #[test]
    fn empty_rows_describe_an_empty_status_block() {
        let dark = theme();
        let mut block = TranscriptStatusBlock::new(&dark, &[]);
        assert!(block.render(20).is_empty());
        let described = block.describe_native(&DescribeContext::new(20));
        assert_eq!(described.props.role(), Some("omp.status-block"));
        assert!(described.children.is_empty());
    }

    #[test]
    fn native_description_is_stable_across_calls() {
        let dark = theme();
        let rows = vec![row(vec![part("a", None)], NoticeTone::Info)];
        let block = TranscriptStatusBlock::new(&dark, &rows);
        let cx = DescribeContext::new(20);
        assert_eq!(block.describe_native(&cx), block.describe_native(&cx));
    }
}
