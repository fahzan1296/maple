//! Port of `l/test/grouped-file-output-render.test.ts`.
//!
//! The crate ports the prefix-folded tree (`format_grouped_files`), per-line
//! path reconstruction (`classify_grouped_lines`), blank grouping, and the
//! flat/simple assemblers used by the other search tools.

use maple_tui::theme::default_theme;
use maple_tui::tools::grouped_file_output::{
	classify_grouped_lines, classify_grouped_lines_with_scope, format_file_matches_flat,
	format_grouped_files, format_grouped_files_simple, group_line_indices_by_blank,
	is_grouped_header, is_match_row, render_grouped_output_themed, FileMatchSection,
	GroupedFileSection, GroupedLineKind,
};

#[test]
fn classifies_match_rows_and_headers() {
	assert!(is_match_row("*12│const y = 1;"));
	assert!(is_match_row("*12:const y = 1;"));
	assert!(is_match_row("  *7|needle();"));
	assert!(!is_match_row("  body"));
	assert!(!is_match_row("*-│no digits"));
	assert!(!is_match_row("# a.ts"));

	assert!(is_grouped_header("# pkg/ai/"));
	assert!(is_grouped_header("## CHANGELOG.md"));
	assert!(is_grouped_header("# /"));
	assert!(!is_grouped_header("  body"));
	assert!(!is_grouped_header("#pkg"));
}

#[test]
fn simple_grouping_emits_one_header_per_file_and_omits_skipped_ones() {
	let files = vec!["a/keep.ts".to_string(), "a/drop.ts".to_string()];
	let out = format_grouped_files_simple(&files, |file| GroupedFileSection {
		header_suffix: Some(" (1)".to_string()),
		model_lines: vec![format!("  {file}")],
		display_lines: None,
		skip: file.ends_with("drop.ts"),
	});
	assert_eq!(out.model, ["# a/keep.ts (1)", "  a/keep.ts"]);
	assert_eq!(out.display, ["# a/keep.ts (1)", "  a/keep.ts"]);
	assert_eq!(out.model_ends.get("a/keep.ts"), Some(&2));
	assert!(!out.model_ends.contains_key("a/drop.ts"));
}

#[test]
fn simple_grouping_blank_separates_top_groups() {
	let files = vec!["pkg/ai/CHANGELOG.md".to_string(), "README.md".to_string()];
	let out = format_grouped_files_simple(&files, |file| GroupedFileSection {
		header_suffix: Some(" (1)".to_string()),
		model_lines: vec![format!("  {file}")],
		..GroupedFileSection::default()
	});
	assert_eq!(
		out.model,
		[
			"# pkg/ai/CHANGELOG.md (1)",
			"  pkg/ai/CHANGELOG.md",
			"",
			"# README.md (1)",
			"  README.md",
		]
	);
	assert_eq!(out.model_ends.get("pkg/ai/CHANGELOG.md"), Some(&2));
	assert_eq!(out.model_ends.get("README.md"), Some(&5));
}

#[test]
fn flat_matches_keep_the_hashline_header_and_fall_back_to_model_lines() {
	let files = vec!["a.ts".to_string(), "b.ts".to_string()];
	let out = format_file_matches_flat(
		&files,
		|file| FileMatchSection {
			model: vec![format!("*1│{file}")],
			display: vec![],
			tag: file.ends_with("a.ts").then(|| "1A2B".to_string()),
		},
		|file, tag| format!("#{tag} {file}"),
	);
	assert_eq!(out.model, ["#1A2B a.ts", "*1│a.ts", "", "*1│b.ts"]);
	assert_eq!(out.display, ["#1A2B a.ts", "*1│a.ts", "", "*1│b.ts"]);
	assert_eq!(out.model_ends.get("a.ts"), Some(&2));
	assert_eq!(out.model_ends.get("b.ts"), Some(&4));
}

#[test]
fn themed_render_paints_headers_and_keeps_match_bodies() {
	let files = vec!["a.ts".to_string()];
	let out = format_grouped_files_simple(&files, |file| GroupedFileSection {
		model_lines: vec!["*1│needle".to_string()],
		..GroupedFileSection::default()
	});
	let theme = default_theme();
	let themed = render_grouped_output_themed(&out, &theme);
	assert_eq!(themed.len(), out.display.len());
	assert!(themed[0].contains("# a.ts"), "{themed:?}");
	assert!(themed[1].contains("needle"), "{themed:?}");
}

#[test]
fn nests_subdirectories_with_deeper_headings() {
	let files = vec![
		"pkg/ai/CHANGELOG.md".to_string(),
		"pkg/ai/src/util/x.ts".to_string(),
		"README.md".to_string(),
	];
	let out = format_grouped_files(&files, |file| GroupedFileSection {
		model_lines: vec![format!("  {file}")],
		header_suffix: Some(" (1)".to_string()),
		..GroupedFileSection::default()
	});
	assert_eq!(
		out.model,
		[
			"# README.md (1)",
			"  README.md",
			"",
			"# pkg/ai/",
			"## CHANGELOG.md (1)",
			"  pkg/ai/CHANGELOG.md",
			"",
			"## src/util/",
			"### x.ts (1)",
			"  pkg/ai/src/util/x.ts",
		]
	);
}

