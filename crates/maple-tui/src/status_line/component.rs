//! The composed status-line bar: grouping, separators, overflow, and input.
//!
//! Ported from `l/src/status-line/component.ts` (`#renderStatusLine`,
//! `#buildContextGaugeFill`) plus the click-action map from
//! `NATIVE_SEGMENT_ACTIONS`. The bar is one terminal row built from the
//! configured left and right groups; rendering is deterministic and performs
//! no I/O. Session facts arrive through [`SegmentContext`].

use crate::component::Component;
use crate::keys::{is_enter, is_shift_tab, is_tab, Key};
use crate::mouse::{MouseButtonKind, MouseKind, MouseEvent};
use crate::status_line::schema::{
    ContextLineMode, EffectiveSettings, SegmentId, SegmentSide, StatusLineItem, StatusLineSettings,
};
use crate::status_line::separators::{get_separator, plain_separator, SeparatorDef};
use crate::status_line::segments::{
    format_number, render_segment_with, SegmentContext,
};
use crate::status_line::{ActiveTheme, StatusTheme};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};

/// Which composer surface the bar is drawn for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusLineLayout {
    /// Powerline groups joined by the context-reactive gauge line.
    #[default]
    Box,
    /// Box layout opened by a soft flush-left band cap.
    Band,
    /// Borderless full-width bar with dot separators.
    PlainFull,
    /// Left group only; the right group lives in the editor's top rule.
    PlainLeft,
    /// Right group only; the left group lives in the editor's top rule.
    PlainRight,
}

/// One rendered part of a group.
struct Part {
    /// ANSI-styled content.
    content: String,
    /// Visible cell width.
    width: usize,
    /// Configured segment id, when this part came from the catalog.
    id: Option<SegmentId>,
}

/// A clickable cell range produced by the last [`Component::render`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HitRegion {
    /// First column of the part.
    start: usize,
    /// Column one past the part.
    end: usize,
    /// Segment the part belongs to.
    id: Option<SegmentId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Left,
    Right,
}

/// Map a segment to the native action a click asks omp to open.
///
/// Mirrors `NATIVE_SEGMENT_ACTIONS`; segments without an entry are inert.
pub fn segment_action(id: SegmentId) -> Option<&'static str> {
    match id {
        SegmentId::Model => Some("status.model"),
        SegmentId::ContextPct | SegmentId::ContextTotal => Some("status.context"),
        SegmentId::Git => Some("status.git"),
        SegmentId::Cost => Some("status.cost"),
        SegmentId::Path => Some("status.path"),
        SegmentId::Cache => Some("status.cache"),
        _ => None,
    }
}

/// The status-line bar component.
pub struct StatusLine {
    settings: StatusLineSettings,
    effective: EffectiveSettings,
    items: Vec<StatusLineItem>,
    layout: StatusLineLayout,
    ctx: SegmentContext,
    job_count: u32,
    subagent_badge: Option<String>,
    on_select: Option<Box<dyn FnMut(&str)>>,
    selected: Option<usize>,
    hits: Vec<HitRegion>,
    visible_ids: Vec<SegmentId>,
}

impl Default for StatusLine {
    fn default() -> Self {
        StatusLine::new()
    }
}

impl StatusLine {
    /// Create a bar with the default preset and an empty context.
    pub fn new() -> Self {
        let settings = StatusLineSettings::default();
        let effective = EffectiveSettings::resolve(&settings);
        let mut ctx = SegmentContext::default();
        // Match the TypeScript default spinner period (`DEFAULT_SPINNER_INTERVAL_MS`).
        ctx.spinner_interval_ms = 80;
        StatusLine {
            settings,
            effective,
            items: Vec::new(),
            layout: StatusLineLayout::Box,
            ctx,
            job_count: 0,
            subagent_badge: None,
            on_select: None,
            selected: None,
            hits: Vec::new(),
            visible_ids: Vec::new(),
        }
    }

    /// Replace the settings and re-resolve the effective preset.
    pub fn set_settings(&mut self, settings: StatusLineSettings) {
        self.effective = EffectiveSettings::resolve(&settings);
        self.settings = settings;
    }

    /// Current settings.
    pub fn settings(&self) -> &StatusLineSettings {
        &self.settings
    }

    /// Explicit segment groups, overriding the preset's groups.
    ///
    /// An empty slice restores the preset's resolved groups.
    pub fn set_segments(&mut self, segments: Vec<StatusLineItem>) {
        self.items = segments;
    }

    /// Explicit segment groups.
    pub fn segments(&self) -> &[StatusLineItem] {
        &self.items
    }

    /// Select the composer layout.
    pub fn set_layout(&mut self, layout: StatusLineLayout) {
        self.layout = layout;
    }

