//! Dynamic border rule that adjusts to the viewport width.
//!
//! Reference: `l/src/chrome/dynamic-border.ts`.
//!
//! Deviation: the reference reads the process-global `theme` through
//! `fgOrPlain`, which returns plain text while that global is undefined. This
//! port holds an explicit `&Theme`; `fg_or_plain` skips the style when
//! `Theme::has_color` reports the token has no real foreground, the closest Rust
//! equivalent of "no colour support". Closure labels compare by identity in
//! the reference; a Rust `BorderLabel` box is always a new value, so installing
//! a closure invalidates the cache.

use crate::component::Component;
use crate::native::describe::{empty_node, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode};
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::Theme;

/// The `fgOrPlain` equivalent: paint `text` with `color`, or leave it unstyled
/// when the theme has no real foreground for the token.
fn fg_or_plain(theme: &Theme, color: &str, text: &str) -> String {
	if theme.has_color(color) {
		theme.fg(color, text).unwrap_or_else(|_| text.to_string())
	} else {
		text.to_string()
	}
}

/// Label drawn inside the rule after a short lead (`─── label ────`).
pub enum BorderLabel<'a> {
	/// Fixed text.
	Fixed(String),
	/// Re-read every render, for live text such as a spinner.
	Dynamic(Box<dyn Fn() -> String + 'a>),
}

impl<'a> BorderLabel<'a> {
	/// A fixed label.
	pub fn fixed(text: impl Into<String>) -> Self {
		BorderLabel::Fixed(text.into())
	}

	/// A label re-evaluated on every render.
	pub fn dynamic(label: impl Fn() -> String + 'a) -> Self {
		BorderLabel::Dynamic(Box::new(label))
	}
}

/// Dynamic border component that adjusts to the viewport width.
///
/// Native terminals draw their own chrome, so [`NativeDescribe`] reports an
/// empty node.
pub struct DynamicBorder<'a> {
	theme: &'a Theme,
	color: Box<dyn Fn(&str) -> String + 'a>,
	label: Option<BorderLabel<'a>>,
	cached_width: Option<usize>,
	cached_label: Option<String>,
	cached_lines: Option<Vec<String>>,
}

impl<'a> DynamicBorder<'a> {
	/// A border painted with the theme's `border` token.
	pub fn new(theme: &'a Theme) -> Self {
		let color: Box<dyn Fn(&str) -> String + 'a> =
			Box::new(move |text| fg_or_plain(theme, "border", text));
		Self::with_color(theme, color)
	}

	/// A border painted by a caller-supplied styler.
	pub fn with_color(theme: &'a Theme, color: Box<dyn Fn(&str) -> String + 'a>) -> Self {
		DynamicBorder {
			theme,
			color,
			label: None,
			cached_width: None,
			cached_label: None,
			cached_lines: None,
		}
	}

	/// Pre-styled text set into the rule after a short lead; `None` draws a
	/// plain rule.
	pub fn set_label(&mut self, label: Option<BorderLabel<'a>>) {
		if self.label_is(&label) {
			return;
		}
		self.label = label;
		self.invalidate();
	}

	/// Drop the rendered cache; the next `render` rebuilds from state.
	pub fn invalidate(&mut self) {
		self.cached_width = None;
		self.cached_lines = None;
	}

	fn label_is(&self, other: &Option<BorderLabel<'a>>) -> bool {
		match (&self.label, other) {
			(None, None) => true,
			(Some(BorderLabel::Fixed(a)), Some(BorderLabel::Fixed(b))) => a == b,
			_ => false,
		}
	}

	fn resolved_label(&self) -> Option<String> {
		match &self.label {
			None => None,
			Some(BorderLabel::Fixed(text)) => Some(text.clone()),
			Some(BorderLabel::Dynamic(label)) => Some(label()),
		}
	}

	fn rule(&self, horizontal: &str, width: usize, label: Option<&str>) -> String {
		let lead = horizontal.repeat(3);
		let label_room = width
			.saturating_sub(visible_width(&lead))
			.saturating_sub(3);
		if label.map_or(true, |text| text.is_empty()) || label_room < 1 {
			return (self.color)(&horizontal.repeat(width));
		}
		let label = label.unwrap_or("");
		let fitted = format!(
			" {} ",
			truncate_to_width(label, label_room, Ellipsis::Unicode)
		);
		let rest = width
			.saturating_sub(visible_width(&lead))
			.saturating_sub(visible_width(&fitted));
		format!(
			"{}{}{}",
			(self.color)(&lead),
			fitted,
			(self.color)(&horizontal.repeat(rest))
		)
	}
}

