//! Git remote and pull-request cache helpers.
//!
//! Reference: `l/src/status-line/git-utils.ts`.
//!
//! Deviation: the reference's `cachedPr` argument is
//! `{ number, url } | null | undefined`, and `canReuseCachedPr` tests
//! `!== undefined` — so an explicit `null` still counts as "cached". Rust has
//! no separate `undefined`, so the tri-state is kept explicitly as
//! [`CachedPr`] instead of collapsing `null` into `None`.

use crate::status_line::segments::PullRequest;
use regex::Regex;
use std::sync::LazyLock;

static GITHUB_REMOTE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"github\.com[:/]([^/]+/[^/]+)").expect("valid GitHub remote pattern"));

/// Extract `owner/repo` from a GitHub remote URL.
///
/// Handles HTTPS, SSH (scp-style), and `git://` URLs. Returns `None` when the
/// URL is not a recognized GitHub remote.
pub fn parse_github_repo(remote_url: &str) -> Option<String> {
    let captured = GITHUB_REMOTE_PATTERN
        .captures(remote_url)?
        .get(1)?
        .as_str();
    Some(captured.strip_suffix(".git").unwrap_or(captured).to_string())
}

/// Extract the branch name from a remote HEAD ref like `origin/main`.
///
/// Returns the portion after the first `/`, or the whole string when it has no
/// `/`.
pub fn parse_default_branch(ref_name: &str) -> String {
    match ref_name.find('/') {
        Some(slash) => ref_name[slash + 1..].to_string(),
        None => ref_name.to_string(),
    }
}

/// The branch and repository a cached pull request belongs to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrCacheContext {
    /// Branch the cache entry was resolved on.
    pub branch: String,
    /// `owner/repo` of the repository, when known.
    pub repo_id: Option<String>,
}

/// Build a cache context from a branch and repository id.
pub fn create_pr_cache_context(branch: &str, repo_id: Option<&str>) -> PrCacheContext {
    PrCacheContext {
        branch: branch.to_string(),
        repo_id: repo_id.map(str::to_string),
    }
}

/// Whether two contexts name the same branch and repository.
///
/// `None` on either side never matches.
pub fn is_same_pr_cache_context(
    a: Option<&PrCacheContext>,
    b: Option<&PrCacheContext>,
) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.branch == b.branch && a.repo_id == b.repo_id,
        _ => false,
    }
}

/// Tri-state `cachedPr` argument: absent (`undefined`), explicit `null`, or a
/// cached value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CachedPr<T> {
    /// The argument was `undefined`.
    #[default]
    Undefined,
    /// The argument was `null`.
    Null,
    /// A cached pull request.
    Value(T),
}

/// Whether a cached pull request can be reused for the current context.
///
/// Requires a cached entry (including an explicit `null`), a non-null current
/// context, and matching cache/current contexts.
pub fn can_reuse_cached_pr(
    cached_pr: CachedPr<&PullRequest>,
    cached_context: Option<&PrCacheContext>,
    current_context: Option<&PrCacheContext>,
) -> bool {
    !matches!(cached_pr, CachedPr::Undefined)
        && current_context.is_some()
        && is_same_pr_cache_context(cached_context, current_context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_remotes() {
        let cases = [
            ("https://github.com/owner/repo.git", Some("owner/repo")),
            ("https://github.com/owner/repo", Some("owner/repo")),
            ("git@github.com:owner/repo.git", Some("owner/repo")),
            ("git://github.com/owner/repo.git", Some("owner/repo")),
            ("ssh://git@github.com/owner/repo.git", Some("owner/repo")),
            ("https://gitlab.com/owner/repo.git", None),
            ("https://github.com/owner", None),
            ("", None),
        ];
        for (remote, expected) in cases {
            assert_eq!(
                parse_github_repo(remote).as_deref(),
                expected,
                "remote {remote}"
            );
        }
    }

    #[test]
    fn parses_default_branch() {
        let cases = [
            ("origin/main", "main"),
            ("origin/feature/x", "feature/x"),
            ("main", "main"),
            ("", ""),
        ];
        for (reference, expected) in cases {
            assert_eq!(parse_default_branch(reference), expected);
        }
    }

    #[test]
    fn compares_pr_cache_contexts() {
        let main = create_pr_cache_context("main", Some("owner/repo"));
        let same = create_pr_cache_context("main", Some("owner/repo"));
        let other_branch = create_pr_cache_context("dev", Some("owner/repo"));
        let other_repo = create_pr_cache_context("main", Some("other/repo"));
        let no_repo = create_pr_cache_context("main", None);

        assert!(is_same_pr_cache_context(Some(&main), Some(&same)));
        assert!(!is_same_pr_cache_context(Some(&main), Some(&other_branch)));
        assert!(!is_same_pr_cache_context(Some(&main), Some(&other_repo)));
        assert!(!is_same_pr_cache_context(Some(&main), Some(&no_repo)));
        assert!(!is_same_pr_cache_context(None, Some(&main)));
        assert!(!is_same_pr_cache_context(Some(&main), None));
        assert!(!is_same_pr_cache_context(None, None));
    }

    #[test]
    fn reuses_cached_pr_only_for_matching_context() {
        let cached = PullRequest {
            number: 7,
            url: "https://github.com/owner/repo/pull/7".to_string(),
        };
        let context = create_pr_cache_context("main", Some("owner/repo"));
        let other = create_pr_cache_context("dev", Some("owner/repo"));

        assert!(can_reuse_cached_pr(
            CachedPr::Value(&cached),
            Some(&context),
            Some(&context)
        ));
        // An explicit `null` entry still counts as cached.
        assert!(can_reuse_cached_pr(
            CachedPr::Null,
            Some(&context),
            Some(&context)
        ));
        assert!(!can_reuse_cached_pr(
            CachedPr::Undefined,
            Some(&context),
            Some(&context)
        ));
        assert!(!can_reuse_cached_pr(
            CachedPr::Value(&cached),
            Some(&context),
            Some(&other)
        ));
        assert!(!can_reuse_cached_pr(
            CachedPr::Value(&cached),
            Some(&context),
            None
        ));
        assert!(!can_reuse_cached_pr(
            CachedPr::Value(&cached),
            None,
            Some(&context)
        ));
    }
}
