//! Port of `l/test/live-tool-session.test.ts`.
//!
//! The reference's third test drives `ExtensionList` and `InspectorPanel`; those
//! components are not ported, so only the join/lookup tests run.

use std::cell::Cell;

use maple_tui::overlays::extensions::inspector_model::live_tools_for_extension;
use maple_tui::overlays::extensions::live_tool_session::{
	LiveToolInfo, LiveToolRecord, LiveToolSessionLookup, SessionToolView, ToolRuntimeSource,
	list_live_tool_records, live_tool_record_from_session,
};
use maple_tui::overlays::extensions::types::{
	Extension, ExtensionKind, ExtensionSource, ExtensionState,
};

fn info(name: &str, source: &str, path: &str) -> LiveToolInfo {
	LiveToolInfo {
		name: name.to_string(),
		source: source.to_string(),
		path: Some(path.to_string()),
	}
}

fn tool(name: &str, overrides: impl FnOnce(&mut SessionToolView)) -> SessionToolView {
	let mut view = SessionToolView {
		name: name.to_string(),
		label: Some(format!("{name} label")),
		description: Some(format!("{name} desc")),
		parameters: Some(serde_json::json!({
			"type": "object",
			"properties": { "q": { "type": "string" } },
		})),
		hidden: None,
		load_mode: None,
	};
	overrides(&mut view);
	view
}

struct FakeSession {
	infos: Vec<LiveToolInfo>,
	tools: Vec<SessionToolView>,
	snapshot_calls: Cell<usize>,
}

impl LiveToolSessionLookup for FakeSession {
	fn get_tool_by_name(&self, name: &str) -> Option<SessionToolView> {
		self.tools.iter().find(|tool| tool.name == name).cloned()
	}

	fn get_all_tool_infos(&self) -> Vec<LiveToolInfo> {
		self.snapshot_calls.set(self.snapshot_calls.get() + 1);
		self.infos.clone()
	}
}

struct ListSource {
	records: Vec<LiveToolRecord>,
}

impl ToolRuntimeSource for ListSource {
	fn get_live_tool(&self, name: &str) -> Option<LiveToolRecord> {
		self.records.iter().find(|record| record.name == name).cloned()
	}

	fn list_live_tools(&self) -> Option<Vec<LiveToolRecord>> {
		Some(self.records.clone())
	}
}

fn extension(id: &str, name: &str, path: &str) -> Extension {
	Extension {
		id: id.to_string(),
		kind: ExtensionKind::Tool,
		name: name.to_string(),
		display_name: name.to_string(),
		description: None,
		trigger: None,
		path: path.to_string(),
		source: ExtensionSource {
			provider: "native".to_string(),
			provider_name: "OMP".to_string(),
			level: "user".to_string(),
		},
		state: ExtensionState::Active,
		disabled_reason: None,
		shadowed_by: None,
		raw: serde_json::json!({ "name": name, "path": path }),
	}
}

