//! Port of `l/test/extension-inspector.test.ts`.

use std::sync::Arc;

use maple_tui::component::Component;
use maple_tui::overlays::extensions::extension_list::{ExtensionList, ExtensionListCallbacks};
use maple_tui::overlays::extensions::inspector_model::{
	live_tools_for_extension, parse_tool_file_header, project_list_hint, tool_params_from_schema,
	LiveToolRecord, ToolRuntimeSource,
};
use maple_tui::overlays::extensions::inspector_panel::InspectorPanel;
use maple_tui::overlays::extensions::types::{
	Extension, ExtensionKind, ExtensionSource, ExtensionState,
};
use serde_json::json;

fn user_source() -> ExtensionSource {
	ExtensionSource {
		provider: "native".to_string(),
		provider_name: "OMP (User)".to_string(),
		level: "user".to_string(),
	}
}

fn project_source(name: &str) -> ExtensionSource {
	ExtensionSource {
		provider: "native".to_string(),
		provider_name: format!("OMP ({name})"),
		level: "project".to_string(),
	}
}

fn render(panel: &InspectorPanel) -> String {
	panel.render(72, usize::MAX).join("\n")
}

fn rule_extension() -> Extension {
	Extension {
		id: "rule:orchestration".to_string(),
		kind: ExtensionKind::Rule,
		name: "orchestration".to_string(),
		display_name: "orchestration".to_string(),
		trigger: Some("always".to_string()),
		path: "/home/sf/worlds/base/rules/orchestration.md".to_string(),
		source: project_source("base"),
		state: ExtensionState::Active,
		raw: json!({
			"name": "orchestration",
			"alwaysApply": true,
			"content": "# asynchronous coordination\n\nnever block a reasoning turn merely waiting for another agent.",
		}),
		..Extension::default()
	}
}

fn tool_extension() -> Extension {
	Extension {
		id: "tool:gmail_send".to_string(),
		kind: ExtensionKind::Tool,
		name: "gmail_send".to_string(),
		display_name: "gmail_send".to_string(),
		description: Some("gmail_send custom tool".to_string()),
		path: "/home/sf/worlds/personal/.omp/tools/gmail_send.ts".to_string(),
		source: project_source("personal"),
		state: ExtensionState::Active,
		raw: json!({
			"name": "gmail_send",
			"description": "gmail_send custom tool",
			"path": "/home/sf/worlds/personal/.omp/tools/gmail_send.ts",
		}),
		..Extension::default()
	}
}

fn command_extension() -> Extension {
	Extension {
		id: "slash-command:triage".to_string(),
		kind: ExtensionKind::SlashCommand,
		name: "triage".to_string(),
		display_name: "triage".to_string(),
		trigger: Some("/triage".to_string()),
		path: "/home/sf/worlds/_template/.omp/commands/triage.md".to_string(),
		source: user_source(),
		state: ExtensionState::Active,
		raw: json!({
			"name": "triage",
			"content": "---\ndescription: \"Triage current world; smallest next actions.\"\n---\n\nInspect the current state relevant to: $ARGUMENTS\n",
		}),
		..Extension::default()
	}
}

fn live_tool(name: &str, description: Option<&str>, parameters: serde_json::Value) -> LiveToolRecord {
	LiveToolRecord {
		name: name.to_string(),
		label: None,
		description: description.map(str::to_string),
		parameters: Some(parameters),
		hidden: None,
		load_mode: None,
		source: None,
		source_path: None,
	}
}

struct Tools(Vec<LiveToolRecord>);

impl ToolRuntimeSource for Tools {
	fn get_live_tool(&self, name: &str) -> Option<LiveToolRecord> {
		self.0.iter().find(|tool| tool.name == name).cloned()
	}
	fn list_live_tools(&self) -> Option<Vec<LiveToolRecord>> {
		Some(self.0.clone())
	}
}

#[test]
fn puts_enablement_before_origin_and_drops_type() {
	let mut panel = InspectorPanel::new(None);
	panel.set_extension(Some(rule_extension()));
	let text = render(&panel);
	assert!(text.contains("orchestration"));
	assert!(text.contains("Active"));
	assert!(text.contains("Origin:"));
	assert!(text.find("Active") < text.find("Origin:"));
	assert!(!text.contains("Type:"));
	assert!(!text.contains("Status:"));
}

#[test]
fn joins_live_schema_instead_of_the_placeholder_description() {
	let mut panel = InspectorPanel::new(None);
	panel.set_tool_source(Some(Box::new(Tools(vec![live_tool(
		"gmail_send",
		Some("Send an email via gog for an authorized personal Gmail account."),
		json!({
			"type": "object",
			"required": ["to", "subject", "body"],
			"properties": {
				"to": { "type": "string", "description": "Recipients, comma-separated" },
				"subject": { "type": "string" },
				"body": { "type": "string" },
				"dry_run": { "type": "boolean", "description": "If true, only print intended send" },
			},
		}),
	)]))));
	panel.set_extension(Some(tool_extension()));
	let text = render(&panel);
	assert!(text.contains("Gmail"));
	assert!(text.contains("Send an email via gog"));
	assert!(text.contains("Arguments"));
	assert!(text.contains("Required"));
	assert!(text.contains("dry_run"));
	assert!(text.contains("Optional"));
	assert!(!text.contains("gmail_send custom tool"));
}

