//! Tool call card: a titled, bordered frame whose body carries status, diff,
//! Reference: `l/src/render/tool-card.ts`.
//! and output sections.
//!
//! [`render_status_line`] reproduces the shared status header primitive (the
//! `render/status-line.ts` module is not part of this crate yet, so the
//! primitive lives here until it lands). [`ToolCard`] composes its frame from
//! [`crate::render::output_block`].
//!
//! The lazy card surface ([`LazyToolCard`], [`framed_tool_card`],
//! [`plain_tool_card`], [`tool_card_state`]) reproduces
//! `l/src/render/tool-card.ts`.

use crate::component::Component;
use crate::keys::Key;
use crate::render::output_block::{
    mark_framed_block_component, output_block_content_width, render_output_block,
    OutputBlockOptions, OutputSection,
};
use crate::render::render_utils::{
    apply_background_ansi, bold_text, format_status_icon, get_diff_stats, get_padding_x,
    padding, paint, replace_tabs, split_terminal_output_lines, stabilize_background_ansi,
    state_bg_color, style_tool_output_line, wrap_brackets, DiffStats, Hasher, State,
    ToolUiStatus, ToolUIColor,
};
use crate::render::status_line::StatusLineOptions;
use crate::text_shape::{visible_width, wrap_text_with_ansi};
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

/// Fixed card topology. Framed cards own their border; plain cards flow inline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolCardVariant {
    #[default]
    Framed,
    Plain,
}

/// Lazily rendered slot content. Components retain their own width caches.
///
/// Reference: `l/src/render/tool-card.ts::ToolCardContent`.
pub enum ToolCardContent {
    /// Pre-rendered rows.
    Lines(Vec<String>),
    /// A child component, rendered to rows at the slot width.
    Component(Box<dyn Component>),
    /// A width-dependent builder.
    Lazy(Box<dyn Fn(usize) -> ToolCardContent>),
}

impl ToolCardContent {
    /// Pre-rendered rows.
    pub fn lines(lines: Vec<String>) -> Self {
        ToolCardContent::Lines(lines)
    }

    /// A child component.
    pub fn component(component: Box<dyn Component>) -> Self {
        ToolCardContent::Component(component)
    }

    /// A width-dependent builder.
    pub fn lazy<F>(build: F) -> Self
    where
        F: Fn(usize) -> ToolCardContent + 'static,
    {
        ToolCardContent::Lazy(Box::new(build))
    }
}

impl From<Vec<String>> for ToolCardContent {
    fn from(lines: Vec<String>) -> Self {
        ToolCardContent::Lines(lines)
    }
}

impl From<Box<dyn Component>> for ToolCardContent {
    fn from(component: Box<dyn Component>) -> Self {
        ToolCardContent::Component(component)
    }
}

/// A labeled body section in a tool card.
///
/// Reference: `l/src/render/tool-card.ts::ToolCardSection`.
pub struct ToolCardSection {
    /// Section label drawn behind a `├── … ──┤` separator.
    pub label: Option<String>,
    /// Section body.
    pub content: ToolCardContent,
    /// Draw a divider before this section.
    pub separator: Option<bool>,
}

impl ToolCardSection {
    /// An unlabelled section.
    pub fn new(content: ToolCardContent) -> Self {
        ToolCardSection {
            label: None,
            content,
            separator: None,
        }
    }

    /// A labelled section.
    pub fn labeled(label: impl Into<String>, content: ToolCardContent) -> Self {
        ToolCardSection {
            label: Some(label.into()),
            content,
            separator: None,
        }
    }

    /// Set whether a divider precedes this section.
    pub fn with_separator(mut self, separator: bool) -> Self {
        self.separator = Some(separator);
        self
    }
}

/// Snapshot returned by a tool card builder on each render.
///
/// Reference: `l/src/render/tool-card.ts::ToolCardSnapshot`.
#[derive(Default)]
pub struct ToolCardSnapshot {
    /// Pre-rendered header. Prefer `status` for ordinary tool lifecycle rows.
    pub header: Option<String>,
    /// Structured status header, rendered by the shared status-line primitive.
    pub status: Option<StatusLineOptions>,
    /// Header meta drawn after the header.
    pub header_meta: Option<String>,
    /// Short facts drawn inside the frame's bottom border (framed); a trailing
    /// row otherwise.
    pub footer_meta: Option<String>,
    pub phase: Option<ToolCardPhase>,
    pub body: Option<ToolCardContent>,
    pub sections: Vec<ToolCardSection>,
    pub footer: Option<ToolCardContent>,
    pub apply_bg: Option<bool>,
    pub content_padding_left: Option<usize>,
    pub content_padding_right: Option<usize>,
    pub border_color: Option<ThemeColor>,
}

