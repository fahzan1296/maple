//! Unified diff parsing and the [`DiffView`] renderer.
//!
//! Ported from `l/src/chrome/diff.ts`. The compact diff text accepted here is
//! omp's `+12|added` / `-12|removed` / ` 12|context` row format (plus the
//! legacy `+12 added` form). The TS renderer batch-highlights context lines
//! through pi-natives' syntax highlighter; no highlighter is available under
//! the crate's dependency set, so context lines render with the context style
//! and indent visualization only.

use crate::component::Component;
use crate::style::{Color, Style};
use crate::text_shape::{truncate_to_width, Ellipsis};

/// Display columns one tab expands to.
const TAB_WIDTH: usize = 3;

/// Line kind encoded in the leading marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffPrefix {
    /// `+` — an added line.
    Added,
    /// `-` — a removed line.
    Removed,
    /// ` ` — an unchanged context line.
    Context,
}

impl DiffPrefix {
    /// The marker character used in the compact diff text.
    pub fn marker(self) -> char {
        match self {
            DiffPrefix::Added => '+',
            DiffPrefix::Removed => '-',
            DiffPrefix::Context => ' ',
        }
    }
}

/// One parsed compact diff row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    /// The row's marker.
    pub prefix: DiffPrefix,
    /// The raw line-number field (may carry leading spaces, may be empty).
    pub line_number: String,
    /// The row content after the number.
    pub content: String,
}

/// A run of consecutive rows, as the native diff node carries it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffHunk {
    /// Old-file line number of the first row.
    pub old_start: i64,
    /// New-file line number of the first row.
    pub new_start: i64,
    /// Rows as `marker + content`.
    pub lines: Vec<String>,
}

/// Parse one compact diff row.
///
/// Accepts the canonical `+12|content` form and the legacy `+12 content` form.
/// Returns `None` when the row does not start with a diff marker.
pub fn parse_diff_line(line: &str) -> Option<DiffLine> {
    let chars: Vec<char> = line.chars().collect();
    let first = *chars.first()?;
    let prefix = match first {
        '+' => DiffPrefix::Added,
        '-' => DiffPrefix::Removed,
        ' ' => DiffPrefix::Context,
        _ => return None,
    };

    // Canonical: `^([+-\s])(\s*\d+)\|(.*)$`
    if let Some(pipe) = chars.iter().position(|&c| c == '|') {
        if pipe > 1 {
            let number = &chars[1..pipe];
            if let Some(first_digit) = number.iter().position(|c| c.is_ascii_digit()) {
                let leading_ok = number[..first_digit].iter().all(|c| c.is_whitespace());
                let digits_ok = number[first_digit..].iter().all(|c| c.is_ascii_digit());
                if leading_ok && digits_ok {
                    return Some(DiffLine {
                        prefix,
                        line_number: number.iter().collect(),
                        content: chars[pipe + 1..].iter().collect(),
                    });
                }
            }
        }
    }

    // Legacy: `^([+-\s])(?:(\s*\d+)\s)?(.*)$`
    let mut index = 1;
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    let digits_start = index;
    while index < chars.len() && chars[index].is_ascii_digit() {
        index += 1;
    }
    if index > digits_start && index < chars.len() && chars[index].is_whitespace() {
        return Some(DiffLine {
            prefix,
            line_number: chars[1..index].iter().collect(),
            content: chars[index + 1..].iter().collect(),
        });
    }

    Some(DiffLine {
        prefix,
        line_number: String::new(),
        content: chars[1..].iter().collect(),
    })
}

