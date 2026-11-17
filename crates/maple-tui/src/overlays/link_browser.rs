//! Reference: `l/src/overlays/link-browser.ts`.
//!
//! Fullscreen extension-link browser: replays the transcript snapshot, tags
//! every extension link in view with a typeable hint, scrolls with the usual
//! keys/wheel, picks a link by tag or click, and stays open afterwards.
//!
//! Deviations:
//! - `ChatTranscriptBuilder`/`AssistantMessageComponent`/`TranscriptBrowser`
//!   have no crate module yet, so the replayed transcript is the injected
//!   [`LinkBrowserTranscript`] trait. It owns the snapshot, the outline
//!   layout, and the older-history replay; this module owns the viewport,
//!   hint tags, scroll floor, and pick routing.
//! - Mouse input is delivered as [`LinkBrowserComponent::click`] /
//!   [`LinkBrowserComponent::wheel`] by the host's SGR router, not parsed from
//!   raw bytes here (`routeSgrMouseInput`).
//! - `TUI` is replaced by a viewport height the host reports through
//!   [`LinkBrowserComponent::set_viewport_rows`].
//! - Theme colors for the tags are supplied by the host through
//!   [`LinkBrowserComponent::set_theme`]; the header/footer strings keep the
//!   reference wording.

use crate::component::Component;
use crate::keys::{is_arrow_down, is_arrow_up, is_escape, is_home, is_end, is_page_down, is_page_up, Key};
use crate::render::extension_links::extension_link_labels;
use crate::render::link_hints::{LinkHints, TypeLetter};
use crate::text_shape::visible_width;
use crate::theme::Theme;

/// Header line of the browser frame.
pub const HEAD: &str = "Extension links · pick one anywhere in the transcript";

/// Footer when every row's tags fit.
pub const FOOTER: &str = "Type a tag or click a link · PgUp/PgDn scroll · Esc close";

/// Footer when a row draws more links than its tags can be read on.
pub const FOOTER_CROWDED: &str =
	"Too many links on one row to tag; widen the terminal · PgUp/PgDn scroll · Esc close";

/// Rows one wheel notch scrolls.
pub const WHEEL_STEP: usize = 3;

/// The replayed transcript behind a [`LinkBrowserComponent`].
pub trait LinkBrowserTranscript {
	/// The whole transcript's rows at `content_width`.
	fn body(&mut self, content_width: usize) -> Vec<String>;
	/// Whether older history is still unreplayed.
	fn truncated(&self) -> bool;
	/// Replay all history; returns the rows prepended above the view (0 when
	/// there was nothing older).
	fn load_older(&mut self, content_width: usize) -> usize;
	/// Redraw the reply sent at `timestamp`; returns whether one was found.
	fn rerender_message(&mut self, timestamp: i64) -> bool;
	/// Component row heights of the last layout, oldest first; empty when the
	/// transcript cannot report them.
	fn heights(&mut self) -> Vec<usize> {
		Vec::new()
	}
	/// How many components the older history added above the replayed ones.
	fn older_child_count(&self) -> usize {
		0
	}
	/// Row index just past the visible viewport, for the scroll floor.
	fn visible_end(&self) -> usize {
		0
	}
}

/// Host callbacks.
#[derive(Default)]
pub struct LinkBrowserCallbacks {
	pub on_activate: Option<Box<dyn FnMut(&str)>>,
	pub on_close: Option<Box<dyn FnMut()>>,
	pub request_render: Option<Box<dyn FnMut()>>,
}

