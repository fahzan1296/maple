//! Bordered output container with a header, labelled sections, and an
//! Reference: `l/src/render/output-block.ts`.
//! optional collapsed state.
//!
//! [`render_output_block`] reproduces the TypeScript framing: rounded border,
//! a header bar with optional meta, `├── label ──┤` separators, body rows
//! wrapped to the inner width, and footer facts that ride the bottom border
//! when they fit whole. [`OutputBlock`] wraps the same layout in the
//! [`Component`] contract with setters and a keyboard toggle.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use crate::component::Component;
use crate::keys::Key;
use crate::render::render_utils::{
    format_expand_hint, get_sixel_line_mask, pad_or_truncate_ansi, paint,
    split_terminal_output_lines, State,
};
use crate::text_shape::{truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};
use crate::theme::{Theme, ThemeColor};

/// One body section of an output block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputSection {
    /// Titled sub-section drawn behind a `├── … ──┤` separator.
    pub label: Option<String>,
    /// Body rows; embedded newlines are split.
    pub lines: Vec<String>,
    /// Draw a plain divider before this section.
    pub separator: bool,
}

impl OutputSection {
    /// An unlabelled section.
    pub fn new(lines: Vec<String>) -> Self {
        OutputSection {
            lines,
            ..OutputSection::default()
        }
    }

    /// A labelled section.
    pub fn labeled(label: impl Into<String>, lines: Vec<String>) -> Self {
        OutputSection {
            label: Some(label.into()),
            lines,
            separator: false,
        }
    }

    /// Mark this section as divided from the previous one.
    pub fn with_separator(mut self, separator: bool) -> Self {
        self.separator = separator;
        self
    }
}

/// Inputs to [`render_output_block`].
#[derive(Clone, Debug)]
pub struct OutputBlockOptions {
    pub header: String,
    pub header_meta: String,
    /// Facts drawn inside the bottom border.
    pub footer_meta: String,
    pub state: Option<State>,
    pub sections: Vec<OutputSection>,
    pub width: usize,
    pub apply_bg: bool,
    /// Left content padding; `None` means the one-column default.
    pub content_padding_left: Option<usize>,
    /// Right content padding; `None` follows the left padding.
    pub content_padding_right: Option<usize>,
    /// Override the state-derived border color.
    pub border_color: Option<ThemeColor>,
    /// Collapse the body to `collapsed_max_lines`.
    pub collapsed: bool,
    pub collapsed_max_lines: usize,
    /// Optional cap while expanded.
    pub expanded_max_lines: Option<usize>,
}

impl Default for OutputBlockOptions {
    fn default() -> Self {
        OutputBlockOptions {
            header: String::new(),
            header_meta: String::new(),
            footer_meta: String::new(),
            state: None,
            sections: Vec::new(),
            width: 0,
            apply_bg: true,
            content_padding_left: Some(1),
            content_padding_right: Some(1),
            border_color: None,
            collapsed: false,
            collapsed_max_lines: 3,
            expanded_max_lines: None,
        }
    }
}

/// Content padding normalized the way the TypeScript port does: unset becomes
/// one column.
fn normalize_padding(value: Option<usize>) -> usize {
    value.unwrap_or(1)
}

/// The border token for a state, unless overridden.
pub fn resolve_border_color(state: Option<State>, border_color: Option<ThemeColor>) -> ThemeColor {
    border_color.unwrap_or_else(|| match state {
        Some(State::Error) => ThemeColor::Error,
        Some(State::Warning) => ThemeColor::Warning,
        Some(State::Running) | Some(State::Pending) => ThemeColor::Accent,
        _ => ThemeColor::Dim,
    })
}

/// Inner content width: both borders plus the symmetric content padding.
pub fn output_block_content_width(
    width: usize,
    content_padding_left: Option<usize>,
    content_padding_right: Option<usize>,
) -> usize {
    let left = normalize_padding(content_padding_left);
    let right = normalize_padding(content_padding_right.or(content_padding_left));
    width.saturating_sub(2).saturating_sub(left + right).max(1)
}

