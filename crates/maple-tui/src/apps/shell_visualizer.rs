//! Shell command visualizer/inspector.
//!
//! Reference: `l/src/apps/shell-visualizer.ts`.
//!
//! Deviations:
//! - TS depends on `@oh-my-pi/pi-wire` (`ShellVizPresentation`), crate TUI
//!   `Component`/native describe/mouse APIs, theme, and shell-viz rendering
//!   helpers. The crate has `tools::shell_viz` types/render helpers but a
//!   different component/native model.
//! - [`ShellVisualizer`] is implemented as a stateful controller owning
//!   scroll/detail state and translating input strings into host actions and
//!   render lines via injected callbacks. Presentation types are abstracted
//!   behind [`ShellPresentationView`] so behavior is portable and testable.
//! - `matchesKey` string semantics are approximated by direct string matches
//!   for the keys used by the reference.

/// Minimal timeline view used by the inspector.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimelineView {
	pub cursor: usize,
	pub length: usize,
	pub elapsed_ns: u64,
	pub duration_ns: u64,
}

/// Minimal layer flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerPrefs {
	pub color: bool,
	pub inline: bool,
	pub idioms: bool,
	pub effects: bool,
	pub unwrap: bool,
}

/// Minimal preference view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrefsView {
	pub view: String,
	pub playing: bool,
	pub speed: f64,
	pub split: bool,
	pub selected_node: Option<usize>,
	pub expanded_nodes: Vec<usize>,
	pub layers: LayerPrefs,
}

/// A renderable row summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VizRow {
	pub node: Option<usize>,
	pub segment_nodes: Vec<Option<usize>>,
	pub detail: Option<String>,
	pub tips: Vec<String>,
	pub expanded: Vec<(usize, String)>,
	pub annotations: Vec<(String, String, Option<String>, Option<usize>)>,
	pub effects: Vec<(String, String)>,
}

/// Read-only presentation view consumed by the inspector.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShellPresentationView {
	pub preferences: PrefsView,
	pub timeline: TimelineView,
	pub rows: Vec<VizRow>,
}

/// Focused inspector host; state and replay timers remain owned by the
/// transcript tool call.
pub trait ShellVisualizerHost {
	fn presentation(&self) -> Option<ShellPresentationView>;
	fn replay_epoch(&self) -> u64;
	fn live_origin(&self) -> bool;
	fn action(&mut self, value: &str);
	fn close(&mut self);
	fn request_render(&mut self);
	fn height(&self) -> usize;
	/// Render shell visualization body lines for given width.
	fn render_body(&self, presentation: &ShellPresentationView, width: usize) -> Vec<String>;
}

/// Keyboard and native inspector for the exact presentation shown in the
/// transcript.
pub struct ShellVisualizer<H> {
	pub host: H,
	scroll: usize,
	detail_scroll: usize,
}

impl<H: ShellVisualizerHost> ShellVisualizer<H> {
	pub fn new(host: H) -> Self {
		Self {
			host,
			scroll: 0,
			detail_scroll: 0,
		}
	}

	pub fn invalidate(&mut self) {}

