use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::link_browser::{
    FOOTER, FOOTER_CROWDED, HEAD, LinkBrowserCallbacks, LinkBrowserComponent,
    LinkBrowserTranscript,
};
use maple_tui::render::extension_links::set_extension_links;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

static SCHEME_LOCK: Mutex<()> = Mutex::new(());

fn marked(id: u32, url: &str, text: &str) -> String {
    format!("\x1b]8;id=omp-extension-link-{id};{url}\x07{text}\x1b]8;;\x07")
}

struct FakeTranscript {
    rows: Vec<String>,
    older: Vec<String>,
    loaded: bool,
    visible_end: usize,
}

impl FakeTranscript {
    fn new(rows: Vec<String>) -> Self {
        FakeTranscript {
            rows,
            older: Vec::new(),
            loaded: false,
            visible_end: 0,
        }
    }
}

impl LinkBrowserTranscript for FakeTranscript {
    fn body(&mut self, _content_width: usize) -> Vec<String> {
        self.rows.clone()
    }

    fn truncated(&self) -> bool {
        !self.older.is_empty() && !self.loaded
    }

    fn load_older(&mut self, _content_width: usize) -> usize {
        if self.loaded {
            return 0;
        }
        self.loaded = true;
        let added = self.older.len();
        let mut all = self.older.clone();
        all.extend(self.rows.clone());
        self.rows = all;
        added
    }

    fn rerender_message(&mut self, _timestamp: i64) -> bool {
        true
    }

    fn visible_end(&self) -> usize {
        self.visible_end
    }
}

fn browser(
    transcript: FakeTranscript,
) -> (
    LinkBrowserComponent,
    Rc<RefCell<Vec<String>>>,
) {
    let activated = Rc::new(RefCell::new(Vec::new()));
    let sink = activated.clone();
    let mut callbacks = LinkBrowserCallbacks::default();
    callbacks.on_activate = Some(Box::new(move |url| {
        sink.borrow_mut().push(url.to_string());
    }));
    (
        LinkBrowserComponent::new(Box::new(transcript), callbacks),
        activated,
    )
}

#[test]
fn opens_at_the_bottom_and_tags_links() {
    let _guard = SCHEME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    set_extension_links(["omp"]);
    let rows: Vec<String> = (0..30)
        .map(|index| {
            if index == 29 {
                format!("see {}", marked(1, "omp://target", "Target"))
            } else {
                format!("row {index}")
            }
        })
        .collect();
    let (mut browser, _activated) = browser(FakeTranscript::new(rows));
    browser.set_viewport_rows(5);
    let frame = browser.render(80);
    assert_eq!(frame[0], HEAD);
    assert_eq!(frame.last().map(String::as_str), Some(FOOTER));
    assert_eq!(frame.len(), 7, "header + 5 viewport rows + footer");
    assert!(frame.iter().any(|row| row.contains("Target")));
    assert!(frame.iter().any(|row| row.contains('\u{1b}')), "tag painted");
    assert_eq!(browser.offset(), 25);
}

#[test]
fn typing_a_tag_activates_its_link_and_stays_open() {
    let _guard = SCHEME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    set_extension_links(["omp"]);
    let rows = vec![format!("see {}", marked(1, "omp://a", "Alpha"))];
    let (mut browser, activated) = browser(FakeTranscript::new(rows));
    browser.set_viewport_rows(4);
    let _ = browser.render(80);
    assert!(browser.handle_key(&Key::Char('a')));
    assert_eq!(&*activated.borrow(), &["omp://a".to_string()]);
    assert!(browser.frame().len() >= 2);
}

#[test]
fn click_picks_the_label_under_the_pointer() {
    let _guard = SCHEME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    set_extension_links(["omp"]);
    let rows = vec![format!("go {}", marked(1, "omp://b", "Beta"))];
    let (mut browser, activated) = browser(FakeTranscript::new(rows));
    browser.set_viewport_rows(4);
    let _ = browser.render(80);
    assert!(browser.click(1, 4));
    assert_eq!(&*activated.borrow(), &["omp://b".to_string()]);
    assert!(!browser.click(1, 0));
    assert!(!browser.click(99, 0));
}

#[test]
fn scroll_keys_move_the_viewport_and_clamp() {
    let _guard = SCHEME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    set_extension_links(["omp"]);
    let rows: Vec<String> = (0..20).map(|index| format!("row {index}")).collect();
    let (mut browser, _) = browser(FakeTranscript::new(rows));
    browser.set_viewport_rows(5);
    let _ = browser.render(60);
    assert_eq!(browser.offset(), 15);
    assert!(browser.handle_key(&Key::PageUp));
    assert_eq!(browser.offset(), 10);
    browser.handle_key(&Key::Home);
    assert_eq!(browser.offset(), 0);
    browser.handle_key(&Key::PageDown);
    assert_eq!(browser.offset(), 5);
    browser.handle_key(&Key::End);
    assert_eq!(browser.offset(), 15);
    assert!(browser.handle_key(&Key::Up));
    assert_eq!(browser.offset(), 14);
}

#[test]
fn older_history_loads_at_the_top() {
    let _guard = SCHEME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    set_extension_links(["omp"]);
    let rows: Vec<String> = (0..5).map(|index| format!("recent {index}")).collect();
    let mut transcript = FakeTranscript::new(rows);
    transcript.older = (0..4).map(|index| format!("old {index}")).collect();
    let (mut browser, _) = browser(transcript);
    browser.set_viewport_rows(5);
    let _ = browser.render(60);
    assert_eq!(browser.offset(), 0);
    assert!(browser.handle_key(&Key::Home));
    assert_eq!(browser.offset(), 4, "older rows keep the view in place");
}

#[test]
fn crowded_rows_drop_the_tags_and_say_why() {
    let _guard = SCHEME_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    set_extension_links(["omp"]);
    let mut row = String::new();
    for index in 0..40 {
        row.push_str(&marked(index, &format!("omp://{index}"), "x"));
        row.push(' ');
    }
    let (mut browser, _) = browser(FakeTranscript::new(vec![row]));
    browser.set_viewport_rows(3);
    let frame = browser.render(6);
    assert!(browser.crowded());
    assert_eq!(frame.last().map(String::as_str), Some(FOOTER_CROWDED));
    assert!(!browser.handle_key(&Key::Char('a')));
}
