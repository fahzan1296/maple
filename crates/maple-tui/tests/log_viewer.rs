//! Port of `l/test/log-viewer.test.ts`.
//!
//! Adaptations: the crate model exposes external history as
//! `set_has_older_logs` + `prepend_logs` (no async `loadOlderLogs` callback), so
//! the two external-source tests drive those. The component has no `onUpdate`
//! callback, so the click test asserts the rendered expansion only.

use maple_tui::apps::debug::log_viewer::{
	build_log_copy_payload, split_log_text, DebugLogViewerComponent,
	DebugLogViewerComponentOptions, DebugLogViewerModel, DebugLogViewerModelOptions,
	LogViewerDeps, ViewerRow, LOAD_OLDER_LABEL, SESSION_BOUNDARY_WARNING,
};
use maple_tui::component::Component;
use maple_tui::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use maple_tui::overlays::extensions::display_text::sanitize_display_text;

const PROCESS_START_MS: i64 = 1_771_070_400_000;

fn options() -> DebugLogViewerModelOptions {
	DebugLogViewerModelOptions {
		process_start_ms: Some(PROCESS_START_MS),
		..DebugLogViewerModelOptions::default()
	}
}

fn label(row: &ViewerRow) -> String {
	match row {
		ViewerRow::Warning => SESSION_BOUNDARY_WARNING.to_string(),
		ViewerRow::LoadOlder => LOAD_OLDER_LABEL.to_string(),
		ViewerRow::Log { log_index } => format!("log:{log_index}"),
	}
}

fn row_labels(model: &DebugLogViewerModel) -> Vec<String> {
	model.rows().iter().map(label).collect()
}

#[test]
fn inserts_session_boundary_warning_between_older_and_current_session_logs() {
	let logs = [
		r#"{"timestamp":"2026-02-14T11:59:00.000Z","level":"info","message":"old"}"#,
		r#"{"timestamp":"2026-02-14T11:59:30.000Z","level":"info","message":"old-2"}"#,
		r#"{"timestamp":"2026-02-14T12:00:05.000Z","level":"info","message":"current"}"#,
	]
	.join("\n");
	let model = DebugLogViewerModel::new(&logs, options());
	assert_eq!(
		row_labels(&model),
		vec!["log:0", "log:1", SESSION_BOUNDARY_WARNING, "log:2"]
	);
}

#[test]
fn filters_logs_with_case_insensitive_substring_matching() {
	let mut model =
		DebugLogViewerModel::new("Alpha\nbeta\nGamma\nBETTER", DebugLogViewerModelOptions::default());
	model.set_filter_query("be".to_string());
	assert_eq!(row_labels(&model), vec!["log:1", "log:3"]);
	assert_eq!(model.visible_log_count(), 2);
}

#[test]
fn filters_to_current_process_pid_when_enabled() {
	let logs = [
		r#"{"pid":42,"level":"info","message":"alpha"}"#,
		r#"{"pid":84,"level":"info","message":"beta"}"#,
		r#"{"level":"info","message":"missing"}"#,
	]
	.join("\n");
	let mut model = DebugLogViewerModel::new(
		&logs,
		DebugLogViewerModelOptions {
			process_pid: Some(42),
			..DebugLogViewerModelOptions::default()
		},
	);
	assert_eq!(model.visible_log_count(), 3);
	model.toggle_process_filter();
	assert_eq!(model.visible_log_count(), 1);
	assert_eq!(row_labels(&model), vec!["log:0"]);
}

#[test]
fn selects_all_visible_log_rows() {
	let mut model =
		DebugLogViewerModel::new("alpha\nbeta\ngamma", DebugLogViewerModelOptions::default());
	model.select_all_visible();
	assert_eq!(model.get_selected_log_indices(), vec![0, 1, 2]);
}

#[test]
fn progressively_loads_older_entries_in_chunks() {
	let logs = (0..120)
		.map(|index| format!("log-{index}"))
		.collect::<Vec<_>>()
		.join("\n");
	let mut model =
		DebugLogViewerModel::new(&logs, DebugLogViewerModelOptions::default());
	assert_eq!(model.visible_log_count(), 50);
	assert_eq!(row_labels(&model)[..2].to_vec(), vec![LOAD_OLDER_LABEL, "log:70"]);

	assert!(model.load_older_in_memory(5));
	assert_eq!(model.visible_log_count(), 55);
	assert_eq!(row_labels(&model)[..2].to_vec(), vec![LOAD_OLDER_LABEL, "log:65"]);

	assert!(model.load_older_in_memory(50));
	assert_eq!(model.visible_log_count(), 105);
	assert_eq!(row_labels(&model)[..2].to_vec(), vec![LOAD_OLDER_LABEL, "log:15"]);
}

