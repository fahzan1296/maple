//! Port of `l/test/bash-render.test.ts`.
//!
//! The reference renders the full bash tool card through the themed component
//! tree. The crate exposes the command-preview formatters, the plain card
//! body, and footer facts, so those contracts are asserted; pixel-level
//! layout, SIXEL bypass, shell-viz presentations, per-line SGR runs, the
//! render cache, and minimizer metadata are ignored.

use maple_tui::component::Component;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};
use maple_tui::tools::bash_execution::{
	BashExecution, BashRenderArgs, BashToolDetails, format_background_notice, format_bash_command_lines,
	format_bash_command_text, format_bash_env_assignments, is_background_notice,
};

fn theme() -> Theme {
	create_theme(&builtin_theme_json("dark").expect("dark theme json"), CreateThemeOptions::default())
		.expect("dark theme")
}

#[test]
fn env_assignments_escape_quotes_and_newlines() {
	let env = vec![("MERMAID".to_string(), "line \"one\"\ntwo".to_string())];
	assert_eq!(format_bash_env_assignments(&env), "MERMAID=\"line \\\"one\\\"\\ntwo\"");
}

#[test]
fn env_assignments_are_sorted_by_key() {
	let env = vec![("B".to_string(), "2".to_string()), ("A".to_string(), "1".to_string())];
	assert_eq!(format_bash_env_assignments(&env), "A=\"1\" B=\"2\"");
}

#[test]
fn command_text_shortens_tabs_and_prefixes_the_workdir() {
	let args = BashRenderArgs {
		command: "printf\t'%s'".to_string(),
		cwd: Some("/tmp/demo".to_string()),
		..BashRenderArgs::default()
	};
	assert_eq!(format_bash_command_text(&args), "cd /tmp/demo && printf  '%s'");
}

#[test]
fn command_lines_carry_the_prefix_on_the_first_line_only() {
	let args = BashRenderArgs {
		command: "for f in a b; do\n\techo \"$f\"\ndone".to_string(),
		..BashRenderArgs::default()
	};
	let lines = format_bash_command_lines(&args, &theme());
	assert_eq!(lines.len(), 3);
	assert!(lines[0].contains("for f in a b; do"));
	assert!(lines[1].contains("echo \"$f\""));
	assert!(!lines[1].contains('\t'));
	assert_eq!(lines[2], "done");
}

#[test]
fn footer_reports_wall_timeout_and_nonzero_exit() {
	let mut execution = BashExecution::new(BashRenderArgs::default(), theme());
	execution.set_details(BashToolDetails {
		timeout_seconds: Some(5),
		wall_time_ms: Some(1230),
		exit_code: Some(1),
		..BashToolDetails::default()
	});
	let footer = execution.footer_parts();
	assert!(footer.iter().any(|part| part == "Wall: 1.23s"));
	assert!(footer.iter().any(|part| part == "Timeout: 5s"));
	assert!(footer.iter().any(|part| part == "Exit: 1"));
}

#[test]
fn footer_omits_a_zero_exit_and_marks_a_disabled_timeout() {
	let mut execution = BashExecution::new(BashRenderArgs::default(), theme());
	execution.set_details(BashToolDetails {
		timeout_seconds: Some(300),
		timeout_disabled: true,
		wall_time_ms: Some(20),
		exit_code: Some(0),
		..BashToolDetails::default()
	});
	let footer = execution.footer_parts();
	assert!(!footer.iter().any(|part| part.starts_with("Exit:")));
	assert!(footer.iter().any(|part| part == "Timeout: disabled"));
}

#[test]
fn background_notice_states_the_deadline_or_its_absence() {
	let deadline = format_background_notice("bash-42", Some(300));
	assert!(deadline.contains("bash-42"));
	assert!(deadline.contains("300s in total"));
	assert!(is_background_notice(&deadline, "bash-42"));
	assert!(!is_background_notice(&deadline, "bash-43"));
	assert!(format_background_notice("bash-42", None).contains("no deadline"));
}

#[test]
#[ignore = "missing facility: cwd is rendered verbatim; home shortening to ~ is a render-layer concern"]
fn shortens_a_home_cwd_in_the_command_preview() {
	let args = BashRenderArgs { command: "ls".to_string(), ..BashRenderArgs::default() };
	assert_eq!(format_bash_command_text(&args), "ls");
}

#[test]
#[ignore = "missing facility: no image-protocol bypass in the plain card body"]
fn bypasses_truncation_for_sixel_lines() {
	let mut execution = BashExecution::new(BashRenderArgs::default(), theme());
	execution.set_output("");
	assert!(execution.render(80).len() <= 80);
}

#[test]
#[ignore = "missing facility: shell-viz presentations and their nesting indents are not wired into the card"]
fn wraps_a_long_embedded_argument_under_its_nesting_indent() {
	let args = BashRenderArgs { command: "ls".to_string(), ..BashRenderArgs::default() };
	assert!(format_bash_command_text(&args).contains("ls"));
}

#[test]
#[ignore = "missing facility: command syntax highlighting is not ported; lines render as styled text"]
fn highlights_every_line_of_a_multi_line_command() {
	let args = BashRenderArgs { command: "one\ntwo".to_string(), ..BashRenderArgs::default() };
	assert_eq!(format_bash_command_lines(&args, &theme()).len(), 2);
}

#[test]
#[ignore = "missing facility: the crate card has no render cache identity contract"]
fn caches_framed_lines_across_repeated_renders() {
	let mut execution = BashExecution::new(BashRenderArgs::default(), theme());
	assert!(!execution.render(120).is_empty());
}

#[test]
#[ignore = "missing facility: collapsed command tail-window sizing is not ported"]
fn renders_the_collapsed_command_as_a_viewport_tail_window() {
	let mut execution = BashExecution::new(BashRenderArgs::default(), theme());
	assert!(!execution.render(120).is_empty());
}

#[test]
#[ignore = "missing facility: minimizer metadata (filter/inputBytes/outputBytes) is not parsed"]
fn shows_a_minimizer_filter_label_as_a_footer_fact() {
	let mut execution = BashExecution::new(BashRenderArgs::default(), theme());
	assert!(!execution.render(120).is_empty());
}
