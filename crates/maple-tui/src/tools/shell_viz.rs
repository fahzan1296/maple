//! Shell visualization: the native presentation decoder and renderer.
//!
//! Ported from `l/src/tools/shell-viz.ts` and `l/src/tools/shell-viz-render.ts`.
//! The engine's presentation JSON is semantic display data (rows of styled
//! segments, effects, watches); it is validated on decode and rendered with the
//! active [`Theme`]. The native `RunTrace` owner is not available in this crate,
//! so [`decode_shell_presentation`] is the entry point for restored traces.

use serde::de::{self, Deserializer};
use serde::Deserialize;

use crate::render::render_utils::replace_tabs;
use crate::text_shape::{truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};
use crate::theme::theme_class::Theme;
use crate::theme::symbols::SymbolPreset;

/// Node keys are opaque `depth:seq:executionSeq` identities.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShellVizNode(String);

impl ShellVizNode {
    /// The raw key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ShellVizNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        if is_shell_viz_node(&text) {
            Ok(ShellVizNode(text))
        } else {
            Err(de::Error::custom("invalid shell visualization node key"))
        }
    }
}

/// Whether `value` is a `depth:seq:executionSeq` node key.
pub fn is_shell_viz_node(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|part| is_canonical_number(part))
}

fn is_canonical_number(text: &str) -> bool {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    text == "0" || !text.starts_with('0')
}

/// A `[start, end]` token range; `start >= 0` and `end >= start`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellVizRange {
    /// First token offset.
    pub start: f64,
    /// Last token offset.
    pub end: f64,
}

impl<'de> Deserialize<'de> for ShellVizRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (start, end) = <(f64, f64)>::deserialize(deserializer)?;
        if start.is_finite() && end.is_finite() && start >= 0.0 && end >= start {
            Ok(ShellVizRange { start, end })
        } else {
            Err(de::Error::custom("invalid shell visualization range"))
        }
    }
}

/// Per-segment/row execution state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellVizState {
    /// Not started.
    Pending,
    /// Currently running.
    Running,
    /// Finished successfully.
    Ok,
    /// Finished with a failure.
    Fail,
    /// Skipped by the executor.
    Skipped,
}

/// Where a row sits in its statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellVizRole {
    /// A full statement.
    Stmt,
    /// First line of a multi-line statement.
    Head,
    /// Continuation line.
    Mid,
    /// Last line of a multi-line statement.
    Tail,
    /// Unparsed source.
    Raw,
    /// A whole document.
    Document,
}

/// Effect categories the engine annotates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellVizEffectKind {
    /// Network access.
    Network,
    /// Filesystem write.
    Write,
    /// Destructive command.
    Destructive,
    /// Secret material.
    Secret,
    /// Privilege escalation.
    Privilege,
    /// Background execution.
    Background,
}

/// An annotated effect chip.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ShellVizEffect {
    /// Effect category.
    pub kind: ShellVizEffectKind,
    /// Effect glyph.
    pub glyph: String,
    /// Short label.
    pub label: String,
    /// Explanation.
    pub why: String,
}

/// A watch annotation (`name = value`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellVizWatch {
    /// Variable name.
    pub name: String,
    /// Last observed value.
    pub value: String,
    /// Full value length when truncated.
    #[serde(default)]
    pub full_length: Option<f64>,
    /// Scope label.
    #[serde(default)]
    pub scope: Option<String>,
    /// Scope identity.
    #[serde(default)]
    pub scope_id: Option<String>,
    /// Node the watch belongs to.
    #[serde(default)]
    pub node: Option<ShellVizNode>,
    /// Token range.
    #[serde(default)]
    pub range: Option<ShellVizRange>,
    /// Setter sequence.
    #[serde(default)]
    pub setter_seq: Option<f64>,
}

/// One styled span of a row.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellVizSegment {
    /// Segment text.
    pub text: String,
    /// Whether the segment is currently active.
    #[serde(default)]
    pub active: Option<bool>,
    /// Segment execution state.
    #[serde(default)]
    pub state: Option<ShellVizState>,
    /// Execution timestamp in nanoseconds.
    #[serde(default)]
    pub executed_at_ns: Option<f64>,
    /// Execution sequence.
    #[serde(default)]
    pub execution_seq: Option<f64>,
    /// Space-separated style tokens.
    #[serde(default)]
    pub style: Option<String>,
    /// Node identity.
    #[serde(default)]
    pub node: Option<ShellVizNode>,
    /// Node that executed the segment.
    #[serde(default)]
    pub execution_node: Option<ShellVizNode>,
    /// Token range.
    #[serde(default)]
    pub range: Option<ShellVizRange>,
    /// Tooltip.
    #[serde(default)]
    pub tip: Option<String>,
    /// Expanded form.
    #[serde(default)]
    pub expanded: Option<String>,
    /// Language hint.
    #[serde(default)]
    pub language: Option<String>,
}

