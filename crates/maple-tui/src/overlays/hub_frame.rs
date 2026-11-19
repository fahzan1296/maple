//! Reference: `l/src/overlays/hub-frame.ts`.
//!
//! Shared fullscreen-hub chrome: sidebar list/rows, footer chip strip, and the
//! persistent split frame with sidebar scrolling and chip hit ranges.
//!
//! Deviations:
//! - `TspText` becomes [`NodeText`]; `SidebarEntry<TKind>` carries `kind` as a
//!   string (hubs call their separator row [`SEPARATOR_KIND`]).
//! - Theme access is injected through [`HubFrameStyler`] instead of a global.
//! - The reference composes `SplitPane`/`Stack`/`PanelRows` and can `locate`
//!   inside them. This crate's `SplitPane`/`Stack` expose no `locate`/
//!   `childRect`, so [`HubFrame::locate`] takes a [`HubGeometry`] the host
//!   measures; frame painting stays in the host (same border helpers exist).

use crate::native::describe::{node, span, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::overlay_card;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::theme_class::Theme;

/// The `kind` of a sidebar row drawn as a horizontal rule.
pub const SEPARATOR_KIND: &str = "separator";

/// A scope row shared by fullscreen hubs, with hub-specific kinds and metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarEntry {
	pub id: String,
	pub kind: String,
	pub label: String,
	pub annotation: Option<String>,
}

impl SidebarEntry {
	#[must_use]
	pub fn new(id: impl Into<String>, kind: impl Into<String>, label: impl Into<String>) -> Self {
		SidebarEntry {
			id: id.into(),
			kind: kind.into(),
			label: label.into(),
			annotation: None,
		}
	}

	#[must_use]
	pub fn is_separator(&self) -> bool {
		self.kind == SEPARATOR_KIND
	}
}

/// A pre-styled footer choice carrying a hub-specific action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripChip<A> {
	pub label: String,
	pub styled: String,
	pub action: A,
}

impl<A> StripChip<A> {
	#[must_use]
	pub fn new(label: impl Into<String>, styled: impl Into<String>, action: A) -> Self {
		StripChip {
			label: label.into(),
			styled: styled.into(),
			action,
		}
	}
}

/// Shared selection state for a hub's chip-bearing strip variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripState<A> {
	pub chips: Vec<StripChip<A>>,
	pub index: usize,
}

impl<A> StripState<A> {
	#[must_use]
	pub fn new(chips: Vec<StripChip<A>>) -> Self {
		StripState { chips, index: 0 }
	}

	/// The selected chip, if any.
	#[must_use]
	pub fn selected(&self) -> Option<&StripChip<A>> {
		self.chips.get(self.index)
	}
}

/// A chip hit range in frame-relative columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChipRange {
	pub start: usize,
	pub end: usize,
	pub index: usize,
}

/// Hub-specific sidebar decoration, independent of viewport and row layout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidebarStyle {
	pub icon: String,
	pub annotation: String,
	pub muted: bool,
	pub hovered: bool,
	pub pad_truncated: bool,
}

/// Styling hooks the host supplies.
pub trait HubFrameStyler {
	fn fg(&self, token: &str, text: &str) -> String;
	fn bg(&self, token: &str, text: &str) -> String;
	fn bold(&self, text: &str) -> String;
	fn symbol(&self, key: &str) -> String;
}

impl HubFrameStyler for Theme {
	fn fg(&self, token: &str, text: &str) -> String {
		Theme::fg(self, token, text).unwrap_or_else(|_| text.to_string())
	}
	fn bg(&self, token: &str, text: &str) -> String {
		Theme::bg(self, token, text).unwrap_or_else(|_| text.to_string())
	}
	fn bold(&self, text: &str) -> String {
		Theme::bold(self, text)
	}
	fn symbol(&self, key: &str) -> String {
		Theme::symbol(self, key).to_string()
	}
}

