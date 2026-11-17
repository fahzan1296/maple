//! Port of `l/test/status-line-path.test.ts`.
//!
//! The reference drives `process.cwd()`/`setProjectDir` and real symlinks; the
//! crate takes `project_dir`/`home_dir` on the context, so the display rules are
//! ported directly. The three Windows 8.3/junction cases stay ignored.

use maple_tui::status_line::schema::{PathOptions, SegmentId};
use maple_tui::status_line::segments::{
	render_segment_with, ActiveRepo, SegmentContext, WorktreeContext,
};
use maple_tui::status_line::{FallbackTheme, StatusTheme};

fn theme() -> FallbackTheme {
	FallbackTheme::new()
}

/// Path content is wrapped in the `statusLinePath` fg and a trailing reset.
fn plain(content: &str) -> &str {
	let ansi = theme().fg_ansi("statusLinePath");
	content
		.strip_prefix(&ansi)
		.and_then(|rest| rest.strip_suffix("\x1b[39m"))
		.unwrap_or(content)
}

fn ctx(project_dir: &str, home: &str) -> SegmentContext {
	let mut ctx = SegmentContext::default();
	ctx.project_dir = project_dir.to_string();
	ctx.home_dir = home.to_string();
	ctx.options.path = Some(PathOptions {
		abbreviate: Some(false),
		max_length: Some(120),
		strip_work_prefix: Some(true),
	});
	ctx
}

fn render(ctx: &SegmentContext) -> String {
	render_segment_with(SegmentId::Path, &theme(), ctx).content
}

fn with_max_length(mut ctx: SegmentContext, max_length: usize) -> SegmentContext {
	ctx.options.path = Some(PathOptions {
		max_length: Some(max_length),
		..ctx.options.path.clone().unwrap_or_default()
	});
	ctx
}

fn with_strip(mut ctx: SegmentContext, strip: bool) -> SegmentContext {
	ctx.options.path = Some(PathOptions {
		strip_work_prefix: Some(strip),
		..ctx.options.path.clone().unwrap_or_default()
	});
	ctx
}

fn worktree_ctx(worktree_name: &str, branch: Option<&str>) -> SegmentContext {
	let mut ctx = ctx("/tmp/whatever", "/home/user");
	ctx.worktree = Some(WorktreeContext {
		project_name: "pi".to_string(),
		worktree_name: worktree_name.to_string(),
	});
	ctx.git.branch = branch.map(str::to_string);
	ctx
}

#[test]
fn strips_the_scratch_root_and_shows_only_the_trailing_folder() {
	let dir = "/tmp/omp-status-line-scratch-x";
	let content = render(&ctx(dir, "/home/user"));
	assert!(content.contains('🗑'), "{content:?}");
	assert!(!content.contains('📁'), "{content:?}");
	assert!(content.contains("omp-status-line-scratch-x"), "{content:?}");
	assert!(!content.contains("/tmp"), "{content:?}");
}

#[test]
fn keeps_nested_subpaths_visible_under_a_scratch_root() {
	let dir = "/tmp/omp-status-line-scratch-nest/sub/deep";
	let content = render(&ctx(dir, "/home/user"));
	assert!(content.contains('🗑'), "{content:?}");
	assert!(content.contains("omp-status-line-scratch-nest/sub/deep"), "{content:?}");
	assert!(!content.contains("/tmp"), "{content:?}");
}

#[test]
fn keeps_the_folder_icon_for_scratch_paths_when_strip_work_prefix_is_disabled() {
	let dir = "/tmp/omp-status-line-scratch-noprefix";
	let content = render(&with_strip(ctx(dir, "/home/user"), false));
	assert!(content.contains('📁'), "{content:?}");
	assert!(!content.contains('🗑'), "{content:?}");
}

#[test]
fn keeps_the_folder_icon_for_paths_outside_any_scratch_root() {
	let content = render(&ctx("/home/user/Projects/omp-status-line-real", "/home/user"));
	assert!(content.contains('📁'), "{content:?}");
	assert!(!content.contains('🗑'), "{content:?}");
}