#[test]
fn strips_control_sequences_from_labels_and_schema() {
	let mut record = live_tool(
		"gmail_send",
		Some("Send\tvia gog"),
		json!({
			"type": "object",
			"required": ["to"],
			"properties": {
				"to": { "type": "string\u{1b}]8;;https://evil.test\u{7}", "description": "Recipients\u{7}, comma-separated" },
				"from": { "type": "string", "default": "\u{7}nobody" },
			},
		}),
	);
	record.label = Some("Gmail\u{1b}]8;;https://evil.test\u{7} Send".to_string());
	let mut panel = InspectorPanel::new(None);
	panel.set_tool_source(Some(Box::new(Tools(vec![record]))));
	panel.set_extension(Some(tool_extension()));
	let raw = panel.render(72, usize::MAX).join("\n");
	assert!(!raw.contains("\u{1b}]8;"));
	assert!(!raw.contains('\u{7}'));
	assert!(!raw.contains('\t'));
	assert!(raw.contains("Gmail Send"));
	assert!(raw.contains("Recipients, comma-separated"));
	assert!(raw.contains("Default: nobody"));
}

#[test]
fn collapses_schema_newlines_to_one_row() {
	let params = tool_params_from_schema(Some(&json!({
		"type": "object",
		"properties": {
			"note": { "type": "string\ninjected", "default": "alpha\nbeta" },
		},
	})));
	assert_eq!(params.len(), 1);
	assert_eq!(params[0].param_type, "string injected");
	assert_eq!(params[0].flag, "Default: alpha beta");
	for param in &params {
		assert!(!param.param_type.contains('\n'));
		assert!(!param.flag.contains('\n'));
	}
}

#[test]
fn joins_multi_export_factories_by_source_file() {
	let path = "/home/sf/.omp/agent/tools/systemd.ts";
	let tools = vec![
		LiveToolRecord {
			source: Some("extension".to_string()),
			source_path: Some(path.to_string()),
			..live_tool("systemd_inspect", Some("Read systemd state."), json!({ "type": "object", "properties": { "action": { "type": "string" } } }))
		},
		LiveToolRecord {
			source: Some("extension".to_string()),
			source_path: Some(path.to_string()),
			..live_tool("systemd_control", Some("Mutate units."), json!({ "type": "object", "properties": { "action": { "type": "string" } } }))
		},
	];
	let ext = Extension {
		id: "tool:systemd".to_string(),
		kind: ExtensionKind::Tool,
		name: "systemd".to_string(),
		display_name: "systemd".to_string(),
		path: path.to_string(),
		source: user_source(),
		state: ExtensionState::Active,
		raw: json!({ "name": "systemd" }),
		..Extension::default()
	};
	let source = Tools(tools.clone());
	let lives = live_tools_for_extension(&ext, Some(&source as &dyn ToolRuntimeSource));
	assert_eq!(lives.len(), 2);

	let mut panel = InspectorPanel::new(None);
	panel.set_tool_source(Some(Box::new(Tools(tools))));
	panel.set_extension(Some(ext.clone()));
	let text = render(&panel);
	assert!(text.contains("systemd_inspect"));
	assert!(text.contains("systemd_control"));
	assert!(text.contains("Read systemd state"));
	assert!(!text.contains("1 arg"));
}

#[test]
fn does_not_join_a_builtin_name_collision() {
	let ext = Extension {
		id: "tool:read".to_string(),
		kind: ExtensionKind::Tool,
		name: "read".to_string(),
		display_name: "read".to_string(),
		path: "/tmp/read.ts".to_string(),
		source: user_source(),
		state: ExtensionState::Active,
		raw: json!({ "name": "read" }),
		..Extension::default()
	};
	let mut builtin = live_tool(
		"read",
		Some("Read a file."),
		json!({ "type": "object", "properties": { "path": { "type": "string" } } }),
	);
	builtin.source = Some("builtin".to_string());
	let source = Tools(vec![builtin]);
	assert!(live_tools_for_extension(&ext, Some(&source as &dyn ToolRuntimeSource)).is_empty());

	let mut panel = InspectorPanel::new(None);
	panel.set_tool_source(Some(Box::new(source)));
	panel.set_extension(Some(ext));
	let text = render(&panel);
	assert!(!text.contains("Read a file."));
	assert!(!text.contains("Arguments"));
}

#[test]
fn shadowed_tools_do_not_join_the_winners_schema() {
	let mut panel = InspectorPanel::new(None);
	panel.set_tool_source(Some(Box::new(Tools(vec![live_tool(
		"gmail_send",
		Some("Send an email via gog for an authorized personal Gmail account."),
		json!({ "type": "object", "properties": { "to": { "type": "string" } } }),
	)]))));
	panel.set_extension(Some(Extension {
		state: ExtensionState::Shadowed,
		shadowed_by: Some("gmail_send".to_string()),
		path: "/home/sf/.omp/agent/tools/gmail_send.ts".to_string(),
		..tool_extension()
	}));
	let text = render(&panel);
	assert!(text.contains("Shadowed"));
	assert!(!text.contains("Send an email via gog"));
	assert!(!text.contains("Arguments"));
}

