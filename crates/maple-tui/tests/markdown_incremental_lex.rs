//! Port of `l/test/markdown-incremental-lex.test.ts`.
//!
//! The E2 contract: a reused streaming instance revealed in chunks must render
//! byte-identically to a cold one-shot render of the same prefix at every step.
//! The crate's `Markdown` has no transient render-prefix cache — it re-renders
//! from the full text — so the "transient" cases reduce to the same deterministic
//! path and are asserted with the same helper.
//!
//! Not ported (named missing facilities, not runtime features): marked `Lexer`
//! work instrumentation and `latexToBlock` spies, `getLastRenderStableText`
//! stable-prefix introspection, reference-link definitions (`[text][label]`),
//! OSC 8 hyperlink normalization, code-highlight callbacks, mutable theme
//! callbacks, table locks, and orphan-fence repair.

use maple_tui::component::Component;
use maple_tui::components::markdown::{Markdown, MarkdownTheme};

fn render_cold(text: &str, width: u16) -> Vec<String> {
    let mut markdown = Markdown::new();
    markdown.set_text(text);
    markdown.set_theme(MarkdownTheme::default());
    markdown.render(width)
}

/// Reveal `full` through one reused instance and compare every frame to a cold
/// render of the same prefix.
fn assert_identical_growth(full: &str, width: u16, step: usize) {
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    let mut len = 1usize;
    loop {
        let slice = &full[..len];
        streaming.set_text(slice);
        assert_eq!(
            streaming.render(width),
            render_cold(slice, width),
            "diverged at prefix length {len}"
        );
        if len >= full.len() {
            break;
        }
        len = (len + step).min(full.len());
    }
    streaming.set_text(full);
    assert_eq!(streaming.render(width), render_cold(full, width));
}

const PROSE: &str = "Para one with **bold** and _italic_ words and a `code span` for flavor.\n\n\
Para two continues the document with more sentences so the lexer has real\n\
block structure to chew on, then a third paragraph grows at the tail end as\n\n\
the stream appends additional content token by token over many frames here.";

const FENCED: &str = "Intro paragraph before the code block begins streaming in slowly.\n\n\
```ts\nconst x: number = compute(a, b) + delta;\nfor (let i = 0; i < x; i++) {\n  emit(i);\n}\n\
return x.toFixed(2);\n```\n\n\
Trailing prose after the fence keeps growing with more and more sentences.";

const LIST: &str = "Lead-in sentence before the list.\n\n\
- first bullet item with `inline`\n- second bullet item in **bold**\n- third bullet item\n\n\
1. ordered one\n2. ordered two\n3. ordered three\n\n\
Closing paragraph that keeps streaming additional words to the very end here.";

const HEADINGS: &str = "# Title heading\n\nIntro text under the title with some detail.\n\n\
## Section two\n\nBody of section two grows over time.\n\n\
### Subsection\n\nDeeper content that streams in at the tail as the reveal advances.";

fn mixed() -> String {
    let para = "The quick brown fox jumps over the lazy dog while a `code span` and **bold** _italic_ exercise things. ";
    let cb = "\n```ts\nconst x = compute(a, b);\nreturn x;\n```\n\n";
    let list = "\n- one\n- two `inline`\n- three\n\n";
    let mut out = String::new();
    for i in 1..=6 {
        out.push_str(&format!("## Section {i}\n\n{para}{para}{cb}{list}"));
    }
    out
}

fn table() -> String {
    let para = "Intro paragraph before the table streams in, with a `code span` and **bold** for flavor. ";
    let mut out = format!("{para}\n\n| col_a | col_b |\n| ----- | ----- |\n");
    for i in 1..=4 {
        out.push_str(&format!("| row_{i}_a | row_{i}_b |\n"));
    }
    out.push_str("\n\nTrailing prose after the table keeps growing with more sentences.");
    out
}

#[test]
fn prose_growth_is_byte_identical_to_full_lex() {
    assert_identical_growth(PROSE, 60, 13);
}