impl<'a> Component for DynamicBorder<'a> {
	fn render(&mut self, width: u16) -> Vec<String> {
		let label = self.resolved_label();
		let raw_width = width as usize;
		if self.cached_width == Some(raw_width) && self.cached_label == label {
			if let Some(lines) = &self.cached_lines {
				return lines.clone();
			}
		}
		let paint_width = raw_width.max(1);
		let horizontal = self.theme.symbol("boxRound.horizontal");
		let lines = vec![self.rule(horizontal, paint_width, label.as_deref())];
		self.cached_width = Some(raw_width);
		self.cached_label = label;
		self.cached_lines = Some(lines.clone());
		lines
	}

	fn invalidate(&mut self) {
		self.cached_width = None;
		self.cached_lines = None;
	}
}

impl<'a> NativeDescribe for DynamicBorder<'a> {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		empty_node()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::color::ColorMode;
	use crate::theme::{create_theme, CreateThemeOptions};
	use std::cell::Cell;

	fn theme(name: &str) -> Theme {
		create_theme(
			builtin::builtin_theme_json(name).expect("builtin theme"),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.expect("theme resolves")
	}

	#[test]
	fn fg_or_plain_skips_unset_tokens_and_paints_set_ones() {
		let dark = theme("dark");
		assert_eq!(fg_or_plain(&dark, "text", "plain"), "plain");
		assert_eq!(
			fg_or_plain(&dark, "border", "x"),
			"\x1b[38;2;23;143;185mx\x1b[39m"
		);
	}

	#[test]
	fn light_and_dark_themes_paint_their_own_border_colour() {
		let dark = theme("dark");
		let mut border = DynamicBorder::new(&dark);
		assert_eq!(
			border.render(20),
			vec![format!(
				"\x1b[38;2;23;143;185m{}\x1b[39m",
				"─".repeat(20)
			)]
		);

		let light = theme("light");
		let mut border = DynamicBorder::new(&light);
		assert_eq!(
			border.render(20),
			vec![format!(
				"\x1b[38;2;84;125;167m{}\x1b[39m",
				"─".repeat(20)
			)]
		);
	}

	#[test]
	fn label_is_inset_with_single_spaces_and_the_rule_fills_the_rest() {
		let dark = theme("dark");
		let mut border = DynamicBorder::new(&dark);
		border.set_label(Some(BorderLabel::fixed("Deploying")));
		assert_eq!(
			border.render(20),
			vec![
				"\x1b[38;2;23;143;185m───\x1b[39m Deploying \x1b[38;2;23;143;185m──────\x1b[39m"
					.to_string()
			]
		);
	}

	#[test]
	fn long_labels_are_truncated_to_the_available_room() {
		let dark = theme("dark");
		let mut border = DynamicBorder::new(&dark);
		border.set_label(Some(BorderLabel::fixed(
			"Deploying the whole monorepo now",
		)));
		assert_eq!(
			border.render(20),
			vec![
				"\x1b[38;2;23;143;185m───\x1b[39m Deploying the… \x1b[38;2;23;143;185m─\x1b[39m"
					.to_string()
			]
		);
	}

	#[test]
	fn narrow_widths_fall_back_to_a_plain_rule() {
		let dark = theme("dark");
		let mut border = DynamicBorder::new(&dark);
		border.set_label(Some(BorderLabel::fixed("x")));
		assert_eq!(
			border.render(4),
			vec![format!(
				"\x1b[38;2;23;143;185m{}\x1b[39m",
				"─".repeat(4)
			)]
		);
		assert_eq!(
			border.render(0),
			vec![format!("\x1b[38;2;23;143;185m\u{2500}\x1b[39m")]
		);
	}

	#[test]
	fn dynamic_labels_are_reread_and_cache_until_changed() {
		let dark = theme("dark");
		let mut border = DynamicBorder::new(&dark);
        let calls = std::rc::Rc::new(Cell::new(0usize));
        let frame = std::rc::Rc::new(Cell::new(0usize));
        let counted = std::rc::Rc::clone(&calls);
        let framed = std::rc::Rc::clone(&frame);
        border.set_label(Some(BorderLabel::dynamic(move || {
            counted.set(counted.get() + 1);
            format!("f{}", framed.get())
        })));
		let first = border.render(20);
		let second = border.render(20);
		assert_eq!(first, second);
		assert_eq!(calls.get(), 2);
		frame.set(1);
		let third = border.render(20);
		assert!(third[0].contains("f1"));
		assert_eq!(calls.get(), 3);
	}
}
