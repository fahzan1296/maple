//! Display-only transcript card for advisor notes.
//!
//! Reference: `l/src/chat/advisor-message.ts`.
//!
//! Styled as a distinct voice so notes never blend into thinking output: a bold
//! `customMessageLabel` header tag, a rail tinted per note severity, and the
//! note body on the default text color. Natively it is a callout tinted by its
//! worst note: that severity's icon and tally head it and, with several notes,
//! each note leads with its own severity icon.
//!
//! Deviations:
//! - The crate's `Disclosure` is a title/body fold, so the header/rail rows are
//!   composed here; the caller still owns transcript-wide expansion.
//! - `chatTranscriptDisplayPreferences.advisorCollapsedLines` has no reader
//!   wired yet, so the collapsed row cap is fixed at three notes.
//! - Icons are text glyphs: the wire `icon` node kind takes names, not SVGs.

use std::collections::HashSet;

use crate::chat::messages::{AdvisorMessageDetails, AdvisorNote, AdvisorSeverity};
use crate::component::Component;
use crate::native::describe::{card, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span, Tone};
use crate::render::render_utils::{format_badge, replace_tabs, ToolUIColor};
use crate::text_shape::{
	truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis,
};
use crate::theme::theme_class::Theme;

/// Notes shown before the fold.
pub const COLLAPSED_NOTES: usize = 3;

/// Widest a note's text column grows.
pub const NOTE_LINE_WIDTH: usize = 110;

/// Wrap the first line at `w1` and the remainder at `w2`, so a severity badge
/// and its metadata only indent the first row.
pub fn wrap_varying(text: &str, w1: usize, w2: usize) -> Vec<String> {
	if text.is_empty() {
		return Vec::new();
	}
	let first_wrap = wrap_text_with_ansi(text, w1);
	if first_wrap.len() <= 1 {
		return first_wrap;
	}
	let first_line = first_wrap[0].clone();
	let Some(index) = text.find(&first_line) else {
		return wrap_text_with_ansi(text, w2);
	};
	let remainder = text[index + first_line.len()..].trim_start();
	let mut rows = vec![first_line];
	rows.extend(wrap_text_with_ansi(remainder, w2));
	rows
}

/// The colour a severity paints with.
pub fn severity_color(severity: Option<AdvisorSeverity>) -> ToolUIColor {
	match severity {
		Some(AdvisorSeverity::Blocker) => ToolUIColor::Error,
		Some(AdvisorSeverity::Concern) => ToolUIColor::Warning,
		_ => ToolUIColor::Muted,
	}
}

/// Severity token name of a note rank.
pub fn severity_token(severity: Option<AdvisorSeverity>) -> &'static str {
	match severity {
		Some(AdvisorSeverity::Blocker) => "error",
		Some(AdvisorSeverity::Concern) => "warning",
		_ => "muted",
	}
}

/// The advisor a note came from; `None` for the implicit single advisor.
pub fn source_of(entry: &AdvisorNote) -> Option<&str> {
	entry
		.advisor
		.as_deref()
		.filter(|advisor| !advisor.is_empty() && *advisor != "default")
}

