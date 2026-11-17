//! Reference: `l/src/prompt/queued-messages.ts`.
//!
//! Messages queued while the agent works, above the editor. ANSI: a heading per
//! group, numbered dim rows and the dequeue hint. Natively: a stack of pills,
//! each `corner-down-right` · the text · the edit key, the total (`N queued`,
//! not a position) in the first pill when more than one waits; the edit control
//! sends `queue.edit`, answered like the dequeue key.
//!
//! Deviations:
//! - The reference reads the global `theme` singleton and `formatKeyHint`; the
//!   Rust port takes a [`Theme`] and formats the key through
//!   [`crate::native::overlay::format_tooltip_key`] (`key-hint-format.ts` has no
//!   port in this crate yet).
//! - `handleNativeEvent` becomes [`QueuedMessagesBand::handle_native_event`] over
//!   a local [`QueueUiEvent`] subset of the reference's `NativeUiEvent` union,
//!   matching the crate's per-module event convention (`native::picker`).

use serde_json::json;

use crate::component::Component;
use crate::components::{Container, Spacer, TruncatedText};
use crate::native::describe::{col, kbd, node, text, NativeDescribe};
use crate::native::node::{DescribeContext, NodeKind, NativeNode, Props, Text as NodeText};
use crate::native::overlay::format_tooltip_key;
use crate::render::render_utils::replace_tabs;
use crate::theme::Theme;

/// Queued messages under one heading ("Steering", "After yield").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedMessageGroup {
	pub label: String,
	pub messages: Vec<String>,
}

/// The action subset of `NativeUiEvent` this band answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueUiEvent {
	/// An action control was clicked; `act` is the action id.
	Action { act: String },
	/// Any other event; ignored.
	Other,
}

impl QueueUiEvent {
	/// The `queue.edit` action carried by `event`, if any.
	pub fn edit_action(event: &QueueUiEvent) -> bool {
		matches!(event, QueueUiEvent::Action { act } if act == "queue.edit")
	}
}

/// The band itself.
pub struct QueuedMessagesBand {
	container: Container,
	native: NativeNode,
	on_edit: Box<dyn Fn()>,
}

impl QueuedMessagesBand {
	/// Build the band. `dequeue_key` is a key id (`"alt+up"`), `theme` supplies
	/// the muted/dim colors and glyphs.
	pub fn new(
		groups: &[QueuedMessageGroup],
		dequeue_key: &str,
		theme: &Theme,
		on_edit: impl Fn() + 'static,
	) -> Self {
		let fg = |color: &str, value: &str| theme.fg(color, value).unwrap_or_else(|_| value.to_string());

		let mut container = Container::new();
		container.add_child(Spacer::new(1));
		for group in groups {
			let heading = format!("{}{}{}", group.label, theme.sep().dot, group.messages.len());
			container.add_child(padded_truncated(fg("muted", &heading)));
			for (index, message) in group.messages.iter().enumerate() {
				container.add_child(padded_truncated(fg(
					"dim",
					&format!(
						"  {}. {}",
						index + 1,
						replace_tabs(message).replace("\r\n", " ↵ ").replace('\n', " ↵ ")
					),
				)));
			}
		}
		container.add_child(padded_truncated(fg(
			"dim",
			&format!(
				"  {} {} to edit",
				theme.tree().hook,
				format_tooltip_key(dequeue_key)
			),
		)));

		let edit_title = format!("Edit  {}", format_tooltip_key(dequeue_key));
		let count: usize = groups.iter().map(|group| group.messages.len()).sum();
		let mut pills: Vec<NativeNode> = Vec::new();
		for group in groups {
			for (index, message) in group.messages.iter().enumerate() {
				let mut children: Vec<NativeNode> = Vec::with_capacity(4);
				let mut icon_props = Props::default();
				icon_props.set_str("name", "corner-down-right");
				children.push(node(NodeKind::Icon, icon_props, Vec::new(), Some("icon")));

				let mut text_props = Props::default();
				text_props.set_str(
					"text",
					collapse_newlines(&replace_tabs(message)),
				);
				text_props.set_str("wrap", "none");
				text_props.set_str("truncate", "end");
				text_props.set_i64("grow", 1);
				children.push(text(NodeText::Plain(String::new()), text_props));

				if pills.is_empty() && count > 1 {
					let mut badge_props = Props::default();
					badge_props.set_str("text", format!("{count} queued"));
					badge_props.set_str("title", format!("{count} messages queued"));
					badge_props.set_role("omp.queue.count");
					children.push(node(NodeKind::Badge, badge_props, Vec::new(), Some("count")));
				}

				let mut edit_props = Props::default();
				edit_props.set_role("omp.queue.edit");
				edit_props.set_str("gap", "xs");
				edit_props.set_str("align", "center");
				edit_props.set_str("title", edit_title.clone());
				edit_props.insert("actions", json!({ "click": "queue.edit" }));
				children.push(node(
					NodeKind::Row,
					edit_props,
					vec![
						kbd(dequeue_key, None),
						node(
							NodeKind::Text,
							{
								let mut props = Props::default();
								props.set_text("Edit");
								props
							},
							Vec::new(),
							None,
						),
					],
					Some("edit"),
				));

				let mut pill_props = Props::default();
				pill_props.set_role("omp.queue.item");
				pill_props.set_str("align", "center");
				pill_props.set_str("gap", "sm");
				pill_props.set_str("title", group.label.clone());
				pills.push(node(
					NodeKind::Row,
					pill_props,
					children,
					Some(&format!("{}/{}", group.label, index)),
				));
			}
		}
		let mut list_props = Props::default();
		list_props.set_role("omp.queue");
		list_props.set_str("gap", "xs");
		let native = col(pills, list_props);

		Self {
			container,
			native,
			on_edit: Box::new(on_edit),
		}
	}