/// The browser.
pub struct LinkBrowserComponent {
	transcript: Box<dyn LinkBrowserTranscript>,
	callbacks: LinkBrowserCallbacks,
	hints: LinkHints,
	/// The last frame, header first; what a click is traced back through.
	frame: Vec<String>,
	/// Some row in view draws more links than their tags fit side by side.
	crowded: bool,
	/// Opens at the bottom; afterwards only the user's requests move it.
	scroll_to_end: bool,
	/// Rows above the viewport.
	offset: usize,
	viewport_rows: usize,
	/// Frame width of the last render, 0 before the first.
	width: usize,
	/// Transcript components the older history added above the replayed ones.
	older_children: usize,
	/// Rows those components take at the last layout.
	prepended_rows: usize,
	/// Body rows held after a reply was redrawn, so the offset is not clamped.
	row_floor: usize,
	theme: Theme,
}

impl LinkBrowserComponent {
	pub fn new(transcript: Box<dyn LinkBrowserTranscript>, callbacks: LinkBrowserCallbacks) -> Self {
		LinkBrowserComponent {
			transcript,
			callbacks,
			hints: LinkHints::new(),
			frame: Vec::new(),
			crowded: false,
			scroll_to_end: true,
			offset: 0,
			viewport_rows: 20,
			width: 0,
			older_children: 0,
			prepended_rows: 0,
			row_floor: 0,
			theme: crate::theme::default_theme(),
		}
	}

	pub fn set_theme(&mut self, theme: Theme) {
		self.theme = theme;
	}

	/// Rows the host's terminal shows.
	pub fn set_viewport_rows(&mut self, rows: usize) {
		self.viewport_rows = rows.max(1);
	}

	#[must_use]
	pub fn viewport_rows(&self) -> usize {
		self.viewport_rows
	}

	#[must_use]
	pub fn offset(&self) -> usize {
		self.offset
	}

	#[must_use]
	pub fn crowded(&self) -> bool {
		self.crowded
	}

	/// The last rendered frame, header first.
	#[must_use]
	pub fn frame(&self) -> &[String] {
		&self.frame
	}

	/// Whether the transcript draws any extension link at frame width `width`.
	pub fn has_links(&mut self, width: usize) -> bool {
		let content_width = width.saturating_sub(1).max(1);
		if self
			.transcript
			.body(content_width)
			.iter()
			.any(|row| !extension_link_labels(row).is_empty())
		{
			return true;
		}
		if !self.transcript.truncated() {
			return false;
		}
		self.load_older();
		self.transcript
			.body(content_width)
			.iter()
			.any(|row| !extension_link_labels(row).is_empty())
	}

	/// Redraw the reply at `timestamp`; tags below it move with their rows.
	/// Returns whether the transcript held such a reply.
	pub fn rerender_message(&mut self, timestamp: i64) -> bool {
		let content_width = self.width.saturating_sub(1).max(1);
		let before = if self.width > 0 {
			Some(self.transcript.heights())
		} else {
			None
		};
		if !self.transcript.rerender_message(timestamp) {
			return false;
		}
		if let Some(before) = before {
			let after = self.transcript.heights();
			if !before.is_empty() && before.len() == after.len() {
				let mut moves: Vec<(usize, isize)> = Vec::new();
				let mut end = 0usize;
				let mut by = 0isize;
				for (index, height) in after.iter().enumerate() {
					end += before[index];
					if *height == before[index] {
						continue;
					}
					by += *height as isize - before[index] as isize;
					moves.push((end, by));
				}
				let prepended = self.prepended_rows;
				self.prepended_rows = self.older_rows(&after);
				let shift = prepended as isize - self.prepended_rows as isize;
				let moves_for = |row: usize| -> isize {
					let target = row + prepended;
					moves
						.iter()
						.rev()
						.find(|(end, _)| target >= *end)
						.map(|(_, by)| *by)
						.unwrap_or(0)
				};

			}
			self.row_floor = self.transcript.visible_end().max(self.row_floor);
		}
		self.request_render();
		true
	}

	/// Rows the older history's components take, given every height.
	fn older_rows(&self, heights: &[usize]) -> usize {
		heights.iter().take(self.older_children).sum()
	}

