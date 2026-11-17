//! Repair double-encoded JSON string arguments for the task tool.
//!
//! Reference: `l/src/tools/task-repair-args.ts`.
//!
//! Deviations:
//! - Operates on `serde_json::Value` task params/items instead of TS interfaces.
//! - Returns a possibly-cloned repaired value; callers can compare for changes.

use serde_json::Value;

fn has_structural_escape(value: &str) -> bool {
	let bytes = value.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'\\' && i + 1 < bytes.len() {
			match bytes[i + 1] {
				b'"' | b'\\' | b'/' => return true,
				b'u' => {
					if i + 6 <= bytes.len()
						&& bytes[i + 2..i + 6].iter().all(|b| b.is_ascii_hexdigit())
					{
						return true;
					}
				}
				_ => {}
			}
		}
		i += 1;
	}
	false
}

fn has_double_encode_signature(value: &str) -> bool {
	if has_structural_escape(value) {
		return true;
	}
	let bytes = value.as_bytes();
	let mut count = 0usize;
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'\\' {
			count += 1;
			if count >= 2 {
				return true;
			}
			i += 2;
		} else {
			i += 1;
		}
	}
	false
}

/// Return once-unescaped string when `value` is uniformly double-encoded JSON.
pub fn repair_double_encoded_json_string(value: &str) -> String {
	if !value.contains('\\') {
		return value.to_string();
	}
	if !has_double_encode_signature(value) {
		return value.to_string();
	}
	let wrapped = format!("\"{}\"", value);
	let Ok(decoded) = serde_json::from_str::<Value>(&wrapped) else {
		return value.to_string();
	};
	match decoded {
		Value::String(s) if s != value => s,
		_ => value.to_string(),
	}
}

fn repair_steps(steps: Option<&Value>) -> Option<Value> {
	let Value::Array(arr) = steps? else {
		return Some(steps?.clone());
	};
	let mut changed = false;
	let mut out = Vec::with_capacity(arr.len());
	for step in arr {
		if let Value::String(s) = step {
			let next = repair_double_encoded_json_string(s);
			if &next != s {
				changed = true;
			}
			out.push(Value::String(next));
		} else {
			out.push(step.clone());
		}
	}
	if changed {
		Some(Value::Array(out))
	} else {
		Some(Value::Array(arr.clone()))
	}
}

/// Repair a single task item's prose fields (`task`, `steps`).
pub fn repair_task_item(item: &Value) -> Value {
	let Value::Object(map) = item else {
		return item.clone();
	};
	let mut out = map.clone();
	let mut changed = false;

	if let Some(Value::String(task)) = map.get("task") {
		let next = repair_double_encoded_json_string(task);
		if &next != task {
			out.insert("task".to_string(), Value::String(next));
			changed = true;
		}
	}
	if let Some(steps) = map.get("steps") {
		if let Some(repaired) = repair_steps(Some(steps)) {
			if &repaired != steps {
				out.insert("steps".to_string(), repaired);
				changed = true;
			}
		}
	}

	if changed {
		Value::Object(out)
	} else {
		item.clone()
	}
}

/// Repair double-encoded prose in task-tool params.
pub fn repair_task_params(params: &Value) -> Value {
	let Value::Object(map) = params else {
		return params.clone();
	};
	let mut out = map.clone();
	let mut changed = false;

	for key in ["task", "context"] {
		if let Some(Value::String(s)) = map.get(key) {
			let next = repair_double_encoded_json_string(s);
			if &next != s {
				out.insert(key.to_string(), Value::String(next));
				changed = true;
			}
		}
	}
	if let Some(steps) = map.get("steps") {
		if let Some(repaired) = repair_steps(Some(steps)) {
			if &repaired != steps {
				out.insert("steps".to_string(), repaired);
				changed = true;
			}
		}
	}
	if let Some(Value::Array(tasks)) = map.get("tasks") {
		let mut t_changed = false;
		let mut repaired_tasks = Vec::with_capacity(tasks.len());
		for item in tasks {
			let next = repair_task_item(item);
			if &next != item {
				t_changed = true;
			}
			repaired_tasks.push(next);
		}
		if t_changed {
			out.insert("tasks".to_string(), Value::Array(repaired_tasks));
			changed = true;
		}
	}

	if changed {
		Value::Object(out)
	} else {
		params.clone()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn leaves_normal_string() {
		let s = "hello\nworld";
		assert_eq!(repair_double_encoded_json_string(s), s);
	}

	#[test]
	fn repairs_double_encoded_newlines_and_quotes() {
		let bad = "# Role\\nSay \\\"hi\\\"";
		assert_eq!(repair_double_encoded_json_string(bad), "# Role\nSay \"hi\"");
	}

	#[test]
	fn repairs_unicode_escape_sequence() {
		let bad = "dash \\u2014 here";
		assert_eq!(repair_double_encoded_json_string(bad), "dash — here");
	}

	#[test]
	fn does_not_repair_literal_escape_mention() {
		let s = "split on \\n";
		assert_eq!(repair_double_encoded_json_string(s), s);
	}

	#[test]
	fn repairs_task_params_fields() {
		let params = json!({
			"task": "line1\\nline2",
			"context": "ctx\\n",
			"steps": ["a\\nb", "ok"],
			"tasks": [{"task": "x\\ny", "steps": ["s\\t1"]}]
		});
		let out = repair_task_params(&params);
		assert_eq!(out["task"], "line1\nline2");
		assert_eq!(out["context"], "ctx\n");
		assert_eq!(out["steps"][0], "a\nb");
		assert_eq!(out["steps"][1], "ok");
		assert_eq!(out["tasks"][0]["task"], "x\ny");
		assert_eq!(out["tasks"][0]["steps"][0], "s\t1");
	}

	#[test]
	fn unchanged_returns_equal_value() {
		let params = json!({"task": "plain", "agent": "scout"});
		let out = repair_task_params(&params);
		assert_eq!(out, params);
	}
}