    /// Current composer layout.
    pub fn layout(&self) -> StatusLineLayout {
        self.layout
    }

    /// Mutable access to the session facts used by the next render.
    pub fn context_mut(&mut self) -> &mut SegmentContext {
        &mut self.ctx
    }

    /// Replace the session facts.
    pub fn set_context(&mut self, ctx: SegmentContext) {
        self.ctx = ctx;
    }

    /// Set the background-job count prepended to the right group.
    pub fn set_job_count(&mut self, count: u32) {
        self.job_count = count;
    }

    /// Set an external subagent badge; while present the `subagents` segment is
    /// skipped so the count is not shown twice.
    pub fn set_subagent_badge(&mut self, badge: Option<String>) {
        self.subagent_badge = badge;
    }

    /// Install the callback invoked with a native action name on selection.
    pub fn set_on_select(&mut self, callback: impl FnMut(&str) + 'static) {
        self.on_select = Some(Box::new(callback));
    }

    /// Segment occupying `column` in the last rendered row, if any.
    pub fn segment_at(&self, column: u16) -> Option<SegmentId> {
        let column = column as usize;
        self.hits
            .iter()
            .find(|hit| column >= hit.start && column < hit.end)
            .and_then(|hit| hit.id)
    }

    /// Visible segment ids in render order from the last [`Component::render`].
    pub fn visible_segments(&self) -> &[SegmentId] {
        &self.visible_ids
    }

    fn groups(&self) -> (Vec<SegmentId>, Vec<SegmentId>) {
        let (mut left, mut right) = if self.items.is_empty() {
            (
                self.effective.left_segments.clone(),
                self.effective.right_segments.clone(),
            )
        } else {
            let mut left = Vec::new();
            let mut right = Vec::new();
            for item in &self.items {
                match item.side {
                    SegmentSide::Left => left.push(item.id),
                    SegmentSide::Right => right.push(item.id),
                }
            }
            (left, right)
        };
        // Standalone composers render only their half; the other half lives in
        // the editor's top rule.
        if matches!(self.layout, StatusLineLayout::PlainRight) {
            left.clear();
        }
        if matches!(self.layout, StatusLineLayout::PlainLeft) {
            right.clear();
        }
        (left, right)
    }

    fn skip_segment(&self, id: SegmentId) -> bool {
        if self.subagent_badge.is_some() && id == SegmentId::Subagents {
            return true;
        }
        matches!(self.layout, StatusLineLayout::Band) && id == SegmentId::SessionName
    }

    fn select_relative(&mut self, delta: isize) {
        if self.visible_ids.is_empty() {
            return;
        }
        let len = self.visible_ids.len() as isize;
        let next = match self.selected {
            None => 0,
            Some(index) => (index as isize + delta).rem_euclid(len),
        };
        self.selected = Some(next as usize);
    }

    fn activate(&mut self, id: SegmentId) {
        let action = match segment_action(id) {
            Some(action) => action,
            None => return,
        };
        if let Some(callback) = self.on_select.as_mut() {
            callback(action);
        }
    }