// ── Framed block brand ───────────────────────────────────────────────────────

/// Components that own their output frame, keyed by data address.
///
/// Reference: `l/src/render/output-block.ts` brands a component object with the
/// `framedBlockComponent` `Symbol` and later tests any type-erased `Component`.
/// Rust has no per-object property bag or trait upcasting, so the brand is an
/// identity set: [`mark_framed_block_component`] records the component's data
/// address and [`is_framed_block_component`] tests it. Mark the instance that is
/// actually stored (typically a `Box<dyn Component>`); moving the value changes
/// its address and drops the brand.
static FRAMED_BLOCK_COMPONENTS: LazyLock<Mutex<HashSet<usize>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn component_address(component: &dyn Component) -> usize {
    component as *const dyn Component as *const () as usize
}

/// Mark a component as owning its output frame.
///
/// Reference: `l/src/render/output-block.ts::markFramedBlockComponent`. The
/// reference brands the object and returns it; the port records the stored
/// instance's address and hands the same `Box` back, so the brand survives moving
/// the box (its heap allocation does not move).
pub fn mark_framed_block_component(component: Box<dyn Component>) -> Box<dyn Component> {
    let address = component_address(&*component);
    if let Ok(mut marked) = FRAMED_BLOCK_COMPONENTS.lock() {
        marked.insert(address);
    }
    component
}

/// Return whether a component owns its output frame.
///
/// Reference: `l/src/render/output-block.ts::isFramedBlockComponent`.
pub fn is_framed_block_component(component: &dyn Component) -> bool {
    let address = component_address(component);
    FRAMED_BLOCK_COMPONENTS
        .lock()
        .map(|marked| marked.contains(&address))
        .unwrap_or(false)
}

/// Component wrapper caching the last rendered frame.
pub struct OutputBlock {
    theme: Theme,
    options: OutputBlockOptions,
    cache: Option<(u16, Vec<String>)>,
}

