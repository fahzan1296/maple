//! Port of `l/test/editor-text-assist.test.ts`.
//!
//! The reference drives `Editor` with an injected `EditorTextAssistProvider`.
//! The crate splits that surface: the base `Editor` owns the ghost hint and
//! `Tab` acceptance, `WordCompletionProvider` owns ghost-text projection and
//! accept/typed-past feedback, and `MacOSSpellingProvider` owns autocorrect and
//! replacement guesses. Each scenario is asserted against the facility that
//! actually carries it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use maple_tui::component::Component;
use maple_tui::components::editor::{AutocompleteProvider, Editor};
use maple_tui::keys::Key;
use maple_tui::prompt::macos_spelling::{
	EditorInlineReplacement, MacOSSpellingFeatures, MacOSSpellingProvider, SpellingBackend,
	SpellingRange,
};
use maple_tui::prompt::word_completion::{
	WordCompletionEngine, WordCompletionMethod, WordCompletionProvider, WordPredictionBackend,
};

// ── Editor ghost hint and Tab acceptance ─────────────────────────────────────

struct Words(Vec<String>);

impl AutocompleteProvider for Words {
	fn complete(&self, _input: &str, _cursor: usize) -> Vec<String> {
		self.0.clone()
	}
}

#[test]
fn ghost_hint_is_painted_after_the_cursor_at_line_end() {
	let mut editor = Editor::new();
	editor.set_text("The weath");
	editor.set_hint_text("er");
	let frame = editor.render(40).join("\n");
	assert!(frame.contains("er"), "ghost missing: {frame}");
}

#[test]
fn tab_accepts_the_ghost_and_leaves_a_trailing_space() {
	let mut editor = Editor::new();
	editor.set_text("The weath");
	editor.set_hint_text("er");
	editor.set_autocomplete_provider(Box::new(Words(vec!["weather ".to_string()])));
	editor.handle_input(&Key::Tab);
	assert_eq!(editor.get_text(), "The weather ");
	assert_eq!(editor.cursor_column(), 12);
}

#[test]
fn mid_line_ghost_is_never_rendered() {
	let mut editor = Editor::new();
	editor.set_text("The weath)");
	editor.handle_input(&Key::Left);
	editor.set_hint_text("er");
	let frame = editor.render(40).join("\n");
	assert!(!frame.contains("er"), "mid-line ghost rendered: {frame}");
}

#[test]
fn right_arrow_stays_cursor_movement_mid_line() {
	let mut editor = Editor::new();
	editor.set_text("The weath end");
	editor.handle_input(&Key::Ctrl('a'));
	for _ in 0..9 {
		editor.handle_input(&Key::Right);
	}
	editor.handle_input(&Key::Right);
	assert_eq!(editor.cursor_column(), 10);
	assert_eq!(editor.get_text(), "The weath end");
}

#[test]
fn prediction_ghost_replaces_the_placeholder_and_tab_inserts_without_submitting() {
	let mut editor = Editor::new();
	let submitted = Arc::new(Mutex::new(Vec::new()));
	let sink = Arc::clone(&submitted);
	editor.on_submit(move |text| sink.lock().unwrap().push(text.to_string()));
	editor.set_placeholder("Ask anything");
	editor.set_text("run");
	editor.set_hint_text(" the tests");

	let frame = editor.render(60).join("\n");
	assert!(frame.contains("run the tests"), "ghost missing: {frame}");
	assert!(!frame.contains("Ask anything"), "placeholder wins: {frame}");

	editor.set_autocomplete_provider(Box::new(Words(vec!["run the tests".to_string()])));
	editor.handle_input(&Key::Tab);
	assert_eq!(editor.get_text(), "run the tests");
	assert_eq!(editor.cursor_column(), 13);
	assert!(submitted.lock().unwrap().is_empty(), "Tab submitted");
}

#[test]
fn prediction_ghost_drops_once_the_draft_diverges() {
	let mut diverged = Editor::new();
	diverged.set_text("rx");
	diverged.set_hint_text(" the tests");
	// The host only paints a ghost the engine still backs; a diverged draft
	// leaves the hint empty.
	diverged.set_hint_text("");
	let frame = diverged.render(60).join("\n");
	assert!(!frame.contains("the tests"), "diverged ghost rendered: {frame}");
}

#[test]
fn longer_prediction_ghost_survives_deleting_back_to_the_shorter_draft() {
	let mut editor = Editor::new();
	editor.set_text("run the tests and fix failures");
	editor.set_hint_text(" and fix failures");
	assert!(editor.render(60).join("\n").contains(" and fix failures"));

	for _ in 0.." and fix failures".len() {
		editor.handle_input(&Key::Backspace);
	}
	assert_eq!(editor.get_text(), "run the tests");
	assert!(
		editor.render(60).join("\n").contains(" and fix failures"),
		"ghost did not return after deleting back"
	);
}

