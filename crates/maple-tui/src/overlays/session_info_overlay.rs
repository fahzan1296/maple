//! Reference: `l/src/overlays/session-info-overlay.ts`.
//!
//! The focused, dismissible `/session` information panel: the session's
//! identity, its context-window meter, then the `/session` report split into
//! headed groups, counts laid out as a two-column stats grid.
//!
//! Deviations:
//! - The report string arrives already themed (ANSI). The port strips the
//!   escapes for parsing and re-applies semantic tokens (`mono`, `muted`,
//!   `path`) instead of carrying the original span styles, because the native
//!   sheet has no styled-span parser for terminal reports.
//! - `Text`/`ScrollView`'s TS layout is reused through
//!   [`crate::text_shape::wrap_text_with_ansi`] plus
//!   [`crate::components::ScrollView`]; the reference's width-probe fallback
//!   that reserves the scrollbar column is kept.
//! - Selection-cancel is matched with `Esc`/`Ctrl+C` directly instead of
//!   `matchesSelectCancel` (the crate's matcher needs a
//!   [`crate::keybindings::KeybindingsManager`], which this panel does not
//!   otherwise take).

use std::cell::RefCell;

use crate::component::Component;
use crate::components::scroll_view::ScrollView;
use crate::keys::Key;
use crate::native::describe::{col, kv, node, row, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{ActionButtonOptions, action_bar, action_button};
use crate::native::backend::NativeUiEvent;
use crate::native::NativeDescribe;
use crate::status_line::host::ContextUsage;
use crate::text_shape::truncate_to_width;

/// Panel chrome rows (top border, divider, footer, bottom border).
pub const PANEL_CHROME_ROWS: usize = 4;

/// Semantic role the terminal sheet reports.
pub const SESSION_INFO_ROLE: &str = "omp.overlay.sessionInfo";

/// One headed block of a key/value report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KvReportSection {
	/// Plain heading; `None` for the leading block.
	pub head: Option<String>,
	/// Entries in order: key/value pairs, or free lines that aren't pairs.
	pub entries: Vec<KvReportEntry>,
}

/// One entry of a [`KvReportSection`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KvReportEntry {
	/// A `Key: value` line.
	Pair {
		/// The key, without its colon.
		key: String,
		/// The plain value text.
		value: String,
	},
	/// A free line that is not a pair.
	Line(String),
}

/// Remove ANSI SGR/CSI escapes, leaving the drawn characters.
fn strip_ansi(input: &str) -> String {
	let mut out = String::with_capacity(input.len());
	let mut chars = input.chars().peekable();
	while let Some(ch) = chars.next() {
		if ch != '\x1b' {
			out.push(ch);
			continue;
		}
		match chars.next() {
			Some('[') => {
				// CSI: parameter/intermediate bytes, then one final byte.
				for next in chars.by_ref() {
					if ('\x40'..='\x7e').contains(&next) {
						break;
					}
				}
			}
			Some(']') => {
				// OSC: terminated by BEL or ST.
				for next in chars.by_ref() {
					if next == '\x07' {
						break;
					}
				}
			}
			_ => {}
		}
	}
	out
}

/// Parse a report of bold headings and `Key: value` lines (the `/session`
/// info format) into sections. Blank lines only separate.
#[must_use]
pub fn parse_kv_report(report: &str) -> Vec<KvReportSection> {
	let mut sections = vec![KvReportSection::default()];
	for raw in report.split('\n') {
		let plain = strip_ansi(raw);
		let trimmed = plain.trim();
		if trimmed.is_empty() {
			continue;
		}
		let entry = match plain.split_once(':') {
			Some((key, value)) if !key.trim().is_empty() => Some(KvReportEntry::Pair {
				key: key.trim().to_string(),
				value: value.trim().to_string(),
			}),
			_ => None,
		};
		let section = sections.last_mut().expect("at least one section");
		match entry {
			Some(entry) => section.entries.push(entry),
			None if !plain.contains(':') => {
				sections.push(KvReportSection {
					head: Some(trimmed.to_string()),
					entries: Vec::new(),
				});
			}
			None => section.entries.push(KvReportEntry::Line(trimmed.to_string())),
		}
	}
	sections
		.into_iter()
		.filter(|section| section.head.is_some() || !section.entries.is_empty())
		.collect()
}

