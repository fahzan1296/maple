//! Inspector panel for extension details.
//!
//! Reference: `l/src/overlays/extensions/inspector-panel.ts`.
//!
//! Deviations:
//! - Kind-specific data is derived from `Extension.raw`/live helpers available in
//!   this crate (`inspector_model`, `mcp_runtime`, `live_tool_session`) rather
//!   than TS-only view-model builders.
//! - Render output is plain/styled text lines for the TUI component model.

use std::path::Path;

use crate::component::Component;
use crate::components::key_value_list::{KeyValueList, KeyValueRow};
use crate::components::section::Section;
use crate::keys::Key;
use crate::render::render_utils::{expand_key_hint, replace_tabs, PREVIEW_LIMITS};
use crate::text_shape::{truncate_to_width, wrap_text_with_ansi, Ellipsis};

use super::display_text::{
    sanitize_display_field, sanitize_display_line, sanitize_display_line_field, sanitize_display_text,
};
use super::inspector_model::{
    command_preview, enablement_label, is_placeholder_tool_description, live_tool_detail,
    live_tools_for_extension, parse_tool_file_header, tool_params_from_schema, ToolParamView,
    ToolRuntimeSource,
};
use super::mcp_runtime::{
    format_mcp_health_label, is_discovered_mcp_server, snapshot_mcp_runtime, visible_mcp_tools,
    McpRuntimeOpts, McpRuntimeSnapshot, McpServerDisplay,
};
use super::types::{is_shadowed_extension, Extension, ExtensionKind, ExtensionState};

const PREVIEW_LINE_BUDGET: usize = PREVIEW_LIMITS.EXPANDED_LINES;
const MCP_TOOL_BUDGET: usize = PREVIEW_LIMITS.COLLAPSED_ITEMS;
const MCP_INLINE_ARG_LIMIT: usize = 3;
const MCP_INLINE_DESC_LINES: usize = 3;

/// Detail pane for the selected extension.
pub struct InspectorPanel {
    extension: Option<Extension>,
    extension_key: Option<String>,
    expanded: bool,
    scroll_offset: usize,
    mcp_source: Option<Box<dyn super::mcp_runtime::McpRuntimeSource>>,
    tool_source: Option<Box<dyn ToolRuntimeSource>>,
}

impl InspectorPanel {
    pub fn new(extension: Option<Extension>) -> Self {
        let extension_key = extension.as_ref().map(inspector_extension_key);
        Self {
            extension,
            extension_key,
            expanded: false,
            scroll_offset: 0,
            mcp_source: None,
            tool_source: None,
        }
    }

    pub fn set_extension(&mut self, extension: Option<Extension>) {
        let next_key = extension.as_ref().map(inspector_extension_key);
        if next_key != self.extension_key {
            self.expanded = false;
            self.scroll_offset = 0;
            self.extension_key = next_key;
        }
        self.extension = extension;
    }

    pub fn extension(&self) -> Option<&Extension> {
        self.extension.as_ref()
    }

    pub fn set_mcp_source(&mut self, source: Option<Box<dyn super::mcp_runtime::McpRuntimeSource>>) {
        self.mcp_source = source;
    }

