//! Reference: `l/src/prompt/word-completion.ts`.
//!
//! Ghost-text word completion for the prose word ending at the cursor, served by
//! an injected [`WordPredictionBackend`]. Rendering is synchronous, so the
//! previous suggestion is projected: if its full word still extends what was
//! typed, the remainder shows immediately and stays until the engine offers a
//! different word.
//!
//! Deviations:
//! - `complete` is synchronous here; the reference awaits it, keeps a single
//!   in-flight request and queues the newest. A Rust cache miss asks the backend
//!   once and stores the answer, so only the projection path survives.
//! - The host resolver lives in a process-wide `Mutex` (the reference's
//!   module-level variable) and [`WordCompletionQuery`] is returned by value.
//! - Offsets count characters, not UTF-16 code units, so `textBefore` cannot cut
//!   a surrogate pair.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use crate::prompt::markdown_prose::mask_non_prose;
use crate::prompt::prose_gate::{is_prose_word, line_context, ProseSource};

/// Values of the `spelling.autocomplete` setting.
pub const WORD_COMPLETION_METHODS: [&str; 5] = ["off", "auto", "ngram", "smollm", "apple"];

/// Configured word-completion engine; `off` disables ghost text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WordCompletionMethod {
	#[default]
	Off,
	Auto,
	Ngram,
	Smollm,
	Apple,
}

impl WordCompletionMethod {
	/// The setting value.
	pub fn as_str(self) -> &'static str {
		match self {
			WordCompletionMethod::Off => "off",
			WordCompletionMethod::Auto => "auto",
			WordCompletionMethod::Ngram => "ngram",
			WordCompletionMethod::Smollm => "smollm",
			WordCompletionMethod::Apple => "apple",
		}
	}

	/// Parse a setting value.
	pub fn from_str(value: &str) -> Option<Self> {
		Some(match value {
			"off" => WordCompletionMethod::Off,
			"auto" => WordCompletionMethod::Auto,
			"ngram" => WordCompletionMethod::Ngram,
			"smollm" => WordCompletionMethod::Smollm,
			"apple" => WordCompletionMethod::Apple,
			_ => return None,
		})
	}

	/// The enabled engine, or `None` for `off`.
	pub fn engine(self) -> Option<WordCompletionEngine> {
		match self {
			WordCompletionMethod::Off => None,
			WordCompletionMethod::Auto => Some(WordCompletionEngine::Auto),
			WordCompletionMethod::Ngram => Some(WordCompletionEngine::Ngram),
			WordCompletionMethod::Smollm => Some(WordCompletionEngine::Smollm),
			WordCompletionMethod::Apple => Some(WordCompletionEngine::Apple),
		}
	}
}

/// Whether `value` is a [`WordCompletionMethod`].
pub fn is_word_completion_method(value: &str) -> bool {
	WordCompletionMethod::from_str(value).is_some()
}

/// An enabled word-completion engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WordCompletionEngine {
	Auto,
	Ngram,
	Smollm,
	Apple,
}

impl WordCompletionEngine {
	/// The setting value.
	pub fn as_str(self) -> &'static str {
		match self {
			WordCompletionEngine::Auto => "auto",
			WordCompletionEngine::Ngram => "ngram",
			WordCompletionEngine::Smollm => "smollm",
			WordCompletionEngine::Apple => "apple",
		}
	}
}

/// Asynchronous engine behind ghost-text word completion (the text-prediction
/// daemon in the coding agent).
pub trait WordPredictionBackend: Send + Sync {
	/// Characters to paint after `prefix` typed after `before`, or `None` to show
	/// nothing.
	fn complete(&self, before: &str, prefix: &str) -> Option<String>;
	/// Report a shown suggestion the user accepted (Tab/→) or typed past.
	fn feedback(&self, _before: &str, _prefix: &str, _suggestion: &str, _accepted: bool) {}
}

