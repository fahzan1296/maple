//! Overlay box chrome: the rounded borders, two-column split layout, a pinned
//! row region, and the [`OverlayPanel`] container.
//!
//! Ported from `l/src/chrome/overlay-box.ts`. The TS helpers are free functions
//! over the active theme; here they take an [`OverlayTheme`] so rendering stays
//! deterministic and side-effect free. Glyphs come from
//! [`crate::symbols::SymbolTheme::box_round`]; the tee junctions (`┬ ┴ ├ ┤`)
//! live in the sharp preset in pi-tui's symbol table and are constants here
//! because [`crate::symbols::LineSymbols`] only carries the six rounded box
//! glyphs.

use crate::component::Component;
use crate::keys::Key;
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};

/// Junction glyphs for split overlays (`boxSharp` in pi-tui's symbol table).
const TEE_DOWN: &str = "┬";
const TEE_UP: &str = "┴";
const TEE_LEFT: &str = "┤";
const TEE_RIGHT: &str = "├";

/// Theme for overlay chrome: border/title/rule paints plus the glyph set.
#[derive(Clone, Debug)]
pub struct OverlayTheme {
    /// Border and rule color (the theme's `border` token).
    pub border: Style,
    /// Inset title color (the theme's `accent` token, bold).
    pub title: Style,
    /// Section divider color (the theme's `dim` token).
    pub rule: Style,
    /// Optional background wash for body rows.
    pub surface: Option<Style>,
    /// Box-drawing glyph set.
    pub symbols: SymbolTheme,
}

impl Default for OverlayTheme {
    fn default() -> Self {
        OverlayTheme {
            border: Style::fg(Color::Blue),
            title: Style::fg(Color::Yellow).bold(),
            rule: Style::fg(Color::DarkGray),
            surface: None,
            symbols: SymbolTheme::unicode(),
        }
    }
}

impl OverlayTheme {
    /// Paint `text` with the border style.
    pub fn paint_border(&self, text: &str) -> String {
        self.border.paint(text)
    }

    /// Paint `text` with the divider style.
    pub fn paint_rule(&self, text: &str) -> String {
        self.rule.paint(text)
    }
}

/// Clamp a finished row to `width` visible cells.
fn fit(row: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    truncate_to_width(row, width as usize, Ellipsis::Omit)
}

/// Top border with an optional title inset into the rule.
///
/// `color` recolors border and title together; the title keeps its own accent
/// paint when the caller only overrides the border. An empty title draws a
/// plain rule. Rows are always clamped to `width`.
pub fn top_border(width: u16, title: &str, theme: &OverlayTheme) -> String {
    if width == 0 {
        return String::new();
    }
    let box_glyphs = &theme.symbols.box_round;
    let inner = (width as usize).saturating_sub(2);
    let mut row = if title.is_empty() {
        theme.paint_border(&format!(
            "{}{}",
            box_glyphs.top_left,
            box_glyphs.horizontal.repeat(inner)
        )) + &box_glyphs.top_right.to_string()
    } else {
        let shown = truncate_to_width(&format!(" {title} "), inner.saturating_sub(2), Ellipsis::Omit);
        let fill = inner.saturating_sub(1 + visible_width(&shown));
        let mut row = theme.paint_border(&format!("{}{}", box_glyphs.top_left, box_glyphs.horizontal));
        row.push_str(&theme.title.paint(&shown));
        row.push_str(&theme.paint_border(&format!(
            "{}{}",
            box_glyphs.horizontal.repeat(fill),
            box_glyphs.top_right
        )));
        row
    };
    if visible_width(&row) < width as usize {
        row.push_str(&" ".repeat(width as usize - visible_width(&row)));
    }
    fit(&row, width)
}

/// A horizontal rule with left/right tees, splitting overlay sections.
pub fn divider(width: u16, theme: &OverlayTheme) -> String {
    if width == 0 {
        return String::new();
    }
    let box_glyphs = &theme.symbols.box_round;
    let inner = (width as usize).saturating_sub(2);
    let row = theme.paint_rule(&format!(
        "{}{}{}",
        TEE_RIGHT,
        box_glyphs.horizontal.repeat(inner),
        TEE_LEFT
    ));
    fit(&row, width)
}

/// Bottom border of an overlay.
pub fn bottom_border(width: u16, theme: &OverlayTheme) -> String {
    if width == 0 {
        return String::new();
    }
    let box_glyphs = &theme.symbols.box_round;
    let inner = (width as usize).saturating_sub(2);
    let row = theme.paint_border(&format!(
        "{}{}{}",
        box_glyphs.bottom_left,
        box_glyphs.horizontal.repeat(inner),
        box_glyphs.bottom_right
    ));
    fit(&row, width)
}

