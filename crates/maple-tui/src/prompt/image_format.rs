//! Reference: `l/src/prompt/image-format.ts`.
//!
//! Safe file extensions for image sidecars: normalize a caller-supplied
//! extension, or derive one from an image MIME type.
//!
//! Deviation: the TypeScript reaches its MIME table with a case-insensitive
//! object lookup; Rust lowercases first and matches the same keys explicitly.

const IMAGE_EXTENSION_BY_MIME: [(&str, &str); 6] = [
	("image/png", "png"),
	("image/jpeg", "jpg"),
	("image/jpg", "jpg"),
	("image/gif", "gif"),
	("image/webp", "webp"),
	("image/svg+xml", "svg"),
];

/// Normalize a safe sidecar extension for image display files.
///
/// Strips one leading `.`, rejects empty/oversized (`>32`) and any name that is
/// not `[a-zA-Z0-9][a-zA-Z0-9._-]*`, then lowercases.
pub fn normalize_blob_extension(extension: Option<&str>) -> Option<String> {
	let extension = extension?;
	let normalized = extension.strip_prefix('.').unwrap_or(extension);
	if normalized.is_empty() || normalized.len() > 32 {
		return None;
	}
	let mut chars = normalized.chars();
	let first = chars.next()?;
	if !first.is_ascii_alphanumeric() {
		return None;
	}
	let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
	if !rest_ok {
		return None;
	}
	Some(normalized.to_ascii_lowercase())
}

/// Resolve a safe display-file extension from an image MIME type.
pub fn blob_extension_for_image_mime_type(mime_type: Option<&str>) -> Option<String> {
	let mime_type = mime_type?;
	let lower = mime_type.to_ascii_lowercase();
	if let Some((_, known)) = IMAGE_EXTENSION_BY_MIME.iter().find(|(key, _)| *key == lower) {
		return Some((*known).to_string());
	}
	let subtype = lower.strip_prefix("image/")?;
	let subtype = subtype.split(';').next().unwrap_or("");
	let subtype = subtype.split('+').next().unwrap_or("");
	normalize_blob_extension(Some(subtype))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalizes_extensions() {
		let cases: [(&str, Option<&str>); 8] = [
			("png", Some("png")),
			(".PNG", Some("png")),
			("", None),
			("..", None),
			("-x", None),
			("a b", None),
			("tar.gz", Some("tar.gz")),
			(&"x".repeat(33), None),
		];
		for (input, expected) in cases {
			assert_eq!(
				normalize_blob_extension(Some(input)).as_deref(),
				expected,
				"input {input:?}"
			);
		}
		assert_eq!(normalize_blob_extension(None), None);
	}

	#[test]
	fn resolves_mime_extensions() {
		let cases: [(&str, Option<&str>); 8] = [
			("image/png", Some("png")),
			("IMAGE/JPEG", Some("jpg")),
			("image/jpg", Some("jpg")),
			("image/svg+xml", Some("svg")),
			("image/avif", Some("avif")),
			("image/x-icon;charset=utf8", Some("x-icon")),
			("text/plain", None),
			("", None),
		];
		for (input, expected) in cases {
			assert_eq!(
				blob_extension_for_image_mime_type(Some(input)).as_deref(),
				expected,
				"mime {input:?}"
			);
		}
		assert_eq!(blob_extension_for_image_mime_type(None), None);
	}
}
