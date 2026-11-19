//! Live MCP runtime view-model for `/extensions`.
//!
//! Ported from `l/src/overlays/extensions/mcp-runtime.ts`.
//!
//! Discovery yields config (`MCPServer`); the live connection lives in the
//! manager. This module joins them by server name without stuffing runtime
//! objects into `Extension.raw`.
//!
//! Deviations:
//! - `MCPServerConfigDisplay`/`MCPServerDisplay` are one flat Rust struct; the
//!   TypeScript interface split exists only for documentation.
//! - `_source` maps to [`McpServerDisplay::source`] (`type` maps to `type_`,
//!   a Rust keyword).
//! - `unknown` is `serde_json::Value`.
//! - `MCPRuntimeSource` is a trait, so tests stub it without a manager.
//! - Sanitizers come from `crate::overlays::extensions::display_text`
//!   (`l/src/overlays/extensions/display-text.ts`), a sibling module.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::overlays::extensions::display_text::{
    sanitize_display_field, sanitize_display_line, sanitize_display_line_field,
    sanitize_display_text,
};
use crate::render::render_utils::PREVIEW_LIMITS;

/// Connection fields actually consumed by the dashboard.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpServerConfigDisplay {
    /// `type` in the wire config.
    pub type_: Option<String>,
    /// `transport` in the wire config.
    pub transport: Option<String>,
    /// stdio command.
    pub command: Option<String>,
    /// stdio args.
    pub args: Option<Vec<String>>,
    /// Remote URL.
    pub url: Option<String>,
    /// Environment entries.
    pub env: Option<BTreeMap<String, Value>>,
}

/// A discovered MCP server config.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpServerDisplay {
    /// `type` in the wire config.
    pub type_: Option<String>,
    /// `transport` in the wire config.
    pub transport: Option<String>,
    /// stdio command.
    pub command: Option<String>,
    /// stdio args.
    pub args: Option<Vec<String>>,
    /// Remote URL.
    pub url: Option<String>,
    /// Environment entries.
    pub env: Option<BTreeMap<String, Value>>,
    /// Server name.
    pub name: String,
    /// Whether the config is enabled.
    pub enabled: Option<bool>,
    /// The original discovery record (`_source`).
    pub source: Option<Value>,
}

impl McpServerDisplay {
    /// Parse a discovery record; `None` unless it has a string `name` and a
    /// `_source` field.
    pub fn from_value(raw: &Value) -> Option<Self> {
        if !is_discovered_mcp_server(raw) {
            return None;
        }
        let obj = raw.as_object()?;
        Some(McpServerDisplay {
            type_: string_field(obj, "type"),
            transport: string_field(obj, "transport"),
            command: string_field(obj, "command"),
            args: string_array_field(obj, "args"),
            url: string_field(obj, "url"),
            env: env_field(obj.get("env")),
            name: obj.get("name")?.as_str()?.to_string(),
            enabled: obj.get("enabled").and_then(Value::as_bool),
            source: obj.get("_source").cloned(),
        })
    }
}

/// The manager's implementation record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpImplementationDisplay {
    /// Implementation name.
    pub name: String,
    /// Implementation version.
    pub version: Option<String>,
    /// Human title.
    pub title: Option<String>,
    /// Human description.
    pub description: Option<String>,
    /// Project website.
    pub website_url: Option<String>,
}

/// A resource or prompt catalog entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpResourceDisplay {
    /// Entry name.
    pub name: String,
    /// Human title.
    pub title: Option<String>,
    /// Human description.
    pub description: Option<String>,
    /// Resource URI.
    pub uri: Option<String>,
    /// Resource template URI.
    pub uri_template: Option<String>,
}

/// Annotations on an MCP tool.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpToolAnnotations {
    /// Human title.
    pub title: Option<String>,
}

/// An MCP tool entry from a live connection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpToolDisplay {
    /// Tool name.
    pub name: String,
    /// Human title.
    pub title: Option<String>,
    /// Human description.
    pub description: Option<String>,
    /// Tool annotations.
    pub annotations: Option<McpToolAnnotations>,
    /// MCP `inputSchema`.
    pub input_schema: Option<Value>,
}

