//! Spotlight sheet overlay.
//!
//! Reference: `l/src/overlays/spotlight-sheet.ts`.
//!
//! Deviations:
//! - Spotlight trace types and native node rendering are modeled locally; the
//!   crate has no `@oh-my-pi/pi-wire`/TS TUI runtime or global theming.
//! - Text-mode rendering returns a compact summary; native description emits a
//!   semantic column with request parameters, legend, window sections, and
//!   shaping notes when provided.
//! - Color/heat styling is represented via span tokens/roles rather than ANSI.

use crate::component::Component;
use crate::keybinding_matchers::matches_select_cancel;
use crate::keys::Key;
use crate::native::describe::{code, col, node, row, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::text_shape::truncate_to_width;

/// Default number of original lines rendered in the spotlight view.
pub const SPOTLIGHT_VIEW_LINES: usize = 400;

/// Why a line was shown or hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotlightReason {
	/// Kept by score.
	A,
	/// Kept as structure.
	S,
	/// Named.
	N,
	/// Kept as context.
	C,
	/// Cut by budget.
	B,
	/// Seen in an earlier call.
	R,
	/// Folded.
	F,
}

impl SpotlightReason {
	/// Parse a single-letter reason code.
	#[must_use]
	pub fn parse(ch: char) -> Option<Self> {
		match ch {
			'a' => Some(Self::A),
			's' => Some(Self::S),
			'n' => Some(Self::N),
			'c' => Some(Self::C),
			'b' => Some(Self::B),
			'r' => Some(Self::R),
			'f' => Some(Self::F),
			_ => None,
		}
	}

	/// Whether the model saw the line.
	#[must_use]
	pub fn shows(self) -> bool {
		matches!(self, Self::A | Self::S | Self::N | Self::C)
	}

	/// Single-letter code.
	#[must_use]
	pub fn code(self) -> char {
		match self {
			Self::A => 'a',
			Self::S => 's',
			Self::N => 'n',
			Self::C => 'c',
			Self::B => 'b',
			Self::R => 'r',
			Self::F => 'f',
		}
	}

	/// Tone token for gutter/legend swatches.
	#[must_use]
	pub fn tone(self) -> &'static str {
		match self {
			Self::A => "accent",
			Self::S => "neutral",
			Self::N => "user",
			Self::C => "info",
			Self::B | Self::R | Self::F => "muted",
		}
	}
}

/// Why a hidden line went, as a fold says it.
#[must_use]
pub fn cut_why(reason: SpotlightReason) -> Option<&'static str> {
	match reason {
		SpotlightReason::B => Some("cut by the budget"),
		SpotlightReason::R => Some("seen in an earlier call"),
		SpotlightReason::F => Some("folded"),
		_ => None,
	}
}

/// A legend class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineClass {
	A,
	S,
	N,
	C,
	Hot,
	Cut,
}

/// One legend entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegendEntry {
	pub cls: LineClass,
	pub tone: &'static str,
	pub label: &'static str,
}

/// The summary legend in order.
pub const LEGEND: &[LegendEntry] = &[
	LegendEntry { cls: LineClass::A, tone: "accent", label: "kept by score" },
	LegendEntry { cls: LineClass::S, tone: "neutral", label: "kept as structure" },
	LegendEntry { cls: LineClass::N, tone: "user", label: "named" },
	LegendEntry { cls: LineClass::C, tone: "info", label: "kept as context" },
	LegendEntry { cls: LineClass::Hot, tone: "error", label: "cut though hot" },
	LegendEntry { cls: LineClass::Cut, tone: "muted", label: "cut" },
];

/// Request parameters shown in the sheet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpotlightRequestParams {
	pub intent: String,
	pub said: String,
	pub task: String,
	pub commands: Vec<String>,
	pub source: String,
	pub probe: String,
	pub model: String,
	pub thinking: String,
}

