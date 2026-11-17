//! `xd://` virtual tool-device URL parsing.
//!
//! Reference: `l/src/tools/xd-url.ts`.

/// Canonical prefix for virtual tool-device URLs.
pub const XD_URL_PREFIX: &str = "xd://";

/// Returns the canonical tool name from a bare or `xd://`-prefixed spelling.
pub fn strip_xd_url_prefix(name: &str) -> &str {
	if name.len() >= XD_URL_PREFIX.len()
		&& name[..XD_URL_PREFIX.len()].eq_ignore_ascii_case(XD_URL_PREFIX)
	{
		&name[XD_URL_PREFIX.len()..]
	} else {
		name
	}
}

/// Returns `name` without an underscore-spelled `xd://` prefix (`xd_...`).
pub fn strip_xd_underscore_prefix(name: &str) -> &str {
	let bytes = name.as_bytes();
	if bytes.len() < 3 {
		return name;
	}
	if !(bytes[0] == b'x' || bytes[0] == b'X') || !(bytes[1] == b'd' || bytes[1] == b'D') {
		return name;
	}
	if bytes[2] != b'_' {
		return name;
	}
	let mut i = 3;
	while i < bytes.len() && bytes[i] == b'_' {
		i += 1;
	}
	&name[i..]
}

/// Parsed `xd://` device URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdUrl {
	/// Device name; `None` for root `xd://`.
	pub name: Option<String>,
}

/// Parse an `xd://` URL into its device target.
/// Returns `None` for other or malformed URLs and `Some { name: None }` for root.
pub fn parse_xd_url(input: &str) -> Option<XdUrl> {
	let trimmed = input.trim();
	if trimmed.len() < XD_URL_PREFIX.len()
		|| !trimmed[..XD_URL_PREFIX.len()].eq_ignore_ascii_case(XD_URL_PREFIX)
	{
		return None;
	}
	let name = &trimmed[XD_URL_PREFIX.len()..];
	if name.is_empty() {
		return Some(XdUrl { name: None });
	}
	if name.contains('/') || name.contains('?') || name.contains('#') {
		return None;
	}
	Some(XdUrl {
		name: Some(name.to_string()),
	})
}

/// Parsed read-only `xd://<tool>/<topic>` documentation URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdTopicUrl {
	/// Device/tool name.
	pub name: String,
	/// Topic segment.
	pub topic: String,
}

/// Parse a read-only `xd://<tool>/<topic>` documentation URL.
pub fn parse_xd_topic_url(input: &str) -> Option<XdTopicUrl> {
	let trimmed = input.trim();
	if trimmed.len() < XD_URL_PREFIX.len()
		|| !trimmed[..XD_URL_PREFIX.len()].eq_ignore_ascii_case(XD_URL_PREFIX)
	{
		return None;
	}
	let rest = &trimmed[XD_URL_PREFIX.len()..];
	if rest.is_empty() || rest.contains('?') || rest.contains('#') {
		return None;
	}
	let mut parts = rest.splitn(3, '/');
	let name = parts.next()?;
	let topic = parts.next()?;
	if name.is_empty() || topic.is_empty() || parts.next().is_some() {
		return None;
	}
	Some(XdTopicUrl {
		name: name.to_string(),
		topic: topic.to_string(),
	})
}

/// Whether a streaming path prefix could still become an `xd://` URL.
pub fn could_become_xd_url(partial_path: &str) -> bool {
	if partial_path.len() <= XD_URL_PREFIX.len() {
		XD_URL_PREFIX
			.to_ascii_lowercase()
			.starts_with(&partial_path.to_ascii_lowercase())
	} else {
		partial_path
			.to_ascii_lowercase()
			.starts_with(&XD_URL_PREFIX.to_ascii_lowercase())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn strip_prefixes() {
		assert_eq!(strip_xd_url_prefix("xd://ast_edit"), "ast_edit");
		assert_eq!(strip_xd_url_prefix("XD://debug"), "debug");
		assert_eq!(strip_xd_url_prefix("debug"), "debug");
		assert_eq!(strip_xd_underscore_prefix("xd_recall"), "recall");
		assert_eq!(strip_xd_underscore_prefix("XD___recall"), "recall");
		assert_eq!(strip_xd_underscore_prefix("recall"), "recall");
	}

	#[test]
	fn parse_device_and_root() {
		assert_eq!(
			parse_xd_url("xd://ast_edit"),
			Some(XdUrl {
				name: Some("ast_edit".into())
			})
		);
		assert_eq!(parse_xd_url("xd://"), Some(XdUrl { name: None }));
		assert!(parse_xd_url("xd://a/b").is_none());
		assert!(parse_xd_url("local://x").is_none());
	}

	#[test]
	fn parse_topic() {
		assert_eq!(
			parse_xd_topic_url("xd://read/urls"),
			Some(XdTopicUrl {
				name: "read".into(),
				topic: "urls".into()
			})
		);
		assert!(parse_xd_topic_url("xd://read").is_none());
		assert!(parse_xd_topic_url("xd://read/urls/more").is_none());
	}

	#[test]
	fn could_become() {
		assert!(could_become_xd_url("x"));
		assert!(could_become_xd_url("xd:/"));
		assert!(could_become_xd_url("xd://foo"));
		assert!(!could_become_xd_url("local://"));
	}
}
