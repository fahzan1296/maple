//! Kind-specific view-models for `/extensions`.
//!
//! Reference: `l/src/overlays/extensions/inspector-model.ts`.
//!
//! Deviations:
//! - `arkToWireSchema`/`isArkSchema` from `@oh-my-pi/pi-ai/utils/schema`,
//!   `parseFrontmatter`/`normalizePathForComparison` from `@oh-my-pi/pi-utils`,
//!   and Node `path` semantics have no crate equivalents. Schema conversion
//!   supports plain JSON-schema-like objects and objects with a `toJsonSchema`
//!   method are not callable from Rust; frontmatter parsing and path
//!   normalization are implemented with small local helpers.
//! - `Extension.raw` is `serde_json::Value`.
//! - `LiveToolRecord`/`ToolRuntimeSource` mirror the TS shapes and are
//!   compatible with [`crate::overlays::extensions::live_tool_session`].

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use crate::overlays::extensions::display_text::{
	sanitize_display_field, sanitize_display_line, sanitize_display_line_field, sanitize_display_text,
};
use crate::overlays::extensions::types::{is_shadowed_extension, DisabledReason, Extension, ExtensionState};

/// Live tool record joined from session state.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveToolRecord {
	pub name: String,
	pub label: Option<String>,
	pub description: Option<String>,
	pub parameters: Option<Value>,
	pub hidden: Option<bool>,
	pub load_mode: Option<String>,
	pub source: Option<String>,
	pub source_path: Option<String>,
}

/// Runtime source for live tools.
pub trait ToolRuntimeSource: Send + Sync {
	fn get_live_tool(&self, name: &str) -> Option<LiveToolRecord>;
	fn list_live_tools(&self) -> Option<Vec<LiveToolRecord>> {
		None
	}
}

/// Tool parameter view-model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolParamView {
	pub name: String,
	pub param_type: String,
	pub required: bool,
	pub flag: String,
	pub description: Option<String>,
}

/// Slash command preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPreview {
	pub description: Option<String>,
	pub body: String,
	pub argument_hint: Option<String>,
	pub uses_arguments: bool,
}

/// Parsed rule fields provided by the host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedRule {
	pub condition: Option<Vec<String>>,
	pub ast_condition: Option<Vec<String>>,
	pub scope: Option<Vec<String>>,
	pub agents: Option<Vec<String>>,
}

/// Inspector source hooks.
pub trait ExtensionInspectorSource {
	fn read_tool_header(&self, _file_path: &str) -> Option<String> {
		None
	}
	fn parse_rule(&self, _raw: &serde_json::Map<String, Value>) -> Option<ParsedRule> {
		None
	}
}

#[must_use]
pub fn is_placeholder_tool_description(name: &str, description: Option<&str>) -> bool {
	match description {
		None => true,
		Some(d) if d.trim().is_empty() => true,
		Some(d) => d == format!("{name} custom tool"),
	}
}

/// First JSDoc on a custom-tool file, minus symlink footnotes.
#[must_use]
pub fn parse_tool_file_header(source: &str) -> Option<String> {
	let start = source.find("/**")?;
	let rest = &source[start + 3..];
	let end = rest.find("*/")?;
	let body = &rest[..end];
	let cleaned = body
		.lines()
		.map(|line| {
			let t = line.trim_start();
			t.strip_prefix('*')
				.map(|s| s.strip_prefix(' ').unwrap_or(s))
				.unwrap_or(t)
				.trim_end()
				.to_string()
		})
		.collect::<Vec<_>>()
		.join("\n");
	let paragraphs: Vec<String> = cleaned
		.split("\n\n")
		.map(|p| p.trim().to_string())
		.filter(|p| !p.is_empty() && !p.to_lowercase().starts_with("symlink:"))
		.collect();
	if paragraphs.is_empty() {
		None
	} else {
		sanitize_display_field(Some(paragraphs.join("\n\n").as_str()))
	}
}

fn optional_trimmed_string(value: Option<&Value>) -> Option<String> {
	value
		.and_then(Value::as_str)
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(str::to_string)
}

