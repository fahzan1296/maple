//! Terminal surface for an agent-written HTML form (parsed by `parse.rs`).
//!
//! Reference: `l/src/html-form/dialog.ts`.
//!
//! This is a clean-room Rust port focused on the public data/behavior contract:
//! host/options wiring, form state, focus/tab bookkeeping, value extraction,
//! validation/submit flow, option notes, and pagination/layout integration via
//! [`crate::html_form::paginate`].
//!
//! Deviations from the TypeScript implementation:
//! - The TS dialog is a live TUI component (`Component`, `Focusable`) tightly
//!   integrated with terminal rendering, keyboard/mouse/native events, timers,
//!   async media loading, image/SVG components, global theme/keybinding state,
//!   and ANSI/native description helpers. Maple-tui does not expose equivalent
//!   runtime APIs in this module; this port provides deterministic core logic
//!   and render/layout model types but does not implement the full interactive
//!   event loop or native describe protocol.
//! - `HtmlFormHost` is modeled as a trait (terminal rows + render request +
//!   optional image budget hook), and callbacks are Rust closures instead of JS
//!   function properties.
//! - Countdown timeout handling is represented as state/expiry helpers rather
//!   than a background timer; hosts drive ticking.
//! - Picture loading is synchronous through [`crate::html_form::media`] and
//!   represented as simple slots; no async abort/render scheduling.
//! - Rendering helpers produce plain/layout strings/placeholders instead of
//!   themed ANSI rows and native nodes; styling tokens and key hints are kept
//!   as text where needed.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::html_form::data::{
	choice_value, default_form_data, field_error, field_questions, text_value, HtmlFormData,
	HtmlFormValue, OptionNote, OTHER_LABEL,
};
use crate::html_form::media::{load_form_image, svg_media, FormMediaOptions};
use crate::html_form::paginate::{page_height, paginate, BreakRank, Page, PageAtom};
use crate::html_form::parse::{spans_text, ChoiceField, FormBlock, FormField, HtmlForm};

/// What the dialog needs from its host: terminal height, a render request, and optional image budget hook.
pub trait HtmlFormHost {
	fn terminal_rows(&self) -> u16;
	fn request_render(&self);
	fn image_budget(&self) -> Option<u64> {
		None
	}
}

/// Input guard for a draft that may own typing until handed to the form.
pub trait AskDialogInputGuard {
	fn is_blocked(&self) -> bool;
	fn handle_input(&self, _data: &str) {}
	fn sync_presentation(&self, _blocked: bool) {}
	fn hint(&self) -> &str {
		"↑/↓ answer"
	}
}

/// Host wiring for [`HtmlFormDialog`].
pub struct HtmlFormDialogOptions<'a> {
	/// Panel title; `Ask` when omitted.
	pub title: Option<String>,
	/// Sizes the panel and repaints when the countdown ticks or a picture lands.
	pub tui: Option<&'a dyn HtmlFormHost>,
	/// Inactivity after which `on_timeout` receives current values and notes.
	pub timeout: Option<Duration>,
	pub on_timeout: Option<Box<dyn FnMut(HtmlFormData, Vec<OptionNote>) + 'a>>,
	/// A composer draft that keeps typing until focus is handed to the form.
	pub input_guard: Option<&'a dyn AskDialogInputGuard>,
	/// Directory relative `<img src>` paths resolve against.
	pub cwd: Option<PathBuf>,
	pub on_submit: Box<dyn FnMut(HtmlFormData, Vec<OptionNote>) + 'a>,
	pub on_cancel: Box<dyn FnMut() + 'a>,
}

impl<'a> std::fmt::Debug for HtmlFormDialogOptions<'a> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("HtmlFormDialogOptions")
			.field("title", &self.title)
			.field("tui", &self.tui.is_some())
			.field("timeout", &self.timeout)
			.field("on_timeout", &self.on_timeout.is_some())
			.field("input_guard", &self.input_guard.is_some())
			.field("cwd", &self.cwd)
			.field("on_submit", &"<fn>")
			.field("on_cancel", &"<fn>")
			.finish()
	}
}

/// Fraction of the terminal the panel may occupy.
pub const DIALOG_HEIGHT_RATIO: f32 = 0.7;
pub const MIN_BODY_ROWS: usize = 6;
/// Top border, footer divider, footer, bottom border.
pub const CHROME_ROWS: usize = 4;
/// Column where field inputs and errors start.
pub const FIELD_INDENT: usize = 4;
pub const MAX_TEXTAREA_ROWS: usize = 10;
pub const DEFAULT_TITLE: &str = "Ask";
pub const SUBMIT_LABEL: &str = "Submit";
/// A text block this short right before a field is its caption and never splits from it.
pub const CAPTION_MAX_ROWS: usize = 4;
/// Tallest a picture draws, in rows, and its share of the terminal below that.
pub const MAX_PICTURE_ROWS: usize = 16;
pub const PICTURE_SHARE: f32 = 0.35;
/// Options a page move in an open dropdown's highlight by.
pub const DROPDOWN_PAGE: usize = 5;
pub const EDIT_ACT: &str = "edit:";
/// List item key of a group's Other answer (option items are `o<index>`).
pub const OTHER_ITEM: &str = "other";
pub const OTHER_PLACEHOLDER: &str = "type your own";
pub const TAB_LABEL_WIDTH: usize = 28;
pub const REVIEW_TAB: &str = "Review";
/// Tab id of the Review tab (question tabs are `t<index>`).
pub const REVIEW_ID: &str = "review";

