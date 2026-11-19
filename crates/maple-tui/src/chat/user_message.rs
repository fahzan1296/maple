//! The user's prompt bubble in the transcript.
//!
//! Reference: `l/src/chat/user-message.ts`.
//!
//! A user-toned frame with the prompt as Markdown under its attached images,
//! the live-steering marker and the agent's reaction chips at the bottom-right,
//! a hover toolbar (Copy, Rewind, time sent) for terminals that handle
//! transcript actions, and a collapse placeholder for huge synthetic inputs.
//!
//! Deviations:
//! - The reference reaches for a process-global `theme`; this port passes
//!   `&Theme` explicitly (the crate's convention).
//! - [`crate::components::Markdown`] takes no background/foreground callbacks
//!   and no `marks`, so [`user_bubble_color`] and [`token_marks`] return the
//!   styled strings/spans the terminal layer wants, while [`Markdown`] draws
//!   the bubble's rows and the bubble re-applies its background per row.
//! - `handleNativeEvent` is [`UserMessageComponent::handle_action`], and
//!   [`CollapsedSyntheticMessageComponent`] keeps its disclosure state itself
//!   (`crate::components::Disclosure` renders text, not a lazy child).
//! - The OSC 133 zone toggle is a process-wide flag, as in the reference.

use std::cell::RefCell;
use std::rc::Rc;

use regex::Regex;

use crate::components::markdown::{Markdown, MarkdownTheme};
use crate::component::Component;
use crate::native::describe::{card, md, node, row, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text, Tone};
use crate::prompt::composer_attachments::{
    attachment_sgr, collapse_image_markers, composer_token_regex, model_chip_style,
    model_mention_chip_label, render_placeholders, skill_chip_style, ChipKind, PlaceholderForm,
    PlaceholderKind, PlaceholderRenderers,
};
use crate::prompt::image_references::{image_links_enabled, image_reference_hyperlink};
use crate::prompt::magic_keywords::highlight_magic_keywords;
use crate::prompt::model_mention_syntax::MODEL_MENTION_TAG_RE;
use crate::render::render_utils::{expand_key_hint, padding, stabilize_background_ansi};
use crate::theme::Theme;
use crate::text_shape::visible_width;

/// OSC 133 prompt zone start.
const OSC133_ZONE_START: &str = "\x1b]133;A\x07";
/// OSC 133 zone end (latches the sticky input semantic).
const OSC133_ZONE_END: &str = "\x1b]133;B\x07";
/// OSC 133 command start.
const OSC133_COMMAND_START: &str = "\x1b]133;C\x07";
/// OSC 133 command done.
const OSC133_COMMAND_DONE: &str = "\x1b]133;D;0\x07";

/// The zone close sequence: end, command start, command done.
fn osc133_zone_close() -> String {
    format!("{OSC133_ZONE_END}{OSC133_COMMAND_START}{OSC133_COMMAND_DONE}")
}

static PROMPT_ZONES_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

/// Turn the OSC 133 zone marks on user bubbles on or off; bubbles pick it up on
/// their next render.
pub fn set_prompt_zones_enabled(enabled: bool) {
    PROMPT_ZONES_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Whether user bubbles currently emit OSC 133 zones.
pub fn prompt_zones_enabled() -> bool {
    PROMPT_ZONES_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// How a user bubble styles its prose and chips (see [`user_bubble_color`]).
#[derive(Clone, Default)]
pub struct UserBubbleOptions {
    /// Filesystem paths for attached image chips, indexed by chip number.
    pub image_links: Option<Vec<Option<String>>>,
    /// The message's attached images in chip order (`#1` first).
    pub images: Option<Vec<crate::prompt::image_source::ImageContent>>,
    /// Agent-attributed input: dim, flat prose.
    pub synthetic: bool,
    /// Delivered into the response that was streaming.
    pub live_steered: bool,
    /// SKILL.md path for a skill chip by name.
    pub skill_path: Option<Rc<dyn Fn(&str) -> Option<String>>>,
    /// When the message was sent (ms); shown beside the hover toolbar.
    pub timestamp: Option<i64>,
}

/// Everything [`user_bubble_color`] needs beyond the theme.
struct BubbleRenderers<'a> {
    theme: &'a Theme,
    options: &'a UserBubbleOptions,
    token_reset: String,
    bubble_reset: String,
}

impl BubbleRenderers<'_> {
    fn render_text(&self, text: &str) -> String {
        if self.options.synthetic {
            self.theme
                .fg("dim", text)
                .unwrap_or_else(|_| text.to_string())
        } else {
            let highlighted = highlight_magic_keywords(
                text,
                Some(&self.token_reset),
                0.0,
                self.theme.get_color_mode(),
            );
            self.theme
                .fg_on_bg("userMessageText", "userMessageBg", &highlighted)
                .unwrap_or_else(|_| text.to_string())
        }
    }
}

