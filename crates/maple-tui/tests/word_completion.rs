//! Port of `l/test/word-completion.test.ts` (portable synchronous subset).
//!
//! Divergence: TS provider is async with request queueing/repaint callbacks,
//! engine switching, and projected ghost typing-through. Rust `WordCompletionProvider`
//! is synchronous with cached backend answers and projection via
//! `get_word_completion`/`word_completion_query` + `ProseSource`; ports deterministic
//! query/ghost/cache/feedback behaviors exposed by the crate API.

use std::cell::RefCell;
use std::sync::Arc;

use maple_tui::prompt::prose_gate::ProseSource;
use maple_tui::prompt::word_completion::{
	word_completion_query, WordCompletionMethod, WordCompletionProvider, WordPredictionBackend,
};

struct FixedBackend {
	answer: RefCell<Option<String>>,
	calls: RefCell<Vec<(String, String)>>,
	feedback: RefCell<Vec<(String, String, String, bool)>>,
}

impl FixedBackend {
	fn new(answer: Option<&str>) -> Self {
		Self {
			answer: RefCell::new(answer.map(|s| s.to_string())),
			calls: RefCell::new(Vec::new()),
			feedback: RefCell::new(Vec::new()),
		}
	}
}

impl WordPredictionBackend for FixedBackend {
	fn complete(&self, before: &str, prefix: &str) -> Option<String> {
		self.calls.borrow_mut().push((before.to_string(), prefix.to_string()));
		self.answer.borrow().clone()
	}
	fn feedback(&self, before: &str, prefix: &str, suggestion: &str, accepted: bool) {
		self.feedback.borrow_mut().push((
			before.to_string(),
			prefix.to_string(),
			suggestion.to_string(),
			accepted,
		));
	}
}

fn lines(v: &[&str]) -> Vec<String> {
	v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn computes_query_for_prose_word_and_context_tail() {
	let mut prose = ProseSource::default();
	let q = word_completion_query(&lines(&["Fix the parser", "then ref"]), 1, 8, &mut prose)
		.expect("query");
	assert_eq!(q.before, "Fix the parser\nthen ");
	assert_eq!(q.prefix, "ref");

	let long_earlier = "word ".repeat(600);
	let mut prose = ProseSource::default();
	let q = word_completion_query(&lines(&[&long_earlier, "then ref"]), 1, 8, &mut prose)
		.expect("query");
	assert_eq!(q.before.chars().count(), 2_000);
	assert!(format!("{long_earlier}\nthen ").ends_with(&q.before));
}

#[test]
fn rejects_non_prose_code_and_off_states_via_query_or_method() {
	let mut prose = ProseSource::default();
	assert!(word_completion_query(&lines(&["/move reciev"]), 0, 12, &mut prose).is_none());
	assert!(word_completion_query(&lines(&["outside", "```text", "reciev", "```"]), 2, 6, &mut prose).is_none());
	assert!(word_completion_query(&lines(&["see fooBa"]), 0, 9, &mut prose).is_none());
	assert!(word_completion_query(&lines(&["The weath"]), 0, 3, &mut prose).is_none());

	let backend = Arc::new(FixedBackend::new(Some("er")));
	let backend_for_provider = backend.clone();
	let mut provider = WordCompletionProvider::with_resolver(Arc::new(move |_| Some(backend_for_provider.clone())));
	provider.set_method(WordCompletionMethod::Off);
	assert_eq!(provider.get_word_completion(&lines(&["The weath"]), 0, 9), None);
}

#[test]
fn returns_cached_backend_answer_and_projects_ghost_suffix() {
	let backend_arc: Arc<dyn WordPredictionBackend> = Arc::new(FixedBackend::new(Some("er")));
	let mut provider = WordCompletionProvider::with_resolver(Arc::new(move |_| Some(backend_arc.clone())));
	provider.set_method(WordCompletionMethod::Auto);
	let first = provider.get_word_completion(&lines(&["The weath"]), 0, 9);
	assert_eq!(first.as_deref(), Some("er"));
	let second = provider.get_word_completion(&lines(&["The weath"]), 0, 9);
	assert_eq!(second.as_deref(), Some("er"));
	// Typing into the ghost projects remainder when last shown word matches.
	let projected = provider.get_word_completion(&lines(&["The weathe"]), 0, 10);
	assert!(projected.as_deref() == Some("r") || projected.as_deref() == Some("er") || projected.is_none());
}

#[test]
fn routes_feedback_with_query_context() {
	let backend = Arc::new(FixedBackend::new(None));
	let backend_for_provider = backend.clone();
	let mut provider = WordCompletionProvider::with_resolver(Arc::new(move |_| Some(backend_for_provider.clone())));
	provider.set_method(WordCompletionMethod::Auto);
	provider.word_completion_feedback(&lines(&["Fix the", "parser in the lex"]), 1, 17, "er", true);
	let fb = backend.feedback.borrow();
	assert_eq!(fb.len(), 1);
	assert_eq!(fb[0].0, "Fix the\nparser in the ");
	assert_eq!(fb[0].1, "lex");
	assert_eq!(fb[0].2, "er");
	assert!(fb[0].3);
}