/// A picture block's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaSlot {
	Loading,
	Failed { reason: String },
	Ready { mime: String, alt: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
	Option { field_index: usize, option: usize },
	/// The free-text Other answer of a radio/checkbox group.
	Other { field_index: usize },
	Select { field_index: usize },
	Text { field_index: usize },
	Submit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
	pub atoms: Vec<PageAtom>,
	/// Atom index of each stop, parallel to the dialog's stops.
	pub stop_atoms: Vec<usize>,
	/// Where the open dropdown hangs from: its field atom and value row inside it.
	pub dropdown: Option<DropdownAnchor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropdownAnchor {
	pub atom: usize,
	pub row: usize,
}

/// Blocks `start` up to `end` (exclusive) of one form section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabPart {
	pub section: usize,
	pub start: usize,
	pub end: usize,
}

/// A question tab: the section parts it shows, and the fields in them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormTab {
	pub label: String,
	pub parts: Vec<TabPart>,
	pub fields: Vec<usize>,
}

/// Interactive form dialog core state.
pub struct HtmlFormDialog<'a> {
	form: HtmlForm,
	options: HtmlFormDialogOptions<'a>,
	stops: Vec<Stop>,
	field_index: HashMap<usize, usize>,
	checked: HashMap<usize, HashSet<usize>>,
	text_inputs: HashMap<usize, String>,
	notes: HashMap<usize, HashMap<usize, String>>,
	note_editor: Option<NoteEditorState>,
	saved_note_text: String,
	others: HashMap<usize, String>,
	other_seen: HashMap<usize, String>,
	errors: HashMap<usize, String>,
	focused: bool,
	focus: usize,
	page: usize,
	follow_focus: bool,
	pages: Vec<Page>,
	stop_atoms: Vec<usize>,
	open: Option<OpenDropdown>,
	guard_released: bool,
	deadline: Option<Instant>,
	remaining: Option<u64>,
	version: u64,
	media: HashMap<usize, MediaSlot>,
	media_version: u64,
	questions: HashMap<usize, String>,
	tabs: Vec<FormTab>,
	field_tab: HashMap<usize, usize>,
	tab: usize,
	left_tabs: HashSet<usize>,
	hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenDropdown {
	field_index: usize,
	highlight: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NoteEditorState {
	field_index: usize,
	option: usize,
	text: String,
}

impl<'a> std::fmt::Debug for HtmlFormDialog<'a> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("HtmlFormDialog")
			.field("title", &self.title_text())
			.field("focus", &self.focus)
			.field("page", &self.page)
			.field("hidden", &self.hidden)
			.field("tab", &self.tab)
			.finish()
	}
}

impl<'a> HtmlFormDialog<'a> {
	pub fn new(form: HtmlForm, options: HtmlFormDialogOptions<'a>) -> Self {
		let mut field_index = HashMap::new();
		for (i, _) in form.fields.iter().enumerate() {
			field_index.insert(i, i);
		}
		let questions = field_questions(&form);
		let tabs = build_tabs(&form, &questions);
		let mut field_tab = HashMap::new();
		for (ti, tab) in tabs.iter().enumerate() {
			for &fi in &tab.fields {
				field_tab.insert(fi, ti);
			}
		}
		let mut dialog = Self {
			form,
			options,
			stops: Vec::new(),
			field_index,
			checked: HashMap::new(),
			text_inputs: HashMap::new(),
			notes: HashMap::new(),
			note_editor: None,
			saved_note_text: String::new(),
			others: HashMap::new(),
			other_seen: HashMap::new(),
			errors: HashMap::new(),
			focused: false,
			focus: 0,
			page: 0,
			follow_focus: false,
			pages: Vec::new(),
			stop_atoms: Vec::new(),
			open: None,
			guard_released: false,
			deadline: None,
			remaining: None,
			version: 0,
			media: HashMap::new(),
			media_version: 0,
			questions,
			tabs,
			field_tab,
			tab: 0,
			left_tabs: HashSet::new(),
			hidden: false,
		};
		dialog.init_state();
		dialog.start_countdown();
		dialog
	}

