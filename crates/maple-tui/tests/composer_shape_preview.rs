//! Port of `l/test/composer-shape-preview.test.ts`.
//!
//! Deviations: the reference resolves styles through the process-global theme;
//! the crate injects a [`ComposerStyleLike`] per shape, so the echo status
//! source below stands in for the live status line. The light-theme contrast
//! test needs a global `setTheme`, which the crate does not have.

use maple_tui::overlays::composer_shape_preview::{
	ComposerPreviewStatusSource, ComposerShapeOption, ComposerShapeRegistryLike, ComposerStyleLike,
	PreviewTopBorder, render_composer_shape_preview,
};
use maple_tui::overlays::composer_shape_registry::{
	ComposerShapeDefinition, get_composer_shape_options, install_extension_composer_shape,
};
use maple_tui::text_shape::visible_width;

struct Status;

fn titled(template: &str, title: Option<&str>) -> PreviewTopBorder {
	let content = format!("{template} {}", title.unwrap_or(""));
	PreviewTopBorder { width: content.chars().count(), content }
}

impl ComposerPreviewStatusSource for Status {
	fn get_top_border(&self, _width: usize, title: Option<&str>) -> PreviewTopBorder {
		titled("TOPBAR", title)
	}
	fn get_band_top_border(&self, _width: usize, title: Option<&str>) -> PreviewTopBorder {
		titled("BAND", title)
	}
	fn get_standalone_top_border(&self, _width: usize, title: Option<&str>) -> PreviewTopBorder {
		titled("CHIP", title)
	}
	fn render_bottom_bar(&self, _width: usize, groups: &str, title: Option<&str>) -> String {
		format!("BOTTOM-{} {}", groups.to_uppercase(), title.unwrap_or(""))
	}
}

enum Top {
	Border,
	Rule,
}

struct Style {
	attachment: &'static str,
	bottom: &'static str,
	gap: bool,
	bottom_rule: bool,
	top: Top,
	gutter: &'static str,
	padding_x: usize,
}

impl ComposerStyleLike for Style {
	fn default_padding_x(&self) -> usize {
		self.padding_x
	}
	fn side_chrome_width(&self, _padding_x: usize) -> usize {
		0
	}
	fn status_attachment(&self) -> &str {
		self.attachment
	}
	fn default_prompt_gutter(&self) -> &str {
		self.gutter
	}
	fn render_top(&self, width: usize, top_border: Option<&str>) -> Option<String> {
		match self.top {
			Top::Border => top_border.map(str::to_string),
			Top::Rule => Some("=".repeat(width)),
		}
	}
	fn render_row(&self, text: &str, pad: &str, gutter: &str, _width: usize) -> Vec<String> {
		vec![format!("{gutter}{text}{pad}")]
	}
	fn render_bottom(&self, _width: usize) -> Option<String> {
		self.bottom_rule.then(|| "───".to_string())
	}
	fn bottom_bar(&self) -> &str {
		self.bottom
	}
	fn bottom_bar_gap(&self) -> bool {
		self.gap
	}
	fn is_filled(&self) -> bool {
		false
	}
}

fn builtin_style(shape: &str) -> Option<Style> {
	let (attachment, bottom, gap, bottom_rule) = match shape {
		"box" => ("top-border", "none", false, false),
		"band" => ("top-band", "none", false, false),
		"claude" => ("top-rule-chip", "left", false, false),
		"rule" => ("top-rule-chip", "left", true, false),
		"pi" => ("none", "full", false, true),
		"borderless" => ("none", "full", false, false),
		"field" | "rail" => ("none", "full", true, false),
		_ => return None,
	};
	Some(Style {
		attachment,
		bottom,
		gap,
		bottom_rule,
		top: Top::Border,
		gutter: "> ",
		padding_x: 1,
	})
}

fn extension_style() -> Style {
	Style {
		attachment: "none",
		bottom: "full",
		gap: false,
		bottom_rule: false,
		top: Top::Rule,
		gutter: "EXT ",
		padding_x: 0,
	}
}

struct Reg;

impl ComposerShapeRegistryLike for Reg {
	fn style_for(&self, shape: &str) -> Option<Box<dyn ComposerStyleLike + '_>> {
		builtin_style(shape).map(|style| Box::new(style) as Box<dyn ComposerStyleLike + '_>)
	}
	fn options(&self) -> Vec<ComposerShapeOption> {
		get_composer_shape_options()
	}
}

struct ExtReg;

impl ComposerShapeRegistryLike for ExtReg {
	fn style_for(&self, shape: &str) -> Option<Box<dyn ComposerStyleLike + '_>> {
		if shape == "extension-dock" {
			Some(Box::new(extension_style()) as Box<dyn ComposerStyleLike + '_>)
		} else {
			Reg.style_for(shape)
		}
	}
	fn options(&self) -> Vec<ComposerShapeOption> {
		get_composer_shape_options()
	}
}

