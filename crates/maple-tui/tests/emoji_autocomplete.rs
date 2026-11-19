//! Port of `l/test/emoji-autocomplete.test.ts`.

use maple_tui::autocomplete::AutocompleteItem;
use maple_tui::prompt::emoji_autocomplete::{
	apply_emoji_completion, get_emoji_suggestions, try_emoji_inline_replace,
};

#[test]
fn returns_none_for_empty_query() {
	assert!(get_emoji_suggestions(":").is_none());
	assert!(get_emoji_suggestions("note:").is_none());
}

#[test]
fn returns_prefix_matches_at_line_start() {
	let (items, prefix) = get_emoji_suggestions(":joy").expect("suggestions");
	assert_eq!(prefix, ":joy");
	assert!(items.iter().any(|item| item.label.contains(":joy:")));
	assert!(items.iter().all(|item| item.label.contains(":joy")));
}

#[test]
fn returns_prefix_matches_after_whitespace() {
	let (items, prefix) = get_emoji_suggestions("hello :sm").expect("suggestions");
	assert_eq!(prefix, ":sm");
	assert!(!items.is_empty());
}

#[test]
fn does_not_trigger_when_colon_is_mid_token() {
	assert!(get_emoji_suggestions("http://example").is_none());
	assert!(get_emoji_suggestions("foo:bar").is_none());
}

#[test]
fn returns_none_for_unknown_prefix() {
	assert!(get_emoji_suggestions(":zzzzzz").is_none());
}

#[test]
fn excludes_regional_indicator_flag_sequences() {
	assert!(get_emoji_suggestions(":jordan").is_none());
}

#[test]
fn caps_the_suggestion_count() {
	let (items, _) = get_emoji_suggestions(":a").expect("suggestions");
	assert!(items.len() <= 12);
}

#[test]
fn inline_replace_returns_none_without_trailing_colon() {
	assert!(try_emoji_inline_replace(":joy").is_none());
	assert!(try_emoji_inline_replace("hello").is_none());
}

#[test]
fn inline_replace_returns_replacement_for_valid_closing_form() {
	assert_eq!(
		try_emoji_inline_replace(":joy:"),
		Some((5, "😂".to_string()))
	);
}

#[test]
fn inline_replace_returns_replacement_when_preceded_by_whitespace() {
	assert_eq!(
		try_emoji_inline_replace("hi :tada:"),
		Some((6, "🎉".to_string()))
	);
}

#[test]
fn inline_replace_returns_none_for_unknown_name() {
	assert!(try_emoji_inline_replace(":notrealemoji:").is_none());
}

#[test]
fn inline_replace_returns_none_when_colon_is_mid_word() {
	assert!(try_emoji_inline_replace("foo:joy:").is_none());
}

#[test]
fn inline_replace_returns_none_for_filtered_flag_shortcodes() {
	assert!(try_emoji_inline_replace(":jordan:").is_none());
}

#[test]
fn apply_completion_replaces_the_prefix_with_the_emoji() {
	let item = AutocompleteItem::new("😂", "😂  :joy:");
	let (lines, cursor_line, cursor_col) =
		apply_emoji_completion(&["hello :joy".to_string()], 0, 10, &item, ":joy");
	assert_eq!(lines, vec!["hello 😂".to_string()]);
	assert_eq!(cursor_line, 0);
	assert_eq!(cursor_col, "hello ".len() + "😂".len());
}
