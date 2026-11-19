//! `cfg://` settings URL parsing.
//!
//! Reference: `l/src/tools/cfg-url.ts`.

/// Canonical prefix for settings URLs.
pub const CFG_URL_PREFIX: &str = "cfg://";

/// Trailing write segment that persists a `cfg://` change instead of scoping it
/// to the session.
pub const CFG_SAVE_SEGMENT: &str = "save";

/// Setting addressed by a `cfg://` URL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CfgTarget {
	/// Setting path segments; empty for the root. `/` and `.` both separate
	/// segments.
	pub segments: Vec<String>,
	/// Whether the URL ends in the `/save` suffix, which is removed from
	/// `segments`.
	pub save: bool,
}

/// Parse a `cfg://` URL (`cfg://advisor/enabled`, `cfg://advisor.enabled/save`).
/// Returns `None` for other schemes; query and fragment are ignored.
pub fn parse_cfg_url(input: &str) -> Option<CfgTarget> {
	let trimmed = input.trim();
	if !trimmed.to_lowercase().starts_with(CFG_URL_PREFIX) {
		return None;
	}
	let mut segments: Vec<String> = trimmed
		[CFG_URL_PREFIX.len()..]
		.split(['?', '#'])
		.next()
		.unwrap_or("")
		.split(['/', '.'])
		.filter(|segment| !segment.is_empty())
		.map(str::to_string)
		.collect();
	let save = segments.len() > 1 && segments.last().is_some_and(|last| last == CFG_SAVE_SEGMENT);
	if save {
		segments.pop();
	}
	Some(CfgTarget { segments, save })
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_slash_and_dot_segments() {
		assert_eq!(
			parse_cfg_url("cfg://advisor/enabled"),
			Some(CfgTarget {
				segments: vec!["advisor".into(), "enabled".into()],
				save: false,
			})
		);
		assert_eq!(
			parse_cfg_url("cfg://advisor.enabled"),
			Some(CfgTarget {
				segments: vec!["advisor".into(), "enabled".into()],
				save: false,
			})
		);
	}

	#[test]
	fn parses_save_suffix() {
		assert_eq!(
			parse_cfg_url("cfg://advisor/enabled/save"),
			Some(CfgTarget {
				segments: vec!["advisor".into(), "enabled".into()],
				save: true,
			})
		);
	}

	#[test]
	fn root_and_repeated_separators() {
		assert_eq!(
			parse_cfg_url("cfg://"),
			Some(CfgTarget {
				segments: Vec::new(),
				save: false,
			})
		);
		assert_eq!(
			parse_cfg_url("cfg://a//b/"),
			Some(CfgTarget {
				segments: vec!["a".into(), "b".into()],
				save: false,
			})
		);
	}

	#[test]
	fn ignores_query_and_fragment() {
		assert_eq!(
			parse_cfg_url("cfg://advisor/enabled?x=1#frag"),
			Some(CfgTarget {
				segments: vec!["advisor".into(), "enabled".into()],
				save: false,
			})
		);
	}

	#[test]
	fn rejects_other_schemes() {
		assert_eq!(parse_cfg_url("https://example.com"), None);
		assert_eq!(parse_cfg_url("cfg:/advisor"), None);
	}

	#[test]
	fn prefix_match_is_case_insensitive() {
		assert!(parse_cfg_url("CFG://advisor").is_some());
	}

	#[test]
	fn lone_save_is_not_a_suffix() {
		assert_eq!(
			parse_cfg_url("cfg://save"),
			Some(CfgTarget {
				segments: vec!["save".into()],
				save: false,
			})
		);
	}
}
