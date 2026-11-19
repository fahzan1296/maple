//! Pictures an HTML form shows: `<img src>` loaded from a URL, a `data:` URI or
//! a local file, and inline `<svg>` drawings, as bytes plus a MIME type —
//! a raster (`image/png`, `image/jpeg`, `image/gif`, `image/webp`) or
//! `image/svg+xml`.
//!
//! Reference: `l/src/html-form/media.ts`.
//!
//! Deviations:
//! - The TypeScript implementation is async (`fetch`, `Bun.file`) and supports
//!   `AbortSignal` cancellation/timeouts; this port is synchronous and uses
//!   `std::fs`/path resolution. No async runtime is added.
//! - Remote `http(s)` loading is not implemented (no HTTP client dependency in
//!   this crate); callers receive an error for remote URLs.
//! - `parseImageMetadata` from `@oh-my-pi/pi-utils` is replaced with small
//!   in-crate header sniffing for PNG/JPEG/GIF/WebP.
//! - `file:` URL handling accepts plain local paths after stripping the scheme
//!   prefix (no full URL parser dependency).

use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};

/// An image's bytes and MIME type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormMedia {
	pub bytes: Vec<u8>,
	pub mime: String,
}

/// Where relative paths resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormMediaOptions {
	pub cwd: PathBuf,
}

/// Largest picture a form loads.
pub const MAX_FORM_IMAGE_BYTES: usize = 10 * 1024 * 1024;

const SVG_MIME: &str = "image/svg+xml";
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Load an `<img src>`: a `data:` URI (base64 or URL-encoded), a `file:` URL,
/// or a path (`~/`-relative, absolute, or relative to `cwd`).
///
/// Remote `http(s)` URLs are not supported in this port (documented deviation).
///
/// # Errors
/// Returns an error if the source cannot be read, exceeds
/// [`MAX_FORM_IMAGE_BYTES`], or is not a PNG, JPEG, GIF, WebP or SVG image.
pub fn load_form_image(src: &str, options: &FormMediaOptions) -> Result<FormMedia, String> {
	let bytes = read_source(src, options)?;
	if bytes.len() > MAX_FORM_IMAGE_BYTES {
		return Err(format!("larger than {} MiB", MAX_FORM_IMAGE_BYTES >> 20));
	}
	if let Some(mime) = raster_mime(&bytes) {
		return Ok(FormMedia { bytes, mime: mime.to_string() });
	}
	let head_len = bytes.len().min(4096);
	let head = String::from_utf8_lossy(&bytes[..head_len]);
	if contains_svg_tag(&head) {
		let markup = String::from_utf8_lossy(&bytes).into_owned();
		return Ok(svg_media(&markup));
	}
	Err("not a PNG, JPEG, GIF, WebP or SVG image".to_string())
}

/// An `<svg>` as standalone media: the SVG namespace added when missing, and a
/// drawing sized only by its `viewBox` given that size as `width`/`height`, since
/// renderers drop a drawing without one.
pub fn svg_media(markup: &str) -> FormMedia {
	let open = find_svg_open_tag(markup);
	let Some((start, end)) = open else {
		return FormMedia {
			bytes: markup.as_bytes().to_vec(),
			mime: SVG_MIME.to_string(),
		};
	};
	let open_tag = &markup[start..end];
	let mut added = Vec::new();
	if !has_xml_attr(open_tag, "xmlns") {
		added.push(format!("xmlns=\"{SVG_NAMESPACE}\""));
	}
	if let Some((w, h)) = view_box_size(open_tag) {
		if !has_xml_attr(open_tag, "width") && !has_xml_attr(open_tag, "height") {
			added.push(format!("width=\"{w}\""));
			added.push(format!("height=\"{h}\""));
		}
	}
	let sized = if added.is_empty() {
		markup.to_string()
	} else {
		let insert_at = start + "<svg".len();
		let mut out = String::with_capacity(markup.len() + added.len() * 24);
		out.push_str(&markup[..insert_at]);
		out.push(' ');
		out.push_str(&added.join(" "));
		out.push_str(&markup[insert_at..]);
		out
	};
	FormMedia {
		bytes: sized.into_bytes(),
		mime: SVG_MIME.to_string(),
	}
}

/// Whether `media` is an SVG drawing rather than a raster.
pub fn is_svg_media(media: &FormMedia) -> bool {
	media.mime == SVG_MIME
}