	fn init_state(&mut self) {
		let cwd = self.options.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
		let sections = self.form.sections.clone();
		let mut block_index = 0usize;
		for section in &sections {
			for block in &section.blocks {
				match block {
					FormBlock::Image { src, alt } => {
						self.media.insert(block_index, self.load_media_block(src, alt, &cwd));
						block_index += 1;
					}
					FormBlock::Svg { markup, alt } => {
						let media = svg_media(markup);
						self.media.insert(
							block_index,
							MediaSlot::Ready {
								mime: media.mime,
								alt: if alt.is_empty() { "drawing".to_string() } else { alt.clone() },
							},
						);
						block_index += 1;
					}
					FormBlock::Field { field, .. } => {
						let fi = self.form.fields.iter().position(|f| std::ptr::eq(f, field)).unwrap_or_else(|| {
							// fallback by matching content index order
							self.stops.len().min(self.form.fields.len().saturating_sub(1))
						});
						match field {
							FormField::Text(t) => {
								self.text_inputs.insert(fi, t.value.clone());
								self.stops.push(Stop::Text { field_index: fi });
							}
							FormField::Choice(c) => {
								self.init_choice(fi, c);
								if is_dropdown(c) {
									if c.options.iter().any(|o| !o.disabled) {
										self.stops.push(Stop::Select { field_index: fi });
									}
								} else {
									for (oi, opt) in c.options.iter().enumerate() {
										if !opt.disabled {
											self.stops.push(Stop::Option { field_index: fi, option: oi });
										}
									}
									if c.other {
										self.others.insert(fi, String::new());
										self.other_seen.insert(fi, String::new());
										self.stops.push(Stop::Other { field_index: fi });
									}
								}
							}
						}
						block_index += 1;
					}
					_ => {
						block_index += 1;
					}
				}
			}
		}
		// Fallback if pointer matching failed: build stops by field order
		if self.stops.is_empty() || self.stops.iter().all(|s| matches!(s, Stop::Submit)) {
			self.stops.clear();
			self.checked.clear();
			self.text_inputs.clear();
			self.others.clear();
			self.other_seen.clear();
			self.notes.clear();
			let fields = self.form.fields.clone();
			for (fi, field) in fields.iter().enumerate() {
				match field {
					FormField::Text(t) => {
						self.text_inputs.insert(fi, t.value.clone());
						self.stops.push(Stop::Text { field_index: fi });
					}
					FormField::Choice(c) => {
						self.init_choice(fi, c);
						if is_dropdown(c) {
							if c.options.iter().any(|o| !o.disabled) {
								self.stops.push(Stop::Select { field_index: fi });
							}
						} else {
							for (oi, opt) in c.options.iter().enumerate() {
								if !opt.disabled {
									self.stops.push(Stop::Option { field_index: fi, option: oi });
								}
							}
							if c.other {
								self.others.insert(fi, String::new());
								self.other_seen.insert(fi, String::new());
								self.stops.push(Stop::Other { field_index: fi });
							}
						}
					}
				}
			}
		}
		self.stops.push(Stop::Submit);
	}

	fn load_media_block(&self, src: &str, alt: &str, cwd: &PathBuf) -> MediaSlot {
		match load_form_image(src, &FormMediaOptions { cwd: cwd.clone() }) {
			Ok(media) => MediaSlot::Ready {
				mime: media.mime,
				alt: if alt.is_empty() {
					if src.len() > 80 { src[..80].to_string() } else { src.to_string() }
				} else {
					alt.to_string()
				},
			},
			Err(reason) => MediaSlot::Failed { reason },
		}
	}

	fn init_choice(&mut self, field_index: usize, field: &ChoiceField) {
		self.notes.entry(field_index).or_default();
		let mut checked = HashSet::new();
		for (i, opt) in field.options.iter().enumerate() {
			if !opt.checked {
				continue;
			}
			if !field.multi {
				checked.clear();
			}
			checked.insert(i);
		}
		self.checked.insert(field_index, checked);
	}

	pub fn focused(&self) -> bool {
		self.focused
	}

	pub fn set_focused(&mut self, value: bool) {
		self.focused = value;
		self.changed();
	}

	pub fn form(&self) -> &HtmlForm {
		&self.form
	}

	pub fn title(&self) -> &str {
		self.options.title.as_deref().unwrap_or(DEFAULT_TITLE)
	}

	pub fn title_text(&self) -> String {
		match self.remaining {
			Some(secs) => format!("{} ({secs}s)", self.title()),
			None => self.title().to_string(),
		}
	}

	pub fn hidden(&self) -> bool {
		self.hidden
	}

	pub fn set_hidden(&mut self, hidden: bool) {
		if self.hidden == hidden {
			return;
		}
		self.hidden = hidden;
		self.open = None;
		self.changed();
	}

	pub fn tabs(&self) -> &[FormTab] {
		&self.tabs
	}

	pub fn tab(&self) -> usize {
		self.tab
	}

	pub fn focus(&self) -> usize {
		self.focus
	}

	pub fn pages(&self) -> &[Page] {
		&self.pages
	}

	pub fn page(&self) -> usize {
		self.page
	}

	pub fn errors(&self) -> &HashMap<usize, String> {
		&self.errors
	}

	pub fn media(&self) -> &HashMap<usize, MediaSlot> {
		&self.media
	}

	/// Current answers, as submission would report them (without validating).
	pub fn data(&self) -> HtmlFormData {
		let mut data = default_form_data(&self.form);
		// Recompute from live state
		data.clear();
		for (name, value) in &self.form.hidden {
			data.insert(name.clone(), HtmlFormValue::Text(value.clone()));
		}
		for (fi, field) in self.form.fields.iter().enumerate() {
			let name = field.name();
			if name.is_empty() {
				continue;
			}
			if let Some(value) = self.value_of(fi, field) {
				data.insert(name.to_string(), value);
			}
		}
		data
	}

	fn value_of(&self, field_index: usize, field: &FormField) -> Option<HtmlFormValue> {
		match field {
			FormField::Text(t) => {
				let text = self.text_inputs.get(&field_index).cloned().unwrap_or_default();
				Some(text_value(t, &text))
			}
			FormField::Choice(c) => {
				let checked = self.checked.get(&field_index).cloned().unwrap_or_default();
				let other = if self.other_chosen(field_index, c) {
					self.others.get(&field_index).map(String::as_str)
				} else {
					None
				};
				choice_value(c, &checked, other)
			}
		}
	}

	fn other_chosen(&self, field_index: usize, field: &ChoiceField) -> bool {
		let Some(input) = self.others.get(&field_index) else { return false };
		if input.trim().is_empty() {
			return false;
		}
		if field.multi {
			return true;
		}
		self.checked.get(&field_index).map_or(true, |s| s.is_empty())
	}