/// Wrap pre-styled content in vertical borders with single-column insets.
///
/// The content field is `width - 4` cells; wider content is truncated and
/// narrower content padded, so the row never exceeds `width`.
pub fn row(content: &str, width: u16, theme: &OverlayTheme) -> String {
    if width == 0 {
        return String::new();
    }
    let box_glyphs = &theme.symbols.box_round;
    let bar = theme.paint_border(&box_glyphs.vertical);
    if width <= 4 {
        return fit(&format!("{bar}  {bar}"), width);
    }
    let inner = (width as usize) - 4;
    let fitted = pad_to_width(&truncate_to_width(content, inner, Ellipsis::Omit), inner);
    fit(&format!("{bar} {fitted} {bar}"), width)
}

/// Column index (0-based) of the inner divider for a two-column layout.
pub fn split_divider_col(sidebar_width: usize) -> usize {
    sidebar_width + 3
}

/// Body content width for a two-column overlay of total `width`.
pub fn split_body_width(width: usize, sidebar_width: usize) -> usize {
    width.saturating_sub(sidebar_width).saturating_sub(7)
}

/// Top border carrying the title, split by a `┬` over the column divider.
pub fn top_border_split(width: u16, title: &str, sidebar_width: usize, theme: &OverlayTheme) -> String {
    if width == 0 {
        return String::new();
    }
    let box_glyphs = &theme.symbols.box_round;
    let divider_col = split_divider_col(sidebar_width);
    let left_len = divider_col.saturating_sub(1);
    let right_len = (width as usize).saturating_sub(2).saturating_sub(divider_col);
    let left = if title.is_empty() {
        theme.paint_border(&format!(
            "{}{}",
            box_glyphs.top_left,
            box_glyphs.horizontal.repeat(left_len)
        ))
    } else {
        let shown = truncate_to_width(&format!(" {title} "), left_len.saturating_sub(1), Ellipsis::Omit);
        let fill = left_len.saturating_sub(1 + visible_width(&shown));
        let mut left = theme.paint_border(&format!("{}{}", box_glyphs.top_left, box_glyphs.horizontal));
        left.push_str(&theme.title.paint(&shown));
        left.push_str(&theme.paint_border(&box_glyphs.horizontal.repeat(fill)));
        left
    };
    let row = format!(
        "{left}{}{}{}",
        TEE_DOWN,
        box_glyphs.horizontal.repeat(right_len),
        box_glyphs.top_right
    );
    fit(&row, width)
}

/// Section rule that closes the sidebar column with a `┴` over the divider.
pub fn divider_split(width: u16, sidebar_width: usize, theme: &OverlayTheme) -> String {
    if width == 0 {
        return String::new();
    }
    let box_glyphs = &theme.symbols.box_round;
    let divider_col = split_divider_col(sidebar_width);
    let left_len = divider_col.saturating_sub(1);
    let right_len = (width as usize).saturating_sub(2).saturating_sub(divider_col);
    let row = theme.paint_rule(&format!(
        "{}{}{}{}{}",
        TEE_RIGHT,
        box_glyphs.horizontal.repeat(left_len),
        TEE_UP,
        box_glyphs.horizontal.repeat(right_len),
        TEE_LEFT
    ));
    fit(&row, width)
}

/// A two-column content row: `│ sidebar │ body │`, each inset by one column.
pub fn split_row(sidebar: &str, body: &str, width: u16, sidebar_width: usize, theme: &OverlayTheme) -> String {
    if width == 0 {
        return String::new();
    }
    let box_glyphs = &theme.symbols.box_round;
    let bar = theme.paint_border(&box_glyphs.vertical);
    let body_width = split_body_width(width as usize, sidebar_width);
    let sidebar_cell = if sidebar_width > 0 {
        pad_to_width(&truncate_to_width(sidebar, sidebar_width, Ellipsis::Omit), sidebar_width)
    } else {
        String::new()
    };
    let body_cell = if body_width > 0 {
        pad_to_width(&truncate_to_width(body, body_width, Ellipsis::Omit), body_width)
    } else {
        String::new()
    };
    fit(
        &format!("{bar} {sidebar_cell} {bar} {body_cell} {bar}"),
        width,
    )
}

