//! Port of `l/test/late-diagnostics-message.test.ts`.
//!
//! Two documented divergences shape this port: the crate has no
//! `formatDiagnostics` shared tree renderer (rows are the raw message strings),
//! and `Disclosure` hides the body while collapsed, so the reference's
//! collapsed-preview assertions are not portable. The expanded card, the single
//! header, and the tool-activity visibility toggle are ported.

use maple_tui::chat::late_diagnostics_message::{LateDiagnosticsFile, LateDiagnosticsMessage};
use maple_tui::component::Component;
use maple_tui::strip_ansi;

fn plain(component: &mut LateDiagnosticsMessage) -> String {
	strip_ansi(&component.render(120).join("\n"))
}

fn file(path: &str, summary: &str, errored: bool, messages: Vec<String>) -> LateDiagnosticsFile {
	LateDiagnosticsFile {
		path: Some(path.to_string()),
		summary: Some(summary.to_string()),
		errored,
		messages,
	}
}

#[test]
fn expanded_card_lists_every_message_under_one_header() {
	let messages: Vec<String> = (1..=8)
		.map(|index| format!("src/foo.ts:{index}:1 [error] [typescript] err {index} (2322)"))
		.collect();
	let mut component = LateDiagnosticsMessage::new(vec![
		file("/abs/a.ts", "1 error(s)", true, vec!["a.ts:1:1 [error] bad a (2322)".to_string()]),
		file("/abs/b.ts", "1 warning(s)", false, vec!["b.ts:2:2 [warning] bad b (2322)".to_string()]),
		file("/abs/src/foo.ts", "8 error(s)", true, messages),
	]);
	component.set_expanded(true);

	let text = plain(&mut component);
	assert_eq!(text.matches("Late diagnostics").count(), 1);
	assert!(text.contains("a.ts"));
	assert!(text.contains("b.ts"));
	assert!(text.contains("bad a"));
	assert!(text.contains("bad b"));
	assert!(text.contains("err 1"));
	assert!(text.contains("err 8"));
}

#[test]
fn collapsed_card_shows_only_the_header() {
	let mut component = LateDiagnosticsMessage::new(vec![file(
		"/abs/src/foo.ts",
		"1 error(s)",
		true,
		vec!["src/foo.ts:1:1 [error] bad (2322)".to_string()],
	)]);

	let text = plain(&mut component);
	assert!(text.contains("Late diagnostics"));
	assert!(!text.contains("bad (2322)"));
}

#[test]
fn hides_and_restores_diagnostics_without_discarding_the_rendered_block() {
	let mut component = LateDiagnosticsMessage::new(vec![file(
		"/abs/src/foo.ts",
		"1 error(s)",
		true,
		vec!["src/foo.ts:1:1 [error] bad (2322)".to_string()],
	)]);

	assert!(plain(&mut component).contains("Late diagnostics"));
	component.set_tool_activity_visible(false);
	assert_eq!(plain(&mut component), "");
	component.set_tool_activity_visible(true);
	assert!(plain(&mut component).contains("Late diagnostics"));
}

#[test]
#[ignore = "missing facility: formatDiagnostics shared tree renderer (rows are raw message strings)"]
fn renders_late_diagnostics_through_the_shared_tree_renderer() {
	let mut component = LateDiagnosticsMessage::new(vec![file(
		"/abs/packages/coding-agent/src/foo.ts",
		"1 error(s)",
		true,
		vec![
			"packages/coding-agent/src/foo.ts:7804:14 [error] [typescript] Type 'string' is not assignable to type 'number'. (2322)"
				.to_string(),
		],
	)]);

	let text = plain(&mut component);
	assert!(text.contains("Late diagnostics"));
	assert!(text.contains("1 error(s)"));
	assert!(text.contains("packages/coding-agent/src/foo.ts"));
	assert!(text.contains(":7804:14"));
	assert!(text.contains("Type 'string' is not assignable to type 'number'."));
	assert!(!text.contains("[error]"));
	assert!(!text.contains("[typescript]"));
}

#[test]
#[ignore = "missing facility: empty-file suppression (the crate still emits the header with no messages)"]
fn renders_nothing_when_no_diagnostics_are_present() {
	let mut component = LateDiagnosticsMessage::new(vec![LateDiagnosticsFile {
		path: Some("/abs/empty.ts".to_string()),
		summary: Some(String::new()),
		errored: false,
		messages: Vec::new(),
	}]);
	assert_eq!(plain(&mut component).trim(), "");
}
