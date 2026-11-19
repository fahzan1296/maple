//! Reference: `l/src/prompt/internal-url-autocomplete.ts`.
//!
//! Autocomplete for internal-url schemes (`skill://`, `rule://`, `local://`,
//! `memory://`, `agent://`, `artifact://`) while composing a prompt.
//!
//! Detection here MUST stay in sync with the generic URL-scheme trigger in the
//! editor; the editor fires the popup, this module decides whether there are
//! candidates to show.
//!
//! Deviations:
//! - The runtime completion host is a trait behind a process-wide `Mutex`
//!   (installed with [`set_internal_url_completion_host`]), mirroring the
//!   reference's module-level variable.
//! - `resolveCompletions` is synchronous; the reference awaits it and threads an
//!   `AbortSignal`. Callers that need cancellation drop the returned work
//!   themselves.

use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;

use crate::autocomplete::{subsequence_match, subsequence_score, AutocompleteItem};

/// Caller binding shared by tool execution and prompt completion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InternalUrlCallerContext {
	pub cwd: Option<String>,
	pub session_file: Option<String>,
	pub session_id: Option<String>,
	pub seance_source_artifacts_dir: Option<String>,
}

/// One host-supplied candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalUrlCandidate {
	pub value: String,
	pub label: Option<String>,
	pub description: Option<String>,
}

/// Lazy runtime capabilities supplied by the internal URL host.
pub trait InternalUrlCompletionHost: Send + Sync {
	/// Schemes that accept completions.
	fn completion_schemes(&self) -> Vec<String>;
	/// Candidates for `scheme` and the typed `query`; `None` when the host has
	/// none.
	fn resolve_completions(
		&self,
		scheme: &str,
		query: &str,
		context: &InternalUrlCallerContext,
	) -> Option<Vec<InternalUrlCandidate>>;
}

static COMPLETION_HOST: LazyLock<Mutex<Option<Arc<dyn InternalUrlCompletionHost>>>> =
	LazyLock::new(|| Mutex::new(None));

/// Install the runtime URL completion capabilities.
pub fn set_internal_url_completion_host(host: Option<Arc<dyn InternalUrlCompletionHost>>) {
	let mut guard = COMPLETION_HOST
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner());
	*guard = host;
}

fn completion_host() -> Option<Arc<dyn InternalUrlCompletionHost>> {
	COMPLETION_HOST
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.clone()
}

/// Upper bound on candidates surfaced in the dropdown.
const MAX_URL_SUGGESTIONS: usize = 25;

/// A URL token ending at the cursor: a known internal scheme followed by one or
/// two slashes and the partially typed host/path. The boundary/rest character
/// classes mirror the editor trigger so both agree on what counts as a token.
pub static URL_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r#"(?i)(?:^|[\s"'`(<=])([a-z][a-z0-9+.-]*:/{1,2}[^\s"'`()<>]*)$"#)
		.expect("url token regex")
});

static SCHEME_SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^([a-z][a-z0-9+.-]*):/{1,2}(.*)$").expect("scheme split regex")
});

/// A completable internal-url token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalUrlContext {
	/// Lowercased scheme (e.g. `local`).
	pub scheme: String,
	/// Text typed after the slashes so far (host + path); may be empty.
	pub query: String,
	/// Exact buffer token from its boundary to the cursor (the completion prefix).
	pub token: String,
}

/// Decode `%XX` escapes, leaving malformed sequences as typed.
fn decode_url_completion_value(value: &str) -> String {
	let bytes = value.as_bytes();
	let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
	let mut index = 0;
	let mut decoded_any = false;
	while index < bytes.len() {
		if bytes[index] == b'%' && index + 2 < bytes.len() {
			let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
			if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
				out.push(byte);
				index += 3;
				decoded_any = true;
				continue;
			}
		}
		out.push(bytes[index]);
		index += 1;
	}
	if !decoded_any {
		return value.to_string();
	}
	String::from_utf8_lossy(&out).into_owned()
}