/// Width information supplied to a tool card's lazy snapshot builder.
///
/// Reference: `l/src/render/tool-card.ts::ToolCardBuildContext`.
pub struct ToolCardBuildContext {
    /// Surface width the card is rendered at.
    pub width: usize,
    /// Inner width for the default one-column framed padding.
    pub content_width: usize,
    framed: bool,
}

impl ToolCardBuildContext {
    /// Compute an inner width when this snapshot opts into custom frame padding.
    pub fn content_width_for(
        &self,
        content_padding_left: Option<usize>,
        content_padding_right: Option<usize>,
    ) -> usize {
        if self.framed {
            output_block_content_width(self.width, content_padding_left, content_padding_right)
        } else {
            self.content_width
        }
    }
}

/// Construction options whose topology remains fixed for a card's lifetime.
///
/// Reference: `l/src/render/tool-card.ts::ToolCardOptions`.
#[derive(Default)]
pub struct ToolCardOptions {
    /// Fixed card topology.
    pub variant: ToolCardVariant,
    /// Horizontal padding for plain cards.
    pub padding_x: Option<usize>,
    /// Vertical padding for plain cards.
    pub padding_y: Option<usize>,
    /// Whether tight layout drops a column of plain-card padding.
    pub ignore_tight: Option<bool>,
    /// Called by [`LazyToolCard::invalidate`].
    pub on_invalidate: Option<Box<dyn Fn()>>,
    /// Called by [`LazyToolCard::release_render_caches`].
    pub on_release_render_caches: Option<Box<dyn Fn()>>,
    /// Called by [`LazyToolCard::dispose`].
    pub on_dispose: Option<Box<dyn Fn()>>,
}

/// Map transcript lifecycle terminology onto the output frame's visual states.
///
/// Reference: `l/src/render/tool-card.ts::toolCardState`.
pub fn tool_card_state(phase: Option<ToolCardPhase>) -> Option<State> {
    match phase {
        Some(ToolCardPhase::Partial) => Some(State::Pending),
        Some(ToolCardPhase::Info) | None => None,
        Some(other) => other.state(),
    }
}

struct ResolvedSection {
    label: Option<String>,
    lines: Vec<String>,
    separator: bool,
}

fn component_address(component: &dyn Component) -> usize {
    component as *const dyn Component as *const () as usize
}

fn state_key(state: Option<State>) -> &'static str {
    match state {
        Some(State::Pending) => "pending",
        Some(State::Running) => "running",
        Some(State::Success) => "success",
        Some(State::Error) => "error",
        Some(State::Warning) => "warning",
        None => "info",
    }
}

/// Resolve one slot's content to rows plus the component it came from, if any.
fn resolve_content(
    content: ToolCardContent,
    width: usize,
) -> (Vec<String>, Option<Box<dyn Component>>) {
    match content {
        ToolCardContent::Lines(lines) => (lines, None),
        ToolCardContent::Component(mut component) => {
            let lines = component.render(width as u16);
            (lines, Some(component))
        }
        ToolCardContent::Lazy(build) => resolve_content(build(width), width),
    }
}

/// Pad `line` to `width` and tint it with a raw SGR background, re-opening the
/// background after nested resets (the plain-card counterpart of
/// `applyBackgroundToLine`).
fn apply_background_to_line(line: &str, width: usize, bg_ansi: &str) -> String {
    let visible = visible_width(line);
    let padded = format!("{line}{}", padding(width.saturating_sub(visible)));
    if line.contains("\x1b[49m") {
        stabilize_background_ansi(&padded, bg_ansi)
    } else {
        apply_background_ansi(&padded, bg_ansi)
    }
}