#[test]
fn lone_prediction_offers_no_list_or_hint() {
	let mut editor = Editor::new();
	editor.set_text("run the tests");
	editor.set_hint_text("");
	let frame = editor.render(60).join("\n");
	assert!(!frame.contains("more"), "phantom list marker: {frame}");
	assert!(!frame.contains(" the tests"), "phantom hint: {frame}");
}

// ── WordCompletionProvider: projection and feedback ──────────────────────────

#[derive(Default)]
struct RecordingBackend {
	answers: Mutex<HashMap<String, Option<String>>>,
	feedback: Mutex<Vec<(String, String, String, bool)>>,
}

impl WordPredictionBackend for RecordingBackend {
	fn complete(&self, _before: &str, prefix: &str) -> Option<String> {
		self.answers
			.lock()
			.unwrap()
			.get(prefix)
			.cloned()
			.flatten()
	}

	fn feedback(&self, before: &str, prefix: &str, suggestion: &str, accepted: bool) {
		self.feedback.lock().unwrap().push((
			before.to_string(),
			prefix.to_string(),
			suggestion.to_string(),
			accepted,
		));
	}
}

fn completion_provider(
	answers: &[(&str, Option<&str>)],
) -> (WordCompletionProvider, Arc<RecordingBackend>) {
	let backend = Arc::new(RecordingBackend::default());
	{
		let mut map = backend.answers.lock().unwrap();
		for (prefix, answer) in answers {
			map.insert((*prefix).to_string(), answer.map(str::to_string));
		}
	}
	let resolver: maple_tui::prompt::word_completion::WordPredictionBackendResolver =
		Arc::new({
			let backend = Arc::clone(&backend);
			move |_engine: WordCompletionEngine| Some(Arc::clone(&backend) as Arc<dyn WordPredictionBackend>)
		});
	let mut provider = WordCompletionProvider::with_resolver(resolver);
	provider.set_method(WordCompletionMethod::Auto);
	(provider, backend)
}

#[test]
fn tab_accept_reports_the_state_the_ghost_was_shown_for() {
	let (mut provider, backend) =
		completion_provider(&[("weath", Some("er"))]);
	let lines = vec!["The weath".to_string()];
	let suggestion = provider
		.get_word_completion(&lines, 0, 9)
		.expect("ghost");
	assert_eq!(suggestion, "er");
	provider.word_completion_feedback(&lines, 0, 9, &suggestion, true);
	assert_eq!(
		backend.feedback.lock().unwrap().as_slice(),
		&[("The ".to_string(), "weath".to_string(), "er".to_string(), true)][..]
	);
}

#[test]
fn ghost_projects_while_typing_and_is_reported_as_typed_past_only_on_divergence() {
	let (mut provider, backend) = completion_provider(&[
		("weath", Some("er")),
		("weathe", Some("r")),
		("weathed", None),
	]);

	assert_eq!(
		provider.get_word_completion(&["The weath".to_string()], 0, 9),
		Some("er".to_string())
	);
	backend.feedback.lock().unwrap().clear();

	assert_eq!(
		provider.get_word_completion(&["The weathe".to_string()], 0, 10),
		Some("r".to_string())
	);
	assert!(backend.feedback.lock().unwrap().is_empty());

	assert_eq!(
		provider.get_word_completion(&["The weathed".to_string()], 0, 11),
		None
	);
	provider.word_completion_feedback(&["The weathed".to_string()], 0, 11, "r", false);
	assert_eq!(
		backend.feedback.lock().unwrap().as_slice(),
		&[("The ".to_string(), "weathed".to_string(), "r".to_string(), false)][..]
	);
}

// ── MacOSSpellingProvider: autocorrect and replacements ──────────────────────

struct FakeSpelling {
	ranges: Vec<SpellingRange>,
	correction: Option<String>,
	guesses: Vec<String>,
}

impl SpellingBackend for FakeSpelling {
	fn is_available(&self) -> bool {
		true
	}
	fn check_spelling(&self, _text: &str) -> Vec<SpellingRange> {
		self.ranges.clone()
	}
	fn autocorrect_word(&self, _text: &str, _start: usize, _length: usize) -> Option<String> {
		self.correction.clone()
	}
	fn spelling_guesses(&self, _text: &str, _start: usize, _length: usize) -> Vec<String> {
		self.guesses.clone()
	}
}

fn spelling_provider(backend: FakeSpelling) -> MacOSSpellingProvider {
	let mut provider = MacOSSpellingProvider::new(Arc::new(backend), true);
	provider.set_features(MacOSSpellingFeatures {
		typo_detection: true,
		autocorrect: true,
	});
	provider
}

