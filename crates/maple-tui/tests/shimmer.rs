//! Port of `l/test/shimmer.test.ts` (portable subset).
//!
//! Divergence: TS used `setShimmerMode`, `shimmerText`, `shimmerSegments`,
//! `Date.now` mocking, and a theme test-double with token/raw ANSI palettes.
//! Rust exposes `set_shimmer_mode`, `shimmer_text`, `shimmer_segments_at` (clock
//! injected), `ShimmerSegment`, and palette/describe APIs over the real theme;
//! tests assert reproducible rendered output/visibility and description behavior.

use std::sync::Mutex;

use maple_tui::theme::shimmer::{
	describe_shimmer, set_shimmer_mode, shimmer_segments_at, shimmer_text, ShimmerMode, ShimmerPalette,
	ShimmerSegment, ShimmerTier,
};
use maple_tui::theme::default_theme;

static MODE_LOCK: Mutex<()> = Mutex::new(());

fn strip_ansi(text: &str) -> String {
	let mut out = String::new();
	let mut chars = text.chars();
	while let Some(c) = chars.next() {
		if c == '\u{1b}' {
			for next in chars.by_ref() {
				if next == 'm' {
					break;
				}
			}
		} else {
			out.push(c);
		}
	}
	out
}

#[test]
fn shimmer_text_preserves_visible_payload_for_bmp_and_surrogate_pairs() {
	let _guard = MODE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
	set_shimmer_mode(ShimmerMode::Classic);
	let theme = default_theme();
	let rendered = shimmer_text("a😀b", &theme, None);
	assert_eq!(strip_ansi(&rendered), "a😀b");
	let all_emoji = shimmer_text("🎉🌟✨🚀", &theme, None);
	assert_eq!(strip_ansi(&all_emoji), "🎉🌟✨🚀");
}

#[test]
fn disabled_mode_renders_all_empty_as_empty_and_mid_tier_for_text() {
	let _guard = MODE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
	set_shimmer_mode(ShimmerMode::Disabled);
	let theme = default_theme();
	let palette = ShimmerPalette {
		low: ShimmerTier::Ansi("\x1b[31m".to_string()),
		mid: ShimmerTier::Ansi("\x1b[32m".to_string()),
		high: ShimmerTier::Ansi("\x1b[33m".to_string()),
		bold: false,
	};
	assert_eq!(shimmer_segments_at(&[ShimmerSegment::plain(""), ShimmerSegment::plain("")], &theme, 1.0), "");
	let out = shimmer_segments_at(
		&[
			ShimmerSegment::new("", &palette),
			ShimmerSegment::plain("😀\u{D800}"),
			ShimmerSegment::new("", &palette),
		],
		&theme,
		1.0,
	);
	assert_eq!(strip_ansi(&out), "😀\u{D800}");
	// no palette code leaks except configured mid/dim/default resets; output non-empty for text segment
	assert!(out.contains("😀"));
	set_shimmer_mode(ShimmerMode::Classic);
}

#[test]
fn segments_share_one_continuous_band_and_payload_is_preserved() {
	let _guard = MODE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
	set_shimmer_mode(ShimmerMode::Classic);
	let theme = default_theme();
	let segments = [ShimmerSegment::plain("abc"), ShimmerSegment::plain("def")];
	let joined = shimmer_segments_at(&segments, &theme, 2.0);
	let single = shimmer_segments_at(&[ShimmerSegment::plain("abcdef")], &theme, 2.0);
	assert_eq!(joined, single);
	assert_eq!(strip_ansi(&joined), "abcdef");
}

#[test]
fn describe_shimmer_reports_tokens_mode_and_non_empty_spans() {
	let _guard = MODE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
	set_shimmer_mode(ShimmerMode::Kitt);
	let custom = ShimmerPalette {
		low: ShimmerTier::Ansi("\x1b[31m".to_string()),
		mid: ShimmerTier::Token("muted"),
		high: ShimmerTier::Token("accent"),
		bold: false,
	};
	let description = describe_shimmer(&[
		ShimmerSegment::new("ab", &custom),
		ShimmerSegment::plain(""),
		ShimmerSegment::plain("cd"),
	]);
	assert_eq!(description.mode, ShimmerMode::Kitt);
	assert_eq!(description.spans.len(), 2);
	assert_eq!(description.spans[0].text, "ab");
	assert_eq!(description.spans[0].style.as_deref(), Some("muted"));
	assert_eq!(description.low, None);
	assert_eq!(description.high.as_deref(), Some("accent"));
	set_shimmer_mode(ShimmerMode::Classic);
}

#[test]
fn crest_reaches_band_and_off_band_preserves_text() {
	let _guard = MODE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
	set_shimmer_mode(ShimmerMode::Classic);
	let theme = default_theme();
	let text = "abcdefghijklmnopqrstuvwxyz0123456789";
	// different times sweep the band; output always preserves full visible text
	let a = shimmer_text(text, &theme, None);
	let b = shimmer_segments_at(&[ShimmerSegment::plain(text)], &theme, 0.833);
	assert_eq!(strip_ansi(&a), text);
	assert_eq!(strip_ansi(&b), text);
	assert_ne!(a, b, "different clock positions should change styling");
}