/// One ranking window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightRank {
	pub parts: Vec<String>,
	pub budget: Option<String>,
	pub cutoff: Option<String>,
}

/// One original window/section.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightWindow {
	pub path: Option<String>,
	pub clip: Option<String>,
	pub rank: Option<SpotlightRank>,
	pub start: usize,
	pub end: usize,
}

/// Original-to-reduced mapping for notes/folds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightOriginal {
	/// Reduced-line index -> original-line index or `None` for a note.
	pub map: Option<Vec<Option<usize>>>,
	/// Per-line reasons.
	pub reasons: Vec<char>,
	/// Optional per-line heat.
	pub heat: Vec<f32>,
	/// Optional windows.
	pub windows: Vec<SpotlightWindow>,
}

/// Spotlight trace details.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightDetails {
	pub tool: String,
	pub in_chars: usize,
	pub out_chars: usize,
	pub holdout: bool,
	pub gated: bool,
	pub request: Option<SpotlightRequestParams>,
	pub original: Option<SpotlightOriginal>,
}

/// Options for [`SpotlightSheet`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightSheetOptions {
	pub spotlight: SpotlightDetails,
	pub original: String,
	pub reduced: String,
}

fn sanitize_text(input: &str) -> String {
	input.replace(['\r', '\t'], " ")
}

/// A count with thousands separators.
#[must_use]
pub fn count(n: usize) -> String {
	let s = n.to_string();
	let mut out = String::with_capacity(s.len() + s.len() / 3);
	for (i, ch) in s.chars().rev().enumerate() {
		if i > 0 && i % 3 == 0 {
			out.push(',');
		}
		out.push(ch);
	}
	out.chars().rev().collect()
}

/// A score or heat to two decimals.
#[must_use]
pub fn fixed(n: f32) -> String {
	format!("{n:.2}")
}

/// Whether a trace has an original to show.
#[must_use]
pub fn has_spotlight_original(spotlight: &SpotlightDetails) -> bool {
	spotlight.original.is_some()
}

/// Saving in tokens/chars proxy (out - in in chars for display fallback).
#[must_use]
pub fn spotlight_saving(details: &SpotlightDetails) -> i64 {
	details.in_chars as i64 - details.out_chars as i64
}

/// The original windows, defaulting to one covering all lines.
#[must_use]
pub fn original_windows(original: &SpotlightOriginal, total_lines: usize) -> Vec<SpotlightWindow> {
	if original.windows.is_empty() {
		vec![SpotlightWindow {
			path: None,
			clip: None,
			rank: None,
			start: 0,
			end: total_lines,
		}]
	} else {
		original.windows.clone()
	}
}

/// Reason at line index.
#[must_use]
pub fn reason_at(original: &SpotlightOriginal, index: usize) -> Option<SpotlightReason> {
	original.reasons.get(index).copied().and_then(SpotlightReason::parse)
}

/// Heat at line index.
#[must_use]
pub fn heat_at(original: &SpotlightOriginal, index: usize) -> Option<f32> {
	original.heat.get(index).copied()
}

/// Whether a line was shown.
#[must_use]
pub fn reason_shows(reason: Option<SpotlightReason>) -> bool {
	reason.is_some_and(SpotlightReason::shows)
}

/// Whether a cut line is hot.
#[must_use]
pub fn spotlight_hot(original: &SpotlightOriginal, index: usize) -> bool {
	!reason_shows(reason_at(original, index)) && heat_at(original, index).unwrap_or(0.0) >= 0.75
}

/// Rank label for parts.
#[must_use]
pub fn rank_label(rank: &SpotlightRank) -> String {
	if rank.parts.len() > 1 {
		format!("row tint, left to right: {}", rank.parts.join(" → "))
	} else if let Some(part) = rank.parts.first() {
		format!("row tint: {part}")
	} else {
		"row tint".to_string()
	}
}

