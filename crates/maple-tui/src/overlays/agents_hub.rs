//! Fullscreen `/agents` hub data and pure helpers.
//!
//! Reference: `l/src/overlays/agents-hub.ts`.
//!
//! Deviations:
//! - `Component` UI, `ModelBrowser`, `HubFrame` rendering, fuzzy matching index
//!   integration, async generation flows, and key/mouse handling are not ported.
//!   This module ports exported data types, constants, enums, and pure parsing/
//!   matching/list helpers plus host traits for discovery/settings/generation.
//! - `@oh-my-pi/pi-ai` `Model`, `FuzzyQuery`, and chrome/layout/native types are
//!   abstracted. `match_agent` takes precomputed token-match predicate inputs
//!   (or host `FuzzyQuery` equivalents) is not possible without fuzzy engine;
//!   provided `agent_search_text` and `tokens_all_match` helpers preserve the
//!   searchable text contract.
//! - `AgentsHubComponent` interactive class is represented by [`AgentsHubState`]
//!   and host dependency trait, not a renderable component.

use serde::{Deserialize, Serialize};

/// Agent source scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSource {
	Project,
	User,
	Bundled,
}

/// One agent with per-agent settings overrides resolved for display.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubAgent {
	pub name: String,
	pub description: String,
	pub system_prompt: String,
	pub source_label: String, // fallback label if enum mapping external
	pub source: Option<AgentSource>,
	pub file_path: Option<String>,
	pub model: Option<Vec<String>>,
	pub prewalk: Option<serde_json::Value>,
	pub advisor: Option<serde_json::Value>,
	pub disabled: bool,
	/// `task.agentModelOverrides[name]` comma-joined pattern list.
	pub override_model: Option<String>,
	/// `task.agentPrewalk[name]`: "on", "off", or model pattern.
	pub prewalk_override: Option<String>,
	/// `task.agentAdvisor[name]`: "on", "off", or model pattern.
	pub advisor_override: Option<String>,
}

/// Source display labels.
#[must_use]
pub fn source_label(source: AgentSource) -> &'static str {
	match source {
		AgentSource::Project => "Project",
		AgentSource::User => "User",
		AgentSource::Bundled => "Bundled",
	}
}

/// Source sort order.
#[must_use]
pub fn source_order(source: AgentSource) -> u8 {
	match source {
		AgentSource::Project => 0,
		AgentSource::User => 1,
		AgentSource::Bundled => 2,
	}
}

/// Ordered sources.
pub const SOURCES: &[AgentSource] = &[AgentSource::Project, AgentSource::User, AgentSource::Bundled];

/// Per-agent override columns (logical; priority lower hides first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentColumn {
	pub id: &'static str,
	pub head: &'static str,
	pub format: &'static str,
	pub priority: u8,
	pub min: Option<u8>,
}

pub const AGENT_COLUMNS: &[AgentColumn] = &[
	AgentColumn { id: "model", head: "Model", format: "text", priority: 4, min: Some(8) },
	AgentColumn { id: "prewalk", head: "Prewalk", format: "text", priority: 2, min: None },
	AgentColumn { id: "advisor", head: "Advisor", format: "text", priority: 3, min: None },
];

pub const SOURCE_COLUMN: AgentColumn = AgentColumn {
	id: "source",
	head: "Source",
	format: "dim",
	priority: 1,
	min: None,
};

/// Sidebar entry kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SidebarKind {
	All,
	Source,
	New,
	Separator,
}

/// Body row in agent list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListRow {
	Agent(HubAgent),
	New,
}

/// Per-agent property kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyKind {
	Model,
	Prewalk,
	Advisor,
}

/// Generated agent spec from model output.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedAgentSpec {
	pub identifier: String,
	#[serde(rename = "whenToUse")]
	pub when_to_use: String,
	#[serde(rename = "systemPrompt")]
	pub system_prompt: String,
}

/// Host dependencies for agents hub.
pub trait AgentsHubDeps {
	fn load_agents(&self) -> Result<Vec<HubAgent>, String>;
	fn effective_model_patterns(&self, agent: &HubAgent) -> Vec<String>;
	fn resolve_patterns(&self, patterns: &[String]) -> Option<String>;
	fn effective_prewalk_pattern(&self, agent: &HubAgent) -> Option<String>;
	fn effective_advisor_pattern(&self, agent: &HubAgent) -> Option<String>;
	fn set_agent_disabled(&self, name: &str, disabled: bool);
	fn set_agent_override(&self, property: PropertyKind, name: &str, value: Option<&str>);
	fn generate_agent(&self, description: &str) -> Result<String, String>;
	fn save_agent(&self, scope: &str, spec: &GeneratedAgentSpec) -> Result<String, String>;
}