/// One displayed row of a presentation.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellVizRow {
    /// Row identity.
    pub id: String,
    /// Nesting depth.
    pub depth: f64,
    /// Node identity.
    #[serde(default)]
    pub node: Option<ShellVizNode>,
    /// Token range.
    #[serde(default)]
    pub range: Option<ShellVizRange>,
    /// Whether the row is active.
    #[serde(default)]
    pub active: Option<bool>,
    /// Row execution state.
    #[serde(default)]
    pub state: Option<ShellVizState>,
    /// Row role.
    #[serde(default)]
    pub role: Option<ShellVizRole>,
    /// Row segments.
    pub segments: Vec<ShellVizSegment>,
    /// Row effects.
    #[serde(default)]
    pub effects: Vec<ShellVizEffect>,
    /// Execution timestamp in nanoseconds.
    #[serde(default)]
    pub executed_at_ns: Option<f64>,
    /// Execution sequence.
    #[serde(default)]
    pub execution_seq: Option<f64>,
    /// Trailing metadata.
    #[serde(default)]
    pub meta: Option<String>,
    /// Detail line.
    #[serde(default)]
    pub detail: Option<String>,
    /// Watch annotations.
    #[serde(default)]
    pub annotations: Vec<ShellVizWatch>,
}

/// Which rendering the presentation uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellVizView {
    /// Raw source rows.
    Raw,
    /// Pseudo-code rows.
    Pseudo,
}

/// Toggleable render layers.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ShellVizLayers {
    /// Syntax colour.
    pub color: bool,
    /// Inline idioms.
    pub inline: bool,
    /// Idiom chips.
    pub idioms: bool,
    /// Effect chips.
    pub effects: bool,
    /// Unwrap chains.
    pub unwrap: bool,
}

/// Presentation preferences.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellVizPreferences {
    /// Raw or pseudo rows.
    pub view: ShellVizView,
    /// Enabled layers.
    pub layers: ShellVizLayers,
    /// Split original/pseudo panes.
    pub split: bool,
    /// Replay playing.
    pub playing: bool,
    /// Replay speed.
    pub speed: f64,
    /// Replay cursor.
    #[serde(default)]
    pub cursor: Option<f64>,
    /// Selected node.
    #[serde(default)]
    pub selected_node: Option<ShellVizNode>,
    /// Expanded nodes.
    #[serde(default)]
    pub expanded_nodes: Vec<ShellVizNode>,
}

/// Replay timeline metadata.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellVizTimeline {
    /// Retained frame count.
    pub length: f64,
    /// Cursor frame.
    pub cursor: f64,
    /// Elapsed nanoseconds.
    pub elapsed_ns: f64,
    /// Total duration in nanoseconds.
    pub duration_ns: f64,
    /// Whether history was trimmed.
    #[serde(default)]
    pub trimmed: Option<bool>,
}

/// The decoded presentation.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ShellVizPresentation {
    /// Replay timeline.
    pub timeline: ShellVizTimeline,
    /// Preferences the presentation was built with.
    pub preferences: ShellVizPreferences,
    /// Rendered rows.
    pub rows: Vec<ShellVizRow>,
    /// Original source rows.
    #[serde(default)]
    pub original: Vec<ShellVizRow>,
    /// Command output.
    pub output: String,
    /// Whether the run is live.
    #[serde(default)]
    pub running: Option<bool>,
    /// Whether a statement is executing.
    pub executing: bool,
    /// Watch annotations.
    #[serde(default)]
    pub watches: Vec<ShellVizWatch>,
    /// Notice line.
    #[serde(default)]
    pub message: Option<String>,
}