    fn compose(&self, width: usize) -> (String, Vec<HitRegion>, Vec<SegmentId>) {
        let theme = ActiveTheme::active();
        let plain = !matches!(
            self.layout,
            StatusLineLayout::Box | StatusLineLayout::Band
        );
        let separator = if plain {
            plain_separator()
        } else {
            get_separator(self.effective.separator, &*theme)
        };

        let mut ctx = self.ctx.clone();
        ctx.options = self.effective.segment_options.clone();
        ctx.compact_thinking_level = self.effective.compact_thinking_level;
        ctx.session_accent = if self.effective.session_accent {
            None
        } else {
            Some(false)
        };
        ctx.width = width as u16;

        let (left_ids, right_ids) = self.groups();
        let mut left: Vec<Part> = Vec::new();
        let mut right: Vec<Part> = Vec::new();
        let mut left_kept: Vec<SegmentId> = Vec::new();
        let mut right_kept: Vec<SegmentId> = Vec::new();

        for id in left_ids {
            if self.skip_segment(id) {
                continue;
            }
            let rendered = render_segment_with(id, &*theme, &ctx);
            if rendered.visible && !rendered.content.is_empty() {
                let part_width = visible_width(&rendered.content);
                left.push(Part {
                    content: rendered.content,
                    width: part_width,
                    id: Some(id),
                });
                left_kept.push(id);
            }
        }
        for id in right_ids {
            if self.skip_segment(id) {
                continue;
            }
            let rendered = render_segment_with(id, &*theme, &ctx);
            if rendered.visible && !rendered.content.is_empty() {
                let part_width = visible_width(&rendered.content);
                right.push(Part {
                    content: rendered.content,
                    width: part_width,
                    id: Some(id),
                });
                right_kept.push(id);
            }
        }

        let embed_context = !plain
            && self.effective.context_line == ContextLineMode::Embedded
            && ctx.context_window > 0
            && (left_kept.iter().any(|id| id.is_context())
                || right_kept.iter().any(|id| id.is_context()))
            && (left_kept.iter().any(|id| !id.is_context())
                || right_kept.iter().any(|id| !id.is_context()));
        if embed_context {
            retain_non_context(&mut left, &mut left_kept);
            retain_non_context(&mut right, &mut right_kept);
        }

        if !matches!(self.layout, StatusLineLayout::PlainLeft) {
            if self.job_count > 0 {
                let content = theme.fg(
                    "statusLineSubagents",
                    &format!("{} {}", theme.symbol("icon.job"), self.job_count),
                );
                insert_part(&mut right, content, None);
            }
            if let Some(badge) = self.subagent_badge.clone() {
                insert_part(&mut right, badge, None);
            }
        }

        let transparent_layout = matches!(
            self.layout,
            StatusLineLayout::PlainFull | StatusLineLayout::PlainLeft
        );
        let bg_ansi = if transparent_layout || self.effective.transparent {
            "\x1b[49m".to_string()
        } else {
            theme.bg_ansi("statusLineBg")
        };
        let transparent_bg = bg_ansi == "\x1b[49m";
        let fg_ansi = theme.fg_ansi("text");
        let sep_ansi = theme.fg_ansi("statusLineSep");
        let caps_visible = separator.end_caps.is_some() && !transparent_bg;
        let left_cap_width = if caps_visible {
            separator.left_cap_width()
        } else {
            0
        };
        let right_cap_width = if caps_visible {
            separator.right_cap_width()
        } else {
            0
        };
        let band_cap = if matches!(self.layout, StatusLineLayout::Band) && caps_visible {
            theme.sep("powerline-cap-left")
        } else {
            String::new()
        };
        let band_cap_width = if band_cap.is_empty() {
            0
        } else {
            visible_width(&band_cap)
        };
        let left_sep_width = separator.left_width();
        let right_sep_width = separator.right_width();

        let mut left_widths: Vec<usize> = left.iter().map(|part| part.width).collect();
        let mut right_widths: Vec<usize> = right.iter().map(|part| part.width).collect();
        let mut left_width = group_width(&left_widths, left_cap_width + band_cap_width, left_sep_width);
        let mut right_width = group_width(&right_widths, right_cap_width, right_sep_width);

        let embedded_width = if embed_context {
            embedded_context_gauge_min_width(ctx.context_percent, ctx.context_window)
        } else {
            0
        };
        let total_width = |left: &[Part],
                           right: &[Part],
                           left_width: usize,
                           right_width: usize|
         -> usize {
            let gap = if embedded_width == 0 {
                if left.is_empty() || right.is_empty() {
                    0
                } else {
                    1
                }
            } else if left.len() + right.len() == 1
                && left_width + right_width + embedded_width > width
            {
                1
            } else {
                embedded_width
            };
            left_width + right_width + gap
        };

        // The session title is the only elastic part on the right.
        if let Some(index) = right_kept
            .iter()
            .position(|id| *id == SegmentId::SessionName)
        {
            let part_index = index + (right.len() - right_kept.len());
            let current_width = right_widths[part_index];
            let shrink_by = current_width
                .saturating_sub(8)
                .min(total_width(&left, &right, left_width, right_width).saturating_sub(width));
            if shrink_by > 0 {
                right[part_index] =
                    truncate_part(&right[part_index], current_width - shrink_by);
                let name_width = visible_width(&right[part_index].content);
                right_width = right_width.saturating_add(name_width).saturating_sub(current_width);
                right_widths[part_index] = name_width;
            }
        }

        while total_width(&left, &right, left_width, right_width) > width && !right.is_empty() {
            let removed_width = right_widths.pop().unwrap_or(0);
            right.pop();
            right_width = if right.is_empty() {
                0
            } else {
                right_width
                    .saturating_sub(removed_width + right_sep_width + 2)
            };
        }

        // The path is the only elastic part on the left.
        if let Some(index) = left_kept.iter().position(|id| *id == SegmentId::Path) {
            let overflow = total_width(&left, &right, left_width, right_width).saturating_sub(width);
            if overflow > 0 {
                let current_width = left_widths[index];
                let shrinkable = current_width.saturating_sub(8);
                if shrinkable > 0 {
                    let shrink_by = shrinkable.min(overflow);
                    let current_max = ctx
                        .options
                        .path
                        .as_ref()
                        .and_then(|options| options.max_length)
                        .unwrap_or(40);
                    let mut max_len = current_max
                        .min(current_width)
                        .saturating_sub(shrink_by)
                        .max(4);
                    let mut rendered = render_path(&ctx, max_len, &*theme);
                    if let Some(part) = rendered.take() {
                        let mut path_width = part.width;
                        for _ in 0..8 {
                            let saved = current_width - path_width;
                            if saved >= shrink_by {
                                break;
                            }
                            let next_len = max_len.saturating_sub(shrink_by - saved).max(4);
                            if next_len >= max_len {
                                break;
                            }
                            max_len = next_len;
                            match render_path(&ctx, max_len, &*theme) {
                                Some(part) => {
                                    path_width = part.width;
                                    rendered = Some(part);
                                }
                                None => break,
                            }
                        }
                        if let Some(part) = rendered {
                            left_width = left_width
                                .saturating_add(part.width)
                                .saturating_sub(current_width);
                            left[index] = part;
                            left_widths[index] = path_width;
                        }
                    }
                }
            }
        }

        while total_width(&left, &right, left_width, right_width) > width && !left.is_empty() {
            let drop_index = left_overflow_drop_index(&left_kept);
            let drop_index = drop_index.min(left.len() - 1);
            let removed_width = left_widths.remove(drop_index);
            left.remove(drop_index);
            left_kept.remove(drop_index);
            left_width = if left.is_empty() {
                0
            } else {
                left_width
                    .saturating_sub(removed_width + left_sep_width + 2)
            };
        }

        let use_bg_as_fg = separator
            .end_caps
            .as_ref()
            .map(|caps| caps.use_bg_as_fg)
            .unwrap_or(false);
        let cap_prefix = if use_bg_as_fg {
            bg_ansi.replace("\x1b[48;", "\x1b[38;")
        } else {
            format!("{bg_ansi}{sep_ansi}")
        };
        let cap_left = if caps_visible {
            separator
                .end_caps
                .as_ref()
                .map(|caps| caps.left.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let cap_right = if caps_visible {
            separator
                .end_caps
                .as_ref()
                .map(|caps| caps.right.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let dim = ctx.focused_agent_id.is_some();

        let concat = plain && (left.is_empty() || right.is_empty());
        let gap_width = if concat {
            0
        } else {
            width
                .saturating_sub(left_width + right_width)
                .max(1)
        };
        let right_start = if concat {
            if left.is_empty() {
                0
            } else {
                left_width + 1
            }
        } else {
            left_width + gap_width
        };

        let (left_group, mut hits) = render_group(
            &left,
            Direction::Left,
            &separator,
            &bg_ansi,
            &fg_ansi,
            &sep_ansi,
            &cap_right,
            &cap_prefix,
            &band_cap,
            dim,
            0,
            left_sep_width,
        );
        let (right_group, mut right_hits) = render_group(
            &right,
            Direction::Right,
            &separator,
            &bg_ansi,
            &fg_ansi,
            &sep_ansi,
            &cap_left,
            &cap_prefix,
            "",
            dim,
            right_start,
            right_sep_width,
        );
        hits.append(&mut right_hits);

        if left_group.is_empty() && right_group.is_empty() {
            return (String::new(), Vec::new(), Vec::new());
        }

        let row = if concat {
            let mut row = left_group;
            row.push_str(&right_group);
            row
        } else if plain {
            let mut row = left_group;
            row.push_str(&" ".repeat(gap_width));
            row.push_str(&right_group);
            row
        } else {
            let gauge = self.context_gauge(&*theme, gap_width, &ctx, embed_context);
            let mut row = left_group;
            row.push_str(&gauge);
            row.push_str(&right_group);
            row
        };

        let visible_ids: Vec<SegmentId> = left
            .iter()
            .chain(right.iter())
            .filter_map(|part| part.id)
            .collect();
        (row, hits, visible_ids)
    }

    fn context_gauge(
        &self,
        theme: &dyn StatusTheme,
        gap_width: usize,
        ctx: &SegmentContext,
        embed_context: bool,
    ) -> String {
        let accent = if self.effective.session_accent {
            ctx.session
                .session_name
                .as_deref()
                .and_then(|name| theme.session_accent_ansi(name))
        } else {
            None
        };
        let used_color = accent.unwrap_or_else(|| theme.fg_ansi("borderAccent"));
        let horizontal = theme.horizontal();
        let mode = self.effective.context_line;
        let percent = ctx.context_percent;
        if mode == ContextLineMode::Off || (percent.is_none() && ctx.context_window == 0) {
            return format!(
                "\x1b[49m{used_color}{}\x1b[39m",
                horizontal.repeat(gap_width)
            );
        }

        let clamped = percent.map(|value| value.clamp(0.0, 100.0)).unwrap_or(0.0);
        let mut percent_label = String::new();
        let mut window_label = String::new();
        let mut percent_start: isize = -1;
        let mut window_start: isize = -1;
        let mut scale_width = gap_width;
        let percent_overflow = percent.map(|value| value > 100.0).unwrap_or(false);
        if embed_context {
            let candidate_percent = match percent {
                Some(value) if percent_overflow => format_embedded_percent(value),
                Some(value) => format_embedded_percent(clamped),
                None => String::new(),
            };
            let candidate_window = format_number(ctx.context_window as f64);
            if gap_width >= embedded_context_gauge_min_width(percent, ctx.context_window) {
                percent_label = candidate_percent;
                window_label = candidate_window;
                if percent_overflow {
                    percent_start = (gap_width - percent_label.len()) as isize;
                    window_start = percent_start - 1 - window_label.len() as isize;
                } else {
                    window_start = (gap_width - window_label.len() - 1) as isize;
                }
                scale_width = window_start.max(0) as usize;
            }
        }

        let raw_used = ((clamped / 100.0) * scale_width as f64).round() as usize;
        let used_count = scale_width.min(raw_used.max(1));

        // Non-overflow embedded labels are not anchored; walk outward from the
        // preferred cell (the used portion) until a free slot is found. The
        // TypeScript loop also avoids compaction markers, which this slice does
        // not render, so every candidate is free.
        if !percent_label.is_empty() && percent_start < 0 {
            let max_start = scale_width as isize - percent_label.len() as isize - 1;
            let preferred_start = max_start.min(used_count.max(1) as isize);
            let mut distance = 0isize;
            while distance <= max_start {
                let left = preferred_start - distance;
                if left >= 1 {
                    percent_start = left;
                    break;
                }
                if distance != 0 {
                    let right = preferred_start + distance;
                    if right <= max_start {
                        percent_start = right;
                        break;
                    }
                }
                distance += 1;
            }
        }
        let unused_color = theme.fg_ansi("border");
        let overflow_color = theme.fg_ansi("error");

        let percent_end = if percent_start >= 0 {
            percent_start + percent_label.len() as isize
        } else {
            -1
        };
        let window_end = if window_start >= 0 {
            window_start + window_label.len() as isize
        } else {
            -1
        };

        let mut out = String::from("\x1b[49m");
        let mut active_color = "";
        let mut index = 0usize;
        while index < gap_width {
            let color;
            let text;
            let next;
            if percent_start >= 0
                && index >= percent_start as usize
                && index < percent_end as usize
            {
                let end = percent_end.min(gap_width as isize);
                color = if percent_overflow {
                    overflow_color.as_str()
                } else {
                    used_color.as_str()
                };
                text = percent_label[index - percent_start as usize..end as usize - percent_start as usize]
                    .to_string();
                next = end as usize;
            } else {
                let in_window = window_start >= 0
                    && index >= window_start as usize
                    && index < window_end as usize;
                let mut end = if in_window {
                    window_end as usize
                } else if index < used_count {
                    used_count
                } else {
                    gap_width
                };
                end = end.min(gap_width);
                if percent_start > index as isize && (percent_start as usize) < end {
                    end = percent_start as usize;
                }
                if !in_window && window_start > index as isize && (window_start as usize) < end {
                    end = window_start as usize;
                }
                if in_window {
                    // The threshold cell needs a dimmed accent; without the
                    // compaction slice the accent itself carries the label.
                    color = used_color.as_str();
                    text = window_label
                        [index - window_start as usize..end - window_start as usize]
                        .to_string();
                } else {
                    color = if index < used_count {
                        used_color.as_str()
                    } else {
                        unused_color.as_str()
                    };
                    text = horizontal.repeat(end - index);
                }
                next = end;
            }
            if color != active_color {
                out.push_str(color);
                active_color = color;
            }
            out.push_str(&text);
            index = next;
        }
        out.push_str("\x1b[39m");
        out
    }
}

impl Component for StatusLine {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            self.hits.clear();
            self.visible_ids.clear();
            return Vec::new();
        }
        let (row, hits, visible_ids) = self.compose(width as usize);
        self.hits = hits;
        self.visible_ids = visible_ids;
        if row.is_empty() {
            Vec::new()
        } else {
            vec![row]
        }
    }

    fn handle_input(&mut self, key: &Key) {
        if self.visible_ids.is_empty() {
            return;
        }
        if is_tab(key) {
            self.select_relative(1);
        } else if is_shift_tab(key) {
            self.select_relative(-1);
        } else if is_enter(key) {
            if let Some(index) = self.selected {
                let id = self.visible_ids[index];
                self.activate(id);
            }
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if event.kind != MouseKind::Down(MouseButtonKind::Left) || event.row != 0 {
            return;
        }
        if let Some(id) = self.segment_at(event.column) {
            self.activate(id);
        }
    }
}

fn group_width(widths: &[usize], cap_width: usize, sep_width: usize) -> usize {
    if widths.is_empty() {
        return 0;
    }
    widths.iter().sum::<usize>() + (widths.len() - 1) * (sep_width + 2) + 2 + cap_width
}

fn retain_non_context(parts: &mut Vec<Part>, ids: &mut Vec<SegmentId>) {
    let mut index = 0;
    parts.retain(|part| {
        let keep = part.id.map(|id| !id.is_context()).unwrap_or(true);
        if !keep {
            ids.remove(index);
        } else {
            index += 1;
        }
        keep
    });
}

fn insert_part(parts: &mut Vec<Part>, content: String, id: Option<SegmentId>) {
    let width = visible_width(&content);
    parts.insert(
        0,
        Part {
            content,
            width,
            id,
        },
    );
}

fn truncate_part(part: &Part, target_width: usize) -> Part {
    let content = truncate_to_width(&part.content, target_width, Ellipsis::Unicode);
    Part {
        content,
        width: target_width,
        id: part.id,
    }
}

fn render_path(ctx: &SegmentContext, max_len: usize, theme: &dyn StatusTheme) -> Option<Part> {
    let mut path_ctx = ctx.clone();
    let mut options = path_ctx.options.path.clone().unwrap_or_default();
    options.max_length = Some(max_len);
    path_ctx.options.path = Some(options);
    let rendered = render_segment_with(SegmentId::Path, theme, &path_ctx);
    if rendered.visible && !rendered.content.is_empty() {
        Some(Part {
            width: visible_width(&rendered.content),
            content: rendered.content,
            id: Some(SegmentId::Path),
        })
    } else {
        None
    }
}

fn left_overflow_drop_index(ids: &[SegmentId]) -> usize {
    for index in (0..ids.len()).rev() {
        if ids[index] != SegmentId::Path {
            return index;
        }
    }
    ids.len().saturating_sub(1)
}

fn render_group(
    parts: &[Part],
    direction: Direction,
    separator: &SeparatorDef,
    bg_ansi: &str,
    fg_ansi: &str,
    sep_ansi: &str,
    cap: &str,
    cap_prefix: &str,
    open_cap: &str,
    dim: bool,
    group_start: usize,
    sep_width: usize,
) -> (String, Vec<HitRegion>) {
    if parts.is_empty() {
        return (String::new(), Vec::new());
    }
    let sep = match direction {
        Direction::Left => &separator.left,
        Direction::Right => &separator.right,
    };
    let mut content = String::new();
    content.push_str(bg_ansi);
    content.push_str(fg_ansi);
    content.push(' ');
    let mut first = true;
    for part in parts {
        if !first {
            content.push(' ');
            content.push_str(sep_ansi);
            content.push_str(sep);
            content.push_str(fg_ansi);
            content.push(' ');
        }
        first = false;
        content.push_str(&part.content);
    }
    content.push(' ');
    content.push_str("\x1b[0m");

    let dim_text = if dim { "\x1b[22m" } else { "" };
    let cap_text = if cap.is_empty() {
        String::new()
    } else {
        format!("{cap_prefix}{dim_text}{cap}\x1b[0m")
    };
    let open_text = if open_cap.is_empty() {
        String::new()
    } else {
        format!("{cap_prefix}{dim_text}{open_cap}\x1b[0m")
    };

    let prefix_width = match direction {
        Direction::Left => visible_width(open_cap),
        Direction::Right => visible_width(cap),
    };
    let mut hits = Vec::new();
    let mut cursor = group_start + prefix_width + 1;
    for part in parts {
        hits.push(HitRegion {
            start: cursor,
            end: cursor + part.width,
            id: part.id,
        });
        cursor += part.width + sep_width + 2;
    }

    let group = match direction {
        Direction::Right => format!("{cap_text}{content}"),
        Direction::Left => format!("{open_text}{content}{cap_text}"),
    };
    (group, hits)
}

fn format_embedded_percent(percent: f64) -> String {
    if percent > 0.0 && percent < 1.0 {
        format!("{percent:.1}%")
    } else {
        format!("{}%", percent.round() as i64)
    }
}

fn embedded_context_gauge_min_width(percent: Option<f64>, context_window: u64) -> usize {
    let percent_width = match percent {
        Some(value) => format_embedded_percent(value).len() + 2,
        None => 0,
    };
    percent_width + format_number(context_window as f64).len() + 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Key;
    use crate::mouse::{MouseButtonKind, MouseKind, MouseEvent};
    use crate::status_line::schema::{
        ModelOptions, PathOptions, SeparatorStyle, SegmentOptions, StatusLinePreset,
    };
    use crate::status_line::{FallbackTheme, StatusTheme};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct PlainTheme;

    impl StatusTheme for PlainTheme {
        fn fg_ansi(&self, _token: &str) -> String {
            String::new()
        }

        fn bg_ansi(&self, _token: &str) -> String {
            String::new()
        }

        fn symbol(&self, key: &str) -> String {
            FallbackTheme::unicode_symbol(key).to_string()
        }

        fn sep(&self, name: &str) -> String {
            FallbackTheme::new().sep(name)
        }

        fn horizontal(&self) -> String {
            "─".to_string()
        }

        fn spinner_frames(&self, kind: &str) -> Vec<String> {
            FallbackTheme::new().spinner_frames(kind)
        }
    }

    fn install_theme() {
        ActiveTheme::set_active(Rc::new(PlainTheme));
    }

    fn bar() -> StatusLine {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_segments(vec![StatusLineItem::left(SegmentId::Model)]);
        bar.context_mut().session.model = crate::status_line::segments::ModelInfo {
            name: Some("Claude Sonnet".to_string()),
            ..crate::status_line::segments::ModelInfo::default()
        };
        bar
    }

    fn click(column: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column,
            row: 0,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    #[test]
    fn row_fits_width_across_sizes() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Pi),
            StatusLineItem::left(SegmentId::Model),
            StatusLineItem::left(SegmentId::Path),
            StatusLineItem::left(SegmentId::Git),
            StatusLineItem::right(SegmentId::SessionName),
            StatusLineItem::right(SegmentId::TokenTotal),
            StatusLineItem::right(SegmentId::Cost),
            StatusLineItem::right(SegmentId::ContextPct),
        ]);
        let mut ctx = bar.context_mut();
        ctx.session.model.name = Some("Claude Sonnet".to_string());
        ctx.session.session_name = Some("A very long session title that will not fit".to_string());
        ctx.project_dir = "/home/user/projects/maple-tui".to_string();
        ctx.home_dir = "/home/user".to_string();
        ctx.context_percent = Some(42.0);
        ctx.context_window = 200_000;
        drop(ctx);

        for width in 1..=120u16 {
            let rows = bar.render(width);
            assert!(
                rows.iter().all(|row| visible_width(row) <= width as usize),
                "width {width}"
            );
        }
        assert!(!bar.render(80).is_empty());
    }

