//! Port of `l/test/image-references.test.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::prompt::composer_attachments::{
	chip_label, collapse_image_markers, collapse_model_mentions, collapse_skill_tokens,
	compact_image_markers, composer_token_regex, model_mention_chip_label, render_placeholders,
	shift_image_markers, skill_chip_label, ChipKind, PlaceholderForm, PlaceholderKind,
	PlaceholderRenderers,
};
use regex::Regex;

#[derive(Default)]
struct Capture {
	refs: RefCell<Vec<(String, PlaceholderKind, i64, PlaceholderForm)>>,
	skills: RefCell<Vec<String>>,
	mentions: RefCell<Vec<String>>,
}

impl PlaceholderRenderers for Capture {
	fn render_text(&self, text: &str) -> String {
		text.to_string()
	}

	fn render_reference(&self, label: &str, kind: PlaceholderKind, index: i64, form: PlaceholderForm) -> String {
		self.refs.borrow_mut().push((label.to_string(), kind, index, form));
		format!("<{kind}:{index}>")
	}

	fn render_skill(&self, _label: &str, name: &str) -> String {
		self.skills.borrow_mut().push(name.to_string());
		format!("<skill:{name}>")
	}

	fn render_mention(&self, label: &str) -> String {
		self.mentions.borrow_mut().push(label.to_string());
		format!("<model:{label}>")
	}
}

fn capture(text: &str, mention_labels: &[&str]) -> (String, Rc<Capture>) {
	let capture = Rc::new(Capture::default());
	let labels: Vec<String> = mention_labels.iter().map(|label| (*label).to_string()).collect();
	let token_regex: Regex = composer_token_regex(&labels);
	let out = render_placeholders(text, capture.as_ref(), Some(&token_regex));
	(out, capture)
}

#[test]
fn classifies_image_and_paste_markers_with_index_and_full_label() {
	let (out, capture) = capture("see [Image #1, 800x600] then [Paste #2, +30 lines] done", &[]);
	assert_eq!(
		*capture.refs.borrow(),
		vec![
			("[Image #1, 800x600]".to_string(), PlaceholderKind::Image, 1, PlaceholderForm::Marker),
			("[Paste #2, +30 lines]".to_string(), PlaceholderKind::Paste, 2, PlaceholderForm::Marker),
		]
	);
	assert_eq!(out, "see <image:1> then <paste:2> done");
}

#[test]
fn matches_the_bare_image_form_and_the_char_count_paste_form() {
	let (_, capture) = capture("[Image #3]", &[]);
	assert_eq!(capture.refs.borrow()[0].1, PlaceholderKind::Image);
	assert_eq!(capture.refs.borrow()[0].2, 3);
	let (_, capture) = capture("[Paste #4, 1500 chars]", &[]);
	assert_eq!(capture.refs.borrow()[0].1, PlaceholderKind::Paste);
	assert_eq!(capture.refs.borrow()[0].2, 4);
}

#[test]
fn passes_plain_text_straight_through_with_no_references() {
	let (out, capture) = capture("no markers here", &[]);
	assert!(capture.refs.borrow().is_empty());
	assert_eq!(out, "no markers here");
}

#[test]
fn does_not_treat_an_unterminated_marker_as_a_reference() {
	let (_, capture) = capture("[Paste #1, +30 lines", &[]);
	assert!(capture.refs.borrow().is_empty());
}

#[test]
fn preserves_the_adjacent_attachment_uri_as_plain_text() {
	let (out, _) = capture("[Image #1, 800x600] attachment://1", &[]);
	assert_eq!(out, "<image:1> attachment://1");
}

#[test]
fn classifies_compact_chip_tokens() {
	let (out, capture) = capture(
		&format!("see {} and {} done", chip_label(ChipKind::Image, 2, None), chip_label(ChipKind::Paste, 1, None)),
		&[],
	);
	assert_eq!(
		*capture.refs.borrow(),
		vec![
			(chip_label(ChipKind::Image, 2, None), PlaceholderKind::Image, 2, PlaceholderForm::Chip),
			(chip_label(ChipKind::Paste, 1, None), PlaceholderKind::Paste, 1, PlaceholderForm::Chip),
		]
	);
	assert_eq!(out, "see <image:2> and <paste:1> done");
}

#[test]
fn recognizes_chip_tokens_from_every_symbol_preset() {
	let (_, capture) = capture("\u{f03e} #3", &[]);
	assert_eq!(capture.refs.borrow()[0].1, PlaceholderKind::Image);
	assert_eq!(capture.refs.borrow()[0].2, 3);
	assert_eq!(capture.refs.borrow()[0].3, PlaceholderForm::Chip);
	let (_, capture) = capture("img #3", &[]);
	assert_eq!(capture.refs.borrow()[0].1, PlaceholderKind::Image);
	let (_, capture) = capture("txt #2", &[]);
	assert_eq!(capture.refs.borrow()[0].1, PlaceholderKind::Paste);
}