#[test]
fn fenced_code_growth_is_byte_identical() {
    assert_identical_growth(FENCED, 60, 13);
}

#[test]
fn list_growth_is_byte_identical() {
    assert_identical_growth(LIST, 60, 13);
}

#[test]
fn heading_growth_is_byte_identical() {
    assert_identical_growth(HEADINGS, 60, 13);
}

#[test]
fn mixed_multi_section_corpus_growth_is_byte_identical() {
    assert_identical_growth(&mixed(), 80, 29);
}

#[test]
fn growing_lists_retain_byte_identical_rows_across_item_and_list_shape_changes() {
    let bullets: Vec<String> = (0..18)
        .map(|index| format!("- item {index} has **styled text** and a long sentence that wraps onto another row"))
        .collect();
    let lists = format!(
        "{}\n  - nested child with _emphasis_\n\n\
         1. [ ] ordered task with a long description\n\n\
         2. [x] next task with a `code span`\n\n\
         A paragraph closes the growing list.",
        bullets.join("\n")
    );
    assert_identical_growth(&lists, 46, 11);
}

#[test]
fn incremental_list_items_preserve_nested_indentation_and_continuation_paragraphs() {
    let list = "- first\n  - nested\n    - deep child\n  continuation\n\
- second\n\n  paragraph after the blank\n- third\n  - last nested child\n- fourth";
    assert_identical_growth(list, 34, 1);
}

#[test]
fn incremental_list_items_preserve_ordered_starts_marker_changes_and_incomplete_markers() {
    let list = "8. eighth\n9. ninth\n10. tenth wraps onto the next row at this width\n\n\
11. eleventh\n12) a different delimiter\n13) last\n\n\
- bullet\n* different bullet\n+ final bullet";
    assert_identical_growth(list, 24, 1);
}

#[test]
fn incremental_list_items_preserve_fenced_and_indented_code_and_task_checkboxes() {
    let list = "- [ ] first task\n- [x] second task\n  ```ts\n  const x = 1;\n  ```\n\
- third\n\n      indented code\n- fourth";
    assert_identical_growth(list, 30, 1);
}

#[test]
fn a_late_display_math_closer_can_absorb_the_mutable_block_preceding_a_list() {
    assert_identical_growth("  $$\n1. one\n  $$", 60, 1);
}

#[test]
fn a_list_checkpoint_follows_a_math_prefix_through_rewind_and_restoration() {
    let open = "Intro.\n\n$$\nx = 1\n\nMiddle.\n\n- first **item**\n- second item\n- third item";
    let closed = format!("{open}\n\n$$");
    let restored = format!("{closed} is text\n- fourth item");
    let finished = format!("{restored}\n\n$$\n\nEnd.");
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    for frame in [open, &closed, &restored, &finished] {
        streaming.set_text(frame);
        assert_eq!(streaming.render(60), render_cold(frame, 60));
    }
}

#[test]
fn a_list_checkpoint_does_not_freeze_preceding_paragraph_escapes_or_quote_continuations() {
    assert_identical_growth("foo\\\n \n- *", 60, 1);
    assert_identical_growth("> quote\n      - six\n- $", 60, 1);
}

#[test]
fn a_late_bare_math_closer_can_absorb_earlier_list_items_after_an_equation_prefix() {
    assert_identical_growth("- a=\n  \\begin{align}\n- b\n  \\end{align}", 60, 1);
}

#[test]
fn transient_list_rows_rewrap_after_a_width_change() {
    let list: Vec<String> = (0..12)
        .map(|index| format!("- item {index} wraps this long descriptive line"))
        .collect();
    let list = list.join("\n");
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    streaming.set_text(&list[..list.len() - 12]);
    streaming.render(80);
    streaming.set_text(&list);
    assert_eq!(streaming.render(36), render_cold(&list, 36));
    assert_eq!(streaming.render(80), render_cold(&list, 80));
}