/// Non-blank request params in sheet order.
#[must_use]
pub fn non_blank_params(params: &SpotlightRequestParams) -> Vec<(String, Vec<String>, Option<&'static str>)> {
	let all: Vec<(String, Vec<String>, Option<&'static str>)> = vec![
		("intent".into(), vec![params.intent.clone()], None),
		("said".into(), vec![params.said.clone()], None),
		("task".into(), vec![params.task.clone()], None),
		("commands".into(), params.commands.clone(), Some("mono")),
		("source".into(), vec![params.source.clone()], Some("mono")),
		("probe".into(), vec![params.probe.clone()], Some("mono")),
		(
			"model".into(),
			vec![params.model.clone(), format!("{} thinking", params.thinking)],
			None,
		),
	];
	all.into_iter()
		.filter(|(_, values, _)| values.iter().any(|v| !sanitize_text(v).trim().is_empty()))
		.collect()
}

/// The shaping notes the model saw/would have seen.
#[must_use]
pub fn shaping_notes(original: &SpotlightOriginal, reduced_lines: &[String]) -> Vec<String> {
	let Some(map) = &original.map else {
		return Vec::new();
	};
	let mut notes = Vec::new();
	for (j, mapped) in map.iter().enumerate() {
		if mapped.is_none() {
			if let Some(line) = reduced_lines.get(j) {
				let trimmed = line.trim_end();
				if !trimmed.is_empty() {
					notes.push(trimmed.to_string());
				}
			}
		}
	}
	notes
}

/// Summary counts for a window/whole original.
#[must_use]
pub fn summarize_lines(original: &SpotlightOriginal, end: usize) -> (usize, usize, usize, usize, usize, usize) {
	let mut a = 0usize;
	let mut s = 0usize;
	let mut n = 0usize;
	let mut c = 0usize;
	let mut hot = 0usize;
	let mut cut = 0usize;
	for i in 0..end.min(original.reasons.len()) {
		match reason_at(original, i) {
			Some(SpotlightReason::A) => a += 1,
			Some(SpotlightReason::S) => s += 1,
			Some(SpotlightReason::N) => n += 1,
			Some(SpotlightReason::C) => c += 1,
			_ => {
				if spotlight_hot(original, i) {
					hot += 1;
				} else {
					cut += 1;
				}
			}
		}
	}
	(a, s, n, c, hot, cut)
}

fn legend_node() -> NativeNode {
	let mut children = Vec::new();
	for entry in LEGEND {
		let swatch = text(NodeText::Spans(vec![span("●", Some(entry.tone))]), Props::new());
		let label = text(NodeText::Spans(vec![span(entry.label, Some("muted"))]), Props::new());
		let mut row_props = Props::new();
		row_props.set_str("gap", "xs");
		row_props.set_str("align", "center");
		row_props.set_role("omp.spotlight.key");
		children.push(row(vec![swatch, label], row_props));
	}
	let mut props = Props::new();
	props.set_str("gap", "md");
	props.set_bool("wrap", true);
	props.set_role("omp.spotlight.legend");
	row(children, props)
}

fn params_node(params: &SpotlightRequestParams, expanded: bool) -> Option<NativeNode> {
	let entries = non_blank_params(params);
	if entries.is_empty() {
		return None;
	}
	let mut rows = Vec::new();
	let shown_entries: Vec<_> = if expanded { entries } else { entries.into_iter().take(1).collect() };
	for (name, values, style) in shown_entries {
		let whole: Vec<String> = values
			.into_iter()
			.map(|v| sanitize_text(&v).trim().to_string())
			.filter(|v| !v.is_empty())
			.collect();
		if whole.is_empty() {
			continue;
		}
		let mut spans_out = Vec::new();
		for (i, value) in whole.iter().enumerate() {
			if i > 0 {
				spans_out.push(span(" · ", style.map(|s| format!("dim {s}")).as_deref().or(Some("dim"))));
			}
			spans_out.push(span(value.replace(char::is_whitespace, " "), style));
		}
		let mut text_props = Props::new();
		text_props.set_i64("lines", 2);
		text_props.set_str("truncate", "end");
		text_props.set_str("title", &whole.join("\n"));
		let mut row_props = Props::new();
		row_props.set_str("gap", "md");
		row_props.set_str("align", "baseline");
		row_props.set_role("omp.spotlight.param");
		rows.push(row(
			vec![
				text(NodeText::Spans(vec![span(name, None)]), Props::new()),
				text(NodeText::Spans(spans_out), text_props),
			],
			row_props,
		));
	}
	if rows.is_empty() {
		return None;
	}
	let mut props = Props::new();
	props.set_str("gap", "xs");
	props.set_role("omp.spotlight.params");
	Some(col(rows, props))
}

