//! Terminal protocol smoke-test panel for the debug menu.
//!
//! Reference: `l/src/apps/debug/protocol-probe.ts`.
//!
//! Exercises escape protocols the renderer can emit: SGR styling + 24-bit
//! truecolor, OSC 8 hyperlinks, OSC 66 text sizing, inline graphics, and
//! desktop notifications (reported by caller).
//!
//! Deviations:
//! - TS uses `node:zlib`, `Bun.hash.crc32`, `Buffer`, `@oh-my-pi/pi-utils/color`,
//!   and TUI components (`Container`, `Text`, `Spacer`, `Image`, borders,
//!   theme/terminal capabilities). No new deps allowed.
//! - PNG encoding is implemented here with a minimal CRC32 and uncompressed
//!   DEFLATE blocks for IDAT (valid PNG, no compression library). Base64 is
//!   implemented locally. HSV->RGB is implemented locally.
//! - `ProtocolProbeComponent` and `RawLines` are represented as data/render
//!   descriptors ([`ProtocolProbeModel`], [`RawLineBlock`]) instead of
//!   `@oh-my-pi/pi-tui` components; terminal capability/theme rendering is
//!   injected/approximated for portability and tests.

/// Encoded image and geometry used by terminal protocol probes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleImage {
	pub base64: String,
	pub mime_type: String,
	pub width_px: u32,
	pub height_px: u32,
}

/// CRC32 (ISO 3309 / PNG).
fn crc32(bytes: &[u8]) -> u32 {
	let mut crc: u32 = 0xFFFF_FFFF;
	for &b in bytes {
		crc ^= b as u32;
		for _ in 0..8 {
			let mask = (crc & 1).wrapping_neg();
			crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
		}
	}
	!crc
}

fn png_chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
	let mut out = Vec::with_capacity(12 + data.len());
	out.extend_from_slice(&(data.len() as u32).to_be_bytes());
	out.extend_from_slice(ty);
	out.extend_from_slice(data);
	let mut crc_input = Vec::with_capacity(4 + data.len());
	crc_input.extend_from_slice(ty);
	crc_input.extend_from_slice(data);
	out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
	out
}

/// Adler-32 checksum for zlib wrapper.
fn adler32(data: &[u8]) -> u32 {
	let mut a: u32 = 1;
	let mut b: u32 = 0;
	for &x in data {
		a = (a + x as u32) % 65521;
		b = (b + a) % 65521;
	}
	(b << 16) | a
}

/// Wrap raw bytes in a zlib stream using uncompressed DEFLATE blocks.
fn zlib_store(data: &[u8]) -> Vec<u8> {
	let mut out = Vec::new();
	// CMF/FLG: deflate, 32K window, default compression check bits.
	out.extend_from_slice(&[0x78, 0x01]);
	let mut pos = 0;
	while pos < data.len() {
		let remaining = data.len() - pos;
		let chunk = remaining.min(65535);
		let bfinal = if pos + chunk >= data.len() { 1u8 } else { 0u8 };
		out.push(bfinal); // BFINAL=final, BTYPE=00 (no compression)
		let len = chunk as u16;
		let nlen = !len;
		out.extend_from_slice(&len.to_le_bytes());
		out.extend_from_slice(&nlen.to_le_bytes());
		out.extend_from_slice(&data[pos..pos + chunk]);
		pos += chunk;
	}
	if data.is_empty() {
		out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
	}
	out.extend_from_slice(&adler32(data).to_be_bytes());
	out
}