/// Cycle a strip's selection for directional and tab input, returning whether
/// it handled the key. `data` is the reference key name (`"left"`, `"tab"`,
/// `"shift+tab"`, …).
pub fn move_strip_selection<A>(strip: &mut StripState<A>, data: &str) -> bool {
	if matches!(data, "left" | "up" | "shift+tab") {
		if !strip.chips.is_empty() {
			strip.index = (strip.index + strip.chips.len() - 1) % strip.chips.len();
		}
		return true;
	}
	if matches!(data, "right" | "down" | "tab") {
		if !strip.chips.is_empty() {
			strip.index = (strip.index + 1) % strip.chips.len();
		}
		return true;
	}
	false
}

/// Native sidebar: a `list` whose `item`s are keyed by entry id (the `item` of
/// `select`/`activate` events), separators as `rule`s.
pub fn describe_hub_sidebar(
	entries: &[SidebarEntry],
	selected_id: &str,
	style: &dyn Fn(&SidebarEntry, usize) -> (String, String, bool),
	key: &str,
) -> NativeNode {
	let mut items = Vec::with_capacity(entries.len());
	for (index, entry) in entries.iter().enumerate() {
		if entry.is_separator() {
			items.push(node(
				NodeKind::Rule,
				Props::new(),
				Vec::new(),
				Some(&entry.id),
			));
			continue;
		}
		let (icon, annotation, muted) = style(entry, index);
		let mut spans = Vec::new();
		if !icon.is_empty() {
			spans.push(span(format!("{icon} "), Some("muted")));
		}
		spans.push(span(
			entry.label.clone(),
			if muted { Some("dim") } else { None },
		));
		let mut props = Props::new();
		props.set_spans(spans);
		if !annotation.is_empty() {
			props.set_str("value", annotation);
		}
		items.push(node(NodeKind::Item, props, Vec::new(), Some(&entry.id)));
	}
	let mut props = Props::new();
	props.set_str("selected", selected_id);
	// The sidebar keeps its width: a body line wider than the pane must not
	// squeeze it.
	props.set_i64("shrink", 0);
	node(NodeKind::List, props, items, Some(key))
}

/// Native footer chip strip: a `tabs` node whose tab ids are the chip indices
/// (`select` events carry the index as `item`), with an optional prefix label.
pub fn describe_hub_chips<A>(
	strip: &StripState<A>,
	prefix: Option<NodeText>,
	key: &str,
) -> NativeNode {
	let tabs = node(
		NodeKind::Tabs,
		{
			let mut props = Props::new();
			props.insert(
				"items",
				serde_json::Value::Array(
					strip
						.chips
						.iter()
						.enumerate()
						.map(|(index, chip)| {
							serde_json::json!({ "id": index.to_string(), "label": chip.label })
						})
						.collect(),
				),
			);
			props.set_str("active", strip.index.to_string());
			props
		},
		Vec::new(),
		Some("tabs"),
	);
	let children = match prefix {
		None => vec![tabs],
		Some(prefix) => vec![text(prefix, Props::new()), tabs],
	};
	let mut props = Props::new();
	props.set_str("gap", "sm");
	props.set_str("align", "center");
	node(NodeKind::Row, props, children, Some(key))
}

/// Native fullscreen hub: a root card titled `title` over a sidebar/body row
/// and a footer. The split geometry, ring and dividers are the terminal's.
pub fn describe_hub_frame(
	role: &str,
	title: NodeText,
	sidebar: NativeNode,
	body: NativeNode,
	footer: NativeNode,
) -> NativeNode {
	let mut row_props = Props::new();
	row_props.set_str("gap", "md");
	row_props.set_str("align", "start");
	row_props.set_i64("grow", 1);
	overlay_card(
		role,
		Some(title),
		vec![
			node(
				NodeKind::Row,
				row_props,
				vec![sidebar, body],
				Some("split"),
			),
			node(NodeKind::Rule, Props::new(), Vec::new(), Some("divider")),
			footer,
		],
		None,
	)
}