impl PlaceholderRenderers for BubbleRenderers<'_> {
    fn render_text(&self, text: &str) -> String {
        BubbleRenderers::render_text(self, text)
    }

    fn render_reference(
        &self,
        label: &str,
        kind: PlaceholderKind,
        index: i64,
        form: PlaceholderForm,
    ) -> String {
        let styled = match form {
            PlaceholderForm::Chip => format!(
                "{}\x1b[1m{label}\x1b[22m{}",
                attachment_sgr(chip_kind(kind), index),
                self.token_reset
            ),
            PlaceholderForm::Marker => self
                .theme
                .fg("accent", &format!("\x1b[1m{label}\x1b[22m"))
                .unwrap_or_else(|_| label.to_string()),
        };
        if kind == PlaceholderKind::Image || kind == PlaceholderKind::Video {
            let links = self.options.image_links.as_deref();
            let slot = index.max(0) as usize;
            return image_reference_hyperlink(label, slot, links, &|_label| styled.clone());
        }
        styled
    }

    fn render_skill(&self, label: &str, name: &str) -> String {
        let styled = skill_chip_style(label, &self.bubble_reset, Some(self.theme));
        match self.options.skill_path.as_ref().and_then(|resolve| resolve(name)) {
            Some(path) => crate::render::hyperlink::file_hyperlink(
                &path,
                &styled,
                crate::render::hyperlink::FilePosition {
                    line: Some(1),
                    col: None,
                },
            ),
            None => styled,
        }
    }

    fn render_mention(&self, label: &str) -> String {
        model_chip_style(label, &self.bubble_reset, Some(self.theme))
    }
}

fn chip_kind(kind: PlaceholderKind) -> ChipKind {
    match kind {
        PlaceholderKind::Image => ChipKind::Image,
        PlaceholderKind::Video => ChipKind::Video,
        PlaceholderKind::Paste => ChipKind::Paste,
    }
}

/// Foreground styling for prose inside a user bubble: the bubble text color
/// with the magic-keyword glow, attachment chips in their composer identity
/// color, and skill chips as soft pills (linked to their SKILL.md).
pub fn user_bubble_color<'a>(
    theme: &'a Theme,
    options: &'a UserBubbleOptions,
    token_regex: Option<&'a Regex>,
) -> impl Fn(&str) -> String + 'a {
    let token_reset = theme
        .get_fg_ansi("userMessageText")
        .ok()
        .map(|fg| format!("{fg}{}", theme.get_bg_ansi("userMessageBg").unwrap_or_default()))
        .unwrap_or_default();
    let bubble_reset = format!(
        "{token_reset}{}",
        theme.get_bg_ansi("userMessageBg").unwrap_or_default()
    );
    let renderers = BubbleRenderers {
        theme,
        options,
        token_reset,
        bubble_reset,
    };
    move |value: &str| render_placeholders(value, &renderers, token_regex)
}

/// A transcript animation frame (only the allocation matters here).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimationFrame {
    /// Unused by this component.
    pub tick: u64,
}