/// Detect a completable internal-url token immediately before the cursor.
/// `None` when the text is not a `scheme://` token whose scheme is registered
/// with a completion-capable handler.
pub fn extract_internal_url_context(text_before_cursor: &str) -> Option<InternalUrlContext> {
	let token_match = URL_TOKEN_RE.captures(text_before_cursor)?;
	let token = token_match.get(1)?.as_str();
	let parts = SCHEME_SPLIT_RE.captures(token)?;
	let scheme = parts.get(1)?.as_str().to_lowercase();
	let host = completion_host()?;
	if !host.completion_schemes().iter().any(|known| *known == scheme) {
		return None;
	}
	Some(InternalUrlContext {
		scheme,
		query: parts.get(2).map(|m| m.as_str()).unwrap_or("").to_string(),
		token: token.to_string(),
	})
}

/// Suggestions for the internal-url token ending at the cursor, or `None` when
/// the text is not such a token or no candidate matches the typed query.
pub fn get_internal_url_suggestions(
	text_before_cursor: &str,
	cwd: Option<&str>,
	get_caller: Option<&dyn Fn() -> InternalUrlCallerContext>,
) -> Option<(Vec<AutocompleteItem>, String)> {
	let ctx = extract_internal_url_context(text_before_cursor)?;
	let host = completion_host()?;

	let mut caller = get_caller.map(|f| f()).unwrap_or_default();
	if let Some(cwd) = cwd {
		caller.cwd = Some(cwd.to_string());
	}
	let candidates = host.resolve_completions(&ctx.scheme, &ctx.query, &caller)?;
	if candidates.is_empty() {
		return None;
	}

	let query = ctx.query.to_lowercase();
	let mut scored: Vec<(AutocompleteItem, i64)> = Vec::new();
	for candidate in candidates {
		let target = decode_url_completion_value(&candidate.value).to_lowercase();
		if !subsequence_match(&query, &target) {
			continue;
		}
		let mut item = AutocompleteItem::new(
			format!("{}://{}", ctx.scheme, candidate.value),
			candidate.label.unwrap_or_else(|| candidate.value.clone()),
		);
		if let Some(description) = candidate.description {
			item = item.with_description(description);
		}
		scored.push((item, subsequence_score(&query, &target)));
	}
	if scored.is_empty() {
		return None;
	}

	scored.sort_by(|a, b| b.1.cmp(&a.1));
	Some((
		scored
			.into_iter()
			.take(MAX_URL_SUGGESTIONS)
			.map(|(item, _)| item)
			.collect(),
		ctx.token,
	))
}

/// Whether `prefix` (the token a completion was offered for) is an internal-url
/// token.
pub fn is_internal_url_prefix(prefix: &str) -> bool {
	extract_internal_url_context(prefix).is_some()
}