/// Where a mouse position lands inside the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HubHit {
	/// Column inside the footer row.
	pub footer_column: Option<usize>,
	pub body_height: usize,
	/// Content line of the split, or `-1` when outside it.
	pub content_line: isize,
	pub over_sidebar: bool,
	pub over_body: bool,
	pub body_line: isize,
}

/// Measured frame geometry, supplied by the host because the crate's
/// `Stack`/`SplitPane` do not expose `locate`/`childRect`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HubGeometry {
	/// Stack child index under the mouse (0 top, 1 split, 2 divider, 3 footer,
	/// 4 bottom) and its local row/column.
	pub stack_child: Option<(usize, usize, usize)>,
	/// Height of the split child.
	pub body_height: usize,
	/// Split pane under the mouse and its local row.
	pub pane: Option<(PaneSide, usize)>,
}

/// Pane a point landed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneSide {
	Left,
	Right,
}

/// Persistent fullscreen split frame state: sidebar viewport and chip hits.
#[derive(Clone, Debug)]
pub struct HubFrame {
	/// Current sidebar viewport offset, shared with the hub's navigation.
	pub sidebar_scroll: usize,
	/// Chip hit ranges from the last footer render.
	pub chip_ranges: Vec<ChipRange>,
	title: String,
	sidebar_min: usize,
	sidebar_max: usize,
	/// Whether narrow layouts may reduce the body floor to keep the sidebar
	/// usable.
	preserve_sidebar: bool,
	/// Preferred body floor, reduced only as needed to retain navigation.
	body_min_width: usize,
}

impl HubFrame {
	/// Compose hub-specific sidebar and body renderers into a persistent frame.
	#[must_use]
	pub fn new(
		title: impl Into<String>,
		sidebar_bounds: (usize, usize),
		options: HubFrameOptions,
	) -> Self {
		HubFrame {
			sidebar_scroll: 0,
			chip_ranges: Vec::new(),
			title: title.into(),
			sidebar_min: sidebar_bounds.0,
			sidebar_max: sidebar_bounds.1.max(sidebar_bounds.0),
			preserve_sidebar: options.preserve_sidebar,
			body_min_width: options.body_min_width,
		}
	}

	#[must_use]
	pub fn title(&self) -> &str {
		&self.title
	}

	#[must_use]
	pub fn sidebar_bounds(&self) -> (usize, usize) {
		(self.sidebar_min, self.sidebar_max)
	}

	#[must_use]
	pub fn body_min_width(&self) -> usize {
		self.body_min_width
	}

	#[must_use]
	pub fn preserve_sidebar(&self) -> bool {
		self.preserve_sidebar
	}

	/// Measure labels and annotations within this hub's sidebar width bounds.
	#[must_use]
	pub fn sidebar_width(&self, entries: &[SidebarEntry]) -> usize {
		let mut longest = 0;
		for entry in entries {
			let annotation = entry.annotation.as_deref().unwrap_or("");
			longest = longest.max(visible_width(&entry.label) + visible_width(annotation) + 5);
		}
		self.sidebar_min.min(self.sidebar_max).max(longest.min(self.sidebar_max))
	}

	/// Pan the sidebar within the current viewport's bounds.
	pub fn scroll_sidebar(&mut self, delta: isize, rows: usize, entry_count: usize) {
		let max_scroll = entry_count.saturating_sub(rows);
		let next = (self.sidebar_scroll as isize).saturating_add(delta).max(0) as usize;
		self.sidebar_scroll = next.min(max_scroll);
	}

	/// Select a footer chip at a frame-relative column, if one was painted
	/// there.
	pub fn select_chip_at<A>(&self, strip: &mut StripState<A>, col: usize) -> bool {
		for range in &self.chip_ranges {
			if col >= range.start && col < range.end {
				strip.index = range.index;
				return true;
			}
		}
		false
	}