	fn load_older(&mut self) -> bool {
		if !self.transcript.truncated() {
			return false;
		}
		let content_width = self.width.saturating_sub(1).max(1);
		let before = self.transcript.body(content_width).len();
		let added = self.transcript.load_older(content_width);
		self.older_children = self.transcript.older_child_count();
		if self.width == 0 {
			return true;
		}
		let after = self.transcript.body(content_width).len();
		self.offset = self.offset.saturating_add(added.max(after.saturating_sub(before)));
		true
	}

	/// Replay older history once the view reaches the top.
	fn load_older_at_top(&mut self) -> bool {
		if self.offset == 0 {
			self.load_older()
		} else {
			false
		}
	}

	/// Scroll by `delta` rows within the transcript.
	pub fn scroll(&mut self, delta: isize) -> bool {
		let content_width = self.width.saturating_sub(1).max(1);
		let rows = self.transcript.body(content_width).len().max(self.row_floor);
		let max = rows.saturating_sub(self.viewport_rows);
		let next = (self.offset as isize + delta).clamp(0, max as isize);
		if next as usize == self.offset {
			return false;
		}
		self.offset = next as usize;
		true
	}

	pub fn scroll_to_top(&mut self) {
		self.offset = 0;
	}

	pub fn scroll_to_bottom(&mut self) {
		let content_width = self.width.saturating_sub(1).max(1);
		let rows = self.transcript.body(content_width).len();
		self.offset = rows.saturating_sub(self.viewport_rows);
	}

	/// The visible body rows, without tags.
	#[must_use]
	pub fn viewport_rows_text(&mut self) -> Vec<String> {
		let content_width = self.width.saturating_sub(1).max(1);
		let body = self.transcript.body(content_width);
		let end = body.len().min(self.offset + self.viewport_rows);
		body.get(self.offset..end).map(<[String]>::to_vec).unwrap_or_default()
	}

	fn request_render(&mut self) {
		if let Some(callback) = self.callbacks.request_render.as_mut() {
			callback();
		}
	}

	fn activate(&mut self, url: &str) {
		if let Some(callback) = self.callbacks.on_activate.as_mut() {
			callback(url);
		}
	}

	/// Left click at a frame-relative `row`/`col`: picks a link label under
	/// the pointer. The frame includes the header row.
	pub fn click(&mut self, row: usize, col: usize) -> bool {
		let Some(line) = self.frame.get(row) else {
			return false;
		};
		let Some(label) = extension_link_labels(line)
			.into_iter()
			.find(|label| col >= label.start && col < label.end)
		else {
			return false;
		};
		self.activate(&label.url);
		true
	}

	/// Wheel scrolling; `notches > 0` scrolls down.
	pub fn wheel(&mut self, notches: isize) -> bool {
		let moved = self.scroll(notches * WHEEL_STEP as isize);
		self.row_floor = 0;
		let loaded = self.load_older_at_top();
		if moved || loaded {
			self.request_render();
		}
		moved || loaded
	}

	/// Keys the browser binds itself (Esc, scroll keys, hint letters).
	/// Returns whether the key was used.
	pub fn handle_key(&mut self, key: &Key) -> bool {
		if is_escape(key) {
			if let Some(callback) = self.callbacks.on_close.as_mut() {
				callback();
			}
			return true;
		}
		if is_page_up(key) {
			self.scroll(-(self.viewport_rows as isize));
			self.row_floor = 0;
			self.load_older_at_top();
			self.request_render();
			return true;
		}
		if is_page_down(key) {
			self.scroll(self.viewport_rows as isize);
			self.row_floor = 0;
			self.request_render();
			return true;
		}
		if is_arrow_up(key) {
			self.scroll(-1);
			self.row_floor = 0;
			self.request_render();
			return true;
		}
		if is_arrow_down(key) {
			self.scroll(1);
			self.row_floor = 0;
			self.request_render();
			return true;
		}
		if is_home(key) {
			self.scroll_to_top();
			self.row_floor = 0;
			self.load_older_at_top();
			self.request_render();
			return true;
		}
		if is_end(key) {
			self.scroll_to_bottom();
			self.row_floor = 0;
			self.request_render();
			return true;
		}
		if self.crowded {
			return false;
		}
		let letter = match key {
			Key::Char(ch) if ch.is_ascii_alphabetic() => Some(ch.to_string()),
			_ => None,
		};
		let Some(letter) = letter else {
			return false;
		};
		if let TypeLetter::Picked(link) = self.hints.type_letter(&letter) {
			self.activate(&link.url);
		}
		self.request_render();
		true
	}
}