    pub fn set_tool_source(&mut self, source: Option<Box<dyn ToolRuntimeSource>>) {
        self.tool_source = source;
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn toggle_expanded(&mut self) -> bool {
        self.expanded = !self.expanded;
        true
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n.max(1));
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n.max(1));
    }

    pub fn render(&self, width: u16, height: usize) -> Vec<String> {
        let width = (width as usize).max(16);
        let mut lines = self.render_lines(width);
        if height == usize::MAX {
            return lines;
        }
        if self.scroll_offset >= lines.len() {
            return vec!["(end)".to_string()];
        }
        lines.drain(0..self.scroll_offset.min(lines.len()));
        if lines.len() > height {
            lines.truncate(height);
        }
        lines
    }

    fn render_lines(&self, width: usize) -> Vec<String> {
        let Some(ext) = &self.extension else {
            return vec![
                "Select an extension".to_string(),
                "to view details".to_string(),
            ];
        };

        let mut lines = Vec::new();
        self.push_identity(&mut lines, ext, width);
        self.push_status_origin(&mut lines, ext, width);
        self.push_description(&mut lines, ext.description.as_deref(), width);

        match ext.kind {
            ExtensionKind::Mcp => self.push_mcp(&mut lines, ext, width),
            ExtensionKind::Tool => self.push_tool(&mut lines, ext, width),
            ExtensionKind::SlashCommand => self.push_command(&mut lines, ext, width),
            ExtensionKind::Prompt => self.push_prompt(&mut lines, ext, width),
            ExtensionKind::Hook => self.push_hook(&mut lines, ext, width),
            ExtensionKind::Rule => self.push_rule(&mut lines, ext, width),
            ExtensionKind::Skill => self.push_skill(&mut lines, ext, width),
            ExtensionKind::ContextFile => self.push_context(&mut lines, ext, width),
            ExtensionKind::Instruction => self.push_instruction(&mut lines, ext, width),
            ExtensionKind::ExtensionModule => {
                self.push_fallback(&mut lines, ext, width)
            }
        }

        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        lines
    }

    fn push_identity(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let name = sanitize_display_line(&ext.display_name);
        lines.push(truncate_to_width(&name, width, Ellipsis::Unicode));
        let trigger = ext.trigger.as_deref().and_then(|t| sanitize_display_line_field(Some(t)));
        if let Some(trigger) = trigger {
            lines.push(truncate_to_width(&format!("Trigger: {trigger}"), width, Ellipsis::Unicode));
        }
        lines.push(String::new());
    }

    fn push_status_origin(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let status = enablement_label(ext.state, ext.disabled_reason, ext.shadowed_by.as_deref());
        self.push_wrapped(lines, &status, width);
        let level = match ext.source.level.as_str() {
            "user" => "User",
            "project" => "Project",
            _ => "Native",
        };
        self.push_wrapped(
            lines,
            &format!("Origin: via {} ({level})", sanitize_display_text(&ext.source.provider_name)),
            width,
        );
        let home = std::env::var("HOME").unwrap_or_default();
        let short = shorten_display_path(&ext.path, &home);
        self.push_wrapped(lines, &format!("Path: {}", sanitize_display_text(&short)), width);
        lines.push(String::new());
    }

    fn push_description(&self, lines: &mut Vec<String>, description: Option<&str>, width: usize) {
        let Some(text) = sanitize_display_field(description) else {
            return;
        };
        self.push_short_text(lines, &text, width);
        lines.push(String::new());
    }

    fn push_short_text(&self, lines: &mut Vec<String>, value: &str, width: usize) {
        let cleaned = sanitize_display_text(value);
        let mut wrapped = Vec::new();
        for raw in cleaned.lines() {
            let folded = wrap_text_with_ansi(&replace_tabs(raw), width.saturating_sub(1).max(8));
            if folded.is_empty() {
                wrapped.push(String::new());
            } else {
                wrapped.extend(folded);
            }
        }
        if self.expanded || wrapped.len() <= MCP_INLINE_DESC_LINES {
            lines.extend(wrapped);
        } else {
            let shown = wrapped[..MCP_INLINE_DESC_LINES].to_vec();
            lines.extend(shown);
            lines.push(format!(
                "  … {} more ({} to expand)",
                wrapped.len() - MCP_INLINE_DESC_LINES,
                expand_key_hint()
            ));
        }
    }

    fn push_wrapped(&self, lines: &mut Vec<String>, text: &str, width: usize) {
        for line in wrap_text_with_ansi(&sanitize_display_text(text), width.max(8)) {
            lines.push(line);
        }
    }

    fn section(&self, title: &str, body: Vec<String>, width: usize) -> Vec<String> {
        let mut sec = Section::new(title.to_string());
        sec.set_children(body);
        sec.render(width as u16)
    }

    fn preview_body(&self, raw: &str, width: usize) -> Vec<String> {
        if raw.is_empty() {
            return vec!["  (empty)".to_string()];
        }
        let mut all = Vec::new();
        for raw_line in sanitize_display_text(raw).lines() {
            let wrapped =
                wrap_text_with_ansi(&replace_tabs(raw_line), width.saturating_sub(2).max(8));
            if wrapped.is_empty() {
                all.push(String::new());
            } else {
                all.extend(wrapped);
            }
        }
        let budget = if self.expanded {
            usize::MAX
        } else {
            PREVIEW_LINE_BUDGET.max(1)
        };
        if all.len() <= budget {
            return all;
        }
        let shown = budget.saturating_sub(1).max(1);
        let mut out = all[..shown].to_vec();
        out.push(format!(
            "  … {} more lines ({} to expand)",
            all.len() - shown,
            expand_key_hint()
        ));
        out
    }

    fn push_preview_section(&self, lines: &mut Vec<String>, heading: &str, raw: &str, width: usize) {
        lines.extend(self.section(heading, self.preview_body(raw, width), width));
    }

    fn push_mcp(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let raw = &ext.raw;
        if raw.is_null() {
            return;
        }
        if !is_discovered_mcp_server(raw) {
            // best-effort: still try parse
        }
        let Some(server) = McpServerDisplay::from_value(raw) else {
            self.push_fallback(lines, ext, width);
            return;
        };
        let snap: McpRuntimeSnapshot = snapshot_mcp_runtime(
            &server,
            self.mcp_source.as_deref(),
            Some(McpRuntimeOpts {
                enabled: Some(ext.state != ExtensionState::Disabled),
                shadowed: is_shadowed_extension(ext),
            }),
        );

        if let Some(desc) = sanitize_display_field(snap.description.as_deref()) {
            self.push_short_text(lines, &desc, width);
            lines.push(String::new());
        }
        let mut rows = vec![KeyValueRow::new(
            "Health".to_string(),
            format_mcp_health_label(snap.health).to_string(),
        )];
        if let Some(title) = sanitize_display_field(snap.title.as_deref()) {
            rows.push(KeyValueRow::new("Title".to_string(), title));
        }
        if let Some(name) = snap.implementation_name.as_ref().or(Some(&server.name)) {
            let mut v = sanitize_display_line(name);
            if let Some(ver) = &snap.implementation_version {
                if !ver.is_empty() {
                    v.push(' ');
                    v.push_str(&sanitize_display_text(ver));
                }
            }
            rows.push(KeyValueRow::new("Server".to_string(), v));
        }
        if let Some(url) = &snap.website_url {
            rows.push(KeyValueRow::new("Website".to_string(), sanitize_display_text(url)));
        }
        if let Some(url) = &snap.url {
            rows.push(KeyValueRow::new("URL".to_string(), sanitize_display_text(url)));
        }
        if let Some(cmd) = &snap.command {
            let mut c = sanitize_display_text(cmd);
            if let Some(args) = &snap.args {
                if !args.is_empty() {
                    c.push(' ');
                    c.push_str(&args.iter().map(|a| sanitize_display_text(a)).collect::<Vec<_>>().join(" "));
                }
            }
            rows.push(KeyValueRow::new("Command".to_string(), c));
        }
        lines.extend(KeyValueList::new(rows).render(width as u16));
        lines.push(String::new());

        if let Some(guidance) = sanitize_display_field(snap.instructions.as_deref()) {
            self.push_short_text(lines, &guidance, width);
            lines.push(String::new());
        }

        if !snap.tools.is_empty() {
            let visible = visible_mcp_tools(&snap.tools, if self.expanded { snap.tools.len() } else { MCP_TOOL_BUDGET });
            let mut body = Vec::new();
            for t in &visible.shown {
                body.push(format!("  {}", sanitize_display_line(&t.name)));
                if let Some(desc) = &t.description {
                    body.push(format!("    {}", sanitize_display_line(desc)));
                }
                let params = tool_params_from_schema(t.parameters.as_ref());
                if params.is_empty() {
                    continue;
                }
                if self.expanded || params.len() <= MCP_INLINE_ARG_LIMIT {
                    for p in &params {
                        body.push(format!(
                            "    {}  {}  {}",
                            sanitize_display_line(&p.name),
                            sanitize_display_line(&p.param_type),
                            sanitize_display_line(&p.flag)
                        ));
                        if let Some(d) = &p.description {
                            body.push(format!("      {}", sanitize_display_line(d)));
                        }
                    }
                } else {
                    body.push(format!(
                        "    {} args ({} to expand)",
                        params.len(),
                        expand_key_hint()
                    ));
                }
            }
            if visible.hidden > 0 {
                body.push(format!(
                    "  … {} more ({} to expand)",
                    visible.hidden,
                    expand_key_hint()
                ));
            }
            lines.extend(self.section("Tools", body, width));
        }
        if !snap.resources.is_empty() {
            let limit = if self.expanded { snap.resources.len() } else { MCP_TOOL_BUDGET };
            let mut body = Vec::new();
            for r in snap.resources.iter().take(limit) {
                body.push(format!("  {}", sanitize_display_line(&r.name)));
            }
            if snap.resources.len() > limit {
                body.push(format!(
                    "  … {} more ({} to expand)",
                    snap.resources.len() - limit,
                    expand_key_hint()
                ));
            }
            lines.extend(self.section("Resources", body, width));
        }
        if !snap.prompts.is_empty() {
            let limit = if self.expanded { snap.prompts.len() } else { MCP_TOOL_BUDGET };
            let mut body = Vec::new();
            for p in snap.prompts.iter().take(limit) {
                body.push(format!("  {}", sanitize_display_line(&p.name)));
            }
            if snap.prompts.len() > limit {
                body.push(format!(
                    "  … {} more ({} to expand)",
                    snap.prompts.len() - limit,
                    expand_key_hint()
                ));
            }
            lines.extend(self.section("Prompts", body, width));
        }
    }

    fn push_tool(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let lives = live_tools_for_extension(ext, self.tool_source.as_deref());
        let live = lives.first();
        if let Some(label) = live
            .and_then(|l| l.label.as_deref())
            .and_then(|l| sanitize_display_line_field(Some(l)))
        {
            self.push_wrapped(lines, &label, width);
            lines.push(String::new());
        }
        let mut desc = live
            .and_then(|l| l.description.clone())
            .filter(|d| !is_placeholder_tool_description(&ext.name, Some(d.as_str())));
        if desc.is_none() {
            desc = ext.description.clone().filter(|d| !is_placeholder_tool_description(&ext.name, Some(d.as_str())));
        }
        if let Some(desc) = desc {
            self.push_description(lines, Some(desc.as_str()), width);
        }
        if let Some(detail) = live_tool_detail(live) {
            self.push_wrapped(lines, &format!("Detail: {detail}"), width);
            lines.push(String::new());
        }
        if lives.len() > 1 {
            let mut body = Vec::new();
            for tool in &lives {
                body.push(format!("  {}", sanitize_display_line(&tool.name)));
                if let Some(label) = tool
                    .label
                    .as_deref()
                    .and_then(|l| sanitize_display_line_field(Some(l)))
                {
                    body.push(format!("    {label}"));
                }
                if let Some(desc) = tool
                    .description
                    .as_deref()
                    .filter(|d| !is_placeholder_tool_description(&ext.name, Some(d)))
                {
                    body.push(format!("    {}", sanitize_display_line(desc)));
                }
                self.push_param_rows(&mut body, &tool_params_from_schema(tool.parameters.as_ref()), "    ");
            }
            lines.extend(self.section("Tools", body, width));
        } else {
            let params_schema = live
                .and_then(|l| l.parameters.as_ref())
                .or_else(|| ext.raw.get("parameters"));
            let mut body = Vec::new();
            self.push_param_rows(&mut body, &tool_params_from_schema(params_schema), "  ");
            lines.extend(self.section("Arguments", body, width));
        }


        // best-effort content/header preview
        if let Some(content) = ext.raw.get("content").and_then(|v| v.as_str()) {
            self.push_preview_section(lines, "Source", content, width);
        } else if let Some(header) = ext.raw.get("header").and_then(|v| v.as_str()) {
            if let Some(parsed) = parse_tool_file_header(header) {
                self.push_preview_section(lines, "Header", &parsed, width);
            }
        }
    }

    fn push_param_rows(&self, body: &mut Vec<String>, params: &[ToolParamView], indent: &str) {
        if params.is_empty() {
            return;
        }
        if !self.expanded && params.len() > MCP_INLINE_ARG_LIMIT {
            body.push(format!(
                "{indent}{} args ({} to expand)",
                params.len(),
                expand_key_hint()
            ));
            return;
        }
        for p in params {
            body.push(format!(
                "{indent}{}  {}  {}",
                sanitize_display_line(&p.name),
                sanitize_display_line(&p.param_type),
                sanitize_display_line(&p.flag)
            ));
            if let Some(d) = &p.description {
                body.push(format!("{indent}  {}", sanitize_display_line(d)));
            }
        }
    }

    fn push_command(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let content = ext
            .raw
            .get("content")
            .and_then(|v| v.as_str())
            .or(ext.description.as_deref())
            .unwrap_or("");
        let preview = command_preview(Some(content));
        if let Some(desc) = preview.description.as_deref() {
            self.push_description(lines, Some(desc), width);
        }
        let mut rows = Vec::new();
        if let Some(hint) = preview.argument_hint.as_deref() {
            rows.push(KeyValueRow::new("Arguments".to_string(), hint.to_string()));
        }
        rows.push(KeyValueRow::new(
            "Uses $ARGUMENTS".to_string(),
            if preview.uses_arguments { "yes" } else { "no" }.to_string(),
        ));
        lines.extend(KeyValueList::new(rows).render(width as u16));
        lines.push(String::new());
        let body = if preview.body.is_empty() {
            content.to_string()
        } else {
            preview.body
        };
        self.push_preview_section(lines, "Template", &body, width);
    }

    fn push_prompt(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let content = ext
            .raw
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        self.push_preview_section(lines, "Prompt", content, width);
    }

    fn push_hook(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        if let Some(events) = ext.raw.get("events").and_then(|v| v.as_array()) {
            let body = events
                .iter()
                .filter_map(|e| e.as_str().map(|s| format!("  {}", sanitize_display_line(s))))
                .collect::<Vec<_>>();
            if !body.is_empty() {
                lines.extend(self.section("Hooks", body, width));
            }
        }
        if let Some(content) = ext.raw.get("content").and_then(|v| v.as_str()) {
            self.push_preview_section(lines, "Source", content, width);
        }
    }

    fn push_rule(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let mut rows = Vec::new();
        if let Some(map) = ext.raw.as_object() {
            for key in ["alwaysApply", "globs", "condition", "ast", "scope", "agents", "interrupt"] {
                if let Some(val) = map.get(key) {
                    let text = match val {
                        serde_json::Value::String(s) => sanitize_display_text(s),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Array(arr) => arr
                            .iter()
                            .filter_map(|v| v.as_str().map(sanitize_display_text))
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => sanitize_display_text(&val.to_string()),
                    };
                    if !text.is_empty() {
                        rows.push(KeyValueRow::new(key.to_string(), text));
                    }
                }
            }
        }
        if !rows.is_empty() {
            lines.extend(KeyValueList::new(rows).render(width as u16));
            lines.push(String::new());
        }
        let content = ext.raw.get("content").and_then(|v| v.as_str()).unwrap_or("");
        self.push_preview_section(lines, "Rule", content, width);
    }

    fn push_skill(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let mut rows = Vec::new();
        if let Some(map) = ext.raw.as_object() {
            if map.get("alwaysApply").and_then(|v| v.as_bool()) == Some(true) {
                rows.push(KeyValueRow::new("Apply".to_string(), "always".to_string()));
            }
            if let Some(globs) = map.get("globs") {
                if let Some(arr) = globs.as_array() {
                    let joined = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(sanitize_display_text))
                        .collect::<Vec<_>>()
                        .join(", ");
                    if !joined.is_empty() {
                        rows.push(KeyValueRow::new("Globs".to_string(), joined));
                    }
                }
            }
            let hidden = map
                .get("frontmatter")
                .and_then(|v| v.get("hide"))
                .and_then(|v| v.as_bool())
                == Some(true)
                || map.get("hidden").and_then(|v| v.as_bool()) == Some(true);
            if hidden {
                rows.push(KeyValueRow::new("Visibility".to_string(), "hidden".to_string()));
            }
        }
        if !rows.is_empty() {
            lines.extend(KeyValueList::new(rows).render(width as u16));
            lines.push(String::new());
        }
        let content = ext
            .raw
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        self.push_preview_section(lines, "Instruction", content, width);
    }

    fn push_context(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let content = ext.raw.get("content").and_then(|v| v.as_str()).unwrap_or("");
        self.push_preview_section(lines, "Context", content, width);
    }

    fn push_instruction(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        let apply_to = ext.raw.get("applyTo").or(ext.raw.get("apply_to")).and_then(|v| v.as_str());
        if let Some(apply_to) = sanitize_display_field(apply_to) {
            lines.extend(self.section("Applies", vec![format!("  files: {apply_to}")], width));
        }
        let content = ext.raw.get("content").and_then(|v| v.as_str()).unwrap_or("");
        self.push_preview_section(lines, "Instruction", content, width);
    }

    fn push_fallback(&self, lines: &mut Vec<String>, ext: &Extension, width: usize) {
        if let Some(trigger) = &ext.trigger {
            self.push_wrapped(lines, &format!("Trigger: {}", sanitize_display_line(trigger)), width);
        }
        let raw = &ext.raw;
        if !raw.is_null() {
            let pretty = serde_json::to_string_pretty(raw).unwrap_or_else(|_| raw.to_string());
            self.push_preview_section(lines, "Raw", &pretty, width);
        } else if let Some(desc) = &ext.description {
            self.push_wrapped(lines, desc, width);
        } else {
            let _ = width;
        }
    }
}