/// The Spotlight sheet component.
#[derive(Clone, Debug)]
pub struct SpotlightSheet {
	options: SpotlightSheetOptions,
	expanded: bool,
}

impl SpotlightSheet {
	/// Create a new spotlight sheet.
	#[must_use]
	pub fn new(options: SpotlightSheetOptions) -> Self {
		Self { options, expanded: false }
	}

	/// Native overlay metadata role/head/size/anchor equivalent.
	#[must_use]
	pub fn native_head(&self) -> String {
		let s = &self.options.spotlight;
		let saving = spotlight_saving(s);
		let saved = if s.holdout || s.gated {
			format!("would save {} toks", count(saving.max(0) as usize))
		} else if saving >= 0 {
			format!("saved {} toks", count(saving as usize))
		} else {
			format!("+{} toks", count((-saving) as usize))
		};
		let held_out = if s.holdout {
			"held out (the model saw the original) · "
		} else if s.gated {
			"kept whole for editing (the model saw the original) · "
		} else {
			""
		};
		format!(
			"★ Spotlight · {held_out}{} · {} → {} chars · {saved}",
			s.tool,
			count(s.in_chars),
			count(s.out_chars)
		)
	}

	fn body_nodes(&self) -> Vec<NativeNode> {
		let s = &self.options.spotlight;
		let Some(original) = &s.original else {
			return vec![text(NodeText::Spans(vec![span("No spotlight original.", Some("muted"))]), Props::new())];
		};
		let all: Vec<String> = sanitize_text(&self.options.original).split('\n').map(str::to_string).collect();
		let lines = &all[..all.len().min(SPOTLIGHT_VIEW_LINES)];
		let windows = original_windows(original, lines.len());
		let (a, st, n, c, hot, cut) = summarize_lines(original, lines.len());
		let mut out = Vec::new();
		let mut summary_props = Props::new();
		summary_props.set_role("omp.spotlight.summary");
		out.push(col(
			vec![
				text(
					NodeText::Spans(vec![span(
						format!(
							"shown {} · hidden {} · hot hidden {} (of {})",
							count(a + st + n + c),
							count(hot + cut),
							count(hot),
							count(lines.len())
						),
						Some("muted"),
					)]),
					Props::new(),
				),
				legend_node(),
			],
			summary_props,
		));

		for window in &windows {
			let mut section_children = Vec::new();
			let mut head_children = Vec::new();
			if let Some(path) = &window.path {
				head_children.push(text(NodeText::Spans(vec![span(path.clone(), Some("mono"))]), Props::new()));
			}
			if let Some(clip) = &window.clip {
				head_children.push(text(NodeText::Spans(vec![span(clip.clone(), Some("muted"))]), {
					let mut p = Props::new();
					p.set_role("omp.spotlight.clip");
					p
				}));
			}
			if !head_children.is_empty() || window.rank.is_some() {
				head_children.push(node(NodeKind::Spacer, {
					let mut p = Props::new();
					p.set_bool("grow", true);
					p
				}, Vec::new(), None));
				if let Some(rank) = &window.rank {
					let mut bits = Vec::new();
					if let Some(budget) = &rank.budget {
						bits.push(format!("budget {budget}"));
					}
					if let Some(cutoff) = &rank.cutoff {
						bits.push(format!("cutoff {cutoff}"));
					}
					if !bits.is_empty() {
						head_children.push(text(NodeText::Spans(vec![span(bits.join(" · "), Some("muted"))]), {
							let mut p = Props::new();
							p.set_role("omp.spotlight.budget");
							p
						}));
					}
				}
				let mut head_props = Props::new();
				head_props.set_str("gap", "md");
				head_props.set_str("align", "baseline");
				head_props.set_role("omp.spotlight.head");
				section_children.push(row(head_children, head_props));
			}
			let start = window.start.min(lines.len());
			let end = window.end.min(lines.len());
			let mut code_lines: Vec<String> = Vec::new();
			for i in start..end {
				let line = &lines[i];
				let reason = reason_at(original, i);
				let marker = if reason_shows(reason) { " " } else { "·" };
				let why = reason.and_then(cut_why).map(|w| format!(" [{w}]")).unwrap_or_default();
				let heat = heat_at(original, i).map(|h| format!(" {h:.2}")).unwrap_or_default();
				code_lines.push(format!("{marker} {:>4}{}{} {}", i + 1, heat, why, line));
			}
			if !code_lines.is_empty() {
				section_children.push(code(code_lines.join("\n"), {
					let mut p = Props::new();
					p.set_str("lang", "text");
					p.set_bool("wrap", false);
					p
				}));
			}
			if !section_children.is_empty() {
				let mut wprops = Props::new();
				wprops.set_str("gap", "xs");
				wprops.set_role("omp.spotlight.window");
				out.push(col(section_children, wprops));
			}
		}

		let reduced_lines: Vec<String> = sanitize_text(&self.options.reduced).split('\n').map(str::to_string).collect();
		let notes = shaping_notes(original, &reduced_lines);
		if !notes.is_empty() {
			let heading = if s.holdout || s.gated {
				"notes the model would have seen"
			} else {
				"notes the model saw"
			};
			let mut children = vec![text(NodeText::Spans(vec![span(heading, Some("muted"))]), Props::new())];
			children.extend(notes.into_iter().map(|note| {
				text(NodeText::Spans(vec![span(note, Some("muted mono"))]), Props::new())
			}));
			let mut props = Props::new();
			props.set_str("gap", "xs");
			props.set_role("omp.spotlight.notes");
			out.push(col(children, props));
		}

		let rank_labels: Vec<String> = windows.iter().filter_map(|w| w.rank.as_ref().map(rank_label)).collect();
		if let Some(label) = rank_labels.first() {
			out.push(text(NodeText::Spans(vec![span(label.clone(), Some("dim"))]), {
				let mut p = Props::new();
				p.set_role("omp.spotlight.legend");
				p
			}));
		}
		out
	}
}