fn read_source(src: &str, options: &FormMediaOptions) -> Result<Vec<u8>, String> {
	if src.len() >= 5 && src[..5].eq_ignore_ascii_case("data:") {
		return decode_data_uri(src);
	}
	if src.len() >= 7 && src[..7].eq_ignore_ascii_case("http://")
		|| src.len() >= 8 && src[..8].eq_ignore_ascii_case("https://")
	{
		return Err("http(s) image loading is not supported in this port".to_string());
	}
	let path = if src.len() >= 5 && src[..5].eq_ignore_ascii_case("file:") {
		file_url_to_path(src)
	} else {
		resolve_path(src, &options.cwd)
	};
	let meta = fs::metadata(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
	if meta.len() as usize > MAX_FORM_IMAGE_BYTES {
		return Err(format!("larger than {} MiB", MAX_FORM_IMAGE_BYTES >> 20));
	}
	fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn resolve_path(src: &str, cwd: &Path) -> PathBuf {
	if src == "~" {
		if let Some(home) = home_dir() {
			return home;
		}
	}
	if let Some(rest) = src.strip_prefix("~/") {
		if let Some(home) = home_dir() {
			return home.join(rest);
		}
	}
	let path = Path::new(src);
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		cwd.join(path)
	}
}

fn home_dir() -> Option<PathBuf> {
	std::env::var_os("HOME").map(PathBuf::from)
}

fn file_url_to_path(src: &str) -> PathBuf {
	let rest = &src[5..]; // "file:"
	let path = if let Some(p) = rest.strip_prefix("//localhost") {
		p
	} else if let Some(p) = rest.strip_prefix("//") {
		// file://host/path - treat remainder after authority as path if present
		match p.find('/') {
			Some(idx) => &p[idx..],
			None => p,
		}
	} else {
		rest
	};
	PathBuf::from(percent_decode_path(path))
}

fn percent_decode_path(path: &str) -> String {
	let bytes = path.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'%' && i + 2 < bytes.len() {
			if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
				out.push((hi << 4) | lo);
				i += 3;
				continue;
			}
		}
		out.push(bytes[i]);
		i += 1;
	}
	String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
	match b {
		b'0'..=b'9' => Some(b - b'0'),
		b'a'..=b'f' => Some(b - b'a' + 10),
		b'A'..=b'F' => Some(b - b'A' + 10),
		_ => None,
	}
}

/// The payload of a `data:` URI; base64 when marked so, else percent-encoded text.
fn decode_data_uri(uri: &str) -> Result<Vec<u8>, String> {
	let comma = uri.find(',').ok_or_else(|| "malformed data: URI".to_string())?;
	let header = &uri[5..comma];
	let payload = &uri[comma + 1..];
	if header.to_ascii_lowercase().ends_with(";base64") {
		let compact: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
		base64::engine::general_purpose::STANDARD
			.decode(compact.as_bytes())
			.or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(compact.as_bytes()))
			.map_err(|e| format!("malformed data: URI: {e}"))
	} else {
		Ok(percent_decode_bytes(payload))
	}
}

fn percent_decode_bytes(s: &str) -> Vec<u8> {
	let bytes = s.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'%' && i + 2 < bytes.len() {
			if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
				out.push((hi << 4) | lo);
				i += 3;
				continue;
			}
		}
		out.push(bytes[i]);
		i += 1;
	}
	out
}

fn raster_mime(bytes: &[u8]) -> Option<&'static str> {
	if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
		return Some("image/png");
	}
	if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
		return Some("image/jpeg");
	}
	if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
		return Some("image/gif");
	}
	if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
		return Some("image/webp");
	}
	None
}

fn contains_svg_tag(text: &str) -> bool {
	let lower = text.to_ascii_lowercase();
	lower
		.find("<svg")
		.is_some_and(|idx| matches!(lower.as_bytes().get(idx + 4), Some(b' ' | b'>' | b'\n' | b'\r' | b'\t' | b'/')))
}

fn find_svg_open_tag(markup: &str) -> Option<(usize, usize)> {
	let lower = markup.to_ascii_lowercase();
	let start = lower.find("<svg")?;
	let rest = &markup[start..];
	let end_rel = rest.find('>')?;
	Some((start, start + end_rel + 1))
}

fn has_xml_attr(tag: &str, name: &str) -> bool {
	let lower = tag.to_ascii_lowercase();
	let needle = format!(" {name}");
	if lower.contains(&(needle.clone() + "=")) || lower.contains(&(needle.clone() + " =")) {
		return true;
	}
	// also at start after <svg
	let after = lower.strip_prefix("<svg").unwrap_or(&lower);
	after
		.trim_start()
		.starts_with(&format!("{name}="))
		|| after.trim_start().starts_with(&format!("{name} ="))
}

