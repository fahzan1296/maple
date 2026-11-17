//! Shared engine for the ephemeral fullscreen transcript selectors (esc-esc
//! rewind, `/copy`, `/fork`).
//!
//! Reference: `l/src/chat/transcript-outline.ts`.
//!
//! Both selectors render the transcript into columns, outline the selected
//! block and draw a dotted frame around it; this module holds the parts they
//! share: prompt-zone stripping, row caching, target discovery, visibility
//! probes, the dotted frame, column composition and the position rail.
//!
//! Deviations:
//! - `ChatTranscriptBuilder` has no crate module yet, so
//!   [`append_outline_entries`] works through the [`OutlineTranscriptBuilder`]
//!   trait (`l/src/chat/chat-transcript-builder.ts` implements it).
//! - `OutlineRowCache` keys stripped rows on per-child row equality instead of
//!   JS array identity (same observable behaviour: unchanged rows reuse the
//!   stripped copy, a repaint re-strips).
//! - `describedVisibility` describes children with the crate's `rows`-fallback
//!   describe (components here cannot be probed for `NativeDescribe` through a
//!   trait object).
//! - `theme` is a `&Theme` parameter, and `ThemeColor` colours are CSS-style
//!   token strings.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::chat::transcript_entry::{
    is_user_request_entry, transcript_entry_message, user_message_label, user_turn_draft,
    AgentMessage, TranscriptEntryLike, UserContent,
};
use crate::component::Component;
use crate::native::describe::describe_child;
use crate::native::node::DescribeContext;
use crate::render::render_utils::pad_to_width;
use crate::text_shape::visible_width;
use crate::theme::Theme;

/// OSC 133 spans a user bubble wraps its rows in.
static OSC133_SPAN_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("\x1b\\]133;[^\x07\x1b]*(?:\x07|\x1b\\\\)").expect("OSC 133 span regex")
});

/// One selectable transcript item: a message entry plus its rendered block
/// range.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineTarget {
    /// Entry the selection resolves to; extended over trailing componentless
    /// tool results.
    pub entry_id: String,
    /// Entry that opened this turn (pre-fold) — anchor for tree lookups.
    pub turn_id: String,
    /// Real user prompt — drives user-turn jumps and role-specific actions.
    pub is_user_turn: bool,
    /// First transcript-container child rendered by this entry.
    pub start: usize,
    /// One past the last child rendered by this entry.
    pub end: usize,
    /// Entries this target spans: the turn plus any folded tool results.
    pub entries: Vec<TranscriptEntryLike>,
}

/// Composed rows of one column plus the outline's line range within them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposedColumn {
    /// The column's rows.
    pub lines: Vec<String>,
    /// First outlined row, or -1.
    pub sel_start: isize,
    /// One past the last outlined row, or -1.
    pub sel_end: isize,
}

/// Presentation of the dotted outline: stroke color and an optional caption
/// inset into the top rule.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutlineStyle {
    /// Stroke colour token.
    pub color: Option<String>,
    /// Short affordance label drawn into the top rule's right end.
    pub caption: Option<String>,
}

/// Copy-on-write removal of OSC 133 spans from a rendered row array.
pub fn strip_prompt_zones(rows: &[String]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            if row.contains("\x1b]133;") {
                OSC133_SPAN_REGEX.replace_all(row, "").into_owned()
            } else {
                row.clone()
            }
        })
        .collect()
}

/// Prompt-zone-stripped rows for one selector column.
///
/// Stripping every child's rows again per frame is pure waste on a long
/// session; a child whose rows are unchanged reuses its stripped copy.
#[derive(Default)]
pub struct OutlineRowCache {
    stripped: HashMap<usize, (Vec<String>, Vec<String>)>,
}

impl OutlineRowCache {
    /// An empty cache.
    pub fn new() -> Self {
        OutlineRowCache::default()
    }

    /// Stripped rows of every child, in order.
    pub fn rows(&mut self, children: &mut [Box<dyn Component>], width: u16) -> Vec<Vec<String>> {
        let mut columns = Vec::with_capacity(children.len());
        for (index, child) in children.iter_mut().enumerate() {
            let rows = child.render(width);
            if let Some((cached_rows, stripped)) = self.stripped.get(&index) {
                if *cached_rows == rows {
                    columns.push(stripped.clone());
                    continue;
                }
            }
            let stripped = strip_prompt_zones(&rows);
            self.stripped.insert(index, (rows, stripped.clone()));
            columns.push(stripped);
        }
        columns
    }

