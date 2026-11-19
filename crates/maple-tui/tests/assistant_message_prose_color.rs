//! Port of `l/test/assistant-message-prose-color.test.ts`.
//!
//! Deviations:
//! - The reference loads a custom theme from disk through `parseThemeJson`;
//!   the crate builds the same dark palette in memory via `builtin_theme_json`
//!   and inserts the `assistantMessageText` token, so schema validation covers
//!   the token exactly as before.
//! - The crate's component always renders colorful Markdown
//!   (`get_markdown_theme` with `colorful = true`), so the `mdHeading`-vs-token
//!   case asserts the heading keeps its own themed style rather than switching
//!   colorful off.
//! - Stable rows are asserted with `contains` on the first prose row: Rust's
//!   `Markdown` applies no horizontal padding, unlike the reference's leading
//!   space.

use maple_tui::chat::assistant_message::{AssistantMessageComponent, StablePart, display_parts};
use maple_tui::chat::messages::{AssistantContent, AssistantMessage, TextContent};
use maple_tui::component::Component;
use maple_tui::theme::ColorValue;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{CreateThemeOptions, Theme, create_theme};

const PROSE: &str = "The build finished and the report is attached.";
const TOKEN_HEX: &str = "#ff8a3d";

fn assistant(text: &str) -> AssistantMessage {
	AssistantMessage {
		content: vec![AssistantContent::Text(TextContent {
			text: text.to_string(),
			..Default::default()
		})],
		..Default::default()
	}
}

/// Dark palette plus an optional `assistantMessageText` override.
fn theme_with(token: Option<&str>) -> Theme {
	let mut json = builtin_theme_json("dark").expect("dark theme json");
	if let Some(hex) = token {
		json.colors
			.set("assistantMessageText", ColorValue::Hex(hex.to_string()));
	}
	create_theme(&json, CreateThemeOptions::default()).expect("theme")
}

fn render(message: &AssistantMessage, theme: &Theme) -> String {
	let mut component = AssistantMessageComponent::new(theme.clone());
	component.update_content(message.clone(), false);
	component.render(80).join("\n")
}

#[test]
fn paints_assistant_paragraph_prose_with_the_assistant_message_text_token() {
	let theme = theme_with(Some(TOKEN_HEX));
	let rendered = render(&assistant(PROSE), &theme);
	assert!(
		rendered.contains(&theme.fg("assistantMessageText", PROSE).unwrap()),
		"prose must carry the token escape"
	);
	assert!(maple_tui::strip_ansi(&rendered).contains(PROSE));
}

#[test]
fn leaves_prose_in_the_terminal_default_foreground_when_the_token_is_unset() {
	let theme = theme_with(None);
	assert!(!theme.has_color("assistantMessageText"));
	let rendered = render(&assistant(PROSE), &theme);
	let prose_line = rendered
		.lines()
		.find(|line| line.contains(PROSE))
		.expect("prose row");
	assert!(
		!prose_line.contains('\x1b'),
		"unset token must not paint prose: {prose_line:?}"
	);
}

#[test]
fn keeps_an_explicitly_installed_prose_transform_over_the_token() {
	let theme = theme_with(Some(TOKEN_HEX));
	let green = "\x1b[38;2;0;255;0m";
	let mut component = AssistantMessageComponent::new(theme.clone());
	component.set_text_color_transform(Some(Box::new(move |text: &str| {
		format!("{green}{text}\x1b[39m")
	})));
	component.update_content(assistant(PROSE), false);
	let rendered = component.render(80).join("\n");
	assert!(rendered.contains(green));
	assert!(!rendered.contains(&theme.fg("assistantMessageText", PROSE).unwrap()));
}

#[test]
fn keeps_md_heading_and_link_colors_over_the_assistant_prose_token() {
	let theme = theme_with(Some(TOKEN_HEX));
	let markdown = "# Heading One\n\nplain paragraph with [link](https://example.com)";
	let rendered = render(&assistant(markdown), &theme);
	let heading_line = rendered
		.lines()
		.find(|line| line.contains("Heading One"))
		.expect("heading row");
	let link_line = rendered
		.lines()
		.find(|line| line.contains("link"))
		.expect("link row");

	assert!(
		heading_line.contains('\x1b'),
		"heading keeps its own themed color"
	);
	assert!(!heading_line.contains(&theme.fg("assistantMessageText", "Heading One").unwrap()));
	assert!(link_line.contains('\x1b'), "link keeps its own themed color");
	assert!(!link_line.contains(&theme.fg("assistantMessageText", "link").unwrap()));
	assert!(
		rendered.contains(&theme.fg("assistantMessageText", "plain paragraph with ").unwrap()),
		"plain prose runs still carry the token"
	);
}

#[test]
fn publishes_token_colored_streaming_prose_as_stable_scrollback_rows() {
	let theme = theme_with(Some(TOKEN_HEX));
	let full = "The first paragraph of a streaming turn is long enough to wrap onto its own rows. "
		.repeat(2)
		+ "\n\nThe second paragraph is still being streamed.";
	let mut component = AssistantMessageComponent::new(theme.clone());
	component.update_content(assistant(&format!("{}\n\nSec", &full[..60])), true);
	component.render(80);
	component.update_content(assistant(&full), true);
	component.render(80);

	let stable_count = component.transcript_stable_rows().len();
	assert!(stable_count > 0, "streaming prose must publish stable rows");
	let token_escape = theme.get_fg_ansi("assistantMessageText").unwrap();
	let stable_rows = component.render_transcript_stable_rows(stable_count, 80);
	let first_prose_row = stable_rows
		.iter()
		.find(|row| row.contains("The first paragraph"))
		.expect("published prose row");
	assert!(
		first_prose_row.contains(&token_escape),
		"published prefix must keep the prose token"
	);
}

#[test]
fn lists_narration_thinking_as_answer_text_in_display_parts() {
	let mut message = assistant(PROSE);
	message.content.insert(
		0,
		AssistantContent::Thinking(maple_tui::chat::messages::ThinkingContent {
			thinking: "hidden reasoning".to_string(),
			narration: None,
			..Default::default()
		}),
	);
	message.content.push(AssistantContent::Thinking(
		maple_tui::chat::messages::ThinkingContent {
			thinking: PROSE.to_string(),
			narration: Some(true),
			..Default::default()
		},
	));
	assert_eq!(
		display_parts(&message, false),
		vec![StablePart::Text(PROSE.to_string())],
		"narration thinking is answer text; plain reasoning stays hidden"
	);
}
