//! Port of the `parseGeneratedAgentSpec` cases in `l/test/agents-hub.test.ts`.
//!
//! The hub layout/picker/native-event suites drive a TUI stub and the settings
//! graph, which the crate's hub state model does not expose; only the pure
//! model-output parser is portable.

use maple_tui::overlays::agents_hub::{GeneratedAgentSpec, parse_generated_agent_spec};

fn spec() -> GeneratedAgentSpec {
	GeneratedAgentSpec {
		identifier: "code-reviewer".to_string(),
		when_to_use: "Use this agent when reviewing code.".to_string(),
		system_prompt: "Review the diff.".to_string(),
	}
}

#[test]
fn parses_plain_json() {
	let parsed =
		parse_generated_agent_spec(&serde_json::to_string(&spec()).expect("json")).expect("parses");
	assert_eq!(parsed, spec());
}

#[test]
fn parses_json_wrapped_in_a_json_code_fence() {
	let raw = format!("Here you go:\n```json\n{}\n```\n", serde_json::to_string(&spec()).unwrap());
	let parsed = parse_generated_agent_spec(&raw).expect("parses");
	assert_eq!(parsed, spec());
}

#[test]
fn keeps_a_code_fence_inside_a_json_string_value() {
	let system_prompt = "Emit this report:\n```\n# Report\n```\nThen stop.";
	let value = serde_json::json!({
		"identifier": "code-reviewer",
		"whenToUse": "Use this agent when reviewing code.",
		"systemPrompt": system_prompt,
	});
	let parsed = parse_generated_agent_spec(&value.to_string()).expect("parses");
	assert_eq!(parsed.system_prompt, system_prompt);
}

#[test]
fn rejects_invalid_identifiers_and_missing_fields() {
	let invalid = serde_json::json!({
		"identifier": "Code_Reviewer",
		"whenToUse": "Use this agent when reviewing code.",
		"systemPrompt": "Review the diff.",
	});
	assert!(parse_generated_agent_spec(&invalid.to_string()).is_err());

	let missing = serde_json::json!({ "identifier": "code-reviewer" });
	assert!(parse_generated_agent_spec(&missing.to_string()).is_err());

	assert!(parse_generated_agent_spec("not json").is_err());
}