/// Wrapped, truncated rail rows for one note.
pub fn render_advisor_note(entry: &AdvisorNote, width: usize, theme: &Theme) -> Vec<String> {
	let badge = entry.severity.map_or_else(String::new, |severity| {
		format!(
			"{} ",
			format_badge(
				severity.as_str(),
				severity_color(Some(severity)),
				theme
			)
		)
	});
	let who = source_of(entry)
		.map(|source| {
			format!(
				"{} ",
				theme
					.fg("dim", &format!("[{}]", replace_tabs(source)))
					.unwrap_or_else(|_| format!("[{}]", replace_tabs(source)))
			)
		})
		.unwrap_or_default();
	let age = entry
		.turns_ago
		.map(|turns| format!("{} ", theme.fg("dim", &format!("T-{turns}")).unwrap_or_else(|_| format!("T-{turns}"))))
		.unwrap_or_default();
	let rail_glyph = {
		let glyph = theme.symbol("advisor.rail");
		if glyph.is_empty() { "▌" } else { glyph }.to_string()
	};
	let rail = theme
		.fg(severity_token(entry.severity), &rail_glyph)
		.unwrap_or_else(|_| rail_glyph.clone());
	let quote_width = visible_width(&format!("  {rail_glyph} "));
	let badge_width = visible_width(&badge);
	let age_width = visible_width(&age);
	let who_width = visible_width(&who);
	let capped = width.min(NOTE_LINE_WIDTH);
	let w1 = capped
		.saturating_sub(quote_width + badge_width + age_width + who_width)
		.max(10);
	let w2 = capped.saturating_sub(quote_width).max(10);

	let mut body = Vec::new();
	for (index, paragraph) in entry
		.note
		.split('\n')
		.filter(|paragraph| !paragraph.trim().is_empty())
		.enumerate()
	{
		if index == 0 {
			body.extend(wrap_varying(paragraph, w1, w2));
		} else {
			body.extend(wrap_text_with_ansi(paragraph, w2));
		}
	}

	body.into_iter()
		.enumerate()
		.map(|(index, line)| {
			let lead = if index == 0 {
				format!("{badge}{age}{who}")
			} else {
				String::new()
			};
			let styled = theme
				.fg("customMessageText", &replace_tabs(&line))
				.unwrap_or_else(|_| replace_tabs(&line));
			format!("  {rail} {lead}{styled}")
		})
		.collect()
}

/// Ranks worst first.
pub fn advisor_ranks() -> [&'static str; 4] {
	["blocker", "concern", "nit", "note"]
}

/// A note's rank: its severity, `note` when absent.
pub fn rank_of(entry: &AdvisorNote) -> &'static str {
	match entry.severity {
		Some(AdvisorSeverity::Blocker) => "blocker",
		Some(AdvisorSeverity::Concern) => "concern",
		Some(AdvisorSeverity::Nit) => "nit",
		None => "note",
	}
}

fn rank_icon(rank: &str) -> &'static str {
	match rank {
		"blocker" => "⊘",
		"concern" => "⚠",
		"nit" => "✦",
		_ => "ⓘ",
	}
}

fn rank_label(rank: &str) -> &'static str {
	match rank {
		"blocker" => "Blocker",
		"concern" => "Concern",
		"nit" => "Nit",
		_ => "Note",
	}
}

fn rank_tone(rank: &str) -> Tone {
	match rank {
		"blocker" => Tone::Error,
		"concern" => Tone::Warning,
		_ => Tone::Neutral,
	}
}

fn rank_style(rank: &str) -> &'static str {
	match rank {
		"blocker" => "error strong",
		"concern" => "warning strong",
		_ => "strong",
	}
}

/// The callout title: one note's rank, else a tally worst first.
pub fn tally_spans(notes: &[AdvisorNote]) -> Vec<Span> {
	if notes.len() == 1 {
		let rank = rank_of(&notes[0]);
		return vec![span(rank_label(rank), Some(rank_style(rank)))];
	}
	let mut spans = Vec::new();
	for rank in advisor_ranks() {
		let count = notes.iter().filter(|entry| rank_of(entry) == rank).count();
		if count == 0 {
			continue;
		}
		if !spans.is_empty() {
			spans.push(span(" · ", Some("dim")));
		}
		spans.push(span(
			format!("{count} {rank}{}", if count == 1 { "" } else { "s" }),
			Some(rank_style(rank)),
		));
	}
	spans
}

/// Native spans for one note: its advisor when the callout mixes several, then
/// the text.
pub fn note_spans(entry: &AdvisorNote, attribute: bool) -> Vec<Span> {
	let body = span(entry.note.clone(), None);
	if attribute {
		if let Some(source) = source_of(entry) {
			return vec![span(format!("{source} · "), Some("dim")), body];
		}
	}
	vec![body]
}