/// A lazily built tool card.
///
/// Reference: `l/src/render/tool-card.ts::ToolCard`. The reference's lazy card
/// class is named [`LazyToolCard`] here because [`ToolCard`] is the crate's
/// stateful card. The builder runs on every render, so it may read mutable
/// streaming state; the card retains child components across renders and
/// disposes the ones a later snapshot drops.
///
/// Deviations: the crate's [`Component`] has no native `describe`, so the
/// reference's `describe`/`debugChildren`/`wantsKeyRelease` surface has no port;
/// the plain-card memo hashes with [`Hasher`] instead of `Bun.hash`; and
/// [`LazyToolCard::new`] does not brand itself (the address is only stable once
/// boxed), so framed cards come from [`framed_tool_card`].
pub struct LazyToolCard {
    theme: Theme,
    options: ToolCardOptions,
    build: Box<dyn Fn(ToolCardBuildContext) -> ToolCardSnapshot>,
    rendered_children: Vec<Box<dyn Component>>,
    disposed: bool,
    plain_cache: Option<(String, Vec<String>)>,
}

impl LazyToolCard {
    /// Create a card bound to `theme`; `options.variant` fixes the topology.
    pub fn new<F>(theme: Theme, options: ToolCardOptions, build: F) -> Self
    where
        F: Fn(ToolCardBuildContext) -> ToolCardSnapshot + 'static,
    {
        LazyToolCard {
            theme,
            options,
            build: Box::new(build),
            rendered_children: Vec::new(),
            disposed: false,
            plain_cache: None,
        }
    }

    /// The card's theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The card's fixed options.
    pub fn options(&self) -> &ToolCardOptions {
        &self.options
    }

    /// Child components retained by the last render.
    pub fn rendered_children(&self) -> &[Box<dyn Component>] {
        &self.rendered_children
    }

    /// Set whether tight layout drops a column of plain-card padding.
    ///
    /// Deviation: the reference also forwards this to retained children; the
    /// crate's [`Component`] has no `set_ignore_tight`.
    pub fn set_ignore_tight(&mut self, ignore: bool) {
        self.options.ignore_tight = Some(ignore);
        self.invalidate();
    }

    /// Drop derived caches; the next render rebuilds from state.
    pub fn invalidate(&mut self) {
        self.plain_cache = None;
        for child in &mut self.rendered_children {
            child.invalidate();
        }
        if let Some(callback) = &self.options.on_invalidate {
            callback();
        }
    }

    /// Release owned resources without disposing the card.
    ///
    /// Deviation: the crate's [`Component`] has no `release_render_caches`
    /// method, so retained children cannot be asked to release theirs.
    pub fn release_render_caches(&mut self) {
        self.plain_cache = None;
        if let Some(callback) = &self.options.on_release_render_caches {
            callback();
        }
    }

    /// Dispose the card and its retained children.
    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        for child in &mut self.rendered_children {
            child.dispose();
        }
        self.rendered_children.clear();
        if let Some(callback) = &self.options.on_dispose {
            callback();
        }
    }

    fn adopt_child(component: Option<Box<dyn Component>>, next: &mut Vec<Box<dyn Component>>) {
        let Some(component) = component else {
            return;
        };
        let address = component_address(&*component);
        if !next.iter().any(|child| component_address(&**child) == address) {
            next.push(component);
        }
    }

    fn render_plain_rows(
        &self,
        text: &str,
        width: usize,
        padding_x: usize,
        state: Option<State>,
        apply_bg: bool,
    ) -> Vec<String> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        let content_width = width.saturating_sub(padding_x * 2).max(1);
        let normalized = replace_tabs(text);
        let wrapped = wrap_text_with_ansi(&normalized, content_width);
        let left_margin = padding(padding_x);
        let right_margin = padding(padding_x);
        let bg_ansi = match (state, apply_bg) {
            (Some(state), true) => self
                .theme
                .get_bg_ansi(state_bg_color(state))
                .ok()
                .filter(|ansi| !ansi.is_empty()),
            _ => None,
        };
        let mut content_lines: Vec<String> = Vec::with_capacity(wrapped.len());
        for line in wrapped {
            let line_with_margins = format!("{left_margin}{line}{right_margin}");
            match &bg_ansi {
                Some(ansi) => {
                    content_lines.push(apply_background_to_line(&line_with_margins, width, ansi));
                }
                None => {
                    let visible = visible_width(&line_with_margins);
                    content_lines.push(format!(
                        "{line_with_margins}{}",
                        padding(width.saturating_sub(visible))
                    ));
                }
            }
        }
        let padding_y = self.options.padding_y.unwrap_or(0);
        let empty_line = padding(width);
        let mut result: Vec<String> = Vec::with_capacity(padding_y * 2 + content_lines.len());
        for _ in 0..padding_y {
            match &bg_ansi {
                Some(ansi) => result.push(apply_background_to_line(&empty_line, width, ansi)),
                None => result.push(empty_line.clone()),
            }
        }
        result.extend(content_lines);
        for _ in 0..padding_y {
            match &bg_ansi {
                Some(ansi) => result.push(apply_background_to_line(&empty_line, width, ansi)),
                None => result.push(empty_line.clone()),
            }
        }
        if result.is_empty() {
            vec![String::new()]
        } else {
            result
        }
    }
}

