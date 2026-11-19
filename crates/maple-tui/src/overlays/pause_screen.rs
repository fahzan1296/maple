//! Reference: `l/src/overlays/pause-screen.ts`.
//!
//! Fullscreen `/pause` screen: a live hold clock, what the hold means, and the
//! work status that has to settle before the process really suspends.
//!
//! Deviations:
//! - `agentPauseGate` lives in `pi-agent-core`; the crate has no gate port, so
//!   [`PauseScreenComponent`] only reports the resume request (the host owns
//!   gate engage/release) and `runPauseScreen` has no counterpart here.
//! - The host is injected as a trait ([`PauseScreenHost`]) instead of a slice of
//!   `InteractiveModeContext`, and the refresh tick is host-driven
//!   ([`PauseScreenComponent::tick`]) rather than a `setInterval`.
//! - The `run()` promise becomes [`PauseScreenComponent::is_resumed`].
//! - The process-global theme and keybindings are injected.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::chrome::format::format_coarse_duration;
use crate::chrome::keybinding_hints::{bound_keys, interrupt_key};
use crate::component::Component;
use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{col, node, row, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{
	ActionButtonOptions, ActionButtonTone, action_bar, action_button, format_tooltip_key,
};
use crate::text_shape::visible_width;
use crate::theme::Theme;

/// Outstanding turn and job counts supplied by the pause-screen host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseWorkStatus {
	/// Turns still running.
	pub unsettled_turns: u64,
	/// Jobs still running.
	pub unfinished_jobs: u64,
}

/// A settled pause screen: no outstanding work.
pub const EMPTY_WORK_STATUS: PauseWorkStatus = PauseWorkStatus { unsettled_turns: 0, unfinished_jobs: 0 };

/// Refresh cadence for the live hold clock and work-status snapshot, in
/// milliseconds.
pub const TICK_MS: u64 = 1_000;
/// Rows of full blocks per pause bar.
pub const BAR_ROWS: usize = 7;
/// Columns of full blocks per pause bar.
pub const BAR_WIDTH: usize = 5;
/// Blank columns between the two pause bars.
pub const BAR_GAP: usize = 4;
/// Below this width the full scene cannot breathe; the compact card renders.
pub const MIN_FULL_WIDTH: usize = 64;
/// Below this height the full scene cannot breathe; the compact card renders.
pub const MIN_FULL_HEIGHT: usize = 23;
/// The full scene's title.
pub const TITLE: &str = "P A U S E D";
/// What the hold means, in the full scene.
pub const BODY_LINES: [&str; 2] = [
	"Main agent, subagents, and advisor hold at their next step.",
	"In-flight calls finish; nothing new starts until you resume.",
];
/// What the hold means, one fact per native row: icon, what, detail.
pub const HOLD_FACTS: [(&str, &str, &str); 4] = [
	("users", "Every agent holds", "Main agent, subagents and advisor stop at their next step."),
	("activity", "Running calls finish", "A reply or tool already under way completes; nothing new starts."),
	("message", "Queued messages wait", "Steering and follow-ups already queued send after you resume."),
	("pen", "The repo is yours", "Edit files freely, then tell the agent what changed."),
];
/// Role of the native sheet.
pub const PAUSE_SCREEN_ROLE: &str = "omp.overlay.pause";

/// Optional live work source for the pause overlay.
#[derive(Default)]
pub struct PauseScreenOptions {
	/// The current work status; absent means nothing is outstanding.
	pub get_work_status: Option<Box<dyn Fn() -> PauseWorkStatus>>,
}

/// What the pause screen needs from its host.
pub trait PauseScreenHost {
	/// Terminal height in rows.
	fn terminal_rows(&self) -> usize;
	/// The session's display name, when it has one.
	fn session_name(&self) -> Option<String>;
	/// Repaint the screen.
	fn request_render(&mut self);
	/// Report a status line (`dim` mutes it).
	fn show_status(&mut self, message: &str, dim: bool);
}

/// Live hold clock, seconds-precise: `0:07`, `12:34`, `1:02:03`.
pub fn format_clock(ms: u64) -> String {
	let total_seconds = ms / 1000;
	let seconds = total_seconds % 60;
	let minutes = (total_seconds / 60) % 60;
	let hours = total_seconds / 3600;
	if hours > 0 {
		format!("{hours}:{minutes:02}:{seconds:02}")
	} else {
		format!("{minutes}:{seconds:02}")
	}
}