/// Display fields already parsed from command frontmatter.
#[must_use]
pub fn slash_command_frontmatter_display(
	frontmatter: &serde_json::Map<String, Value>,
) -> (Option<String>, Option<String>) {
	let description = optional_trimmed_string(frontmatter.get("description"));
	let argument_hint = optional_trimmed_string(
		frontmatter
			.get("argumentHint")
			.or_else(|| frontmatter.get("argument-hint")),
	);
	(description, argument_hint)
}

/// Minimal frontmatter parser (`---` delimited header, key: value lines).
fn parse_simple_frontmatter(content: &str) -> (serde_json::Map<String, Value>, String) {
	let mut map = serde_json::Map::new();
	let Some(rest) = content.strip_prefix("---") else {
		return (map, content.to_string());
	};
	let rest = rest.trim_start_matches(['\r', '\n']);
	let Some(end) = rest.find("\n---") else {
		return (map, content.to_string());
	};
	let header = &rest[..end];
	let body_start = end + "\n---".len();
	let mut body = rest[body_start..].to_string();
	if let Some(stripped) = body.strip_prefix('\n') {
		body = stripped.to_string();
	} else if let Some(stripped) = body.strip_prefix("\r\n") {
		body = stripped.to_string();
	}
	for line in header.lines() {
		if let Some((k, v)) = line.split_once(':') {
			let key = k.trim().to_string();
			let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
			if !key.is_empty() {
				map.insert(key, Value::String(value));
			}
		}
	}
	(map, body)
}

#[must_use]
pub fn command_preview(content: Option<&str>) -> CommandPreview {
	let Some(content) = content.filter(|c| !c.is_empty()) else {
		return CommandPreview {
			description: None,
			body: String::new(),
			argument_hint: None,
			uses_arguments: false,
		};
	};
	let (frontmatter, body) = parse_simple_frontmatter(content);
	let (description, argument_hint) = slash_command_frontmatter_display(&frontmatter);
	let uses_arguments = body.contains("$ARGUMENTS");
	CommandPreview {
		description: sanitize_display_field(description.as_deref()),
		body: sanitize_display_text(&body),
		argument_hint: sanitize_display_field(argument_hint.as_deref()),
		uses_arguments,
	}
}

fn as_record(raw: Option<&Value>) -> Option<&serde_json::Map<String, Value>> {
	raw.and_then(Value::as_object)
}

fn string_field(raw: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
	raw.get(key)
		.and_then(Value::as_str)
		.filter(|s| !s.is_empty())
		.map(sanitize_display_text)
		.filter(|s| !s.is_empty())
}

fn string_array(value: Option<&Value>) -> Option<Vec<String>> {
	match value? {
		Value::String(s) => {
			let token = sanitize_display_text(s.trim());
			if token.is_empty() {
				None
			} else {
				Some(vec![token])
			}
		}
		Value::Array(items) => {
			let out: Vec<String> = items
				.iter()
				.filter_map(Value::as_str)
				.filter(|s| !s.is_empty())
				.map(sanitize_display_text)
				.filter(|s| !s.is_empty())
				.collect();
			if out.is_empty() { None } else { Some(out) }
		}
		_ => None,
	}
}

fn as_json_schema(schema: Option<&Value>) -> Option<&serde_json::Map<String, Value>> {
	let schema = schema?;
	schema.as_object()
}

fn properties_from_schema(schema: Option<&Value>) -> Option<&serde_json::Map<String, Value>> {
	let wire = as_json_schema(schema)?;
	let props = wire.get("properties")?.as_object()?;
	if props.is_empty() { Some(props) } else { Some(props) }
}

fn required_from_schema(schema: Option<&Value>) -> HashSet<String> {
	let mut set = HashSet::new();
	if let Some(req) = as_json_schema(schema).and_then(|w| w.get("required")).and_then(Value::as_array) {
		for v in req {
			if let Some(s) = v.as_str() {
				set.insert(s.to_string());
			}
		}
	}
	set
}

fn param_type(spec: &serde_json::Map<String, Value>) -> String {
	if let Some(t) = spec.get("type").and_then(Value::as_str) {
		if !t.is_empty() {
			return sanitize_display_line(t);
		}
	}
	if spec.get("enum").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
		return "enum".to_string();
	}
	if spec.contains_key("anyOf") || spec.contains_key("oneOf") {
		return "union".to_string();
	}
	"any".to_string()
}