#[test]
fn borrows_status_rows_from_the_status_source_per_shape_layout() {
	let status = Status;
	let reg = Reg;

	let box_lines = render_composer_shape_preview("box", 80, Some(&status), &reg);
	let joined = box_lines.join("\n");
	assert!(joined.contains("TOPBAR"), "{joined}");
	assert!(joined.contains("omp"), "the stand-in title is forwarded: {joined}");
	assert!(!joined.contains("BOTTOM"), "box has no standalone bottom bar: {joined}");

	let band = render_composer_shape_preview("band", 80, Some(&status), &reg).join("\n");
	assert!(band.contains("BAND"), "{band}");
	assert!(band.contains("omp"), "{band}");
	assert!(!band.contains("BOTTOM"), "the band replaces the bottom bar: {band}");

	let claude = render_composer_shape_preview("claude", 80, Some(&status), &reg).join("\n");
	assert!(claude.contains("CHIP"), "{claude}");
	assert!(claude.contains("omp"), "{claude}");
	assert!(claude.contains("BOTTOM-LEFT"), "{claude}");

	let rule = render_composer_shape_preview("rule", 80, Some(&status), &reg);
	let rule_text = rule.join("\n");
	assert!(rule_text.contains("CHIP"), "{rule_text}");
	assert!(rule_text.contains("omp"), "{rule_text}");
	assert!(rule_text.contains("BOTTOM-LEFT"), "{rule_text}");
	assert_eq!(rule[rule.len() - 2], "", "spacer row: rule has no bottom chrome");

	let pi = render_composer_shape_preview("pi", 80, Some(&status), &reg);
	let pi_text = pi.join("\n");
	assert!(!pi_text.contains("CHIP"), "{pi_text}");
	assert!(pi_text.contains("omp"), "{pi_text}");
	assert!(pi_text.contains("BOTTOM-FULL"), "{pi_text}");
	assert!(!pi[pi.len() - 2].is_empty(), "bottom rule already separates the bar");

	let borderless = render_composer_shape_preview("borderless", 80, Some(&status), &reg).join("\n");
	assert!(borderless.contains("omp"), "{borderless}");
	assert!(borderless.contains("BOTTOM-FULL"), "{borderless}");

	for shape in ["field", "rail"] {
		let rendered = render_composer_shape_preview(shape, 80, Some(&status), &reg);
		let text = rendered.join("\n");
		assert!(!text.contains("CHIP"), "{shape}: {text}");
		assert!(text.contains("BOTTOM-FULL"), "{shape}: {text}");
		assert_eq!(rendered[rendered.len() - 2], "", "{shape}: spacer row before the bar");
	}
}

#[test]
fn installs_extension_shapes_into_selectors_and_rendering() {
	let dispose = install_extension_composer_shape(ComposerShapeDefinition {
		id: "extension-dock".to_string(),
		label: "Extension Dock".to_string(),
		description: Some("Custom extension composer".to_string()),
		style: serde_json::json!({}),
	});

	let options = get_composer_shape_options();
	let installed = options
		.iter()
		.find(|option| option.value == "extension-dock")
		.expect("extension shape is registered");
	assert_eq!(installed.label, "Extension Dock");
	assert_eq!(installed.description.as_deref(), Some("Custom extension composer"));

	let rendered = render_composer_shape_preview("extension-dock", 76, Option::<&Status>::None, &ExtReg)
		.join("\n");
	assert!(rendered.contains(&"=".repeat(76)), "{rendered}");
	assert!(rendered.contains("EXT "), "{rendered}");
	assert!(rendered.contains("Ask anything"), "{rendered}");

	dispose();
	assert!(
		!get_composer_shape_options().iter().any(|option| option.value == "extension-dock"),
		"disposing removes the selector entry"
	);
}

struct WideStatus;

impl ComposerPreviewStatusSource for WideStatus {
	fn get_top_border(&self, width: usize, _title: Option<&str>) -> PreviewTopBorder {
		PreviewTopBorder { content: " ".repeat(width), width }
	}
	fn get_band_top_border(&self, width: usize, _title: Option<&str>) -> PreviewTopBorder {
		let content = format!("{}{}", " ".repeat(width.saturating_sub(6)), "STATUS");
		PreviewTopBorder { width, content }
	}
	fn get_standalone_top_border(&self, width: usize, _title: Option<&str>) -> PreviewTopBorder {
		PreviewTopBorder { content: " ".repeat(width), width }
	}
	fn render_bottom_bar(&self, _width: usize, _groups: &str, _title: Option<&str>) -> String {
		String::new()
	}
}

#[test]
fn uses_the_full_overlay_width_instead_of_clipping_the_status_band() {
	let lines = render_composer_shape_preview("band", 200, Some(&WideStatus), &Reg);
	// The renderer prefixes a blank row and a `Preview:` header.
	let status_band = lines.get(2).expect("status band row");
	assert_eq!(visible_width(status_band), 200, "{status_band:?}");
	assert!(status_band.ends_with("STATUS"), "{status_band:?}");
}

#[test]
#[ignore = "missing facility: no global theme/setTheme; styles are injected, so the light-theme contrast case has no counterpart"]
fn resolves_transparent_composer_preview_text_away_from_the_terminal_default() {
	let lines = render_composer_shape_preview("box", 80, Option::<&Status>::None, &Reg);
	assert!(lines.iter().any(|line| line.contains("Ask anything")));
}