/// A live MCP connection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpConnectionDisplay {
    /// Server name.
    pub name: String,
    /// Server config.
    pub config: McpServerConfigDisplay,
    /// Implementation info.
    pub server_info: Option<McpImplementationDisplay>,
    /// `initialize.instructions`.
    pub instructions: Option<String>,
    /// Live tools.
    pub tools: Option<Vec<McpToolDisplay>>,
    /// Live resources.
    pub resources: Option<Vec<McpResourceDisplay>>,
    /// Live resource templates.
    pub resource_templates: Option<Vec<McpResourceDisplay>>,
    /// Live prompts.
    pub prompts: Option<Vec<McpResourceDisplay>>,
}

/// A tool record from `getTools()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpManagerTool {
    /// Owning server name.
    pub mcp_server_name: Option<String>,
    /// Tool name.
    pub mcp_tool_name: Option<String>,
    /// Human description.
    pub description: Option<String>,
    /// Display label (`server/tool`).
    pub label: Option<String>,
    /// Bridged parameters.
    pub parameters: Option<Value>,
}

/// Resources and templates from `getServerResources()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpServerResources {
    /// Resources.
    pub resources: Vec<McpResourceDisplay>,
    /// Resource templates.
    pub templates: Vec<McpResourceDisplay>,
}

/// Connection status reported by the manager.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McpConnectionStatus {
    /// Connected.
    Connected,
    /// Connecting.
    Connecting,
    /// Disconnected.
    #[default]
    Disconnected,
}

/// Dashboard-facing connection health.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McpConnectionHealth {
    /// Connected.
    Connected,
    /// Connecting.
    Connecting,
    /// Disconnected.
    Disconnected,
    /// Switched off.
    #[default]
    Inactive,
}

/// MCP transport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McpTransport {
    /// stdio.
    #[default]
    Stdio,
    /// SSE.
    Sse,
    /// HTTP.
    Http,
}

impl McpTransport {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            McpTransport::Stdio => "stdio",
            McpTransport::Sse => "sse",
            McpTransport::Http => "http",
        }
    }
}

/// A catalog item in the runtime snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpRuntimeCatalogItem {
    /// Item name.
    pub name: String,
    /// Human title.
    pub title: Option<String>,
    /// Human description.
    pub description: Option<String>,
    /// MCP `inputSchema` / bridged `parameters`.
    pub parameters: Option<Value>,
}

/// The joined runtime view of one server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpRuntimeSnapshot {
    /// Connection health.
    pub health: McpConnectionHealth,
    /// Transport.
    pub transport: McpTransport,
    /// Display title.
    pub title: Option<String>,
    /// Display description.
    pub description: Option<String>,
    /// Implementation website.
    pub website_url: Option<String>,
    /// Implementation name.
    pub implementation_name: Option<String>,
    /// Implementation version.
    pub implementation_version: Option<String>,
    /// `initialize.instructions`.
    pub instructions: Option<String>,
    /// Tools.
    pub tools: Vec<McpRuntimeCatalogItem>,
    /// Resources (and templates).
    pub resources: Vec<McpRuntimeCatalogItem>,
    /// Prompts.
    pub prompts: Vec<McpRuntimeCatalogItem>,
    /// stdio command.
    pub command: Option<String>,
    /// stdio args.
    pub args: Option<Vec<String>>,
    /// Remote URL.
    pub url: Option<String>,
    /// Number of env entries.
    pub env_count: usize,
}

/// Narrow manager surface so tests can stub without constructing `MCPManager`.
pub trait McpRuntimeSource: Send + Sync {
    /// Connection status for `name`.
    fn get_connection_status(&self, name: &str) -> McpConnectionStatus;
    /// Live connection for `name`.
    fn get_connection(&self, name: &str) -> Option<McpConnectionDisplay>;
    /// Every live tool.
    fn get_tools(&self) -> Vec<McpManagerTool>;
    /// Resources and templates for `name`.
    fn get_server_resources(&self, _name: &str) -> Option<McpServerResources> {
        None
    }
    /// Prompts for `name`.
    fn get_server_prompts(&self, _name: &str) -> Option<Vec<McpResourceDisplay>> {
        None
    }
}