    #[test]
    fn narrow_width_keeps_path_and_drops_tail_segments() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Model),
            StatusLineItem::left(SegmentId::Path),
            StatusLineItem::left(SegmentId::Git),
        ]);
        bar.context_mut().project_dir = "/home/user/projects/maple-tui".to_string();
        bar.context_mut().home_dir = "/home/user".to_string();
        bar.context_mut().git.branch = Some("main".to_string());
        let rows = bar.render(14);
        assert_eq!(rows.len(), 1);
        assert!(visible_width(&rows[0]) <= 14);
        assert!(rows[0].contains('…'), "{}", rows[0]);
    }

    #[test]
    fn plain_layout_pads_to_width() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_layout(StatusLineLayout::PlainFull);
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Model),
            StatusLineItem::right(SegmentId::Cost),
        ]);
        bar.context_mut().session.model.name = Some("Claude Sonnet".to_string());
        bar.context_mut().usage_stats.cost = 1.0;
        let rows = bar.render(40);
        assert_eq!(rows.len(), 1);
        assert_eq!(visible_width(&rows[0]), 40);
        assert!(rows[0].contains("⬢ Sonnet"));
        assert!(rows[0].contains("$1.00"));
    }

    #[test]
    fn exact_plain_row_matches_expected_string() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_layout(StatusLineLayout::PlainFull);
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Model),
            StatusLineItem::right(SegmentId::Cost),
        ]);
        bar.context_mut().session.model.name = Some("Claude Sonnet".to_string());
        bar.context_mut().usage_stats.cost = 1.0;
        let rows = bar.render(20);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            "\x1b[49m ⬢ Sonnet\x1b[39m \x1b[0m   \x1b[49m $1.00\x1b[39m \x1b[0m"
        );
    }

    #[test]
    fn exact_box_row_with_ascii_separator() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_settings(StatusLineSettings {
            separator: Some(SeparatorStyle::Ascii),
            ..StatusLineSettings::default()
        });
        bar.set_segments(vec![StatusLineItem::left(SegmentId::Model)]);
        bar.context_mut().session.model.name = Some("Claude Sonnet".to_string());
        let rows = bar.render(20);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            " ⬢ Sonnet\x1b[39m \x1b[0m\x1b[49m──────────\x1b[39m"
        );
        assert_eq!(visible_width(&rows[0]), 20);
    }

    #[test]
    fn click_hits_segment_and_fires_action() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut bar = bar();
        let recorded = Rc::clone(&calls);
        bar.set_on_select(move |action| recorded.borrow_mut().push(action.to_string()));
        bar.render(60);
        let part_width = visible_width("⬢ Sonnet");
        assert_eq!(bar.segment_at(1), Some(SegmentId::Model));
        assert_eq!(bar.segment_at(1 + part_width as u16), None);
        bar.handle_mouse(&click(2));
        assert_eq!(*calls.borrow(), vec!["status.model".to_string()]);
    }

    #[test]
    fn keyboard_tab_and_enter_activate() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Model),
            StatusLineItem::left(SegmentId::Cost),
        ]);
        bar.context_mut().session.model.name = Some("Claude Sonnet".to_string());
        bar.context_mut().usage_stats.cost = 1.0;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&calls);
        bar.set_on_select(move |action| recorded.borrow_mut().push(action.to_string()));
        bar.render(60);
        assert_eq!(bar.visible_segments(), &[SegmentId::Model, SegmentId::Cost]);
        bar.handle_input(&Key::Tab);
        bar.handle_input(&Key::Enter);
        assert_eq!(*calls.borrow(), vec!["status.model".to_string()]);
        bar.handle_input(&Key::Tab);
        bar.handle_input(&Key::Enter);
        assert_eq!(
            *calls.borrow(),
            vec!["status.model".to_string(), "status.cost".to_string()]
        );
        bar.handle_input(&Key::BackTab);
        bar.handle_input(&Key::Enter);
        assert_eq!(
            *calls.borrow(),
            vec![
                "status.model".to_string(),
                "status.cost".to_string(),
                "status.model".to_string()
            ]
        );
    }

    #[test]
    fn set_segments_overrides_preset_groups() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_settings(StatusLineSettings {
            preset: Some(StatusLinePreset::Minimal),
            ..StatusLineSettings::default()
        });
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Cost),
            StatusLineItem::right(SegmentId::Model),
        ]);
        bar.context_mut().usage_stats.cost = 2.0;
        bar.context_mut().session.model.name = Some("Claude Sonnet".to_string());
        let rows = bar.render(60);
        assert!(rows[0].find("$2.00").unwrap() < rows[0].find("⬢ Sonnet").unwrap());
    }

    #[test]
    fn external_badge_skips_subagents_segment() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Model),
            StatusLineItem::left(SegmentId::Subagents),
            StatusLineItem::right(SegmentId::SessionName),
        ]);
        bar.context_mut().subagent_count = 3;
        bar.context_mut().session.model.name = Some("Claude Sonnet".to_string());
        bar.set_subagent_badge(Some("badge".to_string()));
        let rows = bar.render(80);
        assert!(!rows[0].contains("👥 3"));
        assert!(rows[0].contains("badge"));
    }

    #[test]
    fn embedded_context_reserves_gauge_width() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_segments(vec![
            StatusLineItem::left(SegmentId::Model),
            StatusLineItem::left(SegmentId::ContextPct),
            StatusLineItem::right(SegmentId::SessionName),
        ]);
        bar.context_mut().session.model.name = Some("Claude Sonnet".to_string());
        bar.context_mut().session.session_name = Some("t".to_string());
        bar.context_mut().context_percent = Some(50.0);
        bar.context_mut().context_window = 200_000;
        let rows = bar.render(60);
        assert!(rows[0].contains("50%"), "{}", rows[0]);
        assert!(rows[0].contains("200K"), "{}", rows[0]);
    }

    #[test]
    fn zero_width_renders_nothing() {
        install_theme();
        let mut bar = bar();
        assert!(bar.render(0).is_empty());
        assert_eq!(bar.segment_at(0), None);
    }

    #[test]
    fn preset_options_drive_segments() {
        install_theme();
        let mut bar = StatusLine::new();
        bar.set_settings(StatusLineSettings {
            preset: Some(StatusLinePreset::Full),
            segment_options: Some(SegmentOptions {
                path: Some(PathOptions {
                    max_length: Some(10),
                    ..PathOptions::default()
                }),
                model: Some(ModelOptions {
                    show_thinking_level: Some(false),
                }),
                ..SegmentOptions::default()
            }),
            ..StatusLineSettings::default()
        });
        bar.context_mut().project_dir = "/home/user/projects/maple-tui".to_string();
        bar.context_mut().home_dir = "/home/user".to_string();
        bar.context_mut().session.model = crate::status_line::segments::ModelInfo {
            name: Some("Claude Sonnet".to_string()),
            thinking: true,
            thinking_level: Some("high".to_string()),
            ..crate::status_line::segments::ModelInfo::default()
        };
        let rows = bar.render(200);
        assert!(rows[0].contains("⬢ Sonnet"));
        assert!(!rows[0].contains("high"), "{}", rows[0]);
    }
}
