//! Port of `l/test/mcp-runtime-snapshot.test.ts`.

use maple_tui::overlays::extensions::mcp_runtime::{
	McpConnectionDisplay, McpConnectionHealth, McpConnectionStatus, McpImplementationDisplay,
	McpManagerTool, McpResourceDisplay, McpRuntimeCatalogItem, McpRuntimeOpts, McpRuntimeSource,
	McpServerConfigDisplay, McpServerDisplay, McpServerResources, McpToolDisplay, McpTransport,
	format_mcp_health_label, format_mcp_list_hint, infer_mcp_transport, is_discovered_mcp_server,
	snapshot_mcp_runtime, visible_mcp_tools,
};

fn source_value() -> serde_json::Value {
	serde_json::json!({
		"provider": "native",
		"providerName": "OMP (User)",
		"path": "/home/sf/.omp/agent/mcp.json",
		"level": "user",
	})
}

fn server(overrides: impl FnOnce(&mut McpServerDisplay)) -> McpServerDisplay {
	let mut server = McpServerDisplay {
		name: "github".to_string(),
		command: Some("/usr/bin/github-mcp-server".to_string()),
		args: Some(vec!["stdio".to_string()]),
		transport: Some("stdio".to_string()),
		source: Some(source_value()),
		..McpServerDisplay::default()
	};
	overrides(&mut server);
	server
}

fn connection(overrides: impl FnOnce(&mut McpConnectionDisplay)) -> McpConnectionDisplay {
	let mut connection = McpConnectionDisplay {
		name: "github".to_string(),
		config: McpServerConfigDisplay {
			command: Some("/usr/bin/github-mcp-server".to_string()),
			args: Some(vec!["stdio".to_string()]),
			..McpServerConfigDisplay::default()
		},
		server_info: Some(McpImplementationDisplay {
			name: "github-mcp-server".to_string(),
			title: Some("GitHub MCP Server".to_string()),
			version: Some("0.19.0".to_string()),
			description: Some("Access GitHub repositories, issues, and pull requests.".to_string()),
			website_url: None,
		}),
		tools: Some(vec![
			McpToolDisplay {
				name: "search_code".to_string(),
				description: Some("Search code across GitHub repositories.".to_string()),
				input_schema: Some(serde_json::json!({
					"type": "object",
					"required": ["query"],
					"properties": {
						"query": { "type": "string", "description": "Search query" },
						"language": { "type": "string", "description": "Optional language filter" },
					},
				})),
				..McpToolDisplay::default()
			},
			McpToolDisplay {
				name: "get_pull_request".to_string(),
				description: Some("Get pull request details.".to_string()),
				input_schema: Some(serde_json::json!({ "type": "object" })),
				..McpToolDisplay::default()
			},
		]),
		resources: Some(vec![McpResourceDisplay {
			name: "repo".to_string(),
			uri: Some("github://repo".to_string()),
			..McpResourceDisplay::default()
		}]),
		prompts: Some(vec![McpResourceDisplay {
			name: "review_pr".to_string(),
			description: Some("Review a pull request".to_string()),
			..McpResourceDisplay::default()
		}]),
		instructions: Some("Prefer search_code over cloning.".to_string()),
		..McpConnectionDisplay::default()
	};
	overrides(&mut connection);
	connection
}

struct StubSource {
	status: McpConnectionStatus,
	connection: Option<McpConnectionDisplay>,
}

impl McpRuntimeSource for StubSource {
	fn get_connection_status(&self, _name: &str) -> McpConnectionStatus {
		self.status
	}

	fn get_connection(&self, _name: &str) -> Option<McpConnectionDisplay> {
		self.connection.clone()
	}

	fn get_tools(&self) -> Vec<McpManagerTool> {
		let Some(connection) = self.connection.as_ref() else {
			return Vec::new();
		};
		connection
			.tools
			.as_deref()
			.unwrap_or_default()
			.iter()
			.map(|tool| McpManagerTool {
				mcp_server_name: Some(connection.name.clone()),
				mcp_tool_name: Some(tool.name.clone()),
				description: tool.description.clone(),
				..McpManagerTool::default()
			})
			.collect()
	}

