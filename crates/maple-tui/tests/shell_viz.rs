//! Port of `l/test/shell-viz.test.ts`.
//!
//! Deviations: the reference's `RunTrace`/`Shell` native execution boundary is
//! Bun-only, so the live-trace, pending-trace, failed-command, and sink/replay
//! cases are not portable. The DTO validation contract is.

use serde_json::{json, Value};
use maple_tui::tools::shell_viz::decode_shell_presentation;

fn valid_fixture() -> Value {
	json!({
		"timeline": { "length": 8, "cursor": 8, "elapsedNs": 4, "durationNs": 4 },
		"preferences": {
			"view": "pseudo",
			"layers": { "color": true, "inline": true, "idioms": true, "effects": true, "unwrap": true },
			"split": false,
			"playing": false,
			"speed": 1.0,
			"selectedNode": "2:0:17",
			"expandedNodes": ["2:0:17"]
		},
		"rows": [{
			"id": "command",
			"depth": 0,
			"node": "2:0:17",
			"range": [0, 9],
			"state": "ok",
			"segments": [{ "text": "print(1)", "node": "2:0:17", "state": "ok", "executedAtNs": 4, "executionSeq": 8 }],
			"effects": [],
			"annotations": [{ "name": "x", "value": "1", "node": "2:0:17", "setterSeq": 7 }]
		}],
		"original": [],
		"output": "1\n",
		"executing": false,
		"watches": [{ "name": "x", "value": "1", "node": "2:0:17", "setterSeq": 7 }]
	})
}

#[test]
fn decodes_a_valid_presentation() {
	let presentation = decode_shell_presentation(&valid_fixture().to_string());
	assert!(presentation.is_ok(), "valid fixture rejected: {presentation:?}");
}

#[test]
fn rejects_malformed_dtos_and_numeric_node_identities_without_compatibility_coercion() {
	let cases: Vec<Value> = vec![
		{
			let mut value = valid_fixture();
			value["rows"][0]["node"] = json!(17);
			value
		},
		{
			let mut value = valid_fixture();
			value["rows"][0]["segments"][0]["node"] = json!("17");
			value
		},
		{
			let mut value = valid_fixture();
			value["rows"][0]["segments"][0]["executionNode"] = json!(17);
			value
		},
		{
			let mut value = valid_fixture();
			value["watches"][0]["node"] = json!("2:0:-1");
			value
		},
		{
			let mut value = valid_fixture();
			value["preferences"]["expandedNodes"] = json!([17]);
			value
		},
		{
			let mut value = valid_fixture();
			value["timeline"]["cursor"] = json!("8");
			value
		},
	];

	for value in cases {
		assert!(
			decode_shell_presentation(&value.to_string()).is_err(),
			"malformed presentation accepted: {value}"
		);
	}
}