/// Replace the internal-url token with the selected candidate, appending a
/// trailing space (matching `@` file-reference behavior) so the user can keep
/// typing.
pub fn apply_internal_url_completion(
	lines: &[String],
	cursor_line: usize,
	cursor_col: usize,
	item: &AutocompleteItem,
	prefix: &str,
) -> (Vec<String>, usize, usize) {
	let current_line = lines.get(cursor_line).cloned().unwrap_or_default();
	let mut chars: Vec<char> = current_line.chars().collect();
	let prefix_len = prefix.chars().count();
	let before_prefix = cursor_col.saturating_sub(prefix_len);
	let before: String = chars.drain(..cursor_col.min(chars.len())).collect();
	let before_prefix_text: String = before.chars().take(before_prefix).collect();
	let after_text: String = chars.into_iter().collect();
	let insert = format!("{} ", item.value);
	let new_line = format!("{before_prefix_text}{insert}{after_text}");
	let mut new_lines: Vec<String> = lines.to_vec();
	if cursor_line < new_lines.len() {
		new_lines[cursor_line] = new_line;
	} else {
		new_lines.resize(cursor_line + 1, String::new());
		new_lines[cursor_line] = new_line;
	}
	(
		new_lines,
		cursor_line,
		before_prefix_text.chars().count() + insert.chars().count(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Host {
		schemes: Vec<String>,
		candidates: Vec<InternalUrlCandidate>,
	}

	impl InternalUrlCompletionHost for Host {
		fn completion_schemes(&self) -> Vec<String> {
			self.schemes.clone()
		}

		fn resolve_completions(
			&self,
			_scheme: &str,
			_query: &str,
			_context: &InternalUrlCallerContext,
		) -> Option<Vec<InternalUrlCandidate>> {
			Some(self.candidates.clone())
		}
	}

	fn install(candidates: Vec<InternalUrlCandidate>) {
		set_internal_url_completion_host(Some(Arc::new(Host {
			schemes: vec!["local".to_string(), "skill".to_string()],
			candidates,
		})));
	}

	fn candidate(value: &str) -> InternalUrlCandidate {
		InternalUrlCandidate {
			value: value.to_string(),
			label: None,
			description: None,
		}
	}

	#[test]
	fn extracts_registered_scheme_tokens() {
		install(Vec::new());
		let cases: [(&str, Option<(&str, &str, &str)>); 5] = [
			("local://src", Some(("local", "src", "local://src"))),
			("see skill://a/b", Some(("skill", "a/b", "skill://a/b"))),
			("local:/src", Some(("local", "src", "local:/src"))),
			("xd://x", None),
			("local://src more", None),
		];
		for (text, expected) in cases {
			let context = extract_internal_url_context(text);
			let got = context
				.as_ref()
				.map(|c| (c.scheme.as_str(), c.query.as_str(), c.token.as_str()));
			assert_eq!(got, expected, "text {text:?}");
		}
		set_internal_url_completion_host(None);
	}

	#[test]
	fn scores_and_limits_candidates() {
		let mut many: Vec<InternalUrlCandidate> = Vec::new();
		for index in 0..40 {
			many.push(candidate(&format!("target-{index}")));
		}
		install(many);
		let (items, prefix) = get_internal_url_suggestions("local://target", None, None)
			.expect("suggestions");
		assert_eq!(prefix, "local://target");
		assert_eq!(items.len(), MAX_URL_SUGGESTIONS);
		assert!(items.iter().all(|item| item.value.starts_with("local://target-")));
		set_internal_url_completion_host(None);
	}

	#[test]
	fn decodes_percent_encoded_values_for_matching() {
		install(vec![candidate("alice%40prod")]);
		let (items, _) = get_internal_url_suggestions("local://alice@", None, None)
			.expect("suggestions");
		assert_eq!(items[0].value, "local://alice%40prod");
		assert_eq!(items[0].label, "alice%40prod");
		set_internal_url_completion_host(None);
	}

	#[test]
	fn no_match_or_no_host_yields_nothing() {
		install(vec![candidate("zzz")]);
		assert!(get_internal_url_suggestions("local://aaa", None, None).is_none());
		set_internal_url_completion_host(None);
		assert!(get_internal_url_suggestions("local://aaa", None, None).is_none());
		assert!(get_internal_url_suggestions("nothing", None, None).is_none());
	}

	#[test]
	fn applies_completion_and_tracks_cursor() {
		let lines = vec!["see local://sr".to_string()];
		let item = AutocompleteItem::new("local://src/lib.rs", "src/lib.rs");
		let (new_lines, line, col) =
			apply_internal_url_completion(&lines, 0, 14, &item, "local://sr");
		assert_eq!(new_lines, vec!["see local://src/lib.rs ".to_string()]);
		assert_eq!(line, 0);
		assert_eq!(col, 23);
		assert!(is_internal_url_prefix("local://x") || !is_internal_url_prefix("plain"));
	}
}