/// Python-like Pseudo, all layers enabled, one pane, live, paused, normal speed.
pub fn default_shell_viz_preferences() -> ShellVizPreferences {
    ShellVizPreferences {
        view: ShellVizView::Pseudo,
        layers: ShellVizLayers {
            color: true,
            inline: true,
            idioms: true,
            effects: true,
            unwrap: true,
        },
        split: false,
        playing: false,
        speed: 1.0,
        cursor: None,
        selected_node: None,
        expanded_nodes: Vec::new(),
    }
}

/// Decode semantic display data, never shell syntax or execution state.
pub fn decode_shell_presentation(json: &str) -> Result<ShellVizPresentation, String> {
    serde_json::from_str(json).map_err(|error| error.to_string())
}

/// Nerd Font glyphs replacing the whole `🔇 out|err|silent` /dev/null badge.
const DISCARD_ICONS: [(&str, &str); 3] = [
    ("badge-discard-out", "\u{f164d}"),
    ("badge-discard-err", "\u{f14bc}"),
    ("badge-discard-all", "\u{f0581}"),
];

/// Strip untrusted control characters from shell source, values and outputs.
pub fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\t' | '\n' | '\r'))
        .collect()
}

/// Sanitize untrusted shell source, values and outputs before styling, and
/// flatten line breaks so a row stays one line.
pub fn shell_viz_text(text: &str) -> String {
    replace_tabs(&sanitize_text(text)).replace(['\r', '\n'], " ")
}

fn theme_fg(theme: &Theme, color: &str, text: &str) -> String {
    theme.fg(color, text).unwrap_or_else(|_| text.to_string())
}

fn style_color(style: &str) -> &'static str {
    let tokens: Vec<&str> = style.split(' ').collect();
    if tokens.contains(&"comment") {
        "syntaxComment"
    } else if tokens.contains(&"kw") {
        "syntaxKeyword"
    } else if tokens.contains(&"var") || tokens.contains(&"secret") {
        "syntaxVariable"
    } else if tokens.contains(&"str") || tokens.contains(&"path") {
        "syntaxString"
    } else if tokens.contains(&"num") {
        "syntaxNumber"
    } else if tokens.contains(&"op") {
        "syntaxOperator"
    } else if tokens.contains(&"cmd") || tokens.contains(&"fn") {
        "syntaxFunction"
    } else if tokens.contains(&"chip") || tokens.contains(&"badge") {
        "accent"
    } else {
        "text"
    }
}

fn segment_text(
    segment: &ShellVizSegment,
    row: &ShellVizRow,
    presentation: &ShellVizPresentation,
    theme: &Theme,
) -> String {
    let tokens: Vec<&str> = segment
        .style
        .as_deref()
        .unwrap_or("")
        .split(' ')
        .collect();
    let discard = if theme.get_symbol_preset() == SymbolPreset::Nerd {
        tokens
            .iter()
            .find(|token| DISCARD_ICONS.iter().any(|(key, _)| key == *token))
            .and_then(|token| {
                DISCARD_ICONS
                    .iter()
                    .find(|(key, _)| key == token)
                    .map(|(_, glyph)| format!(" {glyph}"))
            })
    } else {
        None
    };
    let text = match discard {
        Some(discard) => discard,
        None => shell_viz_text(&segment.text),
    };
    let inactive = (presentation.running.unwrap_or(false) && !(segment.active.unwrap_or(row.active.unwrap_or(false))))
        || segment.state.unwrap_or(row.state.unwrap_or(ShellVizState::Pending)) == ShellVizState::Skipped;
    let semantic = tokens
        .iter()
        .any(|token| matches!(*token, "chip" | "badge" | "secret"));
    let styled = if presentation.preferences.layers.color || semantic {
        theme_fg(theme, style_color(segment.style.as_deref().unwrap_or("")), &text)
    } else {
        text
    };
    if inactive {
        theme.faint(&styled)
    } else if segment.active.unwrap_or(false) && presentation.running.unwrap_or(false) {
        theme.bold(&styled)
    } else {
        styled
    }
}

