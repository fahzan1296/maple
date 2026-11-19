//! The `cache miss` marker: a slim divider after an assistant turn whose
//! request lost the prompt cache.
//!
//! Reference: `l/src/chat/cache-invalidation-marker.ts`.
//!
//! Usage arrives at message end, after completed rows may already be in native
//! history, so the marker trails streamed content.
//!
//! Deviations: `formatNumber` is [`crate::status_line::segments::format_number`]
//! and the reference's process-global `theme` is an explicit `&Theme`.

use crate::chrome::MessageDivider;
use crate::component::Component;
use crate::native::describe::{node, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text};
use crate::status_line::segments::format_number;
use crate::style::{Color, Style};
use crate::theme::Theme;

use crate::chat::messages::Usage;

/// Minimum prefix the previous turn must have read back from cache before a
/// collapse on the current turn counts as an invalidation.
pub const MIN_CACHE_FOOTPRINT: u64 = 2048;

/// A prompt-cache invalidation detected from a turn's usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheInvalidation {
    /// Prompt tokens the cold turn had to (re)process instead of reading from
    /// cache.
    pub reprocessed_tokens: u64,
}

/// Decide whether `current` lost a working prompt cache that `prev` was
/// reusing.
///
/// Only a warm→cold transition counts: the previous turn must have actually
/// read a meaningful prefix back, this turn must have read back less than half
/// of it, and this turn must have written a prefix again (`cacheWrite > 0`),
/// which only an explicit prefix-controlled cache does. Implicit best-effort
/// caches report `cacheWrite: 0`, so their intermittent `cacheRead` drops stay
/// unflagged.
pub fn detect_cache_invalidation(
    prev: Option<&Usage>,
    current: &Usage,
) -> Option<CacheInvalidation> {
    let prev = prev?;
    if prev.cache_read < MIN_CACHE_FOOTPRINT {
        return None;
    }
    if current.cache_read * 2 >= prev.cache_read {
        return None;
    }
    if current.cache_write == 0 {
        return None;
    }
    let reprocessed_tokens = current.cache_write + current.input;
    if reprocessed_tokens < MIN_CACHE_FOOTPRINT {
        return None;
    }
    Some(CacheInvalidation { reprocessed_tokens })
}

impl CacheInvalidation {
    /// The divider's label in the reference's wording.
    pub fn label(&self, theme: &Theme) -> String {
        let icon = theme.icon().cache_miss;
        let head = if icon.is_empty() {
            "cache miss".to_string()
        } else {
            format!("{icon} cache miss")
        };
        if self.reprocessed_tokens > 0 {
            format!(
                "{head} {} {} tokens",
                theme.sep().dot.trim(),
                format_number(self.reprocessed_tokens as f64)
            )
        } else {
            head
        }
    }

    /// The native label (no glyphs, sentence case).
    pub fn native_label(&self) -> String {
        if self.reprocessed_tokens > 0 {
            format!("Cache miss · {} tokens", format_number(self.reprocessed_tokens as f64))
        } else {
            "Cache miss".to_string()
        }
    }
}

/// The slim divider rendered after a cold turn.
pub struct CacheInvalidationMarker {
    divider: MessageDivider,
    info: CacheInvalidation,
}

impl CacheInvalidationMarker {
    /// A marker for `info`.
    pub fn new(info: CacheInvalidation, theme: &Theme) -> Self {
        let mut divider = MessageDivider::new(info.label(theme));
        divider.set_label_style(Style::fg(Color::Gray));
        divider.set_rule_style(Style::fg(Color::DarkGray));
        divider.set_rule_width(10);
        CacheInvalidationMarker { divider, info }
    }

    /// The invalidation this marker reports.
    pub fn invalidation(&self) -> CacheInvalidation {
        self.info
    }
}

impl Component for CacheInvalidationMarker {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.divider.render(width)
    }

    fn invalidate(&mut self) {
        self.divider.invalidate();
    }
}

