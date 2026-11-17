//! Reference: `l/src/prompt/emoji-autocomplete.ts`.
//!
//! `:name:` shortcode completion, western emoticon completion, inline
//! `:name:`/emoticon replacement on commit, and submit-time emoticon expansion.
//!
//! The `:name:` buckets live in this crate's own `data/emojis.json`
//! (`{ "<first char>": [["<name>", "<emoji>"], ...] }`, pre-sorted by name): a
//! curated table written for this port, not the reference's full GitHub set.
//!
//! Deviations: indexes are `char` offsets, not UTF-16.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::autocomplete::AutocompleteItem;

type Entry = (String, String);

/// `data/emojis.json`: each bucket is pre-sorted by name.
static BUCKETS: LazyLock<HashMap<String, Vec<Entry>>> = LazyLock::new(|| {
	let raw: HashMap<String, Vec<Entry>> =
		serde_json::from_str(include_str!("data/emojis.json")).expect("emojis.json");
	raw
});

/// Western text emoticons, sorted longest-first so `:-)` wins over `:)`.
const EMOTICONS: &[(&str, &str)] = &[
	(":'-(", "😢"),
	(">:-(", "😠"),
	(":-)", "🙂"),
	(":-(", "🙁"),
	(":-D", "😃"),
	(":-P", "😛"),
	(":-p", "😛"),
	(":-O", "😮"),
	(":-o", "😮"),
	(":-|", "😐"),
	(":-/", "😕"),
	(":-\\", "😕"),
	(":-*", "😘"),
	(";-)", "😉"),
	(";-P", "😜"),
	(":')", "🥲"),
	(":'D", "😂"),
	(":'(", "😢"),
	("</3", "💔"),
	(">:(", "😠"),
	("B-)", "😎"),
	("8-)", "😎"),
	("o.O", "😳"),
	("O.o", "😳"),
	(":)", "🙂"),
	(":(", "🙁"),
	(":D", "😃"),
	(":P", "😛"),
	(":p", "😛"),
	(":O", "😮"),
	(":o", "😮"),
	(":|", "😐"),
	(":/", "😕"),
	(":\\", "😕"),
	(":*", "😘"),
	(";)", "😉"),
	(":3", "😺"),
	("<3", "❤️"),
	("xD", "😆"),
	("XD", "😆"),
];

const MAX_SUGGESTIONS: usize = 12;

fn lower_bound(arr: &[Entry], target: &str) -> usize {
	arr.partition_point(|(name, _)| name.as_str() < target)
}

/// Exact bucket lookup for an already-lowercased shortcode name.
fn lookup_exact(name: &str) -> Option<&'static str> {
	let bucket = BUCKETS.get(&name.chars().next()?.to_string())?;
	let hit = bucket.get(lower_bound(bucket, name))?;
	(hit.0 == name).then_some(hit.1.as_str())
}

/// GitHub/gemoji shortcode grammar: `a-z`, `A-Z`, `0-9`, `_`, `+`, `-`.
fn is_name_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-'
}

/// Token boundary left of `c`: start of string, whitespace, opening bracket or
/// `>` (quoted block).
fn has_left_boundary(chars: &[char], idx: usize) -> bool {
	match idx.checked_sub(1).and_then(|i| chars.get(i)) {
		None => true,
		Some(c) => matches!(c, ' ' | '\t' | '\n' | '\r' | '(' | '[' | '{' | '>'),
	}
}

struct EmojiTrigger {
	/// Full token including the leading `:`, e.g. `:joy`.
	prefix: String,
	/// Lowercased name portion, e.g. `joy`.
	query: String,
}

fn extract_trigger(text_before_cursor: &str) -> Option<EmojiTrigger> {
	let chars: Vec<char> = text_before_cursor.chars().collect();
	let mut i = chars.len();
	while i > 0 && is_name_char(chars[i - 1]) {
		i -= 1;
	}
	if i == 0 || chars[i - 1] != ':' {
		return None;
	}
	let colon_idx = i - 1;
	if !has_left_boundary(&chars, colon_idx) {
		return None;
	}
	let name: String = chars[i..].iter().collect();
	Some(EmojiTrigger {
		prefix: format!(":{name}"),
		query: name.to_lowercase(),
	})
}

