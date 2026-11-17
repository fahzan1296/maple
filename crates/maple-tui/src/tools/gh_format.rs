//! GitHub argument/renderer formatting helpers.
//!
//! Reference: `l/src/tools/gh-format.ts`.
//!
//! Deviation: the reference's `string | number | boolean | undefined` metadata
//! value is generic in Rust (`Option<T: ToString>`), and a missing value is
//! `None` rather than `undefined`.

/// Number of hex characters kept by [`format_short_sha`].
pub const SHORT_SHA_LENGTH: usize = 12;

/// Return the first 12 hex characters of a commit SHA, or `None` when the
/// input is missing. Short inputs are returned whole, matching `slice(0, 12)`.
pub fn format_short_sha(value: Option<&str>) -> Option<&str> {
	let value = match value {
		Some(value) if !value.is_empty() => value,
		_ => return None,
	};
	if value.len() <= SHORT_SHA_LENGTH {
		return Some(value);
	}
	value
		.char_indices()
		.nth(SHORT_SHA_LENGTH)
		.map(|(index, _)| &value[..index])
		.or(Some(value))
}

/// Append a labeled Markdown metadata value when present. Empty strings are
/// skipped, as the reference skips `""`.
pub fn push_line<T: ToString>(lines: &mut Vec<String>, label: &str, value: Option<T>) {
	let Some(value) = value else { return };
	let text = value.to_string();
	if text.is_empty() {
		return;
	}
	lines.push(format!("{label}: {text}"));
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn short_sha_keeps_twelve_hex_characters() {
		assert_eq!(
			format_short_sha(Some("0123456789abcdef")),
			Some("0123456789ab")
		);
	}

	#[test]
	fn short_sha_returns_short_input_whole() {
		assert_eq!(format_short_sha(Some("abc")), Some("abc"));
	}

	#[test]
	fn short_sha_missing_value_is_none() {
		assert_eq!(format_short_sha(None), None);
		assert_eq!(format_short_sha(Some("")), None);
	}

	#[test]
	fn push_line_appends_present_values() {
		let mut lines = Vec::new();
		push_line(&mut lines, "title", Some("Add feature"));
		push_line(&mut lines, "count", Some(3));
		push_line(&mut lines, "draft", Some(true));
		assert_eq!(lines, vec!["title: Add feature", "count: 3", "draft: true"]);
	}

	#[test]
	fn push_line_skips_missing_and_empty() {
		let mut lines = vec!["kept".to_string()];
		push_line::<String>(&mut lines, "empty", Some(""));
		push_line::<String>(&mut lines, "missing", None);
		assert_eq!(lines, vec!["kept"]);
	}
}
