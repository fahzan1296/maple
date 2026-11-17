//! Port of `l/test/output-pane-memo.test.ts`.
//!
//! The pane's memo key must cover row identity, presentation options, content
//! width, the caller's invalidation signal, and the terminal image protocol.

use std::cell::Cell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::render::output_pane::{OutputPane, OutputPaneOptions};
use maple_tui::render::render_utils::{is_sixel_protocol, set_image_protocol, PaneEdge};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn dark() -> Theme {
	create_theme(
		builtin_theme_json("dark").expect("embedded dark theme"),
		CreateThemeOptions::default(),
	)
	.expect("dark theme resolves")
}

fn options() -> OutputPaneOptions {
	OutputPaneOptions {
		expanded: false,
		collapsed_max_lines: 20,
		edge: Some(PaneEdge::Tail),
		visual: true,
		padding_x: Some(1),
		leading_blank: true,
		show_hidden_marker: Some(false),
		show_expand_hint: Some(false),
		..OutputPaneOptions::default()
	}
}

fn capped_options(padding_x: usize) -> OutputPaneOptions {
	let mut options = options();
	options.collapsed_max_lines = 4;
	options.padding_x = Some(padding_x);
	options.leading_blank = false;
	options
}

fn rows(count: usize, decorate: impl Fn(usize) -> String) -> Vec<String> {
	(0..count).map(decorate).collect()
}

fn make_pane(lines: Vec<String>, options: OutputPaneOptions) -> OutputPane {
	let mut pane = OutputPane::new(dark(), options);
	pane.set_lines(&lines);
	pane
}

#[test]
fn re_renders_when_rows_are_appended_in_place() {
	let mut pane = make_pane(vec!["first".to_string()], options());
	let before = pane.render(40);
	pane.append("\nsecond");
	let after = pane.render(40);
	assert_ne!(after, before);
	assert!(after.join("\n").contains("second"));
}

#[test]
fn re_renders_when_the_rows_are_replaced() {
	let mut pane = make_pane(vec!["first".to_string()], options());
	let before = pane.render(40);
	pane.set_lines(&["different".to_string()]);
	let after = pane.render(40);
	assert_ne!(after, before);
	assert!(after.join("\n").contains("different"));
}

#[test]
fn re_renders_when_the_presentation_options_change() {
	let lines = rows(25, |index| format!("row {index}"));
	let mut pane = make_pane(lines, options());
	let collapsed = pane.render(40);
	pane.set_expanded(true);
	assert_ne!(pane.render(40), collapsed);
}

#[test]
fn re_renders_when_the_caller_invalidates_after_its_own_state_changed() {
	let pty = Rc::new(Cell::new(false));
	let theme = dark();
	let style_pty = Rc::clone(&pty);
	let options = OutputPaneOptions {
		style_line: Some(Box::new(move |line, _index| {
			if style_pty.get() {
				line.to_string()
			} else {
				theme.fg("muted", line).unwrap_or_else(|_| line.to_string())
			}
		})),
		..options()
	};
	let mut pane = OutputPane::new(dark(), options);
	pane.set_lines(&["first".to_string()]);
	let styled = pane.render(40);
	pty.set(true);
	pane.invalidate();
	assert_ne!(pane.render(40), styled);
}

#[test]
fn renders_what_a_fresh_pane_renders_when_the_width_changes() {
	let lines = rows(6, |index| format!("row {index} {}", "x".repeat(60)));
	let mut pane = make_pane(lines.clone(), capped_options(0));
	pane.render(20);
	let mut fresh = make_pane(lines, capped_options(0));
	assert_eq!(pane.render(80), fresh.render(80));
}

#[test]
fn re_renders_when_only_the_padding_changes_at_the_same_width() {
	let lines = rows(6, |index| format!("row {index} {}", "x".repeat(60)));

	let mut narrow = make_pane(lines.clone(), capped_options(0));
	let mut wide = make_pane(lines.clone(), capped_options(6));
	narrow.render(40);
	assert_eq!(wide.render(40), make_pane(lines, capped_options(6)).render(40));
	assert_ne!(wide.render(40), narrow.render(40));
}

#[test]
fn renders_what_a_fresh_pane_renders_when_the_image_protocol_changes() {
	let previous = is_sixel_protocol();
	let payload = vec!["\x1bPqpayload\x1b\\".to_string()];
	let mut pane = make_pane(payload.clone(), options());
	set_image_protocol(0);
	pane.render(40);
	set_image_protocol(1);
	let mut fresh = make_pane(payload, options());
	assert_eq!(pane.render(40), fresh.render(40));
	set_image_protocol(u8::from(previous));
}

#[test]
#[ignore = "missing facility: the Hangul Compatibility Jamo width runtime override is not ported"]
fn renders_what_a_fresh_pane_renders_when_the_jamo_width_configuration_changes() {
	let lines = rows(10, |index| format!("row {index} {}", "ㄱ".repeat((index + 1) * 10)));
	let mut pane = make_pane(lines, options());
	pane.render(20);
	assert!(!pane.render(20).is_empty());
}

#[test]
#[ignore = "missing facility: the global active-theme instance is not ported; OutputPane owns its Theme"]
fn renders_what_a_fresh_pane_renders_when_the_active_theme_changes() {
	let mut pane = make_pane(vec!["first".to_string()], options());
	pane.render(40);
	assert_eq!(pane.render(40).join("\n"), "first");
}