/// Component that renders a user message.
pub struct UserMessageComponent {
    allocation: usize,
    bg_ansi: String,
    live_steered: bool,
    synthetic: bool,
    timestamp: Option<i64>,
    images: Vec<crate::prompt::image_source::ImageContent>,
    image_links: Option<Vec<Option<String>>>,
    /// Display text: image markers collapsed to chips.
    text: String,
    /// Matches the composer tokens in [`Self::text`].
    tokens: Regex,
    reaction: Option<String>,
    markdown: Markdown,
    theme: Theme,
}

impl UserMessageComponent {
    /// A bubble for `text`.
    pub fn new(text: impl Into<String>, options: UserBubbleOptions, theme: &Theme) -> Self {
        let mut text = collapse_image_markers(&text.into(), i64::MAX, &mut |_kind, _label| {});
        let mut mention_labels = Vec::new();
        text = MODEL_MENTION_TAG_RE
            .replace_all(&text, |caps: &regex::Captures<'_>| {
                let name = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
                let label = model_mention_chip_label(name, Some(theme));
                mention_labels.push(label.clone());
                label
            })
            .into_owned();
        let tokens = composer_token_regex(&mention_labels);

        let mut markdown = Markdown::new();
        markdown.set_text(text.clone());
        markdown.set_padding(1, 1);
        markdown.set_theme(MarkdownTheme::default());

        let bg_ansi = theme.get_bg_ansi("userMessageBg").unwrap_or_default();
        UserMessageComponent {
            allocation: usize::MAX,
            bg_ansi,
            live_steered: options.live_steered,
            synthetic: options.synthetic,
            timestamp: options.timestamp,
            images: options.images.unwrap_or_default(),
            image_links: options.image_links,
            text,
            tokens,
            reaction: None,
            markdown,
            theme: theme.clone(),
        }
    }

    /// Set the agent's reaction emoji badge.
    pub fn set_reaction(&mut self, emoji: impl Into<String>) {
        let emoji = emoji.into();
        if self.reaction.as_deref() == Some(emoji.as_str()) {
            return;
        }
        self.reaction = Some(emoji);
    }

    /// The agent's reaction badge, if any.
    pub fn reaction(&self) -> Option<&str> {
        self.reaction.as_deref()
    }

    /// The display text (markers collapsed).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The token regex this bubble's chips match.
    pub fn tokens(&self) -> &Regex {
        &self.tokens
    }

    /// Apply the transcript allocator's current viewport reservation.
    pub fn set_transcript_allocation(&mut self, rows: usize, _frame: Option<AnimationFrame>) {
        self.allocation = rows;
    }

    /// User bubbles are finalized on arrival.
    pub fn is_transcript_block_finalized(&self) -> bool {
        true
    }

    /// Single-row emergency representation for the overflow layout.
    pub fn render_transcript_block_emergency_row(&mut self, width: u16) -> Option<String> {
        let lines = self.render(width);
        select_emergency_row(&lines).cloned()
    }

    /// Run a hover-toolbar or hold-menu act; true when handled.
    pub fn handle_action(&self, act: &str) -> bool {
        use crate::chat::transcript_actions::{
            run_transcript_action, run_turn_action, TranscriptAction, TurnMessage, TurnRole,
        };
        match act {
            "copy-message" => run_transcript_action(TranscriptAction::Copy {
                text: self.text.clone(),
            }),
            "rewind" => run_transcript_action(TranscriptAction::Rewind),
            _ => match self.timestamp {
                Some(at) => run_turn_action(
                    act,
                    TurnMessage {
                        role: TurnRole::User,
                        at,
                    },
                ),
                None => false,
            },
        }
    }

    /// The top padding row: the live-steering marker left-aligned and the
    /// reaction badge right-aligned.
    fn badge_row(&self, width: u16) -> String {
        let marker = if self.live_steered {
            self.theme.fg("accent", "*").unwrap_or_else(|_| "*".into())
        } else {
            String::new()
        };
        let emoji = self.reaction.clone().unwrap_or_default();
        let gap = (width as usize)
            .saturating_sub(2)
            .saturating_sub(visible_width(&marker))
            .saturating_sub(visible_width(&emoji));
        let line = format!(" {marker}{}{emoji}", padding(gap));
        stabilize_background_ansi(&line, &self.bg_ansi)
    }

    fn push_thumb(&self, children: &mut Vec<NativeNode>, _linked: bool, i: usize, image: &crate::prompt::image_source::ImageContent) {
        if !image.mime_type.starts_with("image/") {
            return;
        }
        let label = format!("#{}", i + 1);
        let mut props = Props::new();
        props.set_str("alt", label.clone());
        props.set_str("title", label.clone());
        if let Some(path) = self.image_links.as_ref().and_then(|links| links.get(i)).cloned().flatten() {
            props.set_str("src", path);
        }
        let node = node(NodeKind::Image, props, Vec::new(), Some(&label));
        children.push(node);
    }
}