    /// Drop every cached column.
    pub fn invalidate(&mut self) {
        self.stripped.clear();
    }
}

/// What [`append_outline_entries`] needs from a transcript builder.
pub trait OutlineTranscriptBuilder {
    /// Number of container children.
    fn child_count(&self) -> usize;
    /// Append one entry to the container.
    fn append_entry(&mut self, entry: &TranscriptEntryLike);
    /// Whether container child `index` is a usage-row block.
    fn is_usage_row(&self, index: usize) -> bool;
}

/// Append `entries` to `builder`, returning the selectable targets they
/// produce. Tool results fold into the previous target; notices and hidden
/// messages that render nothing are skipped. Usage rows flushed at the head of
/// an append are attributed to the turn above.
pub fn append_outline_entries<B: OutlineTranscriptBuilder>(
    builder: &mut B,
    entries: &[TranscriptEntryLike],
) -> Vec<OutlineTarget> {
    let mut targets: Vec<OutlineTarget> = Vec::new();
    for entry in entries {
        let before = builder.child_count();
        builder.append_entry(entry);
        let after = builder.child_count();
        let mut start = before;
        while start < after && builder.is_usage_row(start) {
            if let Some(previous) = targets.last_mut() {
                if previous.end == start {
                    previous.end = start + 1;
                }
            }
            start += 1;
        }
        if is_tool_result_entry(entry) {
            if let Some(previous) = targets.last_mut() {
                previous.entry_id = entry_id(entry).to_string();
                previous.entries.push(entry.clone());
                if after > previous.end {
                    previous.end = after;
                }
                continue;
            }
        }
        if start >= after {
            continue;
        }
        targets.push(OutlineTarget {
            entry_id: entry_id(entry).to_string(),
            turn_id: entry_id(entry).to_string(),
            is_user_turn: is_user_turn_entry(entry),
            start,
            end: after,
            entries: vec![entry.clone()],
        });
    }
    targets
}

fn entry_id(entry: &TranscriptEntryLike) -> &str {
    match entry {
        TranscriptEntryLike::Message(message) => &message.id,
        TranscriptEntryLike::CustomMessage(custom) => &custom.id,
    }
}

/// Whether an entry is a persisted tool result (folded into the previous
/// target).
pub fn is_tool_result_entry(entry: &TranscriptEntryLike) -> bool {
    match entry {
        TranscriptEntryLike::Message(message) => match &message.message {
            AgentMessage::Other(value) => value.get("role").and_then(serde_json::Value::as_str) == Some("toolResult"),
            _ => false,
        },
        TranscriptEntryLike::CustomMessage(_) => false,
    }
}

/// Per-target "renders at least one non-blank row" flags at the given rows.
pub fn outline_visibility(child_rows: &[Vec<String>], targets: &[OutlineTarget]) -> Vec<bool> {
    targets
        .iter()
        .map(|target| {
            (target.start..target.end).any(|index| {
                child_rows.get(index).is_some_and(|rows| {
                    rows.iter()
                        .any(|row| !crate::ansi_scan::strip_ansi(row).trim().is_empty())
                })
            })
        })
        .collect()
}

/// Per-target "describes something that shows" flags for a TSP page, where
/// blocks describe nodes instead of rendering rows.
pub fn described_visibility(
    blocks: &mut [Box<dyn Component>],
    targets: &[OutlineTarget],
    cx: &DescribeContext,
) -> Vec<bool> {
    targets
        .iter()
        .map(|target| {
            (target.start..target.end).any(|index| {
                blocks
                    .get_mut(index)
                    .is_some_and(|block| shows(block.as_mut(), cx))
            })
        })
        .collect()
}

/// Whether a child describes a node that shows: a layout `col`/`row` only
/// through a child that does.
fn shows(child: &mut dyn Component, cx: &DescribeContext) -> bool {
    let node = describe_child(child, cx);
    if node.kind != crate::native::node::NodeKind::Col
        && node.kind != crate::native::node::NodeKind::Row
    {
        return true;
    }
    if !node.plain_text().trim().is_empty() {
        return true;
    }
    !node.children.is_empty()
}