impl Component for LazyToolCard {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        let configured_padding = self.options.padding_x.unwrap_or(0);
        let plain_padding = if self.options.ignore_tight == Some(false) {
            get_padding_x(configured_padding)
        } else {
            configured_padding
        };
        let framed = self.options.variant == ToolCardVariant::Framed;
        let default_content_width = if framed {
            output_block_content_width(width, None, None)
        } else {
            width.saturating_sub(plain_padding * 2).max(1)
        };
        let context = ToolCardBuildContext {
            width,
            content_width: default_content_width,
            framed,
        };
        let mut snapshot = (self.build)(context);
        let content_width = if framed {
            output_block_content_width(
                width,
                snapshot.content_padding_left,
                snapshot.content_padding_right,
            )
        } else {
            default_content_width
        };

        let mut sections: Vec<ResolvedSection> = Vec::new();
        let mut next_children: Vec<Box<dyn Component>> = Vec::new();
        if let Some(body) = snapshot.body.take() {
            let (lines, component) = resolve_content(body, content_width);
            sections.push(ResolvedSection {
                label: None,
                lines,
                separator: false,
            });
            Self::adopt_child(component, &mut next_children);
        }
        for section in snapshot.sections.drain(..) {
            let (lines, component) = resolve_content(section.content, content_width);
            sections.push(ResolvedSection {
                label: section.label,
                lines,
                separator: section.separator.unwrap_or(false),
            });
            Self::adopt_child(component, &mut next_children);
        }
        if let Some(footer) = snapshot.footer.take() {
            let (lines, component) = resolve_content(footer, content_width);
            sections.push(ResolvedSection {
                label: None,
                lines,
                separator: !sections.is_empty(),
            });
            Self::adopt_child(component, &mut next_children);
        }
        let previous = std::mem::take(&mut self.rendered_children);
        for mut child in previous {
            let address = component_address(&*child);
            if !next_children
                .iter()
                .any(|kept| component_address(&**kept) == address)
            {
                child.dispose();
            }
        }
        self.rendered_children = next_children;

        let state = tool_card_state(snapshot.phase);
        let header = match snapshot.header.take() {
            Some(header) => Some(header),
            None => snapshot
                .status
                .take()
                .map(|status| crate::render::status_line::render_status_line(&status, &self.theme)),
        };

        if framed {
            let options = OutputBlockOptions {
                header: header.unwrap_or_default(),
                header_meta: snapshot.header_meta.take().unwrap_or_default(),
                footer_meta: snapshot.footer_meta.take().unwrap_or_default(),
                state,
                sections: sections
                    .into_iter()
                    .map(|section| OutputSection {
                        label: section.label,
                        lines: section.lines,
                        separator: section.separator,
                    })
                    .collect(),
                width,
                apply_bg: snapshot.apply_bg.unwrap_or(true),
                content_padding_left: snapshot.content_padding_left,
                content_padding_right: snapshot.content_padding_right,
                border_color: snapshot.border_color,
                collapsed: false,
                collapsed_max_lines: 3,
                expanded_max_lines: None,
            };
            return render_output_block(&options, &self.theme);
        }