/// `:name:` completions for the token at the cursor, or `None`.
pub fn get_emoji_suggestions(text_before_cursor: &str) -> Option<(Vec<AutocompleteItem>, String)> {
	let trigger = extract_trigger(text_before_cursor)?;
	// At least one letter: a bare `:` in prose (`note:`) must not spam the popup.
	if trigger.query.is_empty() {
		return None;
	}

	let mut items: Vec<AutocompleteItem> = Vec::new();

	// Emoticon literals (`:D`, `:-)`) whose pattern starts with `:<query>` come
	// first: the user sees what they are literally typing at the top.
	let wanted = format!(":{}", trigger.query);
	for (pattern, character) in EMOTICONS {
		if items.len() >= MAX_SUGGESTIONS {
			break;
		}
		if pattern.len() < wanted.len() {
			continue;
		}
		if !pattern.to_lowercase().starts_with(&wanted) {
			continue;
		}
		items.push(AutocompleteItem::new(*character, format!("{character}  {pattern}")));
	}

	let first = trigger.query.chars().next().map(|c| c.to_string()).unwrap_or_default();
	if let Some(bucket) = BUCKETS.get(&first) {
		let mut i = lower_bound(bucket, &trigger.query);
		while i < bucket.len() && items.len() < MAX_SUGGESTIONS {
			let (name, character) = &bucket[i];
			if !name.starts_with(&trigger.query) {
				break;
			}
			items.push(AutocompleteItem::new(
				character.as_str(),
				format!("{character}  :{name}:"),
			));
			i += 1;
		}
	}

	if items.is_empty() {
		return None;
	}
	Some((items, trigger.prefix))
}

/// Replace the live `:name` token with the selected emoji.
pub fn apply_emoji_completion(
	lines: &[String],
	cursor_line: usize,
	cursor_col: usize,
	item: &AutocompleteItem,
	prefix: &str,
) -> (Vec<String>, usize, usize) {
	let current_line = lines.get(cursor_line).cloned().unwrap_or_default();
	let chars: Vec<char> = current_line.chars().collect();
	let cursor_col = cursor_col.min(chars.len());
	let start = cursor_col.saturating_sub(prefix.chars().count());
	let before: String = chars[..start].iter().collect();
	let after: String = chars[cursor_col..].iter().collect();
	let new_line = format!("{before}{}{after}", item.value);
	let mut new_lines = lines.to_vec();
	if cursor_line < new_lines.len() {
		new_lines[cursor_line] = new_line;
	} else {
		new_lines.resize(cursor_line + 1, String::new());
		new_lines[cursor_line] = new_line;
	}
	(new_lines, cursor_line, start + item.value.chars().count())
}

fn try_shortcode_inline_replace(text_before_cursor: &str) -> Option<(usize, String)> {
	let chars: Vec<char> = text_before_cursor.chars().collect();
	let len = chars.len();
	// Only fires on a trailing `:`.
	if len == 0 || chars[len - 1] != ':' {
		return None;
	}
	let close_idx = len - 1;
	let mut name_start = close_idx;
	while name_start > 0 && is_name_char(chars[name_start - 1]) {
		name_start -= 1;
	}
	if name_start == close_idx {
		return None; // empty name (`::`)
	}
	if name_start == 0 || chars[name_start - 1] != ':' {
		return None;
	}
	let open_idx = name_start - 1;
	if !has_left_boundary(&chars, open_idx) {
		return None;
	}
	let name: String = chars[name_start..close_idx]
		.iter()
		.collect::<String>()
		.to_lowercase();
	let character = lookup_exact(&name)?;
	// Replace `:name:` (name + 2 colons).
	Some((name.chars().count() + 2, character.to_string()))
}