	/// Whether the sidebar preserves its minimum width when the body floor
	/// would squeeze it.
	#[must_use]
	pub fn effective_body_min_width(&self, total_width: usize) -> Option<usize> {
		if !self.preserve_sidebar || self.body_min_width == 0 {
			return None;
		}
		let sidebar = self.sidebar_min;
		Some(
			self.body_min_width
				.min(total_width.saturating_sub(sidebar)),
		)
	}

	/// Render the sidebar viewport while retaining each hub's follow and
	/// clipping policy. `selection` is `(id, focused, follow, clamp)`.
	#[must_use]
	pub fn render_sidebar(
		&mut self,
		entries: &[SidebarEntry],
		width: usize,
		rows: usize,
		selection: (&str, bool, bool, bool),
		style: &dyn Fn(&SidebarEntry, usize) -> SidebarStyle,
		styler: &dyn HubFrameStyler,
	) -> Vec<String> {
		let (selected_id, focused, follow, clamp) = selection;
		if follow {
			let active_index = entries
				.iter()
				.position(|entry| entry.id == selected_id)
				.unwrap_or(0);
			if active_index < self.sidebar_scroll {
				self.sidebar_scroll = active_index;
			} else if active_index >= self.sidebar_scroll + rows {
				self.sidebar_scroll = active_index.saturating_sub(rows.saturating_sub(1));
			}
		}
		if clamp {
			self.sidebar_scroll = self
				.sidebar_scroll
				.min(entries.len().saturating_sub(rows));
		}
		let mut lines = Vec::new();
		let end = entries.len().min(self.sidebar_scroll + rows);
		for index in self.sidebar_scroll..end {
			let Some(entry) = entries.get(index) else {
				continue;
			};
			if entry.is_separator() {
				lines.push(styler.fg("border", &"─".repeat(width)));
				continue;
			}
			let active = entry.id == selected_id;
			let cursor = if active && focused {
				styler.fg("accent", &styler.symbol("nav.cursor"))
			} else {
				" ".to_string()
			};
			let decoration = style(entry, index);
			let label = if decoration.muted {
				styler.fg("dim", &entry.label)
			} else if active {
				styler.bold(&styler.fg("accent", &entry.label))
			} else {
				entry.label.clone()
			};
			let left = format!("{cursor} {} {label}", decoration.icon);
			let left_width = visible_width(&left);
			let ann_width = visible_width(&decoration.annotation);
			let mut line = if left_width + ann_width + 1 <= width {
				format!(
					"{left}{}{}",
					" ".repeat(width - left_width - ann_width),
					decoration.annotation
				)
			} else {
				let mut clipped = truncate_to_width(&left, width, Ellipsis::Omit);
				if decoration.pad_truncated {
					let clipped_width = visible_width(&clipped);
					if clipped_width < width {
						clipped.push_str(&" ".repeat(width - clipped_width));
					}
				}
				clipped
			};
			if decoration.hovered {
				line = styler.bg("selectedBg", &line);
			}
			lines.push(line);
		}
		lines
	}

	/// Render footer hints or a hub-specific strip, resetting stale mouse
	/// ranges.
	pub fn render_footer(
		&mut self,
		width: usize,
		hint: &str,
		render_strip: Option<&dyn Fn() -> String>,
		styler: &dyn HubFrameStyler,
	) -> String {
		self.chip_ranges.clear();
		match render_strip {
			Some(render) => render(),
			None => truncate_to_width(&styler.fg("dim", hint), width, Ellipsis::Omit),
		}
	}