impl NativeDescribe for CacheInvalidationMarker {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let mut props = Props::new();
        props.set_str("gap", "sm");
        props.set_str("align", "start");
        props.set_str("role", "omp.marker.cache-miss");
        node(
            NodeKind::Row,
            props,
            vec![
                node(
                    NodeKind::Icon,
                    {
                        let mut icon = Props::new();
                        icon.set_str("name", "database");
                        icon
                    },
                    Vec::new(),
                    Some("icon"),
                ),
                text(Text::plain(self.info.native_label()), Props::new()),
            ],
            Some("cache-miss"),
        )
    }
}

/// The marker's head spans, for callers that compose their own row.
pub fn cache_miss_spans(info: CacheInvalidation) -> Vec<crate::native::node::Span> {
    vec![span(info.native_label(), Some("muted"))]
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn usage(input: u64, cache_read: u64, cache_write: u64) -> Usage {
        Usage {
            input,
            cache_read,
            cache_write,
            ..Usage::default()
        }
    }

    #[test]
    fn a_warm_to_cold_transition_is_flagged() {
        let prev = usage(1_000, 120_000, 120_000);
        let current = usage(90_000, 15_000, 100_000);
        assert_eq!(
            detect_cache_invalidation(Some(&prev), &current),
            Some(CacheInvalidation {
                reprocessed_tokens: 190_000
            })
        );
    }

    #[test]
    fn expected_unflagged_cases() {
        let cold = usage(90_000, 10_000, 100_000);
        // The first turn has no predecessor to lose a cache from.
        assert_eq!(detect_cache_invalidation(None, &cold), None);
        // A predecessor below the cacheable-prefix floor proves nothing.
        assert_eq!(
            detect_cache_invalidation(Some(&usage(1_000, 2_047, 2_047)), &cold),
            None
        );
        // A turn that read back at least half the warm prefix kept it warm.
        assert_eq!(
            detect_cache_invalidation(
                Some(&usage(1_000, 120_000, 120_000)),
                &usage(1_000, 60_000, 60_000)
            ),
            None
        );
    }

    #[test]
    fn implicit_caches_and_small_reprocessing_are_unflagged() {
        let warm = usage(1_000, 120_000, 120_000);
        // Google/OpenAI/Fireworks report cacheWrite 0.
        assert_eq!(
            detect_cache_invalidation(Some(&warm), &usage(90_000, 10_000, 0)),
            None
        );
        // A collapse smaller than the cacheable-prefix floor says nothing.
        let tiny_prev = usage(1_000, 4_000, 4_000);
        assert_eq!(
            detect_cache_invalidation(Some(&tiny_prev), &usage(100, 1_000, 1_000)),
            None
        );
    }

    #[test]
    fn labels_follow_the_reference_wording() {
        let theme = theme();
        let info = CacheInvalidation {
            reprocessed_tokens: 50_900,
        };
        let label = info.label(&theme);
        assert!(label.contains("cache miss"));
        assert!(label.contains(&format_number(50_900.0)));
        assert!(label.ends_with("tokens"));
        assert_eq!(
            info.native_label(),
            format!("Cache miss · {} tokens", format_number(50_900.0))
        );
        assert_eq!(
            CacheInvalidation { reprocessed_tokens: 0 }.native_label(),
            "Cache miss"
        );
    }

    #[test]
    fn the_marker_renders_three_rows_and_tags_its_role() {
        let theme = theme();
        let mut marker = CacheInvalidationMarker::new(
            CacheInvalidation {
                reprocessed_tokens: 12_000,
            },
            &theme,
        );
        assert_eq!(marker.render(80).len(), 3);
        assert_eq!(marker.invalidation().reprocessed_tokens, 12_000);
        let described = marker.describe_native(&DescribeContext::new(80));
        assert_eq!(described.props.role(), Some("omp.marker.cache-miss"));
        assert_eq!(described.children[0].props.get_str("name"), Some("database"));
    }

    #[test]
    fn spans_carry_the_native_label() {
        let spans = cache_miss_spans(CacheInvalidation {
            reprocessed_tokens: 4_096,
        });
        assert_eq!(spans.len(), 1);
        assert!(spans[0].t.starts_with("Cache miss · "));
    }
}
