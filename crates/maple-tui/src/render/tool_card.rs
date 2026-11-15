//! Tool call card: a titled, bordered frame whose body carries status, diff,
//! and output sections.
//!
//! [`render_status_line`] reproduces the shared status header primitive (the
//! `render/status-line.ts` module is not part of this crate yet, so the
//! primitive lives here until it lands). [`ToolCard`] composes its frame from
//! [`crate::render::output_block`].

use crate::component::Component;
use crate::keys::Key;
use crate::render::output_block::{render_output_block, OutputBlockOptions, OutputSection};
use crate::render::render_utils::{
    bold_text, format_status_icon, get_diff_stats, paint, replace_tabs,
    split_terminal_output_lines, state_bg_color, style_tool_output_line, wrap_brackets,
    DiffStats, State, ToolUiStatus, ToolUIColor,
};
use crate::text_shape::visible_width;
use crate::theme::{Theme, ThemeBg, ThemeColor};

/// Lifecycle distinctions shared by tool call and result cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolCardPhase {
    #[default]
    Pending,
    Running,
    Partial,
    Success,
    Warning,
    Error,
    Info,
}

impl ToolCardPhase {
    /// The output-frame state this phase maps onto.
    pub fn state(self) -> Option<State> {
        match self {
            ToolCardPhase::Partial => Some(State::Pending),
            ToolCardPhase::Info => None,
            ToolCardPhase::Pending => Some(State::Pending),
            ToolCardPhase::Running => Some(State::Running),
            ToolCardPhase::Success => Some(State::Success),
            ToolCardPhase::Warning => Some(State::Warning),
            ToolCardPhase::Error => Some(State::Error),
        }
    }

    /// The status glyph for this phase.
    pub fn status(self) -> ToolUiStatus {
        match self {
            ToolCardPhase::Pending | ToolCardPhase::Partial => ToolUiStatus::Pending,
            ToolCardPhase::Running => ToolUiStatus::Running,
            ToolCardPhase::Success => ToolUiStatus::Success,
            ToolCardPhase::Warning => ToolUiStatus::Warning,
            ToolCardPhase::Error => ToolUiStatus::Error,
            ToolCardPhase::Info => ToolUiStatus::Info,
        }
    }
}

/// A single-line status header.
#[derive(Clone, Debug, Default)]
pub struct StatusLine {
    /// Icon status; `None` omits the leading glyph.
    pub icon: Option<ToolUiStatus>,
    /// Pre-rendered glyph replacing the status icon.
    pub icon_override: Option<String>,
    /// Spinner frame index used while running.
    pub spinner_frame: Option<usize>,
    pub title: String,
    pub title_color: Option<ThemeColor>,
    pub description: Option<String>,
    /// Short right-aligned tag.
    pub badge: Option<(String, ToolUIColor)>,
    pub meta: Vec<String>,
}

impl StatusLine {
    /// A header with just a title.
    pub fn new(title: impl Into<String>) -> Self {
        StatusLine {
            title: title.into(),
            ..StatusLine::default()
        }
    }