fn is_emoticon_terminator(c: char) -> bool {
	matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// Emoticons fire once a terminator follows; the terminator is preserved.
fn try_emoticon_inline_replace(text_before_cursor: &str) -> Option<(usize, String)> {
	let chars: Vec<char> = text_before_cursor.chars().collect();
	if chars.len() < 2 {
		return None;
	}
	let character_tail = chars[chars.len() - 1];
	if !is_emoticon_terminator(character_tail) {
		return None;
	}
	let tail = chars.len() - 1;
	for (pattern, emoji) in EMOTICONS {
		let plen = pattern.chars().count();
		if tail < plen {
			continue;
		}
		let start = tail - plen;
		let matched = pattern
			.chars()
			.zip(&chars[start..tail])
			.all(|(want, got)| want == *got);
		if !matched {
			continue;
		}
		// Same left-boundary rule as shortcodes: identifiers, URLs and code stay
		// untouched.
		if start > 0 && !has_left_boundary(&chars, start) {
			continue;
		}
		return Some((plen + 1, format!("{emoji}{character_tail}")));
	}
	None
}

/// Inline replace on commit: `:name:` shortcode, else a trailing emoticon.
/// Returns `(replace_len, insert)` in chars.
pub fn try_emoji_inline_replace(text_before_cursor: &str) -> Option<(usize, String)> {
	try_shortcode_inline_replace(text_before_cursor)
		.or_else(|| try_emoticon_inline_replace(text_before_cursor))
}

pub fn is_emoji_prefix(prefix: &str) -> bool {
	prefix.starts_with(':')
}

/// Submit-time pass: rewrite emoticons at token boundaries, catching a message
/// sent without a trailing space.
pub fn expand_emoticons(text: &str) -> String {
	let chars: Vec<char> = text.chars().collect();
	if chars.len() < 2 {
		return text.to_string();
	}
	let mut out = String::new();
	let mut cursor = 0usize;
	let mut i = 0usize;
	while i < chars.len() {
		if i == 0 || has_left_boundary(&chars, i) {
			let mut matched = false;
			for (pattern, emoji) in EMOTICONS {
				let plen = pattern.chars().count();
				if i + plen > chars.len() {
					continue;
				}
				if !pattern
					.chars()
					.zip(&chars[i..i + plen])
					.all(|(want, got)| want == *got)
				{
					continue;
				}
				let end = i + plen;
				if end != chars.len() && !is_emoticon_terminator(chars[end]) {
					continue;
				}
				out.extend(chars[cursor..i].iter());
				out.push_str(emoji);
				cursor = end;
				i = end;
				matched = true;
				break;
			}
			if matched {
				continue;
			}
		}
		i += 1;
	}
	if cursor == 0 {
		return text.to_string();
	}
	out.extend(chars[cursor..].iter());
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn values(items: &[AutocompleteItem]) -> Vec<&str> {
		items.iter().map(|item| item.value.as_str()).collect()
	}

	#[test]
	fn suggestions_require_a_letter_and_a_boundary() {
		assert!(get_emoji_suggestions("note:").is_none());
		assert!(get_emoji_suggestions("::").is_none());
		assert!(get_emoji_suggestions("x:joy").is_none());
		assert!(get_emoji_suggestions("say :j").is_some());
		assert!(get_emoji_suggestions("(:j").is_some());
	}

	#[test]
	fn suggestions_list_emoticons_first_then_names() {
		let (items, prefix) = get_emoji_suggestions("(:d").expect("suggestions");
		assert_eq!(prefix, ":d");
		assert_eq!(values(&items)[0], "😃");
		assert!(items.iter().any(|item| item.label.contains(":disappointed:")));
	}

	#[test]
	fn suggestions_cap_at_twelve() {
		let (items, _) = get_emoji_suggestions(":a").expect("suggestions");
		assert!(items.len() <= MAX_SUGGESTIONS);
	}

	#[test]
	fn applies_over_the_prefix() {
		let lines = vec!["say :jo".to_string()];
		let item = AutocompleteItem::new("😂", "😂  :joy:");
		let (new_lines, line, col) = apply_emoji_completion(&lines, 0, 7, &item, ":jo");
		assert_eq!(new_lines[0], "say 😂");
		assert_eq!((line, col), (0, 5));
	}

	#[test]
	fn inline_replaces_shortcode() {
		assert_eq!(
			try_emoji_inline_replace("hi :joy:"),
			Some((6, "😂".to_string()))
		);
		assert_eq!(try_emoji_inline_replace("hi :nope:"), None);
		assert_eq!(try_emoji_inline_replace("::"), None);
	}

	#[test]
	fn inline_replaces_emoticons_with_terminator() {
		assert_eq!(
			try_emoji_inline_replace("hi :-) "),
			Some((4, "🙂 ".to_string()))
		);
		assert_eq!(try_emoji_inline_replace("hi :-)"), None);
		// Embedded in an identifier stays untouched.
		assert_eq!(try_emoji_inline_replace("a:3 "), None);
	}

	#[test]
	fn expands_emoticons_at_boundaries() {
		assert_eq!(expand_emoticons("nice :)"), "nice 🙂");
		assert_eq!(expand_emoticons("a <3 b"), "a ❤️ b");
		assert_eq!(expand_emoticons("no emoticons"), "no emoticons");
		assert_eq!(expand_emoticons("<"), "<");
	}

	#[test]
	fn prefix_check() {
		assert!(is_emoji_prefix(":x"));
		assert!(!is_emoji_prefix("x"));
	}
}