/// A report key or heading in sentence case: `Tool Calls` → `Tool calls`;
/// acronyms (`MCP`, `ACU`) stay.
#[must_use]
pub fn sentence_case(label: &str) -> String {
	let mut out = String::with_capacity(label.len());
	let mut first = true;
	let mut word = String::new();
	let flush = |out: &mut String, word: &mut String, first: &mut bool| {
		if word.is_empty() {
			return;
		}
		let keep = *first
			|| !(word.len() > 1
				&& word.chars().next().is_some_and(|c| c.is_ascii_uppercase())
				&& word.chars().skip(1).all(|c| c.is_ascii_lowercase()));
		if keep {
			out.push_str(word);
		} else {
			out.push_str(&word.to_ascii_lowercase());
		}
		*first = false;
		word.clear();
	};
	for ch in label.chars() {
		if ch.is_ascii_alphabetic() {
			word.push(ch);
		} else {
			flush(&mut out, &mut word, &mut first);
			out.push(ch);
		}
	}
	flush(&mut out, &mut word, &mut first);
	out
}

/// A figure the stats grid can right-align: `182,440`, `0.4127`, `$1.20`,
/// `21%`.
#[must_use]
pub fn is_numeric_figure(value: &str) -> bool {
	let trimmed = value.trim();
	let digits = trimmed
		.strip_prefix('$')
		.or_else(|| trimmed.strip_prefix('€'))
		.or_else(|| trimmed.strip_prefix('£'))
		.unwrap_or(trimmed);
	let digits = digits.strip_suffix('%').unwrap_or(digits);
	if digits.is_empty() || !digits.chars().any(|c| c.is_ascii_digit()) {
		return false;
	}
	let mut dot = false;
	for ch in digits.chars() {
		match ch {
			'0'..='9' | ',' => {}
			'.' if !dot => dot = true,
			_ => return false,
		}
	}
	true
}

/// One line of the identity block: a sans label, then the value. With `copy`
/// (its tooltip) the value and a copy glyph after it are one target: a click
/// copies the value.
fn identity_field(label: &str, value: &str, style: &str, key: &str, copy: Option<&str>) -> NativeNode {
	let mut shown_props = Props::new();
	shown_props.set_str("truncate", "middle");
	shown_props.set_i64("grow", 1);
	shown_props.set_i64("shrink", 1);
	let shown = text(NodeText::Spans(vec![span(value, Some(style))]), shown_props);

	let mut label_props = Props::new();
	label_props.set_role("omp.info.label");
	label_props.set_str("wrap", "none");
	label_props.set_i64("shrink", 0);
	let label = text(NodeText::Spans(vec![span(label, None)]), label_props);

	let mut row_props = Props::new();
	row_props.set_i64("gap", 2);
	row_props.set_str("align", "center");
	row_props.set_role("omp.info.field");
	let field = match copy {
		Some(tooltip) => {
			let mut copy_props = Props::new();
			copy_props.set_i64("gap", 1);
			copy_props.set_str("align", "center");
			copy_props.set_i64("grow", 1);
			copy_props.set_i64("shrink", 1);
			copy_props.set_role("omp.info.copy");
			copy_props.insert("actions", serde_json::json!({ "click": "copy" }));
			copy_props.set_str("title", tooltip);
			let mut icon_props = Props::new();
			icon_props.set_str("name", "copy");
			icon_props.set_role("omp.info.copy.ic");
			icon_props.set_str("aria", tooltip);
			let icon = node(NodeKind::Icon, icon_props, Vec::new(), None);
			row(vec![shown, icon], copy_props)
		}
		None => shown,
	};
	node(NodeKind::Row, row_props, vec![label, field], Some(key))
}

/// Terminal surface needed to size the session info viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionInfoOverlayHost {
	/// Rows the terminal has.
	pub terminal_rows: usize,
}