#[must_use]
pub fn tool_params_from_schema(schema: Option<&Value>) -> Vec<ToolParamView> {
	let Some(properties) = properties_from_schema(schema) else {
		return Vec::new();
	};
	let required = required_from_schema(schema);
	let mut params = Vec::new();
	for (name, spec) in properties {
		let record = spec.as_object().cloned().unwrap_or_default();
		let is_required = required.contains(name);
		let default_val = record.get("default").map(|v| {
			let text = match v {
				Value::String(s) => sanitize_display_line(s),
				other => sanitize_display_line(&other.to_string()),
			};
			format!("Default: {text}")
		});
		let flag = if is_required {
			"Required".to_string()
		} else {
			default_val.unwrap_or_else(|| "Optional".to_string())
		};
		params.push(ToolParamView {
			name: sanitize_display_line(name),
			param_type: param_type(&record),
			required: is_required,
			flag,
			description: record
				.get("description")
				.and_then(Value::as_str)
				.map(sanitize_display_text)
				.filter(|s| !s.is_empty()),
		});
	}
	params
}

fn normalize_path_for_comparison(path: &str) -> String {
	path.replace('\\', "/")
}

fn is_filesystem_source_path(value: &str) -> bool {
	Path::new(value).is_absolute() || value.starts_with("\\\\")
}

fn same_tool_path(left: &str, right: &str) -> bool {
	normalize_path_for_comparison(left) == normalize_path_for_comparison(right)
}

#[must_use]
pub fn live_tools_for_extension(
	ext: &Extension,
	source: Option<&dyn ToolRuntimeSource>,
) -> Vec<LiveToolRecord> {
	let Some(source) = source else {
		return Vec::new();
	};
	if is_shadowed_extension(ext) {
		return Vec::new();
	}
	let listed = source.list_live_tools();
	let from_list = listed
		.as_ref()
		.and_then(|l| l.iter().find(|t| t.name == ext.name).cloned());
	let exact = from_list.clone().or_else(|| {
		if listed.as_ref().is_none_or(|l| l.is_empty()) {
			source.get_live_tool(&ext.name)
		} else {
			None
		}
	});
	let pool = listed.unwrap_or_default();
	let mut candidates = Vec::new();
	let mut seen = HashSet::new();
	if let Some(e) = exact.clone() {
		if seen.insert(e.name.clone()) {
			candidates.push(e);
		}
	}
	for tool in pool {
		if seen.insert(tool.name.clone()) {
			candidates.push(tool);
		}
	}
	let from_same_file: Vec<LiveToolRecord> = candidates
		.iter()
		.filter(|tool| {
			tool.source_path
				.as_deref()
				.is_some_and(|p| is_filesystem_source_path(p) && same_tool_path(p, &ext.path))
		})
		.cloned()
		.collect();
	if !from_same_file.is_empty() {
		return from_same_file;
	}
	if candidates.iter().any(|tool| {
		tool.source_path
			.as_deref()
			.is_some_and(is_filesystem_source_path)
	}) {
		return Vec::new();
	}
	if let Some(exact) = exact {
		if exact.name == ext.name
			&& !matches!(exact.source.as_deref(), Some("builtin" | "mcp" | "sdk"))
		{
			return vec![exact];
		}
	}
	Vec::new()
}

#[must_use]
pub fn live_tool_detail(live: Option<&LiveToolRecord>) -> Option<String> {
	let live = live?;
	if live.hidden == Some(true) {
		return Some("hidden".to_string());
	}
	None
}

/// Project-local items only.
#[must_use]
pub fn project_list_hint(ext: &Extension) -> Option<String> {
	if ext.source.level != "project" {
		return None;
	}
	let parts: Vec<&str> = ext
		.path
		.split(['/', '\\'])
		.filter(|p| !p.is_empty() && *p != ".")
		.collect();
	let omp_index = parts.iter().rposition(|p| *p == ".omp")?;
	if omp_index == 0 {
		return None;
	}
	let parent = parts.get(omp_index - 1)?;
	if *parent == "." {
		None
	} else {
		Some((*parent).to_string())
	}
}

