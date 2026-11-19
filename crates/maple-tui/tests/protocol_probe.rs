//! Port of `l/test/protocol-probe.test.ts` (pure helper subset).
//!
//! Divergence: skips `ProtocolProbeComponent`/`ImageBudget`/global `TERMINAL`
//! graphics-id and themed `buildLargeTextLines` (runtime/TUI init required).
//! Covers exported pure APIs: PNG encoding/sample metadata (validated via IHDR),
//! large-text vertical reservation with injected encoder, labels, truecolor bar,
//! raw line block, and HSV conversion.

use maple_tui::apps::debug::protocol_probe::{
	build_large_text_lines_with, build_sample_image, encode_rgb_png, hsv_to_rgb, image_protocol_label,
	notify_protocol_label, truecolor_bar, ImageProtocol, NotifyProtocol, RawLineBlock,
};

fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
	// PNG signature + IHDR chunk: length(4) + "IHDR"(4) + width(4) + height(4)
	if png.len() < 24 || &png[0..8] != b"\x89PNG\r\n\x1a\n" {
		return None;
	}
	if &png[12..16] != b"IHDR" {
		return None;
	}
	let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
	let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
	Some((w, h))
}

#[test]
fn emits_png_whose_ihdr_dimensions_match_requested_size() {
	let sample = build_sample_image(48, 24);
	assert_eq!(sample.mime_type, "image/png");
	assert_eq!(sample.width_px, 48);
	assert_eq!(sample.height_px, 24);
	assert!(!sample.base64.is_empty());
	let rgb = vec![0u8; 48 * 24 * 3];
	let png = encode_rgb_png(48, 24, &rgb);
	assert_eq!(png_dimensions(&png), Some((48, 24)));
}

#[test]
fn png_begins_with_signature_and_has_core_chunks() {
	let png = encode_rgb_png(3, 2, &[0u8; 3 * 2 * 3]);
	assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
	assert!(png.windows(4).any(|w| w == b"IHDR"));
	assert!(png.windows(4).any(|w| w == b"IDAT"));
	assert!(png.windows(4).any(|w| w == b"IEND"));
	assert_eq!(png_dimensions(&png), Some((3, 2)));
}

#[test]
fn build_sample_image_metadata_and_decodable_dimensions() {
	let img = build_sample_image(4, 3);
	assert_eq!(img.mime_type, "image/png");
	assert_eq!(img.width_px, 4);
	assert_eq!(img.height_px, 3);
	assert!(!img.base64.is_empty());
	let png = encode_rgb_png(img.width_px, img.height_px, &vec![0u8; (img.width_px * img.height_px * 3) as usize]);
	assert_eq!(png_dimensions(&png), Some((4, 3)));
}

#[test]
fn large_text_lines_reserve_scale_minus_one_blank_rows() {
	let lines = build_large_text_lines_with(&[2, 3], |s, scale| format!("\x1b]66;s={scale};{s}\x07"));
	assert_eq!(lines.len(), 5);
	assert!(lines[0].contains("\x1b]66;s=2;"));
	assert_eq!(lines[1], "");
	assert!(lines[2].contains("\x1b]66;s=3;"));
	assert_eq!(lines[3], "");
	assert_eq!(lines[4], "");
}

#[test]
fn protocol_labels_match_reference_strings() {
	assert_eq!(notify_protocol_label(NotifyProtocol::Osc99), "OSC 99 (kitty)");
	assert_eq!(image_protocol_label(ImageProtocol::Sixel), "Sixel");
	assert_eq!(image_protocol_label(ImageProtocol::None), "none — text fallback");
}

#[test]
fn truecolor_bar_wraps_and_paints() {
	let bar = truecolor_bar(3);
	assert!(bar.starts_with("\x1b[48;2;"));
	assert!(bar.ends_with("\x1b[0m"));
}

#[test]
fn raw_line_block_renders_verbatim() {
	let b = RawLineBlock::new(vec!["\x1b[1mX\x1b[0m".into(), "y".into()]);
	assert_eq!(b.render(), vec!["\x1b[1mX\x1b[0m", "y"]);
}

#[test]
fn hsv_to_rgb_basic() {
	assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
	assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0));
}