/// Focused, dismissible `/session` information panel.
pub struct SessionInfoOverlay {
	host: SessionInfoOverlayHost,
	on_close: Box<dyn FnMut()>,
	context: Option<ContextUsage>,
	info_source: String,
	scroll_view: ScrollView,
	cached: RefCell<Option<(bool, NativeNode)>>,
	last_info_width: Option<usize>,
	last_info_lines: Option<usize>,
	last_height: Option<usize>,
}

impl std::fmt::Debug for SessionInfoOverlay {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SessionInfoOverlay")
			.field("info_source", &self.info_source)
			.field("has_context", &self.context.is_some())
			.finish()
	}
}

impl SessionInfoOverlay {
	/// `context` adds a context-window meter to the native sheet.
	pub fn new(
		host: SessionInfoOverlayHost,
		info: impl Into<String>,
		on_close: impl FnMut() + 'static,
		context: Option<ContextUsage>,
	) -> Self {
		SessionInfoOverlay {
			host,
			on_close: Box::new(on_close),
			context,
			info_source: info.into(),
			scroll_view: ScrollView::new(Vec::new(), 0),
			cached: RefCell::new(None),
			last_info_width: None,
			last_info_lines: None,
			last_height: None,
		}
	}

	/// The sheet title.
	pub fn title(&self) -> &'static str {
		"Session Info"
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &'static str {
		SESSION_INFO_ROLE
	}

	/// The raw report the panel parses.
	pub fn info(&self) -> &str {
		&self.info_source
	}

	/// The scroll offset of the body viewport.
	pub fn scroll_offset(&self) -> usize {
		self.scroll_view.scroll_offset()
	}

	/// Close the panel through the host callback.
	fn close(&mut self) {
		(self.on_close)();
	}

	/// The body, in three tiers: who the session is, how full its context
	/// window is, then the report's sections as headed groups. Groups of plain
	/// counts share a two-column stats grid; the rest are key/value rows.
	fn describe_body(&self, meter: bool) -> NativeNode {
		// The report prints the append-only context mode after the message
		// counts; it is a provider setting, so it joins the Provider group.
		let parsed = parse_kv_report(&self.info_source);
		let has_provider = parsed
			.iter()
			.any(|section| section.head.as_deref() == Some("Provider"));
		let mut moved: Vec<KvReportEntry> = Vec::new();
		let sections: Vec<KvReportSection> = parsed
			.into_iter()
			.map(|section| KvReportSection {
				head: section.head.clone(),
				entries: section
					.entries
					.into_iter()
					.filter(|entry| {
						let relocate = has_provider
							&& section.head.as_deref() != Some("Provider")
							&& matches!(entry, KvReportEntry::Pair { key, .. } if key == "Append-Only");
						if relocate {
							moved.push(entry.clone());
						}
						!relocate
					})
					.collect(),
			})
			.collect();
		let mut body: Vec<NativeNode> = Vec::new();
		let mut stats: Vec<(String, String, Vec<(String, String)>)> = Vec::new();
		let flush_stats = |body: &mut Vec<NativeNode>, stats: &mut Vec<(String, String, Vec<(String, String)>)>| {
			if !stats.is_empty() {
				body.push(describe_stats(&mut stats.drain(..).collect::<Vec<_>>()));
			}
		};
		for (index, section) in sections.iter().enumerate() {
			let Some(head) = section.head.as_deref() else {
				let mut identity: Vec<NativeNode> = Vec::new();
				let mut rest: Vec<NativeNode> = Vec::new();
				for entry in &section.entries {
					match entry {
						KvReportEntry::Line(line) => {
							let mut props = Props::new();
							props.set_str("wrap", "word");
							props.set_role("omp.info.note");
							rest.push(text(NodeText::plain(line.clone()), props));
						}
						KvReportEntry::Pair { key, value } if key == "ID" => {
							identity.insert(0, identity_field("ID", value, "mono", "id", Some("Copy session ID")));
						}
						KvReportEntry::Pair { key, value } if key == "File" => {
							let (style, copy) = if value.contains('/') {
								("path", Some(format!("Copy {value}")))
							} else {
								("muted", None)
							};
							identity.push(identity_field("File", value, style, "file", copy.as_deref()));
						}
						KvReportEntry::Pair { key, value } => {
							if let Some(kv_node) = kv(vec![(
								key.clone(),
								NodeText::Spans(vec![span(value.clone(), None)]),
							)]) {
								rest.push(kv_node);
							}
						}
					}
				}
				if !identity.is_empty() {
					let mut props = Props::new();
					props.set_role("omp.info.identity");
					body.push(node(NodeKind::Col, props, identity, Some("identity")));
				}
				for entry in moved.iter() {
					if let KvReportEntry::Pair { key, value } = entry {
						if let Some(kv_node) = kv(vec![(
							key.clone(),
							NodeText::Spans(vec![span(value.clone(), None)]),
						)]) {
							rest.push(kv_node);
						}
					}
				}
				body.append(&mut rest);
				if let Some(context) = self.context.filter(|context| context.context_window > 0) {
					body.push(describe_context(context, meter));
				}
				continue;
			};
			let pairs: Vec<(&String, &String)> = section
				.entries
				.iter()
				.filter_map(|entry| match entry {
					KvReportEntry::Pair { key, value } => Some((key, value)),
					KvReportEntry::Line(_) => None,
				})
				.collect();
			if !pairs.is_empty()
				&& pairs.len() == section.entries.len()
				&& pairs.iter().all(|(_, value)| is_numeric_figure(value))
			{
				let cost = head == "Cost";
				let items = pairs
					.iter()
					.map(|(key, value)| {
						let shown = if cost && key.as_str() == "Total" {
							format!("${}", value.trim())
						} else {
							value.trim().to_string()
						};
						(sentence_case(key), shown)
					})
					.collect();
				stats.push((head.to_string(), format!("s{index}"), items));
				continue;
			}
			flush_stats(&mut body, &mut stats);
			let mut section_props = Props::new();
			section_props.set_str("head", sentence_case(head));
			section_props.set_role("omp.info.group");
			body.push(node(
				NodeKind::Section,
				section_props,
				describe_entries(&section.entries, head == "Provider"),
				Some(&format!("s{index}")),
			));
		}
		flush_stats(&mut body, &mut stats);
		let button = action_button(
			"Close",
			"close",
			ActionButtonOptions {
				keys: Some("escape".to_string()),
				..ActionButtonOptions::default()
			},
		);
		body.push(action_bar(&[None, Some(button)], "actions"));
		let mut props = Props::new();
		props.set_i64("gap", 2);
		col(body, props)
	}
}