/// Resolves the backend for an engine; `None` means none is available (no ghost
/// text).
pub type WordPredictionBackendResolver =
	Arc<dyn Fn(WordCompletionEngine) -> Option<Arc<dyn WordPredictionBackend>> + Send + Sync>;

static HOST_RESOLVER: LazyLock<Mutex<Option<WordPredictionBackendResolver>>> =
	LazyLock::new(|| Mutex::new(None));

/// Install the host's backend resolver for every editor, including ones built
/// before the host finished starting. Pass `None` to detach.
pub fn set_word_prediction_host(resolver: Option<WordPredictionBackendResolver>) {
	*HOST_RESOLVER
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner()) = resolver;
}

fn host_resolver() -> Option<WordPredictionBackendResolver> {
	HOST_RESOLVER
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.clone()
}

const CACHE_LIMIT: usize = 256;
/// Context sent with each query: the editor text before the word, capped to a
/// recent tail.
const BEFORE_LIMIT: usize = 2_000;

/// The prose word ending at the cursor and the editor text before it: what an
/// engine is asked to complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordCompletionQuery {
	pub before: String,
	pub prefix: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WordQuery {
	key: String,
	before: String,
	prefix: String,
}

/// Characters that may continue or compose a prose word (`[\p{L}\p{M}']`).
fn is_word_char(ch: char) -> bool {
	ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}' || is_mark(ch)
}

fn is_mark(ch: char) -> bool {
	matches!(ch as u32,
		0x0300..=0x036F
		| 0x0483..=0x0489
		| 0x0591..=0x05BD
		| 0x0610..=0x061A
		| 0x064B..=0x065F
		| 0x0900..=0x0903
		| 0x093A..=0x094F
		| 0x0E31..=0x0E3A
		| 0x1AB0..=0x1AFF
		| 0x1DC0..=0x1DFF
		| 0x20D0..=0x20F0
		| 0xFE20..=0xFE2F)
}

/// Editor text before the word starting at `start`, truncated to its last
/// [`BEFORE_LIMIT`] characters.
fn text_before(lines: &[String], cursor_line: usize, start: usize) -> String {
	let line_chars: Vec<char> = lines
		.get(cursor_line)
		.map(|line| line.chars().collect())
		.unwrap_or_default();
	let head: String = line_chars.iter().take(start).collect();
	let mut length = head.chars().count();
	let mut first = cursor_line;
	while first > 0 && length < BEFORE_LIMIT {
		first -= 1;
		length += lines.get(first).map(|line| line.chars().count()).unwrap_or(0) + 1;
	}
	let text = if first < cursor_line {
		let mut parts: Vec<String> = (first..cursor_line)
			.map(|line| lines.get(line).cloned().unwrap_or_default())
			.collect();
		parts.push(head);
		parts.join("\n")
	} else {
		head
	};
	let chars: Vec<char> = text.chars().collect();
	if chars.len() <= BEFORE_LIMIT {
		return text;
	}
	chars[chars.len() - BEFORE_LIMIT..].iter().collect()
}

/// The query for the prose word ending at the cursor, or `None` where no ghost
/// text applies (mid-word, no word, code, paths, commands). `prose` memoizes the
/// whole-buffer mask; pass one instance across keystrokes.
pub fn word_completion_query(
	lines: &[String],
	cursor_line: usize,
	cursor_col: usize,
	prose: &mut ProseSource,
) -> Option<WordCompletionQuery> {
	let line = lines.get(cursor_line).map(String::as_str).unwrap_or("");
	let line_chars: Vec<char> = line.chars().collect();
	let cursor_col = cursor_col.min(line_chars.len());
	if line_chars.get(cursor_col).copied().is_some_and(is_word_char) {
		return None;
	}
	// Single letters are asked too (`figure o|ut`); engines own their minimum
	// prefix.
	let mut start = cursor_col;
	while start > 0 && is_word_char(line_chars[start - 1]) {
		start -= 1;
	}
	if start == cursor_col {
		return None;
	}
	let prefix: String = line_chars[start..cursor_col].iter().collect();
	if !is_prose_word(line, &mask_non_prose(line), start, cursor_col) {
		return None;
	}
	if !prose.is_prose(&line_context(lines, cursor_line), start, cursor_col) {
		return None;
	}
	Some(WordCompletionQuery {
		before: text_before(lines, cursor_line, start),
		prefix,
	})
}

