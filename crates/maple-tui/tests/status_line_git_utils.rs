//! Port of `l/test/status-line-git-utils.test.ts`.

use maple_tui::status_line::git_utils::{
	can_reuse_cached_pr, create_pr_cache_context, is_same_pr_cache_context, parse_default_branch,
	parse_github_repo, CachedPr,
};
use maple_tui::status_line::segments::PullRequest;

#[test]
fn parses_https_url() {
	assert_eq!(
		parse_github_repo("https://github.com/can1357/oh-my-pi.git"),
		Some("can1357/oh-my-pi".to_string())
	);
}

#[test]
fn parses_https_url_without_git_suffix() {
	assert_eq!(
		parse_github_repo("https://github.com/can1357/oh-my-pi"),
		Some("can1357/oh-my-pi".to_string())
	);
}

#[test]
fn parses_ssh_scp_style_url() {
	assert_eq!(
		parse_github_repo("git@github.com:loftiskg/oh-my-pi.git"),
		Some("loftiskg/oh-my-pi".to_string())
	);
}

#[test]
fn parses_ssh_protocol_url() {
	assert_eq!(
		parse_github_repo("ssh://git@github.com/user/repo.git"),
		Some("user/repo".to_string())
	);
}

#[test]
fn returns_none_for_non_github_url() {
	assert_eq!(parse_github_repo("https://gitlab.com/user/repo.git"), None);
}

#[test]
fn handles_github_enterprise_style_urls() {
	assert_eq!(parse_github_repo("https://github.corp.com/org/repo.git"), None);
}

#[test]
fn parses_https_url_with_dots_in_repo_name() {
	assert_eq!(
		parse_github_repo("https://github.com/org/my.repo.name.git"),
		Some("org/my.repo.name".to_string())
	);
}

#[test]
fn parses_url_with_dots_in_repo_name_and_no_git_suffix() {
	assert_eq!(
		parse_github_repo("https://github.com/org/my.repo"),
		Some("org/my.repo".to_string())
	);
}

#[test]
fn strips_origin_prefix_from_origin_main() {
	assert_eq!(parse_default_branch("origin/main"), "main");
}

#[test]
fn strips_upstream_prefix() {
	assert_eq!(parse_default_branch("upstream/main"), "main");
}

#[test]
fn handles_empty_default_branch() {
	assert_eq!(parse_default_branch(""), "");
}

#[test]
fn same_pr_cache_context_matches_branch_and_repo() {
	let a = create_pr_cache_context("feature/one", Some("/repo/.git/HEAD"));
	let b = create_pr_cache_context("feature/one", Some("/repo/.git/HEAD"));
	assert!(is_same_pr_cache_context(Some(&a), Some(&b)));
}

#[test]
fn pr_cache_context_differs_when_repo_changes() {
	let a = create_pr_cache_context("feature/one", Some("/repo-a/.git/HEAD"));
	let b = create_pr_cache_context("feature/one", Some("/repo-b/.git/HEAD"));
	assert!(!is_same_pr_cache_context(Some(&a), Some(&b)));
}

#[test]
fn allows_negative_cache_reuse_when_context_is_unchanged() {
	let context = create_pr_cache_context("feature/one", Some("/repo/.git/HEAD"));
	assert!(can_reuse_cached_pr(
		CachedPr::Null,
		Some(&context),
		Some(&context)
	));
}

#[test]
fn rejects_cached_pr_when_branch_changes() {
	let pr = PullRequest {
		number: 12,
		url: "https://example.test/pr/12".to_string(),
	};
	let cached = create_pr_cache_context("feature/one", Some("/repo/.git/HEAD"));
	let current = create_pr_cache_context("feature/two", Some("/repo/.git/HEAD"));
	assert!(!can_reuse_cached_pr(CachedPr::Value(&pr), Some(&cached), Some(&current)));
}

#[test]
fn rejects_cached_pr_when_repo_context_is_unavailable() {
	let pr = PullRequest {
		number: 12,
		url: "https://example.test/pr/12".to_string(),
	};
	let cached = create_pr_cache_context("feature/one", Some("/repo/.git/HEAD"));
	assert!(!can_reuse_cached_pr(CachedPr::Value(&pr), Some(&cached), None));
}
