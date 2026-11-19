//! Assembly of typed subagent yield calls into a final payload.
//!
//! Reference: `l/src/tools/task-yield-assembly.ts`.
//!
//! Deviations:
//! - `unknown`/record values are `serde_json::Value`.
//! - `YieldSectionShapes` is `HashMap<String, YieldSectionShape>`.
//! - `last_assistant_text` is `Option<&str>`.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// Output-schema shape of each declared top-level property, keyed by label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YieldSectionShape {
	/// Sections accumulate into a list.
	Array,
	/// Sections keep the latest yield.
	Scalar,
}

/// One yield item as consumed by assembly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YieldItem {
	/// Incremental labels (array), terminal marker (string), or absent.
	pub r#type: YieldType,
	/// Yielded data.
	pub data: Option<Value>,
	/// Use the last assistant turn as payload.
	pub use_last_turn: bool,
	/// Accepted although it failed output schema.
	pub schema_overridden: bool,
	/// Aborted yield.
	pub status_aborted: bool,
}

/// Yield `type` field shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum YieldType {
	/// No type.
	#[default]
	None,
	/// Terminal marker / label.
	String(String),
	/// Incremental section labels.
	Array(Vec<String>),
}

/// Outcome of folding yield calls into one payload.
#[derive(Clone, Debug, PartialEq)]
pub struct AssembledYieldResult {
	/// Final payload.
	pub data: Value,
	/// Any contributing yield was schema-overridden.
	pub schema_overridden: bool,
	/// Data came from last assistant text as a raw string result.
	pub raw_text: bool,
	/// Some payload source was missing.
	pub missing_data: bool,
}

fn is_incremental(r#type: &YieldType) -> bool {
	matches!(r#type, YieldType::Array(v) if !v.is_empty())
}

fn get_yield_labels(r#type: &YieldType) -> Vec<String> {
	match r#type {
		YieldType::String(s) => {
			let t = s.trim();
			if t.is_empty() {
				vec![]
			} else {
				vec![t.to_string()]
			}
		}
		YieldType::Array(v) => v
			.iter()
			.filter_map(|s| {
				let t = s.trim();
				if t.is_empty() {
					None
				} else {
					Some(t.to_string())
				}
			})
			.collect(),
		YieldType::None => vec![],
	}
}

struct ResolvedPayload {
	value: Value,
	from_last_assistant_text: bool,
	missing_data: bool,
}

fn resolve_yield_payload(
	item: &YieldItem,
	last_assistant_text: Option<&str>,
	labels: &[String],
) -> ResolvedPayload {
	let has_data = item.data.is_some();
	let should_use_last_turn = item.use_last_turn || (!labels.is_empty() && !has_data);
	if should_use_last_turn {
		if let Some(text) = last_assistant_text {
			return ResolvedPayload {
				value: Value::String(text.to_string()),
				from_last_assistant_text: true,
				missing_data: text.is_empty(),
			};
		}
	}
	let value = item.data.clone().unwrap_or(Value::Null);
	ResolvedPayload {
		from_last_assistant_text: false,
		missing_data: item.data.is_none() || item.data.as_ref().is_some_and(Value::is_null),
		value,
	}
}

fn append_yield_section(
	sections: &mut serde_json::Map<String, Value>,
	section_counts: &mut HashMap<String, usize>,
	label: &str,
	value: Value,
	shape: Option<YieldSectionShape>,
) {
	let count = *section_counts.get(label).unwrap_or(&0);
	let existing = sections.get(label).cloned();
	match shape {
		Some(YieldSectionShape::Scalar) => {
			sections.insert(label.to_string(), value);
		}
		_ if count == 0 => {
			if matches!(shape, Some(YieldSectionShape::Array)) {
				sections.insert(label.to_string(), Value::Array(vec![value]));
			} else {
				sections.insert(label.to_string(), value);
			}
		}
		_ => {
			if let Some(Value::Array(mut arr)) = existing {
				arr.push(value);
				sections.insert(label.to_string(), Value::Array(arr));
			} else {
				let prev = sections.get(label).cloned().unwrap_or(Value::Null);
				sections.insert(label.to_string(), Value::Array(vec![prev, value]));
			}
		}
	}
	section_counts.insert(label.to_string(), count + 1);
}

