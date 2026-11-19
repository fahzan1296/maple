//! Port of `l/test/tools-native-shell.test.ts`.
//!
//! The reference drives the full native run box (shell visualization, media
//! children, eval cells). This crate ports the command preview, run-state/foot
//! facts and the card renderer; the assertions below cover those seams.

use maple_tui::component::Component;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::loader::{create_theme, CreateThemeOptions};
use maple_tui::theme::Theme;
use maple_tui::tools::bash_execution::{
	format_bash_command_lines, format_bash_command_text, format_bash_env_assignments, BashExecution,
	BashRenderArgs, BashToolDetails, ShellRunState,
};
use maple_tui::tools::tool_execution::ToolExecution;

fn theme() -> Theme {
	let json = builtin_theme_json("dark").expect("builtin theme");
	create_theme(json, CreateThemeOptions::default()).expect("theme")
}

fn args() -> BashRenderArgs {
	BashRenderArgs {
		command: "make".to_string(),
		env: vec![("CI".to_string(), "1".to_string())],
		cwd: None,
		cd_separator: None,
	}
}

#[test]
fn command_text_carries_cwd_env_and_tabs() {
	assert_eq!(format_bash_env_assignments(&args().env), "CI=\"1\"");
	assert_eq!(format_bash_command_text(&args()), "CI=\"1\" make");

	let mut nested = args();
	nested.cwd = Some("/tmp".to_string());
	nested.cd_separator = Some(";".to_string());
	nested.command = "echo\tbuilt".to_string();
	assert_eq!(format_bash_command_text(&nested), "cd /tmp; CI=\"1\" echo  built");

	let mut empty = BashRenderArgs::default();
	empty.env.push(("B".to_string(), "two\nlines".to_string()));
	empty.env.push(("A".to_string(), "$HOME".to_string()));
	assert_eq!(format_bash_env_assignments(&empty.env), "A=\"\\$HOME\" B=\"two\\nlines\"");
	assert_eq!(format_bash_command_text(&empty), "A=\"\\$HOME\" B=\"two\\nlines\" …");
}

#[test]
fn command_lines_prefix_only_the_first_line() {
	let mut nested = args();
	nested.cwd = Some("/work".to_string());
	nested.command = "echo one\necho two".to_string();
	let lines = format_bash_command_lines(&nested, &theme());
	assert_eq!(lines.len(), 2);
	assert!(lines[0].ends_with("CI=\"1\" echo one"), "{:?}", lines[0]);
	assert_eq!(lines[1], "echo two");
	assert!(!lines[1].contains("cd /work"));
}

#[test]
fn run_state_and_footer_map_terminal_facts() {
	let mut card = BashExecution::new(args(), theme());
	assert_eq!(card.run_state(), ShellRunState::Running);
	assert!(card.footer_parts().is_empty());

	let mut execution = ToolExecution::new("bash");
	execution.set_title("Building the project");
	assert!(execution.start());
	assert!(execution.fail(Some(2)));
	card.set_execution(execution);
	card.set_details(BashToolDetails {
		timeout_seconds: Some(300),
		requested_timeout_seconds: Some(600),
		wall_time_ms: Some(1500),
		exit_code: Some(2),
		..BashToolDetails::default()
	});
	assert_eq!(card.run_state(), ShellRunState::Failed);
	assert_eq!(
		card.footer_parts(),
		vec![
			"Wall: 1.50s".to_string(),
			"Timeout: 300s (requested 600s clamped)".to_string(),
			"Exit: 2".to_string(),
		]
	);

	let mut background = ToolExecution::new("bash");
	assert!(background.start());
	card.set_execution(background);
	card.set_details(BashToolDetails {
		background: Some("running".to_string()),
		job_id: Some("job-7".to_string()),
		..BashToolDetails::default()
	});
	assert_eq!(card.run_state(), ShellRunState::Background);
	assert_eq!(card.footer_parts(), vec!["Job job-7".to_string()]);
}

#[test]
fn settled_card_renders_head_command_output_and_foot() {
	let mut card = BashExecution::new(
		BashRenderArgs {
			command: "make".to_string(),
			env: vec![("CI".to_string(), "1".to_string())],
			..BashRenderArgs::default()
		},
		theme(),
	);
	let mut execution = ToolExecution::new("bash");
	execution.set_title("Building the project");
	assert!(execution.start());
	assert!(execution.succeed());
	card.set_execution(execution);
	card.set_details(BashToolDetails {
		wall_time_ms: Some(1500),
		timeout_seconds: Some(300),
		..BashToolDetails::default()
	});
	card.set_output("built\n");

	let rendered = card.render(100).join("\n");
	assert!(rendered.contains("Building the project"), "{rendered}");
	assert!(rendered.contains("CI=\"1\" make"), "{rendered}");
	assert!(rendered.contains("built"), "{rendered}");
	assert!(rendered.contains("Wall: 1.50s"), "{rendered}");
	assert!(!rendered.contains("Exit: 0"), "{rendered}");
}