	/// Render footer chips from a hub-selected horizontal window and record
	/// their hit ranges. `prefix` starts at column 2, as in the reference.
	pub fn render_chips<A>(
		&mut self,
		width: usize,
		prefix: &str,
		strip: &StripState<A>,
		start: usize,
		styler: &dyn HubFrameStyler,
	) -> String {
		let mut line = prefix.to_string();
		let mut col = 2 + visible_width(prefix);
		if start > 0 {
			line.push_str(&styler.fg("dim", "… "));
			col += 2;
		}
		for index in start..strip.chips.len() {
			let Some(chip) = strip.chips.get(index) else {
				continue;
			};
			let selected = index == strip.index;
			let body = format!(" {} ", chip.styled);
			let rendered = if selected {
				styler.bg(
					"selectedBg",
					&format!(
						"{}{body}{}",
						styler.fg("accent", "["),
						styler.fg("accent", "]")
					),
				)
			} else {
				body.clone()
			};
			let chip_width = visible_width(&body) + if selected { 2 } else { 0 };
			self.chip_ranges.push(ChipRange {
				start: col,
				end: col + chip_width,
				index,
			});
			line.push_str(&rendered);
			col += chip_width;
			line.push(' ');
			col += 1;
		}
		truncate_to_width(&line, width, Ellipsis::Omit)
	}

	/// Translate a measured frame geometry into footer, sidebar, and body
	/// coordinates.
	#[must_use]
	pub fn locate(&self, geometry: &HubGeometry) -> HubHit {
		let body_height = geometry.body_height;
		let mut content_line: isize = -1;
		let mut over_sidebar = false;
		let mut over_body = false;
		let mut footer_column = None;
		if let Some((child, row, col)) = geometry.stack_child {
			match child {
				1 => {
					if let Some((side, line)) = geometry.pane {
						match side {
							PaneSide::Left => {
								over_sidebar = true;
								content_line = line as isize;
							}
							PaneSide::Right => {
								over_body = true;
								content_line = line as isize;
							}
						}
					}
				}
				3 => footer_column = Some(col),
				_ => {
					let _ = row;
				}
			}
		}
		let over_content = content_line >= 0 && (content_line as usize) < body_height;
		HubHit {
			footer_column,
			body_height,
			content_line,
			over_sidebar: over_sidebar && over_content,
			over_body: over_body && over_content,
			body_line: content_line - 1,
		}
	}

	/// Rows available to the split for a fullscreen `height`.
	#[must_use]
	pub fn content_rows(height: usize) -> usize {
		height.saturating_sub(4).max(10)
	}

	/// Paint the complete fullscreen hub: title bar, split body (sidebar rows
	/// on the left, body rows on the right), divider, footer, and bottom rule.
	///
	/// The reference composes `SplitPane`/`Stack`/`PanelRows`; this port takes
	/// the already-rendered sidebar/body rows so the host keeps its own hub
	/// renderers, matching the reference constructor's injected closures.
	#[must_use]
	pub fn render(
		&self,
		width: usize,
		height: usize,
		entries: &[SidebarEntry],
		footer: &str,
		sidebar: &[String],
		body: &[String],
	) -> Vec<String> {
		let rows = Self::content_rows(height);
		let inner = width.saturating_sub(4).max(1);
		let left_width = self.sidebar_width(entries).min(inner.saturating_sub(4)).max(1);
		let right_width = inner.saturating_sub(left_width + 3).max(1);
		let title = truncate_to_width(&self.title, inner, Ellipsis::Unicode);
		let title_width = visible_width(&title);
		let top = format!(
			"╭─ {title} {}╮",
			"─".repeat(inner.saturating_sub(title_width + 1))
		);
		let rule = format!("├{}┤", "─".repeat(width.saturating_sub(2)));
		let bottom = format!("╰{}╯", "─".repeat(width.saturating_sub(2)));
		let mut lines = Vec::with_capacity(rows + 4);
		lines.push(top);
		for row in 0..rows {
			let left = pad_row(sidebar.get(row).map(String::as_str).unwrap_or(""), left_width);
			let right = pad_row(body.get(row).map(String::as_str).unwrap_or(""), right_width);
			lines.push(format!("│ {left} │ {right} │"));
		}
		lines.push(rule);
		lines.push(format!(
			"│ {} │ {} │",
			pad_row(footer, left_width),
			" ".repeat(right_width)
		));
		lines.push(bottom);
		lines
	}
}