/// A row split into its gutter/indent/effects lead and the wrappable body.
fn row_parts(
    row: &ShellVizRow,
    presentation: &ShellVizPresentation,
    theme: &Theme,
    width: usize,
) -> (String, String) {
    let selected = presentation.preferences.selected_node.as_ref().is_some_and(|selected| {
        row.node.as_ref() == Some(selected)
            || row
                .segments
                .iter()
                .any(|segment| segment.node.as_ref() == Some(selected))
    });
    let mut failures: Vec<String> = Vec::new();
    let row_failed_in_segments = row
        .segments
        .iter()
        .any(|segment| segment.state == Some(ShellVizState::Fail));
    let muted = presentation.running.unwrap_or(false) && !row.active.unwrap_or(false);
    let failed_statement = row.state == Some(ShellVizState::Fail)
        && !row_failed_in_segments
        && row.role != Some(ShellVizRole::Head)
        && row.role != Some(ShellVizRole::Mid)
        && row.role != Some(ShellVizRole::Tail);
    let gutter = if failed_statement {
        theme_fg(theme, "error", "×")
    } else if presentation.running.unwrap_or(false)
        && row.segments.iter().any(|segment| segment.active == Some(true))
    {
        theme_fg(theme, "accent", "▶")
    } else if selected {
        theme_fg(theme, "accent", "›")
    } else {
        " ".to_string()
    };
    let effects = if presentation.preferences.layers.effects {
        let joined: String = row
            .effects
            .iter()
            .map(|effect| shell_viz_text(&effect.glyph))
            .collect();
        if muted {
            theme.faint(&joined)
        } else {
            joined
        }
    } else {
        String::new()
    };
    let indent = "  ".repeat((row.depth as usize).min(width / 4));
    let mut body = String::new();
    for segment in &row.segments {
        let execution = segment
            .execution_node
            .as_ref()
            .or(segment.node.as_ref())
            .or(row.node.as_ref());
        let failed = segment.state == Some(ShellVizState::Fail)
            && execution.is_some()
            && !failures
                .iter()
                .any(|seen| Some(seen.as_str()) == execution.map(|node| node.as_str()));
        if failed {
            if let Some(node) = execution {
                failures.push(node.as_str().to_string());
            }
        }
        let mark = if failed {
            theme_fg(theme, "error", "× ")
        } else {
            String::new()
        };
        let mark = if muted { theme.faint(&mark) } else { mark };
        body.push_str(&mark);
        body.push_str(&segment_text(segment, row, presentation, theme));
    }
    if let Some(meta) = &row.meta {
        let color = if row.state == Some(ShellVizState::Fail) {
            "error"
        } else {
            "dim"
        };
        body.push_str("  ");
        body.push_str(&theme_fg(theme, color, &shell_viz_text(meta)));
    }
    if !row.annotations.is_empty() {
        let annotations: Vec<String> = row
            .annotations
            .iter()
            .map(|variable| {
                let shortened = variable
                    .full_length
                    .is_some_and(|full| full > variable.value.len() as f64);
                let name = theme_fg(theme, "syntaxVariable", &shell_viz_text(&variable.name));
                let value = format!(
                    "\"{}\"{}",
                    escape_json_string(&shell_viz_text(&variable.value)),
                    if shortened { "…" } else { "" }
                );
                let annotation = format!(
                    "{name} {} {value}",
                    theme_fg(theme, "muted", "=")
                );
                if muted {
                    theme.faint(&annotation)
                } else {
                    annotation
                }
            })
            .collect();
        body.push_str("  ");
        body.push_str(&theme_fg(theme, "muted", "‹"));
        body.push(' ');
        body.push_str(&annotations.join(&theme_fg(theme, "muted", " · ")));
        body.push(' ');
        body.push_str(&theme_fg(theme, "muted", "›"));
    }
    let lead = format!(
        "{gutter} {indent}{}{effects}",
        if effects.is_empty() { "" } else { " " }
    );
    (lead, body)
}

fn escape_json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn row_lines(
    row: &ShellVizRow,
    presentation: &ShellVizPresentation,
    theme: &Theme,
    width: usize,
) -> Vec<String> {
    let (lead, body) = row_parts(row, presentation, theme, width);
    let lead_width = visible_width(&lead);
    if lead_width >= width {
        return wrap_text_with_ansi(&format!("{lead}{body}"), width.max(1));
    }
    let hanging = " ".repeat(lead_width);
    wrap_text_with_ansi(&body, width - lead_width)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{lead}{line}")
            } else {
                format!("{hanging}{line}")
            }
        })
        .collect()
}

fn join_panes(left: &[String], right: &[String], width: usize, separator: &str) -> Vec<String> {
    let pane = ((width.saturating_sub(visible_width(separator))) / 2).max(1);
    (0..left.len().max(right.len()))
        .map(|index| {
            let first = truncate_to_width(left.get(index).map(String::as_str).unwrap_or(""), pane, Ellipsis::Omit);
            let padding = pane - visible_width(&first);
            let joined = format!(
                "{first}{}{separator}{}",
                " ".repeat(padding),
                right.get(index).map(String::as_str).unwrap_or("")
            );
            truncate_to_width(&joined, width, Ellipsis::Omit)
        })
        .collect()
}