/// Parse the runs of consecutive changed/context rows the native diff node
/// carries. A blank row, `...`/`…`, or a line-number jump closes the current
/// run; runs without a `+` or `-` row are dropped.
pub fn parse_hunks(diff_text: &str) -> Vec<DiffHunk> {
    fn close(
        hunks: &mut Vec<DiffHunk>,
        lines: &mut Vec<String>,
        changed: &mut bool,
        old_start: i64,
        new_start: i64,
    ) {
        if *changed {
            hunks.push(DiffHunk {
                old_start,
                new_start,
                lines: std::mem::take(lines),
            });
        } else {
            lines.clear();
        }
        *changed = false;
    }

    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut changed = false;
    let mut old_start: i64 = 1;
    let mut new_start: i64 = 1;
    let mut next_old: i64 = 1;
    let mut next_new: i64 = 1;

    for raw in diff_text.split('\n') {
        let parsed = parse_diff_line(raw);
        let trimmed = raw.trim();
        let collapse = parsed.as_ref().is_some_and(|parsed| {
            parsed.prefix == DiffPrefix::Context
                && (parsed.content == "..." || parsed.content == "…")
        });
        if parsed.is_none() || collapse {
            if trimmed.is_empty() || trimmed == "..." || trimmed == "…" {
                close(&mut hunks, &mut lines, &mut changed, old_start, new_start);
            }
            continue;
        }
        let parsed = parsed.unwrap();

        let number = parsed.line_number.trim().parse::<i64>().ok();
        let added = parsed.prefix == DiffPrefix::Added;
        if let Some(number) = number {
            let expected = if added { next_new } else { next_old };
            if number != expected {
                close(&mut hunks, &mut lines, &mut changed, old_start, new_start);
                // The lines skipped are unchanged: both sides move by as many.
                let skipped = number - expected;
                next_old += skipped;
                next_new += skipped;
            }
        }
        if lines.is_empty() {
            old_start = next_old;
            new_start = next_new;
        }
        if parsed.prefix != DiffPrefix::Added {
            next_old += 1;
        }
        if parsed.prefix != DiffPrefix::Removed {
            next_new += 1;
        }
        changed |= parsed.prefix != DiffPrefix::Context;
        lines.push(format!("{}{}", parsed.prefix.marker(), parsed.content));
    }
    close(&mut hunks, &mut lines, &mut changed, old_start, new_start);
    hunks
}

/// Theme for diff rendering.
#[derive(Clone, Debug)]
pub struct DiffTheme {
    /// Added-line paint.
    pub added: Style,
    /// Removed-line paint.
    pub removed: Style,
    /// Context-line paint.
    pub context: Style,
    /// Line-number gutter paint.
    pub gutter: Style,
    /// Inverse paint for intra-line word changes.
    pub inverse: Style,
    /// Indent visualization paint.
    pub indent: Style,
}

impl Default for DiffTheme {
    fn default() -> Self {
        DiffTheme {
            added: Style::fg(Color::Green),
            removed: Style::fg(Color::Red),
            context: Style::fg(Color::DarkGray),
            gutter: Style::fg(Color::DarkGray),
            inverse: Style::default().reverse(),
            indent: Style::default().dim(),
        }
    }
}

/// Added/removed/hunk counts for a compact diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffStats {
    /// Number of `+` rows.
    pub added: usize,
    /// Number of `-` rows.
    pub removed: usize,
    /// Number of runs containing a change.
    pub hunks: usize,
}

/// Count added rows, removed rows, and change runs.
pub fn diff_stats(diff_text: &str) -> DiffStats {
    let mut stats = DiffStats::default();
    let mut in_hunk = false;
    for line in diff_text.split('\n') {
        let parsed = parse_diff_line(line);
        let is_added = parsed.as_ref().is_some_and(|p| p.prefix == DiffPrefix::Added);
        let is_removed = parsed.as_ref().is_some_and(|p| p.prefix == DiffPrefix::Removed);
        if is_added {
            stats.added += 1;
        }
        if is_removed {
            stats.removed += 1;
        }
        let is_change = is_added || is_removed;
        if is_change && !in_hunk {
            stats.hunks += 1;
            in_hunk = true;
        } else if !is_change {
            in_hunk = false;
        }
    }
    stats
}

fn replace_tabs(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB_WIDTH))
}

/// Visualize leading whitespace with dim glyphs: tabs become ` → ` and spaces
/// become `·`. Remaining tabs expand to spaces.
fn visualize_indent(text: &str, theme: &DiffTheme) -> String {
    let indent_len = text.len() - text.trim_start_matches([' ', '\t']).len();
    if indent_len == 0 {
        return replace_tabs(text);
    }
    let (indent, rest) = text.split_at(indent_len);
    let left_padding = TAB_WIDTH / 2;
    let right_padding = TAB_WIDTH - left_padding - 1;
    let mut out = String::new();
    for ch in indent.chars() {
        if ch == '\t' {
            out.push_str(&theme.indent.paint(&format!(
                "{}→{}",
                " ".repeat(left_padding),
                " ".repeat(right_padding)
            )));
        } else {
            out.push_str(&theme.indent.paint("·"));
        }
    }
    out.push_str(&replace_tabs(rest));
    out
}