/// Encode raw 8-bit RGB pixels (`width * height * 3` bytes, row-major) as PNG.
pub fn encode_rgb_png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
	let stride = (width as usize) * 3;
	let mut raw = Vec::with_capacity((stride + 1) * (height as usize));
	for y in 0..(height as usize) {
		raw.push(0); // filter type None
		let start = y * stride;
		let end = start + stride;
		if end <= rgb.len() {
			raw.extend_from_slice(&rgb[start..end]);
		} else if start < rgb.len() {
			raw.extend_from_slice(&rgb[start..]);
			raw.extend(std::iter::repeat(0).take(end.saturating_sub(rgb.len())));
		} else {
			raw.extend(std::iter::repeat(0).take(stride));
		}
	}
	let mut ihdr = [0u8; 13];
	ihdr[0..4].copy_from_slice(&width.to_be_bytes());
	ihdr[4..8].copy_from_slice(&height.to_be_bytes());
	ihdr[8] = 8; // bit depth
	ihdr[9] = 2; // truecolor RGB
				 // compression/filter/interlace = 0
	let idat = zlib_store(&raw);
	let mut out = Vec::new();
	out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
	out.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
	out.extend_from_slice(&png_chunk(b"IDAT", &idat));
	out.extend_from_slice(&png_chunk(b"IEND", &[]));
	out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
	let mut out = String::with_capacity(((data.len() + 2) / 3) * 4);
	let mut i = 0;
	while i < data.len() {
		let b0 = data[i] as u32;
		let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
		let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
		let triple = (b0 << 16) | (b1 << 8) | b2;
		out.push(B64[((triple >> 18) & 63) as usize] as char);
		out.push(B64[((triple >> 12) & 63) as usize] as char);
		out.push(if i + 1 < data.len() { B64[((triple >> 6) & 63) as usize] as char } else { '=' });
		out.push(if i + 2 < data.len() { B64[(triple & 63) as usize] as char } else { '=' });
		i += 3;
	}
	out
}

pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
	let h = ((h % 360.0) + 360.0) % 360.0;
	let c = v * s;
	let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
	let m = v - c;
	let (r1, g1, b1) = match (h / 60.0) as i32 {
		0 => (c, x, 0.0),
		1 => (x, c, 0.0),
		2 => (0.0, c, x),
		3 => (0.0, x, c),
		4 => (x, 0.0, c),
		_ => (c, 0.0, x),
	};
	(
		((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
		((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
		((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
	)
}

/// Build a deterministic RGB gradient PNG (red across, green down, constant blue).
pub fn build_sample_image(width: u32, height: u32) -> SampleImage {
	let w = width.max(1) as usize;
	let h = height.max(1) as usize;
	let denom_x = (w.saturating_sub(1)).max(1) as f64;
	let denom_y = (h.saturating_sub(1)).max(1) as f64;
	let mut rgb = vec![0u8; w * h * 3];
	for y in 0..h {
		for x in 0..w {
			let i = (y * w + x) * 3;
			rgb[i] = ((x as f64 / denom_x) * 255.0).round() as u8;
			rgb[i + 1] = ((y as f64 / denom_y) * 255.0).round() as u8;
			rgb[i + 2] = 128;
		}
	}
	let png = encode_rgb_png(width.max(1), height.max(1), &rgb);
	SampleImage {
		base64: base64_encode(&png),
		mime_type: "image/png".to_string(),
		width_px: width.max(1),
		height_px: height.max(1),
	}
}

pub const LARGE_TEXT_SAMPLE: &str = "Aa Bb 123";

/// Text sizing scale for OSC 66 samples.
pub type TextSizingScale = u8;

/// Build large-text sample lines (one scaled span per scale, with vertical
/// reservation blanks). `encode` provides the terminal-specific sizing escape;
/// deviation: caller supplies encoder because theme/terminal utils vary.
pub fn build_large_text_lines_with<F>(scales: &[TextSizingScale], mut encode: F) -> Vec<String>
where
	F: FnMut(&str, TextSizingScale) -> String,
{
	let mut lines = Vec::new();
	for &scale in scales {
		let sample = format!("{LARGE_TEXT_SAMPLE} ({scale}x)");
		lines.push(format!("  {}", encode(&sample, scale)));
		for _ in 1..scale {
			lines.push(String::new());
		}
	}
	lines
}

/// A 24-bit-color hue sweep rendered as background-painted cells.
pub fn truecolor_bar(cells: usize) -> String {
	let mut out = String::new();
	let cells_f = cells.max(1) as f64;
	for i in 0..cells {
		let (r, g, b) = hsv_to_rgb((i as f64 / cells_f) * 360.0, 0.85, 1.0);
		out.push_str(&format!("\x1b[48;2;{r};{g};{b}m "));
	}
	out.push_str("\x1b[0m");
	out
}

/// Notification protocol labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyProtocol {
	Osc99,
	Osc9,
	Bel,
}

/// Image protocol labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageProtocol {
	Kitty,
	Iterm2,
	Sixel,
	None,
}

pub fn notify_protocol_label(p: NotifyProtocol) -> &'static str {
	match p {
		NotifyProtocol::Osc99 => "OSC 99 (kitty)",
		NotifyProtocol::Osc9 => "OSC 9 (iTerm2/WezTerm)",
		NotifyProtocol::Bel => "BEL",
	}
}

pub fn image_protocol_label(p: ImageProtocol) -> &'static str {
	match p {
		ImageProtocol::Kitty => "Kitty graphics",
		ImageProtocol::Iterm2 => "iTerm2 inline images",
		ImageProtocol::Sixel => "Sixel",
		ImageProtocol::None => "none — text fallback",
	}
}

/// Raw ANSI/escape lines that must reach the terminal byte-for-byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawLineBlock {
	pub lines: Vec<String>,
}