/// Mutable row region for composing an [`OverlayPanel`].
///
/// Callers provide already-styled logical rows and may pin the region to an
/// exact height; extra rows are clipped and missing rows filled. Rendering is
/// memoized until [`PanelRows::set_lines`], [`PanelRows::set_height`], or
/// [`PanelRows::invalidate`] changes the inputs.
pub struct PanelRows {
    lines: Vec<String>,
    height: Option<usize>,
    memo: Option<(u16, Option<usize>, Vec<String>)>,
}

impl Default for PanelRows {
    fn default() -> Self {
        Self::new()
    }
}

impl PanelRows {
    /// An empty, natural-height region.
    pub fn new() -> Self {
        PanelRows {
            lines: Vec::new(),
            height: None,
            memo: None,
        }
    }

    /// Replace the region rows.
    pub fn set_lines(&mut self, lines: &[String]) {
        if self.lines.len() == lines.len() && self.lines.iter().zip(lines).all(|(a, b)| a == b) {
            return;
        }
        self.lines = lines.to_vec();
        self.memo = None;
    }

    /// Pin the region to exactly `height` rows, or `None` for natural height.
    pub fn set_height(&mut self, height: Option<usize>) {
        if self.height == height {
            return;
        }
        self.height = height;
        self.memo = None;
    }

    /// The configured height, if pinned.
    pub fn height(&self) -> Option<usize> {
        self.height
    }

    /// The configured rows.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

impl Component for PanelRows {
    fn render(&mut self, width: u16) -> Vec<String> {
        if let Some((memo_width, memo_height, rows)) = &self.memo {
            if *memo_width == width && *memo_height == self.height {
                return rows.clone();
            }
        }
        let row_count = self.height.unwrap_or(self.lines.len());
        let inner = width as usize;
        let rows: Vec<String> = (0..row_count)
            .map(|index| {
                if inner == 0 {
                    return String::new();
                }
                let line = self.lines.get(index).map(String::as_str).unwrap_or("");
                truncate_to_width(line, inner, Ellipsis::Omit)
            })
            .collect();
        self.memo = Some((width, self.height, rows.clone()));
        rows
    }

    fn invalidate(&mut self) {
        self.memo = None;
    }
}

/// Sentinel child rendered by [`OverlayPanel`] as a `├───┤` section rule.
#[derive(Clone, Copy, Debug, Default)]
pub struct PanelDivider;

impl Component for PanelDivider {
    fn render(&mut self, _width: u16) -> Vec<String> {
        Vec::new()
    }
}

/// Rounded-box container for inline overlays (selectors, run panels).
///
/// Children render inside `│ … │` rows between a titled top border and a
/// bottom border, so inline overlays share the chrome of fullscreen overlays.
/// Content is inset two columns on each side. `Esc` closes the panel when it is
/// closable; focus trapping consumes `Tab`/`Shift+Tab` so the caret cannot
/// leave the overlay.
pub struct OverlayPanel {
    title: String,
    role: String,
    children: Vec<Box<dyn Component>>,
    dividers: Vec<bool>,
    closable: bool,
    closed: bool,
    trap_focus: bool,
    on_close: Option<Box<dyn FnMut() + 'static>>,
    theme: OverlayTheme,
    memo: Option<PanelMemo>,
}

struct PanelMemo {
    width: u16,
    title: String,
    child_lines: Vec<Vec<String>>,
    rows: Vec<String>,
}

impl Default for OverlayPanel {
    fn default() -> Self {
        Self::new(String::new(), "omp.overlay")
    }
}

impl OverlayPanel {
    /// A panel titled `title` with the given semantic role.
    pub fn new(title: impl Into<String>, role: impl Into<String>) -> Self {
        OverlayPanel {
            title: collapse_title(&title.into()),
            role: role.into(),
            children: Vec::new(),
            dividers: Vec::new(),
            closable: false,
            closed: false,
            trap_focus: true,
            on_close: None,
            theme: OverlayTheme::default(),
            memo: None,
        }
    }

    /// Replace the panel theme.
    pub fn set_theme(&mut self, theme: OverlayTheme) {
        self.theme = theme;
        self.memo = None;
    }

    /// The panel theme.
    pub fn theme(&self) -> &OverlayTheme {
        &self.theme
    }

    /// The semantic role reported to the native host.
    pub fn native_role(&self) -> &str {
        &self.role
    }

    /// The collapsed title shown in the top border.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Set the title; whitespace runs collapse to single spaces.
    pub fn set_title(&mut self, value: impl AsRef<str>) {
        let next = collapse_title(value.as_ref());
        if next == self.title {
            return;
        }
        self.title = next;
        self.memo = None;
    }