#[test]
fn transient_list_rows_update_after_an_earlier_item_is_edited() {
    let original = "- first **bold** item\n- second item\n- third item";
    let edited = "- first _italic_ item\n- second item\n- third item";
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    streaming.set_text(original);
    streaming.render(40);
    streaming.set_text(edited);
    assert_eq!(streaming.render(40), render_cold(edited, 40));
    let grown = format!("{edited}\n- fourth item");
    streaming.set_text(&grown);
    assert_eq!(streaming.render(40), render_cold(&grown, 40));
}

#[test]
fn transient_render_prefix_table_growing_in_the_tail_is_byte_identical() {
    assert_identical_growth(&table(), 60, 7);
}

#[test]
fn a_width_change_mid_stream_still_matches_a_cold_render_at_the_new_width() {
    let doc = mixed();
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    let mut len = 1usize;
    while len <= doc.len() {
        streaming.set_text(&doc[..len]);
        streaming.render(80);
        if len >= doc.len() {
            break;
        }
        len = (len + 41).min(doc.len());
    }
    streaming.set_text(&doc);
    assert_eq!(streaming.render(40), render_cold(&doc, 40));
    assert_eq!(streaming.render(100), render_cold(&doc, 100));
}

#[test]
fn two_consecutive_ordered_lists_stay_merged_or_renumbered_while_growing() {
    let two_lists = "1. a\n2. b\n\n1. c\n2. d";
    assert_identical_growth(two_lists, 60, 1);
    assert_identical_growth(two_lists, 60, 3);
}

#[test]
fn a_loose_ordered_list_stays_correct_while_growing() {
    let loose = "Intro line before the numbered list begins here.\n\n\
1. First point with enough words to wrap nicely.\n\n\
2. Second point also with sufficient words here.\n\n\
3. Third and final point streamed at the tail end.";
    assert_identical_growth(loose, 60, 1);
    assert_identical_growth(loose, 60, 3);
}

#[test]
fn a_loose_bullet_list_stays_correct_while_growing() {
    let loose = "Lead-in before the bullets.\n\n\
- alpha item with several words to wrap\n\n\
- beta item with several words to wrap\n\n\
- gamma item streamed at the tail end here.";
    assert_identical_growth(loose, 60, 1);
    assert_identical_growth(loose, 60, 3);
}

#[test]
fn a_non_append_change_falls_back_to_a_correct_full_render() {
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    streaming.set_text("# First document\n\nOriginal body paragraph one.\n\nOriginal body paragraph two.\n");
    streaming.render(60);
    let replacement = "## Different\n\nCompletely new content replacing the old buffer entirely.\n";
    streaming.set_text(replacement);
    assert_eq!(streaming.render(60), render_cold(replacement, 60));
}

#[test]
fn a_transient_non_append_replacement_with_no_block_boundary_is_not_served_stale_prefix_lines() {
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    streaming.set_text("# First document\n\nOriginal body paragraph one.\n\nOriginal body paragraph two.\n");
    streaming.render(60);
    let replacement = "a flat replacement with no double newline at all";
    streaming.set_text(replacement);
    assert_eq!(streaming.render(60), render_cold(replacement, 60));
}

#[test]
fn a_transient_non_append_edit_that_keeps_the_frozen_prefix_renders_like_a_one_shot() {
    for [before, after] in [
        ("Para.\n\nnext para here", "Para.\n\n\u{a0}\nAfter."),
        ("- a\n\nnext", "- a\n\n- b"),
    ] {
        let mut streaming = Markdown::new();
        streaming.set_theme(MarkdownTheme::default());
        streaming.set_text(before);
        streaming.render(60);
        streaming.set_text(after);
        assert_eq!(streaming.render(60), render_cold(after, 60));
    }
}

#[test]
fn crlf_text_renders_identically_to_a_cold_lex() {
    let crlf = "Para one with content.\r\n\r\nPara two with `code`.\r\n\r\nPara three tail.\r\n";
    assert_identical_growth(crlf, 60, 11);
}