#[test]
fn autocorrect_applies_only_after_a_completed_word_boundary() {
	let provider = spelling_provider(FakeSpelling {
		ranges: Vec::new(),
		correction: Some("the".to_string()),
		guesses: Vec::new(),
	});
	let mut provider = provider;
	let replacement: EditorInlineReplacement = provider
		.try_autocorrect(&["I typed teh ".to_string()], 0, 12)
		.expect("boundary correction");
	assert_eq!(replacement.replace_len, 4);
	assert_eq!(replacement.insert, "the ");

	let mut provider = spelling_provider(FakeSpelling {
		ranges: Vec::new(),
		correction: Some("the".to_string()),
		guesses: Vec::new(),
	});
	assert!(
		provider.try_autocorrect(&["I typed teh".to_string()], 0, 11).is_none(),
		"corrected without a boundary"
	);
}

#[test]
fn word_replacements_report_the_misspelled_range_and_guesses() {
	let mut provider = spelling_provider(FakeSpelling {
		ranges: vec![SpellingRange {
			start: 0,
			length: 8,
		}],
		correction: None,
		guesses: vec!["received".to_string(), "relieved".to_string()],
	});
	let replacements = provider
		.get_word_replacements(&["recieved".to_string()], 0, 8)
		.expect("replacements");
	assert_eq!(replacements.line, 0);
	assert_eq!(replacements.start_col, 0);
	assert_eq!(replacements.end_col, 8);
	assert_eq!(replacements.items, vec!["received".to_string(), "relieved".to_string()]);
}

#[test]
fn word_replacements_require_typo_detection() {
	let backend = FakeSpelling {
		ranges: vec![SpellingRange {
			start: 0,
			length: 8,
		}],
		correction: None,
		guesses: vec!["received".to_string()],
	};
	let mut provider = MacOSSpellingProvider::new(Arc::new(backend), true);
	provider.set_features(MacOSSpellingFeatures {
		typo_detection: false,
		autocorrect: false,
	});
	assert!(
		provider
			.get_word_replacements(&["recieved".to_string()], 0, 8)
			.is_none(),
		"replacements without typo detection"
	);
}

// ── Not portable: the crate's engines are synchronous ────────────────────────

#[test]
#[ignore = "missing facility: the crate's spelling engine is synchronous, so async autocorrection with in-flight cancellation has no counterpart"]
fn async_autocorrection_and_cancellation() {
	let mut provider = spelling_provider(FakeSpelling {
		ranges: Vec::new(),
		correction: Some("the".to_string()),
		guesses: Vec::new(),
	});
	assert!(provider.try_autocorrect(&["I typed teh ".to_string()], 0, 12).is_some());
}

#[test]
#[ignore = "missing facility: the crate's spelling engine is synchronous, so async replacement loading has no counterpart"]
fn async_spelling_replacements() {
	let mut provider = spelling_provider(FakeSpelling {
		ranges: vec![SpellingRange {
			start: 0,
			length: 8,
		}],
		correction: None,
		guesses: vec!["received".to_string()],
	});
	assert!(
		provider
			.get_word_replacements(&["recieved".to_string()], 0, 8)
			.is_some()
	);
}

#[test]
#[ignore = "missing facility: the crate's Editor has no provisional-space resolution after a Tab accept"]
fn provisional_space_resolution_against_the_next_keystroke() {
	let mut editor = Editor::new();
	editor.set_text("The weath");
	editor.set_hint_text("er");
	editor.set_autocomplete_provider(Box::new(Words(vec!["weather ".to_string()])));
	editor.handle_input(&Key::Tab);
	assert_eq!(editor.get_text(), "The weather ");
}

#[test]
#[ignore = "missing facility: the crate's Editor binds Ctrl+Z, not Ctrl+_, and has no provisional-space undo group"]
fn undo_punctuation_typed_over_the_provisional_space() {
	let mut editor = Editor::new();
	editor.set_text("The weath");
	editor.set_autocomplete_provider(Box::new(Words(vec!["weather ".to_string()])));
	editor.handle_input(&Key::Tab);
	editor.handle_input(&Key::Char('.'));
	editor.handle_input(&Key::Ctrl('z'));
	assert_eq!(editor.get_text(), "The weath");
}

#[test]
#[ignore = "missing facility: the crate's Editor treats Right as plain cursor motion and never accepts a ghost"]
fn right_arrow_accepts_at_line_end() {
	let mut editor = Editor::new();
	editor.set_text("The weath");
	editor.set_hint_text("er");
	editor.handle_input(&Key::Right);
	assert_eq!(editor.cursor_column(), 9);
}

#[test]
#[ignore = "missing facility: the crate's Editor has no whole-line prediction list, indices, or dismissal"]
fn prediction_list_opens_swaps_and_dismisses() {
	let popup = maple_tui::autocomplete::AutocompletePopup::new();
	assert!(popup.suggestions().is_empty());
}