        let mut lines: Vec<String> = Vec::new();
        if let Some(header) = header {
            match snapshot.header_meta.take() {
                Some(meta) if !meta.is_empty() => lines.push(format!("{header} {meta}")),
                _ => lines.push(header),
            }
        }
        for section in &sections {
            if section.separator && !lines.is_empty() {
                lines.push(String::new());
            }
            if let Some(label) = &section.label {
                lines.push(label.clone());
            }
            lines.extend(section.lines.iter().cloned());
        }
        if let Some(meta) = snapshot.footer_meta.take() {
            if !meta.is_empty() {
                lines.push(meta);
            }
        }
        let text = lines.join("\n");
        let apply_bg = snapshot.apply_bg.unwrap_or(true);
        let mut hasher = Hasher::new();
        hasher.update(&text);
        let key = format!(
            "{width}:{}:{}:{}:{apply_bg}",
            text.len(),
            hasher.digest_u32(),
            state_key(state)
        );
        if let Some((cached_key, rows)) = &self.plain_cache {
            if cached_key == &key {
                return rows.clone();
            }
        }
        let rows = self.render_plain_rows(&text, width, plain_padding, state, apply_bg);
        self.plain_cache = Some((key, rows.clone()));
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        for child in &mut self.rendered_children {
            child.handle_input(key);
        }
    }

    fn invalidate(&mut self) {
        LazyToolCard::invalidate(self);
    }

    fn dispose(&mut self) {
        LazyToolCard::dispose(self);
    }
}

/// Construct a lazy framed tool card.
///
/// Reference: `l/src/render/tool-card.ts::framedToolCard`. The reference takes
/// `Omit<ToolCardOptions, "variant">`; Rust has no `Omit`, so the whole options
/// struct is accepted and `variant` is forced to [`ToolCardVariant::Framed`].
/// The reference brands framed cards with `markFramedBlockComponent`; the port
/// marks the returned box, so
/// [`crate::render::output_block::is_framed_block_component`] reports `true`.
pub fn framed_tool_card<F>(
    theme: Theme,
    build: F,
    options: ToolCardOptions,
) -> Box<dyn Component>
where
    F: Fn(ToolCardBuildContext) -> ToolCardSnapshot + 'static,
{
    let mut options = options;
    options.variant = ToolCardVariant::Framed;
    mark_framed_block_component(Box::new(LazyToolCard::new(theme, options, build)))
}