#[test]
fn loads_older_entries_from_external_sources_while_keeping_cursor_stable() {
	let mut model = DebugLogViewerModel::new(
		"new-1\nnew-2\nnew-3",
		DebugLogViewerModelOptions {
			has_older_logs: true,
			..DebugLogViewerModelOptions::default()
		},
	);
	assert_eq!(model.rows()[0].kind(), "load-older");
	assert_eq!(model.cursor_log_index(), None);
	model.set_has_older_logs(true);
	let prepended = model.prepend_logs("old-1\nold-2");
	assert_eq!(prepended, 2);
	assert_eq!(
		model.get_raw_line(model.cursor_log_index.unwrap_or(0)),
		"new-3"
	);
	assert_eq!(model.rows()[0].kind(), "load-older");
}

#[test]
fn clamps_cursor_when_filtered_list_shrinks() {
	let mut model =
		DebugLogViewerModel::new("alpha\nbeta\ngamma", DebugLogViewerModelOptions::default());
	model.move_cursor(2, false);
	assert_eq!(model.cursor_log_index(), Some(2));
	model.set_filter_query("alpha".to_string());
	assert_eq!(model.cursor_log_index(), Some(0));
	assert_eq!(model.visible_log_count(), 1);
}

#[test]
fn resets_selection_anchor_when_filtered_view_drops_the_anchor_log() {
	let mut model = DebugLogViewerModel::new(
		"alpha\nbeta\ngamma\ndelta",
		DebugLogViewerModelOptions::default(),
	);
	model.move_cursor(-999, false);
	model.move_cursor(2, true);
	assert_eq!(model.get_selected_log_indices(), vec![0, 1, 2]);
	model.set_filter_query("beta".to_string());
	assert_eq!(model.get_selected_log_indices(), vec![1]);
}

#[test]
fn shows_session_boundary_warning_only_when_older_and_newer_logs_are_visible() {
	let logs = [
		r#"{"timestamp":"2026-02-14T11:59:00.000Z","level":"info","message":"old"}"#,
		r#"{"timestamp":"2026-02-14T12:00:05.000Z","level":"info","message":"current"}"#,
		r#"{"timestamp":"2026-02-14T12:00:10.000Z","level":"info","message":"current-2"}"#,
	]
	.join("\n");
	let mut model = DebugLogViewerModel::new(&logs, options());

	model.set_filter_query("old".to_string());
	assert_eq!(
		model.rows().iter().map(|row| row.kind()).collect::<Vec<_>>(),
		vec!["log"]
	);
	model.set_filter_query("current-2".to_string());
	assert_eq!(
		model.rows().iter().map(|row| row.kind()).collect::<Vec<_>>(),
		vec!["log"]
	);
	model.set_filter_query("current".to_string());
	assert_eq!(row_labels(&model), vec!["log:1", "log:2"]);
	model.set_filter_query(String::new());
	assert_eq!(
		row_labels(&model),
		vec!["log:0", SESSION_BOUNDARY_WARNING, "log:1", "log:2"]
	);
}

#[test]
fn copies_only_selected_visible_entries() {
	let mut model =
		DebugLogViewerModel::new("alpha\nbar\nbaz", DebugLogViewerModelOptions::default());
	model.set_filter_query("ba".to_string());
	model.move_cursor(-999, false);
	model.move_cursor(1, true);
	assert_eq!(
		build_log_copy_payload(&model.get_selected_raw_lines()),
		"bar\nbaz"
	);
}

#[test]
fn supports_shift_range_selection_and_reset_on_plain_movement() {
	let mut model =
		DebugLogViewerModel::new("a\nb\nc\nd", DebugLogViewerModelOptions::default());
	model.move_cursor(-999, false);
	model.move_cursor(1, true);
	model.move_cursor(1, true);
	assert_eq!(model.get_selected_log_indices(), vec![0, 1, 2]);
	model.move_cursor(1, false);
	assert_eq!(model.get_selected_log_indices(), vec![3]);
}

#[test]
fn expands_and_collapses_all_selected_rows() {
	let mut model =
		DebugLogViewerModel::new("a\nb\nc", DebugLogViewerModelOptions::default());
	model.move_cursor(-999, false);
	model.move_cursor(1, true);
	model.expand_selected();
	assert!(model.is_expanded(0));
	assert!(model.is_expanded(1));
	model.collapse_selected();
	assert!(!model.is_expanded(0));
	assert!(!model.is_expanded(1));
}

struct Deps;