/// Dotted horizontal rule with rounded corners, spanning the outline width.
pub fn outline_rule(
    theme: &Theme,
    left: &str,
    right: &str,
    inner_width: usize,
    color: Option<&str>,
    caption: Option<&str>,
) -> String {
    let color = color.unwrap_or("accent");
    let label = caption.map(|caption| format!(" {caption} ")).unwrap_or_default();
    let fill = (inner_width + 2).saturating_sub(visible_width(&label));
    let dotted = theme.box_dotted();
    let rule = left.to_string() + &dotted.horizontal.repeat(fill) + &label + right;
    match theme.fg(color, &rule) {
        Ok(styled) => styled,
        Err(_) => rule,
    }
}

/// Wrap pre-rendered rows in the dotted outline (rules above/below, `┆` sides).
pub fn outline_rows(
    theme: &Theme,
    rows: &[String],
    inner_width: usize,
    style: &OutlineStyle,
) -> Vec<String> {
    let color = style.color.as_deref().unwrap_or("accent");
    let dotted = theme.box_dotted();
    let glyph = |ch: &str| theme.fg(color, ch).unwrap_or_else(|_| ch.to_string());
    let vertical = glyph(&dotted.vertical);
    let round = theme.box_round();
    let mut lines = vec![outline_rule(
        theme,
        &round.top_left,
        &round.top_right,
        inner_width,
        Some(color),
        style.caption.as_deref(),
    )];
    for row in rows {
        let body = if inner_width > 0 {
            pad_to_width(row, inner_width, None)
        } else {
            String::new()
        };
        lines.push(format!("{vertical} {body} {vertical}"));
    }
    lines.push(outline_rule(
        theme,
        &round.bottom_left,
        &round.bottom_right,
        inner_width,
        Some(color),
        None,
    ));
    lines
}

/// Compose one column: gutter-prefixed rows for `child_rows[from, to)` with a
/// dotted outline around `targets[selected]`. `header` rows, when given, lead
/// the column.
pub fn compose_outline_column(
    theme: &Theme,
    child_rows: &[Vec<String>],
    from: usize,
    to: usize,
    targets: &[OutlineTarget],
    selected: usize,
    column_width: usize,
    header: Option<&[String]>,
    style: &OutlineStyle,
) -> ComposedColumn {
    let inner = column_width.saturating_sub(4).max(10);
    let mut lines: Vec<String> = header.map(<[String]>::to_vec).unwrap_or_default();
    let mut sel_start = -1isize;
    let mut sel_end = -1isize;
    let target = targets.get(selected);
    let mut index = from;
    while index < to {
        if let Some(target) = target {
            if index == target.start && target.end <= to {
                let mut segment = Vec::new();
                for child in target.start..target.end {
                    if let Some(rows) = child_rows.get(child) {
                        segment.extend(rows.iter().cloned());
                    }
                }
                let mut head = 0usize;
                let mut tail = segment.len();
                while head < tail && segment[head].trim().is_empty() {
                    head += 1;
                }
                while tail > head && segment[tail - 1].trim().is_empty() {
                    tail -= 1;
                }
                for _ in 0..head {
                    lines.push(String::new());
                }
                sel_start = lines.len() as isize;
                lines.extend(outline_rows(theme, &segment[head..tail], inner, style));
                sel_end = lines.len() as isize;
                for _ in tail..segment.len() {
                    lines.push(String::new());
                }
                index = target.end;
                continue;
            }
        }
        if let Some(rows) = child_rows.get(index) {
            for row in rows {
                lines.push(if row.is_empty() {
                    String::new()
                } else {
                    format!("  {row}")
                });
            }
        }
        index += 1;
    }
    ComposedColumn {
        lines,
        sel_start,
        sel_end,
    }
}