/// Hub callbacks.
pub trait AgentsHubCallbacks {
	fn on_cancel(&self);
}

/// Identifier validation pattern: lowercase kebab-case, 2+ words.
pub const IDENTIFIER_PATTERN: &str = r"^[a-z0-9]+(?:-[a-z0-9]+){1,5}$";

fn identifier_is_valid(identifier: &str) -> bool {
	// simple validation equivalent to regex intent
	if identifier.is_empty() || identifier.starts_with('-') || identifier.ends_with('-') {
		return false;
	}
	let parts: Vec<&str> = identifier.split('-').collect();
	if parts.len() < 2 || parts.len() > 6 {
		return false;
	}
	parts
		.iter()
		.all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

/// Extract a JSON object from raw model output (bare or fenced).
#[must_use]
pub fn extract_json_object(raw: &str) -> String {
	if serde_json::from_str::<serde_json::Value>(raw).is_ok() {
		return raw.to_string();
	}
	// try fenced block
	if let Some(start_fence) = raw.find("```") {
		let after = &raw[start_fence + 3..];
		let after = after.strip_prefix("json").unwrap_or(after);
		let after = after.trim_start_matches(|c: char| c == '\n' || c == '\r' || c == ' ' || c == '\t');
		if let Some(end_fence) = after.find("```") {
			return after[..end_fence].trim().to_string();
		}
	}
	let start = raw.find('{');
	let end = raw.rfind('}');
	if let (Some(s), Some(e)) = (start, end) {
		if e >= s {
			return raw[s..=e].trim().to_string();
		}
	}
	raw.trim().to_string()
}

/// Parse model output into a generated agent spec.
pub fn parse_generated_agent_spec(raw: &str) -> Result<GeneratedAgentSpec, String> {
	let json_text = extract_json_object(raw);
	let parsed: serde_json::Value = serde_json::from_str(&json_text)
		.map_err(|_| "Model output is not a JSON object".to_string())?;
	let obj = parsed
		.as_object()
		.ok_or_else(|| "Model output is not a JSON object".to_string())?;
	let identifier = obj
		.get("identifier")
		.and_then(|v| v.as_str())
		.ok_or_else(|| "Model output is missing required fields (identifier, whenToUse, systemPrompt)".to_string())?;
	let when_to_use = obj
		.get("whenToUse")
		.and_then(|v| v.as_str())
		.ok_or_else(|| "Model output is missing required fields (identifier, whenToUse, systemPrompt)".to_string())?;
	let system_prompt = obj
		.get("systemPrompt")
		.and_then(|v| v.as_str())
		.ok_or_else(|| "Model output is missing required fields (identifier, whenToUse, systemPrompt)".to_string())?;
	let identifier = identifier.trim().to_string();
	let when_to_use = when_to_use.trim().to_string();
	let system_prompt = system_prompt.trim().to_string();
	if !identifier_is_valid(&identifier) {
		return Err("Generated identifier is invalid (must be lowercase kebab-case, 2+ words)".to_string());
	}
	if !when_to_use.to_ascii_lowercase().starts_with("use this agent when") {
		return Err("Generated whenToUse must start with 'Use this agent when...'".to_string());
	}
	if system_prompt.is_empty() {
		return Err("Generated systemPrompt is empty".to_string());
	}
	Ok(GeneratedAgentSpec {
		identifier,
		when_to_use,
		system_prompt,
	})
}

/// Stable native list-item id of an agent-list row.
#[must_use]
pub fn list_row_id(row: &ListRow) -> String {
	match row {
		ListRow::New => "new".to_string(),
		ListRow::Agent(agent) => {
			let src = agent
				.source
				.map(|s| match s {
					AgentSource::Project => "project",
					AgentSource::User => "user",
					AgentSource::Bundled => "bundled",
				})
				.unwrap_or(agent.source_label.as_str());
			format!("agent:{src}:{}", agent.name)
		}
	}
}

/// Searchable text for an agent row.
#[must_use]
pub fn agent_search_text(agent: &HubAgent) -> String {
	let src = agent
		.source
		.map(source_label)
		.map(str::to_string)
		.unwrap_or_else(|| agent.source_label.clone());
	format!(
		"{} {} {} {}",
		agent.name,
		agent.description,
		src,
		agent.override_model.as_deref().unwrap_or("")
	)
}

/// Case-insensitive token match helper (approximation of fuzzy token match contract).
#[must_use]
pub fn tokens_all_match(text: &str, tokens: &[String]) -> bool {
	let hay = text.to_ascii_lowercase();
	tokens.iter().all(|t| hay.contains(&t.to_ascii_lowercase()))
}

/// Match agent against prepared string tokens.
#[must_use]
pub fn match_agent(agent: &HubAgent, tokens: &[String]) -> bool {
	tokens_all_match(&agent_search_text(agent), tokens)
}

/// Hub UI state snapshot (data-focused).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentsHubState {
	pub selected_index: usize,
	pub filter_text: String,
	pub loading: bool,
	pub error: Option<String>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn extract_json_object_handles_bare_and_fenced() {
		let bare = r#"{"identifier":"code-helper","whenToUse":"Use this agent when x","systemPrompt":"y"}"#;
		assert_eq!(extract_json_object(bare), bare);
		let fenced = "```json\n{\"identifier\":\"a-b\",\"whenToUse\":\"Use this agent when x\",\"systemPrompt\":\"y\"}\n```";
		assert!(extract_json_object(fenced).contains("\"identifier\""));
	}

	#[test]
	fn parse_generated_agent_spec_validates_fields() {
		let ok = r#"{"identifier":"code-helper","whenToUse":"Use this agent when coding","systemPrompt":"Do it"}"#;
		let spec = parse_generated_agent_spec(ok).expect("spec");
		assert_eq!(spec.identifier, "code-helper");
		let bad_id = r#"{"identifier":"Bad","whenToUse":"Use this agent when coding","systemPrompt":"Do it"}"#;
		assert!(parse_generated_agent_spec(bad_id).is_err());
		let bad_when = r#"{"identifier":"code-helper","whenToUse":"Always","systemPrompt":"Do it"}"#;
		assert!(parse_generated_agent_spec(bad_when).is_err());
		let empty_prompt = r#"{"identifier":"code-helper","whenToUse":"Use this agent when coding","systemPrompt":"   "}"#;
		assert!(parse_generated_agent_spec(empty_prompt).is_err());
	}

	#[test]
	fn list_row_id_and_match_agent() {
		let agent = HubAgent {
			name: "reviewer".into(),
			description: "reviews code".into(),
			source: Some(AgentSource::Project),
			source_label: "Project".into(),
			override_model: Some("gpt-5".into()),
			..Default::default()
		};
		assert_eq!(list_row_id(&ListRow::Agent(agent.clone())), "agent:project:reviewer");
		assert_eq!(list_row_id(&ListRow::New), "new");
		assert!(match_agent(&agent, &["review".into(), "project".into()]));
		assert!(!match_agent(&agent, &["missing".into()]));
	}

	#[test]
	fn source_meta_constants() {
		assert_eq!(source_label(AgentSource::User), "User");
		assert_eq!(source_order(AgentSource::Bundled), 2);
		assert_eq!(SOURCES.len(), 3);
		assert_eq!(AGENT_COLUMNS[0].id, "model");
	}
}
// -----------------------------------------------------------------------------
// Component
// -----------------------------------------------------------------------------