/// Ghost-text word completion for the prose word ending at the cursor.
pub struct WordCompletionProvider {
	method: WordCompletionMethod,
	prose: ProseSource,
	cache: HashMap<String, Option<String>>,
	/// Last non-empty suggestion shown, for projection while typing through it.
	last_shown: Option<(String, String)>,
	/// What the last `get_word_completion` returned, so a fetch repaints only
	/// when it changes that.
	displayed: Option<(String, Option<String>)>,
	/// Last [`Self::query`] input snapshot (lines are copied: the editor mutates
	/// its array in place).
	query_memo: Option<QueryMemo>,
	resolve_backend: WordPredictionBackendResolver,
	/// Invoked when an engine answer changes the rendered ghost text.
	pub on_update: Option<Box<dyn Fn()>>,
}

#[derive(Clone)]
struct QueryMemo {
	method: WordCompletionMethod,
	lines: Vec<String>,
	cursor_line: usize,
	cursor_col: usize,
	query: Option<WordQuery>,
}

impl WordCompletionProvider {
	/// `resolve_backend` defaults to the host installed with
	/// [`set_word_prediction_host`].
	pub fn new() -> Self {
		Self::with_resolver(Arc::new(|engine| host_resolver().and_then(|resolve| resolve(engine))))
	}

	/// Build with an explicit resolver.
	pub fn with_resolver(resolve_backend: WordPredictionBackendResolver) -> Self {
		Self {
			method: WordCompletionMethod::Off,
			prose: ProseSource::default(),
			cache: HashMap::new(),
			last_shown: None,
			displayed: None,
			query_memo: None,
			resolve_backend,
			on_update: None,
		}
	}

	/// The selected engine.
	pub fn method(&self) -> WordCompletionMethod {
		self.method
	}

	/// Select the engine (`off` disables completion) and drop answers from the
	/// previous one.
	pub fn set_method(&mut self, method: WordCompletionMethod) {
		if self.method == method {
			return;
		}
		self.method = method;
		self.cache.clear();
		self.last_shown = None;
		self.displayed = None;
		self.query_memo = None;
	}

	/// Return the ghost-text suffix for the prose word ending at the cursor, or
	/// `None`.
	pub fn get_word_completion(
		&mut self,
		lines: &[String],
		cursor_line: usize,
		cursor_col: usize,
	) -> Option<String> {
		let query = self.query(lines, cursor_line, cursor_col);
		let backend = query.as_ref().and_then(|_| self.backend());
		let (Some(query), Some(backend)) = (query, backend) else {
			self.displayed = None;
			return None;
		};
		if !self.cache.contains_key(&query.key) {
			self.schedule(backend, &query);
		}
		// Typing through a shown ghost without Tab can answer null (the word fell
		// under the show threshold); keep that ghost until the engine offers
		// something else.
		let suffix = self
			.cache
			.get(&query.key)
			.cloned()
			.flatten()
			.or_else(|| self.project(&query));
		self.displayed = Some((query.key.clone(), suffix.clone()));
		if let Some(suffix) = suffix.as_ref() {
			self.last_shown = Some((query.before.clone(), format!("{}{suffix}", query.prefix)));
		}
		suffix
	}