	fn get_server_resources(&self, _name: &str) -> Option<McpServerResources> {
		self.connection.as_ref().map(|connection| McpServerResources {
			resources: connection.resources.clone().unwrap_or_default(),
			templates: connection.resource_templates.clone().unwrap_or_default(),
		})
	}

	fn get_server_prompts(&self, _name: &str) -> Option<Vec<McpResourceDisplay>> {
		self.connection.as_ref().and_then(|connection| connection.prompts.clone())
	}
}

fn source_for(status: McpConnectionStatus, connection: Option<McpConnectionDisplay>) -> StubSource {
	StubSource { status, connection }
}

#[test]
fn does_not_treat_command_or_url_as_a_description() {
	let snap = snapshot_mcp_runtime(&server(|_| {}), None, None);
	assert_eq!(snap.description, None);
	assert_eq!(snap.command.as_deref(), Some("/usr/bin/github-mcp-server"));
	assert_eq!(snap.health, McpConnectionHealth::Disconnected);
	assert_eq!(snap.transport, McpTransport::Stdio);
}

#[test]
fn joins_live_connection_identity_tools_and_instructions() {
	let conn = connection(|_| {});
	let snap = snapshot_mcp_runtime(&server(|_| {}), Some(&source_for(McpConnectionStatus::Connected, Some(conn))), None);
	assert_eq!(snap.health, McpConnectionHealth::Connected);
	assert_eq!(snap.title.as_deref(), Some("GitHub MCP Server"));
	assert_eq!(
		snap.description.as_deref(),
		Some("Access GitHub repositories, issues, and pull requests.")
	);
	assert_eq!(snap.implementation_name.as_deref(), Some("github-mcp-server"));
	assert_eq!(snap.implementation_version.as_deref(), Some("0.19.0"));
	let names: Vec<&str> = snap.tools.iter().map(|tool| tool.name.as_str()).collect();
	assert_eq!(names, vec!["search_code", "get_pull_request"]);
	assert_eq!(
		snap.tools[0].description.as_deref(),
		Some("Search code across GitHub repositories.")
	);
	assert_eq!(
		snap.tools[0].parameters,
		Some(serde_json::json!({
			"type": "object",
			"required": ["query"],
			"properties": {
				"query": { "type": "string", "description": "Search query" },
				"language": { "type": "string", "description": "Optional language filter" },
			},
		}))
	);
	assert_eq!(snap.resources.len(), 1);
	assert_eq!(snap.prompts.len(), 1);
	assert_eq!(snap.instructions.as_deref(), Some("Prefer search_code over cloning."));
	assert_eq!(format_mcp_list_hint(&snap), "2 tools · 1 resource · 1 prompt");
}

#[test]
fn maps_connecting_and_inactive_separately_from_enabled_in_config() {
	assert_eq!(
		snapshot_mcp_runtime(&server(|_| {}), Some(&source_for(McpConnectionStatus::Connecting, None)), None).health,
		McpConnectionHealth::Connecting
	);
	assert_eq!(
		snapshot_mcp_runtime(&server(|_| {}), Some(&source_for(McpConnectionStatus::Disconnected, None)), None).health,
		McpConnectionHealth::Disconnected
	);
	assert_eq!(
		snapshot_mcp_runtime(
			&server(|server| server.enabled = Some(false)),
			Some(&source_for(McpConnectionStatus::Connected, Some(connection(|_| {})))),
			None,
		)
		.health,
		McpConnectionHealth::Inactive
	);
	assert_eq!(
		snapshot_mcp_runtime(
			&server(|_| {}),
			Some(&source_for(McpConnectionStatus::Connected, Some(connection(|_| {})))),
			Some(McpRuntimeOpts {
				enabled: Some(false),
				shadowed: false,
			}),
		)
		.health,
		McpConnectionHealth::Inactive
	);
}