impl Component for UserMessageComponent {
    fn render(&mut self, width: u16) -> Vec<String> {
        let mut lines = self.markdown.render(width);
        if lines.is_empty() {
            return lines;
        }
        if self.allocation == 1 {
            if let Some(emergency) = select_emergency_row(&lines) {
                return vec![emergency.clone()];
            }
        }
        if self.reaction.is_some() || self.live_steered {
            lines[0] = self.badge_row(width);
        }
        for line in &mut lines {
            *line = stabilize_background_ansi(line, &self.bg_ansi);
        }
        if prompt_zones_enabled() {
            lines[0] = format!("{OSC133_ZONE_START}{}", lines[0]);
            if let Some(last) = lines.last_mut() {
                last.push_str(&osc133_zone_close());
            }
        }
        lines
    }
}

impl NativeDescribe for UserMessageComponent {
    fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
        use crate::chat::transcript_actions::{
            has_transcript_actions, turn_hold_entries, MenuEntry, TurnRole,
        };

        let mut children: Vec<NativeNode> = Vec::new();
        if !self.synthetic && has_transcript_actions() {
            let mut tools = Vec::new();
            let mut copy = Props::new();
            copy.set_str("name", "copy");
            copy.set_str("role", "omp.user.tool");
            copy.set_str("title", "Copy message");
            copy.set_str("aria", "Copy message");
            copy.insert("actions".to_string(), serde_json::json!({"click": "copy-message"}));
            tools.push(node(NodeKind::Icon, copy, Vec::new(), Some("copy")));

            let mut rewind = Props::new();
            rewind.set_str("name", "rewind");
            rewind.set_str("role", "omp.user.tool");
            rewind.set_str("title", "Rewind the conversation to an earlier message");
            rewind.set_str("aria", "Rewind");
            rewind.insert("actions".to_string(), serde_json::json!({"click": "rewind"}));
            tools.push(node(NodeKind::Icon, rewind, Vec::new(), Some("rewind")));

            if let Some(at) = self.timestamp {
                let mut dot = Props::new();
                dot.set_text("·");
                tools.push(node(NodeKind::Text, dot, Vec::new(), Some("dot")));
                let clock = clock_label(at, cx.hour12.unwrap_or(false));
                let mut props = Props::new();
                props.set_str("role", "omp.user.time");
                props.set_str("title", clock.clone());
                tools.push(text(Text::spans(vec![span(clock, Some("dim mono"))]), props));
            }

            let mut props = Props::new();
            props.set_str("gap", "xs");
            props.set_str("align", "center");
            props.set_str("role", "omp.user.tools");
            children.push(node(NodeKind::Row, props, tools, Some("tools")));
        }

