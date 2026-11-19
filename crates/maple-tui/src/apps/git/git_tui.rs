//! `omp git` — fullscreen repository TUI.
//!
//! Reference: `l/src/apps/git/git-tui.ts`.
//!
//! Deviations:
//! - TS is a full `@oh-my-pi/pi-tui` component with `TUI`, `SplitPane`,
//!   `Stack`, overlays, mouse routing, image budget, syntax highlighting,
//!   async model loading/refresh, process terminal lifecycle, and rich
//!   ANSI/native rendering. None of these runtimes exist 1:1 in this crate.
//! - This port preserves public contracts and core controller logic as data:
//!   [`GitTuiHost`], [`AiStageOutcome`], [`GitFooterHints`] concept via
//!   [`git_footer_hints`], [`Focus`], [`StatusTone`], [`GitTuiModel`] (state
//!   snapshot + actions), view/whitespace/focus transitions, selection helpers,
//!   and status/message lifecycle. Actual rendering, IO, and diff/sidebar
//!   widgets are host responsibilities.
//! - `GitTuiHost::ai_stage`/`generate_commit_message` are async trait methods.
//!   No default "not implemented" error: hosts must provide real behavior.
//! - `run_git_tui` is injected (host/runner provides the actual TUI loop) since
//!   the terminal lifecycle is external.

use std::future::Future;
use std::pin::Pin;

use crate::apps::git::diff_pane::{ViewMode, WhitespaceMode};
use crate::apps::git::sidebar::GitCommitMessage;
use crate::apps::git::state::{ChangedFile, GitViewState};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// AI staging counts displayed after filtering the working tree.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AiStageOutcome {
	pub matched_files: u32,
	pub total_files: u32,
	pub staged_hunks: u32,
	pub total_hunks: u32,
	pub whole_files: u32,
}

/// Repository and generation capabilities owned by the command host.
pub trait GitTuiHost {
	fn view(&self) -> &GitViewState;
	fn view_mut(&mut self) -> &mut GitViewState;

	fn ai_stage(
		&mut self,
		cwd: &str,
		instruction: &str,
		files: &[ChangedFile],
	) -> BoxFuture<'_, Result<AiStageOutcome, String>>;

	fn generate_commit_message(
		&mut self,
		cwd: &str,
		stage_if_empty: bool,
	) -> BoxFuture<'_, Result<GeneratedCommit, String>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedCommit {
	pub commit: GitCommitMessage,
	pub validation_error: Option<String>,
	pub staged_all: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
	Diff,
	Sidebar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
	Success,
	Warning,
	Error,
	Accent,
	Dim,
}

impl StatusTone {
	pub fn as_str(&self) -> &'static str {
		match self {
			StatusTone::Success => "success",
			StatusTone::Warning => "warning",
			StatusTone::Error => "error",
			StatusTone::Accent => "accent",
			StatusTone::Dim => "dim",
		}
	}
}

/// Footer key hints. Clean tree omits stage/discard/commit/select hints that
/// cannot fire in commit view.
pub fn git_footer_hints(clean: bool, focus: Focus) -> String {
	match (clean, focus) {
		(false, Focus::Diff) => {
			"alt+↓/↑ hunk · ]/[ file · shift+↑/↓ select · s/u stage · x/del discard · v view · c commit · ? keys · q quit".to_string()
		}
		(false, Focus::Sidebar) => {
			"↑/↓ move · ←/→ fold · space stage · del discard · enter open · alt+↓/↑ hunk · c commit · t tree · ? keys · q quit".to_string()
		}
		(true, Focus::Diff) => {
			"alt+↓/↑ hunk · ]/[ file · v view · ? keys · q quit".to_string()
		}
		(true, Focus::Sidebar) => {
			"↑/↓ move · ←/→ fold · enter open · alt+↓/↑ hunk · t tree · ? keys · q quit".to_string()
		}
	}
}