/// Fields shared by a server config and a discovered server.
pub trait McpTransportInfo {
    /// Configured transport.
    fn transport(&self) -> Option<&str>;
    /// Configured type.
    fn type_(&self) -> Option<&str>;
    /// Configured URL.
    fn url(&self) -> Option<&str>;
}

impl McpTransportInfo for McpServerConfigDisplay {
    fn transport(&self) -> Option<&str> {
        self.transport.as_deref()
    }
    fn type_(&self) -> Option<&str> {
        self.type_.as_deref()
    }
    fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

impl McpTransportInfo for McpServerDisplay {
    fn transport(&self) -> Option<&str> {
        self.transport.as_deref()
    }
    fn type_(&self) -> Option<&str> {
        self.type_.as_deref()
    }
    fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }
}

const DEFAULT_VISIBLE_TOOLS: usize = PREVIEW_LIMITS.COLLAPSED_ITEMS;

/// True when `raw` is a discovered MCP server (a name plus a `_source`).
pub fn is_discovered_mcp_server(raw: &Value) -> bool {
    let Some(obj) = raw.as_object() else {
        return false;
    };
    obj.get("name").map(Value::is_string).unwrap_or(false) && obj.contains_key("_source")
}

/// Infer the transport from `transport`, `type`, or a non-empty `url`.
pub fn infer_mcp_transport(server: &impl McpTransportInfo) -> McpTransport {
    if let Some(value) = server.transport() {
        if let Some(transport) = parse_transport(value) {
            return transport;
        }
    }
    if let Some(value) = server.type_() {
        if let Some(transport) = parse_transport(value) {
            return transport;
        }
    }
    if let Some(url) = server.url() {
        if !url.is_empty() {
            return McpTransport::Http;
        }
    }
    McpTransport::Stdio
}

fn parse_transport(value: &str) -> Option<McpTransport> {
    match value {
        "stdio" => Some(McpTransport::Stdio),
        "sse" => Some(McpTransport::Sse),
        "http" => Some(McpTransport::Http),
        _ => None,
    }
}

fn catalog_item(
    name: &str,
    title: Option<&str>,
    description: Option<&str>,
    parameters: Option<Value>,
) -> McpRuntimeCatalogItem {
    let clean_name = sanitize_display_line(name);
    let clean_title = sanitize_display_line_field(title);
    let clean_description = sanitize_display_field(description);
    McpRuntimeCatalogItem {
        name: clean_name.clone(),
        title: match clean_title {
            Some(title) if !title.is_empty() && title != clean_name => Some(title),
            _ => None,
        },
        description: clean_description.filter(|description| !description.is_empty()),
        parameters,
    }
}

fn tools_from_manager(manager: &dyn McpRuntimeSource, server_name: &str) -> Vec<McpRuntimeCatalogItem> {
    let mut items = Vec::new();
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    for tool in manager.get_tools() {
        if tool.mcp_server_name.as_deref() != Some(server_name) {
            continue;
        }
        let name = tool
            .mcp_tool_name
            .clone()
            .or_else(|| tool.label.as_deref().map(|label| label.rsplit('/').next().unwrap_or(label).to_string()))
            .or_else(|| tool.label.clone());
        let Some(name) = name else { continue };
        if name.is_empty() || seen.contains_key(&name) {
            continue;
        }
        seen.insert(name.clone(), true);
        items.push(catalog_item(
            &name,
            None,
            tool.description.as_deref(),
            tool.parameters.clone(),
        ));
    }
    items
}

fn resources_from(
    connection: &McpConnectionDisplay,
    manager: Option<&dyn McpRuntimeSource>,
) -> Vec<McpRuntimeCatalogItem> {
    let listed = manager.and_then(|manager| manager.get_server_resources(&connection.name));
    let resources = listed
        .as_ref()
        .map(|listed| listed.resources.as_slice())
        .or(connection.resources.as_deref())
        .unwrap_or_default();
    let templates = listed
        .as_ref()
        .map(|listed| listed.templates.as_slice())
        .or(connection.resource_templates.as_deref())
        .unwrap_or_default();
    let mut items = Vec::new();
    for resource in resources {
        items.push(catalog_item(
            &resource.name,
            resource.title.as_deref(),
            resource.description.as_deref().or(resource.uri.as_deref()),
            None,
        ));
    }
    for template in templates {
        items.push(catalog_item(
            &template.name,
            template.title.as_deref(),
            template.description.as_deref().or(template.uri_template.as_deref()),
            None,
        ));
    }
    items
}

