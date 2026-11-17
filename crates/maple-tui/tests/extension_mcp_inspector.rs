//! Port of `l/test/extension-mcp-inspector.test.ts`.

use std::sync::Arc;

use maple_tui::component::Component;
use maple_tui::overlays::extensions::extension_list::{ExtensionList, ExtensionListCallbacks};
use maple_tui::overlays::extensions::inspector_panel::InspectorPanel;
use maple_tui::overlays::extensions::mcp_runtime::{
	McpConnectionDisplay, McpConnectionStatus, McpImplementationDisplay, McpManagerTool,
	McpResourceDisplay, McpRuntimeSource, McpServerConfigDisplay, McpServerResources, McpToolDisplay,
};
use maple_tui::overlays::extensions::types::{
	Extension, ExtensionKind, ExtensionSource, ExtensionState,
};
use serde_json::json;

fn source_value() -> serde_json::Value {
	json!({
		"provider": "native",
		"providerName": "OMP (User)",
		"path": "/home/sf/.omp/agent/mcp.json",
		"level": "user",
	})
}

fn mcp_raw(command: &str) -> serde_json::Value {
	json!({
		"name": "github",
		"command": command,
		"args": ["stdio"],
		"transport": "stdio",
		"_source": source_value(),
	})
}

fn mcp_extension(state: ExtensionState) -> Extension {
	Extension {
		id: "mcp:github".to_string(),
		kind: ExtensionKind::Mcp,
		name: "github".to_string(),
		display_name: "github".to_string(),
		path: "/home/sf/.omp/agent/mcp.json".to_string(),
		source: ExtensionSource {
			provider: "native".to_string(),
			provider_name: "OMP (User)".to_string(),
			level: "user".to_string(),
		},
		state,
		raw: mcp_raw("/usr/bin/github-mcp-server"),
		..Extension::default()
	}
}

fn tool(name: &str, description: &str, properties: serde_json::Value, required: &[&str]) -> McpToolDisplay {
	McpToolDisplay {
		name: name.to_string(),
		description: Some(description.to_string()),
		input_schema: Some(json!({
			"type": "object",
			"required": required,
			"properties": properties,
		})),
		..McpToolDisplay::default()
	}
}

fn connection(tools: Vec<McpToolDisplay>, resources: Vec<McpResourceDisplay>) -> McpConnectionDisplay {
	McpConnectionDisplay {
		name: "github".to_string(),
		config: McpServerConfigDisplay {
			command: Some("/usr/bin/github-mcp-server".to_string()),
			args: Some(vec!["stdio".to_string()]),
			..McpServerConfigDisplay::default()
		},
		server_info: Some(McpImplementationDisplay {
			name: "github-mcp-server".to_string(),
			version: Some("0.19.0".to_string()),
			title: Some("GitHub MCP Server".to_string()),
			description: Some("Access GitHub repositories, issues, and pull requests.".to_string()),
			website_url: None,
		}),
		instructions: Some("Prefer search_code over cloning.".to_string()),
		tools: Some(tools),
		resources: Some(resources),
		..McpConnectionDisplay::default()
	}
}

struct Connected {
	tools: Vec<McpToolDisplay>,
	resources: Vec<McpResourceDisplay>,
	prompts: Vec<McpResourceDisplay>,
	status: McpConnectionStatus,
}

impl Default for Connected {
	fn default() -> Self {
		Self {
			tools: vec![
				tool(
					"search_code",
					"Search code across GitHub repositories.",
					json!({
						"query": { "type": "string", "description": "Search query" },
						"language": { "type": "string" },
					}),
					&["query"],
				),
				tool(
					"create_issue",
					"Create a new issue in a repository.",
					json!({ "title": { "type": "string", "description": "Issue title" } }),
					&["title"],
				),
			],
			resources: vec![McpResourceDisplay {
				name: "repo".to_string(),
				uri: Some("github://repo".to_string()),
				..McpResourceDisplay::default()
			}],
			prompts: Vec::new(),
			status: McpConnectionStatus::Connected,
		}
	}
}

impl Connected {
	fn into_source(self) -> Box<dyn McpRuntimeSource> {
		Box::new(ConnectedSource(self))
	}
}

struct ConnectedSource(Connected);