fn view_box_size(tag: &str) -> Option<(String, String)> {
	// Extract viewBox="minx miny w h" or with commas
	let lower = tag.to_ascii_lowercase();
	let key = "viewbox";
	let idx = lower.find(key)?;
	let rest = &tag[idx + key.len()..];
	let rest = rest.trim_start();
	let rest = rest.strip_prefix('=')?.trim_start();
	let (quote, rest) = match rest.as_bytes().first().copied() {
		Some(b'"') => ('"', &rest[1..]),
		Some(b'\'') => ('\'', &rest[1..]),
		_ => return None,
	};
	let end = rest.find(quote)?;
	let value = &rest[..end];
	let parts: Vec<&str> = value
		.split(|c: char| c.is_whitespace() || c == ',')
		.filter(|s| !s.is_empty())
		.collect();
	if parts.len() != 4 {
		return None;
	}
	Some((parts[2].to_string(), parts[3].to_string()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	#[test]
	fn detects_raster_mime_from_magic_bytes() {
		assert_eq!(raster_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), Some("image/png"));
		assert_eq!(raster_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
		assert_eq!(raster_mime(b"GIF89a123"), Some("image/gif"));
		assert_eq!(raster_mime(b"RIFFxxxxWEBP"), Some("image/webp"));
		assert_eq!(raster_mime(b"not image"), None);
	}

	#[test]
	fn decodes_base64_and_percent_data_uris() {
		let b64 = decode_data_uri("data:image/png;base64,aGVsbG8=").expect("b64");
		assert_eq!(b64, b"hello");
		let pct = decode_data_uri("data:text/plain,hi%20there").expect("pct");
		assert_eq!(pct, b"hi there");
	}

	#[test]
	fn svg_media_adds_namespace_and_viewbox_size() {
		let media = svg_media("<svg viewBox=\"0 0 10 20\"><rect/></svg>");
		assert!(is_svg_media(&media));
		let text = String::from_utf8(media.bytes).expect("utf8");
		assert!(text.contains(&format!("xmlns=\"{SVG_NAMESPACE}\"")));
		assert!(text.contains("width=\"10\""));
		assert!(text.contains("height=\"20\""));
	}

	#[test]
	fn svg_media_preserves_existing_dimensions_and_namespace() {
		let src = "<svg xmlns=\"x\" viewBox=\"0 0 1 2\" width=\"5\"><rect/></svg>";
		let media = svg_media(src);
		let text = String::from_utf8(media.bytes).expect("utf8");
		assert_eq!(text.matches("xmlns=").count(), 1);
		assert!(!text.contains("height=\"2\""));
		assert!(text.contains("width=\"5\""));
	}

	#[test]
	fn load_form_image_reads_local_file_and_detects_svg() {
		let dir = std::env::temp_dir().join("maple_tui_html_form_media_test");
		let _ = fs::create_dir_all(&dir);
		let svg_path = dir.join("a.svg");
		fs::write(&svg_path, b"<svg viewBox='0 0 3 4'></svg>").expect("write");
		let media = load_form_image(
			svg_path.to_str().expect("path"),
			&FormMediaOptions { cwd: dir.clone() },
		)
		.expect("load");
		assert!(is_svg_media(&media));
		assert_eq!(media.mime, SVG_MIME);

		let png_path = dir.join("b.bin");
		fs::write(&png_path, [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).expect("write png");
		let media = load_form_image(
			"b.bin",
			&FormMediaOptions { cwd: dir.clone() },
		)
		.expect("load png");
		assert_eq!(media.mime, "image/png");

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn load_form_image_rejects_remote_and_oversize_and_bad_data() {
		let err = load_form_image("https://example.com/x.png", &FormMediaOptions { cwd: PathBuf::from(".") })
			.unwrap_err();
		assert!(err.contains("not supported"));
		assert!(decode_data_uri("data:broken").is_err());
	}

	#[test]
	fn resolves_tilde_and_relative_paths() {
		let cwd = PathBuf::from("/tmp/work");
		assert_eq!(resolve_path("a/b.png", &cwd), PathBuf::from("/tmp/work/a/b.png"));
		assert_eq!(resolve_path("/abs/x.png", &cwd), PathBuf::from("/abs/x.png"));
	}
}