	/// Forward accept (Tab/→) or typed-past feedback for the suggestion shown at
	/// the cursor.
	pub fn word_completion_feedback(
		&mut self,
		lines: &[String],
		cursor_line: usize,
		cursor_col: usize,
		suggestion: &str,
		accepted: bool,
	) {
		let Some(query) = self.query(lines, cursor_line, cursor_col) else {
			return;
		};
		if let Some(backend) = self.backend() {
			backend.feedback(&query.before, &query.prefix, suggestion, accepted);
		}
		if !accepted {
			self.last_shown = None;
		}
	}

	fn backend(&self) -> Option<Arc<dyn WordPredictionBackend>> {
		let engine = self.method.engine()?;
		(self.resolve_backend)(engine)
	}

	fn query(
		&mut self,
		lines: &[String],
		cursor_line: usize,
		cursor_col: usize,
	) -> Option<WordQuery> {
		if self.method == WordCompletionMethod::Off {
			return None;
		}
		// Rendering asks every frame; reuse the answer until the buffer, caret, or
		// engine changes.
		if let Some(memo) = self.query_memo.as_ref() {
			if memo.method == self.method
				&& memo.cursor_line == cursor_line
				&& memo.cursor_col == cursor_col
				&& memo.lines == lines
			{
				return memo.query.clone();
			}
		}
		let query = word_completion_query(lines, cursor_line, cursor_col, &mut self.prose);
		// The method stays in the key so a request for the previous engine can
		// never shadow the same word state on the new one.
		let result = query.map(|query| WordQuery {
			key: format!("{}\u{0}{}\u{0}{}", self.method.as_str(), query.prefix, query.before),
			before: query.before,
			prefix: query.prefix,
		});
		self.query_memo = Some(QueryMemo {
			method: self.method,
			lines: lines.to_vec(),
			cursor_line,
			cursor_col,
			query: result.clone(),
		});
		result
	}

	fn project(&self, query: &WordQuery) -> Option<String> {
		let (before, word) = self.last_shown.as_ref()?;
		if *before != query.before || word.chars().count() <= query.prefix.chars().count() {
			return None;
		}
		if !word
			.to_lowercase()
			.starts_with(&query.prefix.to_lowercase())
		{
			return None;
		}
		let skip = query.prefix.chars().count();
		Some(word.chars().skip(skip).collect())
	}

	fn schedule(&mut self, backend: Arc<dyn WordPredictionBackend>, query: &WordQuery) {
		let suffix = backend.complete(&query.before, &query.prefix).filter(|suffix| !suffix.is_empty());
		if self.cache.len() >= CACHE_LIMIT {
			self.cache.clear();
		}
		self.cache.insert(query.key.clone(), suffix.clone());
		// Repaint only when the answer changes what is displayed.
		let projected = self.project(query);
		if let Some((key, displayed)) = self.displayed.as_ref() {
			if *key == query.key && *displayed != suffix.clone().or(projected) {
				if let Some(on_update) = self.on_update.as_ref() {
					on_update();
				}
			}
		}
	}
}

impl Default for WordCompletionProvider {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct FixedBackend {
		answer: Option<String>,
		feedback: Arc<Mutex<Vec<(String, String, bool)>>>,
	}

	impl WordPredictionBackend for FixedBackend {
		fn complete(&self, _before: &str, _prefix: &str) -> Option<String> {
			self.answer.clone()
		}

		fn feedback(&self, before: &str, prefix: &str, _suggestion: &str, accepted: bool) {
			self.feedback
				.lock()
				.expect("lock")
				.push((before.to_string(), prefix.to_string(), accepted));
		}
	}

	fn backend(answer: Option<&str>) -> (Arc<FixedBackend>, Arc<Mutex<Vec<(String, String, bool)>>>) {
		let feedback = Arc::new(Mutex::new(Vec::new()));
		(
			Arc::new(FixedBackend {
				answer: answer.map(str::to_string),
				feedback: Arc::clone(&feedback),
			}),
			feedback,
		)
	}