/// Centre `line` in `width` columns.
fn center_line(line: &str, width: usize) -> String {
	let line_width = visible_width(line);
	if line_width >= width {
		return line.to_string();
	}
	let left = (width - line_width) / 2;
	format!("{}{line}", " ".repeat(left))
}

/// Drop trailing padding so rows stay comparable.
fn trimmed(line: String) -> String {
	line.trim_end().to_string()
}

/// Paint the pause scene as exactly `height` rows, vertically centered.
pub fn render_pause_screen(
	width: usize,
	height: usize,
	elapsed_ms: u64,
	session_name: Option<&str>,
	work_status: PauseWorkStatus,
	theme: &Theme,
	bindings: &KeybindingsManager,
) -> Vec<String> {
	let screen_width = width.max(1);
	let screen_height = height.max(1);
	let compact = screen_width < MIN_FULL_WIDTH || screen_height < MIN_FULL_HEIGHT;
	let outstanding = work_status.unsettled_turns > 0 || work_status.unfinished_jobs > 0;
	let status = if outstanding { "Pausing — waiting for work to settle" } else { "Paused — ready to suspend" };
	let work_counts = [
		format!("Unsettled turns: {}", work_status.unsettled_turns),
		format!("Unfinished jobs: {}", work_status.unfinished_jobs),
	];
	let mut content: Vec<String> = Vec::new();
	// Resume keys mirror the component's input handling: `app.interrupt`,
	// Enter and Space.
	let esc = interrupt_key(bindings, theme);
	let fg = |tone: &str, value: &str| theme.fg(tone, value).unwrap_or_else(|_| value.to_string());
	let collapsed_name = session_name.map(|name| name.split_whitespace().collect::<Vec<_>>().join(" "));

	if compact {
		let session_line = collapsed_name
			.as_ref()
			.map(|name| trimmed(center_line(&fg("text", name), screen_width)));
		let title_line = trimmed(center_line(&fg("accent", &format!("▌▌ {TITLE}")), screen_width));
		let clock_line = trimmed(center_line(&fg("dim", &format!("paused for {}", format_clock(elapsed_ms))), screen_width));
		let status_line =
			trimmed(center_line(&fg(if outstanding { "warning" } else { "success" }, status), screen_width));
		let resume_line = trimmed(center_line(&fg("dim", &format!("{esc} to resume")), screen_width));
		let mut full_content: Vec<String> = Vec::new();
		if let Some(session_line) = session_line.clone() {
			full_content.push(session_line);
		}
		full_content.push(title_line.clone());
		full_content.push(String::new());
		full_content.push(clock_line.clone());
		full_content.push(status_line.clone());
		if outstanding {
			full_content.extend(
				work_counts
					.iter()
					.map(|line| trimmed(center_line(&fg("muted", line), screen_width))),
			);
		}
		full_content.push(resume_line.clone());

		if screen_height >= full_content.len() {
			content.extend(full_content);
		} else {
			// Keep the live state and a way out on screen before the
			// decorative card details.
			if screen_height == 1 {
				let counts = format!("T:{} J:{}", work_status.unsettled_turns, work_status.unfinished_jobs);
				let summary = if outstanding {
					format!("{counts} · {esc} to resume")
				} else {
					format!("Paused · {esc} to resume")
				};
				let abbreviated = if outstanding {
					format!("T{} J{} {esc} resume", work_status.unsettled_turns, work_status.unfinished_jobs)
				} else {
					format!("{esc} resume")
				};
				let visible = if visible_width(&summary) > screen_width { abbreviated } else { summary };
				content.push(trimmed(center_line(
					&fg(if outstanding { "warning" } else { "success" }, &visible),
					screen_width,
				)));
			} else if screen_height == 2 && outstanding {
				let counts = format!(
					"Pausing · T:{} J:{}",
					work_status.unsettled_turns, work_status.unfinished_jobs
				);
				content.push(trimmed(center_line(&fg("warning", &counts), screen_width)));
				content.push(resume_line.clone());
			} else {
				content.push(status_line.clone());
				if outstanding && screen_height == 3 {
					content.push(trimmed(center_line(
						&fg(
							"muted",
							&format!(
								"Turns: {} · Jobs: {}",
								work_status.unsettled_turns, work_status.unfinished_jobs
							),
						),
						screen_width,
					)));
				} else if outstanding {
					content.extend(
						work_counts
							.iter()
							.map(|line| trimmed(center_line(&fg("muted", line), screen_width))),
					);
				}
				content.push(resume_line.clone());
			}

			if content.len() < screen_height {
				if let Some(session_line) = session_line {
					content.push(session_line);
				}
			}
			if content.len() < screen_height {
				content.push(title_line);
			}
			if content.len() < screen_height {
				content.push(clock_line);
			}
		}
	} else {
		if let Some(name) = collapsed_name.as_ref() {
			content.push(trimmed(center_line(&fg("text", name), screen_width)));
			content.push(String::new());
		}
		let bar = "█".repeat(BAR_WIDTH);
		let glyph_row = format!("{bar}{}{bar}", " ".repeat(BAR_GAP));
		for _ in 0..BAR_ROWS {
			content.push(trimmed(center_line(&fg("accent", &glyph_row), screen_width)));
		}
		content.push(String::new());
		content.push(trimmed(center_line(&fg("accent", TITLE), screen_width)));
		content.push(String::new());
		for line in BODY_LINES {
			content.push(trimmed(center_line(&fg("muted", line), screen_width)));
		}
		content.push(trimmed(center_line(
			&fg(if outstanding { "warning" } else { "success" }, status),
			screen_width,
		)));
		if outstanding {
			for line in &work_counts {
				content.push(trimmed(center_line(&fg("muted", line), screen_width)));
			}
		}
		content.push(trimmed(center_line(&fg("dim", &format!("paused for {}", format_clock(elapsed_ms))), screen_width)));
		content.push(String::new());
		let formatter = KeyHintFormatter::new(theme);
		let resume_hint = format!(
			"{esc} · {} · {} — resume",
			formatter.format_key_hint("enter"),
			formatter.format_key_hint("space")
		);
		content.push(trimmed(center_line(&fg("dim", &resume_hint), screen_width)));
	}

	let top_pad = screen_height.saturating_sub(content.len()) / 2;
	let mut lines: Vec<String> = vec![String::new(); top_pad];
	lines.extend(content);
	lines.resize(screen_height, String::new());
	lines.truncate(screen_height);
	lines
}

