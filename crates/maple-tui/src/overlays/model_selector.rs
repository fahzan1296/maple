//! Reference: `l/src/overlays/model-selector.ts`.
//!
//! Thinking-suffix and upstream-routing parsing for model selectors. The
//! reference reads the `ThinkingLevel` enum from `@oh-my-pi/pi-agent-core`;
//! this port uses [`crate::thinking`], which already carries the same levels
//! and the `auto` sentinel.

use crate::thinking::{parse_thinking_level, ConfiguredThinkingLevel, ThinkingLevel, AUTO_THINKING};
use regex::Regex;
use std::sync::LazyLock;

/// Opt-in selectors that can otherwise be literal model-id suffixes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThinkingSuffixOptions {
	pub allow_max_suffix: bool,
	pub allow_auto_alias: bool,
}

impl ThinkingSuffixOptions {
	/// Options accepting every configured effort selector.
	#[must_use]
	pub const fn all() -> Self {
		ThinkingSuffixOptions {
			allow_max_suffix: true,
			allow_auto_alias: true,
		}
	}
}

/// Recognize all configured effort selectors after literal-id matching.
pub const MAX_THINKING_SUFFIX_OPTIONS: ThinkingSuffixOptions = ThinkingSuffixOptions::all();

/// Selector parsing options with authoritative literal-model lookup.
pub struct ModelStringParseOptions<'a> {
	/// Suffix recognition flags.
	pub suffix: ThinkingSuffixOptions,
	/// Whether `provider/id` is an exact registered model id.
	pub is_literal_model_id: Option<&'a dyn Fn(&str, &str) -> bool>,
}

impl Default for ModelStringParseOptions<'_> {
	fn default() -> Self {
		ModelStringParseOptions {
			suffix: ThinkingSuffixOptions::default(),
			is_literal_model_id: None,
		}
	}
}

fn configured(level: ThinkingLevel) -> ConfiguredThinkingLevel {
	match level {
		ThinkingLevel::Inherit => ConfiguredThinkingLevel::Inherit,
		ThinkingLevel::Off => ConfiguredThinkingLevel::Off,
		ThinkingLevel::Minimal => ConfiguredThinkingLevel::Minimal,
		ThinkingLevel::Low => ConfiguredThinkingLevel::Low,
		ThinkingLevel::Medium => ConfiguredThinkingLevel::Medium,
		ThinkingLevel::High => ConfiguredThinkingLevel::High,
		ThinkingLevel::XHigh => ConfiguredThinkingLevel::XHigh,
		ThinkingLevel::Max => ConfiguredThinkingLevel::Max,
	}
}

/// Parse a suffix while preserving selectors not explicitly enabled.
#[must_use]
pub fn parse_thinking_suffix(
	value: &str,
	options: Option<ThinkingSuffixOptions>,
) -> Option<ConfiguredThinkingLevel> {
	if let Some(level) = parse_thinking_level(Some(value)) {
		if level == ThinkingLevel::Max {
			return options
				.filter(|o| o.allow_max_suffix)
				.map(|_| ConfiguredThinkingLevel::Max);
		}
		return Some(configured(level));
	}
	if options.is_some_and(|o| o.allow_auto_alias) && value == AUTO_THINKING.as_str() {
		return Some(AUTO_THINKING);
	}
	None
}

/// A model pattern split at a trailing thinking selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitThinkingSuffix {
	pub base: String,
	pub level: Option<ConfiguredThinkingLevel>,
}

/// Split a trailing `:<level>` thinking selector off a model pattern.
///
/// `level` is set when the suffix parses as a concrete thinking level (or,
/// when the caller opts in, the guarded `:max` level / `:auto` sentinel);
/// `base` then has the suffix stripped. `min_colon_index` requires the colon to
/// appear strictly after that index — role-alias callers pass the matched alias
/// prefix length.
#[must_use]
pub fn split_thinking_suffix(
	pattern: &str,
	min_colon_index: isize,
	options: Option<ThinkingSuffixOptions>,
) -> SplitThinkingSuffix {
	let Some(colon) = pattern.rfind(':') else {
		return SplitThinkingSuffix {
			base: pattern.to_string(),
			level: None,
		};
	};
	if (colon as isize) <= min_colon_index {
		return SplitThinkingSuffix {
			base: pattern.to_string(),
			level: None,
		};
	}
	match parse_thinking_suffix(&pattern[colon + 1..], options) {
		Some(level) => SplitThinkingSuffix {
			base: pattern[..colon].to_string(),
			level: Some(level),
		},
		None => SplitThinkingSuffix {
			base: pattern.to_string(),
			level: None,
		},
	}
}