impl Component for SpotlightSheet {
	fn render(&mut self, width: u16) -> Vec<String> {
		vec![
			truncate_to_width(&self.native_head(), width as usize, crate::text_shape::Ellipsis::Unicode),
			truncate_to_width("Esc closes", width as usize, crate::text_shape::Ellipsis::Unicode),
		]
	}

	fn handle_input(&mut self, key: &Key) {
		if matches_select_cancel(key, &crate::keybindings::KeybindingsManager::default()) {
			// Closing is host-owned; no-op in component-only port.
		}
	}
}

impl NativeDescribe for SpotlightSheet {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let mut children = Vec::new();
		if let Some(request) = &self.options.spotlight.request {
			if let Some(params) = params_node(request, self.expanded) {
				children.push(params);
			}
		}
		children.extend(self.body_nodes());
		let mut props = Props::new();
		props.set_str("gap", "md");
		props.set_role("omp.overlay.spotlight");
		col(children, props)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn sample() -> SpotlightSheetOptions {
		SpotlightSheetOptions {
			spotlight: SpotlightDetails {
				tool: "read".into(),
				in_chars: 1200,
				out_chars: 300,
				holdout: false,
				gated: false,
				request: Some(SpotlightRequestParams {
					intent: "find auth".into(),
					model: "m".into(),
					thinking: "off".into(),
					..Default::default()
				}),
				original: Some(SpotlightOriginal {
					reasons: vec!['a', 'b', 'r', 'n', 'f'],
					heat: vec![0.9, 0.8, 0.1, 0.2, 0.3],
					windows: vec![SpotlightWindow {
						path: Some("src/lib.rs".into()),
						clip: None,
						rank: Some(SpotlightRank {
							parts: vec!["query".into(), "recency".into()],
							budget: Some("2k".into()),
							cutoff: Some("0.20".into()),
						}),
						start: 0,
						end: 5,
					}],
					map: Some(vec![Some(0), None]),
				}),
			},
			original: "a\nb\nc\nd\ne".into(),
			reduced: "a\n[note]".into(),
		}
	}