impl McpRuntimeSource for ConnectedSource {
	fn get_connection_status(&self, _name: &str) -> McpConnectionStatus {
		self.0.status
	}
	fn get_connection(&self, _name: &str) -> Option<McpConnectionDisplay> {
		if self.0.status == McpConnectionStatus::Connected {
			Some(connection(self.0.tools.clone(), self.0.resources.clone()))
		} else {
			None
		}
	}
	fn get_tools(&self) -> Vec<McpManagerTool> {
		self.0
			.tools
			.iter()
			.map(|tool| McpManagerTool {
				mcp_server_name: Some("github".to_string()),
				mcp_tool_name: Some(tool.name.clone()),
				description: tool.description.clone(),
				label: None,
				parameters: tool.input_schema.clone(),
			})
			.collect()
	}
	fn get_server_resources(&self, _name: &str) -> Option<McpServerResources> {
		Some(McpServerResources {
			resources: self.0.resources.clone(),
			templates: Vec::new(),
		})
	}
	fn get_server_prompts(&self, _name: &str) -> Option<Vec<McpResourceDisplay>> {
		Some(self.0.prompts.clone())
	}
}

fn source_arc(source: Connected) -> Arc<dyn McpRuntimeSource + Send + Sync> {
	Arc::new(ConnectedSource(source))
}

fn render(panel: &InspectorPanel) -> String {
	panel.render(72, usize::MAX).join("\n")
}

fn panel_with(source: Connected) -> InspectorPanel {
	let mut panel = InspectorPanel::new(None);
	panel.set_mcp_source(Some(source.into_source()));
	panel.set_extension(Some(mcp_extension(ExtensionState::Active)));
	panel
}

#[test]
fn shows_identity_health_tools_then_plumbing() {
	let text = render(&panel_with(Connected::default()));
	assert!(text.contains("github"));
	assert!(text.contains("GitHub MCP Server"));
	assert!(text.contains("Access GitHub repositories, issues, and pull requests."));
	assert!(text.contains("Connected"));
	assert!(text.contains("stdio"));
	assert!(text.contains("github-mcp-server 0.19.0"));
	assert!(text.contains("search_code"));
	assert!(text.contains("Search code across GitHub repositories."));
	assert!(text.contains("Prefer search_code over cloning."));
	assert!(!text.contains("Server guidance"));
	assert!(!text.contains("Connection"));
	assert!(text.contains("/usr/bin/github-mcp-server"));
	assert!(!text.contains("Type: mcp"));
	assert!(!text.contains("Status:"));
	assert!(text.find("Access GitHub").unwrap() < text.find("Command").unwrap());
	assert!(text.find("Prefer search_code").unwrap() < text.find("search_code").unwrap());
	assert!(text.find("search_code").unwrap() < text.find("Command").unwrap());
}

#[test]
fn inlines_short_args_and_collapses_long_schemas() {
	let collapsed = render(&panel_with(Connected::default()));
	assert!(collapsed.contains("query"));
	assert!(collapsed.contains("Search query"));
	assert!(collapsed.contains("title"));
	assert!(collapsed.contains("Issue title"));
	assert!(!collapsed.contains("1 arg"));
	assert!(!collapsed.contains("args ("));

	let long = Connected {
		tools: vec![tool(
			"search_code",
			"Search code across GitHub repositories.",
			json!({
				"query": { "type": "string" },
				"a": { "type": "string" },
				"b": { "type": "string" },
				"c": { "type": "string" },
				"d": { "type": "string" },
			}),
			&["query"],
		)],
		..Connected::default()
	};
	let mut panel = panel_with(long);
	let collapsed = render(&panel);
	assert!(collapsed.contains("5 args"));
	assert!(collapsed.contains("args ("));
	assert!(!collapsed.contains("query  string"));

	panel.toggle_expanded();
	let expanded = render(&panel);
	assert!(expanded.contains("query  string"));
	assert!(expanded.contains("Required"));
}

#[test]
fn dead_server_is_not_active() {
	let mut panel = InspectorPanel::new(None);
	panel.set_mcp_source(Some(
		Connected {
			status: McpConnectionStatus::Disconnected,
			..Connected::default()
		}
		.into_source(),
	));
	panel.set_extension(Some(mcp_extension(ExtensionState::Active)));
	let text = render(&panel);
	assert!(text.contains("Not connected"));
	assert!(!text.contains("Status:"));
	assert!(!text.contains("unknown"));
}

#[test]
fn long_commands_wrap_instead_of_ellipsizing() {
	let mut panel = InspectorPanel::new(None);
	panel.set_mcp_source(Some(Connected::default().into_source()));
	panel.set_extension(Some(Extension {
		raw: mcp_raw("/home/sf/worlds/personal/.omp/bin/gog-mcp-readonly"),
		..mcp_extension(ExtensionState::Active)
	}));
	let text = panel.render(42, usize::MAX).join("\n");
	let command = &text[text.find("Command").unwrap()..];
	assert!(!command.lines().any(|line| line.ends_with("...") || line.contains('…')));
	assert!(command.split_whitespace().collect::<String>().contains("gog-mcp-readonly"));
	assert!(!text.contains("Connection"));
}

