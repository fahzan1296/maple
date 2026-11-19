//! Port of `l/test/github-ref-autocomplete.test.ts` token-detection and
//! suggestions sections. The provider-integration section needs the async
//! `CombinedAutocompleteProvider`, which is not part of this module's surface.

use maple_tui::autocomplete::AutocompleteItem;
use maple_tui::prompt::github_ref_autocomplete::{
	get_github_ref_context, get_github_ref_suggestions, GithubRefQualifier,
};

fn qualifier(context: &maple_tui::prompt::github_ref_autocomplete::GithubRefContext) -> Option<&'static str> {
	context.qualifier.map(|qualifier| match qualifier {
		GithubRefQualifier::Pr => "pr",
		GithubRefQualifier::Issue => "issue",
	})
}

fn context(text: &str) -> maple_tui::prompt::github_ref_autocomplete::GithubRefContext {
	get_github_ref_context(text).unwrap_or_else(|| panic!("expected a ref context for {text:?}"))
}

#[test]
fn matches_a_standalone_number_ending_at_the_cursor() {
	assert_eq!(context("#3164").prefix, "#3164");
	assert_eq!(qualifier(&context("#3164")), None);
	assert_eq!(context("#3164").number, "3164");

	assert_eq!(context("look at #3164").prefix, "#3164");
	// only the token ending at the cursor wins
	let seen = context("see #1 then #3164");
	assert_eq!(seen.prefix, "#3164");
	assert_eq!(seen.number, "3164");
}

#[test]
fn requires_a_token_boundary_before_the_hash() {
	assert_eq!(get_github_ref_context("owner/repo#3164"), None);
	assert_eq!(get_github_ref_context("foo#3164"), None);
	assert_eq!(get_github_ref_context("C#12"), None);
	assert_eq!(
		get_github_ref_context("https://github.com/can1357/oh-my-pi#3164"),
		None
	);
	assert_eq!(get_github_ref_context("path/#3164"), None);
}

#[test]
fn does_not_match_bare_hash_text_mixed_zero_or_leading_zeros() {
	assert_eq!(get_github_ref_context("#"), None);
	assert_eq!(get_github_ref_context("#copy"), None);
	assert_eq!(get_github_ref_context("#3164abc"), None);
	assert_eq!(get_github_ref_context("#3a"), None);
	// a space after the digits closes the token
	assert_eq!(get_github_ref_context("#3164 "), None);
	assert_eq!(get_github_ref_context("#0"), None);
	assert_eq!(get_github_ref_context("#0123"), None);
}

#[test]
fn detects_a_qualifier_word_immediately_before_the_number() {
	assert_eq!(context("pr #3164").prefix, "pr #3164");
	assert_eq!(qualifier(&context("pr #3164")), Some("pr"));
	assert_eq!(context("PR #3164").prefix, "PR #3164");
	assert_eq!(qualifier(&context("PR #3164")), Some("pr"));
	assert_eq!(context("pull #3164").prefix, "pull #3164");
	assert_eq!(qualifier(&context("pull #3164")), Some("pr"));
	assert_eq!(context("issue #3164").prefix, "issue #3164");
	assert_eq!(qualifier(&context("issue #3164")), Some("issue"));
	assert_eq!(qualifier(&context("look at the issue #3164")), Some("issue"));
}

#[test]
fn does_not_treat_arbitrary_words_or_glued_qualifiers_as_the_type() {
	assert_eq!(qualifier(&context("review #3164")), None);
	// "pr" inside "src/pr" is preceded by '/', not a boundary
	assert_eq!(qualifier(&context("src/pr #3164")), None);
	assert_eq!(get_github_ref_context("pr#3164"), None);
}

fn item(value: &str, label: &str, description: &str) -> AutocompleteItem {
	AutocompleteItem::new(value, label).with_description(description)
}

#[test]
fn offers_both_candidates_when_no_qualifier_is_given() {
	let (items, prefix) = get_github_ref_suggestions("#3164").expect("suggestions");
	assert_eq!(prefix, "#3164");
	assert_eq!(
		items,
		[
			item("pr://3164", "PR #3164", "GitHub pull request"),
			item("issue://3164", "Issue #3164", "GitHub issue"),
		]
	);
}

#[test]
fn offers_only_the_pr_candidate_for_a_pr_or_pull_qualifier() {
	let (items, prefix) = get_github_ref_suggestions("pr #3164").expect("suggestions");
	assert_eq!(prefix, "pr #3164");
	assert_eq!(items, [item("pr://3164", "PR #3164", "GitHub pull request")]);
}

#[test]
fn offers_only_the_issue_candidate_for_an_issue_qualifier() {
	let (items, _) = get_github_ref_suggestions("issue #3164").expect("suggestions");
	assert_eq!(
		items,
		[item("issue://3164", "Issue #3164", "GitHub issue")]
	);
}

#[test]
fn returns_none_for_embedded_or_non_ref_text() {
	assert_eq!(get_github_ref_suggestions("owner/repo#3164"), None);
	assert_eq!(get_github_ref_suggestions("#copy"), None);
	assert_eq!(get_github_ref_suggestions("#0"), None);
}