/// Fullscreen pause overlay; the host releases the gate once it is resumed.
pub struct PauseScreenComponent {
	host: Box<dyn PauseScreenHost>,
	options: PauseScreenOptions,
	started_at: Instant,
	work_snapshot: PauseWorkStatus,
	resumed: Rc<RefCell<bool>>,
	on_resume: Option<Box<dyn FnMut()>>,
	theme: Theme,
	bindings: KeybindingsManager,
}

impl PauseScreenComponent {
	/// A pause screen over `host`, starting its clock at `started_at`.
	pub fn new(
		theme: Theme,
		bindings: KeybindingsManager,
		host: Box<dyn PauseScreenHost>,
		options: PauseScreenOptions,
		started_at: Instant,
		on_resume: impl FnMut() + 'static,
	) -> Self {
		let work_snapshot = options.get_work_status.as_ref().map(|get| get()).unwrap_or(EMPTY_WORK_STATUS);
		PauseScreenComponent {
			host,
			options,
			started_at,
			work_snapshot,
			resumed: Rc::new(RefCell::new(false)),
			on_resume: Some(Box::new(on_resume)),
			theme,
			bindings,
		}
	}

	/// Start (or restart) the clock and repaint.
	pub fn run(&mut self) {
		self.started_at = Instant::now();
		self.refresh_work_snapshot();
		self.host.request_render();
	}

	/// The hold time so far, in milliseconds.
	pub fn elapsed_ms(&self) -> u64 {
		u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
	}

	/// The work status the sheet shows.
	pub fn work_snapshot(&self) -> PauseWorkStatus {
		self.work_snapshot
	}

	/// Whether the user asked to resume.
	pub fn is_resumed(&self) -> bool {
		*self.resumed.borrow()
	}

	/// The host's session name, for the hero line.
	pub fn session_name(&self) -> Option<String> {
		self.host.session_name()
	}