/// Construct a lazy inline tool card, optionally state-tinted.
///
/// Reference: `l/src/render/tool-card.ts::plainToolCard`. As with
/// [`framed_tool_card`], `variant` is forced to [`ToolCardVariant::Plain`] and
/// the card is returned as `Box<dyn Component>`.
pub fn plain_tool_card<F>(
    theme: Theme,
    build: F,
    options: ToolCardOptions,
) -> Box<dyn Component>
where
    F: Fn(ToolCardBuildContext) -> ToolCardSnapshot + 'static,
{
    let mut options = options;
    options.variant = ToolCardVariant::Plain;
    Box::new(LazyToolCard::new(theme, options, build))
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

    #[test]
    fn tool_card_state_maps_phases() {
        assert_eq!(tool_card_state(None), None);
        assert_eq!(tool_card_state(Some(ToolCardPhase::Info)), None);
        assert_eq!(tool_card_state(Some(ToolCardPhase::Partial)), Some(State::Pending));
        assert_eq!(tool_card_state(Some(ToolCardPhase::Pending)), Some(State::Pending));
        assert_eq!(tool_card_state(Some(ToolCardPhase::Running)), Some(State::Running));
        assert_eq!(tool_card_state(Some(ToolCardPhase::Success)), Some(State::Success));
        assert_eq!(tool_card_state(Some(ToolCardPhase::Warning)), Some(State::Warning));
        assert_eq!(tool_card_state(Some(ToolCardPhase::Error)), Some(State::Error));
    }

    #[test]
    fn build_context_reports_frame_content_width() {
        let framed = ToolCardBuildContext {
            width: 30,
            content_width: 20,
            framed: true,
        };
        assert_eq!(framed.content_width_for(None, None), 26);
        assert_eq!(framed.content_width_for(Some(2), Some(3)), 23);
        let plain = ToolCardBuildContext {
            width: 30,
            content_width: 20,
            framed: false,
        };
        assert_eq!(plain.content_width_for(Some(2), Some(3)), 20);
    }

    #[test]
    fn framed_card_renders_header_body_sections_and_footer() {
        let mut card = framed_tool_card(
            theme(),
            |context| ToolCardSnapshot {
                status: Some(StatusLineOptions {
                    title: "Bash".into(),
                    ..StatusLineOptions::default()
                }),
                header_meta: Some("meta".into()),
                phase: Some(ToolCardPhase::Success),
                body: Some(ToolCardContent::lines(vec![format!(
                    "body {}",
                    context.content_width
                )])),
                sections: vec![ToolCardSection::labeled(
                    "Diff",
                    ToolCardContent::lines(vec!["+added".into()]),
                )],
                footer: Some(ToolCardContent::lines(vec!["footer".into()])),
                footer_meta: Some("Wall 0.1s".into()),
                ..ToolCardSnapshot::default()
            },
            ToolCardOptions::default(),
        );
        let rows = card.render(40);
        let text = rows.join("\n");
        assert!(text.contains("Bash"));
        assert!(text.contains("meta"));
        assert!(text.contains("body"));
        assert!(text.contains("Diff"));
        assert!(text.contains("+added"));
        assert!(text.contains("footer"));
        assert!(text.contains("Wall 0.1s"));
        assert!(rows.iter().all(|row| visible_width(row) <= 40));
        assert!(crate::render::output_block::is_framed_block_component(&*card));
    }

    #[test]
    fn lazy_content_resolves_at_the_render_width() {
        let mut card = LazyToolCard::new(
            theme(),
            ToolCardOptions::default(),
            |context| ToolCardSnapshot {
                body: Some(ToolCardContent::lazy(move |width| {
                    ToolCardContent::lines(vec![format!("w{width}")])
                })),
                ..ToolCardSnapshot::default()
            },
        );
        let rows = card.render(30);
        assert!(rows.iter().any(|row| row.contains("w26")));
    }

    #[test]
    fn plain_card_renders_inline_header_meta_and_separator() {
        let mut card = LazyToolCard::new(
            theme(),
            ToolCardOptions {
                variant: ToolCardVariant::Plain,
                ..ToolCardOptions::default()
            },
            |_context| ToolCardSnapshot {
                header: Some("Title".into()),
                header_meta: Some("meta".into()),
                body: Some(ToolCardContent::lines(vec!["first".into()])),
                sections: vec![ToolCardSection::labeled(
                    "More",
                    ToolCardContent::lines(vec!["second".into()]),
                )
                .with_separator(true)],
                footer_meta: Some("facts".into()),
                ..ToolCardSnapshot::default()
            },
        );
        let rows = card.render(30);
        let text = rows.join("\n");
        assert!(text.contains("Title meta"));
        assert!(text.contains("first"));
        assert!(text.contains("More"));
        assert!(text.contains("second"));
        assert!(text.contains("facts"));
        assert!(rows.iter().all(|row| visible_width(row) <= 30));
    }

    #[test]
    fn invalidate_and_dispose_run_their_callbacks() {
        use std::cell::Cell;
        use std::rc::Rc;

        let calls = Rc::new(Cell::new(0));
        let invalidated = Rc::clone(&calls);
        let released = Rc::clone(&calls);
        let disposed = Rc::clone(&calls);
        let mut options = ToolCardOptions::default();
        options.on_invalidate = Some(Box::new(move || {
            invalidated.set(invalidated.get() + 1);
        }));
        options.on_release_render_caches = Some(Box::new(move || {
            released.set(released.get() + 1);
        }));
        options.on_dispose = Some(Box::new(move || {
            disposed.set(disposed.get() + 1);
        }));
        let mut card = LazyToolCard::new(theme(), options, |_context| ToolCardSnapshot::default());
        card.invalidate();
        card.release_render_caches();
        card.dispose();
        card.dispose();
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn snapshot_defaults_are_empty() {
        let snapshot = ToolCardSnapshot::default();
        assert!(snapshot.header.is_none());
        assert!(snapshot.status.is_none());
        assert!(snapshot.body.is_none());
        assert!(snapshot.sections.is_empty());
        assert_eq!(snapshot.apply_bg, None);
    }
}
