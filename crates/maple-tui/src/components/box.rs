//! Reference: `l/src/components/box.ts`.
//!
//! A container that applies padding, an optional background styler and an
//! optional box-drawing border to all of its children.
//!
//! Deviations from the TypeScript original, all crate-shape only:
//!
//! - The crate has no process-wide active theme, so [`Box::describe`] takes the
//!   [`Theme`] used to sample the background/border tokens. [`NativeDescribe`]
//!   describes with the built-in dark theme for hosts that have none to hand.
//! - Line-width publication (`publishLineWidths`/`getPublishedLineWidths`) and
//!   the width-config epoch do not exist in this port yet, so the memo compares
//!   child rows by value and every row's width is measured with
//!   [`visible_width`].
//! - `setIgnoreTight` does not propagate to children: the crate's [`Component`]
//!   trait exposes no such hook.
//! - `removeChild` takes a child index (as [`crate::components::Container`]
//!   does), because components are not comparable by identity; the describe memo
//!   likewise keys on the child count instead of child identity.

use std::cell::RefCell;
use std::sync::Arc;

use crate::component::Component;
use crate::native::describe::{card, col, describe_child, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, Props, Tone};
use crate::native::tone::{background_chrome, color_tone, sample_background, sample_foreground};
use crate::render::render_utils::{get_padding_x, padding};
use crate::text_shape::visible_width;
use crate::theme::{builtin, create_theme, CreateThemeOptions, Theme};

/// Background styler applied to every row of a [`Box`].
pub type BoxStyler = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Box-drawing glyphs plus an optional colorizer for an outline drawn around a
/// [`Box`].
#[derive(Clone)]
pub struct BoxBorder {
	/// The six glyphs the outline is drawn from.
	pub chars: BoxBorderChars,
	/// Styler for the outline; unstyled when absent.
	pub color: Option<BoxStyler>,
}

/// The glyphs a [`BoxBorder`] draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxBorderChars {
	/// Top-left corner.
	pub top_left: String,
	/// Top-right corner.
	pub top_right: String,
	/// Bottom-left corner.
	pub bottom_left: String,
	/// Bottom-right corner.
	pub bottom_right: String,
	/// Horizontal rule.
	pub horizontal: String,
	/// Vertical rule.
	pub vertical: String,
}

impl BoxBorderChars {
	/// The six glyphs, in reference order.
	pub fn new(
		top_left: impl Into<String>,
		top_right: impl Into<String>,
		bottom_left: impl Into<String>,
		bottom_right: impl Into<String>,
		horizontal: impl Into<String>,
		vertical: impl Into<String>,
	) -> Self {
		BoxBorderChars {
			top_left: top_left.into(),
			top_right: top_right.into(),
			bottom_left: bottom_left.into(),
			bottom_right: bottom_right.into(),
			horizontal: horizontal.into(),
			vertical: vertical.into(),
		}
	}
}

/// One frame's memoized rows plus the inputs they were derived from.
struct Cache {
	width: u16,
	bg_sample: Option<String>,
	border_sample: Option<String>,
	child_lines: Vec<Vec<String>>,
	result: Vec<String>,
}

/// The built-in dark theme [`Box::describe_native`] falls back to for hosts that
/// have no theme to hand.
fn describe_theme() -> Theme {
	let json = builtin::builtin_theme_json("dark").expect("built-in dark theme");
	create_theme(json, CreateThemeOptions::default()).expect("built-in dark theme loads")
}

/// Box component - a container that applies padding and background to all
/// children.
pub struct Box {
	children: RefCell<Vec<std::boxed::Box<dyn Component>>>,
	padding_x: u16,
	padding_y: u16,
	bg_fn: Option<BoxStyler>,
	border: Option<BoxBorder>,
	ignore_tight: bool,
	cached: Option<Cache>,
	native: RefCell<Option<CachedNative>>,
}

/// The described card plus the chrome inputs it was built from.
struct CachedNative {
	/// Identity of the children the node was described from.
	child_count: usize,
	bg_sample: Option<String>,
	border_sample: Option<String>,
	border_color: Option<String>,
	node: NativeNode,
}

impl Default for Box {
	fn default() -> Self {
		Box::new(1, 1)
	}
}

impl Box {
	/// A box padding its interior by `padding_x` columns and `padding_y` rows.
	pub fn new(padding_x: u16, padding_y: u16) -> Self {
		Box {
			children: RefCell::new(Vec::new()),
			padding_x,
			padding_y,
			bg_fn: None,
			border: None,
			ignore_tight: false,
			cached: None,
			native: RefCell::new(None),
		}
	}