#[test]
fn project_hints_handle_windows_omp_paths() {
	let ext = |path: &str| Extension { path: path.to_string(), ..tool_extension() };
	assert_eq!(project_list_hint(&ext("C:\\repo\\.omp\\tools\\x.ts")).as_deref(), Some("repo"));
	assert_eq!(project_list_hint(&ext("C:/repo/.omp/tools/x.ts")).as_deref(), Some("repo"));
	assert_eq!(project_list_hint(&Extension { path: "/tmp/x.ts".to_string(), ..tool_extension() }), None);
}

#[test]
fn list_rows_show_runtime_hints() {
	let project_tool = tool_extension();
	let hidden = LiveToolRecord {
		hidden: Some(true),
		..live_tool("gmail_send", None, json!({ "type": "object", "properties": { "to": { "type": "string" } } }))
	};
	let callbacks = ExtensionListCallbacks {
		tool_source: Some(Arc::new(Tools(vec![hidden]))),
		..Default::default()
	};
	let mut list = ExtensionList::new(vec![project_tool], callbacks, 15);
	list.set_focused(true);
	let text = list.render(80).join("\n");
	assert!(text.contains("hidden · personal"));
	assert!(!text.contains("discoverable"));
	assert!(!text.contains("args"));
}

#[test]
fn rule_shows_apply_then_body() {
	let mut panel = InspectorPanel::new(None);
	panel.set_extension(Some(rule_extension()));
	let text = render(&panel);
	assert!(text.contains("alwaysApply"));
	assert!(text.contains("Rule"));
	assert!(text.contains("asynchronous coordination"));
	assert!(text.find("alwaysApply") < text.find("asynchronous coordination"));
}

#[test]
fn command_parses_frontmatter_description_and_template() {
	let mut panel = InspectorPanel::new(None);
	panel.set_extension(Some(command_extension()));
	let text = render(&panel);
	assert!(text.contains("Triage current world; smallest next actions."));
	assert!(text.contains("/triage"));
	assert!(text.contains("$ARGUMENTS"));
	assert!(text.contains("Template"));
	assert!(text.contains("Inspect the current state relevant to: $ARGUMENTS"));
	assert!(!text.contains("description:"));
	assert!(text.find("Active") < text.find("Template"));
}

#[test]
fn skill_hidden_frontmatter_is_marked() {
	let skill = Extension {
		id: "skill:hcom".to_string(),
		kind: ExtensionKind::Skill,
		name: "hcom".to_string(),
		display_name: "hcom".to_string(),
		description: Some("Named agent sessions that mail, wake, and resume across processes.".to_string()),
		path: "/home/sf/.omp/agent/skills/hcom/SKILL.md".to_string(),
		source: user_source(),
		state: ExtensionState::Active,
		raw: json!({
			"name": "hcom",
			"content": "# hcom\n\na 4-letter name is a session you can come back to.",
			"frontmatter": { "name": "hcom", "hide": true },
		}),
		..Extension::default()
	};
	let mut panel = InspectorPanel::new(None);
	panel.set_extension(Some(skill.clone()));
	let text = render(&panel);
	assert!(text.contains("Instruction"));
	assert!(text.contains("4-letter name"));
	assert!(text.contains("hidden"));

	let mut list = ExtensionList::new(vec![skill], ExtensionListCallbacks::default(), 15);
	list.set_focused(true);
	assert!(list.render(80).join("\n").contains("hidden"));
}

#[test]
fn preview_truncates_then_expands_in_place() {
	let body = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
	let mut panel = InspectorPanel::new(None);
	panel.set_extension(Some(Extension {
		raw: json!({ "name": "triage", "content": format!("---\ndescription: \"long template\"\n---\n\n{body}\n") }),
		..command_extension()
	}));
	let collapsed = render(&panel);
	assert!(collapsed.contains("line 1"));
	assert!(!collapsed.contains("line 20"));
	assert!(collapsed.contains("more lines"));

	panel.toggle_expanded();
	let expanded = render(&panel);
	assert!(expanded.contains("line 20"));
	assert!(!expanded.contains("more lines"));
}

#[test]
fn tool_file_header_drops_symlink_footnotes() {
	let description = parse_tool_file_header(
		"/**\n * cloak — bind a leased Cloak browser and drive the leased tab.\n *\n * Hidden unless an agent tools: list (or --tools) names it.\n *\n * Symlink: ~/.omp/agent/tools/cloak.ts → this file.\n */\nexport default function cloakTool() {}\n",
	)
	.unwrap();
	assert!(description.contains("cloak — bind a leased Cloak browser"));
	assert!(description.contains("Hidden unless an agent tools"));
	assert!(!description.contains("Symlink:"));
}
