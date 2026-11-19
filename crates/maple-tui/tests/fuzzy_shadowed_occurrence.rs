//! Port of `l/test/fuzzy-shadowed-occurrence.test.ts`.

use maple_tui::fuzzy::fuzzy_match;

#[test]
fn keeps_whole_word_bonus_when_an_earlier_word_merely_contains_the_query() {
	let plain = fuzzy_match("image", "image provider");
	let shadowed = fuzzy_match("image", "reimage image provider");

	assert!(plain.matches);
	assert!(shadowed.matches);
	assert!(shadowed.score < plain.score + 100.0);
}

#[test]
fn ranks_an_exact_standalone_word_above_a_prefix_only_candidate() {
	let items = ["reimage image provider", "imagemagick tool"];
	let ranked: Vec<&str> = maple_tui::fuzzy::fuzzy_filter("image", &items)
		.into_iter()
		.map(|index| items[index])
		.collect();
	assert_eq!(ranked[0], "reimage image provider");
}

#[test]
fn keeps_the_compact_word_start_bonus_when_an_earlier_occurrence_is_mid_word() {
	let plain = fuzzy_match("statusline", "status line");
	let shadowed = fuzzy_match("statusline", "mystatuslinex status line");

	assert!(plain.matches);
	assert!(shadowed.matches);
	assert!(shadowed.score < plain.score + 100.0);
}

#[test]
fn keeps_the_multi_word_phrase_bonus_when_an_earlier_occurrence_is_mid_word() {
	let plain = fuzzy_match("gpt 5", "gpt 5");
	let shadowed = fuzzy_match("gpt 5", "xgpt 5x gpt 5");

	assert!(plain.matches);
	assert!(shadowed.matches);
	assert!(shadowed.score < plain.score + 100.0);
}

#[test]
fn does_not_invent_a_bonus_when_every_occurrence_is_mid_word() {
	let weak = fuzzy_match("line", "multiline input mode");
	let strong = fuzzy_match("line", "line height");

	assert!(weak.matches);
	assert!(strong.score < weak.score - 500.0);
}

#[test]
fn does_not_invent_a_bonus_when_a_shadowed_occurrence_also_fails_to_qualify() {
	let none = fuzzy_match("image", "reimage ximagey");
	let weak = fuzzy_match("image", "multiimage bar");

	assert!(none.score > -500.0);
	assert!(weak.score > -500.0);
}

#[test]
fn leaves_a_single_character_query_on_its_leading_occurrence() {
	let shadowed = fuzzy_match("i", "wire input");
	let leading = fuzzy_match("i", "input wire");

	assert!(shadowed.matches);
	assert!(leading.matches);
	assert!(shadowed.score > -500.0);
	assert!(leading.score < -500.0);
}

#[test]
fn leaves_a_two_character_query_on_its_leading_occurrence() {
	let shadowed = fuzzy_match("im", "experimental image rendering");
	let leading = fuzzy_match("im", "image rendering experimental");

	assert!(shadowed.matches);
	assert!(leading.matches);
	assert!(shadowed.score > -500.0);
	assert!(leading.score < -500.0);
}

#[test]
fn treats_a_leading_word_start_hit_as_a_prefix_match_not_a_shadowed_one() {
	let prefix_hit = fuzzy_match(
		"image",
		"describe images for text models when an image is attached",
	);

	assert!(prefix_hit.matches);
	assert!(prefix_hit.score > -2000.0);
	assert!(fuzzy_match("image", "reimage image provider").score < -2000.0);
}