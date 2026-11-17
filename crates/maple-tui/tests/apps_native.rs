//! Port of `l/test/apps-native.test.ts` (pure state/data parts).
//!
//! Deviations: the reference drives `PsTopComponent`, `showGitOverlay` and
//! `CleanseBoardModel` through a Bun TUI and asserts native describe nodes
//! (`list`/`item`/`progress`/`meter`/`agent`). The crate deliberately has no
//! native describe for these apps (rendering is host-owned), so the port
//! covers the same semantics through the portable models: daemon table cells
//! and scope headers, git view/whitespace/focus transitions, footer hints,
//! suggested-file/patch-target selection, ps-top row sorting/selection, and
//! the cleanse live board/final table.

use maple_tui::apps::cleanse_board::{
	CleanseBoard, CleanseFinalEntry, CleanseProgressObserver, CleanseStatusBoard,
	format_cleanse_final_table,
};
use maple_tui::apps::git::diff_pane::ViewMode;
use maple_tui::apps::git::git_tui::{
	Focus, GitTuiModel, patch_target_for, suggested_file,
};
use maple_tui::apps::git::state::{ChangeArea, ChangeKind, ChangedFile, GitViewState, HeadCommit};
use maple_tui::apps::ps_data::{
	PsDaemonRow, PsScope, PsScopeKind, PsScopeReport, scope_header, state_cell, table_cells,
	uptime_cell,
};
use maple_tui::apps::ps_top::{JobStatus, PsTopAction, PsTopModel, TaskRow, action_for_key, sort_rows};
use maple_tui::tools::daemon::{DaemonSnapshot, DaemonState};

fn daemon(name: &str, state: DaemonState, pid: Option<u32>) -> DaemonSnapshot {
	DaemonSnapshot {
		name: name.to_string(),
		id: name.to_string(),
		state,
		pid,
		started_at: 1000.0,
		..DaemonSnapshot::default()
	}
}

fn row(name: &str, state: DaemonState) -> PsDaemonRow {
	PsDaemonRow {
		snapshot: daemon(name, state, Some(100)),
		command: Some(format!("run {name}")),
		cwd: Some("/work/app".to_string()),
		supervised: true,
	}
}

fn file(path: &str, area: ChangeArea) -> ChangedFile {
	ChangedFile {
		path: path.to_string(),
		kind: ChangeKind::Modified,
		area,
		..ChangedFile::default()
	}
}

fn view(unstaged: Vec<ChangedFile>, staged: Vec<ChangedFile>) -> GitViewState {
	GitViewState {
		cwd: "/repo".to_string(),
		branch: Some("main".to_string()),
		clean: false,
		unstaged,
		staged,
		head_commit: None,
	}
}

fn head_file(path: &str) -> ChangedFile {
	ChangedFile {
		path: path.to_string(),
		area: ChangeArea::Commit,
		..ChangedFile::default()
	}
}

fn head_commit(files: Vec<ChangedFile>) -> HeadCommit {
	HeadCommit {
		sha: "abc".to_string(),
		short_sha: "abc".to_string(),
		subject: "s".to_string(),
		body: String::new(),
		author_name: "a".to_string(),
		author_email: "a@b".to_string(),
		author_date: "now".to_string(),
		parents: vec![],
		files,
		files_loaded: true,
	}
}

fn task(id: &str, status: JobStatus, start: i64, end: Option<i64>, preview: &[&str]) -> TaskRow {
	TaskRow {
		id: id.to_string(),
		label: format!("label {id}"),
		kind: "proc".to_string(),
		start_time_ms: start,
		end_time_ms: end,
		status,
		working_dir: "/work/app".to_string(),
		preview_lines: preview.iter().map(|line| line.to_string()).collect(),
	}
}

#[test]
fn ps_scope_header_and_daemon_table_cells() {
	let scope = PsScope {
		kind: PsScopeKind::Project,
		runtime_dir: "/tmp/omp/run".to_string(),
		project_dir: Some("/work/app".to_string()),
		service: None,
		broker_pid: Some(42),
	};
	let report = PsScopeReport {
		scope: scope.clone(),
		daemons: vec![row("api", DaemonState::Ready), row("web", DaemonState::Running)],
	};
	let header = scope_header(&scope);
	assert!(header.contains("project"), "{header}");
	assert!(header.contains("/work/app"), "{header}");
	assert!(header.contains("42"), "{header}");

	let cells = table_cells(&report.daemons[0], 6000);
	assert_eq!(cells[0], "api");
	assert_eq!(cells[2], "100");
	assert_eq!(cells[6], "run api");
	assert!(state_cell(&report.daemons[0]).contains("ready"));
	assert!(state_cell(&report.daemons[1]).contains("running"));
}