/// Assemble typed yield calls into the final payload.
pub fn assemble_yield_result(
	yield_items: &[YieldItem],
	last_assistant_text: Option<&str>,
	section_shapes: Option<&HashMap<String, YieldSectionShape>>,
) -> Option<AssembledYieldResult> {
	if yield_items.is_empty() {
		return None;
	}

	let mut terminal_item: Option<&YieldItem> = None;
	for item in yield_items.iter().rev() {
		if !is_incremental(&item.r#type) {
			terminal_item = Some(item);
			break;
		}
	}

	let mut sections = serde_json::Map::new();
	let mut section_counts: HashMap<String, usize> = HashMap::new();
	let mut overridden_scalars: HashSet<String> = HashSet::new();
	let mut schema_overridden = false;
	let mut missing_data = false;
	let mut has_sections = false;

	for item in yield_items {
		if item.status_aborted {
			continue;
		}
		if !is_incremental(&item.r#type) {
			continue;
		}
		let overridden = item.schema_overridden;
		let labels = get_yield_labels(&item.r#type);
		let resolved = resolve_yield_payload(item, last_assistant_text, &labels);
		missing_data |= resolved.missing_data;
		if labels.is_empty() {
			schema_overridden |= overridden;
		}
		for label in labels {
			let shape = section_shapes.and_then(|m| m.get(&label)).copied();
			append_yield_section(
				&mut sections,
				&mut section_counts,
				&label,
				resolved.value.clone(),
				shape,
			);
			if matches!(shape, Some(YieldSectionShape::Scalar)) {
				if overridden {
					overridden_scalars.insert(label.clone());
				} else {
					overridden_scalars.remove(&label);
				}
			} else {
				schema_overridden |= overridden;
			}
			has_sections = true;
		}
	}

	if let Some(term) = terminal_item {
		if term.data.is_some() {
			let resolved = resolve_yield_payload(term, last_assistant_text, &[]);
			return Some(AssembledYieldResult {
				data: resolved.value.clone(),
				schema_overridden: term.schema_overridden,
				raw_text: resolved.from_last_assistant_text && resolved.value.is_string(),
				missing_data: resolved.missing_data,
			});
		}
	}

	if has_sections {
		return Some(AssembledYieldResult {
			data: Value::Object(sections),
			schema_overridden: schema_overridden || !overridden_scalars.is_empty(),
			raw_text: false,
			missing_data,
		});
	}

	let term = terminal_item?;
	let resolved = resolve_yield_payload(term, last_assistant_text, &get_yield_labels(&term.r#type));
	Some(AssembledYieldResult {
		data: resolved.value.clone(),
		schema_overridden: term.schema_overridden,
		raw_text: resolved.from_last_assistant_text && resolved.value.is_string(),
		missing_data: resolved.missing_data,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn empty_returns_none() {
		assert!(assemble_yield_result(&[], None, None).is_none());
	}

	#[test]
	fn terminal_data_wins() {
		let items = vec![YieldItem {
			r#type: YieldType::String("result".into()),
			data: Some(json!({"a": 1})),
			..Default::default()
		}];
		let out = assemble_yield_result(&items, None, None).unwrap();
		assert_eq!(out.data, json!({"a": 1}));
		assert!(!out.raw_text);
	}

	#[test]
	fn incremental_sections_accumulate() {
		let items = vec![
			YieldItem {
				r#type: YieldType::Array(vec!["notes".into()]),
				data: Some(json!("a")),
				..Default::default()
			},
			YieldItem {
				r#type: YieldType::Array(vec!["notes".into()]),
				data: Some(json!("b")),
				..Default::default()
			},
		];
		let out = assemble_yield_result(&items, None, None).unwrap();
		assert_eq!(out.data, json!({"notes": ["a", "b"]}));
	}

	#[test]
	fn scalar_section_keeps_last() {
		let mut shapes = HashMap::new();
		shapes.insert("summary".to_string(), YieldSectionShape::Scalar);
		let items = vec![
			YieldItem {
				r#type: YieldType::Array(vec!["summary".into()]),
				data: Some(json!("first")),
				..Default::default()
			},
			YieldItem {
				r#type: YieldType::Array(vec!["summary".into()]),
				data: Some(json!("second")),
				..Default::default()
			},
		];
		let out = assemble_yield_result(&items, None, Some(&shapes)).unwrap();
		assert_eq!(out.data, json!({"summary": "second"}));
	}

	#[test]
	fn data_less_terminal_uses_last_assistant_text() {
		let items = vec![YieldItem {
			r#type: YieldType::String("result".into()),
			data: None,
			use_last_turn: true,
			..Default::default()
		}];
		let out = assemble_yield_result(&items, Some("hello"), None).unwrap();
		assert_eq!(out.data, json!("hello"));
		assert!(out.raw_text);
	}
}