impl Component for LinkBrowserComponent {
	/// Header, the tagged viewport, and the footer.
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width as usize;
		if width != self.width {
			self.row_floor = 0;
		}
		self.width = width;
		if self.scroll_to_end {
			self.scroll_to_end = false;
			self.scroll_to_bottom();
		}
		let content_width = width.saturating_sub(1).max(1);
		let body = self.transcript.body(content_width);
		let end = body.len().min(self.offset + self.viewport_rows);
		let visible: Vec<String> = body
			.get(self.offset..end)
			.map(<[String]>::to_vec)
			.unwrap_or_default();
		let trimmed: Vec<String> = visible
			.iter()
			.map(|row| {
				crate::render::render_utils::slice_by_column(row, 0, content_width, true).text
			})
			.collect();
		self.hints
			.update(&trimmed, self.offset.saturating_sub(self.prepended_rows));
		let crowded = self.hints.crowded(content_width);
		self.crowded = crowded;
		let painted = if crowded {
			trimmed
		} else {
			self.hints.paint(&trimmed, content_width, &self.theme)
		};
		let mut frame = Vec::with_capacity(painted.len() + 2);
		frame.push(HEAD.to_string());
		frame.extend(painted);
		frame.push(if crowded { FOOTER_CROWDED } else { FOOTER }.to_string());
		self.frame = frame.clone();
		frame
	}

	fn handle_input(&mut self, key: &Key) {
		self.handle_key(key);
	}

	fn invalidate(&mut self) {
		self.frame.clear();
	}
}