    /// Set the title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the status icon.
    pub fn with_icon(mut self, icon: ToolUiStatus) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set the description drawn after a colon.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

fn flatten_for_header(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_newline = false;
    for ch in text.chars() {
        match ch {
            '\r' | '\n' => {
                if !previous_newline {
                    out.push(' ');
                }
                previous_newline = true;
            }
            other => {
                out.push(other);
                previous_newline = false;
            }
        }
    }
    out
}

/// Render a single-line status header, flattening multiline fields.
pub fn render_status_line(line: &StatusLine, theme: &Theme) -> String {
    let icon = match (&line.icon_override, line.icon) {
        (Some(override_icon), _) => override_icon.clone(),
        (None, Some(icon)) => format_status_icon(icon, theme, line.spinner_frame),
        (None, None) => String::new(),
    };
    let title_color = line.title_color.unwrap_or(ThemeColor::Accent);
    let title = paint(theme, title_color, &flatten_for_header(&line.title));
    let mut out = if icon.is_empty() {
        title
    } else {
        format!("{icon} {title}")
    };
    if let Some(description) = &line.description {
        out.push_str(": ");
        out.push_str(&paint(
            theme,
            ThemeColor::Muted,
            &flatten_for_header(description),
        ));
    }
    if let Some((label, color)) = &line.badge {
        out.push(' ');
        out.push_str(&paint(
            theme,
            color_token(*color),
            &wrap_brackets(&flatten_for_header(label), theme),
        ));
    }
    let meta: Vec<String> = line
        .meta
        .iter()
        .map(|entry| flatten_for_header(entry))
        .filter(|entry| !entry.trim().is_empty())
        .collect();
    if !meta.is_empty() {
        out.push(' ');
        out.push_str(&paint(
            theme,
            ThemeColor::Dim,
            &meta.join(theme.symbol("sep.dot")),
        ));
    }
    out
}

fn color_token(color: ToolUIColor) -> ThemeColor {
    match color {
        ToolUIColor::Success => ThemeColor::Success,
        ToolUIColor::Error => ThemeColor::Error,
        ToolUIColor::Warning => ThemeColor::Warning,
        ToolUIColor::Accent => ThemeColor::Accent,
        ToolUIColor::Muted => ThemeColor::Muted,
    }
}

/// Style one unified-diff row with its theme token.
pub fn style_diff_line(line: &str, theme: &Theme) -> String {
    let body = replace_tabs(line);
    if body.starts_with("+++") || body.starts_with("---") {
        return paint(theme, ThemeColor::ToolDiffContext, &bold_text(&body));
    }
    if body.starts_with("@@") {
        return paint(theme, ThemeColor::ToolDiffContext, &body);
    }
    if body.starts_with('+') {
        return paint(theme, ThemeColor::ToolDiffAdded, &body);
    }
    if body.starts_with('-') {
        return paint(theme, ThemeColor::ToolDiffRemoved, &body);
    }
    paint(theme, ThemeColor::ToolDiffContext, &body)
}

/// A titled tool card with status, diff, and output sections.
pub struct ToolCard {
    theme: Theme,
    status: StatusLine,
    phase: ToolCardPhase,
    body: Vec<String>,
    diff: Vec<String>,
    output: Vec<String>,
    footer_meta: String,
    collapsed: bool,
    collapsed_max_lines: usize,
    border_color: Option<ThemeColor>,
    padding: usize,
    show_diff: bool,
    show_output: bool,
    cache: Option<(usize, Vec<String>)>,
}


impl ToolCard {
    /// Create a card bound to `theme` with a title.
    pub fn new(theme: Theme, status: StatusLine) -> Self {
        ToolCard {
            theme,
            status,
            phase: ToolCardPhase::Info,
            body: Vec::new(),
            diff: Vec::new(),
            output: Vec::new(),
            footer_meta: String::new(),
            collapsed: false,
            collapsed_max_lines: 6,
            border_color: None,
            padding: 1,
            show_diff: true,
            show_output: true,
            cache: None,
        }
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.cache = None;
    }

    /// The current theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Set the card title.
    pub fn set_title(&mut self, title: impl Into<String>) -> &mut Self {
        self.status.title = title.into();
        self.cache = None;
        self
    }

    /// Set the lifecycle phase (icon + frame tone).
    pub fn set_status(&mut self, phase: ToolCardPhase) -> &mut Self {
        self.phase = phase;
        self.status.icon = Some(phase.status());
        self.cache = None;
        self
    }

    /// Set the header description.
    pub fn set_description(&mut self, description: impl Into<String>) -> &mut Self {
        self.status.description = Some(description.into());
        self.cache = None;
        self
    }

    /// Replace the body rows.
    pub fn set_body(&mut self, lines: Vec<String>) -> &mut Self {
        self.body = lines;
        self.cache = None;
        self
    }

    /// Replace the diff body from raw unified-diff text.
    pub fn set_diff(&mut self, diff: &str) -> &mut Self {
        self.diff = if diff.is_empty() {
            Vec::new()
        } else {
            diff.split('\n').map(str::to_string).collect()
        };
        self.cache = None;
        self
    }

    /// Append a diff chunk.
    pub fn append_diff(&mut self, chunk: &str) -> &mut Self {
        self.diff.extend(chunk.split('\n').map(str::to_string));
        self.cache = None;
        self
    }
    pub fn set_output(&mut self, output: &str) -> &mut Self {
        self.output = split_terminal_output_lines(output);
        self.cache = None;
        self
    }