        if !self.synthetic {
            let mut thumbs = Vec::new();
            let linked = image_links_enabled();
            let images = self.images.clone();
            for (i, image) in images.iter().enumerate() {
                if !self.synthetic {
                    let mut scratch = Vec::new();
                    self.push_thumb(&mut scratch, linked, i, image);
                    thumbs.extend(scratch);
                }
            }
            if !thumbs.is_empty() {
                let mut props = Props::new();
                props.set_str("gap", "sm");
                props.set_bool("wrap", true);
                props.set_str("align", "start");
                props.set_str("role", "omp.user.images");
                children.push(row(thumbs, props));
            }
        }

        children.push(md(&self.text, Props::new()));

        let mut badges = Vec::new();
        if self.live_steered {
            let mut props = Props::new();
            props.set_text("steered");
            props.set_tone(Tone::Accent);
            props.set_str("title", "Delivered into the response that was streaming");
            badges.push(node(NodeKind::Badge, props, Vec::new(), None));
        }
        if let Some(reaction) = &self.reaction {
            let mut props = Props::new();
            props.set_text(reaction.clone());
            props.set_str("role", "omp.reaction");
            badges.push(node(NodeKind::Badge, props, Vec::new(), None));
        }
        if !badges.is_empty() {
            let mut props = Props::new();
            props.set_str("gap", "xs");
            props.set_str("justify", "end");
            props.set_str("role", "omp.user.badges");
            children.push(node(NodeKind::Row, props, badges, Some("badges")));
        }

        let mut props = Props::new();
        props.set_str("role", if self.synthetic { "omp.user.synthetic" } else { "omp.user" });
        props.set_tone(if self.synthetic { Tone::Muted } else { Tone::User });
        if !self.synthetic && has_transcript_actions() && self.timestamp.is_some() {
            let entries = turn_hold_entries(
                TurnRole::User,
                MenuEntry::new("copy-message", "Copy message"),
            );
            props.insert("actions".to_string(), serde_json::json!({"hold": entries_json(&entries)}));
        }
        card(props, children)
    }
}

fn entries_json(entries: &[crate::chat::transcript_actions::MenuEntry]) -> serde_json::Value {
    serde_json::Value::Array(
        entries
            .iter()
            .map(|entry| {
                let mut object = serde_json::Map::new();
                object.insert("id".into(), entry.id.clone().into());
                object.insert("label".into(), entry.label.clone().into());
                if let Some(icon) = &entry.icon {
                    object.insert("icon".into(), icon.clone().into());
                }
                if let Some(detail) = &entry.detail {
                    object.insert("detail".into(), detail.clone().into());
                }
                serde_json::Value::Object(object)
            })
            .collect(),
    )
}