/// The advisor card: controlled expansion plus terminal-local toggles.
pub struct AdvisorMessageCard {
	theme: Theme,
	notes: Vec<AdvisorNote>,
	meta: Vec<String>,
	get_expanded: Box<dyn Fn() -> bool>,
	last_global: bool,
	override_expanded: Option<bool>,
	cache: Option<(u16, bool, Vec<String>)>,
}

impl AdvisorMessageCard {
	/// The card for `details`, drawing in `theme`.
	pub fn new(
		details: Option<&AdvisorMessageDetails>,
		get_expanded: Box<dyn Fn() -> bool>,
		theme: Theme,
	) -> Self {
		let notes = details.map(|details| details.notes.clone()).unwrap_or_default();
		let blockers = notes
			.iter()
			.filter(|note| note.severity == Some(AdvisorSeverity::Blocker))
			.count();
		let mut meta = vec![format!(
			"{} {}",
			notes.len(),
			if notes.len() == 1 { "note" } else { "notes" }
		)];
		if blockers > 0 {
			let label = format!(
				"{blockers} blocker{}",
				if blockers == 1 { "" } else { "s" }
			);
			meta.push(theme.fg("error", &label).unwrap_or(label));
		}
		let last_global = get_expanded();
		AdvisorMessageCard {
			theme,
			notes,
			meta,
			get_expanded,
			last_global,
			override_expanded: None,
			cache: None,
		}
	}

	/// The notes the card shows.
	pub fn notes(&self) -> &[AdvisorNote] {
		&self.notes
	}

	/// The current expansion, honouring a terminal-local toggle until the next
	/// transcript-wide change.
	pub fn expanded(&mut self) -> bool {
		let global = (self.get_expanded)();
		if global != self.last_global {
			self.last_global = global;
			self.override_expanded = None;
		}
		self.override_expanded.unwrap_or(global)
	}

	/// Follow a transcript-wide expansion change.
	pub fn set_expanded(&mut self, expanded: bool) {
		self.last_global = expanded;
		self.override_expanded = None;
		self.cache = None;
	}

	/// Apply a terminal-local toggle.
	pub fn toggle(&mut self, expanded: bool) {
		self.override_expanded = Some(expanded);
		self.cache = None;
	}

	/// The always-visible header tag rows.
	pub fn header_rows(&self, width: u16) -> Vec<String> {
		let width = width.max(1) as usize;
		let info = self.theme.status().info.clone();
		let title = self.theme.bold(&format!("{info} Advisor"));
		let tag = self
			.theme
			.fg("customMessageLabel", &title)
			.unwrap_or(title);
		let dot = self.theme.symbol("sep.dot");
		let meta = self
			.theme
			.fg("dim", &self.meta.join(dot))
			.unwrap_or_else(|_| self.meta.join(dot));
		vec![truncate_to_width(
			&format!("{tag} {meta}"),
			width,
			Ellipsis::Unicode,
		)]
	}

	/// The note rows for `notes`.
	pub fn note_rows(&self, notes: &[AdvisorNote], hidden: usize, width: u16) -> Vec<String> {
		let width = (width.max(1)) as usize;
		let mut rows = Vec::new();
		for entry in notes {
			rows.extend(render_advisor_note(entry, width, &self.theme));
		}
		if hidden > 0 {
			let rail = {
				let glyph = self.theme.symbol("advisor.rail");
				if glyph.is_empty() { "▌" } else { glyph }.to_string()
			};
			let rail = self
				.theme
				.fg("dim", &rail)
				.unwrap_or_else(|_| rail.clone());
			let more = format!(
				"… +{hidden} more {}",
				if hidden == 1 { "note" } else { "notes" }
			);
			let more = self.theme.fg("dim", &more).unwrap_or(more);
			rows.push(format!("  {rail} {more}"));
		}
		rows.into_iter()
			.map(|row| truncate_to_width(&row, width, Ellipsis::Unicode))
			.collect()
	}