impl RawLineBlock {
	pub fn new(lines: Vec<String>) -> Self {
		Self { lines }
	}
	pub fn render(&self) -> Vec<String> {
		self.lines.clone()
	}
}

/// Image budget and notification state for the protocol panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolProbeOptions {
	pub image: SampleImage,
	pub notification_suppressed: bool,
}

/// Protocol probe model (data-oriented component stand-in).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolProbeModel {
	pub hyperlinks: bool,
	pub text_sizing: bool,
	pub true_color: bool,
	pub notify: NotifyProtocol,
	pub image_proto: ImageProtocol,
	pub options: ProtocolProbeOptions,
}

impl ProtocolProbeModel {
	pub fn image_protocol_label(&self) -> &'static str {
		image_protocol_label(self.image_proto)
	}
	pub fn notify_protocol_label(&self) -> &'static str {
		notify_protocol_label(self.notify)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn crc32_known_vector() {
		// "123456789" -> 0xCBF43926
		assert_eq!(crc32(b"123456789"), 0xCBF43926);
	}

	#[test]
	fn base64_encode_known() {
		assert_eq!(base64_encode(b"Man"), "TWFu");
		assert_eq!(base64_encode(b"Ma"), "TWE=");
		assert_eq!(base64_encode(b"M"), "TQ==");
	}

	#[test]
	fn png_has_signature_and_chunks() {
		let png = encode_rgb_png(2, 2, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]);
		assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]));
		assert!(png.windows(4).any(|w| w == b"IHDR"));
		assert!(png.windows(4).any(|w| w == b"IDAT"));
		assert!(png.windows(4).any(|w| w == b"IEND"));
	}

	#[test]
	fn build_sample_image_metadata() {
		let img = build_sample_image(4, 3);
		assert_eq!(img.mime_type, "image/png");
		assert_eq!(img.width_px, 4);
		assert_eq!(img.height_px, 3);
		assert!(!img.base64.is_empty());
	}

	#[test]
	fn large_text_lines_reserve_vertical_space() {
		let lines = build_large_text_lines_with(&[2, 3], |s, _| s.to_string());
		// scale 2 -> 1 line + 1 blank, scale 3 -> 1 line + 2 blanks
		assert_eq!(lines.len(), 5);
		assert!(lines[0].contains("(2x)"));
		assert_eq!(lines[1], "");
		assert!(lines[2].contains("(3x)"));
	}

	#[test]
	fn truecolor_bar_wraps_and_paints() {
		let bar = truecolor_bar(3);
		assert!(bar.starts_with("\x1b[48;2;"));
		assert!(bar.ends_with("\x1b[0m"));
	}

	#[test]
	fn protocol_labels() {
		assert_eq!(notify_protocol_label(NotifyProtocol::Osc99), "OSC 99 (kitty)");
		assert_eq!(image_protocol_label(ImageProtocol::Sixel), "Sixel");
		assert_eq!(image_protocol_label(ImageProtocol::None), "none — text fallback");
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
}