fn prompts_from(
    connection: &McpConnectionDisplay,
    manager: Option<&dyn McpRuntimeSource>,
) -> Vec<McpRuntimeCatalogItem> {
    let listed = manager
        .and_then(|manager| manager.get_server_prompts(&connection.name))
        .or_else(|| connection.prompts.clone())
        .unwrap_or_default();
    listed
        .iter()
        .map(|prompt| catalog_item(&prompt.name, prompt.title.as_deref(), prompt.description.as_deref(), None))
        .collect()
}

/// Join a live connection onto a discovered config.
pub fn snapshot_mcp_runtime(
    server: &McpServerDisplay,
    manager: Option<&dyn McpRuntimeSource>,
    opts: Option<McpRuntimeOpts>,
) -> McpRuntimeSnapshot {
    let opts = opts.unwrap_or_default();
    let enabled = opts.enabled.unwrap_or(server.enabled.unwrap_or(true));
    let transport = infer_mcp_transport(server);
    let env_count = server.env.as_ref().map(BTreeMap::len).unwrap_or(0);
    let mut base = McpRuntimeSnapshot {
        health: if enabled {
            McpConnectionHealth::Disconnected
        } else {
            McpConnectionHealth::Inactive
        },
        transport,
        command: sanitize_display_field(server.command.as_deref()),
        args: server.args.as_ref().map(|args| args.iter().map(|arg| sanitize_display_text(arg)).collect()),
        url: sanitize_display_field(server.url.as_deref()),
        env_count,
        ..McpRuntimeSnapshot::default()
    };

    // Shadowed same-name configs share a name with the winner. Joining by
    // server.name would steal the live connection's health/tools/instructions.
    if opts.shadowed || !enabled || manager.is_none() {
        return base;
    }
    let manager = manager.expect("manager checked");

    let health = match manager.get_connection_status(&server.name) {
        McpConnectionStatus::Connected => McpConnectionHealth::Connected,
        McpConnectionStatus::Connecting => McpConnectionHealth::Connecting,
        McpConnectionStatus::Disconnected => McpConnectionHealth::Disconnected,
    };
    let connection = manager.get_connection(&server.name);
    let identity = identity_from(connection.as_ref().and_then(|connection| connection.server_info.as_ref()), &server.name);
    let connected_tools = connection
        .as_ref()
        .and_then(|connection| connection.tools.as_deref())
        .unwrap_or_default()
        .iter()
        .map(|tool| {
            catalog_item(
                &tool.name,
                tool.title.as_deref().or(tool.annotations.as_ref().and_then(|a| a.title.as_deref())),
                tool.description.as_deref(),
                tool.input_schema.clone(),
            )
        })
        .collect::<Vec<_>>();
    let tools = if !connected_tools.is_empty() {
        connected_tools
    } else {
        tools_from_manager(manager, &server.name)
    };

    base.health = health;
    base.title = identity.title;
    base.description = identity.description;
    base.website_url = identity.website_url;
    base.implementation_name = identity.implementation_name;
    base.implementation_version = identity.implementation_version;
    base.instructions = sanitize_display_field(connection.as_ref().and_then(|connection| connection.instructions.as_deref()));
    base.tools = tools;
    base.resources = match &connection {
        Some(connection) => resources_from(connection, Some(manager)),
        None => Vec::new(),
    };
    base.prompts = match &connection {
        Some(connection) => prompts_from(connection, Some(manager)),
        None => Vec::new(),
    };
    base.command = sanitize_display_field(server.command.as_deref()).or_else(|| {
        connection
            .as_ref()
            .and_then(|connection| connection.config.command.as_deref())
            .and_then(|s| sanitize_display_field(Some(s)))
    });
    base.args = server
        .args
        .as_ref()
        .map(|args| args.iter().map(|arg| sanitize_display_text(arg)).collect())
        .or_else(|| {
            connection
                .as_ref()
                .and_then(|connection| connection.config.args.as_ref())
                .map(|args| args.iter().map(|arg| sanitize_display_text(arg)).collect())
        });
    base.url = sanitize_display_field(server.url.as_deref()).or_else(|| {
        connection
            .as_ref()
            .and_then(|connection| connection.config.url.as_deref())
            .and_then(|s| sanitize_display_field(Some(s)))
    });
    base.transport = match &connection {
        Some(connection) => infer_mcp_transport(&connection.config),
        None => transport,
    };
    base
}

