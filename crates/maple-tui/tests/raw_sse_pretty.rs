//! Reference: `l/test/raw-sse-pretty.test.ts`.
//!
//! Ports the five `expandPrettyDataLines` cases against
//! `apps::debug::raw_sse::expand_pretty_data_lines`.

use maple_tui::apps::debug::raw_sse::{PRETTY_PRINT_DATA_THRESHOLD, expand_pretty_data_lines};

/// The wide payload the reference uses in its "round-trips through multi-line data" case.
fn wide_payload() -> String {
	serde_json::json!({
		"type": "response.output_item.done",
		"item": {
			"type": "message",
			"id": "msg_1234567890",
			"role": "assistant",
			"status": "completed",
			"content": [{ "type": "output_text", "text": "Hello there, this is a long enough message" }],
		},
	})
	.to_string()
}

fn data_lines<'a>(lines: &'a [String]) -> Vec<&'a str> {
	lines
		.iter()
		.filter(|line| line.starts_with("data: "))
		.map(|line| &line["data: ".len()..])
		.collect()
}

#[test]
fn case1_expands_a_wide_json_data_line_with_two_space_indent() {
	let wide = format!("data: {}", wide_payload());
	assert!(wide.len() > PRETTY_PRINT_DATA_THRESHOLD);
	let sse = vec![": ws ← response.output_item.done".to_string(), wide];
	let out = expand_pretty_data_lines(&sse);
	assert!(out.len() > sse.len(), "expected expansion: {out:?}");
	assert!(out[0].starts_with(": ws ← "), "{out:?}");
	let body = data_lines(&out);
	assert!(!body.is_empty());
	let rejoined = body.join("\n");
	let parsed = serde_json::from_str::<serde_json::Value>(&rejoined).expect("round-trips");
	assert_eq!(parsed, serde_json::from_str::<serde_json::Value>(&sse[1]["data: ".len()..]).unwrap());
	// Two-space indent and multi-line SSE `data:` prefixes.
	assert!(out.iter().any(|line| line.contains("  \"type\": \"response.output_item.done\"")), "{out:?}");
}

#[test]
fn case2_preserves_every_other_record_line() {
	let wide = format!("data: {}", wide_payload());
	let sse = vec![
		": event: message".to_string(),
		"data: {\"event\":\"start\",\"index\":0}".to_string(),
		wide.clone(),
		"".to_string(),
		": ws ← response.output_item.done".to_string(),
		"data: {\"event\":\"done\",\"index\":1}".to_string(),
	];
	let out = expand_pretty_data_lines(&sse);
	// Comment, short data, blank and trailing short data keep their slot; only the wide line grows.
	assert_eq!(out[0], sse[0]);
	assert_eq!(out[1], sse[1]);
	assert_eq!(out[out.len() - 2], sse[3]);
	assert_eq!(out[out.len() - 1], sse[5]);
	assert!(out.iter().all(|line| line.is_empty() || line.starts_with("data: ") || line.starts_with(": ")), "{out:?}");
	assert!(out.len() > sse.len(), "only the wide line expands: {out:?}");
	// Exactly one original line (the wide one) expanded, so it appears once at the end of its block.
	let wide_out: Vec<&String> = out
		.iter()
		.filter(|line| line.starts_with("data: ") && line.contains("response.output_item.done"))
		.collect();
	assert_eq!(wide_out.len(), 1, "{out:?}");
}

#[test]
fn case3_leaves_a_wide_non_json_data_line_untouched() {
	let line = format!("data: {}", "x".repeat(200));
	assert!(line.len() > PRETTY_PRINT_DATA_THRESHOLD);
	assert_eq!(expand_pretty_data_lines(&[line.clone()]), vec![line]);
}

#[test]
fn case4_leaves_an_invalid_json_data_line_untouched() {
	let line = format!("data: {{\"unterminated\":\"{}", "x".repeat(200));
	assert!(line.len() > PRETTY_PRINT_DATA_THRESHOLD);
	assert_eq!(expand_pretty_data_lines(&[line.clone()]), vec![line]);
}

#[test]
fn case5_leaves_a_short_data_line_on_a_single_line() {
	let line = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}".to_string();
	assert!(line.len() <= PRETTY_PRINT_DATA_THRESHOLD);
	let out = expand_pretty_data_lines(&[line.clone()]);
	assert_eq!(out, vec![line]);
}