	/// Saved notes on selected options; unselected drafts stay private.
	pub fn option_notes(&self) -> Vec<OptionNote> {
		let mut selected = Vec::new();
		for (fi, field) in self.form.fields.iter().enumerate() {
			let FormField::Choice(choice) = field else { continue };
			if choice.name.is_empty() {
				continue;
			}
			let Some(notes) = self.notes.get(&fi) else { continue };
			let checked = self.checked.get(&fi).cloned().unwrap_or_default();
			for (oi, option) in choice.options.iter().enumerate() {
				let Some(text) = notes.get(&oi) else { continue };
				let trimmed = text.trim();
				if trimmed.is_empty() || option.disabled || !checked.contains(&oi) {
					continue;
				}
				selected.push(OptionNote {
					name: choice.name.clone(),
					question: self.questions.get(&fi).cloned().unwrap_or_else(|| choice.name.clone()),
					option: option_text(option).if_empty_then(option.value.clone()),
					value: option.value.clone(),
					text: trimmed.to_string(),
				});
			}
		}
		selected
	}

	pub fn debug_state(&self) -> HashMap<String, String> {
		let mut out = HashMap::new();
		out.insert("page".into(), (self.page + 1).to_string());
		out.insert("pages".into(), self.pages.len().to_string());
		out.insert("focus".into(), format!("{:?}", self.stops.get(self.focus)));
		out.insert("open".into(), self.open.as_ref().map(|o| o.field_index.to_string()).unwrap_or_else(|| "null".into()));
		out.insert("remaining".into(), self.remaining.map(|r| r.to_string()).unwrap_or_else(|| "null".into()));
		out.insert("hidden".into(), self.hidden.to_string());
		out.insert("tab".into(), self.tab.to_string());
		out.insert("errors".into(), self.errors.len().to_string());
		out
	}

	fn changed(&mut self) {
		self.version = self.version.saturating_add(1);
	}

	fn start_countdown(&mut self) {
		let Some(timeout) = self.options.timeout else { return };
		if timeout.is_zero() {
			return;
		}
		self.deadline = Some(Instant::now() + timeout);
		self.remaining = Some(timeout.as_secs().max(1));
	}

	/// Host-driven tick for inactivity timeout.
	pub fn tick(&mut self) {
		let Some(deadline) = self.deadline else { return };
		let now = Instant::now();
		if now >= deadline {
			self.deadline = None;
			self.remaining = None;
			if self.options.on_timeout.is_some() {
				let data = self.data();
				let notes = self.option_notes();
				if let Some(cb) = self.options.on_timeout.as_mut() {
					cb(data, notes);
				}
			}
			return;
		}
		self.remaining = Some((deadline - now).as_secs().max(1));
	}

	pub fn reset_countdown(&mut self) {
		let Some(timeout) = self.options.timeout else { return };
		if self.deadline.is_some() {
			self.deadline = Some(Instant::now() + timeout);
			self.remaining = Some(timeout.as_secs().max(1));
		}
	}

	pub fn set_text(&mut self, field_index: usize, value: impl Into<String>) {
		self.text_inputs.insert(field_index, value.into());
		self.errors.remove(&field_index);
		self.reset_countdown();
		self.changed();
	}

	pub fn set_other_text(&mut self, field_index: usize, value: impl Into<String>) {
		let value = value.into();
		self.other_seen.insert(field_index, value.clone());
		if let Some(FormField::Choice(c)) = self.form.fields.get(field_index) {
			if !c.multi && !value.trim().is_empty() {
				if let Some(checked) = self.checked.get_mut(&field_index) {
					checked.clear();
				}
			}
		}
		self.others.insert(field_index, value);
		self.errors.remove(&field_index);
		self.reset_countdown();
		self.changed();
	}

	pub fn toggle(&mut self, field_index: usize, option: usize) {
		let Some(FormField::Choice(field)) = self.form.fields.get(field_index).cloned() else { return };
		if field.options.get(option).map_or(true, |o| o.disabled) {
			return;
		}
		if !field.multi {
			self.choose(field_index, option);
			return;
		}
		let checked = self.checked.entry(field_index).or_default();
		if !checked.remove(&option) {
			checked.insert(option);
		}
		self.errors.remove(&field_index);
		self.reset_countdown();
		self.changed();
	}

	pub fn choose(&mut self, field_index: usize, option: usize) {
		let Some(FormField::Choice(field)) = self.form.fields.get(field_index) else { return };
		if field.options.get(option).map_or(true, |o| o.disabled) {
			return;
		}
		let checked = self.checked.entry(field_index).or_default();
		checked.clear();
		checked.insert(option);
		self.errors.remove(&field_index);
		self.reset_countdown();
		self.changed();
	}

	pub fn set_focus(&mut self, index: usize) {
		if self.stops.is_empty() {
			return;
		}
		self.focus = index.min(self.stops.len() - 1);
		self.follow_focus = true;
		if !self.tabs.is_empty() {
			self.enter_tab(self.tab_of_stop(self.focus));
		}
		self.changed();
	}

	pub fn move_stop(&mut self, delta: isize) {
		let next = (self.focus as isize + delta).clamp(0, self.stops.len().saturating_sub(1) as isize) as usize;
		self.set_focus(next);
	}