fn catalogs() -> (Vec<McpResourceDisplay>, Vec<McpResourceDisplay>) {
	let resources = (0..20)
		.map(|i| McpResourceDisplay {
			name: format!("resource_{i}"),
			uri: Some(format!("github://repo/{i}")),
			..McpResourceDisplay::default()
		})
		.collect();
	let prompts = (0..12)
		.map(|i| McpResourceDisplay {
			name: format!("prompt_{i}"),
			..McpResourceDisplay::default()
		})
		.collect();
	(resources, prompts)
}

#[test]
fn large_catalogs_collapse_until_expand() {
	let (resources, prompts) = catalogs();
	let mut panel = panel_with(Connected {
		tools: Vec::new(),
		resources,
		prompts,
		..Connected::default()
	});
	let collapsed = render(&panel);
	assert!(collapsed.contains("resource_0"));
	assert!(!collapsed.contains("resource_19"));
	assert!(collapsed.contains("prompt_0"));
	assert!(!collapsed.contains("prompt_11"));
	assert!(collapsed.contains("more ("));

	panel.toggle_expanded();
	let expanded = render(&panel);
	assert!(expanded.contains("resource_19"));
	assert!(expanded.contains("prompt_11"));
}

#[test]
fn expand_is_per_selected_extension() {
	let (resources, prompts) = catalogs();
	let mut panel = panel_with(Connected {
		tools: Vec::new(),
		resources,
		prompts,
		..Connected::default()
	});
	panel.toggle_expanded();
	assert!(render(&panel).contains("resource_19"));

	panel.set_extension(Some(Extension {
		id: "mcp:linear".to_string(),
		name: "linear".to_string(),
		display_name: "linear".to_string(),
		raw: json!({
			"name": "linear",
			"command": "/usr/bin/linear-mcp",
			"transport": "stdio",
			"_source": source_value(),
		}),
		..mcp_extension(ExtensionState::Active)
	}));
	let other = render(&panel);
	assert!(!other.contains("resource_19"));

	panel.set_extension(Some(mcp_extension(ExtensionState::Active)));
	assert!(!render(&panel).contains("resource_19"));

	panel.toggle_expanded();
	panel.set_extension(Some(mcp_extension(ExtensionState::Active)));
	assert!(render(&panel).contains("resource_19"));
}

#[test]
fn list_row_shows_tool_and_resource_counts() {
	let callbacks = ExtensionListCallbacks {
		mcp_source: Some(source_arc(Connected::default())),
		..Default::default()
	};
	let mut list = ExtensionList::new(vec![mcp_extension(ExtensionState::Active)], callbacks, 15);
	list.set_focused(true);
	let text = list.render(80).join("\n");
	assert!(text.contains("github"));
	assert!(text.contains("2 tools · 1 resource"));
	assert!(!text.contains("stdio"));
}

#[test]
fn list_row_marks_disconnected_server_unavailable() {
	let callbacks = ExtensionListCallbacks {
		mcp_source: Some(source_arc(Connected {
			status: McpConnectionStatus::Disconnected,
			..Connected::default()
		})),
		..Default::default()
	};
	let mut list = ExtensionList::new(vec![mcp_extension(ExtensionState::Active)], callbacks, 15);
	let text = list.render(80).join("\n");
	assert!(text.contains("unavailable"));
}

#[test]
fn shadowed_row_does_not_inherit_health_or_counts() {
	let shadowed = Extension {
		state: ExtensionState::Shadowed,
		shadowed_by: Some("github".to_string()),
		raw: mcp_raw("/usr/bin/shadowed-github"),
		..mcp_extension(ExtensionState::Active)
	};
	let callbacks = ExtensionListCallbacks {
		mcp_source: Some(source_arc(Connected::default())),
		..Default::default()
	};
	let mut list = ExtensionList::new(
		vec![mcp_extension(ExtensionState::Active), shadowed.clone()],
		callbacks,
		15,
	);
	list.set_focused(true);
	let text = list.render(80).join("\n");
	assert!(text.contains("2 tools · 1 resource"));
	let second = text[text.find("2 tools · 1 resource").unwrap() + 1..]
		.find("github")
		.map(|i| i + text.find("2 tools · 1 resource").unwrap() + 1);
	assert!(second.is_some());
	assert!(!text[second.unwrap()..].contains("2 tools"));

	let mut panel = InspectorPanel::new(None);
	panel.set_mcp_source(Some(Connected::default().into_source()));
	panel.set_extension(Some(shadowed));
	let inspector = render(&panel);
	assert!(inspector.contains("Shadowed"));
	assert!(!inspector.contains("Connected"));
	assert!(!inspector.contains("search_code"));
	assert!(!inspector.contains("GitHub MCP Server"));
	assert!(inspector.contains("/usr/bin/shadowed-github"));
}