use crate::chrome::overlay_box::{bottom_border, divider, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::keys::{self, Key};
use crate::mouse::MouseEvent;
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props};

/// Fullscreen `/agents` hub component.
///
/// Reference: `AgentsHubComponent` in `l/src/overlays/agents-hub.ts`.
///
/// Deviations: async loading/reload, `ModelBrowser`, `HubFrame` sidebar/body
/// layout, fuzzy index, create flow AI generation/streaming, and host settings
/// persistence are injected via [`AgentsHubDeps`]/callbacks and modeled as
/// state transitions; rendering is bordered text with keyboard/mouse navigation.
pub struct AgentsHubComponent<D: AgentsHubDeps, C: AgentsHubCallbacks> {
	pub all_agents: Vec<HubAgent>,
	pub rows: Vec<ListRow>,
	pub row_index: usize,
	pub filter_text: String,
	pub notice: Option<String>,
	pub error: Option<String>,
	pub loading: bool,
	pub focus_scope: bool,
	pub active_source: Option<AgentSource>,
	deps: D,
	callbacks: C,
	theme: OverlayTheme,
}

impl<D: AgentsHubDeps, C: AgentsHubCallbacks> std::fmt::Debug for AgentsHubComponent<D, C> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AgentsHubComponent")
			.field("rows", &self.rows.len())
			.field("row_index", &self.row_index)
			.finish_non_exhaustive()
	}
}

