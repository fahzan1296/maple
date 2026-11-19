//! Reference: `l/src/prompt/tools-markdown.ts`.
//!
//! Markdown table of the tools visible to the agent, with `xd://`-mounted
//! tools listed after the active set.

/// Inputs for [`build_tools_markdown`].
pub struct ToolsMarkdownBindings {
	pub tools: Vec<ToolEntry>,
	/// Tools mounted under `xd://` URLs, listed after the active set.
	pub xdev_tools: Option<Vec<XdevToolEntry>>,
}

/// An active tool.
pub struct ToolEntry {
	pub description: String,
	pub name: String,
}

/// An `xd://`-mounted tool.
pub struct XdevToolEntry {
	pub name: String,
	pub summary: String,
}

fn escape_table_cell(value: &str) -> String {
	let escaped = value.replace('|', "\\|");
	let mut out = String::with_capacity(escaped.len());
	let mut last_was_newline = false;
	for ch in escaped.chars() {
		if ch == '\n' {
			if !last_was_newline {
				out.push(' ');
			}
			last_was_newline = true;
			continue;
		}
		if ch == '\r' {
			continue;
		}
		last_was_newline = false;
		out.push(ch);
	}
	out.trim().to_string()
}

/// Render the tool table. Empty input yields the "no tools" sentence.
pub fn build_tools_markdown(bindings: &ToolsMarkdownBindings) -> String {
	let xdev = bindings.xdev_tools.as_deref().unwrap_or(&[]);
	if bindings.tools.is_empty() && xdev.is_empty() {
		return "No tools are currently visible to the agent.".to_string();
	}

	let mut rows: Vec<String> = Vec::with_capacity(bindings.tools.len() + xdev.len());
	for tool in &bindings.tools {
		let description = escape_table_cell(&tool.description);
		let description = if description.is_empty() {
			"No description provided.".to_string()
		} else {
			description
		};
		rows.push(format!("| `{}` | {} |", tool.name, description));
	}
	for mounted in xdev {
		let summary = escape_table_cell(&mounted.summary);
		let summary = if summary.is_empty() {
			"No description provided.".to_string()
		} else {
			summary
		};
		rows.push(format!("| `xd://{}` | {} |", mounted.name, summary));
	}

	let mut lines = vec![
		"| Tool | Description |".to_string(),
		"|------|-------------|".to_string(),
	];
	lines.extend(rows);
	lines.join("\n")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(name: &str, description: &str) -> ToolEntry {
		ToolEntry {
			name: name.to_string(),
			description: description.to_string(),
		}
	}

	#[test]
	fn empty_bindings_report_no_tools() {
		let bindings = ToolsMarkdownBindings {
			tools: Vec::new(),
			xdev_tools: None,
		};
		assert_eq!(
			build_tools_markdown(&bindings),
			"No tools are currently visible to the agent."
		);
	}

	#[test]
	fn tables_escape_and_order_rows() {
		let cases: [(&str, &str, &str); 3] = [
			(
				"shell",
				"run | a\ncommand",
				"| `shell` | run \\| a command |",
			),
			("empty", "", "| `empty` | No description provided. |"),
			("padded", "  padded  ", "| `padded` | padded |"),
		];
		for (name, description, expected) in cases {
			let bindings = ToolsMarkdownBindings {
				tools: vec![entry(name, description)],
				xdev_tools: None,
			};
			let markdown = build_tools_markdown(&bindings);
			assert_eq!(markdown.lines().count(), 3);
			assert_eq!(markdown.lines().nth(2), Some(expected));
		}

		let bindings = ToolsMarkdownBindings {
			tools: vec![entry("shell", "run")],
			xdev_tools: Some(vec![XdevToolEntry {
				name: "browser".to_string(),
				summary: "drive tabs".to_string(),
			}]),
		};
		let lines: Vec<&str> = build_tools_markdown(&bindings).lines().collect::<Vec<_>>();
		assert_eq!(
			lines,
			vec![
				"| Tool | Description |",
				"|------|-------------|",
				"| `shell` | run |",
				"| `xd://browser` | drive tabs |",
			]
		);
	}
}