	fn provider(answer: Option<&str>) -> (WordCompletionProvider, Arc<Mutex<Vec<(String, String, bool)>>>) {
		let (fixed, feedback) = backend(answer);
		let mut provider = WordCompletionProvider::with_resolver(Arc::new(move |_engine| {
			Some(Arc::clone(&fixed) as Arc<dyn WordPredictionBackend>)
		}));
		provider.set_method(WordCompletionMethod::Smollm);
		(provider, feedback)
	}

	#[test]
	fn method_round_trips() {
		for method in WORD_COMPLETION_METHODS {
			let parsed = WordCompletionMethod::from_str(method).expect("method");
			assert_eq!(parsed.as_str(), method);
		}
		assert!(!is_word_completion_method("bogus"));
		assert_eq!(WordCompletionMethod::Off.engine(), None);
	}

	#[test]
	fn queries_the_word_at_the_cursor() {
		let mut prose = ProseSource::default();
		let lines = vec!["please figure out".to_string()];
		let query = word_completion_query(&lines, 0, 12, &mut prose).expect("query");
		assert_eq!(query.prefix, "figure");
		assert_eq!(query.before, "please ");
		// Mid-word, no word, and code-ish tokens never query.
		assert!(word_completion_query(&lines, 0, 9, &mut prose).is_none());
		assert!(word_completion_query(&["  ".to_string()], 0, 2, &mut prose).is_none());
		assert!(word_completion_query(&["src/main.rs".to_string()], 0, 7, &mut prose).is_none());
		assert!(word_completion_query(&["/help".to_string()], 0, 5, &mut prose).is_none());
	}

	#[test]
	fn caps_before_context() {
		let mut prose = ProseSource::default();
		let long = "x".repeat(BEFORE_LIMIT + 50);
		let lines = vec![format!("{long} word")];
		let col = long.chars().count() + 4;
		let query = word_completion_query(&lines, 0, col, &mut prose).expect("query");
		assert_eq!(query.prefix, "word");
		assert_eq!(query.before.chars().count(), BEFORE_LIMIT);
	}

	#[test]
	fn returns_and_projects_ghost_text() {
		let (mut provider, _feedback) = provider(Some(" figure it out"));
		let lines = vec!["please fig".to_string()];
		assert_eq!(
			provider.get_word_completion(&lines, 0, 10).as_deref(),
			Some(" figure it out")
		);

		// Typed one more character: the cached answer no longer applies, but the
		// projection still completes the word it started.
		provider.cache.clear();
		let typed = vec!["please figu".to_string()];
		assert_eq!(
			provider.get_word_completion(&typed, 0, 11).as_deref(),
			Some("re it out")
		);
	}

	#[test]
	fn off_method_shows_nothing() {
		let (mut provider, _feedback) = provider(Some("suffix"));
		provider.set_method(WordCompletionMethod::Off);
		assert_eq!(provider.get_word_completion(&["word".to_string()], 0, 4), None);
	}

	#[test]
	fn feedback_reaches_the_backend() {
		let (mut provider, feedback) = provider(Some("x"));
		let lines = vec!["word".to_string()];
		provider.word_completion_feedback(&lines, 0, 4, "x", true);
		assert_eq!(
			feedback.lock().expect("lock").clone(),
			vec![("".to_string(), "word".to_string(), true)]
		);
	}

	#[test]
	fn repaints_when_an_answer_changes_the_ghost() {
		use std::cell::Cell;
		use std::rc::Rc;

		let (mut provider, _feedback) = provider(Some("ing"));
		let paints = Rc::new(Cell::new(0));
		let counter = Rc::clone(&paints);
		provider.on_update = Some(Box::new(move || counter.set(counter.get() + 1)));
		let lines = vec!["run".to_string()];
		assert_eq!(provider.get_word_completion(&lines, 0, 3).as_deref(), Some("ing"));
		assert_eq!(paints.get(), 0);
	}
}