impl<D: AgentsHubDeps, C: AgentsHubCallbacks> AgentsHubComponent<D, C> {
	pub fn new(deps: D, callbacks: C) -> Self {
		let mut s = Self {
			all_agents: Vec::new(),
			rows: Vec::new(),
			row_index: 0,
			filter_text: String::new(),
			notice: None,
			error: None,
			loading: false,
			focus_scope: false,
			active_source: None,
			deps,
			callbacks,
			theme: OverlayTheme::default(),
		};
		s.reload();
		s
	}

	pub fn set_theme(&mut self, theme: OverlayTheme) {
		self.theme = theme;
	}

	pub fn reload(&mut self) {
		self.loading = true;
		self.error = None;
		match self.deps.load_agents() {
			Ok(mut agents) => {
				agents.sort_by(|a, b| {
					let ao = a.source.map(source_order).unwrap_or(99);
					let bo = b.source.map(source_order).unwrap_or(99);
					ao.cmp(&bo).then_with(|| a.name.cmp(&b.name))
				});
				self.all_agents = agents;
			}
			Err(e) => self.error = Some(e),
		}
		self.loading = false;
		self.build_rows();
	}

	fn build_rows(&mut self) {
		let tokens: Vec<String> = self
			.filter_text
			.split_whitespace()
			.filter(|t| !t.is_empty())
			.map(str::to_string)
			.collect();
		self.rows.clear();
		for a in &self.all_agents {
			if let Some(src) = self.active_source {
				if a.source != Some(src) {
					continue;
				}
			}
			if tokens.is_empty() || match_agent(a, &tokens) {
				self.rows.push(ListRow::Agent(a.clone()));
			}
		}
		self.rows.push(ListRow::New);
		if self.row_index >= self.rows.len() {
			self.row_index = self.rows.len().saturating_sub(1);
		}
	}

	fn selected_agent(&self) -> Option<&HubAgent> {
		match self.rows.get(self.row_index) {
			Some(ListRow::Agent(a)) => Some(a),
			_ => None,
		}
	}

	fn toggle_selected(&mut self) {
		if let Some(ListRow::Agent(a)) = self.rows.get(self.row_index).cloned() {
			self.deps.set_agent_disabled(&a.name, !a.disabled);
			self.notice = Some(format!(
				"{} {}",
				a.name,
				if a.disabled { "enabled" } else { "disabled" }
			));
			self.reload();
		}
	}

	fn move_row(&mut self, delta: isize) {
		if self.rows.is_empty() {
			self.row_index = 0;
			return;
		}
		let cur = self.row_index as isize;
		self.row_index = (cur + delta).clamp(0, self.rows.len().saturating_sub(1) as isize) as usize;
	}

	fn render_rows(&self, width: u16) -> Vec<String> {
		let mut out = vec![top_border(width, "Agents", &self.theme)];
		let scope = self
			.active_source
			.map(|s| source_label(s).to_string())
			.unwrap_or_else(|| "All".to_string());
		out.push(row(
			&format!("Scope: {scope}   Filter: {}", self.filter_text),
			width,
			&self.theme,
		));
		if self.loading {
			out.push(row("Loading…", width, &self.theme));
		}
		if let Some(e) = &self.error {
			out.push(row(&format!("Error: {e}"), width, &self.theme));
		}
		for (i, r) in self.rows.iter().enumerate() {
			let cursor = if i == self.row_index { "›" } else { " " };
			let line = match r {
				ListRow::Agent(a) => {
					let src = a.source.map(source_label).unwrap_or(&a.source_label);
					let dis = if a.disabled { " [disabled]" } else { "" };
					format!("{cursor} {}  ({src}){dis}", a.name)
				}
				ListRow::New => format!("{cursor} + New agent"),
			};
			out.push(row(&line, width, &self.theme));
		}
		out.push(divider(width, &self.theme));
		let hint = self.notice.clone().unwrap_or_else(|| {
			"↑/↓ move · enter open · space toggle · / filter · tab scope · esc cancel".to_string()
		});
		out.push(row(&hint, width, &self.theme));
		out.push(bottom_border(width, &self.theme));
		out
	}
}

