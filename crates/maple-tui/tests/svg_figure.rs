//! Port of `l/test/svg-figure.test.ts` (state/limits subset).
//!
//! The reference rasterizes SVG through Kitty graphics in a virtual terminal
//! and asserts pixel/cell placement. The crate ports the fence state machine,
//! limits, palette, and rendering flag; those are asserted here. Raster painting
//! is host concern.

use std::rc::Rc;

use maple_tui::Component;
use maple_tui::chat::svg_figure::{
	default_markdown_factory, limits_for, set_svg_figure_rendering, svg_figure_palette,
	svg_figure_rendering, CellDimensions, SvgFigure, SvgFigureOptions, MAX_VIEWPORT_SHARE,
};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions};

const SVG: &str = "<svg viewBox=\"0 0 838 200\"><rect width=\"838\" height=\"200\"/></svg>";

#[test]
fn computes_cell_limits_from_width_cells_and_row_ceiling() {
	let cell = CellDimensions { width_px: 16, height_px: 36 };
	let limits = limits_for(80, cell, Some(10));
	assert_eq!(limits.columns, 80);
	assert_eq!(limits.rows, 10);

	let default_rows = limits_for(80, cell, None).rows;
	assert_eq!(default_rows, (24.0 * MAX_VIEWPORT_SHARE).floor() as usize);
	// A ceiling of zero is clamped to one row; edges cap at MAX_EDGE_PX.
	assert_eq!(limits_for(10_000, CellDimensions::default(), Some(0)).rows, 1);
}

#[test]
fn toggles_figure_rendering() {
	set_svg_figure_rendering(false);
	assert!(!svg_figure_rendering());
	set_svg_figure_rendering(true);
	assert!(svg_figure_rendering());
}

#[test]
fn builds_the_theme_palette_with_series_colors() {
	let theme = create_theme(
		builtin_theme_json("dark").expect("embedded dark theme"),
		CreateThemeOptions::default(),
	)
	.expect("theme");
	let palette = svg_figure_palette(&theme);
	for token in ["fg", "muted", "border", "accent", "success", "warning", "error"] {
		assert!(palette.contains_key(token), "missing {token}");
	}
	for index in 1..=6 {
		assert!(palette.contains_key(&format!("c{index}")), "missing c{index}");
	}
}

#[test]
fn tracks_streaming_final_and_pending_state() {
	let options = SvgFigureOptions {
		markdown: Some(default_markdown_factory()),
		rasterizer: None,
		..Default::default()
	};
	let mut figure = SvgFigure::new(options);
	assert!(!figure.pending());
	figure.update(SVG, false);
	assert!(!figure.pending());
	figure.update(SVG, true);
	assert!(figure.pending());
	assert_eq!(figure.source(), SVG);
	assert_eq!(figure.debug_state()["final"], serde_json::json!(true));

	// No rasterizer and no markdown renderer path: render still yields rows.
	let rows = figure.render(80);
	assert!(!rows.is_empty());
	assert_eq!(figure.width(), Some(80));

	figure.invalidate();
	figure.dispose();
	assert!(!figure.pending());
}

#[test]
fn keeps_one_raster_revision_key_per_show() {
	let calls = Rc::new(std::cell::Cell::new(0usize));
	let counter = Rc::clone(&calls);
	let rasterizer = move |prepared: &str, _limits, _cell| {
		counter.set(counter.get() + 1);
		assert!(prepared.starts_with("<svg"));
		Some(("ZkFQ".to_string(), 100, 50))
	};
	let options = SvgFigureOptions {
		markdown: Some(default_markdown_factory()),
		rasterizer: Some(Rc::new(rasterizer)),
		cell: CellDimensions::default(),
		..Default::default()
	};
	let mut figure = SvgFigure::new(options);
	figure.update(SVG, true);
	let rows = figure.render(80);
	assert_eq!(calls.get(), 1, "one rasterization attempt");
	let raster = figure.raster().expect("raster");
	assert_eq!(raster.width_px, 100);
	assert_eq!(raster.height_px, 50);
	assert!(raster.key.ends_with(":1"), "{}", raster.key);
	assert!(!rows.is_empty());
}