#[test]
fn ps_terminal_state_has_no_uptime_or_pid() {
	let exited = PsDaemonRow {
		snapshot: {
			let mut snapshot = daemon("api", DaemonState::Exited, None);
			snapshot.exit_code = Some(143);
			snapshot
		},
		command: None,
		cwd: None,
		supervised: false,
	};
	assert!(state_cell(&exited).contains("exited(143)"));
	assert_eq!(uptime_cell(&exited.snapshot, 9999), "-");
	assert_eq!(table_cells(&exited, 9999)[2], "-");
}

#[test]
fn git_diff_layout_cycles_split_inline_hunk_file() {
	let mut model = GitTuiModel::new();
	assert_eq!(model.view_mode, ViewMode::Split);
	model.cycle_mode();
	assert_eq!(model.view_mode, ViewMode::Inline);
	model.cycle_mode();
	assert_eq!(model.view_mode, ViewMode::Hunk);
	model.cycle_mode();
	assert_eq!(model.view_mode, ViewMode::File);
	model.cycle_mode();
	assert_eq!(model.view_mode, ViewMode::Split);
	model.set_mode(ViewMode::Inline);
	assert_eq!(model.view_mode, ViewMode::Inline);
}

#[test]
fn git_footer_hints_omit_unavailable_actions_when_clean() {
	let dirty_diff = maple_tui::apps::git::git_tui::git_footer_hints(false, Focus::Diff);
	assert!(dirty_diff.contains("s/u stage"), "{dirty_diff}");
	assert!(dirty_diff.contains("x/del discard"), "{dirty_diff}");
	let clean_diff = maple_tui::apps::git::git_tui::git_footer_hints(true, Focus::Diff);
	assert!(!clean_diff.contains("stage"), "{clean_diff}");
	assert!(!clean_diff.contains("discard"), "{clean_diff}");
	assert!(!clean_diff.contains("commit"), "{clean_diff}");
	let sidebar = maple_tui::apps::git::git_tui::git_footer_hints(false, Focus::Sidebar);
	assert!(sidebar.contains("space stage"), "{sidebar}");
}

#[test]
fn git_suggested_file_prefers_unstaged_then_staged_then_head() {
	let state = view(vec![file("a.ts", ChangeArea::Unstaged)], vec![]);
	assert_eq!(suggested_file(&state).map(|f| f.path).as_deref(), Some("a.ts"));
	let state = view(vec![], vec![file("b.ts", ChangeArea::Staged)]);
	assert_eq!(suggested_file(&state).map(|f| f.path).as_deref(), Some("b.ts"));
	let state = GitViewState {
		clean: true,
		unstaged: vec![],
		staged: vec![],
		head_commit: Some(head_commit(vec![head_file("c.ts")])),
		..state
	};
	assert_eq!(suggested_file(&state).map(|f| f.path).as_deref(), Some("c.ts"));
}

#[test]
fn git_patch_target_follows_the_file_area() {
	assert_eq!(patch_target_for(Some(&file("a.ts", ChangeArea::Unstaged))).as_deref(), Some("stage"));
	assert_eq!(patch_target_for(Some(&file("b.ts", ChangeArea::Staged))).as_deref(), Some("unstage"));
	let untracked = ChangedFile {
		kind: ChangeKind::Untracked,
		..file("c.ts", ChangeArea::Unstaged)
	};
	assert_eq!(patch_target_for(Some(&untracked)), None);
	assert_eq!(patch_target_for(None), None);
}

#[test]
fn git_show_file_clears_pending_discard_and_sync_keeps_existing() {
	let mut model = GitTuiModel::new();
	let current = file("a.ts", ChangeArea::Unstaged);
	model.pending_discard = Some("a.ts".to_string());
	model.show_file(Some(current.clone()));
	assert_eq!(model.pending_discard, None);
	assert_eq!(model.current_file.as_ref(), Some(&current));

	let state = view(vec![current.clone(), file("b.ts", ChangeArea::Unstaged)], vec![]);
	assert_eq!(model.sync_current_file(&state), Some(current.clone()));
	let refreshed = view(vec![file("c.ts", ChangeArea::Unstaged)], vec![]);
	assert_eq!(model.sync_current_file(&refreshed).map(|f| f.path).as_deref(), Some("c.ts"));
}

