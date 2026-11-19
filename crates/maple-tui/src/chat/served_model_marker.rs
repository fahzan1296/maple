//! The `served by another model` marker: a slim divider after the first
//! assistant turn whose response provably came from a different model than was
//! requested.
//!
//! Reference: `l/src/chat/served-model-marker.ts`.
//!
//! Deviation: the reference compares ids through `classifyModel` from
//! `@oh-my-pi/pi-catalog/identity`, a KDL-taxonomy classifier this crate does
//! not have. [`classify_model`] reproduces the part the marker needs: a family
//! token set and a numeric revision, `None` for an id whose first token names no
//! known model family (an internal codename, which must stay unverifiable
//! rather than reading as a mismatch). Snapshot suffixes and gateway prefixes
//! therefore compare as the same model.

use crate::chrome::MessageDivider;
use crate::component::Component;
use crate::native::describe::{node, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text};
use crate::style::{Color, Style};
use crate::theme::Theme;

use crate::chat::messages::AssistantMessage;

/// A turn whose response provably came from a different model than was
/// requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServedModelMismatch {
    /// Model id the request asked for.
    pub requested: String,
    /// Model id recovered from the response.
    pub served: String,
    /// Configured provider the request went to.
    pub provider: String,
    /// Upstream host the provider routed to, when it reported one.
    pub upstream_provider: Option<String>,
}

impl ServedModelMismatch {
    /// `provider/upstream`, or just `provider`.
    pub fn via(&self) -> String {
        match &self.upstream_provider {
            Some(upstream) => format!("{}/{}", self.provider, upstream),
            None => self.provider.clone(),
        }
    }

    /// The divider's label in the reference's wording.
    pub fn label(&self, theme: &Theme) -> String {
        let separators = theme.sep();
        let dot = separators.dot.trim();
        format!(
            "{} served {} {} requested {} {} via {}",
            theme.status().warning,
            self.served,
            dot,
            self.requested,
            dot,
            self.via()
        )
    }

    /// The native label (no glyphs, sentence case).
    pub fn native_label(&self) -> String {
        format!(
            "Served {} · requested {} · via {}",
            self.served,
            self.requested,
            self.via()
        )
    }
}

/// A model id reduced to what identifies its weights.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelIdentity {
    /// Lower-case alphabetic tokens of the id, sorted and deduplicated.
    pub family: Vec<String>,
    /// Numeric revision components, in order.
    pub revision: Vec<u64>,
}

/// Model-family prefixes the reference's taxonomy recognises.
const MODEL_FAMILY_PREFIXES: [&str; 19] = [
    "claude", "gemini", "gpt", "grok", "glm", "qwen", "deepseek", "kimi", "mimo", "doubao", "ernie",
    "gpt-oss", "gemma", "minimax", "step", "command", "jamba", "llama", "o",
];

/// Classify `model_id`, or `None` when no model family can be named.
pub fn classify_model(model_id: &str) -> Option<ModelIdentity> {
    let bare = bare_model_id(model_id);
    let normalized = strip_bracketed_affixes(&bare);
    let lowered = normalized.trim().to_lowercase();
    if lowered.is_empty() {
        return None;
    }
    let mut family = Vec::new();
    let mut revision = Vec::new();
    let mut known = false;
    for (index, token) in lowered.split(|ch: char| !ch.is_ascii_alphanumeric()).filter(|token| !token.is_empty()).enumerate() {
        if token.chars().all(|ch| ch.is_ascii_digit()) {
            revision.push(token.parse::<u64>().unwrap_or(0));
            continue;
        }
        if index == 0 && !is_model_family(token) {
            return None;
        }
        known = true;
        if !family.iter().any(|existing| existing == token) {
            family.push(token.to_string());
        }
    }
    if !known {
        return None;
    }
    family.sort();
    Some(ModelIdentity { family, revision })
}

/// Whether `token` starts a known model family (`o1`, `o3`, `o4` count).
fn is_model_family(token: &str) -> bool {
    MODEL_FAMILY_PREFIXES.iter().any(|prefix| {
        token.starts_with(prefix)
            && (*prefix != "o" || token.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit()))
    })
}

/// `openai/gpt-5.4` → `gpt-5.4`.
pub fn bare_model_id(model_id: &str) -> String {
    match model_id.rfind('/') {
        Some(index) => model_id[index + 1..].to_string(),
        None => model_id.to_string(),
    }
}

