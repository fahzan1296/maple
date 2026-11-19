// Ported/adapted subset of l/test/segment-track.test.ts to Rust segment_track APIs.
// Deviations: Rust render/describe APIs differ (describe_segment_track, render_segment_track,
// resolve_segment_palette, TrackSegment struct); contrast/RGB/theme internals tested differently,
// so assertions focus on palette resolution, describe roles/labels, and rendered chip structure.

use maple_tui::strip_ansi;
use maple_tui::chrome::segment_track::{
    describe_segment_track, render_segment_track, resolve_segment_palette, TrackSegment,
};
use maple_tui::native::node::NodeKind;
use maple_tui::theme::{builtin, color::ColorMode, create_theme, CreateThemeOptions, Theme, ThemeColor};

fn theme() -> Theme {
    create_theme(
        builtin::builtin_theme_json("dark").expect("builtin theme"),
        CreateThemeOptions { mode: Some(ColorMode::TrueColor), ..Default::default() },
    )
    .expect("theme resolves")
}

fn segments(labels: &[&str]) -> Vec<TrackSegment> {
    labels.iter().map(|l| TrackSegment::new(*l)).collect()
}

#[test]
fn resolve_segment_palette_starts_at_accent_and_respects_count() {
    let t = theme();
    assert_eq!(resolve_segment_palette(1, &t), vec![ThemeColor::Accent]);
    assert!(resolve_segment_palette(4, &t).len() <= 4);
}

#[test]
fn describe_marks_active_segment_and_keeps_labels() {
    let described = describe_segment_track(&segments(&["smol", "default", "slow"]), 1, &theme());
    assert_eq!(described.kind, NodeKind::Row);
    assert_eq!(described.props.role(), Some("omp.track"));
    assert_eq!(described.children.len(), 3);
    assert_eq!(described.children[1].props.role(), Some("omp.track.active"));
    assert_eq!(described.children[0].props.role(), Some("omp.track.item"));
    assert_eq!(described.children[1].key.as_deref(), Some("1"));
}

#[test]
fn render_includes_labels_and_fills_active_chip() {
    let rendered = render_segment_track(&segments(&["smol", "default", "slow"]), 1, &theme());
    let plain = strip_ansi(&rendered);
    assert!(plain.contains("smol"));
    assert!(plain.contains("default"));
    assert!(plain.contains("slow"));
    // Active chip is bold-wrapped label in Rust renderer.
    assert!(rendered.contains("\x1b[1m default \x1b[22m"));
}

#[test]
fn render_moves_filled_chip_with_active_index() {
    let t = theme();
    let first = render_segment_track(&segments(&["smol", "default", "slow"]), 0, &t);
    let last = render_segment_track(&segments(&["smol", "default", "slow"]), 2, &t);
    assert!(first.contains("\x1b[1m smol \x1b[22m"));
    assert!(last.contains("\x1b[1m slow \x1b[22m"));
    assert!(!first.contains("\x1b[1m slow \x1b[22m"));
}
