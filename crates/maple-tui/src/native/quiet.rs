//! Describe-time gate for the animations a TSP terminal clocks itself.
//!
//! The protocol has no "static spinner" or "no effects" knob, so the motion
//! switches are honoured on the way to the wire: with static spinners a
//! `spinner` node becomes its frame-0 glyph (label and common props kept), and
//! with decorative motion off every `fx` is dropped.
//!
//! Reference: `l/src/native/quiet.ts`.
//!
//! Deviations:
//! - The reference reads two process-global switches (`spinnerAnimated()`,
//!   `motionEffectsEnabled()`) and memoizes translations in a `WeakMap`.
//!   maple-tui has no global UI state, so [`QuietSwitches`] is passed in and the
//!   memo is dropped: the transform is pure, so identical input yields
//!   identical output.
//! - The reference skips children that are components (it checks for a `render`
//!   function). Here children are already resolved plain nodes, so there is
//!   nothing to skip.
//! - `quiet_motion` returns `None` when the node is untouched, which carries the
//!   reference's identity-preservation contract (the reconciler memoizes on it).

use serde_json::{Map, Value};
use crate::native::node::{NativeNode, NodeKind, Props, Span, Text};

/// The two motion switches the reference reads from its global UI state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuietSwitches {
	/// Spinners are pinned to their frame-0 glyph.
	pub still_spinners: bool,
	/// Decorative per-span effects (`fx`) are dropped.
	pub no_effects: bool,
}

impl QuietSwitches {
	/// Whether either switch changes the description at all.
	pub fn is_quietening(self) -> bool {
		self.still_spinners || self.no_effects
	}
}

/// `node` as described, or with its terminal-clocked animations stilled.
///
/// `None` means the node is returned unchanged, exactly as the reference does
/// when neither switch applies or nothing below the node carries an effect.
pub fn quiet_motion(node: &NativeNode, switches: QuietSwitches) -> Option<NativeNode> {
	if !switches.is_quietening() {
		return None;
	}
	quiet(node, switches)
}

fn quiet(node: &NativeNode, switches: QuietSwitches) -> Option<NativeNode> {
	let quiet_props = match switches.no_effects {
		true => without_effects_props(&node.props),
		false => None,
	};
	let quiet_children = match node.children.is_empty() {
		true => None,
		false => quiet_children(&node.children, switches),
	};
	if switches.still_spinners && node.kind == NodeKind::Spinner {
		return Some(still_spinner(node, quiet_props));
	}
	if quiet_props.is_none() && quiet_children.is_none() {
		return None;
	}
	let mut out = node.clone();
	if let Some(props) = quiet_props {
		out.props = props;
	}
	if let Some(children) = quiet_children {
		out.children = children;
	}
	Some(out)
}

/// Quiet one child slot; `None` when the child is unchanged.
fn quiet_child(child: &NativeNode, switches: QuietSwitches) -> Option<NativeNode> {
	quiet(child, switches)
}

/// Quiet the child list; `None` when nothing below it changed.
fn quiet_children(children: &[NativeNode], switches: QuietSwitches) -> Option<Vec<NativeNode>> {
	let mut out: Option<Vec<NativeNode>> = None;
	for (index, child) in children.iter().enumerate() {
		if let Some(next) = quiet_child(child, switches) {
			out.get_or_insert_with(|| children.to_vec())[index] = next;
		}
	}
	out
}

/// A `spinner` as the static text it would show at frame 0: glyph, then its
/// label, under its common props. Starburst's first facet matches the ANSI
/// thinking pulse's frame 0; every other style shows the braille frame 0 the
/// ANSI spinners pin to.
fn still_spinner(node: &NativeNode, props: Option<Props>) -> NativeNode {
	let style = node.props.get_str("style").unwrap_or_default();
	let mut common = match props {
		Some(props) => props,
		None => node.props.clone(),
	};
	common.remove("style");
	common.remove("label");

	let mut spans = vec![Span::new(if style == "starburst" { "✻" } else { "⠋" })];
	match node.props.get("label") {
		Some(Value::String(label)) if !label.is_empty() => {
			spans.push(Span::new(format!(" {}", label)));
		}
		Some(Value::Array(runs)) if !runs.is_empty() => {
			spans.push(Span::new(" "));
			spans.extend(runs.iter().filter_map(Span::from_value));
		}
		_ => {}
	}
	common.insert("spans".to_string(), Text::Spans(spans).to_value());

	NativeNode {
		kind: NodeKind::Text,
		props: common,
		children: Vec::new(),
		key: node.key.clone(),
		reveal: node.reveal.clone(),
		scroll: node.scroll.clone(),
	}
}

/// `props` with `fx` removed from every plain object carrying one — spans in any
/// `TspText`, editor decorations, whatever a future prop nests. Copy-on-write:
/// `None` comes back when nothing below it had an effect.
fn without_effects_props(props: &Props) -> Option<Props> {
	let mut out: Option<Props> = None;
	for (key, value) in props.iter() {
		if key == "fx" {
			out.get_or_insert_with(|| props.clone()).remove("fx");
			continue;
		}
		if let Some(next) = without_effects_value(value) {
			out.get_or_insert_with(|| props.clone())
				.insert(key.clone(), next);
		}
	}
	out
}