#[test]
fn bullet_list_followed_by_a_paragraph_grows_byte_identically() {
    let doc = "- alpha item with words\n- beta item with words\n- gamma item\n\n\
Closing paragraph that keeps streaming additional words to the end.";
    assert_identical_growth(doc, 60, 1);
    assert_identical_growth(doc, 60, 3);
}

#[test]
fn bullet_list_followed_by_a_different_marker_list_stays_two_lists() {
    let doc = "- alpha\n- beta\n\n* starred one\n* starred two\n\n+ plus one\n+ plus two";
    assert_identical_growth(doc, 60, 1);
    assert_identical_growth(doc, 60, 3);
}

#[test]
fn ordered_list_followed_by_a_paren_delimited_list_stays_two_lists() {
    let doc = "1. dot one\n2. dot two\n\n1) paren one\n2) paren two";
    assert_identical_growth(doc, 60, 1);
    assert_identical_growth(doc, 60, 3);
}

#[test]
fn list_followed_by_blockquote_grows_byte_identically() {
    let doc = "- alpha\n- beta\n\n> quoted line one with words\n> quoted line two here";
    assert_identical_growth(doc, 60, 1);
    assert_identical_growth(doc, 60, 3);
}

#[test]
fn list_followed_by_heading_grows_byte_identically() {
    let doc = "1. one\n2. two\n\n# Heading after the list\n\nTail prose keeps going on.";
    assert_identical_growth(doc, 60, 1);
    assert_identical_growth(doc, 60, 3);
}

#[test]
fn list_followed_by_fenced_code_grows_byte_identically() {
    let doc = "- alpha\n- beta\n\n```ts\nconst x = compute(a, b);\nreturn x;\n```\n\ntail text";
    assert_identical_growth(doc, 60, 1);
    assert_identical_growth(doc, 60, 3);
}

#[test]
fn a_same_marker_list_across_a_blank_line_still_merges_while_growing() {
    assert_identical_growth("- a\n- b\n\n- c\n- d", 60, 1);
    assert_identical_growth("- a\n- b\n\n- c\n- d", 60, 3);
}

#[test]
fn orphan_fence_repair_starting_mid_stream_keeps_growth_byte_identical() {
    let doc = "Intro paragraph before the stray fence lands in the stream.\n\n\
```\n# Heading after the orphan fence\n\n| col a | col b |\n| --- | --- |\n\n\
Trailing paragraph that keeps streaming after the table ends.";
    assert_identical_growth(doc, 60, 1);
    assert_identical_growth(doc, 60, 3);
    assert_identical_growth(doc, 60, 13);
}

#[test]
fn a_document_that_is_one_still_growing_list_never_diverges() {
    let doc = "- one two three\n- four five six\n\n- seven eight nine";
    assert_identical_growth(doc, 60, 1);
}