	/// A box with a background styler and (optionally) a border.
	pub fn styled(
		padding_x: u16,
		padding_y: u16,
		bg_fn: Option<BoxStyler>,
		border: Option<BoxBorder>,
	) -> Self {
		Box { bg_fn, border, ..Box::new(padding_x, padding_y) }
	}

	/// Ignore tight layout for this box, keeping its configured padding.
	pub fn set_ignore_tight(&mut self, ignore: bool) -> &mut Self {
		self.ignore_tight = ignore;
		self.invalidate_cache();
		self
	}

	/// Container layout and child-count state, for debug inspection.
	pub fn debug_state(&self) -> serde_json::Value {
		serde_json::json!({
			"childCount": self.children.borrow().len(),
			"paddingX": self.padding_x,
			"paddingY": self.padding_y,
			"bordered": self.border.is_some(),
			"ignoreTight": self.ignore_tight,
		})
	}

	/// Append a child.
	pub fn add_child<C: Component + 'static>(&mut self, child: C) -> &mut Self {
		self.children.borrow_mut().push(std::boxed::Box::new(child));
		self.invalidate_cache();
		self
	}

	/// Remove and return the child at `index`.
	pub fn remove_child(&mut self, index: usize) -> Option<std::boxed::Box<dyn Component>> {
		let mut children = self.children.borrow_mut();
		if index >= children.len() {
			return None;
		}
		let child = children.remove(index);
		drop(children);
		self.invalidate_cache();
		Some(child)
	}

	/// Drop every child.
	pub fn clear(&mut self) {
		self.children.borrow_mut().clear();
		self.invalidate_cache();
	}

	/// The current children.
	pub fn children(&self) -> std::cell::Ref<'_, Vec<std::boxed::Box<dyn Component>>> {
		self.children.borrow()
	}

	/// Set the horizontal padding; a no-op when unchanged.
	pub fn set_padding_x(&mut self, padding_x: u16) {
		if self.padding_x == padding_x {
			return;
		}
		self.padding_x = padding_x;
		self.invalidate_cache();
	}

	/// Set the vertical padding; a no-op when unchanged.
	pub fn set_padding_y(&mut self, padding_y: u16) {
		if self.padding_y == padding_y {
			return;
		}
		self.padding_y = padding_y;
		self.invalidate_cache();
	}

	/// Replace the background styler. The cache is deliberately kept: a styler
	/// swap is detected by sampling its output.
	pub fn set_bg_fn(&mut self, bg_fn: Option<BoxStyler>) {
		self.bg_fn = bg_fn;
	}

	/// Replace the border.
	pub fn set_border(&mut self, border: Option<BoxBorder>) {
		self.border = border;
		self.invalidate_cache();
	}

	fn invalidate_cache(&mut self) {
		self.cached = None;
	}

	/// A `card` when the box has a border or background: the background token
	/// picks tone/role (user message, tool state, selection), a border colour
	/// picks the ring tone. A plain `col` otherwise. Padding is the terminal's.
	pub fn describe(&self, cx: &DescribeContext, theme: &Theme) -> NativeNode {
		let bg = sample_background(self.bg_fn.as_deref(), theme);
		let border_color = self
			.border
			.as_ref()
			.and_then(|border| sample_foreground(border.color.as_deref(), theme));
		let bg_sample = self.bg_fn.as_ref().map(|bg_fn| bg_fn("test"));
		let border_sample = self.border.as_ref().map(|border| {
			let side = border.color.as_ref().map_or_else(|| "|".to_string(), |color| color("|"));
			format!("{side}{}{}", border.chars.top_left, border.chars.vertical)
		});
		let border_color_sample = border_color.map(|color| color.as_str().to_string());
		if let Some(cached) = self.native.borrow().as_ref() {
			if cached.child_count == self.children.borrow().len()
				&& cached.bg_sample == bg_sample
				&& cached.border_sample == border_sample
				&& cached.border_color == border_color_sample
			{
				return cached.node.clone();
			}
		}
		// Children without a native description render to a `rows` fallback, so a
		// described child is resolved by its own `describe` hook.
		let described_children: Vec<NativeNode> = {
			let mut children = self.children.borrow_mut();
			children.iter_mut().map(|child| describe_child(child.as_mut(), cx)).collect()
		};
		let described = if self.border.is_none() && self.bg_fn.is_none() {
			col(described_children, Props::new())
		} else {
			let chrome = background_chrome(bg);
			let mut props = Props::new();
			props.set_str("role", chrome.role.unwrap_or("omp.panel"));
			let tone = border_color
				.and_then(|color| color_tone(Some(color)))
				.or(chrome.tone)
				.unwrap_or(Tone::Neutral);
			props.set_str("tone", tone.as_str());
			if let Some(selected) = chrome.selected {
				props.set_bool("selected", selected);
			}
			// Fill without a ring: a flat inset panel.
			if self.border.is_none() {
				props.set_bool("inset", true);
			}
			card(props, described_children)
		};
		*self.native.borrow_mut() = Some(CachedNative {
			child_count: self.children.borrow().len(),
			bg_sample,
			border_sample,
			border_color: border_color_sample,
			node: described.clone(),
		});
		described
	}
}

