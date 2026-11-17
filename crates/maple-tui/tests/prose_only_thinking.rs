//! Port of `l/test/prose-only-thinking.test.ts`.

use maple_tui::chat::thinking_display::{format_thinking_for_display, has_displayable_thinking};

#[test]
fn does_not_format_when_prose_only_is_false() {
	let text = "Let me rewrite readString:\n```go\nfunc foo() {}\n```";
	assert_eq!(format_thinking_for_display(text, false), text);
}

#[test]
fn replaces_fully_enclosed_code_blocks_with_an_ellipsis() {
	let text = "Let me rewrite readString:\n```go\nfunc foo() {}\n```\nAnd then test it.";
	assert_eq!(
		format_thinking_for_display(text, true),
		"Let me rewrite readString:...\nAnd then test it."
	);
}

#[test]
fn replaces_unclosed_code_blocks_with_an_ellipsis() {
	let text = "Let me rewrite readString and the dq handling.\n```go\n  func (l *Lexer) readString(pos Pos) (string, error) {\n     l.advance() // opening '\n     var b strings.Builder\n     for {";
	assert_eq!(
		format_thinking_for_display(text, true),
		"Let me rewrite readString and the dq handling..."
	);
}

#[test]
fn preserves_trailing_one_and_two_character_fence_prefixes_as_prose() {
	assert_eq!(format_thinking_for_display("Writing bla.\n`", true), "Writing bla.\n`");
	assert_eq!(
		format_thinking_for_display("Writing bla.\n``", true),
		"Writing bla.\n``"
	);
	assert_eq!(format_thinking_for_display("Writing bla.\n```", true), "Writing bla...");
}

#[test]
fn preserves_inline_code_in_prose() {
	assert_eq!(
		format_thinking_for_display("Use `readString` here", true),
		"Use `readString` here"
	);
}

fn handles_tilde_code_blocks() {
	let text = "Use tilde:\n~~~\ncode inside\n~~~\nprose after";
	assert_eq!(
		format_thinking_for_display(text, true),
		"Use tilde:...\nprose after"
	);
}

#[test]
fn returns_exactly_ascii_ellipsis_for_pure_code_blocks() {
	let text = "```js\nconst x = 1;\n```";
	let formatted = format_thinking_for_display(text, true);
	assert_eq!(formatted, "...");
	assert!(has_displayable_thinking(Some(text), Some(formatted.as_str())));
}