#[test]
fn does_not_treat_a_word_ending_in_an_ascii_icon_as_a_chip_token() {
	let (_, capture) = capture("boximg #1", &[]);
	assert!(capture.refs.borrow().is_empty());
}

#[test]
fn routes_skill_chips_to_render_skill_stopping_at_punctuation() {
	let stencil = skill_chip_label("stencil-design", None);
	let versioned = skill_chip_label("v2.1", None);
	let (out, capture) = capture(&format!("use {stencil}, then {versioned}."), &[]);
	assert_eq!(*capture.skills.borrow(), vec!["stencil-design".to_string(), "v2.1".to_string()]);
	assert!(capture.refs.borrow().is_empty());
	assert_eq!(out, "use <skill:stencil-design>, then <skill:v2.1>.");
}

#[test]
fn recognizes_skill_chips_from_every_symbol_preset() {
	let (_, capture) = capture("\u{f0eb} reviewer", &[]);
	assert_eq!(*capture.skills.borrow(), vec!["reviewer".to_string()]);
	let (_, capture) = capture("SK reviewer", &[]);
	assert_eq!(*capture.skills.borrow(), vec!["reviewer".to_string()]);
	let (_, capture) = capture("TASK reviewer", &[]);
	assert!(capture.skills.borrow().is_empty());
}

#[test]
fn routes_registered_model_labels_with_spaces_and_parentheses() {
	let short = model_mention_chip_label("Claude", None);
	let long = model_mention_chip_label("Claude (Fast)", None);
	let (out, capture) = capture(&format!("ask {long} then {short}"), &[&short, &long]);
	assert_eq!(*capture.mentions.borrow(), vec![long.clone(), short.clone()]);
	assert!(capture.refs.borrow().is_empty());
	assert_eq!(out, format!("ask <model:{long}> then <model:{short}>"));
}

#[test]
fn dispatches_an_ascii_model_label_before_bracketed_attachment_markers() {
	let label = "[M] Claude (Fast)";
	let (out, capture) = capture(&format!("ask {label} now"), &[label]);
	assert_eq!(*capture.mentions.borrow(), vec![label.to_string()]);
	assert!(capture.refs.borrow().is_empty());
	assert_eq!(out, format!("ask <model:{label}> now"));
}

#[test]
fn collapses_known_whitespace_delimited_model_selectors() {
	let label_for = |selector: &str| {
		if selector == "a/x" {
			Some((model_mention_chip_label("X One", None), "a/x".to_string()))
		} else {
			None
		}
	};
	let mut registered: Vec<(String, String)> = Vec::new();
	let out = collapse_model_mentions("ask ^a/x now", &label_for, &mut |label, expansion| {
		registered.push((label.to_string(), expansion.to_string()));
	});
	let expected = model_mention_chip_label("X One", None);
	assert_eq!(out, format!("ask {expected} now"));
	assert_eq!(registered, vec![(expected, "^a/x".to_string())]);
}

#[test]
fn keeps_unknown_and_punctuation_attached_model_selectors_literal() {
	let label_for = |_: &str| None::<(String, String)>;
	assert_eq!(
		collapse_model_mentions("^nope/z and ^a/x,", &label_for, &mut |_, _| {}),
		"^nope/z and ^a/x,"
	);
}

#[test]
fn allows_slash_prompts_but_not_local_execution_drafts() {
	let label_for = |selector: &str| {
		if selector == "a/x" {
			Some((model_mention_chip_label("X One", None), "a/x".to_string()))
		} else {
			None
		}
	};
	let expected = model_mention_chip_label("X One", None);
	assert_eq!(
		collapse_model_mentions("/plan ask ^a/x now", &label_for, &mut |_, _| {}),
		format!("/plan ask {expected} now")
	);
	assert_eq!(collapse_model_mentions("!echo ^a/x", &label_for, &mut |_, _| {}), "!echo ^a/x");
	assert_eq!(collapse_model_mentions("$ echo ^a/x", &label_for, &mut |_, _| {}), "$ echo ^a/x");
}

#[test]
fn collapses_known_skill_tokens_into_chips() {
	let known = |name: &str| name == "reviewer";
	let mut registered: Vec<(String, String)> = Vec::new();
	let out = collapse_skill_tokens("fix it /skill:reviewer now", &known, &mut |label, expansion| {
		registered.push((label.to_string(), expansion.to_string()));
	});
	let label = skill_chip_label("reviewer", None);
	assert_eq!(out, format!("fix it {label} now"));
	assert_eq!(registered, vec![(label, "/skill:reviewer".to_string())]);
}

#[test]
fn leaves_unknown_skills_and_glued_tokens_literal() {
	let known = |name: &str| name == "reviewer";
	assert_eq!(
		collapse_skill_tokens("/skill:nope and x/skill:reviewer", &known, &mut |_, _| {}),
		"/skill:nope and x/skill:reviewer"
	);
}

#[test]
fn does_not_collapse_inside_a_bash_or_foreign_slash_command_draft() {
	let known = |name: &str| name == "reviewer";
	assert_eq!(collapse_skill_tokens("!echo /skill:reviewer", &known, &mut |_, _| {}), "!echo /skill:reviewer");
	assert_eq!(
		collapse_skill_tokens("/compact /skill:reviewer", &known, &mut |_, _| {}),
		"/compact /skill:reviewer"
	);
}