/// `value` with `fx` removed from every plain object carrying one, walking
/// arrays and objects alike. Copy-on-write.
fn without_effects_value(value: &Value) -> Option<Value> {
	match value {
		Value::Array(items) => {
			let mut out: Option<Vec<Value>> = None;
			for (index, item) in items.iter().enumerate() {
				if let Some(next) = without_effects_value(item) {
					out.get_or_insert_with(|| items.clone())[index] = next;
				}
			}
			out.map(Value::Array)
		}
		Value::Object(map) => {
			let mut out: Option<Map<String, Value>> = None;
			for (key, item) in map {
				if key == "fx" {
					out.get_or_insert_with(|| map.clone()).remove("fx");
					continue;
				}
				if let Some(next) = without_effects_value(item) {
					out.get_or_insert_with(|| map.clone())
						.insert(key.clone(), next);
				}
			}
			out.map(Value::Object)
		}
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::{json, Map, Value};

	fn spinner(style: Option<&str>, label: Option<Value>) -> NativeNode {
		let mut node = NativeNode::new(NodeKind::Spinner);
		if let Some(style) = style {
			node.set_prop("style", Value::String(style.to_string()));
		}
		if let Some(label) = label {
			node.set_prop("label", label);
		}
		node
	}

	#[test]
	fn returns_none_when_no_switch_applies() {
		let node = spinner(None, None);
		assert!(quiet_motion(&node, QuietSwitches::default()).is_none());
	}

	#[test]
	fn stills_a_spinner_to_its_frame_zero_glyph() {
		let out = quiet_motion(
			&spinner(None, Some(Value::String("working".to_string()))),
			QuietSwitches {
				still_spinners: true,
				no_effects: false,
			},
		)
		.expect("spinner is stilled");
		assert_eq!(out.kind, NodeKind::Text);
		assert_eq!(out.children, Vec::new());
		let spans = out.props.spans().expect("spans prop");
		let text: String = spans.iter().map(|span| span.t.as_str()).collect();
		assert_eq!(text, "⠋ working");
	}

	#[test]
	fn starburst_uses_its_first_facet() {
		let out = quiet_motion(
			&spinner(Some("starburst"), None),
			QuietSwitches {
				still_spinners: true,
				no_effects: false,
			},
		)
		.expect("spinner is stilled");
		assert_eq!(out.plain_text(), "✻");
	}

	#[test]
	fn still_spinner_keeps_common_props_and_drops_style_and_label() {
		let mut node = spinner(Some("dots"), Some(Value::String("busy".to_string())));
		node.set_prop("tone", Value::String("accent".to_string()));
		let out = quiet_motion(
			&node,
			QuietSwitches {
				still_spinners: true,
				no_effects: false,
			},
		)
		.expect("spinner is stilled");
		assert_eq!(out.props.get_str("tone"), Some("accent"));
		assert!(out.props.get("style").is_none());
		assert!(out.props.get("label").is_none());
	}

	#[test]
	fn span_labels_are_kept_as_spans() {
		let label = json!([{ "t": "working", "s": "dim" }]);
		let out = quiet_motion(
			&spinner(None, Some(label)),
			QuietSwitches {
				still_spinners: true,
				no_effects: false,
			},
		)
		.expect("spinner is stilled");
		let spans = out.props.spans().expect("spans prop");
		assert_eq!(spans.len(), 3);
		assert_eq!(spans[1].t, " ");
		assert_eq!(spans[2].t, "working");
		assert_eq!(spans[2].s.as_deref(), Some("dim"));
	}

	#[test]
	fn an_empty_string_label_adds_nothing() {
		let out = quiet_motion(
			&spinner(None, Some(Value::String(String::new()))),
			QuietSwitches {
				still_spinners: true,
				no_effects: false,
			},
		)
		.expect("spinner is stilled");
		assert_eq!(out.plain_text(), "⠋");
	}

	#[test]
	fn effects_are_stripped_from_nested_spans() {
		let mut node = NativeNode::new(NodeKind::Text);
		node.set_prop(
			"spans",
			json!([{ "t": "hi", "fx": "shimmer" }, { "t": "there" }]),
		);
		let out = quiet_motion(
			&node,
			QuietSwitches {
				still_spinners: false,
				no_effects: true,
			},
		)
		.expect("effects are stripped");
		let spans = out.props.spans().expect("spans prop");
		assert_eq!(spans.len(), 2);
		assert!(spans[0].fx.is_none());
		assert_eq!(spans[0].t, "hi");
	}

	#[test]
	fn effect_free_nodes_are_left_untouched() {
		let mut node = NativeNode::new(NodeKind::Text);
		node.set_prop("text", Value::String("plain".to_string()));
		assert!(quiet_motion(
			&node,
			QuietSwitches {
				still_spinners: false,
				no_effects: true,
			},
		)
		.is_none());
	}

	#[test]
	fn effects_nested_in_a_child_are_stripped() {
		let mut child = NativeNode::new(NodeKind::Text);
		child.set_prop("fx", Value::String("pulse".to_string()));
		let parent = NativeNode::with_children(NodeKind::Col, Props::new(), vec![child]);
		let out = quiet_motion(
			&parent,
			QuietSwitches {
				still_spinners: false,
				no_effects: true,
			},
		)
		.expect("child effects are stripped");
		assert!(out.children[0].props.get("fx").is_none());
	}

	#[test]
	fn non_object_values_are_never_reallocated() {
		let mut props = Props::new();
		props.insert("n".to_string(), Value::from(7));
		props.insert("fx".to_string(), Value::String("sweep".to_string()));
		let out = without_effects_props(&props).expect("fx removed");
		assert!(out.get("fx").is_none());
		assert_eq!(out.get("n"), Some(&Value::from(7)));
	}

	#[test]
	fn an_empty_fx_key_is_still_removed() {
		let mut map = Map::new();
		map.insert("fx".to_string(), Value::Null);
		let out = without_effects_value(&Value::Object(map)).expect("fx removed");
		assert!(out.get("fx").is_none());
	}
}