#[test]
fn display_math_block_with_blank_lines_stays_whole_across_chunk_sizes() {
    let math_body = (0..6)
        .map(|i| format!("a_{{{i}}} + b_{{{i}}} = c_{{{i}}}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let doc = format!("Intro.\n\n$$\n{math_body}\n$$\n\nAfter the math.\n");
    for step in [1, 7, 40] {
        assert_identical_growth(&doc, 60, step);
    }
}

#[test]
fn an_own_line_display_math_that_never_closes_renders_like_a_one_shot() {
    let paragraphs: Vec<String> = (0..4)
        .map(|i| format!("Body paragraph {i} keeps the stream going."))
        .collect();
    let doc = format!("Intro.\n\n$$\nx = 1\n\n{}\n", paragraphs.join("\n\n"));
    for step in [1, 7, 40] {
        assert_identical_growth(&doc, 60, step);
    }
}

#[test]
fn an_open_display_math_opener_of_one_kind_watches_blocks_of_the_other_kind() {
    let paragraphs: Vec<String> = (0..2)
        .map(|i| format!("Body paragraph {i} keeps the stream going."))
        .collect();
    let paragraphs = paragraphs.join("\n\n");
    for doc in [
        format!("Intro.\n\n\\[\nx = 1\n\n{paragraphs}\n\n$$\na = b\n\nc = d\n$$\n\n{paragraphs}\n\n\\]\n\nAfter.\n"),
        format!("Intro.\n\n$$\nx = 1\n\n{paragraphs}\n\n\\[\na = b\n\nc = d\n\\]\n\n{paragraphs}\n\n$$\n\nAfter.\n"),
    ] {
        for step in [1, 7, 40] {
            assert_identical_growth(&doc, 60, step);
        }
    }
}

#[test]
fn a_closed_display_math_pair_around_a_blank_body_grows_byte_identically() {
    let paragraphs: Vec<String> = (0..80)
        .map(|i| format!("Body paragraph {i} keeps the stream going."))
        .collect();
    let doc = format!("Intro.\n\n$$\n \n$$\n\n{}\n", paragraphs.join("\n\n"));
    assert_identical_growth(&doc, 60, 40);
}

#[test]
fn a_fenced_block_with_a_definition_like_indented_line_grows_byte_identically() {
    let paragraphs: Vec<String> = (0..40)
        .map(|i| format!("Body paragraph {i} keeps the stream going."))
        .collect();
    let doc = format!(
        "Intro paragraph.\n\n```ts\ninterface Bag {{\n    [key: string]: T;\n}}\n```\n\n{}\n",
        paragraphs.join("\n\n")
    );
    assert_identical_growth(&doc, 60, 40);
}

#[test]
fn a_no_break_space_line_after_a_blank_line_stays_in_the_blank_run() {
    let paragraphs: Vec<String> = (0..3)
        .map(|i| format!("Body paragraph {i} keeps the stream going."))
        .collect();
    let doc = format!("Intro.\n\nFirst paragraph.\n\n\u{a0}\nAfter.\n\n{}\n", paragraphs.join("\n\n"));
    assert_identical_growth(&doc, 60, 1);
}

#[test]
fn finalizes_as_a_one_shot_render_when_an_orphan_fence_is_dropped() {
    let doc = "Intro.\n\n~~~\n\n\n| a | b |\n|---|---|\n| 1 | 2 |\n### Heading\n";
    assert_identical_growth(doc, 60, 7);
}

#[test]
fn a_list_item_keeps_its_formatted_rows_while_a_nested_bullet_is_typed() {
    let item = "3. **Platform choice**: the requirement is \"preferably an Android camera app\" for the final wearable, \
but the fastest practical working prototype on this Mac (darwin arm64) would be... Options:\n";
    let full = format!(
        "Key engineering decisions:\n\
1. **Model selection**: lightweight, on-device detection. Options:\n\
   - YOLOv8n / YOLO11n (Ultralytics) — fast on CPU/GPU\n\
   - MediaPipe\n\
2. **Latency budget**: capture → detect → decide → speak.\n\
{item}\
   - Python desktop prototype using macOS camera (AVFoundation via av/cv2/AVFoundation)\n"
    );
    let mut streaming = Markdown::new();
    streaming.set_theme(MarkdownTheme::default());
    let start = full.find(item).unwrap() + "3. **Platform choice**: the requirement".len();
    let item_end = full.find(item).unwrap() + item.len() - 1;
    let mut item_rows: Option<Vec<String>> = None;
    let mut len = start;
    while len <= full.len() {
        streaming.set_text(&full[..len]);
        let rows = streaming.render(77);
        let text: Vec<String> = rows.iter().map(|row| maple_tui::ansi_scan::strip_ansi(row)).collect();
        let text = text.join("\n");
        assert!(text.contains("3. Platform choice: the requirement"));
        assert!(!text.contains("**"));
        if len == item_end {
            item_rows = Some(rows.clone());
        }
        if len > item_end && full[item_end..len].trim() == "-" {
            assert_eq!(rows, item_rows.as_ref().expect("item rows"));
        }
        len += 1;
    }
    streaming.set_text(&full);
    assert_eq!(streaming.render(77), render_cold(&full, 77));
}