/// A parsed `provider/modelId[:level]` selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedModelString {
	pub provider: String,
	pub id: String,
	pub thinking_level: Option<ConfiguredThinkingLevel>,
}

/// Parse a model string in `provider/modelId` format.
///
/// Returns `None` if the format is invalid.
#[must_use]
pub fn parse_model_string(
	model_str: &str,
	options: Option<&ModelStringParseOptions<'_>>,
) -> Option<ParsedModelString> {
	let slash = model_str.find('/')?;
	if slash == 0 {
		return None;
	}
	let id = &model_str[slash + 1..];
	let provider = &model_str[..slash];
	// Strip strict thinking level suffixes first
	// (`claude-sonnet-4-6:high` -> id `claude-sonnet-4-6`, level `high`).
	let strict = split_thinking_suffix(id, -1, None);
	if let Some(level) = strict.level {
		return Some(ParsedModelString {
			provider: provider.to_string(),
			id: strict.base,
			thinking_level: Some(level),
		});
	}
	// `max` is a real thinking level, but real model ids can also end in
	// `:max`. Context-aware callers pass a literal lookup so those models win.
	let max_alias = split_thinking_suffix(id, -1, options.map(|o| o.suffix));
	if let Some(level) = max_alias.level {
		let literal = options
			.and_then(|o| o.is_literal_model_id)
			.is_some_and(|f| f(provider, id));
		return Some(if literal {
			ParsedModelString {
				provider: provider.to_string(),
				id: id.to_string(),
				thinking_level: None,
			}
		} else {
			ParsedModelString {
				provider: provider.to_string(),
				id: max_alias.base,
				thinking_level: Some(level),
			}
		});
	}
	Some(ParsedModelString {
		provider: provider.to_string(),
		id: id.to_string(),
		thinking_level: None,
	})
}

/// Render a selector plus its thinking level for display/persistence.
#[must_use]
pub fn format_model_selector_value(
	selector: &str,
	thinking_level: Option<ConfiguredThinkingLevel>,
) -> String {
	match thinking_level {
		Some(level) if level != ConfiguredThinkingLevel::Inherit => {
			format!("{selector}:{}", level.as_str())
		}
		_ => selector.to_string(),
	}
}

/// Bare slug (`cerebras`) or tiered/regional slug
/// (`google-ai-studio/priority`, `google-vertex/global/flex`).
static UPSTREAM_ROUTING_SLUG: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:/[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)*\z")
		.expect("upstream routing slug regex")
});

/// A model pattern split at a trailing `@<upstream>` routing selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitUpstreamRouting {
	pub base: String,
	pub upstream: String,
}