impl Component for Box {
	fn render(&mut self, width: u16) -> Vec<String> {
		let padding_x = if self.ignore_tight {
			usize::from(self.padding_x)
		} else {
			get_padding_x(usize::from(self.padding_x))
		};
		let padding_y = usize::from(self.padding_y);
		// A border eats one column on each side; skip it unless the interior can
		// still hold the horizontal padding plus at least one content column, so a
		// bordered Box never overflows the width it was given.
		let border = self
			.border
			.as_ref()
			.filter(|_| usize::from(width) >= padding_x * 2 + 3);
		let inner_width = if border.is_some() { usize::from(width.saturating_sub(2)) } else { usize::from(width) };
		let content_width = inner_width.saturating_sub(padding_x * 2).max(1);
		// bg_fn / border output can change without the function reference changing
		// (theme mutation); sample both so a silent palette swap still misses the
		// cache.
		let bg_sample = self.bg_fn.as_ref().map(|bg_fn| bg_fn("test"));
		let border_sample = border.map(|border| {
			let side = border.color.as_ref().map_or_else(|| "|".to_string(), |color| color("|"));
			format!("{side}{}{}", border.chars.top_left, border.chars.vertical)
		});

		// Render every child every frame (renders may carry side effects); the
		// memo only skips re-deriving the padded/background rows.
		let mut child_lines: Vec<Vec<String>> = Vec::with_capacity(self.children.borrow().len());
		for child in self.children.borrow_mut().iter_mut() {
			child_lines.push(child.render(content_width as u16));
		}
		let content_rows: usize = child_lines.iter().map(Vec::len).sum();

		if let Some(cached) = self.cached.as_ref() {
			if cached.width == width
				&& cached.bg_sample == bg_sample
				&& cached.border_sample == border_sample
				&& cached.child_lines == child_lines
			{
				return cached.result.clone();
			}
		}

		let mut result: Vec<String> = Vec::new();
		if content_rows > 0 {
			let left_pad = padding(padding_x);
			let mut interior: Vec<String> = Vec::new();
			let mut push_row = |row: &str, visible: usize| {
				let pad_needed = inner_width.saturating_sub(visible);
				let padded = if pad_needed > 0 { format!("{row}{}", padding(pad_needed)) } else { row.to_string() };
				interior.push(match self.bg_fn.as_ref() {
					Some(bg_fn) => bg_fn(&padded),
					None => padded,
				});
			};
			// Top padding.
			for _ in 0..padding_y {
				push_row("", 0);
			}
			// Content.
			for lines in &child_lines {
				for line in lines {
					let row = if padding_x > 0 { format!("{left_pad}{line}") } else { line.clone() };
					let visible = visible_width(&row);
					push_row(&row, visible);
				}
			}
			// Bottom padding.
			for _ in 0..padding_y {
				push_row("", 0);
			}
			drop(push_row);

			match border {
				Some(border) => {
					let paint = |text: &str| match border.color.as_ref() {
						Some(color) => color(text),
						None => text.to_string(),
					};
					let rule = border.chars.horizontal.repeat(inner_width);
					let side = paint(&border.chars.vertical);
					result.push(paint(&format!(
						"{}{rule}{}",
						border.chars.top_left, border.chars.top_right
					)));
					for row in &interior {
						result.push(format!("{side}{row}{side}"));
					}
					result.push(paint(&format!(
						"{}{rule}{}",
						border.chars.bottom_left, border.chars.bottom_right
					)));
				}
				None => result.extend(interior),
			}
		}

		self.cached = Some(Cache {
			width,
			bg_sample,
			border_sample,
			child_lines,
			result: result.clone(),
		});
		result
	}

	fn invalidate(&mut self) {
		self.invalidate_cache();
		for child in self.children.borrow_mut().iter_mut() {
			child.invalidate();
		}
	}

	fn dispose(&mut self) {
		self.invalidate_cache();
		for child in self.children.borrow_mut().iter_mut() {
			child.dispose();
		}
	}
}