#[test]
fn strips_the_projects_root_for_symlink_equivalent_aliases() {
	let content = render(&ctx("/home/user/Projects/omp-status-line/nested", "/home/user"));
	assert!(content.contains("omp-status-line/nested"), "{content:?}");
	assert!(!content.contains("/Projects/"), "{content:?}");
}

#[test]
fn uses_the_current_home_when_switching_project_directories() {
	let mut ctx = ctx("/home/first/Projects/first-project", "/home/first");
	let first = render(&ctx);
	assert!(first.contains("first-project"), "{first:?}");

	ctx.project_dir = "/home/second/Projects/second-project".to_string();
	ctx.home_dir = "/home/second".to_string();
	let second = render(&ctx);
	assert!(second.contains("second-project"), "{second:?}");
	assert!(!second.contains("first-project"), "{second:?}");
}

#[test]
fn renders_the_active_nested_repo_suffix_after_the_parent_cwd() {
	let mut ctx = ctx("/tmp/omp-status-line-parent", "/home/user");
	ctx.active_repo = Some(ActiveRepo {
		cwd: "/tmp/omp-status-line-parent".to_string(),
		relative_repo_root: "pr-workspace".to_string(),
	});
	let content = render(&ctx);
	assert!(content.contains("omp-status-line-parent ↳ pr-workspace"), "{content:?}");
	assert!(!content.contains("/tmp"), "{content:?}");
}

#[test]
fn keeps_the_active_nested_repo_suffix_visible_when_the_parent_path_is_truncated() {
	let mut ctx = with_max_length(ctx("/tmp/omp-status-line-parent", "/home/user"), 4);
	ctx.active_repo = Some(ActiveRepo {
		cwd: "/tmp/omp-status-line-parent".to_string(),
		relative_repo_root: "pr-workspace".to_string(),
	});
	let content = render(&ctx);
	assert!(content.contains("↳ pr-workspace"), "{content:?}");
}

#[test]
fn collapses_to_the_project_name_when_the_worktree_dir_equals_the_branch() {
	let content = render(&worktree_ctx("xx", Some("xx")));
	assert_eq!(plain(&content), "🌳 pi");
}

#[test]
fn keeps_the_worktree_dir_when_it_diverges_from_the_branch() {
	let content = render(&worktree_ctx("wt-icon", Some("icon")));
	assert_eq!(plain(&content), "🌳 pi/wt-icon");
}

#[test]
fn keeps_the_worktree_dir_when_no_branch_is_shown() {
	let content = render(&worktree_ctx("xx", None));
	assert_eq!(plain(&content), "🌳 pi/xx");
}

#[test]
fn falls_back_to_the_on_disk_path_when_strip_work_prefix_is_disabled() {
	let mut ctx = with_strip(worktree_ctx("xx", Some("xx")), false);
	ctx.project_dir = "/tmp/omp-status-line-wt-noprefix".to_string();
	let content = render(&ctx);
	assert!(!content.contains('🌳'), "{content:?}");
	assert!(content.contains('📁'), "{content:?}");
}

#[test]
fn clamps_a_long_worktree_label_to_max_length() {
	let mut ctx = with_max_length(worktree_ctx("feature", Some("other")), 10);
	ctx.worktree = Some(WorktreeContext {
		project_name: "very-long-project-name".to_string(),
		worktree_name: "feature".to_string(),
	});
	let content = render(&ctx);
	let label = plain(&content).strip_prefix("🌳 ").expect("worktree icon prefix");
	assert!(label.chars().count() <= 10, "{label:?}");
	assert!(label.starts_with('…'), "{label:?}");
	assert!(label.ends_with("feature"), "{label:?}");
}

#[test]
#[ignore = "Windows-only reference case: 8.3 alias expansion"]
fn renders_the_long_cwd_when_launched_through_an_existing_8_3_alias() {}

#[test]
#[ignore = "Windows-only reference case: raw 8.3 repository path abbreviation"]
fn abbreviates_a_raw_8_3_repository_path_only_when_enabled() {}

#[test]
#[ignore = "Windows-only reference case: junction spelling in the displayed cwd"]
fn keeps_junction_spelling_in_the_displayed_cwd() {}