	/// Refresh the work snapshot; returns whether it changed.
	pub fn refresh_work_snapshot(&mut self) -> bool {
		let next = self.options.get_work_status.as_ref().map(|get| get()).unwrap_or(EMPTY_WORK_STATUS);
		if next == self.work_snapshot {
			return false;
		}
		self.work_snapshot = next;
		true
	}

	/// The host's refresh tick: re-read the clock and the work status.
	pub fn tick(&mut self) {
		let changed = self.refresh_work_snapshot();
		if changed {
			self.host.request_render();
		}
	}

	/// Request a resume; the host releases the gate when it sees
	/// [`Self::is_resumed`].
	pub fn resume(&mut self) {
		if *self.resumed.borrow() {
			return;
		}
		*self.resumed.borrow_mut() = true;
		if let Some(mut on_resume) = self.on_resume.take() {
			on_resume();
			self.on_resume = Some(on_resume);
		}
	}

	/// The host's teardown hook: report how long the hold lasted.
	pub fn dispose(&mut self, held_ms: Option<u64>) {
		if let Some(held_ms) = held_ms {
			let message = format!(
				"Resumed after {} — agents are running again.",
				format_coarse_duration(held_ms)
			);
			self.host.show_status(&message, false);
		}
	}

	/// The settlement status line text and tone.
	fn settlement(&self) -> (String, &'static str) {
		if self.outstanding() {
			("Pausing — waiting for work to settle".to_string(), "warning")
		} else {
			("Paused — ready to suspend".to_string(), "success")
		}
	}

	/// Whether turns or jobs are still running.
	pub fn outstanding(&self) -> bool {
		self.work_snapshot.unsettled_turns > 0 || self.work_snapshot.unfinished_jobs > 0
	}

	/// The native sheet: hero, settlement status, hold facts and Resume.
	pub fn describe(&self, _cx: &DescribeContext) -> NativeNode {
		let snapshot = self.work_snapshot;
		let (status, tone) = self.settlement();
		let mut settlement_children = vec![text(
			NodeText::Spans(vec![span(status, Some(tone))]),
			props_with_role("omp.pause.settlement"),
		)];
		if self.outstanding() {
			settlement_children.push(text(
				NodeText::Spans(vec![span(format!("Unsettled turns: {}", snapshot.unsettled_turns), Some("muted"))]),
				props_with_role("omp.pause.unsettled-turn-count"),
			));
			settlement_children.push(text(
				NodeText::Spans(vec![span(format!("Unfinished jobs: {}", snapshot.unfinished_jobs), Some("muted"))]),
				props_with_role("omp.pause.unfinished-job-count"),
			));
		}
		let mut settlement_props = props_with_role("omp.pause.settlement-status");
		settlement_props.set_str("gap", "sm");
		let status_node = col(settlement_children, settlement_props);

		let mut icon_props = Props::new();
		icon_props.set_str("name", "pause");
		icon_props.set_role("omp.pause.glyph");
		let glyph = node(NodeKind::Icon, icon_props, Vec::new(), Some("glyph"));
		let mut clock_props = Props::new();
		clock_props.set_str("format", "clock");
		clock_props.set_role("omp.pause.clock");
		clock_props.set_str("title", "Paused for");
		clock_props.set_f64("age", self.elapsed_ms() as f64);
		let clock = node(NodeKind::Elapsed, clock_props, Vec::new(), Some("clock"));
		let session_name = self.host.session_name().unwrap_or_else(|| "Every agent in this session".to_string());
		let mut session_props = Props::new();
		session_props.set_role("omp.pause.session");
		session_props.set_str("wrap", "none");
		session_props.set_str("truncate", "end");
		let session = text(
			NodeText::Spans(vec![span(session_name, Some("muted"))]),
			session_props,
		);
		let mut hero_inner = Props::new();
		hero_inner.set_str("gap", "none");
		hero_inner.set_str("grow", "1");
		hero_inner.set_str("shrink", "1");
		let hero_children = vec![glyph, col(vec![clock, session], hero_inner)];
		let mut hero_props = Props::new();
		hero_props.set_role("omp.pause.hero");
		hero_props.set_str("gap", "md");
		hero_props.set_str("align", "center");
		let hero = row(hero_children, hero_props);

		let facts = HOLD_FACTS
			.iter()
			.map(|(icon, what, detail)| {
				let mut fact_icon = Props::new();
				fact_icon.set_str("name", (*icon).to_string());
				let icon = node(NodeKind::Icon, fact_icon, Vec::new(), None);
				let mut fact_text = Props::new();
				fact_text.set_str("gap", "none");
				fact_text.set_str("grow", "1");
				fact_text.set_str("shrink", "1");
				let mut detail_props = Props::new();
				detail_props.set_str("wrap", "word");
				let body = col(
					vec![
						text(NodeText::Spans(vec![span(*what, Some("strong"))]), Props::new()),
						text(NodeText::Spans(vec![span(*detail, Some("muted"))]), detail_props),
					],
					fact_text,
				);
				let mut fact_props = Props::new();
				fact_props.set_role("omp.pause.fact");
				fact_props.set_str("gap", "md");
				fact_props.set_str("align", "start");
				row(vec![icon, body], fact_props)
			})
			.collect();
		let mut facts_props = Props::new();
		facts_props.set_role("omp.pause.facts");
		facts_props.set_str("gap", "md");
		let facts = col(facts, facts_props);

		let resume_key = bound_keys(&self.bindings, "app.interrupt", &["escape".to_string()])
			.into_iter()
			.next()
			.unwrap_or_else(|| "escape".to_string());
		let formatter = KeyHintFormatter::new(&self.theme);
		let title = format!(
			"Resume  {} ·   {} ·   {}",
			format_tooltip_key(&resume_key),
			formatter.format_tooltip_key("enter"),
			formatter.format_tooltip_key("space")
		);
		let resume = action_button(
			"Resume",
			"resume",
			ActionButtonOptions {
				keys: Some(resume_key),
				tone: Some(ActionButtonTone::Accent),
				title: Some(title),
				..Default::default()
			},
		);
		let actions = action_bar(&[None, Some(resume)], PAUSE_SCREEN_ROLE);

		let mut props = Props::new();
		props.set_role(PAUSE_SCREEN_ROLE);
		props.set_str("gap", "md");
		props.set_head(NodeText::Plain("Paused".to_string()));
		col(vec![hero, status_node, facts, actions], props)
	}