/// Drop `[Kiro]`-style wrapper tags around an upstream id.
pub fn strip_bracketed_affixes(model_id: &str) -> String {
    let mut text = model_id.trim();
    loop {
        let trimmed = strip_leading_bracket(text);
        let trimmed = strip_trailing_bracket(trimmed).trim();
        if trimmed.len() == text.len() {
            return trimmed.to_string();
        }
        text = trimmed;
    }
}

fn strip_leading_bracket(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !(trimmed.starts_with('[') || trimmed.starts_with('【')) {
        return trimmed;
    }
    let close = trimmed.find([']', '】']);
    match close {
        Some(index) => trimmed[index + ']'.len_utf8()..].trim_start(),
        None => trimmed,
    }
}

fn strip_trailing_bracket(text: &str) -> &str {
    let trimmed = text.trim_end();
    if !(trimmed.ends_with(']') || trimmed.ends_with('】')) {
        return trimmed;
    }
    let open = trimmed.rfind(['[', '【']);
    match open {
        Some(index) => trimmed[..index].trim_end(),
        None => trimmed,
    }
}

/// Whether two ids name the same weights.
fn same_model(left: &ModelIdentity, right: &ModelIdentity) -> bool {
    if left.family != right.family {
        return false;
    }
    let shared = left.revision.len().min(right.revision.len());
    left.revision[..shared] == right.revision[..shared]
}

/// Decide whether `message` was served by a model other than the one requested.
pub fn detect_served_model_mismatch(
    message: &AssistantMessage,
) -> Option<ServedModelMismatch> {
    let served = message.upstream_model.as_deref()?;
    if served == message.model {
        return None;
    }
    let requested = classify_model(&message.model)?;
    let actual = classify_model(served)?;
    if same_model(&requested, &actual) {
        return None;
    }
    Some(ServedModelMismatch {
        requested: message.model.clone(),
        served: served.to_string(),
        provider: message.provider.clone(),
        upstream_provider: message.upstream_provider.clone(),
    })
}

/// Per-transcript memory that surfaces each distinct (requested → served)
/// substitution once. Replaced wholesale on rebuild, so replaying history
/// re-derives the same first occurrence.
#[derive(Default)]
pub struct ServedModelTracker {
    announced: std::collections::HashSet<String>,
}

impl ServedModelTracker {
    /// An empty tracker.
    pub fn new() -> Self {
        ServedModelTracker::default()
    }

    /// The mismatch to render for `message`, or `None` when there is none or it
    /// was already shown.
    pub fn check(&mut self, message: &AssistantMessage) -> Option<ServedModelMismatch> {
        let mismatch = detect_served_model_mismatch(message)?;
        let key = format!(
            "{}\0{}\0{}",
            mismatch.provider, mismatch.requested, mismatch.served
        );
        if !self.announced.insert(key) {
            return None;
        }
        Some(mismatch)
    }
}

/// The slim divider rendered after a substituted turn.
pub struct ServedModelMarker {
    divider: MessageDivider,
    mismatch: ServedModelMismatch,
}

impl ServedModelMarker {
    /// A marker for `info`.
    pub fn new(info: ServedModelMismatch, theme: &Theme) -> Self {
        let mut divider = MessageDivider::new(info.label(theme));
        divider.set_label_style(Style::fg(Color::Yellow));
        divider.set_rule_style(Style::fg(Color::DarkGray));
        divider.set_rule_width(10);
        ServedModelMarker {
            divider,
            mismatch: info,
        }
    }

    /// The mismatch this marker reports.
    pub fn mismatch(&self) -> &ServedModelMismatch {
        &self.mismatch
    }
}

impl Component for ServedModelMarker {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.divider.render(width)
    }

    fn invalidate(&mut self) {
        self.divider.invalidate();
    }
}

