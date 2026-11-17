//! Port of `l/test/bash-execution-sixel.test.ts`.
//!
//! SIXEL sanitization is a pure helper in the crate (no env-gated component
//! path), so the passthrough tests call `sanitize_with_sixel_passthrough`
//! directly. The chunk-throttle gate is absent (the caller owns batching), so
//! that one test is ignored; the streaming cap, `set_complete` replacement,
//! and expand footer are asserted.

use maple_tui::chat::bash_execution::{BashCompletion, BashExecutionComponent};
use maple_tui::component::Component;
use maple_tui::render::sixel::sanitize_with_sixel_passthrough;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{CreateThemeOptions, Theme, create_theme};

const SIXEL: &str = "\x1bPqabc\x1b\\";

fn theme() -> Theme {
    create_theme(&builtin_theme_json("dark").expect("dark theme json"), CreateThemeOptions::default())
        .expect("dark theme")
}

fn truncate_marker(text: &str) -> String {
    if text.chars().count() > 20 {
        format!("{} visible columns omitted", text.chars().take(20).collect::<String>())
    } else {
        text.to_string()
    }
}

#[test]
fn preserves_sixel_when_passthrough_enabled() {
    let out = sanitize_with_sixel_passthrough(SIXEL, true, |text| text.replace('\x1b', ""));
    assert!(out.contains(SIXEL));
}

#[test]
fn does_not_truncate_long_sixel_payload_lines() {
    let payload = format!("\x1bPq{}\x1b\\", "A".repeat(5000));
    let out = sanitize_with_sixel_passthrough(&payload, true, truncate_marker);
    assert!(out.contains("\x1bPq"));
    assert!(out.contains("\x1b\\"));
    assert!(!out.contains("visible columns omitted"));
}

#[test]
fn still_truncates_long_non_sixel_lines() {
    let out = sanitize_with_sixel_passthrough(&"x".repeat(5000), true, truncate_marker);
    assert!(out.contains("visible columns omitted"));
    assert!(!out.contains("\x1bPq"));
}

#[test]
fn strips_sixel_when_passthrough_disabled() {
    let out = sanitize_with_sixel_passthrough(SIXEL, false, |text| text.replace('\x1b', ""));
    assert!(!out.contains("\x1bPq"));
    assert_eq!(out, "Pqabc\\");
}

#[test]
fn caps_stored_lines_during_streaming_and_keeps_tail() {
    let mut component = BashExecutionComponent::new("test", theme(), false);
    let lines: Vec<String> = (0..500).map(|i| format!("line{i}")).collect();
    component.append_output(&lines.join("\n"));
    let output = component.output();
    assert!(output.split('\n').count() <= 101, "lines: {}", output.split('\n').count());
    assert!(output.contains("line499"));
    assert!(!output.contains("line0\n"));
}

#[test]
fn set_complete_replaces_streaming_output() {
    let mut component = BashExecutionComponent::new("test", theme(), false);
    component.append_output("streaming_line\n");
    component.set_complete(
        Some(0),
        false,
        BashCompletion {
            output: Some("final_line_1\nfinal_line_2".to_string()),
            ..BashCompletion::default()
        },
        0.0,
    );
    let output = component.output();
    assert!(output.contains("final_line_1"));
    assert!(output.contains("final_line_2"));
    assert!(!output.contains("streaming_line"));
}

fn completed_component() -> BashExecutionComponent {
    let mut component = BashExecutionComponent::new("ls", theme(), false);
    let lines: Vec<String> = (0..27).map(|i| format!("entry{i}")).collect();
    component.set_complete(
        Some(0),
        false,
        BashCompletion {
            output: Some(lines.join("\n")),
            ..BashCompletion::default()
        },
        0.0,
    );
    component
}

#[test]
fn collapsed_footer_advertises_hidden_lines() {
    let mut component = completed_component();
    let rendered = component.render(120).join("\n");
    assert!(rendered.contains("more lines"), "rendered: {rendered}");
}

#[test]
fn expanded_drops_footer_and_shows_every_line() {
    let mut component = completed_component();
    component.set_expanded(true);
    let rendered = component.render(120).join("\n");
    assert!(!rendered.contains("more lines"));
    assert!(rendered.contains("entry0"));
    assert!(rendered.contains("entry26"));
}

#[test]
fn collapsing_restores_footer() {
    let mut component = completed_component();
    component.set_expanded(true);
    component.set_expanded(false);
    let rendered = component.render(120).join("\n");
    assert!(rendered.contains("more lines"));
}

#[test]
#[ignore = "missing facility: streaming chunk throttle gate (caller owns batching)"]
fn gate_drops_rapid_chunks() {
    let mut component = BashExecutionComponent::new("test", theme(), false);
    for i in 0..100 {
        component.append_output(&format!("chunk{i}\n"));
    }
    // Real adjacent behavior: without a gate every chunk is stored.
    let output = component.output();
    assert!(output.contains("chunk0"));
    assert!(output.contains("chunk99"));
}