	/// Resume runs what Esc/Enter/Space run.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		if let NativeUiEvent::Action { act, .. } = event {
			if act == "resume" {
				self.resume();
			}
		}
	}
}

fn props_with_role(role: &str) -> Props {
	let mut props = Props::new();
	props.set_role(role);
	props
}

impl Component for PauseScreenComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let rows = self.host.terminal_rows().max(1);
		let session = self.host.session_name();
		render_pause_screen(
			usize::from(width).max(1),
			rows,
			self.elapsed_ms(),
			session.as_deref(),
			self.work_snapshot,
			&self.theme,
			&self.bindings,
		)
	}

	fn handle_input(&mut self, key: &Key) {
		// Every dismissal path resumes — including ctrl+c, which must never
		// double as "abort agents" while the whole point of the screen is that
		// nothing gets lost.
		if matches!(key, Key::Esc | Key::Enter | Key::Ctrl('c')) {
			self.resume();
			return;
		}
		if let Key::Char(' ') = key {
			self.resume();
		}
	}

	fn invalidate(&mut self) {}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::{CreateThemeOptions, create_theme};

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin dark theme"),
			CreateThemeOptions::default(),
		)
		.expect("valid theme")
	}

	struct FakeHost {
		rows: usize,
		name: Option<String>,
		renders: usize,
		status: Rc<RefCell<Option<String>>>,
	}

	impl PauseScreenHost for FakeHost {
		fn terminal_rows(&self) -> usize {
			self.rows
		}

		fn session_name(&self) -> Option<String> {
			self.name.clone()
		}

		fn request_render(&mut self) {
			self.renders += 1;
		}

		fn show_status(&mut self, message: &str, _dim: bool) {
			*self.status.borrow_mut() = Some(message.to_string());
		}
	}

	#[test]
	fn clock_is_seconds_precise_and_grows_to_hours() {
		assert_eq!(format_clock(0), "0:00");
		assert_eq!(format_clock(7_400), "0:07");
		assert_eq!(format_clock(754_000), "12:34");
		assert_eq!(format_clock(3_723_000), "1:02:03");
	}

	#[test]
	fn full_scene_pads_to_the_screen_height_and_centres() {
		let lines = render_pause_screen(
			100,
			30,
			7_000,
			Some("maple work"),
			EMPTY_WORK_STATUS,
			&theme(),
			&KeybindingsManager::new(),
		);
		assert_eq!(lines.len(), 30);
		assert!(lines.iter().any(|line| line.contains("P A U S E D")));
		assert!(lines.iter().any(|line| line.contains("paused for 0:07")));
		assert!(lines.iter().any(|line| line.contains("Paused — ready to suspend")));
		assert!(lines.iter().any(|line| line.contains("resume")));
		assert!(lines.iter().any(|line| line.contains("maple work")));
	}

	#[test]
	fn outstanding_work_adds_counts_to_the_full_scene() {
		let lines = render_pause_screen(
			100,
			30,
			0,
			None,
			PauseWorkStatus { unsettled_turns: 2, unfinished_jobs: 3 },
			&theme(),
			&KeybindingsManager::new(),
		);
		assert!(lines.iter().any(|line| line.contains("Pausing — waiting for work to settle")));
		assert!(lines.iter().any(|line| line.contains("Unsettled turns: 2")));
		assert!(lines.iter().any(|line| line.contains("Unfinished jobs: 3")));
	}

	#[test]
	fn narrow_screens_fall_back_to_the_compact_card() {
		let lines = render_pause_screen(40, 10, 61_000, None, EMPTY_WORK_STATUS, &theme(), &KeybindingsManager::new());
		assert_eq!(lines.len(), 10);
		assert!(lines.iter().any(|line| line.contains("paused for 1:01")));
		assert!(!lines.iter().any(|line| line.contains("█")));
	}

	#[test]
	fn a_one_row_screen_keeps_the_way_out() {
		let lines = render_pause_screen(30, 1, 0, None, EMPTY_WORK_STATUS, &theme(), &KeybindingsManager::new());
		assert_eq!(lines.len(), 1);
		assert!(lines[0].contains("resume"));
	}

	#[test]
	fn component_resume_keys_all_resume() {
		for key in [Key::Esc, Key::Enter, Key::Ctrl('c'), Key::Char(' ')] {
			let host = FakeHost { rows: 30, name: None, renders: 0, status: Rc::default() };
			let resumed = Rc::new(RefCell::new(0));
			let sink = Rc::clone(&resumed);
			let mut component = PauseScreenComponent::new(
				theme(),
				KeybindingsManager::new(),
				Box::new(host),
				PauseScreenOptions::default(),
				Instant::now(),
				move || *sink.borrow_mut() += 1,
			);
			component.handle_input(&key);
			assert!(component.is_resumed());
			assert_eq!(*resumed.borrow(), 1);
			// A second resume request is a no-op.
			component.handle_input(&key);
			assert_eq!(*resumed.borrow(), 1);
		}
	}

	#[test]
	fn tick_reports_a_changed_work_snapshot() {
		let snapshot = Rc::new(RefCell::new(EMPTY_WORK_STATUS));
		let source = Rc::clone(&snapshot);
		let host = FakeHost { rows: 30, name: None, renders: 0, status: Rc::default() };
		let mut component = PauseScreenComponent::new(
			theme(),
			KeybindingsManager::new(),
			Box::new(host),
			PauseScreenOptions {
				get_work_status: Some(Box::new(move || *source.borrow())),
			},
			Instant::now(),
			|| {},
		);
		assert!(!component.tick());
		*snapshot.borrow_mut() = PauseWorkStatus { unsettled_turns: 1, unfinished_jobs: 0 };
		assert!(component.tick());
		assert_eq!(component.work_snapshot().unsettled_turns, 1);
		assert!(component.outstanding());
	}

	#[test]
	fn dispose_reports_the_hold_duration() {
		let status = Rc::new(RefCell::new(None));
		let host = FakeHost { rows: 30, name: None, renders: 0, status: Rc::clone(&status) };
		let mut component = PauseScreenComponent::new(
			theme(),
			KeybindingsManager::new(),
			Box::new(host),
			PauseScreenOptions::default(),
			Instant::now(),
			|| {},
		);
		component.dispose(Some(125_000));
		let message = status.borrow().clone().expect("dispose reports the hold");
		assert!(message.starts_with("Resumed after "));
		assert!(message.ends_with("— agents are running again."));
	}
}