/// Render a bordered output block.
pub fn render_output_block(options: &OutputBlockOptions, theme: &Theme) -> Vec<String> {
    let line_width = options.width;
    let horizontal = theme.symbol("boxRound.horizontal");
    let vertical = theme.symbol("boxRound.vertical");
    let cap = horizontal.repeat(3);
    let border_color = resolve_border_color(options.state, options.border_color);
    let border = |text: &str| paint(theme, border_color, text);

    let bg_ansi = match (options.state, options.apply_bg) {
        (Some(state), true) => theme
            .get_bg_ansi(state.bg_color())
            .ok()
            .filter(|ansi| !ansi.is_empty()),

        _ => None,
    };


    let padding_left = normalize_padding(options.content_padding_left);
    let padding_right =
        normalize_padding(options.content_padding_right.or(options.content_padding_left));
    let content_width = line_width
        .saturating_sub(visible_width(vertical))
        .saturating_sub(padding_left)
        .saturating_sub(padding_right)
        .saturating_sub(visible_width(vertical));
    let left_padding = " ".repeat(padding_left);
    let right_padding = " ".repeat(padding_right);

    // ── Layout pass: collect row descriptors before emitting bordered lines. ──
    enum Row {
        Bar {
            left: String,
            right: String,
            label: Option<String>,
            meta: Option<String>,
        },
        Content {
            inner: String,
        },
        Raw {
            raw: String,
        },
    }

    let mut rows: Vec<Row> = Vec::new();
    rows.push(Row::Bar {
        left: theme.symbol("boxRound.topLeft").to_string(),
        right: theme.symbol("boxRound.topRight").to_string(),
        label: option_str(&options.header),
        meta: option_str(&options.header_meta),
    });

    let mut sections: Vec<OutputSection> = if options.sections.is_empty() {
        vec![OutputSection::default()]
    } else {
        options.sections.clone()
    };

    // A collapsed (or capped) body keeps the first rows and marks the rest.
    let limit = if options.collapsed {
        Some(options.collapsed_max_lines)
    } else {
        options.expanded_max_lines
    };
    let total_lines: usize = sections
        .iter()
        .map(|section| {
            section
                .lines
                .iter()
                .map(|line| line.split('\n').count())
                .sum::<usize>()
        })
        .sum();
    if let Some(limit) = limit {
        if total_lines > limit {
            let mut budget = limit;
            let mut hidden = 0usize;
            for section in &mut sections {
                let mut kept: Vec<String> = Vec::new();
                for line in &section.lines {
                    for part in line.split('\n') {
                        if budget > 0 {
                            kept.push(part.to_string());
                            budget -= 1;
                        } else {
                            hidden += 1;
                        }
                    }
                }
                section.lines = kept;
            }
            let marker = capped_marker(hidden, options.collapsed, theme);
            sections.push(OutputSection::new(vec![marker]));
        }
    }

    let sixel_mask_enabled = crate::render::render_utils::is_sixel_protocol();
    for (section_index, section) in sections.iter().enumerate() {
        if let Some(label) = &section.label {
            rows.push(Row::Bar {
                left: theme.symbol("boxSharp.teeRight").to_string(),
                right: theme.symbol("boxSharp.teeLeft").to_string(),
                label: Some(label.clone()),
                meta: None,
            });
        } else if section.separator && section_index > 0 {
            rows.push(Row::Bar {
                left: theme.symbol("boxSharp.teeRight").to_string(),
                right: theme.symbol("boxSharp.teeLeft").to_string(),
                label: None,
                meta: None,
            });
        }
        let all_lines: Vec<String> = section
            .lines
            .iter()
            .flat_map(|line| line.split('\n').map(str::to_string))
            .collect();
        let mask = if sixel_mask_enabled {
            get_sixel_line_mask(&all_lines)
        } else {
            Vec::new()
        };
        for (index, line) in all_lines.iter().enumerate() {
            if mask.get(index).copied().unwrap_or(false) {
                rows.push(Row::Raw { raw: line.clone() });
                continue;
            }
            let wrapped = wrap_text_with_ansi(line.trim_end(), content_width);
            for wrapped_line in wrapped {
                let inner_padding =
                    " ".repeat(content_width.saturating_sub(visible_width(&wrapped_line)));
                rows.push(Row::Content {
                    inner: format!("{wrapped_line}{inner_padding}"),
                });
            }
        }
    }

    let footer_fits = options.footer_meta.is_empty()
        || visible_width(&format!(" {} ", options.footer_meta))
            <= line_width
                .saturating_sub(visible_width(&format!(
                    "{}{cap}",
                    theme.symbol("boxRound.bottomLeft")
                )))
                .saturating_sub(visible_width(theme.symbol("boxRound.bottomRight")));
    if !options.footer_meta.is_empty() && !footer_fits {
        for wrapped_line in wrap_text_with_ansi(&options.footer_meta, content_width) {
            let inner_padding =
                " ".repeat(content_width.saturating_sub(visible_width(&wrapped_line)));
            rows.push(Row::Content {
                inner: format!("{wrapped_line}{inner_padding}"),
            });
        }
    }
    rows.push(Row::Bar {
        left: theme.symbol("boxRound.bottomLeft").to_string(),
        right: theme.symbol("boxRound.bottomRight").to_string(),
        label: None,
        meta: if footer_fits {
            option_str(&options.footer_meta)
        } else {
            None
        },
    });

    let render_bar = |left: &str,
                      right: &str,
                      label: Option<&str>,
                      meta: Option<&str>|
     -> String {
        let left_glyphs = format!("{left}{cap}");
        if line_width == 0 {
            let mut out = border(&left_glyphs);
            out.push_str(&border(right));
            return out;
        }
        let label_text = match (label, meta) {
            (Some(label), Some(meta)) if !label.is_empty() && !meta.is_empty() => {
                format!("{label}{}", theme.symbol("sep.dot")) + meta
            }
            (Some(label), _) if !label.is_empty() => label.to_string(),
            (_, Some(meta)) if !meta.is_empty() => meta.to_string(),
            _ => String::new(),
        };
        if label_text.is_empty() {
            let fill = line_width
                .saturating_sub(visible_width(&left_glyphs))
                .saturating_sub(visible_width(right));
            return format!(
                "{}{}{}",
                border(&left_glyphs),
                border(&horizontal.repeat(fill)),
                border(right)
            );
        }
        let raw_label = format!(" {label_text} ");
        let left_width = visible_width(&left_glyphs);
        let right_width = visible_width(right);
        let max_label_width = line_width
            .saturating_sub(left_width)
            .saturating_sub(right_width);
        let trimmed_label = truncate_to_width(&raw_label, max_label_width, Ellipsis::Unicode);
        let label_width = visible_width(&trimmed_label);
        let fill = line_width
            .saturating_sub(left_width)
            .saturating_sub(label_width)
            .saturating_sub(right_width);
        format!(
            "{}{}{}{}",
            border(&left_glyphs),
            trimmed_label,
            border(&horizontal.repeat(fill)),
            border(right)
        )
    };

    let clip_frame = line_width
        < (visible_width(&cap) + 2)
            .max(padding_left + padding_right + 2);

    let mut lines: Vec<String> = Vec::with_capacity(rows.len());
    for row in rows {
        let line = match row {
            Row::Raw { raw } => {
                if clip_frame {
                    truncate_to_width(&raw, line_width, Ellipsis::Omit)
                } else {
                    raw
                }
            }
            Row::Bar {
                left,
                right,
                label,
                meta,
            } => render_bar(&left, &right, label.as_deref(), meta.as_deref()),
            Row::Content { inner } => format!(
                "{}{}{}{}{}",
                border(vertical),
                left_padding,
                inner,
                right_padding,
                border(vertical)
            ),
        };
        let fitted = if clip_frame {
            truncate_to_width(&line, line_width, Ellipsis::Omit)
        } else {
            line
        };
        lines.push(pad_or_truncate_ansi(&fitted, line_width, bg_ansi.as_deref()));
    }
    lines
}

