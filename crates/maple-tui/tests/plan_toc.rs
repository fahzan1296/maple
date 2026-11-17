// Ported from l/test/plan-toc.test.ts (pure parsing/joining/deletion helpers).

use std::collections::HashSet;

use maple_tui::overlays::plan_toc::{
    join_plan_sections, parse_plan_sections, section_deletion_span, strip_inline_markdown, PlanSection,
};

fn titles(sections: &[PlanSection]) -> Vec<&str> {
    sections.iter().map(|s| s.title.as_str()).collect()
}
fn levels(sections: &[PlanSection]) -> Vec<usize> {
    sections.iter().map(|s| s.level).collect()
}

#[test]
fn splits_preamble_and_sections_per_atx_heading_tracking_depth() {
    let sections = parse_plan_sections("intro\n\n# Overview\n\nbody\n\n## Goal\n\ngoal\n\n# Risks\n\nrisk\n");
    assert_eq!(levels(&sections), vec![0, 1, 2, 1]);
    assert_eq!(titles(&sections), vec!["", "Overview", "Goal", "Risks"]);
    assert_eq!(sections[0].raw, "intro\n\n");
}

#[test]
fn emits_no_preamble_section_when_document_opens_with_heading() {
    let sections = parse_plan_sections("# Top\n\nbody\n");
    assert_eq!(levels(&sections), vec![1]);
    assert_eq!(sections[0].title, "Top");
}

#[test]
fn does_not_treat_hash_inside_fenced_code_blocks_as_heading() {
    let sections = parse_plan_sections("# Real\n\n```\n# not a heading\n```\n\n~~~\n## also not\n~~~\n");
    assert_eq!(levels(&sections), vec![1]);
    assert_eq!(titles(&sections), vec!["Real"]);
}

#[test]
fn requires_whitespace_after_hashes_so_tag_is_body_text() {
    let sections = parse_plan_sections("#tag is not a heading\nmore body\n");
    assert_eq!(levels(&sections), vec![0]);
}

#[test]
fn strips_inline_markdown_and_closing_hashes_from_titles() {
    let sections = parse_plan_sections("## **Goal** & [docs](http://x) ##\n\nbody\n");
    assert_eq!(sections[0].title, "Goal & docs");
}

#[test]
fn join_round_trips_newline_terminated_document_and_normalizes_trailing_newline() {
    let text = "intro\n\n# A\n\nbody a\n\n## A1\n\nnested\n\n# B\n\nbody b\n";
    assert_eq!(join_plan_sections(&parse_plan_sections(text)), text);
    assert_eq!(join_plan_sections(&parse_plan_sections("# A\n\nbody")), "# A\n\nbody\n");
    assert_eq!(join_plan_sections(&parse_plan_sections("")), "");
}

#[test]
fn section_deletion_span_removes_heading_with_nested_children_and_skips_preamble() {
    let sections = parse_plan_sections("intro\n\n# A\n\na\n\n## A1\n\na1\n\n## A2\n\na2\n\n# B\n\nb\n");
    assert_eq!(section_deletion_span(&sections, 1), vec![1, 2, 3]);
    assert_eq!(section_deletion_span(&sections, 2), vec![2]);
    assert!(section_deletion_span(&sections, 0).is_empty());
    let span: HashSet<usize> = section_deletion_span(&sections, 1).into_iter().collect();
    let survivors: Vec<PlanSection> = sections
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !span.contains(i))
        .map(|(_, s)| s)
        .collect();
    let result = join_plan_sections(&survivors);
    assert!(result.contains("# B"));
    assert!(!result.contains("# A"));
    assert!(!result.contains("## A1"));
}

#[test]
fn strip_inline_markdown_collapses_to_readable_text() {
    assert_eq!(strip_inline_markdown("**bold** _it_ `code` [t](u)"), "bold it code t");
    assert_eq!(strip_inline_markdown("a   b\tc"), "a b c");
}