    /// Append a child component.
    pub fn add_child<C: Component + 'static>(&mut self, child: C) -> &mut Self {
        self.children.push(Box::new(child));
        self.dividers.push(false);
        self.memo = None;
        self
    }

    /// Append a [`PanelDivider`] section rule.
    pub fn add_divider(&mut self) -> &mut Self {
        self.children.push(Box::new(PanelDivider));
        self.dividers.push(true);
        self.memo = None;
        self
    }

    /// Remove the child at `index`.
    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index >= self.children.len() {
            return None;
        }
        self.dividers.remove(index);
        self.memo = None;
        Some(self.children.remove(index))
    }

    /// Remove every child.
    pub fn clear(&mut self) {
        self.children.clear();
        self.dividers.clear();
        self.memo = None;
    }

    /// Number of children, including dividers.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Make the panel close on `Esc`.
    pub fn set_closable(&mut self, closable: bool) {
        self.closable = closable;
    }

    /// Whether the panel closes on `Esc`.
    pub fn closable(&self) -> bool {
        self.closable
    }

    /// Whether `Esc` already closed the panel.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Consume `Tab`/`Shift+Tab` so focus cannot escape the overlay.
    pub fn set_trap_focus(&mut self, trap: bool) {
        self.trap_focus = trap;
    }

    /// Whether focus trapping is enabled.
    pub fn trap_focus(&self) -> bool {
        self.trap_focus
    }

    /// Register the close callback, invoked once when `Esc` closes the panel.
    pub fn on_close(&mut self, callback: impl FnMut() + 'static) {
        self.on_close = Some(Box::new(callback));
    }

    fn wrapped_rows(&mut self, width: u16, child_lines: &[Vec<String>]) -> Vec<String> {
        let mut rows = vec![top_border(width, &self.title, &self.theme)];
        for (index, child) in self.children.iter().enumerate() {
            if self.dividers.get(index).copied().unwrap_or(false) {
                rows.push(divider(width, &self.theme));
                continue;
            }
            for line in child_lines.get(index).map(Vec::as_slice).unwrap_or(&[]) {
                let mut wrapped = row(line, width, &self.theme);
                if let Some(surface) = self.theme.surface {
                    wrapped = surface.paint(&pad_to_width(&wrapped, width as usize));
                }
                rows.push(wrapped);
            }
        }
        rows.push(bottom_border(width, &self.theme));
        rows
    }

    /// Render only the children's rows, with no border chrome.
    pub fn render_content(&mut self, width: u16) -> Vec<String> {
        let inner = (width as usize).saturating_sub(4).max(1) as u16;
        let mut rows = Vec::new();
        for (index, child) in self.children.iter_mut().enumerate() {
            if self.dividers.get(index).copied().unwrap_or(false) {
                continue;
            }
            rows.extend(child.render(inner));
        }
        rows
    }
}

impl Component for OverlayPanel {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width.max(0);
        let inner = (width as usize).saturating_sub(4).max(1) as u16;
        let mut child_lines: Vec<Vec<String>> = Vec::with_capacity(self.children.len());
        for (index, child) in self.children.iter_mut().enumerate() {
            if self.dividers.get(index).copied().unwrap_or(false) {
                child_lines.push(Vec::new());
            } else {
                child_lines.push(child.render(inner));
            }
        }
        if let Some(memo) = &self.memo {
            if memo.width == width
                && memo.title == self.title
                && memo.child_lines.len() == child_lines.len()
                && memo
                    .child_lines
                    .iter()
                    .zip(child_lines.iter())
                    .all(|(a, b)| a == b)
            {
                return memo.rows.clone();
            }
        }
        let rows = self.wrapped_rows(width, &child_lines);
        self.memo = Some(PanelMemo {
            width,
            title: self.title.clone(),
            child_lines,
            rows: rows.clone(),
        });
        rows
    }

    fn handle_input(&mut self, key: &Key) {
        if self.closed {
            return;
        }
        if self.closable && crate::keys::is_escape(key) {
            self.closed = true;
            self.memo = None;
            if let Some(callback) = self.on_close.as_mut() {
                callback();
            }
            return;
        }
        if self.trap_focus && (crate::keys::is_tab(key) || crate::keys::is_shift_tab(key)) {
            // Consumed: focus stays inside the overlay.
        }
    }

    fn invalidate(&mut self) {
        self.memo = None;
        for child in &mut self.children {
            child.invalidate();
        }
    }

    fn dispose(&mut self) {
        for child in &mut self.children {
            child.dispose();
        }
    }
}