	/// The callout's worst rank.
	pub fn worst_rank(&self) -> &'static str {
		advisor_ranks()
			.into_iter()
			.find(|rank| self.notes.iter().any(|entry| rank_of(entry) == *rank))
			.unwrap_or("note")
	}

	/// Advisors whose notes the callout mixes.
	pub fn sources(&self) -> HashSet<Option<&str>> {
		self.notes.iter().map(source_of).collect()
	}
}

impl Component for AdvisorMessageCard {
	fn render(&mut self, width: u16) -> Vec<String> {
		let expanded = self.expanded();
		if let Some((cached_width, cached_expanded, rows)) = &self.cache {
			if *cached_width == width && *cached_expanded == expanded {
				return rows.clone();
			}
		}
		let mut rows = self.header_rows(width);
		if expanded {
			rows.extend(self.note_rows(&self.notes, 0, width));
		} else {
			let shown = self.notes.len().min(COLLAPSED_NOTES);
			rows.extend(self.note_rows(&self.notes[..shown], self.notes.len() - shown, width));
		}
		self.cache = Some((width, expanded, rows.clone()));
		rows
	}

	fn invalidate(&mut self) {
		self.cache = None;
	}
}

impl crate::native::describe::NativeDescribe for AdvisorMessageCard {
	fn describe_native(&self, _context: &DescribeContext) -> NativeNode {
		let rank = self.worst_rank();
		let expanded = self.override_expanded.unwrap_or(self.last_global);
		let sources = self.sources();
		let sole = if sources.len() == 1 { sources.iter().next().copied().flatten() } else { None };
		let mut head_props = Props::new();
		head_props.set_str("gap", "sm");
		head_props.set_str("align", "center");
		head_props.set_str("role", "omp.advisor.head");
		let source_label = sole.unwrap_or("Advisor");
		let head = node(
			NodeKind::Row,
			head_props,
			vec![
				icon_node(rank_icon(rank), rank_tone(rank)),
				text(NodeText::Spans(tally_spans(&self.notes)), {
					let mut props = Props::new();
					props.set_bool("grow", true);
					props
				}),
				text(
					NodeText::Spans(vec![span(source_label.to_string(), Some("dim"))]),
					{
						let mut props = Props::new();
						props.set_str("role", "omp.advisor.source");
						props
					},
				),
			],
			Some("head"),
		);
		let attribute = sources.len() > 1;
		let mut children = vec![head];
		for (index, entry) in self.notes.iter().enumerate() {
			let spans = note_spans(entry, attribute);
			if self.notes.len() == 1 {
				let mut props = Props::new();
				props.set_str("wrap", "word");
				props.set_str("role", "omp.advisor.note");
				children.push(crate::native::describe::keyed(
					text(NodeText::Spans(spans), props),
					format!("n{index}"),
				));
			} else {
				let entry_rank = rank_of(entry);
				let mut row_props = Props::new();
				row_props.set_str("gap", "sm");
				row_props.set_str("align", "start");
				row_props.set_str("role", "omp.advisor.note");
				row_props.set_tone(rank_tone(entry_rank));
				let mut body_props = Props::new();
				body_props.set_str("wrap", "word");
				body_props.set_bool("grow", true);
				body_props.set_bool("shrink", true);
				children.push(crate::native::describe::keyed(
					node(
						NodeKind::Row,
						row_props,
						vec![
							icon_node(rank_icon(entry_rank), rank_tone(entry_rank)),
							text(NodeText::Spans(spans), body_props),
						],
						None,
					),
					format!("n{index}"),
				));
			}
		}
		let mut props = Props::new();
		props.set_str("role", "omp.advisor");
		props.set_tone(rank_tone(rank));
		let collapsible = self.notes.len() > COLLAPSED_NOTES;
		props.set_bool("collapsible", collapsible);
		if collapsible {
			props.set_bool("collapsed", !expanded);
			props.set_str("preview", "auto");
		}
		card(props, children)
	}
}