#[test]
fn ps_top_selection_moves_and_clamps() {
	let mut model = PsTopModel::new();
	assert_eq!(model.selected(), 0);
	model.apply_action(PsTopAction::MoveDown, 2);
	assert_eq!(model.selected(), 1);
	model.apply_action(PsTopAction::MoveDown, 2);
	assert_eq!(model.selected(), 1, "selection stops at the last row");
	model.apply_action(PsTopAction::MoveUp, 2);
	assert_eq!(model.selected(), 0);
	model.clamp(1);
	assert_eq!(model.selected(), 0);
	model.clamp(0);
	assert_eq!(model.selected(), 0);
	assert_eq!(action_for_key("down"), PsTopAction::MoveDown);
	assert_eq!(action_for_key("q"), PsTopAction::Quit);
	assert_eq!(action_for_key("ctrl+k"), PsTopAction::KillSelected);
}

#[test]
fn ps_top_sort_running_first_then_runtime_descending() {
	let mut rows = vec![
		task("slow", JobStatus::Completed, 0, Some(9_000), &[]),
		task("old", JobStatus::Running, 0, None, &[]),
		task("new", JobStatus::Running, 8_000, None, &[]),
	];
	sort_rows(&mut rows, 10_000);
	let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
	assert_eq!(ids, vec!["old", "new", "slow"]);
}

#[test]
fn ps_top_render_marks_the_selected_row_and_previews_its_output() {
	let mut model = PsTopModel::new();
	model.apply_action(PsTopAction::MoveDown, 2);
	let rows = model.render_lines(
		vec![
			task("a", JobStatus::Completed, 0, Some(1_000), &[]),
			task("b", JobStatus::Running, 2_000, None, &["one", "two", "three", "four"]),
		],
		3_000,
		120,
	);
	let marked = rows.iter().filter(|line| line.starts_with('>')).count();
	assert_eq!(marked, 1, "rows: {rows:?}");
	assert!(rows.iter().any(|line| line.contains("label b")), "rows: {rows:?}");
	assert!(rows.iter().any(|line| line.contains("two")), "rows: {rows:?}");
	assert!(rows.iter().any(|line| line.contains("four")), "rows: {rows:?}");
	assert!(!rows.iter().any(|line| line.contains("one")), "only the last three preview lines");
}

#[test]
fn cleanse_board_keeps_running_lanes_and_meters_progress() {
	let mut board = CleanseStatusBoard::new(&["CleanseA".to_string(), "CleanseB".to_string()], true);
	board.on_wave(1, 2);
	board.on_start("CleanseA", "fix a");
	board.on_start("CleanseB", "fix b");
	board.on_done("CleanseA", true, 500, None);
	let lines = board.render();
	let text = lines.join("\n");
	assert!(text.contains("wave 1  active 2"), "{text}");
	assert!(text.contains("CleanseA"), "{text}");
	assert!(text.contains("✓"), "{text}");
	assert!(text.contains("CleanseB"), "{text}");
	assert!(!board.is_closed());
	board.close();
	assert!(board.is_closed());
}

#[test]
fn cleanse_non_interactive_log_and_final_table() {
	let mut board = CleanseStatusBoard::new(&["A".to_string()], false);
	board.on_start("A", "cmd");
	board.on_log("A", "  spaced   message ");
	board.on_done("A", false, 1_200, Some("boom"));
	let log = board.log_lines().join("\n");
	assert!(log.contains("RUN  A  cmd"), "{log}");
	assert!(log.contains("A  spaced message"), "{log}");
	assert!(log.contains("FAIL A 1200ms  boom"), "{log}");

	let table = format_cleanse_final_table(&[CleanseFinalEntry {
		label: "A".to_string(),
		ran: true,
		ok: false,
		duration_ms: 1_200,
		error: Some("boom".to_string()),
	}]);
	assert!(table.contains("A"), "{table}");
	assert!(table.contains("boom"), "{table}");
}