#[test]
fn does_not_join_a_shadowed_same_name_config_against_the_winner() {
	let winner = connection(|_| {});
	let snap = snapshot_mcp_runtime(
		&server(|server| server.command = Some("/usr/bin/shadowed-github".to_string())),
		Some(&source_for(McpConnectionStatus::Connected, Some(winner))),
		Some(McpRuntimeOpts {
			enabled: None,
			shadowed: true,
		}),
	);
	assert_eq!(snap.health, McpConnectionHealth::Disconnected);
	assert_eq!(snap.title, None);
	assert_eq!(snap.description, None);
	assert!(snap.tools.is_empty());
	assert_eq!(snap.instructions, None);
	assert_eq!(snap.command.as_deref(), Some("/usr/bin/shadowed-github"));
}

#[test]
fn infers_http_from_url_when_transport_is_omitted() {
	let remote = McpServerDisplay {
		name: "remote".to_string(),
		url: Some("https://example.test/mcp".to_string()),
		source: Some(source_value()),
		..McpServerDisplay::default()
	};
	assert_eq!(infer_mcp_transport(&remote), McpTransport::Http);
	let sse = McpServerConfigDisplay {
		type_: Some("sse".to_string()),
		url: Some("https://example.test/sse".to_string()),
		..McpServerConfigDisplay::default()
	};
	assert_eq!(infer_mcp_transport(&sse), McpTransport::Sse);
	assert!(is_discovered_mcp_server(&serde_json::json!({
		"name": "github",
		"command": "/usr/bin/github-mcp-server",
		"_source": source_value(),
	})));
	assert!(!is_discovered_mcp_server(&serde_json::json!({ "command": "echo" })));
}

#[test]
fn visible_mcp_tools_truncates_with_a_leftover_count() {
	let tools: Vec<McpRuntimeCatalogItem> = (0..12)
		.map(|index| McpRuntimeCatalogItem {
			name: format!("tool_{index}"),
			..McpRuntimeCatalogItem::default()
		})
		.collect();
	let visible = visible_mcp_tools(&tools, 8);
	assert_eq!(visible.shown.len(), 8);
	assert_eq!(visible.hidden, 4);
	assert_eq!(format_mcp_health_label(McpConnectionHealth::Disconnected), "Not connected");
}

#[test]
fn strips_osc_bel_and_tabs_from_server_provided_display_fields() {
	// The reference expects the tab to expand to two columns; the crate's
	// replace_tabs uses TAB_WIDTH = 3, so the exact title spacing differs.
	let dirty = connection(|connection| {
		connection.server_info = Some(McpImplementationDisplay {
			name: "github-mcp-server".to_string(),
			title: Some("GitHub\nMCP\tServer".to_string()),
			version: Some("0.19.0".to_string()),
			description: Some("Access\x1b[31m GitHub".to_string()),
			website_url: None,
		});
		connection.tools = Some(vec![McpToolDisplay {
			name: "search_code".to_string(),
			description: Some("Search\x07 code".to_string()),
			input_schema: Some(serde_json::json!({ "type": "object" })),
			..McpToolDisplay::default()
		}]);
		connection.instructions = Some("Prefer\x1b[1m search_code".to_string());
	});
	let snap = snapshot_mcp_runtime(
		&server(|_| {}),
		Some(&source_for(McpConnectionStatus::Connected, Some(dirty))),
		None,
	);
	let title = snap.title.as_deref().expect("title");
	assert!(!title.contains('\n'));
	assert!(!title.contains('\t'));
	assert!(title.starts_with("GitHub"));
	assert!(title.ends_with("Server"));
	assert_eq!(snap.description.as_deref(), Some("Access GitHub"));
	assert_eq!(snap.tools[0].description.as_deref(), Some("Search code"));
	assert_eq!(snap.instructions.as_deref(), Some("Prefer search_code"));
}