/// Key/value rows, free lines as notes between them. The provider's values are
/// machine strings (`mono`); elsewhere the keys are names.
fn describe_entries(entries: &[KvReportEntry], mono: bool) -> Vec<NativeNode> {
	let mut children = Vec::new();
	let mut items: Vec<(String, NodeText)> = Vec::new();
	let flush = |children: &mut Vec<NativeNode>, items: &mut Vec<(String, NodeText)>| {
		if items.is_empty() {
			return;
		}
		let mut kv_props = Props::new();
		let pairs: Vec<(String, NodeText)> = std::mem::take(items);
		let mut values = Vec::new();
		for (key, value) in pairs {
			let mut item = serde_json::Map::new();
			item.insert(
				"k".to_string(),
				serde_json::Value::Array(vec![span(key, Some("muted")).to_value()]),
			);
			item.insert("v".to_string(), value.to_value());
			values.push(serde_json::Value::Object(item));
		}
		kv_props.insert("items", serde_json::Value::Array(values));
		kv_props.set_str("layout", "grid");
		kv_props.set_role(if mono { "omp.info.kv" } else { "omp.info.names" });
		children.push(node(NodeKind::Kv, kv_props, Vec::new(), None));
	};
	for entry in entries {
		match entry {
			KvReportEntry::Line(line) => {
				flush(&mut children, &mut items);
				let mut props = Props::new();
				props.set_str("wrap", "word");
				props.set_role("omp.info.note");
				children.push(text(NodeText::plain(line.clone()), props));
			}
			KvReportEntry::Pair { key, value } => {
				let label = if mono { sentence_case(key) } else { key.clone() };
				let style = if mono { "mono" } else { "default" };
				items.push((
					label,
					NodeText::Spans(vec![span(value.clone(), Some(style))]),
				));
			}
		}
	}
	flush(&mut children, &mut items);
	children
}