#[test]
fn lists_every_tool_from_one_get_all_tool_infos_call() {
	let git_path = "/tmp/tools/git.ts";
	let commit_path = "/tmp/tools/git_commit.ts";
	let systemd_path = "/tmp/tools/systemd.ts";
	let infos = vec![
		info("read", "builtin", "<builtin:read>"),
		info("mcp_ping", "mcp", "<mcp:mcp_ping>"),
		info("sdk_host", "sdk", "<sdk:sdk_host>"),
		info("git", "extension", git_path),
		info("git_commit", "extension", commit_path),
		info("systemd_inspect", "extension", systemd_path),
		info("systemd_control", "extension", systemd_path),
		info("orphan", "extension", "/tmp/tools/orphan.ts"),
	];
	let tools = vec![
		tool("read", |_| {}),
		tool("mcp_ping", |_| {}),
		tool("sdk_host", |_| {}),
		tool("git", |view| {
			view.parameters = Some(serde_json::json!({
				"type": "object",
				"properties": { "path": { "type": "string" } },
			}));
		}),
		tool("git_commit", |_| {}),
		tool("systemd_inspect", |view| {
			view.hidden = Some(true);
			view.load_mode = Some("discoverable".to_string());
		}),
		tool("systemd_control", |view| {
			view.load_mode = Some("essential".to_string());
		}),
	];
	let session = FakeSession {
		infos,
		tools,
		snapshot_calls: Cell::new(0),
	};

	let listed = list_live_tool_records(&session);
	assert_eq!(session.snapshot_calls.get(), 1);
	let names: Vec<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
	assert_eq!(
		names,
		vec![
			"read",
			"mcp_ping",
			"sdk_host",
			"git",
			"git_commit",
			"systemd_inspect",
			"systemd_control",
		]
	);
	let find = |name: &str| listed.iter().find(|entry| entry.name == name).unwrap();
	assert_eq!(find("git").source_path.as_deref(), Some(git_path));
	assert_eq!(find("git_commit").source_path.as_deref(), Some(commit_path));
	assert_eq!(find("systemd_inspect").source_path.as_deref(), Some(systemd_path));
	assert_eq!(find("systemd_control").source_path.as_deref(), Some(systemd_path));
	assert_eq!(find("read").source.as_deref(), Some("builtin"));
	assert_eq!(find("mcp_ping").source.as_deref(), Some("mcp"));
	assert_eq!(find("sdk_host").source.as_deref(), Some("sdk"));
	assert_eq!(find("git").source.as_deref(), Some("extension"));
	assert_eq!(find("read").source_path, None);
	assert_eq!(
		find("git").parameters,
		Some(serde_json::json!({
			"type": "object",
			"properties": { "path": { "type": "string" } },
		}))
	);
	assert_eq!(find("systemd_inspect").hidden, Some(true));
	assert_eq!(find("systemd_inspect").load_mode.as_deref(), Some("discoverable"));
	assert_eq!(find("systemd_control").load_mode.as_deref(), Some("essential"));

	let tool_source = ListSource {
		records: listed.clone(),
	};
	let git_row = extension("tool:git", "git", git_path);
	let systemd_row = extension("tool:systemd", "systemd", systemd_path);
	let builtin_shadow = extension("tool:read", "read", "/tmp/tools/read.ts");
	let git_names: Vec<&str> = live_tools_for_extension(&git_row, Some(&tool_source))
		.iter()
		.map(|entry| entry.name.as_str())
		.collect();
	assert_eq!(git_names, vec!["git"]);
	let systemd_names: Vec<&str> = live_tools_for_extension(&systemd_row, Some(&tool_source))
		.iter()
		.map(|entry| entry.name.as_str())
		.collect();
	assert_eq!(systemd_names, vec!["systemd_inspect", "systemd_control"]);
	assert!(live_tools_for_extension(&builtin_shadow, Some(&tool_source)).is_empty());

	let _ = live_tool_record_from_session(&session, "git");
	assert_eq!(session.snapshot_calls.get(), 2);
}

#[test]
fn joins_factory_siblings_on_a_unc_source_path() {
	let unc = "\\\\server\\share\\.omp\\tools\\systemd.ts";
	let infos = vec![
		info("systemd_inspect", "extension", unc),
		info("systemd_control", "extension", unc),
	];
	let session = FakeSession {
		infos,
		tools: vec![tool("systemd_inspect", |_| {}), tool("systemd_control", |_| {})],
		snapshot_calls: Cell::new(0),
	};
	let listed = list_live_tool_records(&session);
	assert!(listed.iter().all(|entry| entry.source_path.as_deref() == Some(unc)));
	let row = extension("tool:systemd", "systemd", unc);
	let tool_source = ListSource {
		records: listed.clone(),
	};
	let names: Vec<&str> = live_tools_for_extension(&row, Some(&tool_source))
		.iter()
		.map(|entry| entry.name.as_str())
		.collect();
	assert_eq!(names, vec!["systemd_inspect", "systemd_control"]);
}

#[test]
#[ignore = "missing facility: ExtensionList and InspectorPanel components are not ported"]
fn one_list_live_tools_call_for_many_rows_and_the_inspector() {
	let systemd_path = "/tmp/tools/systemd.ts";
	let git_path = "/tmp/tools/git.ts";
	let infos = vec![
		info("git", "extension", git_path),
		info("systemd_inspect", "extension", systemd_path),
		info("systemd_control", "extension", systemd_path),
	];
	let session = FakeSession {
		infos,
		tools: vec![
			tool("git", |_| {}),
			tool("systemd_inspect", |_| {}),
			tool("systemd_control", |_| {}),
		],
		snapshot_calls: Cell::new(0),
	};
	let production = ListSource {
		records: list_live_tool_records(&session),
	};
	let listed = production.list_live_tools().unwrap_or_default();
	assert_eq!(listed.len(), 3);
	assert_eq!(session.snapshot_calls.get(), 1);
}
