//! Port of `l/test/typing-word.test.ts`.

use maple_tui::prompt::typing_word::{TypingWord, TypingWordTracker};

fn lines(values: &[&str]) -> Vec<String> {
	values.iter().map(|value| (*value).to_string()).collect()
}

fn word(line: usize, start: usize, end: usize) -> Option<TypingWord> {
	Some(TypingWord { line, start, end })
}

#[test]
fn holds_the_word_under_the_caret_until_a_separator_is_typed() {
	let mut tracker = TypingWordTracker::default();
	tracker.update(0, &lines(&[""]), 0, 0, true);
	assert_eq!(tracker.word(), None);

	assert!(tracker.update(1, &lines(&["helo"]), 0, 4, true));
	assert_eq!(tracker.word(), word(0, 0, 4));

	assert!(tracker.update(2, &lines(&["helo "]), 0, 5, true));
	assert_eq!(tracker.word(), None);

	tracker.update(3, &lines(&["helo,"]), 0, 5, true);
	assert_eq!(tracker.word(), None);
}

#[test]
fn keeps_the_word_while_the_caret_stays_in_it_and_drops_it_once_the_caret_leaves() {
	let mut tracker = TypingWordTracker::default();
	tracker.update(0, &lines(&["wrld "]), 0, 5, true);
	tracker.update(1, &lines(&["wrld helo"]), 0, 9, true);
	assert_eq!(tracker.word(), word(0, 5, 9));

	assert!(!tracker.update(1, &lines(&["wrld helo"]), 0, 6, true));
	assert_eq!(tracker.word(), word(0, 5, 9));

	assert!(tracker.update(1, &lines(&["wrld helo"]), 0, 2, true));
	assert_eq!(tracker.word(), None);

	// Coming back without typing leaves its mark shown.
	tracker.update(1, &lines(&["wrld helo"]), 0, 9, true);
	assert_eq!(tracker.word(), None);
}

#[test]
fn drops_the_word_on_another_line_or_on_blur() {
	let mut tracker = TypingWordTracker::default();
	tracker.update(0, &lines(&["", "x"]), 0, 0, true);
	tracker.update(1, &lines(&["helo", "x"]), 0, 4, true);
	assert!(tracker.update(1, &lines(&["helo", "x"]), 1, 0, true));
	assert_eq!(tracker.word(), None);

	tracker.update(2, &lines(&["helo", "x"]), 0, 4, true);
	assert_eq!(tracker.word(), word(0, 0, 4));
	assert!(tracker.update(2, &lines(&["helo", "x"]), 0, 4, false));
	assert_eq!(tracker.word(), None);
}

#[test]
fn does_not_hold_a_word_the_first_time_it_sees_the_text() {
	let mut tracker = TypingWordTracker::default();
	tracker.update(7, &lines(&["restored helo"]), 0, 13, true);
	assert_eq!(tracker.word(), None);
}

#[test]
fn joins_contractions_and_keeps_accented_letters() {
	let mut tracker = TypingWordTracker::default();
	tracker.update(0, &lines(&[""]), 0, 0, true);
	tracker.update(1, &lines(&["say don't"]), 0, 9, true);
	assert_eq!(tracker.word(), word(0, 4, 9));
	tracker.update(2, &lines(&["café"]), 0, 4, true);
	assert_eq!(tracker.word(), word(0, 0, 4));
}