/// Count groups as a two-column grid that stacks when narrow: each group goes
/// to the shorter column (report order holds when stacked), its Total last
/// over a hairline.
fn describe_stats(groups: &[(String, String, Vec<(String, String)>)]) -> NativeNode {
	let mut columns: [Vec<NativeNode>; 2] = [Vec::new(), Vec::new()];
	let mut lines = [0usize; 2];
	for (head, key, items) in groups {
		let total = if items.len() > 1 {
			items.iter().find(|(label, _)| label == "Total").cloned()
		} else {
			None
		};
		let body_items: Vec<(String, String)> = items
			.iter()
			.filter(|item| total.as_ref() != Some(item))
			.cloned()
			.collect();
		let mut children = vec![kv_grid(&body_items, "omp.info.nums")];
		if let Some(total) = &total {
			children.push(kv_grid(std::slice::from_ref(total), "omp.info.total"));
		}
		// Ties go right, so the stacked order (left, then right) stays the
		// report's.
		let column = if lines[0] == 0 || lines[0] < lines[1] { 0 } else { 1 };
		lines[column] += items.len() + 2;
		let mut props = Props::new();
		props.set_str("head", sentence_case(head));
		props.set_role("omp.info.group");
		columns[column].push(node(NodeKind::Section, props, children, Some(key)));
	}
	let mut props = Props::new();
	props.set_str("align", "start");
	props.set_i64("wrap", 1);
	props.set_role("omp.info.stats");
	let cols: Vec<NativeNode> = columns
		.into_iter()
		.enumerate()
		.filter(|(_, children)| !children.is_empty())
		.map(|(index, children)| {
			let mut props = Props::new();
			props.set_role("omp.info.column");
			node(NodeKind::Col, props, children, Some(&format!("c{index}")))
		})
		.collect();
	node(NodeKind::Row, props, cols, Some("stats"))
}

fn kv_grid(items: &[(String, String)], role: &str) -> NativeNode {
	let mut props = Props::new();
	props.set_str("layout", "grid");
	props.set_role(role);
	let values: Vec<serde_json::Value> = items
		.iter()
		.map(|(key, value)| {
			serde_json::json!({
				"k": [span(key.clone(), Some("muted")).to_value()],
				"v": [span(value.clone(), Some("mono")).to_value()],
			})
		})
		.collect();
	props.insert("items", serde_json::Value::Array(values));
	node(NodeKind::Kv, props, Vec::new(), None)
}

/// The context window: a head row (label, `41K / 200K`, the share) over a
/// full-width meter.
fn describe_context(context: ContextUsage, meter: bool) -> NativeNode {
	let percent = context.percent.unwrap_or(0.0);
	let value = (percent / 100.0).clamp(0.0, 1.0);
	let title = format!(
		"{} of {} tokens",
		format_thousands(context.tokens),
		format_thousands(context.context_window)
	);
	let mut label_props = Props::new();
	label_props.set_i64("grow", 1);
	label_props.set_i64("shrink", 1);
	label_props.set_str("truncate", "end");
	let label = text(
		NodeText::Spans(vec![span("Context window", None)]),
		label_props,
	);
	let mut figure_props = Props::new();
	figure_props.set_str("wrap", "none");
	figure_props.set_i64("shrink", 0);
	let figure = text(
		NodeText::Spans(vec![
			span(
				format!(
					"{} / {}",
					format_thousands(context.tokens),
					format_thousands(context.context_window)
				),
				Some("mono muted"),
			),
			span("  ", None),
			span(format!("{}%", percent.round()), Some("mono strong")),
		]),
		figure_props,
	);
	let mut head_props = Props::new();
	head_props.set_str("align", "baseline");
	head_props.set_role("omp.info.contextHead");
	let head = node(NodeKind::Row, head_props, vec![label, figure], Some("head"));

	let mut gauge_props = Props::new();
	gauge_props.insert("value", serde_json::json!(value));
	gauge_props.set_str("title", title.clone());
	let gauge = if meter {
		gauge_props.set_str("style", "bar");
		gauge_props.set_str("size", "md");
		gauge_props.insert("thresholds", serde_json::json!({ "warn": 0.7, "bad": 0.9 }));
		node(NodeKind::Meter, gauge_props, Vec::new(), Some("meter"))
	} else {
		node(NodeKind::Progress, gauge_props, Vec::new(), Some("meter"))
	};
	let mut props = Props::new();
	props.set_role("omp.info.context");
	props.set_str("title", title.clone());
	node(NodeKind::Col, props, vec![head, gauge], Some("context"))
}