/// Split a trailing `@<upstream>` provider-routing selector off a model pattern.
///
/// `openrouter/z-ai/glm-4.7@cerebras` -> base `openrouter/z-ai/glm-4.7`,
/// upstream `cerebras`. Tiered upstreams keep their path
/// (`...@google-ai-studio/priority`). A `:thinking` suffix after the slug is
/// kept on the base (`...@cerebras:high` -> base `...:high`). Returns `None`
/// when there is no `@` or the suffix is not a bare provider slug, so model ids
/// that legitimately contain `@` (`claude-opus-4-8@default`,
/// `workers-ai/@cf/...`) are never split.
#[must_use]
pub fn split_upstream_routing(pattern: &str) -> Option<SplitUpstreamRouting> {
	let at = pattern.rfind('@')?;
	if at == 0 {
		return None;
	}
	let rest = &pattern[at + 1..];
	let colon = rest.find(':');
	let upstream = colon.map_or(rest, |c| &rest[..c]);
	if !UPSTREAM_ROUTING_SLUG.is_match(upstream) {
		return None;
	}
	let trailing = colon.map_or("", |c| &rest[c..]);
	Some(SplitUpstreamRouting {
		base: format!("{}{trailing}", &pattern[..at]),
		upstream: upstream.to_string(),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn max_suffix_is_gated() {
		assert_eq!(parse_thinking_suffix("max", None), None);
		assert_eq!(
			parse_thinking_suffix("max", Some(MAX_THINKING_SUFFIX_OPTIONS)),
			Some(ConfiguredThinkingLevel::Max)
		);
		assert_eq!(
			parse_thinking_suffix("high", None),
			Some(ConfiguredThinkingLevel::High)
		);
		assert_eq!(parse_thinking_suffix("bogus", None), None);
	}

	#[test]
	fn auto_alias_is_gated() {
		assert_eq!(parse_thinking_suffix("auto", None), None);
		assert_eq!(
			parse_thinking_suffix(
				"auto",
				Some(ThinkingSuffixOptions {
					allow_max_suffix: false,
					allow_auto_alias: true,
				})
			),
			Some(AUTO_THINKING)
		);
	}

	#[test]
	fn thinking_suffix_respects_min_colon_index() {
		let split = split_thinking_suffix("provider/model:high", -1, None);
		assert_eq!(split.base, "provider/model");
		assert_eq!(split.level, Some(ConfiguredThinkingLevel::High));

		let kept = split_thinking_suffix("provider/model:high", 8, None);
		assert_eq!(kept.base, "provider/model:high");
		assert_eq!(kept.level, None);
	}

	#[test]
	fn parses_provider_model_and_strict_suffix() {
		let parsed = parse_model_string("anthropic/claude-sonnet-4-6:high", None).unwrap();
		assert_eq!(parsed.provider, "anthropic");
		assert_eq!(parsed.id, "claude-sonnet-4-6");
		assert_eq!(parsed.thinking_level, Some(ConfiguredThinkingLevel::High));

		assert!(parse_model_string("noslash", None).is_none());
		assert!(parse_model_string("/leading", None).is_none());
	}

	#[test]
	fn literal_model_id_beats_max_alias() {
		let literal = |provider: &str, id: &str| provider == "zai" && id == "glm-4.7:max";
		let options = ModelStringParseOptions {
			suffix: MAX_THINKING_SUFFIX_OPTIONS,
			is_literal_model_id: Some(&literal),
		};
		let parsed = parse_model_string("zai/glm-4.7:max", Some(&options)).unwrap();
		assert_eq!(parsed.id, "glm-4.7:max");
		assert_eq!(parsed.thinking_level, None);

		let parsed = parse_model_string("zai/glm-4.7:max", None).unwrap();
		assert_eq!(parsed.id, "glm-4.7:max");
		assert_eq!(parsed.thinking_level, None);

		let parsed = parse_model_string(
			"zai/glm-4.7:max",
			Some(&ModelStringParseOptions {
				suffix: MAX_THINKING_SUFFIX_OPTIONS,
				is_literal_model_id: None,
			}),
		)
		.unwrap();
		assert_eq!(parsed.id, "glm-4.7");
		assert_eq!(parsed.thinking_level, Some(ConfiguredThinkingLevel::Max));
	}

	#[test]
	fn formats_selector_without_inherit_suffix() {
		assert_eq!(
			format_model_selector_value("p/m", Some(ConfiguredThinkingLevel::High)),
			"p/m:high"
		);
		assert_eq!(
			format_model_selector_value("p/m", Some(ConfiguredThinkingLevel::Inherit)),
			"p/m"
		);
		assert_eq!(format_model_selector_value("p/m", None), "p/m");
	}

	#[test]
	fn splits_upstream_routing_slugs() {
		let split = split_upstream_routing("openrouter/z-ai/glm-4.7@cerebras").unwrap();
		assert_eq!(split.base, "openrouter/z-ai/glm-4.7");
		assert_eq!(split.upstream, "cerebras");

		let tiered = split_upstream_routing("openrouter/z-ai/glm-4.7@google-ai-studio/priority").unwrap();
		assert_eq!(tiered.upstream, "google-ai-studio/priority");

		let with_level = split_upstream_routing("openrouter/z-ai/glm-4.7@cerebras:high").unwrap();
		assert_eq!(with_level.base, "openrouter/z-ai/glm-4.7:high");
		assert_eq!(with_level.upstream, "cerebras");
	}

	#[test]
	fn rejects_non_slug_upstreams() {
		assert!(split_upstream_routing("claude-opus-4-8@default").is_none());
		assert!(split_upstream_routing("workers-ai/@cf/meta").is_none());
		assert!(split_upstream_routing("@cerebras").is_none());
		assert!(split_upstream_routing("provider/model").is_none());
	}
}
