//! Port of `l/test/markdown-prose.test.ts`.
//!
//! `mask_non_prose` and `keyword_in_prose` are pure functions over `&str` plus a
//! caller-supplied `Regex`, so every reference case ports directly.

use maple_tui::prompt::markdown_prose::{keyword_in_prose, mask_non_prose};
use regex::Regex;

fn orchestrate() -> Regex {
    Regex::new(r"(?i)\borchestrate\b").expect("keyword regex")
}

#[test]
fn blanks_fenced_code_blocks_while_keeping_indices_aligned() {
    let text = "before\n```\norchestrate\n```\nafter orchestrate";
    let masked = mask_non_prose(text);
    assert_eq!(masked.len(), text.len());
    let found = masked.find("orchestrate").expect("prose occurrence survives");
    let expected = text.rfind("orchestrate").expect("reference index");
    assert_eq!(found, expected);
    assert!(masked.starts_with("before\n"));
}

#[test]
fn blanks_tilde_fences_and_language_tagged_fences() {
    assert!(!mask_non_prose("~~~\norchestrate\n~~~").contains("orchestrate"));
    assert!(!mask_non_prose("```ts\nconst orchestrate = 1\n```").contains("orchestrate"));
}

#[test]
fn treats_an_unterminated_fence_as_code_through_end_of_text() {
    assert!(!mask_non_prose("```\norchestrate").contains("orchestrate"));
}

#[test]
fn matches_a_standalone_keyword_in_prose() {
    let word = orchestrate();
    assert!(keyword_in_prose("please orchestrate this", &word));
    assert!(keyword_in_prose("orchestrate", &word));
}

#[test]
fn ignores_keywords_inside_inline_code_spans() {
    let word = orchestrate();
    assert!(!keyword_in_prose("use `orchestrate` now", &word));
    assert!(!keyword_in_prose("``a `orchestrate` b``", &word));
}

#[test]
fn treats_an_unmatched_backtick_run_as_literal_text() {
    let word = orchestrate();
    assert!(keyword_in_prose("the ` then orchestrate", &word));
}

#[test]
fn ignores_keywords_inside_fenced_code_blocks() {
    let word = orchestrate();
    assert!(!keyword_in_prose("```\norchestrate\n```", &word));
    assert!(!keyword_in_prose("text\n```\norchestrate here\n```\ndone", &word));
}

#[test]
fn ignores_keywords_inside_xml_html_sections_including_nested_and_comments() {
    let word = orchestrate();
    assert!(!keyword_in_prose("<note>orchestrate</note>", &word));
    assert!(!keyword_in_prose("<a><a>orchestrate</a></a>", &word));
    assert!(!keyword_in_prose("<!-- orchestrate -->", &word));
    assert!(!keyword_in_prose("<tag attr=\"x>y\">orchestrate</tag>", &word));
}

#[test]
fn still_matches_prose_outside_of_code_markup_regions() {
    let word = orchestrate();
    assert!(keyword_in_prose("<b>orchestrate</b> then orchestrate", &word));
    assert!(keyword_in_prose("run `x` then orchestrate", &word));
    assert!(keyword_in_prose("```\ncode\n```\norchestrate", &word));
}

#[test]
fn does_not_over_mask_on_a_stray_less_than_or_an_unbalanced_tag() {
    let word = orchestrate();
    assert!(keyword_in_prose("a < b and orchestrate", &word));
    assert!(keyword_in_prose("<br> orchestrate after", &word));
}

#[test]
fn respects_word_boundaries_regardless_of_region() {
    let word = orchestrate();
    assert!(!keyword_in_prose("reorchestrate the orchestration", &word));
}
