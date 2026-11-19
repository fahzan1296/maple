//! Port of `l/test/svg-source.test.ts`.

use std::collections::HashMap;

use maple_tui::chat::svg_source::{
	close_partial_svg, has_svg_fence, prepare_svg, split_svg_fences, FigureSegment,
};

fn markdown(text: &str) -> FigureSegment {
	FigureSegment::Markdown {
		text: text.to_string(),
	}
}

fn svg(source: &str, closed: bool) -> FigureSegment {
	FigureSegment::Svg {
		source: source.to_string(),
		closed,
	}
}

#[test]
fn lifts_top_level_svg_fences_out_of_the_prose_around_them() {
	let source = "Intro\n\n```svg\n<svg/>\n```\n\n~~~ SVG title\n<svg></svg>\n~~~\nOutro";
	assert_eq!(
		split_svg_fences(source),
		vec![
			markdown("Intro\n\n"),
			svg("<svg/>\n", true),
			svg("<svg></svg>\n", true),
			markdown("Outro"),
		]
	);
	assert!(has_svg_fence(source));
}

#[test]
fn lifts_crlf_svg_fences_without_dropping_line_endings() {
	let source = "Intro\r\n```svg\r\n<svg/>\r\n```\r\nOutro";
	assert!(has_svg_fence(source));
	assert_eq!(
		split_svg_fences(source),
		vec![
			markdown("Intro\r\n"),
			svg("<svg/>\r\n", true),
			markdown("Outro"),
		]
	);
}

#[test]
fn lifts_cr_only_svg_fences_as_closed_figures() {
	assert_eq!(
		split_svg_fences("```svg\r<svg/>\r```"),
		vec![svg("<svg/>\r", true)]
	);
}

#[test]
fn leaves_nested_or_indented_svg_fences_as_prose() {
	let nested = "````markdown\n```svg\n<svg/>\n```\n````";
	let indented = "    ```svg\n    <svg/>\n    ```";
	assert_eq!(split_svg_fences(nested), vec![markdown(nested)]);
	assert_eq!(split_svg_fences(indented), vec![markdown(indented)]);
	assert!(!has_svg_fence(indented));
	assert!(!has_svg_fence("```svgx\n```"));
}

#[test]
fn keeps_a_streaming_fence_open_and_needs_a_long_enough_closer() {
	assert_eq!(
		split_svg_fences("Look:\n````svg\n<svg>\n```\n<rect/>"),
		vec![markdown("Look:\n"), svg("<svg>\n```\n<rect/>", false)]
	);
	assert_eq!(split_svg_fences("```svg"), vec![svg("", false)]);
}

#[test]
fn close_partial_returns_null_until_the_root_start_tag_is_complete() {
	assert_eq!(close_partial_svg(""), None);
	assert_eq!(close_partial_svg("<?xml version=\"1.0\"?>\n<svg viewBox=\"0 0"), None);
	assert_eq!(close_partial_svg("<!-- <svg> -->"), None);
}

#[test]
fn close_partial_passes_a_complete_document_through_unchanged() {
	let source = "<svg viewBox=\"0 0 10 10\"><g><rect width=\"1\"/></g></svg>\n";
	assert_eq!(close_partial_svg(source), Some(source.to_string()));
}

#[test]
fn close_partial_cuts_the_construct_being_written_and_closes_innermost_first() {
	assert_eq!(
		close_partial_svg("<svg><g><text x=\"1\">Hel"),
		Some("<svg><g><text x=\"1\">Hel</text></g></svg>".to_string())
	);
	assert_eq!(
		close_partial_svg("<svg><g><path d=\"M0 0\" data-x=\"a>b"),
		Some("<svg><g></g></svg>".to_string())
	);
	assert_eq!(
		close_partial_svg("<svg><g></g><!-- note"),
		Some("<svg><g></g></svg>".to_string())
	);
	assert_eq!(
		close_partial_svg("<svg><style><![CDATA[ rect { fill"),
		Some("<svg><style></style></svg>".to_string())
	);
	assert_eq!(
		close_partial_svg("<svg><text>a &amp; b &am"),
		Some("<svg><text>a &amp; b </text></svg>".to_string())
	);
	assert_eq!(
		close_partial_svg("<svg><g><rect/></"),
		Some("<svg><g><rect/></g></svg>".to_string())
	);
}

fn palette() -> HashMap<String, String> {
	[("fg", "#eeeeee"), ("accent", "#ff8800")]
		.iter()
		.map(|(key, value)| ((*key).to_string(), (*value).to_string()))
		.collect()
}

#[test]
fn prepare_resolves_theme_tokens_with_fallbacks() {
	assert_eq!(
		prepare_svg(
			"<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"red\" font-family=\"serif\"><style>.a{fill:var(--accent)}</style>\
			 <rect stroke=\"var( --accent , #000)\" fill=\"var(--nope, rgb(1, 2, 3))\"/><text fill=\"var(--missing)\"/></svg>",
			&palette()
		),
		"<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"red\" font-family=\"serif\"><style>.a{fill:#ff8800}</style>\
		 <rect stroke=\"#ff8800\" fill=\"rgb(1, 2, 3)\"/><text fill=\"#eeeeee\"/></svg>"
	);
}

#[test]
fn prepare_gives_a_bare_root_theme_color_font_and_namespaces() {
	assert_eq!(
		prepare_svg(
			"<svg viewBox=\"0 0 1 1\"><use xlink:href=\"#a\"/></svg>",
			&palette()
		),
		"<svg color=\"#eeeeee\" font-family=\"sans-serif\" xmlns=\"http://www.w3.org/2000/svg\" \
		 xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"0 0 1 1\"><use xlink:href=\"#a\"/></svg>"
	);
}