/// Emergency row: the first non-blank content line.
/// `HH:MM` (or `h:MM AM/PM`) for epoch milliseconds. The reference formats in
/// the host's local timezone; this port has no local-time source in the build,
/// so the clock reads UTC.
pub fn clock_label(at_ms: i64, hour12: bool) -> String {
    let minutes = at_ms.div_euclid(60_000).rem_euclid(24 * 60) as u32;
    let (hour, minute) = (minutes / 60, minutes % 60);
    if hour12 {
        let hour12 = match hour % 12 {
            0 => 12,
            hour => hour,
        };
        let meridiem = if hour < 12 { "AM" } else { "PM" };
        format!("{hour12:02}:{minute:02} {meridiem}")
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

fn select_emergency_row(lines: &[String]) -> Option<&String> {
    if lines.is_empty() {
        return None;
    }
    if lines.len() >= 3 {
        for line in &lines[1..lines.len() - 1] {
            if !crate::ansi_scan::strip_ansi(line).trim().is_empty() {
                return Some(line);
            }
        }
        return lines.get(1);
    }
    lines.first()
}

/// The composer tokens in `text` as `md` marks, styled as the composer
/// decorates them. (The crate's Markdown takes no marks; the spans are
/// returned for hosts that lay them out themselves.)
pub fn token_marks(
    text: &str,
    tokens: Option<&Regex>,
    image_links: Option<&[Option<String>]>,
    theme: Option<&Theme>,
) -> Vec<crate::native::node::Span> {
    struct Marks<'a> {
        marks: RefCell<Vec<crate::native::node::Span>>,
        image_links: Option<&'a [Option<String>]>,
    }
    impl PlaceholderRenderers for Marks<'_> {
        fn render_text(&self, _text: &str) -> String {
            String::new()
        }

        fn render_reference(
            &self,
            label: &str,
            kind: PlaceholderKind,
            index: i64,
            _form: PlaceholderForm,
        ) -> String {
            let link = if kind == PlaceholderKind::Image || kind == PlaceholderKind::Video {
                self.image_links
                    .and_then(|links| links.get((index - 1).max(0) as usize))
                    .cloned()
                    .flatten()
            } else {
                None
            };
            let span = match link {
                Some(path) => crate::render::hyperlink::file_link_span(&path, label, "accent strong"),
                None => span(label, Some("accent strong")),
            };
            self.marks.borrow_mut().push(span);
            String::new()
        }

        fn render_skill(&self, label: &str, _name: &str) -> String {
            self.marks
                .borrow_mut()
                .push(span(label, Some("customMessageLabel strong")));
            String::new()
        }

        fn render_mention(&self, label: &str) -> String {
            self.marks
                .borrow_mut()
                .push(span(label, Some("statusLineModel strong")));
            String::new()
        }
    }

    let marks = Marks {
        marks: RefCell::new(Vec::new()),
        image_links,
    };
    let _ = theme;
    let _ = render_placeholders(text, &marks, tokens);
    marks.marks.take()
}

/// Truncate a plain summary label to `max_width` display columns, appending `…`.
pub fn truncate_summary(text: &str, max_width: usize) -> String {
    if visible_width(text) <= max_width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = visible_width(&ch.to_string());
        if width + ch_width > max_width.saturating_sub(1) {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    format!("{out}…")
}

/// One-line summary for a collapsed synthetic input: `<label> · <size> · <n>
/// lines`.
pub fn summarize_synthetic_input(text: &str, theme: &Theme) -> String {
    let size = format_bytes(text.len() as u64);
    let line_count = if text.is_empty() { 0 } else { text.split('\n').count() };
    let dot = theme.sep().dot.trim().to_string();
    format!(
        "{} {dot} {size} {dot} {line_count} line{}",
        synthetic_input_label(text),
        if line_count == 1 { "" } else { "s" }
    )
}

/// Byte count in the reference's `formatBytes` units.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// First Markdown heading text in `text`, else `Synthetic input`.
pub fn synthetic_input_label(text: &str) -> String {
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let depth = line.chars().take_while(|ch| *ch == '#').count();
        if (1..=6).contains(&depth) {
            let rest = line[depth..].trim_start();
            let rest = rest.trim();
            return if rest.is_empty() {
                "Synthetic input".to_string()
            } else {
                rest.to_string()
            };
        }
        return "Synthetic input".to_string();
    }
    "Synthetic input".to_string()
}

/// Collapsed placeholder for a synthetic (agent-attributed) user input.
pub struct CollapsedSyntheticMessageComponent {
    text: String,
    image_links: Option<Vec<Option<String>>>,
    expanded: bool,
    ignore_tight: bool,
    theme: Theme,
    /// Built lazily on the first expanded render and retained.
    body: Option<UserMessageComponent>,
}

impl CollapsedSyntheticMessageComponent {
    /// A collapsed placeholder for `text`.
    pub fn new(
        text: impl Into<String>,
        image_links: Option<Vec<Option<String>>>,
        theme: &Theme,
    ) -> Self {
        CollapsedSyntheticMessageComponent {
            text: text.into(),
            image_links,
            expanded: false,
            ignore_tight: false,
            theme: theme.clone(),
            body: None,
        }
    }

    /// `ctrl+o` toggle: reveal/hide the full Markdown body.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    /// Whether the body is shown.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// The one-line summary row's text.
    pub fn summary(&self) -> String {
        summarize_synthetic_input(&self.text, &self.theme)
    }