fn icon_node(name: &str, tone: Tone) -> NativeNode {
	let mut props = Props::new();
	props.set_str("name", name);
	props.set_tone(tone);
	node(NodeKind::Icon, props, Vec::new(), None)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::native::describe::NativeDescribe;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};
	use crate::theme::theme_class::ColorMode;

	fn theme() -> Theme {
		create_theme(
			builtin_theme_json("dark").unwrap(),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.unwrap()
	}

	fn note(text: &str, severity: Option<AdvisorSeverity>) -> AdvisorNote {
		AdvisorNote {
			note: text.to_string(),
			severity,
			advisor: Some("alice".to_string()),
			turns_ago: Some(2),
			..AdvisorNote::default()
		}
	}

	fn plain(rows: &[String]) -> String {
		rows.iter()
			.map(|row| crate::ansi_scan::strip_ansi(row))
			.collect::<Vec<_>>()
			.join("\n")
	}

	#[test]
	fn severity_ranks_and_colors_line_up() {
		assert_eq!(rank_of(&note("a", Some(AdvisorSeverity::Blocker))), "blocker");
		assert_eq!(rank_of(&note("a", None)), "note");
		assert_eq!(severity_color(Some(AdvisorSeverity::Blocker)), ToolUIColor::Error);
		assert_eq!(severity_color(None), ToolUIColor::Muted);
		assert_eq!(severity_token(Some(AdvisorSeverity::Concern)), "warning");
	}

	#[test]
	fn wrap_varying_keeps_indent_off_continuations() {
		let rows = wrap_varying("bbbbbbbbbbbbbbbbbbbbbb", 6, 20);
		assert!(rows.len() > 1);
		assert_eq!(visible_width(&rows[0]), 6);
		assert!(rows[1].len() > 6, "continuation wraps at the wider width");
	}

	#[test]
	fn notes_render_badge_age_and_source() {
		let rows = render_advisor_note(
			&note("prefer a guard clause", Some(AdvisorSeverity::Concern)),
			60,
			&theme(),
		);
		assert_eq!(rows.len(), 1);
		let text = plain(&rows);
		assert!(text.contains("concern"), "{text}");
		assert!(text.contains("T-2"), "{text}");
		assert!(text.contains("[alice]"), "{text}");
		assert!(text.contains("prefer a guard clause"), "{text}");
	}

	#[test]
	fn card_folds_beyond_three_notes_and_toggles_locally() {
		let details = AdvisorMessageDetails {
			notes: (0..5)
				.map(|i| note(&format!("note {i}"), Some(AdvisorSeverity::Nit)))
				.collect(),
		};
		let mut card = AdvisorMessageCard::new(Some(&details), Box::new(|| false), theme());
		let collapsed = plain(&card.render(80));
		assert!(collapsed.contains("note 2"), "{collapsed}");
		assert!(!collapsed.contains("note 3"), "{collapsed}");
		assert!(collapsed.contains("+2 more notes"), "{collapsed}");
		assert!(collapsed.contains("5 notes"), "{collapsed}");

		card.toggle(true);
		assert!(card.expanded());
		let expanded = plain(&card.render(80));
		assert!(expanded.contains("note 4"), "{expanded}");
		assert!(!expanded.contains("more notes"), "{expanded}");
	}

	#[test]
	fn tally_orders_worst_first_and_card_picks_the_blocker() {
		let notes = vec![
			note("a", Some(AdvisorSeverity::Nit)),
			note("b", Some(AdvisorSeverity::Blocker)),
			note("c", Some(AdvisorSeverity::Nit)),
		];
		let tally = tally_spans(&notes)
			.into_iter()
			.map(|s| s.t)
			.collect::<Vec<_>>()
			.join("");
		assert!(tally.starts_with("1 blocker"), "{tally}");
		assert!(tally.contains("2 nits"), "{tally}");
		let details = AdvisorMessageDetails { notes };
		let card = AdvisorMessageCard::new(Some(&details), Box::new(|| false), theme());
		assert_eq!(card.worst_rank(), "blocker");
		let described = card.describe_native(&DescribeContext::new(80));
		assert_eq!(described.props.get_str("role"), Some("omp.advisor"));
		assert_eq!(described.props.get_bool("collapsible"), Some(false));
	}
}