/// Truncate `text` to `width` and pad it with spaces to exactly that width.
fn pad_row(text: &str, width: usize) -> String {
	let clipped = truncate_to_width(text, width, Ellipsis::Unicode);
	let pad = width.saturating_sub(visible_width(&clipped));
	format!("{clipped}{}", " ".repeat(pad))
}

/// Options of [`HubFrame::new`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HubFrameOptions {
	pub body_min_width: usize,
	pub preserve_sidebar: bool,
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Styler;

	impl HubFrameStyler for Styler {
		fn fg(&self, token: &str, text: &str) -> String {
			format!("<{token}>{text}</{token}>")
		}
		fn bg(&self, token: &str, text: &str) -> String {
			format!("[{token}]{text}[/{token}]")
		}
		fn bold(&self, text: &str) -> String {
			format!("*{text}*")
		}
		fn symbol(&self, key: &str) -> String {
			if key == "nav.cursor" {
				"❯".to_string()
			} else {
				String::new()
			}
		}
	}

	fn entries() -> Vec<SidebarEntry> {
		let mut first = SidebarEntry::new("a", "agent", "alpha");
		first.annotation = Some("3".to_string());
		vec![
			first,
			SidebarEntry::new("sep", SEPARATOR_KIND, ""),
			SidebarEntry::new("b", "agent", "beta"),
		]
	}

	#[test]
	fn strip_selection_cycles_and_wraps() {
		let mut strip = StripState::new(vec![
			StripChip::new("A", "A", "a"),
			StripChip::new("B", "B", "b"),
			StripChip::new("C", "C", "c"),
		]);
		assert!(move_strip_selection(&mut strip, "right"));
		assert_eq!(strip.index, 1);
		assert!(move_strip_selection(&mut strip, "shift+tab"));
		assert_eq!(strip.index, 0);
		assert!(move_strip_selection(&mut strip, "left"));
		assert_eq!(strip.index, 2);
		assert!(move_strip_selection(&mut strip, "tab"));
		assert_eq!(strip.index, 0);
		assert!(!move_strip_selection(&mut strip, "enter"));
	}

	#[test]
	fn sidebar_width_clamps_into_bounds() {
		let frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		let mut wide = SidebarEntry::new("x", "agent", &"x".repeat(80));
		wide.annotation = Some("annotation".to_string());
		assert_eq!(frame.sidebar_width(&[wide]), 30);
		assert_eq!(frame.sidebar_width(&[SidebarEntry::new("t", "agent", "ab")]), 10);
		assert_eq!(
			frame.sidebar_width(&[SidebarEntry {
				annotation: Some("123".to_string()),
				..SidebarEntry::new("t", "agent", "abcd")
			}]),
			12
		);
	}

	#[test]
	fn scroll_sidebar_clamps_to_entry_count() {
		let mut frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		frame.scroll_sidebar(5, 4, 10);
		assert_eq!(frame.sidebar_scroll, 5);
		frame.scroll_sidebar(99, 4, 10);
		assert_eq!(frame.sidebar_scroll, 6);
		frame.scroll_sidebar(-99, 4, 10);
		assert_eq!(frame.sidebar_scroll, 0);
	}

	#[test]
	fn sidebar_follow_keeps_the_selection_visible() {
		let mut frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		let entries: Vec<SidebarEntry> = (0..10)
			.map(|index| SidebarEntry::new(format!("e{index}"), "agent", format!("entry {index}")))
			.collect();
		let style = |_: &SidebarEntry, _: usize| SidebarStyle::default();
		let lines = frame.render_sidebar(
			&entries,
			12,
			3,
			("e6", true, true, true),
			&style,
			&Styler,
		);
		assert_eq!(frame.sidebar_scroll, 4);
		assert_eq!(lines.len(), 3);
		assert!(lines[0].contains("entry 4"));
		assert!(lines[2].contains("<accent>❯</accent>"));
		assert!(lines[2].contains("<accent>entry 6</accent>"));
	}

	#[test]
	fn sidebar_renders_separators_annotations_and_hover() {
		let mut frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		let style = |entry: &SidebarEntry, _: usize| SidebarStyle {
			icon: if entry.id == "a" { "●".to_string() } else { String::new() },
			annotation: entry.annotation.clone().unwrap_or_default(),
			hovered: entry.id == "b",
			..SidebarStyle::default()
		};
		let lines = frame.render_sidebar(&entries(), 12, 3, ("a", false, false, false), &style, &Styler);
		assert_eq!(lines.len(), 3);
		assert_eq!(lines[0], "  ● alpha  3");
		assert_eq!(lines[1], "<border>────────────</border>");
		assert!(lines[2].starts_with("[selectedBg]"));
	}

	#[test]
	fn chips_record_hit_ranges_from_column_two() {
		let mut frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		let strip = StripState::new(vec![
			StripChip::new("All", "All", "all"),
			StripChip::new("Mine", "Mine", "mine"),
		]);
		let line = frame.render_chips(80, "› ", &strip, 0, &Styler);
		assert!(line.starts_with("› "));
		assert_eq!(frame.chip_ranges.len(), 2);
		assert_eq!(frame.chip_ranges[0].index, 0);
		assert_eq!(frame.chip_ranges[0].start, 4);
		assert_eq!(frame.chip_ranges[0].end, 11);

		let mut selectable = strip;
		assert!(frame.select_chip_at(&mut selectable, 4));
		assert_eq!(selectable.index, 0);
		assert!(frame.select_chip_at(&mut selectable, 13));
		assert_eq!(selectable.index, 1);
		assert!(!frame.select_chip_at(&mut selectable, 0));
	}

	#[test]
	fn chips_from_a_window_prefix_an_ellipsis() {
		let mut frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		let strip = StripState::new(vec![
			StripChip::new("A", "A", "a"),
			StripChip::new("B", "B", "b"),
		]);
		let line = frame.render_chips(80, "", &strip, 1, &Styler);
		assert!(line.starts_with("<dim>… </dim>"));
		assert_eq!(frame.chip_ranges[0].start, 4);
		assert_eq!(frame.chip_ranges[0].index, 1);
	}

	#[test]
	fn render_footer_clears_stale_ranges() {
		let mut frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		frame.chip_ranges.push(ChipRange {
			start: 1,
			end: 2,
			index: 0,
		});
		let line = frame.render_footer(20, "esc close", None, &Styler);
		assert!(frame.chip_ranges.is_empty());
		assert_eq!(line, "<dim>esc close</dim>");
		let strip_line = frame.render_footer(20, "ignored", Some(&|| "strip".to_string()), &Styler);
		assert_eq!(strip_line, "strip");
	}

	#[test]
	fn locate_maps_footer_sidebar_and_body() {
		let frame = HubFrame::new("Hub", (10, 30), HubFrameOptions::default());
		let footer = frame.locate(&HubGeometry {
			stack_child: Some((3, 0, 17)),
			..HubGeometry::default()
		});
		assert_eq!(footer.footer_column, Some(17));

		let sidebar = frame.locate(&HubGeometry {
			stack_child: Some((1, 2, 4)),
			body_height: 10,
			pane: Some((PaneSide::Left, 2)),
		});
		assert!(sidebar.over_sidebar);
		assert!(!sidebar.over_body);
		assert_eq!(sidebar.content_line, 2);
		assert_eq!(sidebar.body_line, 1);

		let body = frame.locate(&HubGeometry {
			stack_child: Some((1, 0, 30)),
			body_height: 10,
			pane: Some((PaneSide::Right, 0)),
		});
		assert!(body.over_body);
		assert_eq!(body.body_line, -1);

		let outside = frame.locate(&HubGeometry {
			stack_child: Some((1, 0, 30)),
			body_height: 1,
			pane: Some((PaneSide::Right, 5)),
		});
		assert!(!outside.over_body);
		assert_eq!(outside.content_line, 5);
	}

	#[test]
	fn describe_sidebar_and_frame_shape() {
		let entries = entries();
		let sidebar = describe_hub_sidebar(
			&entries,
			"a",
			&|entry, _| (String::new(), entry.annotation.clone().unwrap_or_default(), false),
			"sidebar",
		);
		assert_eq!(sidebar.kind, NodeKind::List);
		assert_eq!(sidebar.props.get_str("selected"), Some("a"));
		assert_eq!(sidebar.props.get_i64("shrink"), Some(0));
		assert_eq!(sidebar.children.len(), 3);
		assert_eq!(sidebar.children[1].kind, NodeKind::Rule);

		let strip = StripState::new(vec![StripChip::new("All", "All", "all")]);
		let chips = describe_hub_chips(&strip, None, "chips");
		let tabs = &chips.children[0];
		assert_eq!(tabs.props.get_str("active"), Some("0"));
		assert_eq!(
			tabs.props
				.get("items")
				.and_then(|items| items.as_array())
				.and_then(|items| items.first())
				.and_then(|item| item.get("id"))
				.and_then(|id| id.as_str()),
			Some("0")
		);

		let frame = describe_hub_frame(
			"omp.overlay.hub",
			NodeText::Plain("Hub".to_string()),
			sidebar,
			node(NodeKind::Col, Props::new(), Vec::new(), Some("body")),
			node(NodeKind::Row, Props::new(), Vec::new(), Some("footer")),
		);
		assert_eq!(frame.props.get_str("role"), Some("omp.overlay.hub"));
		assert_eq!(frame.children.len(), 3);
		assert_eq!(frame.children[1].kind, NodeKind::Rule);
	}

	#[test]
	fn body_floor_shrinks_for_the_sidebar_only_when_preserved() {
		let frame = HubFrame::new(
			"Hub",
			(12, 30),
			HubFrameOptions {
				body_min_width: 40,
				preserve_sidebar: true,
			},
		);
		assert_eq!(frame.effective_body_min_width(60), Some(40));
		assert_eq!(frame.effective_body_min_width(20), Some(8));
		let plain = HubFrame::new("Hub", (12, 30), HubFrameOptions::default());
		assert_eq!(plain.effective_body_min_width(60), None);
	}

	#[test]
	fn render_composes_bordered_split_footer_and_bottom() {
		let frame = HubFrame::new("Agents", (12, 30), HubFrameOptions::default());
		let entries = entries();
		let sidebar = vec!["one".to_string(), "two".to_string()];
		let body = vec!["body a".to_string()];
		let lines = frame.render(80, 8, &entries, "hints", &sidebar, &body);
		assert_eq!(lines.len(), HubFrame::content_rows(8) + 4);
		assert!(lines[0].starts_with("╭─ Agents"));
		assert_eq!(crate::text_shape::visible_width(&lines[0]), 80);
		assert!(lines[1].contains("one"));
		assert!(lines[1].contains("body a"));
		assert!(lines[2].contains("two"));
		assert_eq!(lines[lines.len() - 3], format!("├{}┤", "─".repeat(78)));
		assert!(lines[lines.len() - 2].contains("hints"));
		assert_eq!(lines[lines.len() - 1], format!("╰{}╯", "─".repeat(78)));
		for line in &lines {
			assert_eq!(crate::text_shape::visible_width(line), 80, "row: {line:?}");
		}
	}
}