impl NativeDescribe for Box {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		self.describe(cx, &describe_theme())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::components::Text;
	use crate::theme::{ColorMode, ThemeBg};

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin dark theme"),
			CreateThemeOptions { mode: Some(ColorMode::TrueColor), ..Default::default() },
		)
		.expect("valid theme")
	}

	fn border(chars: BoxBorderChars, color: Option<BoxStyler>) -> BoxBorder {
		BoxBorder { chars, color }
	}

	fn ascii_border() -> BoxBorderChars {
		BoxBorderChars::new("+", "+", "+", "+", "-", "|")
	}

	#[test]
	fn pads_children_to_the_requested_width() {
		crate::render::render_utils::set_tight_layout(false);
		let mut container = Box::new(1, 1);
		container.add_child(Text::new("hi"));
		assert_eq!(container.render(10), vec!["          ", " hi       ", "          "]);
	}

	#[test]
	fn border_wraps_the_padded_interior() {
		crate::render::render_utils::set_tight_layout(false);
		let mut container = Box::styled(0, 0, None, Some(border(ascii_border(), None)));
		container.add_child(Text::new("abc"));
		assert_eq!(container.render(6), vec!["+----+", "|abc |", "+----+"]);
	}

	#[test]
	fn border_is_dropped_when_the_interior_would_overflow() {
		crate::render::render_utils::set_tight_layout(false);
		let mut container = Box::styled(1, 0, None, Some(border(ascii_border(), None)));
		container.add_child(Text::new("x"));
		// width 3 cannot hold a border plus one column of horizontal padding.
		assert_eq!(container.render(3), vec![" x "]);
	}

	#[test]
	fn identical_child_rows_reuse_the_memoized_frame() {
		crate::render::render_utils::set_tight_layout(false);
		let mut container = Box::new(0, 0);
		container.add_child(Text::new("a"));
		let first = container.render(4);
		assert_eq!(first, vec!["a   "]);
		assert_eq!(container.render(4), first);
		// A new child changes the row set, so the memo must miss.
		container.add_child(Text::new("b"));
		assert_eq!(container.render(4), vec!["a   ", "b   "]);
	}

	#[test]
	fn background_styler_paints_every_row() {
		crate::render::render_utils::set_tight_layout(false);
		let mut container =
			Box::styled(1, 0, Some(Arc::new(|text| format!("\x1b[48;5;17m{text}\x1b[49m"))), None);
		container.add_child(Text::new("hi"));
		let rows = container.render(6);
		assert_eq!(rows.len(), 1);
		assert!(rows[0].starts_with("\x1b[48;5;17m"));
		assert!(rows[0].ends_with("\x1b[49m"));
	}

	#[test]
	fn plain_boxes_describe_as_a_col() {
		let mut container = Box::new(1, 0);
		container.add_child(Text::new("hi"));
		let node = container.describe(&DescribeContext::new(40), &theme());
		assert_eq!(node.kind, crate::native::node::NodeKind::Col);
	}

	#[test]
	fn background_tokens_describe_a_tinted_card() {
		let theme = theme();
		let bg_theme = theme.clone();
		let mut container = Box::styled(
			1,
			0,
			Some(Arc::new(move |text| bg_theme.bg(ThemeBg::UserMessageBg.as_str(), text).unwrap_or_else(|| text.to_string()))),
			None,
		);
		container.add_child(Text::new("hi"));
		let node = container.describe(&DescribeContext::new(40), &theme);
		assert_eq!(node.kind, crate::native::node::NodeKind::Card);
		assert_eq!(node.props.get_str("role"), Some("omp.user"));
		assert_eq!(node.props.get_str("tone"), Some("user"));
		assert_eq!(node.props.get_bool("inset"), Some(true));
	}

	#[test]
	fn border_colour_describes_the_ring_tone() {
		let theme = theme();
		let color_theme = theme.clone();
		let mut container = Box::styled(
			0,
			0,
			None,
			Some(border(
				ascii_border(),
				Some(Arc::new(move |text| color_theme.fg("borderAccent", text).unwrap_or_else(|| text.to_string()))),
			)),
		);
		container.add_child(Text::new("hi"));
		let node = container.describe(&DescribeContext::new(40), &theme);
		assert_eq!(node.kind, crate::native::node::NodeKind::Card);
		assert_eq!(node.props.get_str("role"), Some("omp.panel"));
		assert_eq!(node.props.get_str("tone"), Some("accent"));
		assert_eq!(node.props.get_bool("inset"), None);
	}
}
