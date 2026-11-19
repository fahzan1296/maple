use maple_tui::component::Component;
use maple_tui::components::markdown::{fast_line_start_hazard, Markdown};

fn cold(text: &str, width: u16) -> Vec<String> {
    let mut markdown = Markdown::new();
    markdown.set_text(text);
    markdown.render(width)
}

#[test]
fn prose_reveal_is_byte_identical_every_frame_and_uses_the_fast_path() {
    let full = "This is a plain prose paragraph that streams in one character at a time without any markdown styling markers.";
    let mut streaming = Markdown::new();
    streaming.set_transient_render_cache(true);
    for len in 1..=full.len() {
        let slice = &full[..len];
        streaming.set_text(slice);
        let frame = streaming.render(60);
        assert_eq!(frame, cold(slice, 60), "frame {len} diverged");
    }
    assert!(streaming.fast_path_hits() > 0, "fast path never engaged");
}

#[test]
fn run_level_style_disarms_the_fast_path() {
    let full = "a styled streaming paragraph grows one character at a time";
    let mut streaming = Markdown::new();
    streaming.set_transient_render_cache(true);
    streaming.set_prose_transform(Some(Box::new(|text: &str| {
        format!("\x1b[35m{text}\x1b[39m")
    })));
    for len in 1..=full.len() {
        let slice = &full[..len];
        streaming.set_text(slice);
        let frame = streaming.render(60);
        let mut oracle = Markdown::new();
        oracle.set_prose_transform(Some(Box::new(|text: &str| {
            format!("\x1b[35m{text}\x1b[39m")
        })));
        oracle.set_text(slice);
        assert_eq!(frame, oracle.render(60), "styled frame {len} diverged");
    }
    assert_eq!(streaming.fast_path_hits(), 0);
}

#[test]
fn emphasis_markers_disarm_and_stay_byte_identical() {
    let full = "*the whole span* must render emphasized, not stale";
    let mut streaming = Markdown::new();
    streaming.set_transient_render_cache(true);
    for len in 1..=full.len() {
        let slice = &full[..len];
        streaming.set_text(slice);
        assert_eq!(streaming.render(60), cold(slice, 60), "frame {len}");
    }
    assert_eq!(streaming.fast_path_hits(), 0);
}

#[test]
fn width_change_and_finalize_relex_cold() {
    let full = "a streamed paragraph that must reflow cleanly when the terminal width changes mid-stream";
    let mut streaming = Markdown::new();
    streaming.set_transient_render_cache(true);
    for len in (1..=full.len()).step_by(3) {
        streaming.set_text(&full[..len]);
        let _ = streaming.render(80);
    }
    streaming.set_text(full);
    assert_eq!(streaming.render(40), cold(full, 40));

    streaming.set_transient_render_cache(false);
    streaming.set_text(full);
    assert_eq!(streaming.render(60), cold(full, 60));
}

#[test]
fn line_start_hazard_matches_block_markers_and_ref_defs() {
    for line in [
        "hello world",
        "a b c",
        "the quick brown fox",
        "plain text here",
        "foo",
        "x y z",
    ] {
        assert!(!fast_line_start_hazard(line), "prose hazard: {line}");
    }
    for line in [
        "# h", "> q", "1. x", "- a", "+ a", "---", "***", "═══", "───", "===",
    ] {
        assert!(fast_line_start_hazard(line), "marker hazard: {line}");
    }
    assert!(fast_line_start_hazard("[label]: https://x"));
}
