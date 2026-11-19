//! Reference: `l/src/prompt/queue-input.ts`.
//!
//! The `->` / `=>` yield-queue shorthand, and splitting a sequential numeric,
//! Roman-numeral or alphabetic list into separate queued messages.
//!
//! Deviation: [`QUEUE_LIST_MARKER_RE`] drops the reference's trailing
//! `(?=[\t ]|$)` lookahead (unsupported by the `regex` crate); callers that need
//! the delimiter rule use [`queue_list_marker`], which applies it.

use std::sync::LazyLock;

use regex::Regex;

const QUEUE_PREFIXES: [&str; 2] = ["->", "=>"];

/// Prefix matcher shared by queue-list parsing and editor highlighting.
pub static QUEUE_LIST_MARKER_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^([\t ]*)(\d+|[A-Za-z]+)([.)])").expect("queue list marker regex")
});

static CANONICAL_ROMAN_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^(?=[MDCLXVI])M{0,3}(?:CM|CD|D?C{0,3})(?:XC|XL|L?X{0,3})(?:IX|IV|V?I{0,3})$")
		.expect("roman numeral regex")
});

/// One enumerated line: `1.`, `ii)`, `c.` …
#[derive(Clone, Debug, PartialEq, Eq)]
struct EnumeratedItem {
	line: usize,
	indent: String,
	marker: String,
	punctuation: String,
	content: String,
}

/// A parsed sequential list.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EnumeratedList {
	source: String,
	lines: Vec<String>,
	items: Vec<EnumeratedItem>,
}

/// Extract the message body from the `->` / `=>` yield-queue shorthand.
pub fn parse_queue_shorthand(text: &str) -> Option<String> {
	let prefix = QUEUE_PREFIXES.iter().find(|candidate| text.starts_with(**candidate))?;
	Some(text[prefix.len()..].trim().to_string())
}

/// Render messages as `->` / `=>` shorthand that [`parse_queue_shorthand`] and
/// [`split_queued_messages`] read back as the same messages.
pub fn format_queue_shorthand(messages: &[String]) -> String {
	if messages.len() == 1 {
		return format!("=> {}", messages[0]);
	}
	let rows: Vec<String> = messages
		.iter()
		.enumerate()
		.map(|(index, message)| format!("{}. {}", index + 1, message.replace('\n', "\n   ")))
		.collect();
	format!("=>\n{}", rows.join("\n"))
}

/// A marker match that honours the trailing delimiter: the character after the
/// punctuation must be a tab, space or end of line.
pub fn queue_list_marker(line: &str) -> Option<(String, String, String)> {
	let caps = QUEUE_LIST_MARKER_RE.captures(line)?;
	let whole = caps.get(0).expect("whole match");
	match line[whole.end()..].chars().next() {
		None | Some('\t') | Some(' ') => {}
		Some(_) => return None,
	}
	Some((
		caps.get(1).expect("indent").as_str().to_string(),
		caps.get(2).expect("marker").as_str().to_string(),
		caps.get(3).expect("punctuation").as_str().to_string(),
	))
}

fn parse_enumerated_item(line: &str, line_index: usize) -> Option<EnumeratedItem> {
	let (indent, marker, punctuation) = queue_list_marker(line)?;
	let matched_len = indent.len() + marker.len() + punctuation.len();
	Some(EnumeratedItem {
		line: line_index,
		indent,
		marker,
		punctuation,
		content: line[matched_len..].trim_start().to_string(),
	})
}

fn decode_decimal(marker: &str) -> Option<i64> {
	if marker.is_empty() || !marker.bytes().all(|b| b.is_ascii_digit()) {
		return None;
	}
	let value: i64 = marker.parse().ok()?;
	(value <= 9_007_199_254_740_991).then_some(value)
}

fn decode_roman(marker: &str) -> Option<i64> {
	if !CANONICAL_ROMAN_RE.is_match(marker) {
		return None;
	}
	let upper = marker.to_uppercase();
	let value_of = |ch: char| -> Option<i64> {
		Some(match ch {
			'I' => 1,
			'V' => 5,
			'X' => 10,
			'L' => 50,
			'C' => 100,
			'D' => 500,
			'M' => 1000,
			_ => return None,
		})
	};
	let chars: Vec<char> = upper.chars().collect();
	let mut value = 0i64;
	for (index, ch) in chars.iter().enumerate() {
		let current = value_of(*ch)?;
		let next = chars.get(index + 1).and_then(|c| value_of(*c)).unwrap_or(0);
		value += if current < next { -current } else { current };
	}
	Some(value)
}

fn decode_alpha(marker: &str) -> Option<i64> {
	if marker.is_empty() || !marker.chars().all(|ch| ch.is_ascii_alphabetic()) {
		return None;
	}
	let mut value = 0i64;
	for ch in marker.to_uppercase().chars() {
		value = value * 26 + (ch as i64 - 64);
		if value > 9_007_199_254_740_991 {
			return None;
		}
	}
	Some(value)
}

