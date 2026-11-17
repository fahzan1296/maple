//! Port of `l/test/theme-extension-link-color.test.ts` (portable subset).
//!
//! Divergence: TS exercised `getMarkdownTheme()`/`setExtensionLinks()` globals.
//! Rust renders via `theme::tui_adapters::get_markdown_theme` and the
//! process-wide `set_extension_links` registry; theme construction uses
//! `create_theme` over the builtin dark JSON.

use maple_tui::component::Component;
use maple_tui::components::markdown::{Markdown, MarkdownTheme};
use maple_tui::render::extension_links::set_extension_links;
use maple_tui::style::Style;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::color::ColorMode;
use maple_tui::theme::tui_adapters::get_markdown_theme;
use maple_tui::theme::create_theme;

fn dark_with(
	colors: serde_json::Value,
	terminal: Option<serde_json::Value>,
) -> maple_tui::theme::Theme {
	let mut json = builtin_theme_json("dark").expect("dark theme json");
	let root = json.as_object_mut().expect("theme object");
	let base_colors = root
		.get("colors")
		.and_then(|v| v.as_object())
		.cloned()
		.expect("colors");
	let mut merged = base_colors;
	if let Some(obj) = colors.as_object() {
		for (k, v) in obj {
			merged.insert(k.clone(), v.clone());
		}
	}
	root.insert("colors".to_string(), serde_json::Value::Object(merged));
	if let Some(term) = terminal {
		root.insert("terminal".to_string(), term);
	}
	create_theme(json, Some(ColorMode::Truecolor)).expect("theme")
}

fn hex(theme: &maple_tui::theme::Theme, name: &str) -> String {
	theme.get_color_hex(name).unwrap_or_default()
}

#[test]
fn derives_md_extension_link_as_link_text_blend_when_omitted() {
	let theme = dark_with(
		serde_json::json!({"mdLink": "#0000ff", "text": "#ffffff"}),
		None,
	);
	assert_eq!(hex(&theme, "mdExtensionLink"), "#8c8cff");
}

#[test]
fn resolves_palette_index_md_link_before_blending() {
	let theme = dark_with(
		serde_json::json!({"mdLink": 21, "text": "#ffffff"}),
		None,
	);
	assert_eq!(hex(&theme, "mdExtensionLink"), "#8c8cff");
}

#[test]
fn blends_terminal_default_text_with_terminal_foreground() {
	let theme = dark_with(
		serde_json::json!({"mdLink": "#0000ff", "text": ""}),
		Some(serde_json::json!({"foreground": "#ffffff"})),
	);
	assert_eq!(hex(&theme, "mdExtensionLink"), "#8c8cff");
}

#[test]
fn reads_basic_palette_index_through_terminal_ansi() {
	let mut ansi = vec!["#000000"; 16];
	ansi[4] = "#0000ff".to_string();
	let theme = dark_with(
		serde_json::json!({"mdLink": 4, "text": "#ffffff"}),
		Some(serde_json::json!({"ansi": ansi})),
	);
	assert_eq!(hex(&theme, "mdExtensionLink"), "#8c8cff");
}

#[test]
fn keeps_explicit_md_extension_link_value() {
	let theme = dark_with(
		serde_json::json!({"mdLink": "#0000ff", "text": "#ffffff", "mdExtensionLink": "#123456"}),
		None,
	);
	assert_eq!(hex(&theme, "mdExtensionLink"), "#123456");
}

#[test]
fn paints_extension_links_with_extension_color_and_web_links_with_md_link() {
	let theme = dark_with(
		serde_json::json!({"mdLink": "#0000ff", "text": "#ffffff"}),
		None,
	);
	let markdown_theme = get_markdown_theme(&theme, false);
	set_extension_links(["demo"]);
	let mut markdown = Markdown::new();
	markdown.set_text("Open [the details](demo:open/7) or [the docs](https://example.com/docs).");
	markdown.set_theme(markdown_theme.clone());
	let out = markdown.render(80).join("\n");
	set_extension_links(Vec::<String>::new());
	let ext = Style::merge(markdown_theme.extension_link, markdown_theme.underline)
		.paint("the details");
	let web = Style::merge(markdown_theme.link, markdown_theme.underline).paint("the docs");
	let plain_link = Style::merge(markdown_theme.link, markdown_theme.underline)
		.paint("the details");
	assert!(out.contains(&ext), "expected extension-colored label: {out:?}");
	assert!(
		!out.contains(&plain_link),
		"extension label must not use plain mdLink: {out:?}"
	);
	assert!(out.contains(&web), "expected mdLink+underline web label: {out:?}");
	assert!(
		!out.contains("(demo:open/7)"),
		"extension link drops the url suffix: {out:?}"
	);
	assert!(
		out.contains("(https://example.com/docs)"),
		"web link keeps the url suffix: {out:?}"
	);
}