impl Component for InspectorPanel {
    fn render(&mut self, width: u16) -> Vec<String> {
        InspectorPanel::render(self, width, usize::MAX)
    }

    fn handle_input(&mut self, _key: &Key) {}

    fn invalidate(&mut self) {}
}

fn inspector_extension_key(ext: &Extension) -> String {
    format!("{}:{}:{}", kind_key(ext.kind), ext.id, ext.path)
}

fn kind_key(kind: ExtensionKind) -> &'static str {
    kind.as_str()
}

fn shorten_display_path(path: &str, home: &str) -> String {
    if !home.is_empty() {
        if let Some(stripped) = path.strip_prefix(home) {
            let stripped = stripped.strip_prefix('/').unwrap_or(stripped);
            return format!("~/{stripped}");
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlays::extensions::types::{Extension, ExtensionKind, ExtensionSource, ExtensionState};
    use serde_json::json;

    fn ext(kind: ExtensionKind, id: &str) -> Extension {
        Extension {
            id: id.into(),
            name: id.into(),
            display_name: id.into(),
            kind,
            state: ExtensionState::Active,
            disabled_reason: None,
            shadowed_by: None,
            description: Some("desc".into()),
            trigger: None,
            path: "/tmp/x".into(),
            source: ExtensionSource {
                provider: "skill".into(),
                provider_name: "Skills".into(),
                level: "user".into(),
            },
            raw: json!({}),
        }
    }

    #[test]
    fn renders_placeholder_when_no_selection() {
        let panel = InspectorPanel::new(None);
        let out = panel.render(40, 10).join("\n");
        assert!(out.contains("Select an extension"));
    }

    #[test]
    fn renders_selected_extension_identity_origin_and_content() {
        let mut e = ext(ExtensionKind::Skill, "s1");
        e.raw = json!({ "content": "hello\nworld" });
        let panel = InspectorPanel::new(Some(e));
        let out = panel.render(60, 40).join("\n");
        assert!(out.contains("s1"));
        assert!(out.contains("Origin:"));
        assert!(out.contains("Instruction") || out.contains("hello"));
    }

    #[test]
    fn toggle_expand_and_scroll() {
        let mut e = ext(ExtensionKind::Prompt, "p1");
        e.raw = json!({ "content": "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn" });
        let mut panel = InspectorPanel::new(Some(e));
        assert!(!panel.is_expanded());
        panel.toggle_expanded();
        assert!(panel.is_expanded());
        panel.scroll_down(2);
        panel.scroll_up(1);
        let out = panel.render(50, 5);
        assert!(!out.is_empty());
    }

    #[test]
    fn renders_command_preview() {
        let mut e = ext(ExtensionKind::SlashCommand, "cmd");
        e.raw = json!({ "content": "---\ndescription: hi\nargument-hint: <x>\n---\nRun $ARGUMENTS" });
        let out = InspectorPanel::new(Some(e)).render(80, 40).join("\n");
        assert!(out.contains("Template") || out.contains("Arguments") || out.contains("$ARGUMENTS"));
    }
}