	#[test]
	fn cut_why_maps_hidden_reasons() {
		assert_eq!(cut_why(SpotlightReason::B), Some("cut by the budget"));
		assert_eq!(cut_why(SpotlightReason::R), Some("seen in an earlier call"));
		assert_eq!(cut_why(SpotlightReason::F), Some("folded"));
		assert_eq!(cut_why(SpotlightReason::A), None);
	}

	#[test]
	fn legend_has_expected_classes() {
		assert_eq!(LEGEND.len(), 6);
		assert_eq!(LEGEND[0].label, "kept by score");
		assert_eq!(LEGEND[5].label, "cut");
	}

	#[test]
	fn non_blank_params_filters_and_keeps_order() {
		let params = SpotlightRequestParams {
			intent: "x".into(),
			said: " ".into(),
			task: String::new(),
			commands: vec!["rg foo".into()],
			source: String::new(),
			probe: String::new(),
			model: "m".into(),
			thinking: "low".into(),
		};
		let out = non_blank_params(&params);
		assert_eq!(out[0].0, "intent");
		assert!(out.iter().any(|(name, _, _)| name == "commands"));
		assert!(out.iter().any(|(name, _, _)| name == "model"));
	}

	#[test]
	fn summary_counts_hot_cut_and_shown() {
		let original = sample().spotlight.original.unwrap();
		let (a, _s, n, _c, hot, cut) = summarize_lines(&original, 5);
		assert_eq!(a, 1);
		assert_eq!(n, 1);
		assert_eq!(hot, 1);
		assert_eq!(cut, 2);
	}

	#[test]
	fn shaping_notes_collect_unmapped_reduced_lines() {
		let original = sample().spotlight.original.unwrap();
		let notes = shaping_notes(&original, &["a".into(), "[note]".into()]);
		assert_eq!(notes, vec!["[note]".to_string()]);
	}

	#[test]
	fn head_and_describe_reflect_original_and_params() {
		let sheet = SpotlightSheet::new(sample());
		let head = sheet.native_head();
		assert!(head.contains("★ Spotlight"));
		assert!(head.contains("read"));
		assert!(head.contains("saved"));
		let described = sheet.describe_native(&DescribeContext::new(80));
		assert_eq!(described.kind, NodeKind::Col);
		assert_eq!(described.props.role.as_deref(), Some("omp.overlay.spotlight"));
		assert!(!described.children.is_empty());
	}

	#[test]
	fn rank_label_joins_parts() {
		let rank = SpotlightRank {
			parts: vec!["query".into(), "recency".into()],
			budget: None,
			cutoff: None,
		};
		assert_eq!(rank_label(&rank), "row tint, left to right: query → recency");
	}

	#[test]
	fn render_is_compact_summary() {
		let mut sheet = SpotlightSheet::new(sample());
		let rows = sheet.render(60);
		assert_eq!(rows.len(), 2);
		assert!(rows[0].contains("Spotlight"));
		assert!(rows[1].contains("Esc closes"));
	}
}