	pub fn move_field(&mut self, delta: isize) {
		if self.stops.is_empty() { return; }
		let field_of = |stop: &Stop| -> Option<usize> {
			match *stop {
				Stop::Option { field_index, .. }
				| Stop::Other { field_index }
				| Stop::Select { field_index }
				| Stop::Text { field_index } => Some(field_index),
				Stop::Submit => None,
			}
		};
		let current = field_of(&self.stops[self.focus]);
		let mut index = self.focus as isize;
		while index + delta >= 0
			&& (index + delta as isize) < self.stops.len() as isize
			&& field_of(&self.stops[index as usize]) == current
		{
			index += delta;
		}
		let target = field_of(&self.stops[index.clamp(0, self.stops.len() as isize - 1) as usize]);
		if delta < 0 {
			while index > 0 && field_of(&self.stops[(index - 1) as usize]) == target {
				index -= 1;
			}
		}
		self.set_focus(index.clamp(0, self.stops.len() as isize - 1) as usize);
	}

	pub fn turn_page(&mut self, delta: isize) {
		if self.pages.len() < 2 {
			return;
		}
		let target = (self.page as isize + delta).clamp(0, self.pages.len() as isize - 1) as usize;
		let page = self.pages[target];
		if let Some(stop) = self.stop_atoms.iter().position(|&atom| atom >= page.start && atom < page.end) {
			self.set_focus(stop);
			return;
		}
		self.page = target;
		self.follow_focus = false;
		self.changed();
	}

	pub fn submit(&mut self) -> bool {
		self.errors.clear();
		for (fi, field) in self.form.fields.iter().enumerate() {
			let value = self.value_of(fi, field);
			if let Some(error) = field_error(field, value.as_ref()) {
				self.errors.insert(fi, error);
			}
		}
		if self.errors.is_empty() {
			self.deadline = None;
			self.remaining = None;
			let data = self.data();
			let notes = self.option_notes();
			(self.options.on_submit)(data, notes);
			return true;
		}
		if let Some(first) = self.stops.iter().position(|stop| match *stop {
			Stop::Option { field_index, .. }
			| Stop::Other { field_index }
			| Stop::Select { field_index }
			| Stop::Text { field_index } => self.errors.contains_key(&field_index),
			Stop::Submit => false,
		}) {
			self.set_focus(first);
		}
		self.changed();
		false
	}

	pub fn cancel(&mut self) {
		self.deadline = None;
		self.remaining = None;
		(self.options.on_cancel)();
	}

	fn tabbed(&self) -> bool {
		self.tabs.len() > 1
	}

	fn tab_of_stop(&self, index: usize) -> usize {
		match self.stops.get(index).copied().unwrap_or(Stop::Submit) {
			Stop::Submit => self.tabs.len(),
			Stop::Option { field_index, .. }
			| Stop::Other { field_index }
			| Stop::Select { field_index }
			| Stop::Text { field_index } => *self.field_tab.get(&field_index).unwrap_or(&0),
		}
	}

	pub fn show_tab(&mut self, index: usize) {
		let tab = index.min(self.tabs.len());
		if let Some(stop) = self.stops.iter().position(|_| self.tab_of_stop(self.focus) == tab) {
			self.set_focus(stop);
		} else if let Some(stop) = (0..self.stops.len()).find(|&i| self.tab_of_stop(i) == tab) {
			self.set_focus(stop);
		}
		self.enter_tab(tab);
	}

	fn enter_tab(&mut self, tab: usize) {
		if tab != self.tab {
			self.left_tabs.insert(self.tab);
		}
		self.tab = tab;
		self.changed();
	}

	/// Compute layout/pages for a given width using current state/errors/media.
	pub fn layout(&mut self, width: usize) -> Layout {
		let atoms = self.build_atoms(width.max(1));
		self.pages = paginate(&atoms, self.body_rows(width.max(1), &atoms));
		self.stop_atoms = self.compute_stop_atoms();
		let focus_page = self.page_of_atom(*self.stop_atoms.get(self.focus).unwrap_or(&0));
		if self.follow_focus {
			self.page = focus_page;
		}
		if !self.pages.is_empty() {
			self.page = self.page.min(self.pages.len() - 1);
		}
		if self.page == focus_page {
			self.follow_focus = true;
		}
		Layout {
			atoms,
			stop_atoms: self.stop_atoms.clone(),
			dropdown: self.open.as_ref().map(|_open| DropdownAnchor { atom: 0, row: 0 }),
		}
	}

	fn body_rows(&self, _width: usize, atoms: &[PageAtom]) -> usize {
		let term_rows = self.options.tui.map(|t| t.terminal_rows() as usize).unwrap_or(40);
		let cap = ((term_rows as f32 * DIALOG_HEIGHT_RATIO) as usize)
			.saturating_sub(CHROME_ROWS)
			.max(MIN_BODY_ROWS);
		let natural = page_height(atoms, 0, atoms.len()).max(1);
		natural.min(cap.max(1))
	}

	fn page_of_atom(&self, atom: usize) -> usize {
		self.pages
			.iter()
			.position(|page| atom >= page.start && atom < page.end)
			.unwrap_or(0)
	}

	fn compute_stop_atoms(&self) -> Vec<usize> {
		// Approximate: one atom per stop in order, ending at submit atom.
		let total = self.stops.len();
		(0..total).collect()
	}