/// Options for [`snapshot_mcp_runtime`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct McpRuntimeOpts {
    /// Override the config's enabled flag.
    pub enabled: Option<bool>,
    /// The config is a shadowed same-name copy.
    pub shadowed: bool,
}

/// The joined identity fields of a snapshot.
struct McpIdentity {
    title: Option<String>,
    description: Option<String>,
    website_url: Option<String>,
    implementation_name: Option<String>,
    implementation_version: Option<String>,
}

fn identity_from(info: Option<&McpImplementationDisplay>, fallback_name: &str) -> McpIdentity {
    let Some(info) = info else {
        return McpIdentity {
            title: None,
            description: None,
            website_url: None,
            implementation_name: None,
            implementation_version: None,
        };
    };
    let implementation_name = sanitize_display_line_field(Some(info.name.as_str()));
    let display_title = sanitize_display_line_field(info.title.as_deref());
    McpIdentity {
        title: match display_title {
            Some(title) if title != fallback_name => Some(title),
            _ => None,
        },
        description: sanitize_display_field(info.description.as_deref()),
        website_url: sanitize_display_line_field(info.website_url.as_deref()),
        implementation_name,
        implementation_version: sanitize_display_line_field(info.version.as_deref()),
    }
}

/// The list-row hint for a snapshot.
pub fn format_mcp_list_hint(snapshot: &McpRuntimeSnapshot) -> String {
    match snapshot.health {
        McpConnectionHealth::Inactive => "inactive".to_string(),
        McpConnectionHealth::Connecting => "connecting…".to_string(),
        McpConnectionHealth::Disconnected => "unavailable".to_string(),
        McpConnectionHealth::Connected => {
            let mut parts = vec![format!(
                "{} tool{}",
                snapshot.tools.len(),
                if snapshot.tools.len() == 1 { "" } else { "s" }
            )];
            if !snapshot.resources.is_empty() {
                parts.push(format!(
                    "{} resource{}",
                    snapshot.resources.len(),
                    if snapshot.resources.len() == 1 { "" } else { "s" }
                ));
            }
            if !snapshot.prompts.is_empty() {
                parts.push(format!(
                    "{} prompt{}",
                    snapshot.prompts.len(),
                    if snapshot.prompts.len() == 1 { "" } else { "s" }
                ));
            }
            parts.join(" · ")
        }
    }
}

/// The health label shown in the inspector.
pub fn format_mcp_health_label(health: McpConnectionHealth) -> &'static str {
    match health {
        McpConnectionHealth::Connected => "Connected",
        McpConnectionHealth::Connecting => "Connecting",
        McpConnectionHealth::Disconnected => "Not connected",
        McpConnectionHealth::Inactive => "Inactive",
    }
}

/// The visible prefix of `tools` and how many rows it hides.
pub struct VisibleMcpTools {
    /// Rows to show.
    pub shown: Vec<McpRuntimeCatalogItem>,
    /// Rows hidden past `limit`.
    pub hidden: usize,
}

/// Split `tools` at `limit`.
pub fn visible_mcp_tools(tools: &[McpRuntimeCatalogItem], limit: usize) -> VisibleMcpTools {
    if tools.len() <= limit {
        return VisibleMcpTools {
            shown: tools.to_vec(),
            hidden: 0,
        };
    }
    VisibleMcpTools {
        shown: tools[..limit].to_vec(),
        hidden: tools.len() - limit,
    }
}