	/// Handle an input token/string; returns `true` if consumed.
	pub fn handle_input(&mut self, data: &str) -> bool {
		if data == "escape" || data == "q" {
			self.host.close();
			self.host.request_render();
			return true;
		}
		let Some(presentation) = self.host.presentation() else {
			self.host.request_render();
			return true;
		};
		let prefs = presentation.preferences.clone();
		match data {
			"v" => {
				self.scroll = 0;
				let next = if prefs.view == "raw" { "pseudo" } else { "raw" };
				self.host.action(&format!("view:{next}"));
			}
			"1" | "2" => {
				self.scroll = 0;
				let name = if data == "1" { "raw" } else { "pseudo" };
				self.host.action(&format!("view:{name}"));
			}
			"c" => self.host.action("layer:color"),
			"i" => self.host.action("layer:inline"),
			"d" => self.host.action("layer:idioms"),
			"e" => self.host.action("layer:effects"),
			"u" => self.host.action("layer:unwrap"),
			"s" => self.host.action("split"),
			" " => self
				.host
				.action(if prefs.playing { "pause" } else { "play" }),
			"." | "right" => self.host.action("step"),
			"left" => {
				let seek = presentation.timeline.cursor.saturating_sub(1);
				self.host.action(&format!("seek:{seek}"));
			}
			"r" => self.host.action("reset"),
			"l" => self.host.action("live"),
			"+" | "=" => {
				let s = if prefs.speed < 1.0 {
					1.0
				} else if prefs.speed < 2.0 {
					2.0
				} else {
					4.0
				};
				self.host.action(&format!("speed:{s}"));
			}
			"-" => {
				let s = if prefs.speed > 2.0 {
					2.0
				} else if prefs.speed > 1.0 {
					1.0
				} else {
					0.5
				};
				self.host.action(&format!("speed:{s}"));
			}
			"home" => self.host.action("seek:0"),
			"end" => self
				.host
				.action(&format!("seek:{}", presentation.timeline.length)),
			"[" => self.detail_scroll = self.detail_scroll.saturating_sub(1),
			"]" => self.detail_scroll += 1,
			"pageDown" => self.scroll += self.host.height().saturating_sub(8).max(1),
			"pageUp" => {
				self.scroll = self
					.scroll
					.saturating_sub(self.host.height().saturating_sub(8).max(1))
			}
			"up" | "k" | "down" | "j" | "tab" => {
				let mut nodes: Vec<usize> = Vec::new();
				for row in &presentation.rows {
					if let Some(n) = row.node {
						if !nodes.contains(&n) {
							nodes.push(n);
						}
					}
					for sn in &row.segment_nodes {
						if let Some(n) = sn {
							if !nodes.contains(n) {
								nodes.push(*n);
							}
						}
					}
				}
				if !nodes.is_empty() {
					let dir: isize = if data == "up" || data == "k" { -1 } else { 1 };
					let idx = nodes
						.iter()
						.position(|n| Some(*n) == prefs.selected_node)
						.unwrap_or(0) as isize;
					let next = (idx + dir).rem_euclid(nodes.len() as isize) as usize;
					self.detail_scroll = 0;
					self.host.action(&format!("select:{}", nodes[next]));
				}
			}
			"enter" => {
				if let Some(n) = prefs.selected_node {
					self.detail_scroll = 0;
					self.host.action(&format!("expand:{n}"));
				}
			}
			_ => {}
		}
		self.host.request_render();
		true
	}

	/// Render inspector lines for `width`.
	pub fn render(&mut self, width: usize) -> Vec<String> {
		let Some(presentation) = self.host.presentation() else {
			return vec![
				"No complete bash command available".into(),
				"Escape closes".into(),
			];
		};
		let prefs = presentation.preferences.clone();
		let headings = format!(
			"v  Raw ↔ Pseudo  ·  {}",
			if prefs.view == "pseudo" { "Pseudo" } else { "Raw" }
		);
		let layers = format!(
			"color:{}  inline:{}  idioms:{}  effects:{}  unwrap:{}",
			on(prefs.layers.color),
			on(prefs.layers.inline),
			on(prefs.layers.idioms),
			on(prefs.layers.effects),
			on(prefs.layers.unwrap),
		);
		let header = vec![
			"Bash visualizer  ·  Escape closes".to_string(),
			headings,
			layers,
			format!(
				"c/i/d/e/u layers · s split:{} · ↑↓/Tab select · Enter details · PgUp/PgDn scroll",
				on(prefs.split)
			),
			format!(
				"Space {} · ←/→ step · r reset · l live · Home/End seek · −/+ speed {}× · {} / {}",
				if prefs.playing { "pause" } else { "play" },
				prefs.speed,
				format_ms(presentation.timeline.elapsed_ns / 1_000_000),
				format_ms(presentation.timeline.duration_ns / 1_000_000),
			),
		];
		let body = self.host.render_body(&presentation, width);
		let mut detail: Vec<String> = Vec::new();
		if let Some(sel) = prefs.selected_node {
			for row in &presentation.rows {
				let row_match = row.node == Some(sel) || row.segment_nodes.iter().any(|n| *n == Some(sel));
				if !row_match {
					continue;
				}
				if let Some(d) = &row.detail {
					detail.push(d.clone());
				}
				for t in &row.tips {
					detail.push(t.clone());
				}
				for (n, exp) in &row.expanded {
					if prefs.expanded_nodes.contains(n) {
						for line in exp.split('\n') {
							detail.push(line.to_string());
						}
					}
				}
				for (name, val, scope, full_len) in &row.annotations {
					let mut s = format!("{name} = {val}");
					if let Some(sc) = scope {
						s.push_str(&format!(" ({sc})"));
					}
					if let Some(fl) = full_len {
						if *fl > val.len() {
							s.push_str(&format!(" [{fl} bytes; retained value truncated]"));
						}
					}
					detail.push(s);
				}
				for (label, why) in &row.effects {
					detail.push(format!("{label}: {why}"));
				}
				break;
			}
		}
		let height = self.host.height().max(8);
		let detail_height = detail.len().min(height / 3);
		self.detail_scroll = self
			.detail_scroll
			.min(detail.len().saturating_sub(detail_height));
		let available = height
			.saturating_sub(header.len())
			.saturating_sub(detail_height)
			.saturating_sub(if detail_height > 0 { 1 } else { 0 })
			.max(1);
		self.scroll = self.scroll.min(body.len().saturating_sub(available));
		let mut out: Vec<String> = Vec::new();
		out.extend(header);
		let end = (self.scroll + available).min(body.len());
		if self.scroll < body.len() {
			out.extend(body[self.scroll..end].iter().cloned());
		}
		if detail_height > 0 {
			out.push(format!(
				"Selected details {}–{}/{} · [/] scroll",
				self.detail_scroll + 1,
				self.detail_scroll + detail_height,
				detail.len()
			));
			out.extend(
				detail[self.detail_scroll..self.detail_scroll + detail_height]
					.iter()
					.cloned(),
			);
		}
		out.truncate(width.max(1).min(out.len().max(1)).max(out.len()));
		out
	}

