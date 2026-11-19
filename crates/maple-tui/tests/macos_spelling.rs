use maple_tui::prompt::macos_spelling::{
    EditorInlineReplacement, MacOSSpellingFeatures, MacOSSpellingProvider, SpellingBackend,
    SpellingRange,
};
use maple_tui::prompt::magic_keywords::{set_magic_keywords, MagicKeywordSpec};
use maple_tui::prompt::prose_gate::SpellingDecorationContext;
use maple_tui::prompt::typing_word::TypingWord;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FakeBackend {
    available: bool,
    ranges: Mutex<Vec<SpellingRange>>,
    autocorrect: Mutex<Option<String>>,
    guesses: Mutex<Vec<String>>,
    checks: AtomicUsize,
}

impl SpellingBackend for FakeBackend {
    fn is_available(&self) -> bool {
        self.available
    }

    fn check_spelling(&self, _text: &str) -> Vec<SpellingRange> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        self.ranges.lock().unwrap().clone()
    }

    fn autocorrect_word(&self, _text: &str, _start: usize, _length: usize) -> Option<String> {
        self.autocorrect.lock().unwrap().clone()
    }

    fn spelling_guesses(&self, _text: &str, _start: usize, _length: usize) -> Vec<String> {
        self.guesses.lock().unwrap().clone()
    }
}

fn context(text: &str) -> SpellingDecorationContext {
    SpellingDecorationContext {
        editor_text: text.to_string(),
        lines: vec![text.to_string()],
        line: 0,
        start_col: 0,
        typing: None,
    }
}

fn provider(backend: Arc<FakeBackend>) -> MacOSSpellingProvider {
    let mut provider = MacOSSpellingProvider::new(backend, true);
    provider.set_features(MacOSSpellingFeatures {
        typo_detection: true,
        autocorrect: true,
    });
    provider
}

#[test]
fn decorate_marks_typo_ranges_with_styled_undercurls() {
    let backend = Arc::new(FakeBackend {
        available: true,
        ranges: Mutex::new(vec![SpellingRange { start: 6, length: 5 }]),
        ..FakeBackend::default()
    });
    let mut spelling = provider(backend);
    let text = "hello wrold";
    let ranges = spelling.typo_ranges(text, &context(text));
    assert_eq!(ranges, vec![SpellingRange { start: 6, length: 5 }]);

    let painted = spelling.decorate_typos(text, &context(text), &(|piece: &str| piece.to_string()));
    assert!(painted.contains("\x1b[4:3m"), "styled undercurl on: {painted:?}");
    assert!(painted.contains("wrold"));
    assert!(painted.contains("\x1b[4:0m"));
}

#[test]
fn overlapping_backend_ranges_never_reemit_text() {
    let backend = Arc::new(FakeBackend {
        available: true,
        ranges: Mutex::new(vec![
            SpellingRange { start: 0, length: 3 },
            SpellingRange { start: 1, length: 2 },
        ]),
        ..FakeBackend::default()
    });
    let mut spelling = provider(backend);
    let text = "abcdef";
    let ranges = spelling.typo_ranges(text, &context(text));
    assert_eq!(ranges, vec![SpellingRange { start: 0, length: 3 }]);
}

#[test]
fn typing_word_holds_its_mark_but_keeps_others() {
    let backend = Arc::new(FakeBackend {
        ranges: Mutex::new(vec![
            SpellingRange { start: 0, length: 4 },
            SpellingRange { start: 6, length: 5 },
        ]),
        ..FakeBackend::default()
    });
    let mut spelling = provider(backend);
    let text = "wrold wrold";
    let mut ctx = context(text);
    ctx.typing = Some(TypingWord {
        line: 0,
        start: 6,
        end: 11,
    });
    let ranges = spelling.typo_ranges(text, &ctx);
    assert_eq!(ranges, vec![SpellingRange { start: 0, length: 4 }]);
}

#[test]
fn autocorrect_is_independently_gated_from_typo_detection() {
    let backend = Arc::new(FakeBackend {
        available: true,
        ranges: Mutex::new(vec![SpellingRange { start: 0, length: 5 }]),
        autocorrect: Mutex::new(Some("world".to_string())),
        ..FakeBackend::default()
    });
    let mut spelling = MacOSSpellingProvider::new(backend, false);
    let lines = vec!["hellp ".to_string()];

    spelling.set_features(MacOSSpellingFeatures {
        typo_detection: true,
        autocorrect: false,
    });
    assert_eq!(
        spelling.try_autocorrect(&lines, 0, 6),
        None,
        "typo detection alone must not autocorrect"
    );

    spelling.set_features(MacOSSpellingFeatures {
        typo_detection: false,
        autocorrect: true,
    });
    assert!(
        spelling.typo_ranges("hellp", &context("hellp")).is_empty(),
        "autocorrect alone must not check spelling"
    );

    spelling.set_features(MacOSSpellingFeatures {
        typo_detection: true,
        autocorrect: true,
    });
    assert_eq!(
        spelling.try_autocorrect(&lines, 0, 6),
        Some(EditorInlineReplacement {
            replace_len: 6,
            insert: "world ".to_string(),
        })
    );
}

#[test]
fn magic_keywords_are_exempt_from_autocorrect() {
    set_magic_keywords(&[MagicKeywordSpec {
        word: "mapleflow".to_string(),
        hue: (0.0, 120.0),
        lightness: None,
    }]);
    let backend = Arc::new(FakeBackend {
        available: true,
        autocorrect: Mutex::new(Some("mapleflu".to_string())),
        ..FakeBackend::default()
    });
    let mut spelling = provider(backend);
    let lines = vec!["mapleflow ".to_string()];
    assert_eq!(spelling.try_autocorrect(&lines, 0, 10), None);
    let plain = vec!["hellp ".to_string()];
    assert!(spelling.try_autocorrect(&plain, 0, 6).is_some());
}

#[test]
fn huge_buffers_get_no_spelling_work() {
    let backend = Arc::new(FakeBackend {
        available: true,
        ranges: Mutex::new(vec![SpellingRange { start: 0, length: 1 }]),
        ..FakeBackend::default()
    });
    let mut spelling = provider(backend.clone());
    let text = "x ".repeat(20_001);
    assert!(spelling.typo_ranges(&text, &context(&text)).is_empty());
    assert_eq!(backend.checks.load(Ordering::SeqCst), 0);
}

#[test]
fn word_replacements_come_from_the_backend_guesses() {
    let backend = Arc::new(FakeBackend {
        available: true,
        ranges: Mutex::new(vec![SpellingRange { start: 0, length: 5 }]),
        guesses: Mutex::new(vec![
            "world".to_string(),
            "world".to_string(),
            "worth".to_string(),
        ]),
        ..FakeBackend::default()
    });
    let mut spelling = provider(backend);
    let lines = vec!["hellp world".to_string()];
    let replacements = spelling
        .get_word_replacements(&lines, 0, 2)
        .expect("replacements");
    assert_eq!(replacements.start_col, 0);
    assert_eq!(replacements.end_col, 5);
    assert_eq!(replacements.items, vec!["world".to_string(), "worth".to_string()]);
}