/// The default visible tool count (`PREVIEW_LIMITS.COLLAPSED_ITEMS`).
pub fn default_visible_tools() -> usize {
    DEFAULT_VISIBLE_TOOLS
}

fn string_field(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(Value::as_str).map(str::to_string)
}

fn string_array_field(obj: &serde_json::Map<String, Value>, key: &str) -> Option<Vec<String>> {
    let value = obj.get(key)?;
    let array = value.as_array()?;
    Some(
        array
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

fn env_field(value: Option<&Value>) -> Option<BTreeMap<String, Value>> {
    let value = value?.as_object()?;
    Some(value.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn server(value: Value) -> McpServerDisplay {
        McpServerDisplay::from_value(&value).expect("discovered server")
    }

    struct StubManager {
        status: McpConnectionStatus,
        connection: Option<McpConnectionDisplay>,
        tools: Vec<McpManagerTool>,
    }

    impl McpRuntimeSource for StubManager {
        fn get_connection_status(&self, _name: &str) -> McpConnectionStatus {
            self.status
        }
        fn get_connection(&self, _name: &str) -> Option<McpConnectionDisplay> {
            self.connection.clone()
        }
        fn get_tools(&self) -> Vec<McpManagerTool> {
            self.tools.clone()
        }
    }

    #[test]
    fn discovered_server_requires_name_and_source() {
        let cases = [
            (json!({"name": "github", "_source": {}}), true),
            (json!({"name": "github"}), false),
            (json!({"_source": {}}), false),
            (json!(null), false),
            (json!("github"), false),
        ];
        for (raw, expected) in cases {
            assert_eq!(is_discovered_mcp_server(&raw), expected, "{raw}");
        }
    }

    #[test]
    fn transport_precedence_stdio_type_url() {
        let cases = [
            (json!({"transport": "sse", "type": "http", "url": "https://x"}), "sse"),
            (json!({"type": "http", "url": "https://x"}), "http"),
            (json!({"url": "https://x"}), "http"),
            (json!({"url": ""}), "stdio"),
            (json!({}), "stdio"),
        ];
        for (raw, expected) in cases {
            let display = server(raw);
            assert_eq!(infer_mcp_transport(&display).as_str(), expected, "{display:?}");
        }
    }

    #[test]
    fn snapshot_disabled_is_inactive_without_manager() {
        let display = server(json!({"name": "github", "enabled": false, "_source": {}}));
        let snapshot = snapshot_mcp_runtime(&display, None, None);
        assert_eq!(snapshot.health, McpConnectionHealth::Inactive);
        assert_eq!(snapshot.env_count, 0);
    }

    #[test]
    fn snapshot_joins_connection_and_tools() {
        let display = server(json!({
            "name": "github",
            "command": "npx",
            "args": ["-y", "x"],
            "env": {"A": 1, "B": 2},
            "_source": {"_shadowed": false}
        }));
        let connection = McpConnectionDisplay {
            name: "github".to_string(),
            server_info: Some(McpImplementationDisplay {
                name: "gh".to_string(),
                title: "GitHub".to_string(),
                ..McpImplementationDisplay::default()
            }),
            instructions: Some("be nice".to_string()),
            tools: Some(vec![McpToolDisplay {
                name: "search".to_string(),
                annotations: Some(McpToolAnnotations { title: Some("Search".to_string()) }),
                ..McpToolDisplay::default()
            }]),
            resources: Some(vec![McpResourceDisplay {
                name: "r1".to_string(),
                uri: Some("file:///r1".to_string()),
                ..McpResourceDisplay::default()
            }]),
            prompts: Some(vec![McpResourceDisplay {
                name: "p1".to_string(),
                ..McpResourceDisplay::default()
            }]),
            ..McpConnectionDisplay::default()
        };
        let manager = StubManager {
            status: McpConnectionStatus::Connected,
            connection: Some(connection),
            tools: vec![],
        };
        let snapshot = snapshot_mcp_runtime(&display, Some(&manager), None);
        assert_eq!(snapshot.health, McpConnectionHealth::Connected);
        assert_eq!(snapshot.title.as_deref(), Some("GitHub"));
        assert_eq!(snapshot.implementation_name.as_deref(), Some("gh"));
        assert_eq!(snapshot.instructions.as_deref(), Some("be nice"));
        assert_eq!(snapshot.tools.len(), 1);
        assert_eq!(snapshot.tools[0].title.as_deref(), Some("Search"));
        assert_eq!(snapshot.resources.len(), 1);
        assert_eq!(snapshot.prompts.len(), 1);
        assert_eq!(snapshot.env_count, 2);
        assert_eq!(snapshot.command.as_deref(), Some("npx"));
        assert_eq!(snapshot.transport, McpTransport::Stdio);
    }

    #[test]
    fn shadowed_snapshot_skips_manager_join() {
        let display = server(json!({"name": "github", "_source": {}}));
        let manager = StubManager {
            status: McpConnectionStatus::Connected,
            connection: None,
            tools: vec![],
        };
        let snapshot = snapshot_mcp_runtime(
            &display,
            Some(&manager),
            Some(McpRuntimeOpts {
                shadowed: true,
                ..McpRuntimeOpts::default()
            }),
        );
        assert_eq!(snapshot.health, McpConnectionHealth::Disconnected);
        assert!(snapshot.tools.is_empty());
    }

    #[test]
    fn tools_from_manager_split_label_and_dedupe() {
        let manager = StubManager {
            status: McpConnectionStatus::Connected,
            connection: None,
            tools: vec![
                McpManagerTool {
                    mcp_server_name: Some("github".to_string()),
                    label: Some("github/search".to_string()),
                    ..McpManagerTool::default()
                },
                McpManagerTool {
                    mcp_server_name: Some("github".to_string()),
                    label: Some("github/search".to_string()),
                    ..McpManagerTool::default()
                },
                McpManagerTool {
                    mcp_server_name: Some("other".to_string()),
                    mcp_tool_name: Some("skip".to_string()),
                    ..McpManagerTool::default()
                },
            ],
        };
        let items = tools_from_manager(&manager, "github");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "search");
    }

    #[test]
    fn list_hint_and_health_labels() {
        let snapshot = McpRuntimeSnapshot {
            health: McpConnectionHealth::Connected,
            tools: vec![McpRuntimeCatalogItem {
                name: "a".to_string(),
                ..McpRuntimeCatalogItem::default()
            }],
            resources: vec![
                McpRuntimeCatalogItem {
                    name: "r".to_string(),
                    ..McpRuntimeCatalogItem::default()
                },
                McpRuntimeCatalogItem {
                    name: "s".to_string(),
                    ..McpRuntimeCatalogItem::default()
                },
            ],
            ..McpRuntimeSnapshot::default()
        };
        assert_eq!(format_mcp_list_hint(&snapshot), "1 tool · 2 resources");
        let cases = [
            (McpConnectionHealth::Inactive, "inactive"),
            (McpConnectionHealth::Connecting, "connecting…"),
            (McpConnectionHealth::Disconnected, "unavailable"),
        ];
        for (health, expected) in cases {
            assert_eq!(format_mcp_list_hint(&McpRuntimeSnapshot { health, ..McpRuntimeSnapshot::default() }), expected);
        }
        assert_eq!(format_mcp_health_label(McpConnectionHealth::Connected), "Connected");
        assert_eq!(format_mcp_health_label(McpConnectionHealth::Disconnected), "Not connected");
    }

    #[test]
    fn visible_tools_splits_at_limit() {
        let tools: Vec<McpRuntimeCatalogItem> = (0..5)
            .map(|index| McpRuntimeCatalogItem {
                name: format!("t{index}"),
                ..McpRuntimeCatalogItem::default()
            })
            .collect();
        let all = visible_mcp_tools(&tools, 8);
        assert_eq!(all.shown.len(), 5);
        assert_eq!(all.hidden, 0);
        let cut = visible_mcp_tools(&tools, 2);
        assert_eq!(cut.shown.len(), 2);
        assert_eq!(cut.hidden, 3);
        assert_eq!(default_visible_tools(), 8);
    }
}
