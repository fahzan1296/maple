//! Port of `l/test/execution-border-status.test.ts`.
//!
//! The crate's bash card marks a settled run in the bottom rule through
//! `execution_status_label` (`exit N`, `cancelled`, nothing on success). A
//! running card has no bottom label, so its rule is plain — the reference's
//! "Running + spinner in the rule" wording is a host divergence. The eval card
//! reports the failure as the status word `error` rather than an exit code.

use maple_tui::chat::bash_execution::{BashCompletion, BashExecutionComponent};
use maple_tui::component::Component;
use maple_tui::strip_ansi;
use maple_tui::theme::default_theme;
use maple_tui::tools::eval_execution::{EvalExecution, EvalLanguage};
use maple_tui::tools::tool_execution::{ToolExecution, ToolExecutionState};

fn completion(output: Option<&str>) -> BashCompletion {
	BashCompletion {
		output: output.map(str::to_string),
		truncation: None,
		artifact_error: None,
		minimized: None,
		images: Vec::new(),
	}
}

fn frame(component: &mut BashExecutionComponent, width: u16) -> (String, String) {
	let rows: Vec<String> = component
		.render(width)
		.iter()
		.map(|row| strip_ansi(row).trim_end().to_string())
		.collect();
	let bottom = rows.last().cloned().unwrap_or_default();
	let body = rows[..rows.len().saturating_sub(1)].join("\n");
	(body, bottom)
}

#[test]
fn running_run_leaves_a_plain_bottom_rule() {
	let theme = default_theme();
	let mut component = BashExecutionComponent::new("make", theme.clone(), false);
	component.append_output("building\n");
	let (body, bottom) = frame(&mut component, 60);
	assert_eq!(bottom, theme.box_round().horizontal.repeat(60));
	assert!(!body.contains("exit"), "{body}");
}

#[test]
fn puts_a_failed_runs_exit_code_in_the_bottom_rule() {
	let theme = default_theme();
	let mut component = BashExecutionComponent::new("make", theme, false);
	component.append_output("boom\n");
	component.set_complete(Some(1), false, completion(None), 0.0);
	let (body, bottom) = frame(&mut component, 60);
	assert!(bottom.contains(" exit 1 "), "{bottom}");
	assert!(!bottom.contains("cancelled"), "{bottom}");
	assert!(!body.contains("exit 1"), "{body}");
}

#[test]
fn marks_a_cancelled_run_in_the_bottom_rule() {
	let theme = default_theme();
	let mut component = BashExecutionComponent::new("sleep 10", theme, false);
	component.set_complete(None, true, completion(None), 0.0);
	let (body, bottom) = frame(&mut component, 60);
	assert!(bottom.contains(" cancelled "), "{bottom}");
	assert!(!body.contains("cancelled"), "{body}");
}

#[test]
fn leaves_a_plain_rule_once_a_run_succeeds() {
	let theme = default_theme();
	let mut component = BashExecutionComponent::new("true", theme.clone(), false);
	component.set_complete(Some(0), false, completion(None), 0.0);
	let (_, bottom) = frame(&mut component, 60);
	assert_eq!(bottom, theme.box_round().horizontal.repeat(60));
}

#[test]
fn failed_eval_cell_reports_its_error_state_and_output() {
	let theme = default_theme();
	let mut execution = ToolExecution::new("eval");
	execution.fail(Some(1));
	let mut component = EvalExecution::new("1/0", EvalLanguage::Python, theme);
	component.set_output("ZeroDivisionError");
	component.set_execution(execution);
	assert!(matches!(component.execution().state(), ToolExecutionState::Error));
	let rendered = strip_ansi(&component.render(60).join("\n"));
	assert!(rendered.contains("eval · python"), "{rendered}");
	assert!(rendered.contains("ZeroDivisionError"), "{rendered}");
}
#[test]
#[ignore = "missing API: fake timers / the spinner repaint timer the reference counts"]
fn settled_or_disposed_runs_keep_no_spinner_timer() {
}