/// Render the canonical presentation; transcript cards and the focused
/// inspector share this path. Every row is clamped to `width`.
pub fn render_shell_visualization(
    presentation: &ShellVizPresentation,
    theme: &Theme,
    width: u16,
    include_output: bool,
) -> Vec<String> {
    let width = (width.max(1)) as usize;
    let mut lines: Vec<String> = Vec::new();
    if let Some(message) = &presentation.message {
        lines.push(theme_fg(theme, "dim", &shell_viz_text(message)));
    }
    if presentation.preferences.split && presentation.preferences.view != ShellVizView::Raw {
        if width >= 90 {
            let pane = (width - 3) / 2;
            let left = std::iter::once(theme_fg(theme, "dim", "Original"))
                .chain(
                    presentation
                        .original
                        .iter()
                        .flat_map(|row| row_lines(row, presentation, theme, pane)),
                )
                .collect::<Vec<_>>();
            let right = std::iter::once(theme_fg(theme, "dim", "Pseudo"))
                .chain(
                    presentation
                        .rows
                        .iter()
                        .flat_map(|row| row_lines(row, presentation, theme, pane)),
                )
                .collect::<Vec<_>>();
            lines.extend(join_panes(&left, &right, width, " │ "));
        } else {
            lines.push(theme_fg(
                theme,
                "dim",
                "Split stacked (terminal too narrow for side-by-side)",
            ));
            lines.push(theme_fg(theme, "dim", "Original"));
            lines.extend(
                presentation
                    .original
                    .iter()
                    .flat_map(|row| row_lines(row, presentation, theme, width)),
            );
            lines.push(theme_fg(theme, "dim", "Pseudo"));
            lines.extend(
                presentation
                    .rows
                    .iter()
                    .flat_map(|row| row_lines(row, presentation, theme, width)),
            );
        }
    } else {
        lines.extend(
            presentation
                .rows
                .iter()
                .flat_map(|row| row_lines(row, presentation, theme, width)),
        );
    }
    if include_output && !presentation.output.is_empty() {
        lines.push(theme_fg(theme, "dim", "Output"));
        let trimmed = presentation.output.trim_end();
        for line in trimmed.split('\n') {
            lines.extend(wrap_text_with_ansi(&shell_viz_text(line), width));
        }
    }
    if presentation.timeline.trimmed == Some(true) {
        lines.push(theme_fg(
            theme,
            "warning",
            "Replay history/output trimmed to retained limits",
        ));
    }
    lines
        .into_iter()
        .map(|line| truncate_to_width(&line, width, Ellipsis::Omit))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_keys_are_validated() {
        assert!(is_shell_viz_node("0:0:0"));
        assert!(is_shell_viz_node("1:2:3"));
        assert!(!is_shell_viz_node("01:2:3"));
        assert!(!is_shell_viz_node("1:2"));
        assert!(!is_shell_viz_node("a:b:c"));
        assert!(!is_shell_viz_node(""));
    }

    #[test]
    fn decode_accepts_a_presentation() {
        let json = r#"{
            "timeline": {"length": 1, "cursor": 0, "elapsedNs": 5, "durationNs": 9},
            "preferences": {"view": "pseudo", "layers": {"color": true, "inline": true, "idioms": true, "effects": true, "unwrap": true}, "split": false, "playing": false, "speed": 1, "expandedNodes": []},
            "rows": [{"id": "r1", "depth": 0, "segments": [{"text": "ls -la"}], "effects": []}],
            "original": [],
            "output": "ok\n",
            "executing": false,
            "watches": []
        }"#;
        let presentation = decode_shell_presentation(json).unwrap();
        assert_eq!(presentation.rows.len(), 1);
        assert_eq!(presentation.rows[0].segments[0].text, "ls -la");
        assert_eq!(presentation.preferences.view, ShellVizView::Pseudo);
    }

    #[test]
    fn decode_rejects_bad_nodes_and_states() {
        let base = r#"{
            "timeline": {"length": 1, "cursor": 0, "elapsedNs": 5, "durationNs": 9},
            "preferences": {"view": "pseudo", "layers": {"color": true, "inline": true, "idioms": true, "effects": true, "unwrap": true}, "split": false, "playing": false, "speed": 1, "expandedNodes": []},
            "rows": [{"id": "r1", "depth": 0, "segments": [{"text": "x", "node": %NODE%}], "effects": []}],
            "original": [], "output": "", "executing": false, "watches": []
        }"#;
        let bad = base.replace("%NODE%", "\"nope\"");
        assert!(decode_shell_presentation(&bad).is_err());
        let good = base.replace("%NODE%", "\"1:2:3\"");
        assert!(decode_shell_presentation(&good).is_ok());
        let bad_state = base.replace("%NODE%", "\"1:2:3\"").replace("\"x\"", "\"x\", \"state\": \"weird\"");
        assert!(decode_shell_presentation(&bad_state).is_err());
    }

    #[test]
    fn decode_rejects_invalid_ranges() {
        let json = r#"{
            "timeline": {"length": 1, "cursor": 0, "elapsedNs": 5, "durationNs": 9},
            "preferences": {"view": "pseudo", "layers": {"color": true, "inline": true, "idioms": true, "effects": true, "unwrap": true}, "split": false, "playing": false, "speed": 1, "expandedNodes": []},
            "rows": [{"id": "r1", "depth": 0, "segments": [], "effects": [], "range": [4, 2]}],
            "original": [], "output": "", "executing": false, "watches": []
        }"#;
        assert!(decode_shell_presentation(json).is_err());
    }

    #[test]
    fn default_preferences_enable_every_layer() {
        let preferences = default_shell_viz_preferences();
        assert_eq!(preferences.view, ShellVizView::Pseudo);
        assert!(preferences.layers.color && preferences.layers.effects && preferences.layers.unwrap);
        assert!(!preferences.split && !preferences.playing);
        assert_eq!(preferences.speed, 1.0);
    }

    #[test]
    fn text_is_sanitized_and_flattened() {
        assert_eq!(shell_viz_text("a\tb\nc\rd"), "a   b c d");
        assert_eq!(shell_viz_text("a\u{7}b"), "ab");
    }

    #[test]
    fn render_clamps_rows_and_includes_output() {
        let json = r#"{
            "timeline": {"length": 1, "cursor": 0, "elapsedNs": 5, "durationNs": 9},
            "preferences": {"view": "raw", "layers": {"color": true, "inline": true, "idioms": true, "effects": true, "unwrap": true}, "split": false, "playing": false, "speed": 1, "expandedNodes": []},
            "rows": [{"id": "r1", "depth": 0, "segments": [{"text": "echo hello", "style": "cmd"}], "effects": []}],
            "original": [], "output": "hello\n", "executing": false, "watches": []
        }"#;
        let presentation = decode_shell_presentation(json).unwrap();
        let theme = test_theme();
        let rows = render_shell_visualization(&presentation, &theme, 40, true);
        assert!(rows.iter().any(|row| row.contains("Output")));
        assert!(rows.iter().any(|row| row.contains("hello")));
        assert!(rows.iter().all(|row| visible_width(row) <= 40));
        let without_output = render_shell_visualization(&presentation, &theme, 40, false);
        assert!(!without_output.iter().any(|row| row.contains("Output")));
    }

    #[test]
    fn render_narrow_width_stays_bounded() {
        let json = r#"{
            "timeline": {"length": 1, "cursor": 0, "elapsedNs": 5, "durationNs": 9},
            "preferences": {"view": "raw", "layers": {"color": true, "inline": true, "idioms": true, "effects": true, "unwrap": true}, "split": false, "playing": false, "speed": 1, "expandedNodes": []},
            "rows": [{"id": "r1", "depth": 3, "segments": [{"text": "a very long command line that must wrap"}], "effects": []}],
            "original": [], "output": "", "executing": false, "watches": []
        }"#;
        let presentation = decode_shell_presentation(json).unwrap();
        let theme = test_theme();
        for width in [1u16, 4, 8, 12, 20] {
            for row in render_shell_visualization(&presentation, &theme, width, true) {
                assert!(visible_width(&row) <= width as usize, "width {width}: {row:?}");
            }
        }
    }

    fn test_theme() -> Theme {
        crate::theme::create_theme(
            crate::theme::builtin::builtin_theme_json("dark").unwrap(),
            crate::theme::CreateThemeOptions::default(),
        )
        .unwrap()
    }
}