#[test]
fn collapses_a_collision_namespaced_skill_as_one_chip() {
	let known = |name: &str| name == "superpowers/tdd";
	let mut registered: Vec<(String, String)> = Vec::new();
	let out = collapse_skill_tokens("fix it /skill:superpowers/tdd now", &known, &mut |label, expansion| {
		registered.push((label.to_string(), expansion.to_string()));
	});
	let label = skill_chip_label("superpowers/tdd", None);
	assert_eq!(out, format!("fix it {label} now"));
	assert_eq!(registered, vec![(label.clone(), "/skill:superpowers/tdd".to_string())]);
	let (_, capture) = capture(&out, &[]);
	assert_eq!(*capture.skills.borrow(), vec!["superpowers/tdd".to_string()]);
}

#[test]
fn collapses_bracketed_markers_into_chip_tokens() {
	let mut registered: Vec<(String, String)> = Vec::new();
	let label = chip_label(ChipKind::Image, 1, None);
	let out = collapse_image_markers("see [Image #1, 800x600] end", 1, &mut |label, expansion| {
		registered.push((label.to_string(), expansion.to_string()));
	});
	assert_eq!(out, format!("see {label} end"));
	assert_eq!(registered, vec![(label, "[Image #1, 800x600]".to_string())]);
}

#[test]
fn strips_a_legacy_trailing_attachment_uri_while_collapsing() {
	let label = chip_label(ChipKind::Image, 1, None);
	let out = collapse_image_markers("[Image #1] attachment://1 tail", 1, &mut |_, _| {});
	assert_eq!(out, format!("{label} tail"));
}

#[test]
fn leaves_markers_beyond_the_pending_image_count_as_plain_text() {
	assert_eq!(
		collapse_image_markers("[Image #2] is from before", 1, &mut |_, _| {}),
		"[Image #2] is from before"
	);
}

#[test]
fn compact_returns_none_when_every_pending_image_is_still_referenced() {
	assert_eq!(compact_image_markers("[Image #1] [Image #2]", 2, false), None);
}

#[test]
fn compact_renumbers_surviving_markers_after_a_middle_deletion() {
	assert_eq!(
		compact_image_markers("[Image #1, 10x10] and [Image #3]", 3, false),
		Some((
			"[Image #1, 10x10] and [Image #2]".to_string(),
			vec![0, 2]
		))
	);
}

#[test]
fn compact_drops_every_image_when_no_marker_references_one() {
	assert_eq!(
		compact_image_markers("no references", 2, false),
		Some(("no references".to_string(), vec![]))
	);
}

#[test]
fn compact_leaves_out_of_range_references_untouched() {
	assert_eq!(
		compact_image_markers("[Image #2] mentions [Image #9]", 2, false),
		Some(("[Image #1] mentions [Image #9]".to_string(), vec![1]))
	);
}

#[test]
fn compact_renumbers_a_legacy_attachment_uri_alongside_its_marker() {
	assert_eq!(
		compact_image_markers("[Image #2] attachment://2", 2, false),
		Some(("[Image #1] attachment://1".to_string(), vec![1]))
	);
}

#[test]
fn shift_returns_text_unchanged_when_offset_is_zero() {
	let text = "[Image #1] attachment://1 then [Image #2, 100x100] attachment://2";
	assert_eq!(shift_image_markers(text, 0, None), text);
}

#[test]
fn shift_renumbers_marker_and_attachment_uri_by_the_offset() {
	assert_eq!(
		shift_image_markers("see [Image #1, 800x600] attachment://1", 3, None),
		"see [Image #4, 800x600] attachment://4"
	);
}

#[test]
fn shift_does_not_shift_unrelated_attachment_uri_text() {
	assert_eq!(
		shift_image_markers("attachment://1 then [Image #1] attachment://9 and attachment://2", 3, None),
		"attachment://1 then [Image #4] attachment://9 and attachment://2"
	);
}

#[test]
fn shift_handles_multiple_pairs_exactly_once() {
	assert_eq!(
		shift_image_markers("[Image #1] attachment://1 then [Image #2, 100x100] attachment://2", 2, None),
		"[Image #3] attachment://3 then [Image #4, 100x100] attachment://4"
	);
}

#[test]
fn shift_leaves_markers_above_the_image_count_alone() {
	assert_eq!(
		shift_image_markers("[Image #1] attachment://1 beside [Image #2]", 3, Some(1)),
		"[Image #4] attachment://4 beside [Image #2]"
	);
	assert_eq!(shift_image_markers("see [Image #1]", 3, Some(0)), "see [Image #1]");
}

#[test]
fn shift_never_touches_paste_markers() {
	assert_eq!(
		shift_image_markers("[Image #1] [Paste #1, +5 lines]", 2, None),
		"[Image #3] [Paste #1, +5 lines]"
	);
}