fn option_str(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// The dim marker drawn when a body is capped.
pub fn capped_marker(hidden: usize, collapsed: bool, theme: &Theme) -> String {
    let mut label = format!("… {hidden} more {}", plural(hidden));
    if collapsed {
        let hint = format_expand_hint(theme, Some(false), Some(true));
        if !hint.is_empty() {
            label.push(' ');
            label.push_str(&hint);
        }
    }
    paint(theme, ThemeColor::Dim, &label)
}

fn plural(hidden: usize) -> String {
    if hidden == 1 {
        "line".to_string()
    } else {
        "lines".to_string()
    }
}


impl OutputBlock {
    /// Create a block bound to `theme`.
    pub fn new(theme: Theme) -> Self {
        OutputBlock {
            theme,
            options: OutputBlockOptions {
                apply_bg: true,
                collapsed_max_lines: 3,
                ..OutputBlockOptions::default()
            },
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

    /// Set the header title.
    pub fn set_header(&mut self, header: impl Into<String>) -> &mut Self {
        self.options.header = header.into();
        self.cache = None;
        self
    }

    /// Set the header meta drawn beside the title.
    pub fn set_header_meta(&mut self, meta: impl Into<String>) -> &mut Self {
        self.options.header_meta = meta.into();
        self.cache = None;
        self
    }

    /// Set the facts drawn inside the bottom border.
    pub fn set_footer_meta(&mut self, meta: impl Into<String>) -> &mut Self {
        self.options.footer_meta = meta.into();
        self.cache = None;
        self
    }

    /// Replace the body with `lines` in one unlabelled section.
    pub fn set_lines(&mut self, lines: Vec<String>) -> &mut Self {
        self.options.sections = if lines.is_empty() {
            Vec::new()
        } else {
            vec![OutputSection::new(lines)]
        };
        self.cache = None;
        self
    }

    /// Replace the body from raw terminal text, collapsing CR progress
    /// overwrites and splitting lines.
    pub fn set_text(&mut self, text: &str) -> &mut Self {
        self.set_lines(split_terminal_output_lines(text))
    }

    /// Replace every section.
    pub fn set_sections(&mut self, sections: Vec<OutputSection>) -> &mut Self {
        self.options.sections = sections;
        self.cache = None;
        self
    }

    /// Set the lifecycle state driving border color and background.
    pub fn set_state(&mut self, state: Option<State>) -> &mut Self {
        self.options.state = state;
        self.cache = None;
        self
    }

    /// Whether the state background tint is applied.
    pub fn set_apply_bg(&mut self, apply: bool) -> &mut Self {
        self.options.apply_bg = apply;
        self.cache = None;
        self
    }

    /// Set the border color override.
    pub fn set_border_color(&mut self, color: Option<ThemeColor>) -> &mut Self {
        self.options.border_color = color;
        self.cache = None;
        self
    }

    /// Set the left and right content padding.
    pub fn set_padding(&mut self, left: usize, right: usize) -> &mut Self {
        self.options.content_padding_left = Some(left);
        self.options.content_padding_right = Some(right);
        self.cache = None;
        self
    }

    /// Collapse or expand the body.
    pub fn set_collapsed(&mut self, collapsed: bool) -> &mut Self {
        self.options.collapsed = collapsed;
        self.cache = None;
        self
    }

    /// Toggle the collapsed state.
    pub fn toggle_collapsed(&mut self) -> &mut Self {
        self.options.collapsed = !self.options.collapsed;
        self.cache = None;
        self
    }

    /// Whether the body is collapsed.
    pub fn collapsed(&self) -> bool {
        self.options.collapsed
    }

    /// Set the collapsed row budget.
    pub fn set_collapsed_max_lines(&mut self, max: usize) -> &mut Self {
        self.options.collapsed_max_lines = max;
        self.cache = None;
        self
    }

    /// Set an optional cap while expanded.
    pub fn set_expanded_max_lines(&mut self, max: Option<usize>) -> &mut Self {
        self.options.expanded_max_lines = max;
        self.cache = None;
        self
    }

    /// Drop the render cache.
    pub fn invalidate(&mut self) {
        self.cache = None;
    }

    /// The options this block renders from.
    pub fn options(&self) -> &OutputBlockOptions {
        &self.options
    }
}

impl Component for OutputBlock {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width as usize;
        if let Some((cached_width, rows)) = &self.cache {
            if *cached_width == width as u16 {
                return rows.clone();
            }
        }
        self.options.width = width;
        let rows = render_output_block(&self.options, &self.theme);
        self.cache = Some((width as u16, rows.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi_scan::strip_ansi;
    use crate::render::render_utils::tests::sample_theme;
    use crate::render::render_utils::{default_hidden_label, PaneEdge};

    fn theme() -> Theme {
        sample_theme()
    }

    #[test]
    fn plain_block_frames_header_and_body() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            sections: vec![OutputSection::new(vec!["echo hi".into()])],
            width: 12,
            apply_bg: false,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert_eq!(strip_ansi(&rows[0]), "╭─── Bash ─╮");
        assert_eq!(strip_ansi(&rows[1]), "│ echo hi  │");
        assert_eq!(strip_ansi(&rows[2]), "╰──────────╯");
        assert!(rows.iter().all(|row| visible_width(row) <= 12));
    }

    #[test]
    fn labelled_section_draws_separator() {
        let options = OutputBlockOptions {
            header: "Code".into(),
            sections: vec![
                OutputSection::new(vec!["a".into()]),
                OutputSection::labeled("Output", vec!["b".into()]),
            ],
            width: 16,
            apply_bg: false,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert_eq!(strip_ansi(&rows[2]), "├─── Output ───┤");
        assert!(rows.iter().all(|row| visible_width(row) <= 16));
    }

    #[test]
    fn footer_that_fits_rides_the_bottom_border() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            footer_meta: "Wall 0.16s".into(),
            sections: vec![OutputSection::new(vec!["x".into()])],
            width: 18,
            apply_bg: false,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert_eq!(strip_ansi(&rows[2]), "╰─── Wall 0.16s ─╯");
    }

    #[test]
    fn overflowing_footer_wraps_into_body_rows() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            footer_meta: "a very long footer that cannot fit".into(),
            sections: vec![OutputSection::new(vec!["x".into()])],
            width: 12,
            apply_bg: false,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert!(rows.iter().all(|row| visible_width(row) <= 12));
        assert_eq!(strip_ansi(rows.last().unwrap()), "╰──────────╯");
    }

    #[test]
    fn body_wraps_to_inner_width() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            sections: vec![OutputSection::new(vec!["aaaa bbbb cccc".into()])],
            width: 10,
            apply_bg: false,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert_eq!(strip_ansi(&rows[1]), "│ aaaa   │");
        assert_eq!(strip_ansi(&rows[2]), "│ bbbb   │");
        assert_eq!(strip_ansi(&rows[3]), "│ cccc   │");
    }

    #[test]
    fn collapsed_body_caps_and_marks() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            sections: vec![OutputSection::new(vec![
                "a".into(),
                "b".into(),
                "c".into(),
                "d".into(),
            ])],
            width: 40,
            apply_bg: false,
            collapsed: true,
            collapsed_max_lines: 2,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert_eq!(strip_ansi(&rows[1]), "│ a                                    │");
        assert_eq!(strip_ansi(&rows[2]), "│ b                                    │");
        assert!(rows.iter().any(|row| row.contains("more lines")));
        assert!(strip_ansi(rows.last().unwrap()).starts_with("╰"));
        assert!(rows.iter().all(|row| visible_width(row) <= 40));
    }

    #[test]
    fn expanded_cap_marks_hidden_tail() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            sections: vec![OutputSection::new(vec!["a".into(), "b".into(), "c".into()])],
            width: 40,
            apply_bg: false,
            expanded_max_lines: Some(2),
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert!(rows.iter().any(|row| row.contains("1 more line")));
    }

    #[test]
    fn component_setters_and_toggle() {
        let mut block = OutputBlock::new(theme());
        block.set_header("Bash");
        block.set_lines(vec!["one".into(), "two".into()]);
        let rows = block.render(10);
        assert_eq!(rows.len(), 4);
        block.handle_input(&Key::Ctrl('o'));
        assert!(block.collapsed());
        block.handle_input(&Key::Enter);
        assert!(!block.collapsed());
    }

    #[test]
    fn zero_width_is_safe() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            sections: vec![OutputSection::new(vec!["x".into()])],
            width: 0,
            apply_bg: false,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| row.is_empty()));
    }

    #[test]
    fn content_width_never_underflows() {
        assert_eq!(output_block_content_width(0, Some(1), None), 1);
        assert_eq!(output_block_content_width(10, Some(1), None), 6);
        assert_eq!(output_block_content_width(10, Some(2), Some(3)), 3);
    }

    #[test]
    fn hidden_label_matches_source_wording() {
        assert_eq!(
            default_hidden_label(3, 2, 5, PaneEdge::Tail),
            "… (3 earlier lines, showing 2 of 5)"
        );
        assert_eq!(
            default_hidden_label(1, 4, 5, PaneEdge::Head),
            "… 1 more line"
        );
    }

    #[test]
    fn state_background_tints_rows() {
        let options = OutputBlockOptions {
            header: "Bash".into(),
            state: Some(State::Success),
            sections: vec![OutputSection::new(vec!["x".into()])],
            width: 8,
            apply_bg: true,
            ..OutputBlockOptions::default()
        };
        let rows = render_output_block(&options, &theme());
        assert!(rows[0].starts_with("\x1b["));
        assert!(rows[0].ends_with("\x1b[49m"));
    }

    #[test]
    fn framed_block_brand_is_detected_on_the_stored_component() {
        struct Plain;
        impl Component for Plain {
            fn render(&mut self, _width: u16) -> Vec<String> {
                vec!["plain".to_string()]
            }
        }

        let unmarked: Box<dyn Component> = Box::new(Plain);
        assert!(!is_framed_block_component(&*unmarked));

        let marked = mark_framed_block_component(Box::new(Plain));
        assert!(is_framed_block_component(&*marked));

        // Moving the box keeps the heap allocation (and the brand) put.
        let moved = marked;
        assert!(is_framed_block_component(&*moved));
    }
}