/// Centered position rail for horizontally windowed content: `… ○ ◉ ○ …`.
pub fn position_rail(
    theme: &Theme,
    count: usize,
    active: usize,
    more_left: bool,
    more_right: bool,
    width: usize,
) -> String {
    let radio = theme.radio();
    let mut dots = Vec::with_capacity(count);
    for index in 0..count {
        dots.push(if index == active {
            theme
                .fg("accent", &radio.selected)
                .unwrap_or_else(|_| radio.selected.clone())
        } else {
            theme
                .fg("dim", &radio.unselected)
                .unwrap_or_else(|_| radio.unselected.clone())
        });
    }
    let left = if more_left { "… " } else { "  " };
    let right = if more_right { " …" } else { "" };
    let rail = format!("{left}{}{right}", dots.join(" "));
    let pad = width.saturating_sub(visible_width(&rail)) / 2;
    format!("{}{rail}", " ".repeat(pad))
}

/// A turn the user can rewind past and re-edit: a user message with prompt
/// text, or a user-initiated custom message (skill / collab prompt).
pub fn is_user_turn_entry(entry: &TranscriptEntryLike) -> bool {
    match entry {
        TranscriptEntryLike::Message(message) => match &message.message {
            AgentMessage::User(user) => user_message_has_text(&user.content),
            _ => is_user_request_entry(Some(entry)),
        },
        TranscriptEntryLike::CustomMessage(_) => is_user_request_entry(Some(entry)),
    }
}

/// Single-line label for a user turn: its prompt text, or the custom message's
/// draft.
pub fn user_turn_label(entry: &TranscriptEntryLike) -> Option<String> {
    match transcript_entry_message(entry) {
        Some(AgentMessage::User(user)) => Some(user_message_label(&user.content)),
        _ => user_turn_draft(entry).map(|draft| user_message_label(&UserContent::Text(draft))),
    }
}