#[must_use]
pub fn join_list_hints(parts: &[Option<String>]) -> Option<String> {
	let hints: Vec<String> = parts.iter().filter_map(|p| p.clone()).filter(|p| !p.is_empty()).collect();
	if hints.is_empty() { None } else { Some(hints.join(" · ")) }
}

#[must_use]
pub fn enablement_label(state: ExtensionState, reason: Option<DisabledReason>, shadowed_by: Option<&str>) -> String {
	match state {
		ExtensionState::Active => "Active".to_string(),
		ExtensionState::Disabled => {
			let reason_text = match reason {
				Some(DisabledReason::ProviderDisabled) => "provider disabled",
				Some(DisabledReason::UserOptIn) => "~/ config not enabled",
				Some(DisabledReason::ItemDisabled) => "manually disabled",
				_ => "unknown",
			};
			format!("Disabled ({reason_text})")
		}
		ExtensionState::Shadowed => match shadowed_by {
			Some(by) => format!("Shadowed by {}", sanitize_display_text(by)),
			None => "Shadowed".to_string(),
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::overlays::extensions::types::{ExtensionKind, ExtensionSource};
	use serde_json::json;

	#[test]
	fn placeholder_tool_description_detects_default() {
		assert!(is_placeholder_tool_description("bash", None));
		assert!(is_placeholder_tool_description("bash", Some("")));
		assert!(is_placeholder_tool_description("bash", Some("bash custom tool")));
		assert!(!is_placeholder_tool_description("bash", Some("Run shell")));
	}

	#[test]
	fn parse_tool_file_header_extracts_first_jsdoc() {
		let src = "/** Hello\n * world\n *\n * symlink: ignored\n */\ncode";
		assert_eq!(parse_tool_file_header(src).as_deref(), Some("Hello\nworld"));
	}

	#[test]
	fn command_preview_detects_arguments_and_frontmatter() {
		let content = "---\ndescription: Hi\nargument-hint: <x>\n---\nRun $ARGUMENTS";
		let p = command_preview(Some(content));
		assert_eq!(p.description.as_deref(), Some("Hi"));
		assert_eq!(p.argument_hint.as_deref(), Some("<x>"));
		assert!(p.uses_arguments);
		assert!(p.body.contains("$ARGUMENTS"));
	}

	#[test]
	fn tool_params_from_schema_extracts_required_and_types() {
		let schema = json!({
			"properties": {
				"name": {"type":"string","description":"n"},
				"count": {"enum":[1,2]}
			},
			"required":["name"]
		});
		let params = tool_params_from_schema(Some(&schema));
		assert_eq!(params.len(), 2);
		let name = params.iter().find(|p| p.name == "name").unwrap();
		assert!(name.required);
		assert_eq!(name.param_type, "string");
		let count = params.iter().find(|p| p.name == "count").unwrap();
		assert!(!count.required);
		assert_eq!(count.param_type, "enum");
	}

	#[test]
	fn enablement_label_formats_states() {
		assert_eq!(enablement_label(ExtensionState::Active, None, None), "Active");
		assert!(enablement_label(ExtensionState::Disabled, Some(DisabledReason::ProviderDisabled), None).contains("provider disabled"));
		assert!(enablement_label(ExtensionState::Shadowed, None, Some("x")).contains("x"));
	}

	#[test]
	fn project_list_hint_finds_parent_of_omp_dir() {
		let ext = Extension {
			id: "t:x".into(),
			kind: ExtensionKind::Tool,
			name: "x".into(),
			display_name: "x".into(),
			description: None,
			trigger: None,
			path: "/repo/pkg/.omp/tools/x.ts".into(),
			source: ExtensionSource { provider: "p".into(), provider_name: "p".into(), level: "project".into() },
			state: ExtensionState::Active,
			disabled_reason: None,
			shadowed_by: None,
			raw: json!({}),
		};
		assert_eq!(project_list_hint(&ext).as_deref(), Some("pkg"));
	}
}