/// Titles inset into a single border row never carry line breaks.
fn collapse_title(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plain_theme() -> OverlayTheme {
        OverlayTheme {
            border: Style::default(),
            title: Style::default(),
            rule: Style::default(),
            surface: None,
            symbols: SymbolTheme::unicode(),
        }
    }

    use crate::components::Text;

    #[test]
    fn top_border_with_title_is_exact() {
        let theme = plain_theme();
        assert_eq!(top_border(20, "Pick", &theme), "╭─ Pick ───────────╮");
        assert_eq!(top_border(20, "", &theme), "╭──────────────────╮");
    }

    #[test]
    fn borders_fit_narrow_widths() {
        let theme = OverlayTheme::default();
        for width in 0..8 {
            for row in [
                top_border(width, "Title", &theme),
                divider(width, &theme),
                bottom_border(width, &theme),
                row("content", width, &theme),
            ] {
                assert!(visible_width(&row) <= width as usize, "width {width}: {row:?}");
            }
        }
    }

    #[test]
    fn split_geometry_matches_reference() {
        assert_eq!(split_divider_col(10), 13);
        assert_eq!(split_body_width(40, 10), 23);
        assert_eq!(split_body_width(10, 10), 0);
        let theme = plain_theme();
        assert_eq!(
            top_border_split(30, "Sessions", 10, &theme),
            "╭─ Sessions ─┬───────────────╮"
        );
        assert_eq!(divider_split(30, 10, &theme), "├────────────┴───────────────┤");
        assert_eq!(split_row("side", "body", 30, 10, &theme), "│ side       │ body          │");
    }

    #[test]
    fn row_insets_and_clamps_content() {
        let theme = plain_theme();
        assert_eq!(row("hello", 12, &theme), "│ hello    │");
        let long = row(&"x".repeat(40), 12, &theme);
        assert_eq!(visible_width(&long), 12);
    }

    #[test]
    fn panel_rows_clips_and_fills_height() {
        let mut rows = PanelRows::new();
        rows.set_lines(&["a".to_string(), "b".to_string()]);
        assert_eq!(rows.render(5), vec!["a", "b"]);
        rows.set_height(Some(4));
        assert_eq!(rows.render(5), vec!["a", "b", "", ""]);
        rows.set_height(Some(1));
        assert_eq!(rows.render(5), vec!["a"]);
    }

    #[test]
    fn panel_rows_truncates_wide_lines() {
        let mut rows = PanelRows::new();
        rows.set_lines(&["abcdef".to_string()]);
        assert_eq!(rows.render(3), vec!["abc"]);
        assert_eq!(rows.render(0), vec![""]);
    }

    #[test]
    fn panel_renders_titled_box() {
        let mut panel = OverlayPanel::new("Pick", "omp.overlay.pick");
        panel.set_theme(plain_theme());
        panel.add_child(Text::new("alpha"));
        panel.add_divider();
        panel.add_child(Text::new("beta"));
        let rows = panel.render(14);
        assert_eq!(
            rows,
            vec![
                "╭─ Pick ─────╮",
                "│ alpha      │",
                "├────────────┤",
                "│ beta       │",
                "╰────────────╯",
            ]
        );
        assert!(rows.iter().all(|row| visible_width(row) <= 14));
    }

    #[test]
    fn panel_render_content_excludes_chrome() {
        let mut panel = OverlayPanel::new("T", "omp.overlay");
        panel.add_child(Text::new("body"));
        assert_eq!(panel.render_content(20), vec!["body            "]);
    }

    #[test]
    fn escape_closes_and_fires_callback() {
        let mut panel = OverlayPanel::new("T", "omp.overlay");
        panel.set_closable(true);
        let hits = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = hits.clone();
        panel.on_close(move || counter.set(counter.get() + 1));
        panel.handle_input(&Key::Esc);
        assert!(panel.is_closed());
        assert_eq!(hits.get(), 1);
        panel.handle_input(&Key::Esc);
        assert_eq!(hits.get(), 1);
    }

    #[test]
    fn focus_trapping_consumes_tabs_only() {
        let mut panel = OverlayPanel::new("T", "omp.overlay");
        panel.handle_input(&Key::Tab);
        panel.handle_input(&Key::BackTab);
        assert!(!panel.is_closed());
        panel.set_closable(false);
        panel.handle_input(&Key::Esc);
        assert!(!panel.is_closed());
    }

    #[test]
    fn title_collapses_whitespace() {
        let mut panel = OverlayPanel::new("  a\n\t b  ", "omp.overlay");
        assert_eq!(panel.title(), "a b");
    }
}