    /// Append output text, merging a continuation into the last row.
    pub fn append_output(&mut self, chunk: &str) -> &mut Self {
        let mut lines = split_terminal_output_lines(chunk);
        if !self.output.is_empty() && !chunk.starts_with('\n') && !lines.is_empty() {
            let tail = self.output.pop().unwrap_or_default();
            let first = lines.remove(0);
            self.output.push(format!("{tail}{first}"));
        }
        self.output.append(&mut lines);
        self.cache = None;
        self
    }

    /// Set the facts drawn inside the bottom border.
    pub fn set_footer_meta(&mut self, meta: impl Into<String>) -> &mut Self {
        self.footer_meta = meta.into();
        self.cache = None;
        self
    }

    /// Collapse or expand the body.
    pub fn set_collapsed(&mut self, collapsed: bool) -> &mut Self {
        self.collapsed = collapsed;
        self.cache = None;
        self
    }

    /// Toggle the collapsed state.
    pub fn toggle_collapsed(&mut self) -> &mut Self {
        self.collapsed = !self.collapsed;
        self.cache = None;
        self
    }

    /// Whether the body is collapsed.
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// Set the collapsed row budget.
    pub fn set_collapsed_max_lines(&mut self, max: usize) -> &mut Self {
        self.collapsed_max_lines = max;
        self.cache = None;
        self
    }

    /// Override the frame border color.
    pub fn set_border_color(&mut self, color: Option<ThemeColor>) -> &mut Self {
        self.border_color = color;
        self.cache = None;
        self
    }

    /// Set the content padding.
    pub fn set_padding(&mut self, padding: usize) -> &mut Self {
        self.padding = padding;
        self.cache = None;
        self
    }

    /// Show or hide the diff section.
    pub fn set_show_diff(&mut self, show: bool) -> &mut Self {
        self.show_diff = show;
        self.cache = None;
        self
    }

    /// Show or hide the output section.
    pub fn set_show_output(&mut self, show: bool) -> &mut Self {
        self.show_output = show;
        self.cache = None;
        self
    }

    /// The current phase.
    pub fn phase(&self) -> ToolCardPhase {
        self.phase
    }

    /// Diff statistics for the current diff body.
    pub fn diff_stats(&self) -> DiffStats {
        get_diff_stats(&self.diff.join("\n"))
    }

    fn sections(&self) -> Vec<OutputSection> {
        let mut sections = Vec::new();
        if !self.body.is_empty() {
            sections.push(OutputSection::new(self.body.clone()));
        }
        if self.show_diff && !self.diff.is_empty() {
            sections.push(OutputSection::labeled(
                paint(&self.theme, ThemeColor::ToolTitle, "Diff"),
                self.diff
                    .iter()
                    .map(|line| style_diff_line(line, &self.theme))
                    .collect(),
            ));
        }
        if self.show_output && !self.output.is_empty() {
            sections.push(OutputSection::labeled(
                paint(&self.theme, ThemeColor::ToolTitle, "Output"),
                self.output
                    .iter()
                    .map(|line| style_tool_output_line(line, &self.theme))
                    .collect(),
            ));
        }
        sections
    }

    /// Render the card's rows at `width`, bypassing the component cache.
    pub fn render_at(&mut self, width: u16) -> Vec<String> {
        let header = render_status_line(&self.status, &self.theme);
        let options = OutputBlockOptions {
            header,
            header_meta: String::new(),
            footer_meta: self.footer_meta.clone(),
            state: self.phase.state(),
            sections: self.sections(),
            width: width as usize,
            apply_bg: true,
            content_padding_left: Some(self.padding),
            content_padding_right: Some(self.padding),
            border_color: self.border_color,
            collapsed: self.collapsed,
            collapsed_max_lines: self.collapsed_max_lines,
            expanded_max_lines: None,
        };
        render_output_block(&options, &self.theme)
    }
}

impl Component for ToolCard {
    fn render(&mut self, width: u16) -> Vec<String> {
        if let Some((cached_width, rows)) = &self.cache {
            if *cached_width == width as usize {
                return rows.clone();
            }
        }
        let rows = self.render_at(width);
        self.cache = Some((width as usize, rows.clone()));
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        if matches!(key, Key::Ctrl('o')) {
            self.toggle_collapsed();
        } else if matches!(key, Key::Enter) {
            self.set_collapsed(false);
        }
    }