/// Whether a user message carries prompt text (string or text blocks).
pub fn user_message_has_text(content: &UserContent) -> bool {
    match content {
        UserContent::Text(text) => !text.trim().is_empty(),
        UserContent::Blocks(blocks) => blocks.iter().any(|block| match block {
            crate::chat::messages::MessageBlock::Text(text) => !text.text.trim().is_empty(),
            crate::chat::messages::MessageBlock::Image(_) => false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::transcript_entry::{SessionMessageEntryLike, UserMessage};
    use crate::theme::{builtin, create_theme, ColorMode, CreateThemeOptions};

    fn theme() -> Theme {
        create_theme(
            builtin::builtin_theme_json("dark").unwrap(),
            CreateThemeOptions {
                mode: Some(ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn user_entry(id: &str, text: &str) -> TranscriptEntryLike {
        TranscriptEntryLike::Message(SessionMessageEntryLike {
            id: id.into(),
            parent_id: None,
            timestamp: String::new(),
            message: AgentMessage::User(UserMessage {
                content: UserContent::Text(text.into()),
                timestamp: 0,
            }),
        })
    }

    fn tool_result_entry(id: &str) -> TranscriptEntryLike {
        TranscriptEntryLike::Message(SessionMessageEntryLike {
            id: id.into(),
            parent_id: None,
            timestamp: String::new(),
            message: AgentMessage::Other(serde_json::json!({"role": "toolResult"})),
        })
    }

    struct FakeBuilder {
        children: usize,
        usage_rows: Vec<bool>,
    }

    impl OutlineTranscriptBuilder for FakeBuilder {
        fn child_count(&self) -> usize {
            self.children
        }

        fn append_entry(&mut self, entry: &TranscriptEntryLike) {
            if is_tool_result_entry(entry) || entry_id(entry) == "hidden" {
                return;
            }
            self.children += 1;
            self.usage_rows.resize(self.children, false);
        }

        fn is_usage_row(&self, index: usize) -> bool {
            self.usage_rows.get(index).copied().unwrap_or(false)
        }
    }

    #[test]
    fn prompt_zones_are_stripped_from_rows() {
        let rows = vec![
            "\x1b]133;A\x07hello\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07".to_string(),
            "plain".to_string(),
        ];
        let stripped = strip_prompt_zones(&rows);
        assert_eq!(stripped[0], "hello");
        assert_eq!(stripped[1], "plain");
        assert_eq!(strip_prompt_zones(&[]).len(), 0);
    }

    #[test]
    fn row_cache_reuses_stripped_rows_for_unchanged_children() {
        struct Static(Vec<String>);
        impl Component for Static {
            fn render(&mut self, _width: u16) -> Vec<String> {
                self.0.clone()
            }
        }
        let mut cache = OutlineRowCache::new();
        let mut children: Vec<Box<dyn Component>> = vec![
            Box::new(Static(vec!["\x1b]133;A\x07a\x1b]133;B\x07".into()])),
            Box::new(Static(vec!["b".into()])),
        ];
        let first = cache.rows(&mut children, 20);
        assert_eq!(first[0][0], "a");
        let second = cache.rows(&mut children, 20);
        assert_eq!(first, second);
        cache.invalidate();
        assert_eq!(cache.rows(&mut children, 20)[1][0], "b");
    }

    #[test]
    fn targets_skip_componentless_entries_and_fold_tool_results() {
        let mut builder = FakeBuilder {
            children: 0,
            usage_rows: Vec::new(),
        };
        let entries = vec![
            user_entry("a", "hi"),
            tool_result_entry("t1"),
            user_entry("hidden", "nothing renders"),
            user_entry("b", "again"),
        ];
        let targets = append_outline_entries(&mut builder, &entries);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].entry_id, "t1");
        assert_eq!(targets[0].entries.len(), 2);
        assert!(targets[0].is_user_turn);
        assert_eq!(targets[1].entry_id, "b");
        assert_eq!(targets[1].start, 1);
    }

    #[test]
    fn visibility_flags_blank_targets() {
        let child_rows = vec![vec!["".to_string()], vec!["  ".to_string(), "x".to_string()]];
        let targets = vec![
            OutlineTarget {
                entry_id: "a".into(),
                turn_id: "a".into(),
                is_user_turn: false,
                start: 0,
                end: 1,
                entries: Vec::new(),
            },
            OutlineTarget {
                entry_id: "b".into(),
                turn_id: "b".into(),
                is_user_turn: false,
                start: 0,
                end: 2,
                entries: Vec::new(),
            },
        ];
        assert_eq!(outline_visibility(&child_rows, &targets), [false, true]);
    }

    #[test]
    fn columns_outline_only_the_selected_target() {
        let theme = theme();
        let child_rows = vec![
            vec!["".to_string()],
            vec!["body".to_string()],
            vec!["".to_string()],
            vec!["other".to_string()],
        ];
        let targets = vec![OutlineTarget {
            entry_id: "a".into(),
            turn_id: "a".into(),
            is_user_turn: true,
            start: 1,
            end: 3,
            entries: Vec::new(),
        }];
        let column = compose_outline_column(
            &theme,
            &child_rows,
            0,
            child_rows.len(),
            &targets,
            0,
            40,
            None,
            &OutlineStyle::default(),
        );
        assert_eq!(column.sel_start, 1);
        assert_eq!(column.sel_end, 4);
        assert!(column.lines[0].contains('╭'));
        assert!(column.lines.iter().any(|line| line.contains("body")));
        assert!(column.lines.iter().any(|line| line.starts_with("  other")));
    }

    #[test]
    fn position_rail_marks_the_active_dot() {
        let theme = theme();
        let rail = position_rail(&theme, 3, 1, true, false, 20);
        let visible = crate::ansi_scan::strip_ansi(&rail);
        assert!(visible.contains('◉'));
        assert!(visible.starts_with(' '));
        assert!(visible.contains("…"));
    }

    #[test]
    fn user_turns_need_prompt_text_and_labels_collapse_whitespace() {
        let entry = user_entry("a", "  hello\n world ");
        assert!(is_user_turn_entry(&entry));
        assert_eq!(user_turn_label(&entry).as_deref(), Some("hello world"));
        let blank = user_entry("b", "   ");
        assert!(!is_user_turn_entry(&blank));
        let tool = tool_result_entry("t");
        assert!(!is_user_turn_entry(&tool));
    }

    #[test]
    fn rules_carry_their_caption() {
        let theme = theme();
        let rule = outline_rule(&theme, "╭", "╮", 10, None, Some("3 blocks →"));
        let plain = crate::ansi_scan::strip_ansi(&rule);
        assert!(plain.starts_with('╭'));
        assert!(plain.ends_with('╮'));
        assert!(plain.contains(" 3 blocks → "));
    }
}