impl NativeDescribe for ServedModelMarker {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let mut props = Props::new();
        props.set_str("gap", "sm");
        props.set_str("align", "start");
        props.set_str("role", "omp.marker.served-model");
        node(
            NodeKind::Row,
            props,
            vec![
                node(
                    NodeKind::Icon,
                    {
                        let mut icon = Props::new();
                        icon.set_str("name", "warn");
                        icon
                    },
                    Vec::new(),
                    Some("icon"),
                ),
                text(Text::plain(self.mismatch.native_label()), Props::new()),
            ],
            Some("served-model"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::messages::StopReason;
    use crate::theme::builtin;
    use crate::theme::color::ColorMode;
    use crate::theme::{create_theme, CreateThemeOptions};

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

    fn message(model: &str, upstream: Option<&str>) -> AssistantMessage {
        AssistantMessage {
            model: model.to_string(),
            upstream_model: upstream.map(str::to_string),
            provider: "openrouter".into(),
            stop_reason: StopReason::Stop,
            ..AssistantMessage::default()
        }
    }

    #[test]
    fn snapshot_suffixes_and_gateway_prefixes_are_the_same_model() {
        let cases = [
            ("claude-opus-4-5", "claude-opus-4-5-20251001"),
            ("gpt-5.4", "openai/gpt-5.4"),
            ("[Kiro] claude-opus-4-8", "claude-opus-4-8"),
        ];
        for (requested, served) in cases {
            assert_eq!(
                detect_served_model_mismatch(&message(requested, Some(served))),
                None,
                "{requested} vs {served}"
            );
        }
    }

    #[test]
    fn a_different_family_or_revision_is_a_substitution() {
        let family_swap = detect_served_model_mismatch(&message("claude-opus-5", Some("claude-haiku-4-5")));
        assert_eq!(
            family_swap,
            Some(ServedModelMismatch {
                requested: "claude-opus-5".into(),
                served: "claude-haiku-4-5".into(),
                provider: "openrouter".into(),
                upstream_provider: None,
            })
        );
        let revision_swap =
            detect_served_model_mismatch(&message("claude-opus-4-5", Some("claude-opus-4-9")));
        assert!(revision_swap.is_some());
    }

    #[test]
    fn unclassifiable_ids_are_not_mismatches() {
        assert!(detect_served_model_mismatch(&message("claude-opus-5", Some("numbat-v6-abc"))).is_none());
        assert!(detect_served_model_mismatch(&message("gpt-5.4", Some("gpt-5.4"))).is_none());
        assert!(detect_served_model_mismatch(&message("gpt-5.4", None)).is_none());
        assert_eq!(classify_model("numbat-v6-abc"), None);
    }

    #[test]
    fn the_tracker_announces_each_pair_once() {
        let mut tracker = ServedModelTracker::new();
        let turn = message("claude-opus-5", Some("claude-haiku-4-5"));
        assert!(tracker.check(&turn).is_some());
        assert!(tracker.check(&turn).is_none());
        let other = message("gpt-5.4", Some("gemini-3.1-pro"));
        assert!(tracker.check(&other).is_some());
        assert!(ServedModelTracker::new()
            .check(&message("claude-opus-5", Some("claude-opus-5")))
            .is_none());
    }

    #[test]
    fn labels_name_both_models_and_the_route() {
        let theme = theme();
        let mismatch = ServedModelMismatch {
            requested: "claude-opus-5".into(),
            served: "claude-haiku-4-5".into(),
            provider: "openrouter".into(),
            upstream_provider: Some("Amazon Bedrock".into()),
        };
        assert_eq!(mismatch.via(), "openrouter/Amazon Bedrock");
        let label = mismatch.label(&theme);
        assert!(label.contains("served claude-haiku-4-5"));
        assert!(label.contains("requested claude-opus-5"));
        assert!(label.contains("via openrouter/Amazon Bedrock"));
        assert_eq!(
            mismatch.native_label(),
            "Served claude-haiku-4-5 · requested claude-opus-5 · via openrouter/Amazon Bedrock"
        );
    }

    #[test]
    fn the_marker_renders_three_rows_and_describes_a_served_model_row() {
        let theme = theme();
        let info = ServedModelMismatch {
            requested: "a".into(),
            served: "b".into(),
            provider: "p".into(),
            upstream_provider: None,
        };
        let mut marker = ServedModelMarker::new(info, &theme);
        assert_eq!(marker.render(80).len(), 3);
        let described = marker.describe_native(&DescribeContext::new(80));
        assert_eq!(described.kind, NodeKind::Row);
        assert_eq!(described.props.role(), Some("omp.marker.served-model"));
    }

    #[test]
    fn bracket_stripping_handles_both_ends() {
        assert_eq!(strip_bracketed_affixes("[gcli转] gemini-3.1-pro-preview [假流]"), "gemini-3.1-pro-preview");
        assert_eq!(strip_bracketed_affixes("claude-opus-4-8"), "claude-opus-4-8");
        assert_eq!(bare_model_id("anthropic/claude-opus-5"), "claude-opus-5");
    }
}