fn is_sequential(markers: &[String], decode: impl Fn(&str) -> Option<i64>) -> bool {
	let Some(mut previous) = markers.first().and_then(|marker| decode(marker)) else {
		return false;
	};
	for marker in &markers[1..] {
		let Some(current) = decode(marker) else {
			return false;
		};
		if current != previous + 1 {
			return false;
		}
		previous = current;
	}
	true
}

fn is_enumerated_sequence(items: &[EnumeratedItem]) -> bool {
	let markers: Vec<String> = items.iter().map(|item| item.marker.clone()).collect();
	if markers.iter().all(|marker| marker.bytes().all(|b| b.is_ascii_digit())) {
		return is_sequential(&markers, decode_decimal);
	}
	let all_upper = markers.iter().all(|marker| marker.to_uppercase() == *marker);
	let all_lower = markers.iter().all(|marker| marker.to_lowercase() == *marker);
	if !all_upper && !all_lower {
		return false;
	}
	is_sequential(&markers, decode_roman) || is_sequential(&markers, decode_alpha)
}

fn parse_enumerated_list(text: &str) -> Option<EnumeratedList> {
	let source = text.trim().to_string();
	if source.is_empty() {
		return None;
	}
	let lines: Vec<String> = source
		.split('\n')
		.map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
		.collect();
	let first = parse_enumerated_item(&lines[0], 0)?;

	let mut items = vec![first.clone()];
	for (line_index, line) in lines.iter().enumerate().skip(1) {
		if let Some(item) = parse_enumerated_item(line, line_index) {
			if item.indent == first.indent {
				items.push(item);
			}
		}
	}
	if items.len() < 2
		|| items
			.iter()
			.any(|item| item.punctuation != first.punctuation)
		|| !is_enumerated_sequence(&items)
	{
		return None;
	}
	Some(EnumeratedList {
		source,
		lines,
		items,
	})
}

/// Whether text currently forms a sequential queue list, including an
/// unfinished trailing item.
pub fn is_queued_message_list(text: &str) -> bool {
	parse_enumerated_list(text).is_some()
}

/// Split a sequential numeric, Roman-numeral, or alphabetic list into queue
/// entries.
pub fn split_queued_messages(text: &str) -> Vec<String> {
	let Some(list) = parse_enumerated_list(text) else {
		let source = text.trim();
		return if source.is_empty() {
			Vec::new()
		} else {
			vec![source.to_string()]
		};
	};

	let mut messages: Vec<String> = list
		.items
		.iter()
		.enumerate()
		.map(|(index, item)| {
			let next_line = list
				.items
				.get(index + 1)
				.map(|next| next.line)
				.unwrap_or(list.lines.len());
			let mut body = vec![item.content.clone()];
			body.extend(list.lines[item.line + 1..next_line].iter().cloned());
			body.join("\n").trim().to_string()
		})
		.collect();
	while messages.last().is_some_and(String::is_empty) {
		messages.pop();
	}
	if !messages.is_empty() && messages.iter().all(|message| !message.is_empty()) {
		messages
	} else {
		vec![list.source]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn owned(messages: &[&str]) -> Vec<String> {
		messages.iter().map(|m| (*m).to_string()).collect()
	}

	#[test]
	fn parses_queue_shorthand() {
		let cases: [(&str, Option<&str>); 5] = [
			("-> hello", Some("hello")),
			("=>  spaced  ", Some("spaced")),
			("->", Some("")),
			("plain", None),
			("=> multiline\nbody", Some("multiline\nbody")),
		];
		for (text, expected) in cases {
			assert_eq!(parse_queue_shorthand(text).as_deref(), expected, "text {text:?}");
		}
	}

	#[test]
	fn formats_round_trippable_shorthand() {
		assert_eq!(format_queue_shorthand(&owned(&["one"])), "=> one");
		assert_eq!(
			format_queue_shorthand(&owned(&["one", "two\nlines"])),
			"=>\n1. one\n2. two\n   lines"
		);
	}

	#[test]
	fn splits_decimal_roman_and_alpha_lists() {
		let cases: [(&str, Vec<&str>); 5] = [
			("1. one\n2. two", vec!["one", "two"]),
			("i. one\nii. two", vec!["one", "two"]),
			("A) one\nB) two", vec!["one", "two"]),
			("1) one\n3) three", vec!["1) one\n3) three"]),
			("just a message", vec!["just a message"]),
		];
		for (text, expected) in cases {
			assert_eq!(split_queued_messages(text), owned(&expected), "text {text:?}");
		}
	}

	#[test]
	fn keeps_continuation_lines_with_their_item() {
		let text = "1. first\nmore\n2. second";
		assert_eq!(split_queued_messages(text), owned(&["first\nmore", "second"]));
		assert!(is_queued_message_list(text));
		assert!(!is_queued_message_list("single line"));
	}

	#[test]
	fn trailing_item_without_body_is_dropped() {
		let text = "1. a\n2.";
		assert_eq!(split_queued_messages(text), owned(&["1. a\n2."]));
		assert!(is_queued_message_list(text));
	}

	#[test]
	fn marker_requires_trailing_delimiter() {
		assert!(queue_list_marker("1. x").is_some());
		assert!(queue_list_marker("1.").is_some());
		assert!(queue_list_marker("1.x").is_none());
		assert!(queue_list_marker("1.\tx").is_some());
	}
}
