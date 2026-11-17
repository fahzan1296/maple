//! Reference: `l/test/provider-error-expand.test.ts`.
//!
//! The reference renders the whole assistant component and filters rows. This
//! port calls the crate's public error seam, [`AssistantMessageComponent::error_rows`],
//! which is exactly the rows the reference's render emits for a turn-ending
//! error. The reference's ` Error: ` prefix has no leading space in the crate.

use maple_tui::chat::assistant_message::AssistantMessageComponent;
use maple_tui::chat::messages::{AssistantMessage, StopReason};
use maple_tui::text_shape::visible_width;
use maple_tui::theme::default_theme;
use maple_tui::{strip_ansi};

fn long_error(lines: usize) -> String {
	(0..lines)
		.map(|index| format!("provider error detail line {index}"))
		.collect::<Vec<_>>()
		.join("\n")
}

fn error_message(body: &str) -> AssistantMessage {
	AssistantMessage {
		content: Vec::new(),
		stop_reason: StopReason::Error,
		error_message: Some(body.to_string()),
		..AssistantMessage::default()
	}
}

fn component(body: &str) -> AssistantMessageComponent {
	let mut component = AssistantMessageComponent::new(default_theme());
	component.update_content(error_message(body), false);
	component
}

fn plain(rows: &[String]) -> String {
	rows.iter()
		.map(|row| strip_ansi(row))
		.collect::<Vec<_>>()
		.join("\n")
}

#[test]
fn wraps_a_long_single_line_error_across_rows_instead_of_cutting_it() {
	let body = "400 {\"error\":{\"message\":\"The requested model is not supported.\",\"code\":\"model_not_supported\",\"param\":\"model\",\"type\":\"invalid_request_error\"}}}\nraw-http-request=/home/user/.omp/logs/http-400-requests/1756800000000-abc123.json";
	let component = component(body);

	let rows = component.error_rows(80);
	assert!(plain(&rows).lines().next().unwrap().starts_with("Error: 400 "));
	for row in &rows {
		assert!(visible_width(row) <= 80, "row too wide: {row:?}");
	}
	let joined = plain(&rows).lines().map(str::trim).collect::<String>();
	assert!(joined.contains("\"type\":\"invalid_request_error\"}}"), "{joined}");
	assert!(
		joined.contains("raw-http-request=/home/user/.omp/logs/http-400-requests/1756800000000-abc123.json"),
		"{joined}"
	);
	assert!(!joined.contains("more line"), "{joined}");
}

#[test]
fn clamps_a_huge_single_line_error_to_the_row_budget_with_an_expand_hint() {
	let body = format!("502 {}", "<p>Bad Gateway</p> ".repeat(200));
	let mut component = component(&body);

	let collapsed = plain(&component.error_rows(100));
	let body_rows = collapsed.lines().filter(|row| row.contains("<p>")).count();
	assert_eq!(body_rows, 8, "{collapsed}");
	assert!(
		collapsed
			.lines()
			.any(|row| row.contains("more line") && row.contains("to expand")),
		"{collapsed}"
	);

	component.set_expanded(true);
	let expanded = plain(&component.error_rows(100));
	assert!(
		expanded.lines().filter(|row| row.contains("<p>")).count() > 8,
		"{expanded}"
	);
}

#[test]
fn caps_a_long_error_collapsed_and_reveals_the_full_body_when_expanded() {
	let mut component = component(&long_error(30));

	let collapsed = plain(&component.error_rows(120));
	let shown = collapsed
		.lines()
		.filter(|row| row.contains("provider error detail line "))
		.count();
	assert_eq!(shown, 8, "{collapsed}");
	assert!(!collapsed.contains("provider error detail line 29"), "{collapsed}");
	assert!(collapsed.contains("+22 more lines"), "{collapsed}");

	component.set_expanded(true);
	let expanded = plain(&component.error_rows(120));
	let shown_expanded = expanded
		.lines()
		.filter(|row| row.contains("provider error detail line "))
		.count();
	assert_eq!(shown_expanded, 30, "{expanded}");
	assert!(expanded.contains("provider error detail line 0"), "{expanded}");
	assert!(expanded.contains("provider error detail line 29"), "{expanded}");
	assert!(!expanded.contains("more lines"), "{expanded}");

	component.set_expanded(false);
	let recollapsed = plain(&component.error_rows(120));
	assert!(!recollapsed.contains("provider error detail line 29"), "{recollapsed}");
}

#[test]
fn reveals_the_full_body_inline_when_expanded_while_the_error_is_pinned() {
	let mut component = component(&long_error(30));
	component.set_error_pinned(true);

	let pinned = plain(&component.error_rows(120));
	assert!(!pinned.contains("provider error detail line 0"), "{pinned}");

	component.set_expanded(true);
	let expanded = plain(&component.error_rows(120));
	let shown = expanded
		.lines()
		.filter(|row| row.contains("provider error detail line "))
		.count();
	assert_eq!(shown, 30, "{expanded}");
	assert!(expanded.contains("provider error detail line 29"), "{expanded}");

	component.set_expanded(false);
	let recollapsed = plain(&component.error_rows(120));
	assert!(!recollapsed.contains("provider error detail line 0"), "{recollapsed}");
}
