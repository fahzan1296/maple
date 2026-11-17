//! Shared chrome formatting helpers.
//!
//! Reference: `l/src/chrome/format.ts`.
//!
//! Deviations:
//! - [`render_ascii_bar`] adapts the reference's free
//!   `renderProgressBar(value,width,options)` to the crate's [`ProgressBar`]
//!   component/state API and renders a single styled row; the reference's full
//!   `ProgressBarOptions`/`ProgressBarStyle` object (custom glyph/style hooks,
//!   theme shimmer indeterminate animation) has no direct crate equivalent.
//! - Indeterminate rendering uses the progress bar's static indeterminate glyph
//!   because the crate has no `shimmerText` helper.
//! - `ProgressBarTheme`-like parameter is omitted; the crate progress bar draws
//!   with its own styles.

use crate::component::Component;
use crate::components::progress_bar::ProgressBar;
use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, Props};

/// Title-case a provider id for display (`openai-codex` → `Openai Codex`).
pub fn format_provider_name(provider: &str) -> String {
	provider
		.split(|c| c == '-' || c == '_')
		.map(|part| {
			let mut chars = part.chars();
			match chars.next() {
				Some(first) => {
					let mut out = String::new();
					out.extend(first.to_uppercase());
					out.push_str(chars.as_str());
					out
				}
				None => String::new(),
			}
		})
		.collect::<Vec<_>>()
		.join(" ")
}

/// Format a millisecond duration as a coarse-grained human label.
pub fn format_coarse_duration(ms: u64) -> String {
	let seconds = ms.div_ceil(1000);
	if seconds < 60 {
		return format!("{seconds}s");
	}
	let minutes = seconds.div_ceil(60);
	if minutes < 60 {
		return format!("{minutes}m");
	}
	let hours = minutes.div_ceil(60);
	if hours < 48 {
		return format!("{hours}h");
	}
	let days = hours.div_ceil(24);
	format!("{days}d")
}

/// Native progress node: a terminal-drawn progress bar with the same percent
/// label; `None` is indeterminate.
pub fn describe_ascii_bar(fraction: Option<f64>) -> NativeNode {
	let mut props = Props::new();
	match fraction {
		Some(value) => {
			let value = value.clamp(0.0, 1.0);
			props.insert(
				"value",
				serde_json::Value::Number(
					serde_json::Number::from_f64(value).unwrap_or_else(|| serde_json::Number::from(0)),
				),
			);
			props.set_str("label", format!("{}%", (value * 100.0).round() as i64));
		}
		None => {
			props.insert("value", serde_json::Value::Null);
		}
	}
	node(NodeKind::Progress, props, Vec::new(), None)
}

/// Render an ASCII progress bar with a trailing percent label.
///
/// `fraction` is clamped to `[0, 1]`. `None` renders an indeterminate
/// placeholder.
pub fn render_ascii_bar(fraction: Option<f64>, width: usize) -> String {
	let mut bar = ProgressBar::new();
	bar.set_total(1.0);
	bar.set_progress(fraction);
	bar.set_prefix("[");
	bar.set_suffix("]");
	bar.set_filled_glyph("█");
	bar.set_empty_glyph("░");
	bar.set_indeterminate_glyph(Some("·".to_string()));
	bar.set_show_percentage(fraction.is_some());
	bar.render(width.max(1) as u16).into_iter().next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_provider_name_title_cases_segments() {
		assert_eq!(format_provider_name("openai-codex"), "Openai Codex");
		assert_eq!(format_provider_name("anthropic"), "Anthropic");
		assert_eq!(format_provider_name("a_b"), "A B");
	}

	#[test]
	fn format_coarse_duration_thresholds() {
		assert_eq!(format_coarse_duration(0), "0s");
		assert_eq!(format_coarse_duration(59_000), "59s");
		assert_eq!(format_coarse_duration(60_000), "1m");
		assert_eq!(format_coarse_duration(3_600_000), "1h");
		assert_eq!(format_coarse_duration(172_800_000), "2d");
	}

	#[test]
	fn describe_ascii_bar_sets_value_and_label() {
		let node = describe_ascii_bar(Some(0.5));
		assert_eq!(node.kind, NodeKind::Progress);
		assert_eq!(node.prop("label").and_then(|v| v.as_str()), Some("50%"));
		assert!(node.prop("value").is_some());
		let ind = describe_ascii_bar(None);
		assert!(ind.prop("value").unwrap().is_null());
	}

	#[test]
	fn render_ascii_bar_renders_percent() {
		let row = render_ascii_bar(Some(0.5), 12);
		assert!(row.contains("50%"));
		let ind = render_ascii_bar(None, 8);
		assert!(ind.contains('[') && ind.contains(']'));
	}
}