/// `182440` → `182,440`.
fn format_thousands(value: u64) -> String {
	let digits = value.to_string();
	let mut out = String::with_capacity(digits.len() + digits.len() / 3);
	for (index, ch) in digits.chars().enumerate() {
		if index > 0 && (digits.len() - index) % 3 == 0 {
			out.push(',');
		}
		out.push(ch);
	}
	out
}

impl NativeDescribe for SessionInfoOverlay {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let meter = cx.supports(NodeKind::Meter);
		if let Some((cached_meter, cached)) = self.cached.borrow().as_ref() {
			if *cached_meter == meter {
				return cached.clone();
			}
		}
		let described = self.describe_body(meter);
		*self.cached.borrow_mut() = Some((meter, described.clone()));
		described
	}
}

impl SessionInfoOverlay {
	/// The Close button runs Esc's path.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		if let NativeUiEvent::Action { act, .. } = event {
			if act == "close" {
				self.close();
			}
		}
	}
}

impl Component for SessionInfoOverlay {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner_width = (width as usize).saturating_sub(4).max(1);
		let footer_hint = format!("↑/↓ scroll · {} close", self.key_label());
		let max_body_height = self.host.terminal_rows.saturating_sub(PANEL_CHROME_ROWS).max(1);
		let full_width_lines = crate::text_shape::wrap_text_with_ansi(&self.info_source, inner_width);
		let info_width = if full_width_lines.len() > max_body_height {
			inner_width.saturating_sub(1).max(1)
		} else {
			inner_width
		};
		if self.last_info_width != Some(info_width) {
			let lines = if info_width == inner_width {
				full_width_lines
			} else {
				crate::text_shape::wrap_text_with_ansi(&self.info_source, info_width)
			};
			self.last_info_lines = Some(lines.len());
			self.scroll_view.set_lines(lines);
			self.last_info_width = Some(info_width);
		}
		let total = self.last_info_lines.unwrap_or(0);
		let height = total.clamp(1, max_body_height);
		if self.last_height != Some(height) {
			self.scroll_view.set_height(height);
			self.last_height = Some(height);
		}
		let body = self.scroll_view.render(inner_width as u16);
		let mut rows = vec![crate::chrome::overlay_box::top_border(
			width,
			"Session Info",
			&crate::chrome::overlay_box::OverlayTheme::default(),
		)];
		for line in body {
			rows.push(crate::chrome::overlay_box::row(
				&line,
				width,
				&crate::chrome::overlay_box::OverlayTheme::default(),
			));
		}
		rows.push(crate::chrome::overlay_box::divider(
			width,
			&crate::chrome::overlay_box::OverlayTheme::default(),
		));
		rows.push(crate::chrome::overlay_box::row(
			&truncate_to_width(&footer_hint, inner_width, crate::text_shape::Ellipsis::Unicode),
			width,
			&crate::chrome::overlay_box::OverlayTheme::default(),
		));
		rows.push(crate::chrome::overlay_box::bottom_border(
			width,
			&crate::chrome::overlay_box::OverlayTheme::default(),
		));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		if matches!(key, Key::Esc | Key::Ctrl('c')) {
			self.close();
			return;
		}
		self.scroll_view.handle_input(key);
	}
}

impl SessionInfoOverlay {
	/// The key that closes the panel, for the footer hint.
	fn key_label(&self) -> &'static str {
		"esc"
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_report_splits_into_headed_sections() {
		let report = "ID: abc123\nFile: /tmp/session.jsonl\n\nProvider\nName: Anthropic\nModel: opus\n\nMessages\nUser: 12\nAssistant: 40\n";
		let sections = parse_kv_report(report);
		assert_eq!(sections.len(), 3);
		assert_eq!(sections[0].head, None);
		assert_eq!(
			sections[0].entries[0],
			KvReportEntry::Pair {
				key: "ID".to_string(),
				value: "abc123".to_string()
			}
		);
		assert_eq!(sections[1].head.as_deref(), Some("Provider"));
		assert_eq!(sections[1].entries.len(), 2);
		assert_eq!(sections[2].head.as_deref(), Some("Messages"));
	}