#[test]
fn omits_skipped_files_and_now_empty_directories_from_the_tree() {
	let files = vec!["a/keep.ts".to_string(), "a/drop.ts".to_string()];
	let out = format_grouped_files(&files, |file| GroupedFileSection {
		model_lines: vec![format!("  {file}")],
		skip: file.ends_with("drop.ts"),
		..GroupedFileSection::default()
	});
	assert_eq!(out.model, ["# a/", "## keep.ts", "  a/keep.ts"]);
}

#[test]
fn reconstructs_absolute_and_display_paths_across_a_nested_stack() {
	let lines = vec!["# pkg/ai/", "## CHANGELOG.md", "  match", "## src/util/", "### x.ts", "*12│const y = 1;"];
	let ctx = classify_grouped_lines(&lines, Some("/repo"));
	assert_eq!(ctx[0].kind, Some(GroupedLineKind::Dir));
	assert_eq!(ctx[1].header_path.as_deref(), Some("/repo/pkg/ai/CHANGELOG.md"));
	assert_eq!(ctx[1].display_path.as_deref(), Some("pkg/ai/CHANGELOG.md"));
	assert_eq!(ctx[3].kind, Some(GroupedLineKind::Dir));
	assert_eq!(ctx[4].header_path.as_deref(), Some("/repo/pkg/ai/src/util/x.ts"));
	assert_eq!(ctx[4].display_path.as_deref(), Some("pkg/ai/src/util/x.ts"));
}

#[test]
fn keeps_an_absolute_folded_prefix() {
	let ctx = classify_grouped_lines(&["# /outside/dir/", "## file.txt"], Some("/repo"));
	assert_eq!(ctx[1].header_path.as_deref(), Some("/outside/dir/file.txt"));
}

#[test]
fn keeps_a_directorys_parenthesized_part_in_its_label() {
	let ctx = classify_grouped_lines(&["# src (old)/", "## file.ts (2 replacements)"], Some("/repo"));
	assert_eq!(ctx[1].display_path.as_deref(), Some("src (old)/file.ts"));
	assert_eq!(ctx[1].suffix.as_deref(), Some("(2 replacements)"));
}

#[test]
fn keeps_the_leading_slash_of_absolute_paths() {
	let ctx = classify_grouped_lines(&["# /", "## z.ts", "", "## tmp/a/", "### x.ts"], Some("/repo"));
	assert_eq!(ctx[1].display_path.as_deref(), Some("/z.ts"));
	assert_eq!(ctx[4].display_path.as_deref(), Some("/tmp/a/x.ts"));
}

#[test]
fn drops_only_a_hashline_tag_from_a_file_name() {
	let ctx = classify_grouped_lines(
		&["# a.ts#1A2B (1 replacement)", "# component.ts#dead"],
		Some("/repo"),
	);
	assert_eq!(ctx[0].display_path.as_deref(), Some("a.ts"));
	assert_eq!(ctx[0].suffix.as_deref(), Some("(1 replacement)"));
	assert_eq!(ctx[1].display_path.as_deref(), Some("component.ts#dead"));
}

#[test]
fn links_body_lines_before_any_header_to_the_search_base() {
	let ctx = classify_grouped_lines(&["*7│needle();"], Some("/repo/file.ts"));
	assert_eq!(ctx[0].file_path.as_deref(), Some("/repo/file.ts"));
}

#[test]
fn leaves_url_like_headers_to_caller_side_resolution() {
	let ctx = classify_grouped_lines(&["# omp://docs/", "  body"], Some("/repo"));
	assert_eq!(ctx[0].kind, Some(GroupedLineKind::File));
	assert_eq!(ctx[0].url.as_deref(), Some("omp://docs/"));
	assert_eq!(ctx[0].header_path, None);
	assert_eq!(ctx[1].url.as_deref(), Some("omp://docs/"));
}

#[test]
fn gives_a_headerless_url_scopes_lines_its_url() {
	let ctx = classify_grouped_lines_with_scope(&["*7│needle();"], Some("/repo"), Some("local://notes/a.ts"));
	assert_eq!(ctx[0].url.as_deref(), Some("local://notes/a.ts"));
	assert_eq!(ctx[0].file_path, None);
}

#[test]
fn breaks_on_blank_line_runs_and_keeps_indices() {
	assert_eq!(group_line_indices_by_blank(&["a", "b", "", "", "c"]), vec![vec![0, 1], vec![4]]);
	assert_eq!(group_line_indices_by_blank(&["", "a", "", ""]), vec![vec![1]]);
}