    /// The summary row with the expand hint.
    pub fn summary_line(&self, width: u16) -> String {
        let dot = self.theme.sep().dot.trim().to_string();
        let hint = format!("{dot} {}", expand_key_hint());
        let label = truncate_summary(&format!("{} {hint}", self.summary()), width.max(10) as usize - 1);
        let dim = self.theme.fg("dim", &label).unwrap_or(label);
        format!(" {dim}")
    }

    /// Native toggle handling; `expanded` when the event toggled the root.
    pub fn handle_toggle(&mut self, expanded: Option<bool>) -> bool {
        match expanded {
            Some(expanded) => {
                self.set_expanded(expanded);
                true
            }
            None => false,
        }
    }

    /// Whether the body ignores tight line spacing (kept for API parity).
    pub fn set_ignore_tight(&mut self, ignore: bool) {
        self.ignore_tight = ignore;
    }

    /// Whether tight spacing is ignored.
    pub fn ignore_tight(&self) -> bool {
        self.ignore_tight
    }

    fn body(&mut self) -> &mut UserMessageComponent {
        let theme = self.theme.clone();
        let links = self.image_links.clone();
        self.body.get_or_insert_with(|| {
            UserMessageComponent::new(
                self.text.clone(),
                UserBubbleOptions {
                    synthetic: true,
                    image_links: links,
                    ..UserBubbleOptions::default()
                },
                &theme,
            )
        })
    }
}

impl Component for CollapsedSyntheticMessageComponent {
    fn render(&mut self, width: u16) -> Vec<String> {
        if self.expanded {
            return self.body().render(width);
        }
        vec![self.summary_line(width)]
    }

    fn invalidate(&mut self) {
        if let Some(body) = self.body.as_mut() {
            body.invalidate();
        }
    }
}

impl NativeDescribe for CollapsedSyntheticMessageComponent {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let mut props = Props::new();
        props.set_str("role", "omp.user.synthetic");
        props.set_tone(Tone::Muted);
        props.insert(
            "head".to_string(),
            serde_json::Value::Array(vec![span(self.summary(), Some("dim")).to_value()]),
        );
        props.set_bool("collapsible", true);
        props.set_bool("collapsed", !self.expanded);
        let children = if self.expanded {
            vec![md(&self.text, Props::new())]
        } else {
            Vec::new()
        };
        card(props, children)
    }
}