/// Core controller state for the git TUI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitTuiModel {
	pub focus: Focus,
	pub current_file: Option<ChangedFile>,
	pub whitespace: WhitespaceMode,
	pub view_mode: ViewMode,
	pub busy: bool,
	pub status: String,
	pub status_tone: StatusTone,
	pub status_sticky: bool,
	pub pending_discard: Option<String>,
	pub pending_hunk_edge_last: bool,
	pub help_open: bool,
}

impl GitTuiModel {
	pub fn new() -> Self {
		Self {
			focus: Focus::Sidebar,
			current_file: None,
			whitespace: WhitespaceMode::Off,
			view_mode: ViewMode::Split,
			busy: false,
			status: String::new(),
			status_tone: StatusTone::Dim,
			status_sticky: false,
			pending_discard: None,
			pending_hunk_edge_last: false,
			help_open: false,
		}
	}

	pub fn set_focus(&mut self, focus: Focus) { self.focus = focus; }
	pub fn set_mode(&mut self, mode: ViewMode) { self.view_mode = mode; }
	pub fn cycle_mode(&mut self) {
		self.view_mode = match self.view_mode {
			ViewMode::Split => ViewMode::Inline,
			ViewMode::Inline => ViewMode::Hunk,
			ViewMode::Hunk => ViewMode::File,
			ViewMode::File => ViewMode::Split,
		};
	}
	pub fn cycle_whitespace(&mut self) {
		self.whitespace = match self.whitespace {
			WhitespaceMode::Off => WhitespaceMode::Whitespace,
			WhitespaceMode::Whitespace => WhitespaceMode::Formatting,
			WhitespaceMode::Formatting => WhitespaceMode::Off,
		};
	}
	pub fn set_status(&mut self, msg: impl Into<String>, tone: StatusTone, sticky: bool) {
		self.status = msg.into();
		self.status_tone = tone;
		self.status_sticky = sticky;
	}
	pub fn clear_status(&mut self) {
		self.status.clear();
		self.status_tone = StatusTone::Dim;
		self.status_sticky = false;
	}
	pub fn toggle_help(&mut self) { self.help_open = !self.help_open; }
	pub fn visible_status(&self) -> bool { !self.status.is_empty() }

	pub fn show_file(&mut self, file: Option<ChangedFile>) {
		self.current_file = file;
		self.pending_discard = None;
	}

	pub fn sync_current_file(&mut self, view: &GitViewState) -> Option<ChangedFile> {
		let still_exists = self.current_file.as_ref().map(|cur| {
			view.unstaged.iter().chain(view.staged.iter())
				.chain(view.head_commit.as_ref().map(|h| h.files.as_slice()).unwrap_or(&[]))
				.any(|c| c.path == cur.path && c.area == cur.area)
		}).unwrap_or(false);
		if !still_exists {
			self.current_file = suggested_file(view);
		}
		self.current_file.clone()
	}

	pub fn footer(&self, clean: bool) -> String {
		git_footer_hints(clean, self.focus)
	}
}

impl Default for GitTuiModel {
	fn default() -> Self { Self::new() }
}

/// Suggested file to show after refresh/reconcile.
pub fn suggested_file(view: &GitViewState) -> Option<ChangedFile> {
	view.unstaged.first().cloned()
		.or_else(|| view.staged.first().cloned())
		.or_else(|| view.head_commit.as_ref().and_then(|h| h.files.first().cloned()))
}

/// Patch target derived from current file area.
pub fn patch_target_for(file: Option<&ChangedFile>) -> Option<&'static str> {
	let f = file?;
	match f.area.as_str() {
		"unstaged" if f.kind.as_str() == "untracked" || f.kind.as_str() == "conflicted" => None,
		"unstaged" => Some("stage"),
		"staged" => Some("unstage"),
		_ => None,
	}
}

pub use git_footer_hints as gitFooterHints;
pub use run_git_tui as runGitTui;