	pub fn scroll(&self) -> usize {
		self.scroll
	}
	pub fn detail_scroll(&self) -> usize {
		self.detail_scroll
	}
}

fn on(b: bool) -> &'static str {
	if b { "on" } else { "off" }
}

fn format_ms(ms: u64) -> String {
	if ms < 1000 {
		format!("{ms}ms")
	} else {
		let s = ms / 1000;
		let rem = (ms % 1000) / 100;
		if rem == 0 {
			format!("{s}s")
		} else {
			format!("{s}.{rem}s")
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	struct H {
		p: Option<ShellPresentationView>,
		actions: Vec<String>,
		closed: bool,
		h: usize,
	}
	impl H {
		fn new(p: Option<ShellPresentationView>) -> Self {
			Self { p, actions: vec![], closed: false, h: 20 }
		}
	}
	impl ShellVisualizerHost for H {
		fn presentation(&self) -> Option<ShellPresentationView> { self.p.clone() }
		fn replay_epoch(&self) -> u64 { 1 }
		fn live_origin(&self) -> bool { false }
		fn action(&mut self, value: &str) { self.actions.push(value.into()); }
		fn close(&mut self) { self.closed = true; }
		fn request_render(&mut self) {}
		fn height(&self) -> usize { self.h }
		fn render_body(&self, _p: &ShellPresentationView, _w: usize) -> Vec<String> {
			vec!["body-line".into()]
		}
	}

	fn pres() -> ShellPresentationView {
		ShellPresentationView {
			preferences: PrefsView {
				view: "raw".into(),
				playing: false,
				speed: 1.0,
				split: false,
				selected_node: None,
				expanded_nodes: vec![],
				layers: LayerPrefs::default(),
			},
			timeline: TimelineView { cursor: 2, length: 5, elapsed_ns: 1_500_000_000, duration_ns: 5_000_000_000 },
			rows: vec![],
		}
	}

	#[test]
	fn render_without_presentation_shows_placeholder() {
		let mut v = ShellVisualizer::new(H::new(None));
		let lines = v.render(40);
		assert_eq!(lines[0], "No complete bash command available");
	}

	#[test]
	fn escape_closes_host() {
		let mut v = ShellVisualizer::new(H::new(Some(pres())));
		v.handle_input("escape");
		assert!(v.host.closed);
	}

	#[test]
	fn view_toggle_and_seek_actions() {
		let mut v = ShellVisualizer::new(H::new(Some(pres())));
		v.handle_input("v");
		v.handle_input("left");
		assert_eq!(v.host.actions, vec!["view:pseudo", "seek:1"]);
	}

	#[test]
	fn render_includes_header_and_body() {
		let mut v = ShellVisualizer::new(H::new(Some(pres())));
		let lines = v.render(80);
		assert!(lines[0].contains("Bash visualizer"));
		assert!(lines.iter().any(|l| l == "body-line"));
		assert!(lines.iter().any(|l| l.contains("speed 1")));
	}

	#[test]
	fn format_ms_helpers() {
		assert_eq!(format_ms(500), "500ms");
		assert_eq!(format_ms(2000), "2s");
		assert_eq!(format_ms(2500), "2.5s");
	}
}