/// A spacer every bubble leaves under itself (reference `Spacer` usage parity).
pub fn bubble_spacer() -> crate::components::Spacer {
    crate::components::Spacer::new(1)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn zones_toggle_globally() {
        assert!(prompt_zones_enabled());
        set_prompt_zones_enabled(false);
        assert!(!prompt_zones_enabled());
        set_prompt_zones_enabled(true);
        assert!(prompt_zones_enabled());
    }

    #[test]
    fn synthetic_labels_come_from_the_first_heading() {
        let cases = [
            ("# Session update\nbody", "Session update"),
            ("### Deep\n", "Deep"),
            ("plain text", "Synthetic input"),
            ("", "Synthetic input"),
            ("#   \nx", "Synthetic input"),
            ("####### seven", "Synthetic input"),
        ];
        for (text, expected) in cases {
            assert_eq!(synthetic_input_label(text), expected, "{text:?}");
        }
    }

    #[test]
    fn summaries_name_size_and_line_count() {
        let theme = theme();
        assert_eq!(summarize_synthetic_input("", &theme), "Synthetic input · 0 B · 0 lines");
        assert_eq!(summarize_synthetic_input("a", &theme), "Synthetic input · 1 B · 1 line");
        let ten = "x".repeat(1536);
        assert!(summarize_synthetic_input(&ten, &theme).contains("1.5 KiB"));
        assert!(summarize_synthetic_input(&ten, &theme).contains("1 line"));
    }

    #[test]
    fn summaries_truncate_to_the_width() {
        assert_eq!(truncate_summary("abc", 5), "abc");
        assert_eq!(truncate_summary("abcdef", 4), "abc…");
        assert_eq!(truncate_summary("abcdef", 1), "…");
    }

    #[test]
    fn bubbles_hold_their_text_and_actions() {
        let theme = theme();
        let mut message = UserMessageComponent::new(
            "hello",
            UserBubbleOptions {
                timestamp: Some(0),
                ..UserBubbleOptions::default()
            },
            &theme,
        );
        assert_eq!(message.text(), "hello");
        assert!(message.is_transcript_block_finalized());
        assert!(!message.handle_action("retry-turn"));
        message.set_reaction("👍");
        assert_eq!(message.reaction(), Some("👍"));
        message.set_transcript_allocation(0, None);
        assert!(!message.render(20).is_empty());
    }

    #[test]
    fn bubbles_emit_osc133_zones_around_their_rows() {
        set_prompt_zones_enabled(true);
        let theme = theme();
        let mut message = UserMessageComponent::new("hi", UserBubbleOptions::default(), &theme);
        let lines = message.render(20);
        assert!(lines[0].starts_with(OSC133_ZONE_START));
        assert!(lines.last().unwrap().ends_with(&osc133_zone_close()));
        set_prompt_zones_enabled(false);
        let lines = message.render(20);
        assert!(!lines[0].starts_with(OSC133_ZONE_START));
        set_prompt_zones_enabled(true);
    }

    #[test]
    fn emergency_rows_skip_blank_edges() {
        assert!(select_emergency_row(&[]).is_none());
        let one = vec!["only".to_string()];
        assert_eq!(select_emergency_row(&one).map(String::as_str), Some("only"));
        let many = vec![
            "".to_string(),
            "  ".to_string(),
            "   ".to_string(),
            "body".to_string(),
            "".to_string(),
        ];
        assert_eq!(select_emergency_row(&many).map(String::as_str), Some("body"));
    }

    #[test]
    fn collapses_synthetic_inputs_until_expanded() {
        let theme = theme();
        let mut collapsed = CollapsedSyntheticMessageComponent::new(
            "# Session update\nbody",
            None,
            &theme,
        );
        assert!(!collapsed.is_expanded());
        assert_eq!(collapsed.render(40).len(), 1);
        assert!(collapsed.handle_toggle(Some(true)));
        assert!(collapsed.is_expanded());
        assert!(!collapsed.render(40).is_empty());
        let node = collapsed.describe_native(&DescribeContext::new(40));
        assert_eq!(node.props.role(), Some("omp.user.synthetic"));
        assert_eq!(node.props.get("collapsible").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(node.props.get("collapsed").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn native_bubbles_carry_roles_images_and_badges() {
        let theme = theme();
        let message = UserMessageComponent::new(
            "hi",
            UserBubbleOptions {
                live_steered: true,
                timestamp: Some(0),
                ..UserBubbleOptions::default()
            },
            &theme,
        );
        let node = message.describe_native(&DescribeContext::new(80));
        assert_eq!(node.props.role(), Some("omp.user"));
        assert_eq!(node.props.tone(), Some(Tone::User));
        let role_of = |name: &str| {
            node.children
                .iter()
                .find(|child| child.props.role() == Some(name))
                .is_some()
        };
        assert!(role_of("omp.user.badges"));
        let synthetic = UserMessageComponent::new(
            "hi",
            UserBubbleOptions {
                synthetic: true,
                ..UserBubbleOptions::default()
            },
            &theme,
        );
        let node = synthetic.describe_native(&DescribeContext::new(80));
        assert_eq!(node.props.role(), Some("omp.user.synthetic"));
        assert_eq!(node.props.tone(), Some(Tone::Muted));
    }

    #[test]
    fn token_marks_collect_chip_spans_once() {
        let marks = token_marks("no tokens here", None, None, None);
        assert!(marks.is_empty());
    }
}