/// Split into whitespace and non-whitespace runs.
fn word_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_space = false;
    for ch in text.chars() {
        let space = ch.is_whitespace();
        if current.is_empty() {
            current_space = space;
        } else if space != current_space {
            tokens.push(std::mem::take(&mut current));
            current_space = space;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Longest-common-subsequence keep flags for two token sequences.
fn lcs_flags(left: &[String], right: &[String]) -> (Vec<bool>, Vec<bool>) {
    let n = left.len();
    let m = right.len();
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if left[i] == right[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut keep_left = vec![false; n];
    let mut keep_right = vec![false; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if left[i] == right[j] {
            keep_left[i] = true;
            keep_right[j] = true;
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (keep_left, keep_right)
}

/// Render one side of a word-level intra-line diff.
fn render_word_side(tokens: &[String], keep: &[bool], theme: &DiffTheme) -> String {
    let mut out = String::new();
    let mut first_changed = true;
    for (index, token) in tokens.iter().enumerate() {
        if keep.get(index).copied().unwrap_or(false) {
            out.push_str(token);
            continue;
        }
        let value = if first_changed {
            first_changed = false;
            let ws_len = token.len() - token.trim_start_matches(|c: char| c.is_whitespace()).len();
            let (ws, rest) = token.split_at(ws_len);
            out.push_str(ws);
            rest.to_string()
        } else {
            token.clone()
        };
        if !value.is_empty() {
            out.push_str(&theme.inverse.paint(&value));
        }
    }
    out
}

/// Word-level intra-line diff for a single removed/added pair.
fn intra_line_diff(old: &str, new: &str, theme: &DiffTheme) -> (String, String) {
    let old_tokens = word_tokens(old);
    let new_tokens = word_tokens(new);
    if old_tokens.len() > 200 || new_tokens.len() > 200 {
        return (old.to_string(), new.to_string());
    }
    let (keep_old, keep_new) = lcs_flags(&old_tokens, &new_tokens);
    (
        render_word_side(&old_tokens, &keep_old, theme),
        render_word_side(&new_tokens, &keep_new, theme),
    )
}

/// Format one row with a padded gutter and a `│` separator.
fn format_line(
    prefix: DiffPrefix,
    line_number: &str,
    content: &str,
    gutter_width: usize,
    previous: &mut String,
) -> String {
    if line_number.trim().is_empty() {
        previous.clear();
        return format!("{}{}", prefix.marker(), content);
    }
    let trimmed = line_number.trim();
    let display = if trimmed == previous.as_str() { "" } else { trimmed };
    *previous = trimmed.to_string();
    let gutter_text = if display.is_empty() {
        prefix.marker().to_string()
    } else {
        format!("{}{}", prefix.marker(), display)
    };
    let padded = format!("{:>width$}", gutter_text, width = gutter_width + 1);
    format!("{padded}│{content}")
}

/// Render a compact diff into styled rows.
///
/// Rows are painted with the theme's added/removed/context styles; callers
/// clamp them with [`crate::text_shape::truncate_to_width`] through
/// [`DiffView`], which guarantees the component width contract.
pub fn render_diff(diff_text: &str, theme: &DiffTheme) -> Vec<String> {
    let lines: Vec<&str> = diff_text.split('\n').collect();
    let parsed: Vec<Option<DiffLine>> = lines.iter().map(|line| parse_diff_line(line)).collect();
    let gutter_width = parsed
        .iter()
        .map(|line| line.as_ref().map_or(0, |p| p.line_number.trim().len()))
        .max()
        .unwrap_or(0)
        .max(3);

    let mut previous = String::new();
    let mut result = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let Some(parsed_line) = parsed[index].as_ref() else {
            previous.clear();
            let trimmed = line.trim();
            let is_gap = trimmed.is_empty() || trimmed == "..." || trimmed == "…";
            let content = if is_gap { "…".to_string() } else { replace_tabs(line) };
            result.push(theme.context.paint(&content));
            index += 1;
            continue;
        };

        match parsed_line.prefix {
            DiffPrefix::Removed => {
                let mut removed = Vec::new();
                while index < lines.len() {
                    match parsed[index].as_ref() {
                        Some(p) if p.prefix == DiffPrefix::Removed => {
                            removed.push(p.clone());
                            index += 1;
                        }
                        _ => break,
                    }
                }
                let mut added = Vec::new();
                while index < lines.len() {
                    match parsed[index].as_ref() {
                        Some(p) if p.prefix == DiffPrefix::Added => {
                            added.push(p.clone());
                            index += 1;
                        }
                        _ => break,
                    }
                }
                if removed.len() == 1 && added.len() == 1 {
                    let old = replace_tabs(&removed[0].content);
                    let new = replace_tabs(&added[0].content);
                    let (removed_line, added_line) = intra_line_diff(&old, &new, theme);
                    result.push(theme.removed.paint(&format_line(
                        DiffPrefix::Removed,
                        &removed[0].line_number,
                        &visualize_indent(&removed_line, theme),
                        gutter_width,
                        &mut previous,
                    )));
                    result.push(theme.added.paint(&format_line(
                        DiffPrefix::Added,
                        &added[0].line_number,
                        &visualize_indent(&added_line, theme),
                        gutter_width,
                        &mut previous,
                    )));
                } else {
                    for line in &removed {
                        result.push(theme.removed.paint(&format_line(
                            DiffPrefix::Removed,
                            &line.line_number,
                            &visualize_indent(&line.content, theme),
                            gutter_width,
                            &mut previous,
                        )));
                    }
                    for line in &added {
                        result.push(theme.added.paint(&format_line(
                            DiffPrefix::Added,
                            &line.line_number,
                            &visualize_indent(&line.content, theme),
                            gutter_width,
                            &mut previous,
                        )));
                    }
                }
            }
            DiffPrefix::Added => {
                result.push(theme.added.paint(&format_line(
                    DiffPrefix::Added,
                    &parsed_line.line_number,
                    &visualize_indent(&parsed_line.content, theme),
                    gutter_width,
                    &mut previous,
                )));
                index += 1;
            }
            DiffPrefix::Context => {
                result.push(theme.context.paint(&format_line(
                    DiffPrefix::Context,
                    &parsed_line.line_number,
                    &visualize_indent(&parsed_line.content, theme),
                    gutter_width,
                    &mut previous,
                )));
                index += 1;
            }
        }
    }
    result
}

/// A rendered compact diff, clamped to the viewport width.
pub struct DiffView {
    text: String,
    theme: DiffTheme,
    memo: Option<(u16, Vec<String>)>,
}

impl Default for DiffView {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl DiffView {
    /// An empty diff view.
    pub fn new(text: impl Into<String>) -> Self {
        DiffView {
            text: text.into(),
            theme: DiffTheme::default(),
            memo: None,
        }
    }

    /// Replace the diff text.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.memo = None;
    }

    /// The diff text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: DiffTheme) {
        self.theme = theme;
        self.memo = None;
    }

    /// The theme.
    pub fn theme(&self) -> &DiffTheme {
        &self.theme
    }

    /// Added/removed/hunk counts for the current text.
    pub fn stats(&self) -> DiffStats {
        diff_stats(&self.text)
    }
}

impl Component for DiffView {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return Vec::new();
        }
        if let Some((memo_width, rows)) = &self.memo {
            if *memo_width == width {
                return rows.clone();
            }
        }
        let rows: Vec<String> = render_diff(&self.text, &self.theme)
            .into_iter()
            .map(|row| truncate_to_width(&row, width as usize, Ellipsis::Omit))
            .collect();
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

    #[test]
    fn parses_canonical_rows() {
        let line = parse_diff_line("+12|added").unwrap();
        assert_eq!(line.prefix, DiffPrefix::Added);
        assert_eq!(line.line_number, "12");
        assert_eq!(line.content, "added");

        let context = parse_diff_line(" 7|ctx").unwrap();
        assert_eq!(context.prefix, DiffPrefix::Context);
        assert_eq!(context.line_number, "7");
        assert_eq!(context.content, "ctx");
    }

    #[test]
    fn parses_legacy_rows() {
        let line = parse_diff_line("-12 removed").unwrap();
        assert_eq!(line.prefix, DiffPrefix::Removed);
        assert_eq!(line.line_number, "12");
        assert_eq!(line.content, "removed");

        let bare = parse_diff_line("+ added").unwrap();
        assert_eq!(bare.line_number, "");
        assert_eq!(bare.content, " added");

        let empty = parse_diff_line("+").unwrap();
        assert_eq!(empty.content, "");
        assert_eq!(empty.line_number, "");
    }

    #[test]
    fn rejects_non_diff_rows() {
        assert!(parse_diff_line("diff --git a b").is_none());
        assert!(parse_diff_line("").is_none());
    }

    #[test]
    fn hunks_close_on_gaps_and_number_jumps() {
        let text = "+1|a\n+2|b\n\n-5|c\n-6|d\n 7|e\n...\n+20|f\n";
        let hunks = parse_hunks(text);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].lines, vec!["+a", "+b"]);
        assert_eq!(hunks[1].old_start, 5);
        assert_eq!(hunks[1].new_start, 7);
        assert_eq!(hunks[1].lines, vec!["-c", "-d", " e"]);
        assert_eq!(hunks[2].old_start, 20);
        assert_eq!(hunks[2].new_start, 20);
        assert_eq!(hunks[2].lines, vec!["+f"]);
    }

    #[test]
    fn hunks_drop_context_only_runs() {
        let text = " 1|a\n 2|b\n";
        assert!(parse_hunks(text).is_empty());
    }

    #[test]
    fn render_marks_added_removed_and_context() {
        let theme = DiffTheme::default();
        let rows = render_diff("+1|added\n-1|removed\n 1|ctx", &theme);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], "\x1b[32m  +1│added\x1b[0m");
        assert_eq!(rows[1], "\x1b[31m   -│removed\x1b[0m");
        assert_eq!(rows[2], "\x1b[90m    │ctx\x1b[0m");
    }

    #[test]
    fn render_blanks_duplicate_line_numbers() {
        let theme = DiffTheme::default();
        let rows = render_diff("-1|old\n+1|new", &theme);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains("  -1│"), "{}", rows[0]);
        assert!(rows[0].contains("old"), "{}", rows[0]);
        assert!(rows[1].contains("   +│"), "{}", rows[1]);
        assert!(rows[1].contains("new"), "{}", rows[1]);
    }

    #[test]
    fn render_visualizes_indent() {
        let theme = DiffTheme::default();
        let rows = render_diff("+1|\tcode", &theme);
        assert!(rows[0].contains("\u{2192}"), "{}", rows[0]);
        assert!(rows[0].contains("code"), "{}", rows[0]);
    }

    #[test]
    fn render_marks_gap_rows() {
        let theme = DiffTheme::default();
        let rows = render_diff("+1|a\n\n+2|b", &theme);
        assert_eq!(rows[1], "\x1b[90m…\x1b[0m");
    }

    #[test]
    fn stats_count_changes() {
        let stats = diff_stats("+1|a\n+2|b\n-2|c\n 3|d\n\n-9|e\n");
        assert_eq!(stats.added, 2);
        assert_eq!(stats.removed, 2);
        assert_eq!(stats.hunks, 2);
    }

    #[test]
    fn view_clamps_rows_to_width() {
        let mut view = DiffView::new("+1|a very long added line\n");
        for width in [1u16, 4, 8, 20] {
            for row in view.render(width) {
                assert!(
                    crate::text_shape::visible_width(&row) <= width as usize,
                    "width {width}: {row:?}"
                );
            }
        }
        assert!(view.render(0).is_empty());
    }

    #[test]
    fn empty_diff_renders_no_rows() {
        let mut view = DiffView::new("");
        assert_eq!(view.render(10), vec!["\x1b[90m…\x1b[0m".to_string()]);
        assert_eq!(view.stats(), DiffStats::default());
    }
}