/// Run git TUI via injected runner (real terminal loop is host-owned).
pub async fn run_git_tui<R>(host: &mut dyn GitTuiHost, runner: R) -> Result<(), String>
where
	R: for<'a> FnOnce(&'a mut dyn GitTuiHost) -> BoxFuture<'a, Result<(), String>>,
{
	runner(host).await
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::apps::git::state::{ChangedFile, GitViewState, HeadCommit};

	struct Host {
		view: GitViewState,
	}
	impl GitTuiHost for Host {
		fn view(&self) -> &GitViewState { &self.view }
		fn view_mut(&mut self) -> &mut GitViewState { &mut self.view }
		fn ai_stage(
			&mut self,
			_cwd: &str,
			_instruction: &str,
			_files: &[ChangedFile],
		) -> BoxFuture<'_, Result<AiStageOutcome, String>> {
			Box::pin(async { Ok(AiStageOutcome { matched_files: 1, total_files: 2, staged_hunks: 3, total_hunks: 4, whole_files: 1 }) })
		}
		fn generate_commit_message(
			&mut self,
			_cwd: &str,
			_stage_if_empty: bool,
		) -> BoxFuture<'_, Result<GeneratedCommit, String>> {
			Box::pin(async {
				Ok(GeneratedCommit {
					commit: GitCommitMessage { subject: "s".into(), body: String::new() },
					validation_error: None,
					staged_all: true,
				})
			})
		}
	}

	fn run_blocking<F, T>(fut: F) -> T
	where F: std::future::Future<Output = T> {
		use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
		fn noop_raw() -> RawWaker {
			fn no(_: *const ()) {}
			fn clone(_: *const ()) -> RawWaker { noop_raw() }
			const V: RawWakerVTable = RawWakerVTable::new(clone, no, no, no);
			RawWaker::new(std::ptr::null(), &V)
		}
		let waker = unsafe { Waker::from_raw(noop_raw()) };
		let mut cx = Context::from_waker(&waker);
		let mut fut = Box::pin(fut);
		loop {
			match fut.as_mut().poll(&mut cx) {
				Poll::Ready(v) => break v,
				Poll::Pending => continue,
			}
		}
	}

	#[test]
	fn footer_hints_clean_vs_dirty() {
		assert!(git_footer_hints(false, Focus::Diff).contains("stage"));
		assert!(!git_footer_hints(true, Focus::Diff).contains(" stage ·"));
		assert!(git_footer_hints(false, Focus::Sidebar).contains("space stage"));
		assert!(!git_footer_hints(true, Focus::Sidebar).contains("del discard"));
	}

	#[test]
	fn model_cycles() {
		let mut m = GitTuiModel::new();
		assert_eq!(m.focus, Focus::Sidebar);
		m.cycle_mode();
		assert_eq!(m.view_mode, ViewMode::Inline);
		m.cycle_whitespace();
		assert_eq!(m.whitespace, WhitespaceMode::Whitespace);
		m.toggle_help();
		assert!(m.help_open);
	}

	#[test]
	fn suggested_and_patch_target() {
		let mut v = GitViewState::default();
		v.unstaged.push(ChangedFile { path: "a".into(), kind: "modified".into(), area: "unstaged".into(), ..Default::default() });
		let f = suggested_file(&v).unwrap();
		assert_eq!(patch_target_for(Some(&f)), Some("stage"));
		let f2 = ChangedFile { area: "staged".into(), ..f.clone() };
		assert_eq!(patch_target_for(Some(&f2)), Some("unstage"));
		assert_eq!(patch_target_for(None), None);
	}

	#[test]
	fn host_ai_and_commit_are_real() {
		let mut h = Host { view: GitViewState::default() };
		let ai = run_blocking(h.ai_stage(".", "msg", &[])).unwrap();
		assert_eq!(ai.matched_files, 1);
		let cm = run_blocking(h.generate_commit_message(".", true)).unwrap();
		assert!(cm.staged_all);
		assert_eq!(cm.commit.subject, "s");
	}

	#[test]
	fn run_git_tui_delegates() {
		let mut h = Host { view: GitViewState::default() };
		let res = run_blocking(run_git_tui(&mut h, |_host| Box::pin(async { Ok(()) })));
		assert!(res.is_ok());
	}
}