	/// The pills' Edit control: the dequeue key's path (queued messages back
	/// into the editor).
	pub fn handle_native_event(&self, event: &QueueUiEvent) {
		if QueueUiEvent::edit_action(event) {
			(self.on_edit)();
		}
	}
}

impl Component for QueuedMessagesBand {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.container.render(width)
	}
}

impl NativeDescribe for QueuedMessagesBand {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		self.native.clone()
	}
}

fn padded_truncated(value: String) -> TruncatedText {
	let mut row = TruncatedText::new(value);
	row.set_padding(1, 0);
	row
}

/// `/\s*\r?\n\s*/g` → `" ↵ "`.
fn collapse_newlines(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut pending_space = false;
	let mut chars = text.chars().peekable();
	while let Some(ch) = chars.next() {
		if ch == '\n' {
			while out.ends_with(char::is_whitespace) {
				out.pop();
			}
			out.push_str(" ↵ ");
			pending_space = false;
			continue;
		}
		if ch == '\r' && chars.peek() == Some(&'\n') {
			continue;
		}
		if pending_space && !ch.is_whitespace() {
			out.push(' ');
			pending_space = false;
		}
		if ch.is_whitespace() {
			pending_space = true;
			continue;
		}
		out.push(ch);
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::create_theme;

	fn theme() -> Theme {
		create_theme("dark", None).expect("dark theme")
	}

	fn groups() -> Vec<QueuedMessageGroup> {
		vec![
			QueuedMessageGroup {
				label: "Steering".to_string(),
				messages: vec!["first\tline".to_string(), "two\nlines".to_string()],
			},
			QueuedMessageGroup {
				label: "After yield".to_string(),
				messages: vec!["third".to_string()],
			},
		]
	}

	#[test]
	fn renders_heading_numbered_rows_and_hint() {
		let mut band = QueuedMessagesBand::new(&groups(), "alt+up", &theme(), || {});
		let rows: Vec<String> = band
			.render(80)
			.into_iter()
			.map(|row| crate::ansi_scan::strip_ansi(&row).trim_end().to_string())
			.collect();
		assert_eq!(rows[0], "");
		assert!(rows[1].contains("Steering"), "rows {rows:?}");
		assert!(rows[1].ends_with('2'), "rows {rows:?}");
		assert!(rows[2].contains("1. first    line") || rows[2].contains("1. first"), "rows {rows:?}");
		assert!(rows[3].contains("2. two ↵ lines"), "rows {rows:?}");
		assert!(rows[4].contains("After yield"), "rows {rows:?}");
		assert!(rows[5].contains("3. third"), "rows {rows:?}");
		assert!(rows[6].contains("to edit"), "rows {rows:?}");
	}

	#[test]
	fn describes_pills_with_count_and_edit_action() {
		let band = QueuedMessagesBand::new(&groups(), "alt+up", &theme(), || {});
		let native = band.describe_native(&DescribeContext::default());
		assert_eq!(native.kind, NodeKind::Col);
		let json = serde_json::to_string(&native).expect("serialize");
		assert!(json.contains("omp.queue"), "native {json}");
		assert!(json.contains("3 queued"), "native {json}");
		assert!(json.contains("queue.edit"), "native {json}");
		assert_eq!(json.matches("omp.queue.edit").count(), 3);
	}

	#[test]
	fn edit_event_calls_handler() {
		use std::cell::Cell;
		use std::rc::Rc;

		let calls = Rc::new(Cell::new(0));
		let counter = Rc::clone(&calls);
		let band = QueuedMessagesBand::new(&groups(), "alt+up", &theme(), move || {
			counter.set(counter.get() + 1)
		});
		band.handle_native_event(&QueueUiEvent::Other);
		assert_eq!(calls.get(), 0);
		band.handle_native_event(&QueueUiEvent::Action {
			act: "queue.edit".to_string(),
		});
		assert_eq!(calls.get(), 1);
	}
}