	fn build_atoms(&self, width: usize) -> Vec<PageAtom> {
		let mut atoms = Vec::new();
		for (section_index, section) in self.form.sections.iter().enumerate() {
			let title = one_line(&spans_text(&section.title));
			let mut gap = if section_index == 0 { 0 } else { 1 };
			let mut rank = BreakRank::Section;
			if !title.is_empty() {
				atoms.push(PageAtom {
					lines: vec![title.clone()],
					gap,
					break_rank: rank,
					continuation: None,
				});
				gap = 0;
				rank = BreakRank::Never;
			}
			for block in &section.blocks {
				let mut block_atoms = match block {
					FormBlock::Field { field, .. } => self.field_atoms(field, width),
					_ => self.context_atoms(block, width),
				};
				if block_atoms.is_empty() {
					continue;
				}
				block_atoms[0].gap = gap;
				block_atoms[0].break_rank = rank;
				if block_atoms.len() > 1 {
					block_atoms[1].break_rank = BreakRank::Never;
					if let Some(last) = block_atoms.last_mut() {
						last.break_rank = BreakRank::Never;
					}
				}
				for atom in &mut block_atoms {
					if !title.is_empty() {
						atom.continuation = Some(title.clone());
					}
				}
				atoms.extend(block_atoms);
				gap = 1;
				rank = BreakRank::Block;
			}
		}
		let pending = if self.errors.is_empty() { String::new() } else { format!("  {} to fix", self.errors.len()) };
		atoms.push(PageAtom {
			lines: vec![format!("  {SUBMIT_LABEL}{pending}")],
			gap: 1,
			break_rank: BreakRank::Block,
			continuation: None,
		});
		atoms
	}

	fn field_atoms(&self, field: &FormField, width: usize) -> Vec<PageAtom> {
		let fi = self.form.fields.iter().position(|f| std::ptr::eq(f, field)).unwrap_or(0);
		let error = self.errors.get(&fi);
		let error_lines = error
			.map(|e| vec![format!("{}{}", " ".repeat(FIELD_INDENT), truncate_to_width(e, width.saturating_sub(FIELD_INDENT)))])
			.unwrap_or_default();
		match field {
			FormField::Text(t) => {
				let mut lines = Vec::new();
				let label = one_line(&spans_text(&t.label));
				if !label.is_empty() {
					lines.push(label);
				}
				let value = self.text_inputs.get(&fi).cloned().unwrap_or_default();
				let shown = if t.r#type == "password" { "•".repeat(value.chars().count()) } else if value.is_empty() { t.placeholder.clone() } else { value };
				lines.push(format!("{}{}", " ".repeat(FIELD_INDENT.saturating_sub(2)), truncate_to_width(&shown.replace('\n', " "), width.saturating_sub(FIELD_INDENT))));
				lines.extend(error_lines);
				vec![PageAtom { lines, gap: 0, break_rank: BreakRank::Inside, continuation: None }]
			}
			FormField::Choice(c) => {
				let mut atoms = Vec::new();
				let label = one_line(&spans_text(&c.label));
				if !label.is_empty() {
					atoms.push(PageAtom { lines: vec![label], gap: 0, break_rank: BreakRank::Inside, continuation: None });
				}
				let checked = self.checked.get(&fi).cloned().unwrap_or_default();
				for (oi, opt) in c.options.iter().enumerate() {
					let mark = if c.multi {
						if checked.contains(&oi) { "[x]" } else { "[ ]" }
					} else if checked.contains(&oi) { "(*)" } else { "( )" };
					let note = self.notes.get(&fi).and_then(|n| n.get(&oi)).filter(|n| !n.trim().is_empty()).map(|_| "  note").unwrap_or("");
					let line = format!("  {mark} {}{note}", truncate_to_width(&option_text(opt), width.saturating_sub(8)));
					atoms.push(PageAtom { lines: vec![line], gap: 0, break_rank: BreakRank::Inside, continuation: None });
				}
				if c.other {
					let text = self.others.get(&fi).cloned().unwrap_or_default();
					let shown = if text.trim().is_empty() { OTHER_PLACEHOLDER.to_string() } else { text };
					atoms.push(PageAtom { lines: vec![format!("  {OTHER_LABEL}: {shown}")], gap: 0, break_rank: BreakRank::Inside, continuation: None });
				}
				if !error_lines.is_empty() {
					atoms.push(PageAtom { lines: error_lines, gap: 0, break_rank: BreakRank::Never, continuation: None });
				}
				atoms
			}
		}
	}

	fn context_atoms(&self, block: &FormBlock, width: usize) -> Vec<PageAtom> {
		let atom = |lines: Vec<String>| PageAtom { lines, gap: 0, break_rank: BreakRank::Inside, continuation: None };
		match block {
			FormBlock::Text { spans, heading: _ } => {
				let text = spans_text(spans);
				split_lines_text(&text, width.max(1)).into_iter().map(|l| atom(vec![l])).collect()
			}
			FormBlock::List { items, ordered } => items
				.iter()
				.enumerate()
				.map(|(i, item)| {
					let prefix = if *ordered { format!("{}. ", i + 1) } else { "• ".to_string() };
					atom(vec![format!("{prefix}{}", truncate_to_width(&spans_text(item), width.saturating_sub(prefix.len())))])
				})
				.collect(),
			FormBlock::Pre { text } => sanitize_text(text)
				.lines()
				.map(|line| atom(vec![format!("  {}", truncate_to_width(line, width.saturating_sub(2)))]))
				.collect(),
			FormBlock::Table { rows, header_rows } => {
				let mut out = Vec::new();
				for (i, row) in rows.iter().enumerate() {
					let line = row.iter().map(|c| sanitize_text(&spans_text(c)).replace('\n', " ")).collect::<Vec<_>>().join(" | ");
					out.push(atom(vec![truncate_to_width(&line, width)]));
					if i + 1 == *header_rows {
						out.push(PageAtom { lines: vec!["-".repeat(width.max(1).min(40))], gap: 0, break_rank: BreakRank::Never, continuation: None });
					}
				}
				out
			}
			FormBlock::Image { src, alt } | FormBlock::Svg { markup: src, alt } => {
				let label = if alt.is_empty() { src.chars().take(40).collect::<String>() } else { alt.clone() };
				vec![atom(vec![format!("▣ {label}")])]
			}
			FormBlock::Field { .. } => Vec::new(),
		}
	}
}