    fn invalidate(&mut self) {
        self.cache = None;
    }
}

/// Background token for a phase.
pub fn phase_bg(phase: ToolCardPhase) -> ThemeBg {
    match phase.state() {
        Some(state) => state_bg_color(state),
        None => ThemeBg::ToolPendingBg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render_utils::pad_or_truncate;
    use crate::text_shape::{truncate_to_width, Ellipsis};

    fn theme() -> Theme {
        crate::render::render_utils::tests::sample_theme()
    }

    #[test]
    fn status_line_renders_icon_title_and_meta() {
        let line = StatusLine {
            icon: Some(ToolUiStatus::Success),
            title: "Bash".into(),
            description: Some("ls -l".into()),
            meta: vec!["0.16s".into(), "2 files".into()],
            ..StatusLine::default()
        };
        let row = render_status_line(&line, &theme());
        assert!(row.contains("Bash"));
        assert!(row.contains("ls -l"));
        assert!(row.contains("0.16s"));
        assert!(row.contains("2 files"));
        assert!(visible_width(&row) > 0);
    }

    #[test]
    fn status_line_flattens_newlines() {
        let line = StatusLine {
            title: "a\nb".into(),
            description: Some("c\nd".into()),
            ..StatusLine::default()
        };
        let row = render_status_line(&line, &theme());
        assert!(!row.contains('\n'));
        assert!(row.contains("a b"));
        assert!(row.contains("c d"));
    }

    #[test]
    fn diff_lines_use_their_theme_tokens() {
        let theme = theme();
        let added = style_diff_line("+new", &theme);
        let removed = style_diff_line("-old", &theme);
        let context = style_diff_line(" same", &theme);
        assert_ne!(added, "+new");
        assert_ne!(removed, "-old");
        assert_ne!(context, " same");
        assert!(added.starts_with('\x1b'));
        assert!(removed.starts_with('\x1b'));
    }

    #[test]
    fn diff_stats_are_reported() {
        let mut card = ToolCard::new(theme(), StatusLine::new("Edit"));
        card.set_diff("+a\n-b\n context\n+c");
        let stats = card.diff_stats();
        assert_eq!((stats.added, stats.removed, stats.hunks), (2, 1, 2));
    }

    #[test]
    fn card_renders_title_body_and_output() {
        let mut card = ToolCard::new(theme(), StatusLine::new("Bash"));
        card.set_status(ToolCardPhase::Success);
        card.set_body(vec!["echo hi".into()]);
        card.set_output("out");
        let rows = card.render(20);
        assert!(rows[0].contains("Bash"));
        assert!(rows.iter().any(|row| row.contains("echo hi")));
        assert!(rows.iter().any(|row| row.contains("Output")));
        assert!(rows.iter().any(|row| row.contains("out")));
        assert!(rows.iter().all(|row| visible_width(row) <= 20));
    }

    #[test]
    fn collapsed_card_caps_rows() {
        let mut card = ToolCard::new(theme(), StatusLine::new("Bash"));
        card.set_body(
            (0..10)
                .map(|index| format!("line {index}"))
                .collect::<Vec<_>>(),
        );
        card.set_collapsed_max_lines(3);
        card.set_collapsed(true);
        let rows = card.render(30);
        let body_rows = rows.iter().filter(|row| row.contains("line ")).count();
        assert_eq!(body_rows, 3);
        assert!(rows.iter().any(|row| row.contains("more lines")));
    }

    #[test]
    fn append_output_merges_continuation() {
        let mut card = ToolCard::new(theme(), StatusLine::new("Bash"));
        card.append_output("progress 1");
        card.append_output(" progress 2\nnext");
        assert_eq!(card.output, vec!["progress 1 progress 2", "next"]);
    }

    #[test]
    fn keyboard_toggles_collapse() {
        let mut card = ToolCard::new(theme(), StatusLine::new("Bash"));
        assert!(!card.collapsed());
        card.handle_input(&Key::Ctrl('o'));
        assert!(card.collapsed());
        card.handle_input(&Key::Enter);
        assert!(!card.collapsed());
    }

    #[test]
    fn narrow_width_is_safe() {
        let mut card = ToolCard::new(theme(), StatusLine::new("Bash"));
        card.set_body(vec!["a very long body row".into()]);
        let rows = card.render(1);
        assert!(rows.iter().all(|row| visible_width(row) <= 1));
    }

    #[test]
    fn pad_helper_caps_rows() {
        assert_eq!(pad_or_truncate("abcdef", 3, None), "abc");
        assert_eq!(truncate_to_width("abcdef", 3, Ellipsis::Omit), "abc");
    }
}