impl<D: AgentsHubDeps, C: AgentsHubCallbacks> Component for AgentsHubComponent<D, C> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if keys::is_escape(key) {
			if !self.filter_text.is_empty() {
				self.filter_text.clear();
				self.build_rows();
			} else {
				self.callbacks.on_cancel();
			}
			return;
		}
		if keys::is_arrow_up(key) {
			self.move_row(-1);
			return;
		}
		if keys::is_arrow_down(key) {
			self.move_row(1);
			return;
		}
		if matches!(key, Key::Tab | Key::BackTab) {
			self.active_source = match self.active_source {
				None => Some(AgentSource::Project),
				Some(AgentSource::Project) => Some(AgentSource::User),
				Some(AgentSource::User) => Some(AgentSource::Bundled),
				Some(AgentSource::Bundled) => None,
			};
			self.build_rows();
			return;
		}
		if matches!(key, Key::Char(' ')) {
			self.toggle_selected();
			return;
		}
		if keys::is_enter(key) {
			self.notice = self
				.selected_agent()
				.map(|a| format!("Selected {}", a.name))
				.or(Some("New agent (host create flow)".to_string()));
			return;
		}
		if keys::is_backspace(key) {
			self.filter_text.pop();
			self.build_rows();
			return;
		}
		if let Key::Char(c) = key {
			self.filter_text.push(*c);
			self.row_index = 0;
			self.build_rows();
		}
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		if event.is_scroll_up() {
			self.move_row(-1);
		} else if event.is_scroll_down() {
			self.move_row(1);
		}
	}

	fn invalidate(&mut self) {}
}

impl<D: AgentsHubDeps, C: AgentsHubCallbacks> NativeDescribe for AgentsHubComponent<D, C> {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let lines = self.render_rows(cx.cols.max(20));
		let mut props = Props::new();
		props.set_i64("cols", i64::from(cx.cols.max(20)));
		props.insert(
			"lines",
			serde_json::Value::Array(lines.into_iter().map(serde_json::Value::String).collect()),
		);
		NativeNode::with_props(NodeKind::Rows, props)
	}
}

#[cfg(test)]
mod component_tests {
	use super::*;

	#[derive(Default)]
	struct Deps {
		agents: Vec<HubAgent>,
	}
	impl AgentsHubDeps for Deps {
		fn load_agents(&self) -> Result<Vec<HubAgent>, String> {
			Ok(self.agents.clone())
		}
		fn effective_model_patterns(&self, _agent: &HubAgent) -> Vec<String> { vec![] }
		fn resolve_patterns(&self, _patterns: &[String]) -> Option<String> { None }
		fn effective_prewalk_pattern(&self, _agent: &HubAgent) -> Option<String> { None }
		fn effective_advisor_pattern(&self, _agent: &HubAgent) -> Option<String> { None }
		fn set_agent_disabled(&self, _name: &str, _disabled: bool) {}
		fn set_agent_override(&self, _property: PropertyKind, _name: &str, _value: Option<&str>) {}
		fn generate_agent(&self, _description: &str) -> Result<String, String> { Err("no".into()) }
		fn save_agent(&self, _scope: &str, _spec: &GeneratedAgentSpec) -> Result<String, String> { Err("no".into()) }
	}
	struct Cb;
	impl AgentsHubCallbacks for Cb {
		fn on_cancel(&self) {}
	}

	#[test]
	fn agents_hub_component_renders_filters_and_moves() {
		let deps = Deps {
			agents: vec![
				HubAgent { name: "alpha".into(), source: Some(AgentSource::Project), source_label: "Project".into(), ..Default::default() },
				HubAgent { name: "beta".into(), source: Some(AgentSource::User), source_label: "User".into(), ..Default::default() },
			],
		};
		let mut c = AgentsHubComponent::new(deps, Cb);
		let lines = c.render(70);
		assert!(lines.iter().any(|l| l.contains("Agents")));
		assert!(lines.iter().any(|l| l.contains("alpha")));
		c.handle_input(&Key::Char('b'));
		assert!(c.rows.iter().any(|r| matches!(r, ListRow::Agent(a) if a.name == "beta")));
		c.handle_input(&Key::Down);
		let node = c.describe_native(&DescribeContext { cols: 50, ..Default::default() });
		assert!(matches!(node.kind, NodeKind::Rows));
	}
}