impl LogViewerDeps for Deps {
	fn copy_to_clipboard(&self, _text: &str) -> Result<(), String> {
		Ok(())
	}
}

fn viewer(logs: &str) -> DebugLogViewerComponent<Deps> {
	DebugLogViewerComponent::new(DebugLogViewerComponentOptions {
		logs: logs.to_string(),
		terminal_rows: 12,
		deps: Deps,
		on_exit: Box::new(|| {}),
		process_start_ms: None,
		process_pid: None,
	})
}

fn plain(lines: &[String]) -> String {
	lines
		.iter()
		.map(|line| sanitize_display_text(line))
		.collect::<Vec<_>>()
		.join("\n")
}

fn click_row(viewer: &mut DebugLogViewerComponent<Deps>, row: u16) {
	viewer.handle_mouse(&MouseEvent {
		kind: MouseKind::Down(MouseButtonKind::Left),
		column: 2,
		row,
		shift: false,
		ctrl: false,
		alt: false,
	});
}

#[test]
fn selects_and_expands_a_clicked_visible_log_row() {
	let mut viewer = viewer("alpha\nbeta\ngamma");
	viewer.render(80);
	click_row(&mut viewer, 6);
	assert!(plain(&viewer.render(80)).contains("▾ beta"));
}

#[test]
fn stops_loading_older_history_at_the_entry_cap_and_never_evicts_the_newest_entries() {
	let mut model = DebugLogViewerModel::new(
		"today-1\ntoday-2\ntoday-3",
		DebugLogViewerModelOptions {
			has_older_logs: true,
			max_log_entries: 5,
			..DebugLogViewerModelOptions::default()
		},
	);
	assert_eq!(model.prepend_logs("old-1\nold-2\nold-3\nold-4"), 2);
	let lines = (0..model.log_count())
		.map(|index| model.get_raw_line(index).to_string())
		.collect::<Vec<_>>();
	assert_eq!(lines, vec!["old-3", "old-4", "today-1", "today-2", "today-3"]);
	assert!(model.history_limit_reached());
	while model.can_load_older() {
		model.load_older_in_memory(10);
	}
	assert!(!model.load_older_in_memory(10));
	assert_eq!(model.log_count(), 5);
}

#[test]
fn keeps_the_newest_entries_when_the_initial_log_already_exceeds_the_cap() {
	let model = DebugLogViewerModel::new(
		"a\nb\nc\nd",
		DebugLogViewerModelOptions {
			max_log_entries: 2,
			..DebugLogViewerModelOptions::default()
		},
	);
	assert_eq!(
		[model.get_raw_line(0), model.get_raw_line(1)],
		["c".to_string(), "d".to_string()]
	);
}

#[test]
fn tracks_selection_queries_across_cursor_moves_anchors_and_filter_changes() {
	let mut model = DebugLogViewerModel::new(
		"alpha\nbeta\ngamma\ndelta",
		DebugLogViewerModelOptions::default(),
	);
	model.move_cursor(-999, false);
	assert!(model.is_selected(0));
	assert_eq!(model.get_selected_count(), 1);
	model.move_cursor(2, true);
	assert_eq!(
		[0, 1, 2, 3].map(|index| model.is_selected(index)),
		[true, true, true, false]
	);
	assert_eq!(model.get_selected_count(), 3);
	model.move_cursor(1, false);
	assert_eq!(
		[0, 1, 2, 3].map(|index| model.is_selected(index)),
		[false, false, false, true]
	);
	model.set_filter_query("a".to_string());
	assert_eq!(model.get_selected_count(), 1);
	assert!(model.is_selected(model.cursor_log_index.unwrap_or(999)));
}

#[test]
fn re_renders_a_cached_row_when_its_expansion_or_the_width_changes() {
	let long = format!("beta {}", "x".repeat(120));
	let mut viewer = viewer(&format!("alpha\n{long}\ngamma"));
	assert!(plain(&viewer.render(160)).contains(&long));
	assert!(!plain(&viewer.render(80)).contains(&long));
	click_row(&mut viewer, 6);
	assert!(plain(&viewer.render(80)).contains("▾ beta"));
}

#[test]
fn joins_selected_lines_and_strips_control_ansi_sequences() {
	assert_eq!(
		build_log_copy_payload(&[
			"plain".to_string(),
			"\u{1b}[31mred\u{1b}[0m".to_string(),
			"ok\u{7}".to_string(),
			String::new()
		]),
		"plain\nred\nok"
	);
	assert_eq!(
		split_log_text("a\n\nb\n"),
		vec!["a".to_string(), "b".to_string()]
	);
}