fn is_dropdown(field: &ChoiceField) -> bool {
	field.select && !field.multi
}

fn option_text(option: &crate::html_form::parse::ChoiceOption) -> String {
	sanitize_text(&spans_text(&option.label)).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sanitize_text(text: &str) -> String {
	text.replace('\t', "    ")
		.chars()
		.filter(|&c| c == '\n' || (c >= ' ' && c != '\x7f'))
		.collect()
}

fn one_line(text: &str) -> String {
	sanitize_text(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_to_width(text: &str, width: usize) -> String {
	if width == 0 { return String::new(); }
	let mut out = String::new();
	let mut count = 0usize;
	for ch in text.chars() {
		let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
		if count + w > width { break; }
		out.push(ch);
		count += w;
	}
	out
}

fn split_lines_text(text: &str, width: usize) -> Vec<String> {
	let text = sanitize_text(text);
	if text.is_empty() { return Vec::new(); }
	let mut out = Vec::new();
	for para in text.split('\n') {
		if para.is_empty() { out.push(String::new()); continue; }
		let mut line = String::new();
		let mut cols = 0usize;
		for word in para.split_whitespace() {
			let w = unicode_width::UnicodeWidthStr::width(word);
			let need = if line.is_empty() { w } else { w + 1 };
			if !line.is_empty() && cols + need > width {
				out.push(std::mem::take(&mut line));
				cols = 0;
			}
			if !line.is_empty() { line.push(' '); cols += 1; }
			line.push_str(word);
			cols += w;
		}
		if !line.is_empty() { out.push(line); }
	}
	out
}

trait IfEmptyThen {
	fn if_empty_then(self, fallback: String) -> String;
}
impl IfEmptyThen for String {
	fn if_empty_then(self, fallback: String) -> String {
		if self.trim().is_empty() { fallback } else { self }
	}
}

fn reachable(field: &FormField) -> bool {
	match field {
		FormField::Text(_) => true,
		FormField::Choice(c) => c.other || c.options.iter().any(|o| !o.disabled),
	}
}

fn build_tabs(form: &HtmlForm, questions: &HashMap<usize, String>) -> Vec<FormTab> {
	let tabbed = form.sections.iter().filter(|s| s.blocks.iter().any(|b| matches!(b, FormBlock::Field { .. }))).count() > 1;
	let mut tabs = Vec::new();
	let mut lead: Vec<TabPart> = Vec::new();
	for (section_index, section) in form.sections.iter().enumerate() {
		let end = section.blocks.len();
		let has_field = section.blocks.iter().any(|b| matches!(b, FormBlock::Field { .. }));
		if !has_field {
			lead.push(TabPart { section: section_index, start: 0, end });
			continue;
		}
		let title = one_line(&spans_text(&section.title));
		let mut field_blocks: Vec<(usize, usize, usize)> = Vec::new(); // (at, through, field_index)
		for (at, block) in section.blocks.iter().enumerate() {
			if let FormBlock::Field { field, options_end } = block {
				if let Some(fi) = form.fields.iter().position(|f| std::ptr::eq(f, field)) {
					field_blocks.push((at, at + 1.max(options_end.unwrap_or(0).saturating_sub(at)), fi));
				}
			}
		}
		let mut cuts: Vec<usize> = Vec::new();
		if tabbed && !section.fieldset {
			for (nth, &(at, through, fi)) in field_blocks.iter().enumerate() {
				if !reachable(&form.fields[fi]) { continue; }
				let next_at = field_blocks.get(nth + 1).map(|x| x.0).unwrap_or(end);
				cuts.push(if through <= next_at { through } else { at + 1 });
			}
		}
		if cuts.is_empty() { cuts.push(end); } else if let Some(last) = cuts.last_mut() { *last = end; }
		let mut start = 0usize;
		for cut in cuts {
			let fields: Vec<usize> = section.blocks[start..cut].iter().filter_map(|b| match b {
				FormBlock::Field { field, .. } => form.fields.iter().position(|f| std::ptr::eq(f, field)),
				_ => None,
			}).collect();
			let question_field = fields.iter().copied().find(|&fi| reachable(&form.fields[fi])).or_else(|| fields.first().copied());
			let question = question_field.and_then(|fi| questions.get(&fi)).cloned().unwrap_or_default();
			let label = if !title.is_empty() { title.clone() } else if !question.is_empty() { question } else { format!("Question {}", tabs.len() + 1) };
			let mut parts = lead.clone();
			parts.push(TabPart { section: section_index, start, end: cut });
			tabs.push(FormTab { label: truncate_to_width(&label, TAB_LABEL_WIDTH), parts, fields });
			lead.clear();
			start = cut;
		}
	}
	if !lead.is_empty() {
		if let Some(last) = tabs.last_mut() {
			last.parts.extend(lead);
		} else {
			tabs.push(FormTab { label: String::new(), parts: lead, fields: Vec::new() });
		}
	}
	tabs
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::html_form::parse::parse_html_form;

	struct Host(u16);
	impl HtmlFormHost for Host {
		fn terminal_rows(&self) -> u16 { self.0 }
		fn request_render(&self) {}
	}

	fn opts<'a>(
		host: &'a Host,
		on_submit: impl FnMut(HtmlFormData, Vec<OptionNote>) + 'a,
		on_cancel: impl FnMut() + 'a,
	) -> HtmlFormDialogOptions<'a> {
		HtmlFormDialogOptions {
			title: Some("T".into()),
			tui: Some(host),
			timeout: None,
			on_timeout: None,
			input_guard: None,
			cwd: None,
			on_submit: Box::new(on_submit),
			on_cancel: Box::new(on_cancel),
		}
	}

	#[test]
	fn builds_stops_data_and_tabs() {
		let form = parse_html_form(
			"<form><fieldset><legend>DB</legend>\
			 <label><input type=radio name=db value=sqlite checked> SQLite</label>\
			 <label><input type=radio name=db value=pg> Postgres</label>\
			 </fieldset><label>Name <input name=name value=Bob></label></form>",
		);
		let host = Host(40);
		let mut submitted = false;
		let mut cancelled = false;
		let dialog = HtmlFormDialog::new(form, opts(&host, |_, _| submitted = true, || cancelled = true));
		assert!(!submitted && !cancelled);
		assert!(dialog.stops.iter().any(|s| matches!(s, Stop::Option { .. } | Stop::Text { .. })));
		assert!(matches!(dialog.stops.last(), Some(Stop::Submit)));
		let data = dialog.data();
		assert_eq!(data.get("db"), Some(&HtmlFormValue::Text("sqlite".into())));
		assert_eq!(data.get("name"), Some(&HtmlFormValue::Text("Bob".into())));
		assert!(!dialog.tabs().is_empty());
	}

	#[test]
	fn choose_toggle_other_and_notes() {
		let form = parse_html_form(
			"<form>\
			 <label><input type=checkbox name=x value=a> A</label>\
			 <label><input type=checkbox name=x value=b> B</label>\
			 </form>",
		);
		let host = Host(40);
		let mut dialog = HtmlFormDialog::new(form, opts(&host, |_, _| {}, || {}));
		// field 0 is checkbox group with other enabled by parser
		dialog.toggle(0, 0);
		dialog.toggle(0, 1);
		let data = dialog.data();
		match data.get("x") {
			Some(HtmlFormValue::Array(v)) => {
				assert!(v.contains(&"a".to_string()));
				assert!(v.contains(&"b".to_string()));
			}
			other => panic!("expected array, got {other:?}"),
		}
		dialog.set_other_text(0, "custom");
		dialog.notes.entry(0).or_default().insert(0, "note".into());
		let notes = dialog.option_notes();
		assert!(notes.iter().any(|n| n.name == "x" && n.value == "a" && n.text == "note"));
	}

	#[test]
	fn submit_validates_and_reports_errors() {
		let form = parse_html_form("<form><label>Name <input name=name required></label></form>");
		let host = Host(40);
		let mut got: Option<HtmlFormData> = None;
		let mut dialog = HtmlFormDialog::new(form, HtmlFormDialogOptions {
			title: None,
			tui: Some(&host),
			timeout: None,
			on_timeout: None,
			input_guard: None,
			cwd: None,
			on_submit: Box::new(|d, _| got = Some(d)),
			on_cancel: Box::new(|| {}),
		});
		assert!(!dialog.submit());
		assert!(!dialog.errors().is_empty());
		dialog.set_text(0, "Zed");
		assert!(dialog.submit());
		assert_eq!(got.as_ref().and_then(|d| d.get("name")), Some(&HtmlFormValue::Text("Zed".into())));
	}

	#[test]
	fn layout_paginates_with_existing_paginate_module() {
		let form = parse_html_form("<form><p>a</p><p>b</p><p>c</p><input name=x></form>");
		let host = Host(10);
		let mut dialog = HtmlFormDialog::new(form, opts(&host, |_, _| {}, || {}));
		let layout = dialog.layout(20);
		assert!(!layout.atoms.is_empty());
		assert!(!dialog.pages().is_empty());
		assert_eq!(page_height(&layout.atoms, 0, layout.atoms.len()), page_height(&layout.atoms, 0, layout.atoms.len()));
	}

	#[test]
	fn hidden_and_tab_navigation_state() {
		let form = parse_html_form(
			"<form><fieldset><legend>A</legend><input name=a></fieldset><fieldset><legend>B</legend><input name=b></fieldset></form>",
		);
		let host = Host(40);
		let mut dialog = HtmlFormDialog::new(form, opts(&host, |_, _| {}, || {}));
		assert!(dialog.tabbed());
		dialog.show_tab(1);
		assert_eq!(dialog.tab(), 1);
		dialog.set_hidden(true);
		assert!(dialog.hidden());
		dialog.set_hidden(false);
		assert!(!dialog.hidden());
	}
}