	#[test]
	fn ansi_escapes_are_stripped_before_parsing() {
		let report = "\x1b[2mID:\x1b[39m \x1b[36mabc\x1b[39m\n\x1b[1mCost\x1b[22m\n\x1b[2mTotal:\x1b[39m $1.20\n";
		let sections = parse_kv_report(report);
		assert_eq!(sections.len(), 2);
		assert_eq!(
			sections[0].entries[0],
			KvReportEntry::Pair {
				key: "ID".to_string(),
				value: "abc".to_string()
			}
		);
		assert_eq!(sections[1].head.as_deref(), Some("Cost"));
		assert_eq!(
			sections[1].entries[0],
			KvReportEntry::Pair {
				key: "Total".to_string(),
				value: "$1.20".to_string()
			}
		);
	}

	#[test]
	fn free_lines_stay_notes() {
		let sections = parse_kv_report("ID: a1\nno server configured\nMCP\n");
		assert_eq!(
			sections[0].entries[1],
			KvReportEntry::Line("no server configured".to_string())
		);
		assert!(sections[1].entries.is_empty());
	}

	#[test]
	fn numeric_figures_are_recognised() {
		assert!(is_numeric_figure("182,440"));
		assert!(is_numeric_figure("0.4127"));
		assert!(is_numeric_figure("$1.20"));
		assert!(is_numeric_figure("21%"));
		assert!(!is_numeric_figure("1.2.3"));
		assert!(!is_numeric_figure("abc"));
		assert!(!is_numeric_figure(","));
	}

	#[test]
	fn headings_lose_capital_letters_but_acronyms_stay() {
		assert_eq!(sentence_case("Tool Calls"), "Tool calls");
		assert_eq!(sentence_case("MCP Servers"), "MCP Servers");
		assert_eq!(sentence_case("Cost"), "Cost");
		assert_eq!(sentence_case("Tokens (ACU)"), "Tokens (ACU)");
	}

	#[test]
	fn totals_are_set_off_in_the_cost_grid() {
		let stats = describe_stats(&[(
			"Cost".to_string(),
			"s3".to_string(),
			vec![
				("Input".to_string(), "0.10".to_string()),
				("Total".to_string(), "$1.20".to_string()),
			],
		)]);
		assert_eq!(stats.kind, NodeKind::Row);
		let text = serde_json::to_string(&stats.props).unwrap();
		assert!(text.contains("Total"), "{text}");
	}

	#[test]
	fn the_close_button_reports_close() {
		let closes = std::rc::Rc::new(std::cell::RefCell::new(0));
		let sink = std::rc::Rc::clone(&closes);
		let mut overlay = SessionInfoOverlay::new(
			SessionInfoOverlayHost { terminal_rows: 24 },
			"ID: a1",
			move || *sink.borrow_mut() += 1,
			None,
		);
		overlay.handle_native_event(&NativeUiEvent::Action {
			key: Vec::new(),
			act: "close".to_string(),
			value: None,
			mods: Vec::new(),
		});
		assert_eq!(*closes.borrow(), 1);
		overlay.handle_input(&Key::Esc);
		assert_eq!(*closes.borrow(), 2);
	}

	#[test]
	fn identity_keeps_the_session_id_first() {
		let overlay = SessionInfoOverlay::new(
			SessionInfoOverlayHost { terminal_rows: 40 },
			"File: /tmp/s.jsonl\nID: abc\nOrg: Acme\n",
			|| {},
			None,
		);
		let mut cx = DescribeContext::new(80);
		cx.enable_feature("meter");
		let described = overlay.describe_native(&cx);
		let identity = described
			.children
			.iter()
			.find(|child| child.key.as_deref() == Some("identity"))
			.expect("identity");
		assert_eq!(identity.children[0].key.as_deref(), Some("id"));
		assert_eq!(identity.children[1].key.as_deref(), Some("file"));
	}
}