/// Width-checked slice of one row (kept here so the browser owns its column
/// policy without an `ansi` dependency).
#[must_use]
pub fn row_fits(row: &str, width: usize) -> bool {
	visible_width(row) <= width
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::render::extension_links::{set_extension_links, test_lock};

	fn marked(id: u32, url: &str, text: &str) -> String {
		format!("\x1b]8;id=omp-extension-link-{id};{url}\x07{text}\x1b]8;;\x07")
	}

	struct FakeTranscript {
		rows: Vec<String>,
		older: Vec<String>,
		loaded: bool,
		heights: Vec<usize>,
		visible_end: usize,
		rerendered: Vec<i64>,
	}

	impl FakeTranscript {
		fn new(rows: Vec<String>) -> Self {
			FakeTranscript {
				rows,
				older: Vec::new(),
				loaded: false,
				heights: Vec::new(),
				visible_end: 0,
				rerendered: Vec::new(),
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

		fn rerender_message(&mut self, timestamp: i64) -> bool {
			self.rerendered.push(timestamp);
			true
		}

		fn heights(&mut self) -> Vec<usize> {
			self.heights.clone()
		}

		fn visible_end(&self) -> usize {
			self.visible_end
		}
	}

	fn browser(transcript: FakeTranscript) -> (LinkBrowserComponent, std::rc::Rc<std::cell::RefCell<Vec<String>>>) {
		let activated = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = activated.clone();
		let mut callbacks = LinkBrowserCallbacks::default();
		callbacks.on_activate = Some(Box::new(move |url| sink.borrow_mut().push(url.to_string())));
		let browser = LinkBrowserComponent::new(Box::new(transcript), callbacks);
		(browser, activated)
	}

	#[test]
	fn render_opens_at_the_bottom_and_tags_links() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
	fn typing_a_tag_activates_its_link() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let rows = vec![format!("see {}", marked(1, "omp://a", "Alpha"))];
		let (mut browser, activated) = browser(FakeTranscript::new(rows));
		browser.set_viewport_rows(4);
		let _ = browser.render(80);
		// Tags prefer the label's letters: "a".
		assert!(browser.handle_key(&Key::Char('a')));
		assert_eq!(&*activated.borrow(), &["omp://a".to_string()]);
		// The browser stays open after a pick.
		assert!(browser.frame().len() >= 2);
	}

	#[test]
	fn click_picks_the_label_under_the_pointer() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let rows = vec![format!("go {}", marked(1, "omp://b", "Beta"))];
		let (mut browser, activated) = browser(FakeTranscript::new(rows));
		browser.set_viewport_rows(4);
		let _ = browser.render(80);
		// Header occupies frame row 0; the label starts at cell 3 ("go ").
		assert!(browser.click(1, 4));
		assert_eq!(&*activated.borrow(), &["omp://b".to_string()]);
		assert!(!browser.click(1, 0));
		assert!(!browser.click(99, 0));
	}

	#[test]
	fn scroll_keys_move_the_viewport_and_clamp() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
	fn wheel_scrolls_three_rows_per_notch() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let rows: Vec<String> = (0..20).map(|index| format!("row {index}")).collect();
		let (mut browser, _) = browser(FakeTranscript::new(rows));
		browser.set_viewport_rows(5);
		let _ = browser.render(60);
		browser.scroll_to_top();
		assert!(browser.wheel(1));
		assert_eq!(browser.offset(), WHEEL_STEP);
		assert!(browser.wheel(-1));
		assert_eq!(browser.offset(), 0);
		assert!(!browser.wheel(-1));
	}

	#[test]
	fn older_history_loads_at_the_top() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
	fn has_links_checks_recent_then_whole_branch() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let plain = vec!["nothing here".to_string()];
		let (mut browser, _) = browser(FakeTranscript::new(plain));
		assert!(!browser.has_links(40));
		let mut transcript = FakeTranscript::new(vec!["fresh".to_string()]);
		transcript.older = vec![format!("old {}", marked(1, "omp://old", "Old"))];
		let (mut browser, _) = browser(transcript);
		assert!(browser.has_links(40));
	}

	#[test]
	fn escape_closes_and_other_printables_are_reported() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let (mut browser, _) = browser(FakeTranscript::new(vec!["plain".to_string()]));
		let closed = std::rc::Rc::new(std::cell::RefCell::new(0));
		let sink = closed.clone();
		browser.callbacks.on_close = Some(Box::new(move || *sink.borrow_mut() += 1));
		assert!(browser.handle_key(&Key::Esc));
		assert_eq!(*closed.borrow(), 1);
		assert!(!browser.handle_key(&Key::Char('z')));
	}

	#[test]
	fn crowded_rows_drop_the_tags_and_say_why() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		// 27 labels exhaust one-letter tags; the 28th re-tags with two letters,
		// and a very narrow width makes the row crowded.
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

	#[test]
	fn rerender_reports_and_sets_the_row_floor() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let mut transcript = FakeTranscript::new(vec!["a".to_string(), "b".to_string()]);
		transcript.heights = vec![1, 1];
		transcript.visible_end = 2;
		let (mut browser, _) = browser(transcript);
		browser.set_viewport_rows(4);
		let _ = browser.render(40);
		assert!(browser.rerender_message(1234));
	}

	#[test]
	fn scroll_to_bottom_clamps_when_content_is_short() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let (mut browser, _) = browser(FakeTranscript::new(vec!["only".to_string()]));
		browser.set_viewport_rows(10);
		browser.scroll_to_bottom();
		assert_eq!(browser.offset(), 0);
		assert_eq!(browser.viewport_rows_text(), vec!["only".to_string()]);
		assert!(row_fits("short", 5));
	}
}
