//! Job widget view/state data models and plain summary helpers.
//!
 //! Reference: `l/src/tools/job-widget.ts`.
 //!
 //! Deviations:
//! - True TUI chart painting (ANSI sparklines/progress bars/tiles/hovercards/
//!   interactive composition) requires dedicated chart rendering components not
//!   present; structural/text summaries and block listing are provided.
//! - Themed string helpers take `&Theme`; native describe emits token-styled
//!   nodes for host theme-aware rendering.
use crate::components::sparkline::{fit_spark_values, spark_glyph};
use crate::native::describe::{col, compact, elapsed, keyed, node, row, span as nspan, stable_key, text as ntext};
use crate::native::node::{NodeKind, NodeText, Props, Span, Tone};
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, Ellipsis};
use regex::Regex;
use std::sync::LazyLock;

/// Tiles the HUD hover card shows (two rows of two) before it counts the rest.
const HOVERCARD_JOBS: usize = 4;
/// Keys a tile's bars block lists before it counts the rest.
const TILE_BARS: usize = 3;
/// Cells of a stage shown as a tile's body: two or three lines of a tile.
const TILE_LEAD_CELLS: usize = 120;
/// ANSI widgets stop growing here so wide terminals keep bars and labels together.
const ANSI_MAX_WIDTH: usize = 80;
const ANSI_LABEL_MIN: usize = 4;
const ANSI_LABEL_MAX: usize = 16;
const ANSI_BAR_MAX: usize = 32;
/// Sparkline cells below which the mark labels give way to the series.
const ANSI_SPARK_MIN: usize = 12;
/// Lines a grid's labelled cells may wrap onto before it falls back to bare cells.
const GRID_LINES: usize = 3;
const INDENT: &str = "  ";
const GAP: &str = "  ";
const DEFAULT_TONE: &str = "accent";

/// A figure's leading number and the unit after it: `1.10k req/s`, `7/9 trials`, `68%`.
static FIGURE_NUMBER_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^([-+]?(?:\d[\d,]*(?:\.\d+)?|\.\d+)[kKMGT]?%?)(?:\s+|(?=\/)|$)(.*)$")
		.expect("valid figure regex")
});

/// The omp theme colour an ANSI renderer draws a TSP tone with.
fn tone_token(tone: &str) -> &'static str {
	match tone {
		"neutral" => "text",
		"accent" | "info" => "accent",
		"success" => "success",
		"warning" | "pending" => "warning",
		"error" => "error",
		"muted" => "muted",
		"user" => "customMessageLabel",
		_ => "muted",
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobWidgetState {
	/// Cheap model authoring spec.
	Authoring,
	/// Spec-driven live view.
	Live,
	/// Fallback (no spec/model/data).
	Fallback,
}

/// Trend tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrendTone {
	Success,
	Error,
	Muted,
}

impl TrendTone {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Success => "success",
			Self::Error => "error",
			Self::Muted => "muted",
		}
	}
}

/// A formatted number with movement direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobWidgetTrend {
	/// Text (e.g. `▲ 15%`).
	pub text: String,
	/// Tone.
	pub tone: TrendTone,
}

/// Headline value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobWidgetHeadline {
	/// Formatted value with unit.
	pub text: String,
	/// Label.
	pub label: Option<String>,
	/// Trend.
	pub trend: Option<JobWidgetTrend>,
}

/// Spark reference mark.
#[derive(Clone, Debug, PartialEq)]
pub struct JobWidgetMark {
	pub value: f64,
	pub label: String,
	pub tone: Option<String>,
}

/// Spark block.
#[derive(Clone, Debug, PartialEq)]
pub struct JobWidgetSparkBlock {
	pub label: String,
	pub values: Vec<f64>,
	pub min: f64,
	pub max: f64,
	pub marks: Vec<JobWidgetMark>,
	pub last: String,
	pub tips: Vec<String>,
	pub tone: Option<String>,
}

/// Bar item.
#[derive(Clone, Debug, PartialEq)]
pub struct JobWidgetBarItem {
	pub label: String,
	pub value: f64,
	pub text: String,
	pub tone: Option<String>,
}

/// Bars block.
#[derive(Clone, Debug, PartialEq)]
pub struct JobWidgetBarsBlock {
	pub label: String,
	pub items: Vec<JobWidgetBarItem>,
	pub max: f64,
}

/// Meter block.
#[derive(Clone, Debug, PartialEq)]
pub struct JobWidgetMeterBlock {
	pub label: String,
	pub value: f64,
	pub text: String,
	pub tone: Option<String>,
}

/// Stat item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobWidgetStatItem {
	pub label: String,
	pub text: String,
	pub tone: Option<String>,
	pub trend: Option<JobWidgetTrend>,
}

/// Stat block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobWidgetStatBlock {
	pub items: Vec<JobWidgetStatItem>,
}

/// Grid item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobWidgetGridItem {
	pub label: String,
	pub text: String,
	pub tone: String,
}

/// Grid block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobWidgetGridBlock {
	pub label: String,
	pub items: Vec<JobWidgetGridItem>,
}

/// Widget block variants.
#[derive(Clone, Debug, PartialEq)]
pub enum JobWidgetBlock {
	Spark(JobWidgetSparkBlock),
	Bars(JobWidgetBarsBlock),
	Meter(JobWidgetMeterBlock),
	Stat(JobWidgetStatBlock),
	Grid(JobWidgetGridBlock),
	Text(String),
}

/// Everything needed to draw one job's widget.
#[derive(Clone, Debug, PartialEq)]
pub struct JobWidgetView {
	pub title: String,
	pub headline: Option<JobWidgetHeadline>,
	pub stage: Option<String>,
	pub blocks: Vec<JobWidgetBlock>,
	pub state: JobWidgetState,
	pub updated_at: i64,
}

/// HUD/status glance.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobWidgetGlance {
	pub text: String,
	pub label: Option<String>,
	pub trend: Option<JobWidgetTrend>,
}

/// Running job entry with widget.
#[derive(Clone, Debug, PartialEq)]
pub struct JobWidgetEntry {
	pub id: String,
	pub r#type: String, // bash/task/eval
	pub view: JobWidgetView,
}

/// Where a tiled job stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobTileStatus {
	Running,
	Completed,
	Failed,
	Cancelled,
}

impl JobTileStatus {
	/// Wire name.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Running => "running",
			Self::Completed => "completed",
			Self::Failed => "failed",
			Self::Cancelled => "cancelled",
		}
	}

	/// Status glyph.
	pub fn glyph(self) -> &'static str {
		match self {
			Self::Running => "●",
			Self::Completed => "✓",
			Self::Failed => "✕",
			Self::Cancelled => "■",
		}
	}

	/// Status tone.
	pub fn tone(self) -> &'static str {
		match self {
			Self::Running => "accent",
			Self::Completed => "success",
			Self::Failed => "error",
			Self::Cancelled => "warning",
		}
	}

	/// TSP card tone.
	pub fn card_tone(self) -> Tone {
		match self {
			Self::Running => Tone::Accent,
			Self::Completed => Tone::Success,
			Self::Failed => Tone::Error,
			Self::Cancelled => Tone::Warning,
		}
	}
}

/// What kind of job a tile shows: a subagent (`task`) goes by its name, any other job by what it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobTileType {
	Bash,
	Task,
	Eval,
}

impl JobTileType {
	/// Wire name.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Bash => "bash",
			Self::Task => "task",
			Self::Eval => "eval",
		}
	}

	/// Parse a wire name (`bash`/`task`/`eval`).
	pub fn from_wire(value: &str) -> Self {
		match value {
			"task" => Self::Task,
			"eval" => Self::Eval,
			_ => Self::Bash,
		}
	}
}

/// One job as a tile draws it: who it is, how long it has run, and its widget when it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct JobTile {
	/// Job id.
	pub id: String,
	/// Job kind.
	pub r#type: JobTileType,
	/// Where the job stands.
	pub status: JobTileStatus,
	/// The job's label (a command): the title of a job that is no subagent, before a widget names it.
	pub label: String,
	/// How long the job has run; a tile without it shows no clock.
	pub duration_ms: Option<f64>,
	/// Whether the job is live (its dot pulses, its clock runs); settled and sealed tiles freeze.
	pub live: bool,
	/// The job's widget; only a live one draws figures and charts.
	pub widget: Option<JobWidgetView>,
	/// A task's own completion estimate (0–100), drawn as a ring.
	pub completion_percent: Option<f64>,
	/// What the job is doing now, when its widget names nothing.
	pub stage: Option<String>,
	/// Process exit code.
	pub exit_code: Option<i64>,
}

/// Options for [`render_job_tile_lines`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobTileLineOptions {
	/// Opens the title line: the caller's status icon and type badge.
	pub lead: String,
	/// Closes the title line at the right, after the figure: the duration.
	pub meta: String,
	/// Every block rather than the primary one alone (an expanded card).
	pub full: bool,
}

fn plain_line(s: &str) -> String {
	s.replace(['\n', '\r', '\t'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn headline_glance(view: &JobWidgetView) -> Option<JobWidgetGlance> {
	let h = view.headline.as_ref()?;
	let text = plain_line(&h.text);
	if text.is_empty() {
		return None;
	}
	let label = h.label.as_ref().map(|l| plain_line(l)).filter(|s| !s.is_empty());
	Some(JobWidgetGlance {
		text,
		label,
		trend: h.trend.clone(),
	})
}

fn meter_whole(headline: &JobWidgetGlance, view: &JobWidgetView) -> Option<String> {
	for b in &view.blocks {
		if let JobWidgetBlock::Meter(m) = b {
			let whole = plain_line(&m.text);
			let ht = headline.text.trim();
			if whole.starts_with(ht) {
				let rest = &whole[ht.len()..];
				if rest.trim_start().starts_with('/') {
					return Some(whole);
				}
			}
		}
	}
	None
}

/// What one glance at the widget reads.
pub fn job_widget_glance(view: &JobWidgetView) -> Option<JobWidgetGlance> {
	if let Some(mut h) = headline_glance(view) {
		if let Some(whole) = meter_whole(&h, view) {
			h.text = whole;
		}
		return Some(h);
	}
	for b in &view.blocks {
		match b {
			JobWidgetBlock::Spark(s) if !s.values.is_empty() && !plain_line(&s.last).is_empty() => {
				return Some(JobWidgetGlance {
					text: plain_line(&s.last),
					label: Some(plain_line(&s.label)).filter(|s| !s.is_empty()),
					trend: None,
				});
			}
			JobWidgetBlock::Meter(m) if !plain_line(&m.text).is_empty() => {
				return Some(JobWidgetGlance {
					text: plain_line(&m.text),
					label: Some(plain_line(&m.label)).filter(|s| !s.is_empty()),
					trend: None,
				});
			}
			_ => {}
		}
	}
	None
}

/// One plain-line summary: `title — label text trend`.
pub fn job_widget_summary(view: &JobWidgetView) -> String {
	let title = plain_line(&view.title);
	let Some(glance) = job_widget_glance(view) else {
		return title;
	};
	let mut figure_parts = Vec::new();
	if let Some(label) = &glance.label {
		figure_parts.push(label.clone());
	}
	figure_parts.push(glance.text.clone());
	if let Some(trend) = &glance.trend {
		let t = plain_line(&trend.text);
		if !t.is_empty() {
			figure_parts.push(t);
		}
	}
	let figure = figure_parts.join(" ");
	if title.is_empty() {
		figure
	} else {
		format!("{title} — {figure}")
	}
}

/// Latest live widget entry by `updated_at`.
pub fn latest_job_widget(entries: &[JobWidgetEntry]) -> Option<&JobWidgetEntry> {
	entries
		.iter()
		.filter(|e| e.view.state == JobWidgetState::Live)
		.max_by_key(|e| e.view.updated_at)
}

pub fn render_job_widget_lines(view: &JobWidgetView) -> Vec<String> {
	let mut lines = Vec::new();
	let mut head = plain_line(&view.title);
	if let Some(stage) = &view.stage { if !stage.trim().is_empty() { head.push_str(" · "); head.push_str(&plain_line(stage)); } }
	if let Some(g) = job_widget_glance(view) {
		head.push_str(" — ");
		head.push_str(&plain_line(&g.text));
		if let Some(t) = &g.trend { head.push_str(&format!(" {}", plain_line(&t.text))); }
	}
	lines.push(head);
	for b in &view.blocks {
		match b {
			JobWidgetBlock::Spark(s) => lines.push(format!("spark: {} points", s.values.len())),
			JobWidgetBlock::Bars(bk) => lines.push(format!("bars: {}", bk.items.len())),
			JobWidgetBlock::Meter(m) => lines.push(format!("{} {}", plain_line(&m.label), plain_line(&m.text))),
			JobWidgetBlock::Stat(s) => lines.push(s.items.iter().map(|i| format!("{} {}", plain_line(&i.label), plain_line(&i.text))).collect::<Vec<_>>().join(" · ")),
			JobWidgetBlock::Grid(g) => lines.push(format!("grid: {} items", g.items.len())),
			JobWidgetBlock::Text(t) => lines.push(plain_line(t)),
		}
	}
	lines
}

/// A tile as terminal lines within `width`: the lead, name and id with the
/// figure and meta at the right, then a live widget's primary block (every
/// block when `full`) and dim lines of stats and stage, capped at 80 cells.
pub fn render_job_tile_lines(
	tile: &JobTile,
	width: usize,
	theme: &crate::theme::Theme,
	options: &JobTileLineOptions,
) -> Vec<String> {
	let w = width.min(ANSI_MAX_WIDTH);
	if w == 0 {
		return Vec::new();
	}
	let view = tile.widget.as_ref().filter(|view| view.state == JobWidgetState::Live);
	let glance = view.and_then(job_widget_glance);
	let title = tile_title(tile);
	let id = plain_line(&tile.id);
	let mut left = options.lead.clone();
	left.push_str(&theme.bold(&title));
	if id != title {
		left.push(' ');
		let dim_id = theme.fg("dim", &id).unwrap_or_else(|_| id.clone());
		left.push_str(&dim_id);
	}
	let mut right_parts = Vec::new();
	if let Some(glance) = glance.as_ref() {
		right_parts.push(ansi_glance(glance, theme));
	}
	if !options.meta.is_empty() {
		right_parts.push(options.meta.clone());
	}
	let right = right_parts.join(GAP);
	let mut lines = vec![ansi_head_row(&left, &right, w)];

	let shown: Vec<&JobWidgetBlock> = match view {
		Some(view) if options.full => view.blocks.iter().collect(),
		Some(view) => view.blocks.iter().take(1).collect(),
		None => Vec::new(),
	};
	let chart_blocks: Vec<&JobWidgetBlock> = shown
		.into_iter()
		.filter(|block| !matches!(block, JobWidgetBlock::Stat(_) | JobWidgetBlock::Text(_)))
		.collect();
	let label_width = ansi_label_width(&chart_blocks, w);
	for block in chart_blocks {
		lines.extend(render_block_lines(block, w, label_width, theme));
	}

	let stage = plain_line(
		tile.widget
			.as_ref()
			.and_then(|view| view.stage.as_deref())
			.or(tile.stage.as_deref())
			.unwrap_or(""),
	);
	if let Some(view) = view {
		for block in &view.blocks {
			match block {
				JobWidgetBlock::Stat(_) => lines.extend(render_block_lines(block, w, 0, theme)),
				JobWidgetBlock::Text(text) if plain_line(text) != stage => {
					lines.extend(render_block_lines(block, w, 0, theme));
				}
				_ => {}
			}
		}
	}
	if !stage.is_empty() {
		let dim_stage = theme.fg("muted", &stage).unwrap_or_else(|_| stage.clone());
		lines.push(format!("{INDENT}{dim_stage}"));
	}
	lines
		.into_iter()
		.map(|line| truncate_to_width(&line, w, Ellipsis::Unicode))
		.collect()
}

/// `label` cut to fit with `right` flush at the right edge; `right` gives way
/// when `left` would keep under 8 cells.
fn ansi_head_row(left: &str, right: &str, width: usize) -> String {
	if right.is_empty() {
		return truncate_to_width(left, width, Ellipsis::Unicode);
	}
	let room = width.saturating_sub(visible_width(right) + GAP.len());
	if room < visible_width(left).min(8) {
		return truncate_to_width(left, width, Ellipsis::Unicode);
	}
	let head = truncate_to_width(left, room, Ellipsis::Unicode);
	let padding = width
		.saturating_sub(visible_width(&head) + visible_width(right))
		.max(GAP.len());
		format!("{head}{}{right}", " ".repeat(padding))
}

fn ansi_glance(glance: &JobWidgetGlance, theme: &crate::theme::Theme) -> String {
	let mut out = theme.bold(&glance.text);
	if let Some(label) = glance.label.as_deref().filter(|label| !label.is_empty()) {
		let muted = theme.fg("muted", label).unwrap_or_else(|_| label.to_string());
		out = format!("{muted} {out}");
	}
	if let Some(trend) = &glance.trend {
		let tone = trend.tone.as_str();
		let text = theme.fg(tone, &plain_line(&trend.text)).unwrap_or_else(|_| plain_line(&trend.text));
		out.push(' ');
		out.push_str(&text);
	}
	out
}

/// The indent and the padded label column that open a block row.
fn ansi_lead(label: &str, label_width: usize, theme: &crate::theme::Theme) -> String {
	if label_width == 0 {
		return INDENT.to_string();
	}
	let truncated = truncate_to_width(&plain_line(label), label_width, Ellipsis::Unicode);
	let padded = pad_to_width(&truncated, label_width);
	let muted = theme.fg("muted", &padded).unwrap_or(padded);
	format!("{INDENT}{muted}{GAP}")
}

/// One label column for every block: the widest label, within 4–16 cells and a
/// third of the width.
fn ansi_label_width(blocks: &[&JobWidgetBlock], width: usize) -> usize {
	let mut widest = 0usize;
	for block in blocks {
		match block {
			JobWidgetBlock::Spark(block) | JobWidgetBlock::Meter(block) => {
				widest = widest.max(visible_width(&plain_line(&block.label)));
			}
			JobWidgetBlock::Grid(block) => {
				widest = widest.max(visible_width(&plain_line(&block.label)));
			}
			JobWidgetBlock::Bars(block) => {
				for item in &block.items {
					widest = widest.max(visible_width(&plain_line(&item.label)));
				}
			}
			_ => {}
		}
	}
	if widest == 0 {
		return 0;
	}
	let cap = ANSI_LABEL_MAX.min((width.saturating_sub(INDENT.len())) / 3);
	widest.clamp(ANSI_LABEL_MIN, cap).max(0)
}

fn render_block_lines(
	block: &JobWidgetBlock,
	width: usize,
	label_width: usize,
	theme: &crate::theme::Theme,
) -> Vec<String> {
	match block {
		JobWidgetBlock::Spark(block) => vec![render_spark_lines(block, width, label_width, theme)],
		JobWidgetBlock::Bars(block) => {
			let mut lines = vec![format!(
				"{INDENT}{}",
				theme.fg("dim", &plain_line(&block.label)).unwrap_or_else(|_| plain_line(&block.label))
			)];
			if block.items.is_empty() {
				let waiting = theme.fg("dim", "waiting for data").unwrap_or_else(|_| "waiting for data".into());
				lines.push(format!("{INDENT}{waiting}"));
			}
			let texts: Vec<String> = block.items.iter().map(|item| plain_line(&item.text)).collect();
			let text_width = texts.iter().map(|text| visible_width(text)).max().unwrap_or(0);
			for (index, item) in block.items.iter().enumerate() {
				let lead = ansi_lead(&item.label, label_width, theme);
				let value = texts[index].clone();
				let bar_room = width
					.saturating_sub(visible_width(&lead) + GAP.len() + text_width);
				let bar = ansi_bar(fraction(item.value, block.max), bar_room, item.tone.as_deref(), theme);
				let padding = " ".repeat(text_width.saturating_sub(visible_width(&value)));
				lines.push(format!("{lead}{bar}{GAP}{padding}{value}"));
			}
			lines
		}
		JobWidgetBlock::Meter(block) => {
			let lead = ansi_lead(&block.label, label_width, theme);
			let value = plain_line(&block.text);
			let bar_room = width
				.saturating_sub(visible_width(&lead) + GAP.len() + visible_width(&value));
			let bar = ansi_bar(fraction(block.value, 1.0), bar_room, block.tone.as_deref(), theme);
			let bold = theme.bold(&value);
			vec![format!("{lead}{bar}{GAP}{bold}")]
		}
		JobWidgetBlock::Stat(block) => {
			let separator = theme.fg("dim", " · ").unwrap_or_else(|_| " · ".to_string());
			let items: Vec<String> = block
				.items
				.iter()
				.map(|item| {
					let color = item.tone.as_deref().map(tone_token).unwrap_or("toolOutput");
					let dim_label = theme.fg("dim", &plain_line(&item.label)).unwrap_or_else(|_| plain_line(&item.label));
					let colored = theme.fg(color, &plain_line(&item.text)).unwrap_or_else(|_| plain_line(&item.text));
					match &item.trend {
						Some(trend) => {
							let text = theme
								.fg(trend.tone.as_str(), &plain_line(&trend.text))
								.unwrap_or_else(|_| plain_line(&trend.text));
							format!("{dim_label} {colored} {text}")
						}
						None => format!("{dim_label} {colored}"),
					}
				})
				.collect();
			vec![format!("{INDENT}{}", items.join(&separator))]
		}
		JobWidgetBlock::Grid(block) => render_grid_lines(block, width, label_width, theme),
		JobWidgetBlock::Text(text) => {
			let dim = theme.fg("dim", &plain_line(text)).unwrap_or_else(|_| plain_line(text));
			vec![format!("{INDENT}{dim}")]
		}
	}
}

fn render_spark_lines(
	block: &JobWidgetSparkBlock,
	width: usize,
	label_width: usize,
	theme: &crate::theme::Theme,
) -> String {
	let lead = ansi_lead(&block.label, label_width, theme);
	if block.values.is_empty() {
		let waiting = theme.fg("dim", "waiting for data").unwrap_or_else(|_| "waiting for data".into());
		return format!("{lead}{waiting}");
	}
	let last = theme.bold(&plain_line(&block.last));
	let marks: Vec<String> = block
		.marks
		.iter()
		.map(|mark| {
			let tone = tone_token(mark.tone.as_deref().unwrap_or("muted"));
			let dash = theme.fg(tone, "┄").unwrap_or_else(|_| "┄".to_string());
			let label = theme.fg("dim", &plain_line(&mark.label)).unwrap_or_else(|_| plain_line(&mark.label));
			format!("{dash} {label}")
		})
		.collect();
	let room = width.saturating_sub(visible_width(&lead) + GAP.len());
	let mut tail = std::iter::once(last.clone()).chain(marks.iter().cloned()).collect::<Vec<_>>().join(GAP);
	if !block.marks.is_empty() && room.saturating_sub(visible_width(&tail)) < ANSI_SPARK_MIN.min(block.values.len()) {
		tail = last;
	}
	let cells = room.saturating_sub(visible_width(&tail)).max(1);
	let spark = ansi_spark(block, &fit_spark_values(&block.values, cells), theme);
	format!("{lead}{spark}{GAP}{tail}")
}

fn ansi_spark(block: &JobWidgetSparkBlock, values: &[f64], theme: &crate::theme::Theme) -> String {
	let color = tone_token(block.tone.as_deref().unwrap_or(DEFAULT_TONE));
	let min = block.min;
	let max = block.max;
	let span = max - min;
	let mut out = String::new();
	for value in values {
		if !value.is_finite() {
			out.push(' ');
			continue;
		}
		let fraction = if span > 0.0 {
			(value - min) / span
		} else if value > min {
			1.0
		} else {
			0.0
		};
		let glyph = spark_glyph(fraction);
		match theme.fg(color, glyph) {
			Ok(styled) => out.push_str(&styled),
			Err(_) => out.push_str(glyph),
		}
	}
	out
}

/// A `█…░` bar `room` cells wide at most (capped at 32), filled in the item's tone.
fn ansi_bar(value: f64, room: usize, tone: Option<&str>, theme: &crate::theme::Theme) -> String {
	let color = tone_token(tone.unwrap_or(DEFAULT_TONE));
	let bar_width = room.min(ANSI_BAR_MAX);
	let fraction = fraction(value, 1.0);
	let filled = ((fraction * bar_width as f64).round() as usize).min(bar_width);
	let filled_raw = "█".repeat(filled);
	let empty_raw = "░".repeat(bar_width - filled);
	let filled = theme.fg(color, &filled_raw).unwrap_or(filled_raw);
	let empty = theme.fg("dim", &empty_raw).unwrap_or(empty_raw);
	format!("{filled}{empty}")
}

fn render_grid_lines(
	block: &JobWidgetGridBlock,
	width: usize,
	label_width: usize,
	theme: &crate::theme::Theme,
) -> Vec<String> {
	let lead = ansi_lead(&block.label, label_width, theme);
	if block.items.is_empty() {
		let waiting = theme.fg("dim", "waiting for data").unwrap_or_else(|_| "waiting for data".into());
		return vec![format!("{lead}{waiting}")];
	}
	let lead_width = visible_width(&lead);
	let room = width.saturating_sub(lead_width);
	let glyph = |tone: &str| -> String {
		let glyph = if matches!(tone, "pending" | "muted" | "neutral") { "◌" } else { "●" };
		theme.fg(tone_token(tone), glyph).unwrap_or_else(|_| glyph.to_string())
	};
	let mut rows: Vec<Vec<String>> = vec![Vec::new()];
	let mut used = 0usize;
	for item in &block.items {
		let name = plain_line(&item.label);
		let cell_width = 2 + visible_width(&name);
		let needed = if rows.last().is_some_and(|row| row.is_empty()) {
			cell_width
		} else {
			used + GAP.len() + cell_width
		};
		let current = rows.last().is_some_and(|row| !row.is_empty());
		if current && needed > room {
			rows.push(Vec::new());
			used = cell_width;
		} else {
			used = needed;
		}
		let colored_name = theme.fg("toolOutput", &name).unwrap_or(name.clone());
		rows.last_mut()
			.expect("row exists")
			.push(format!("{} {colored_name}", glyph(&item.tone)));
	}
	let fits = rows.len() <= GRID_LINES
		&& rows
			.iter()
			.all(|cells| visible_width(&cells.join(GAP)) <= room);
	if fits {
		let pad = " ".repeat(lead_width);
		return rows
			.into_iter()
			.enumerate()
			.map(|(index, cells)| {
				let prefix = if index == 0 { lead.clone() } else { pad.clone() };
				format!("{prefix}{}", cells.join(GAP))
			})
			.collect();
	}
	let bare: Vec<String> = block.items.iter().map(|item| glyph(&item.tone)).collect();
	vec![format!("{lead}{}", bare.join(" "))]
}

/// A job's whole widget as a native subtree (role `omp.job.widget`): a head row
/// over every block.
pub fn describe_job_widget(view: &JobWidgetView) -> crate::native::node::NativeNode {
	let mut props = Props::new();
	props.set_str("role", "omp.job.widget");
	props.set_str("gap", "xs");
	let mut children = vec![keyed(describe_head(view), "head")];
	for (index, block) in view.blocks.iter().enumerate() {
		children.push(keyed(describe_block(block), format!("{index}-{}", block_kind(block))));
	}
	col(children, props)
}

fn block_kind(block: &JobWidgetBlock) -> &'static str {
	match block {
		JobWidgetBlock::Spark(_) => "spark",
		JobWidgetBlock::Bars(_) => "bars",
		JobWidgetBlock::Meter(_) => "meter",
		JobWidgetBlock::Stat(_) => "stat",
		JobWidgetBlock::Grid(_) => "grid",
		JobWidgetBlock::Text(_) => "text",
	}
}

/// Spans of a glance: muted label, strong number, toned trend.
fn job_widget_glance_spans(glance: &JobWidgetGlance) -> Vec<Span> {
	let mut spans = Vec::new();
	if let Some(label) = glance.label.as_deref().filter(|s| !s.is_empty()) {
		spans.push(nspan(format!("{label} "), Some("muted")));
	}
	spans.push(nspan(plain_line(&glance.text), Some("num strong")));
	if let Some(trend) = &glance.trend {
		spans.push(nspan(format!(" {}", plain_line(&trend.text)), Some(trend.tone.as_str())));
	}
	spans
}

fn describe_head(view: &JobWidgetView) -> crate::native::node::NativeNode {
	let stage = view.stage.as_deref().map(plain_line).filter(|s| !s.is_empty()).unwrap_or("");
	let headline = headline_glance(view);
	let mut title_props = Props::new();
	title_props.set_str("role", "omp.job.widget.title");
	title_props.set_str("wrap", "none");
	title_props.set_str("truncate", "end");
	if stage.is_empty() {
		title_props.set_bool("grow", true);
	}
	let mut children = vec![ntext(
		NodeText::Spans(vec![nspan(plain_line(&view.title), Some("strong"))]),
		title_props,
	)];
	if !stage.is_empty() {
		let mut stage_props = Props::new();
		stage_props.set_str("role", "omp.job.widget.stage");
		stage_props.set_str("wrap", "none");
		stage_props.set_str("truncate", "end");
		stage_props.set_bool("grow", true);
		children.push(ntext(NodeText::Spans(vec![nspan(stage, Some("muted"))]), stage_props));
	}
	if let Some(glance) = headline {
		let mut headline_props = Props::new();
		headline_props.set_str("role", "omp.job.widget.headline");
		headline_props.set_str("wrap", "none");
		children.push(ntext(NodeText::Spans(job_widget_glance_spans(&glance)), headline_props));
	}
	let mut row_props = Props::new();
	row_props.set_str("role", "omp.job.widget.head");
	row_props.set_str("gap", "sm");
	row_props.set_str("align", "baseline");
	row(children, row_props)
}

fn labelled(label: &str, content: crate::native::node::NativeNode, role: &str) -> crate::native::node::NativeNode {
	let mut label_props = Props::new();
	label_props.set_str("wrap", "none");
	label_props.set_str("truncate", "end");
	label_props.set_str("min", "12ch");
	label_props.set_str("max", "12ch");
	let mut props = Props::new();
	props.set_str("role", role);
	props.set_str("gap", "sm");
	props.set_str("align", "center");
	row(
		vec![ntext(NodeText::Spans(vec![nspan(plain_line(label), Some("muted"))]), label_props), content],
		props,
	)
}

fn waiting() -> crate::native::node::NativeNode {
	let mut props = Props::new();
	props.set_str("wrap", "none");
	ntext(NodeText::Spans(vec![nspan("waiting for data", Some("dim"))]), props)
}

fn fraction(value: f64, max: f64) -> f64 {
	if !(max > 0.0) || !value.is_finite() {
		return 0.0;
	}
	(value / max).clamp(0.0, 1.0)
}

fn meter_node(value: f64, tone: Option<&str>, label: &str) -> crate::native::node::NativeNode {
	let mut props = Props::new();
	props.set_f64("value", fraction(value, 1.0));
	props.set_str("label", plain_line(label));
	props.set_str("size", "sm");
	props.set_bool("grow", true);
	if let Some(tone) = tone {
		props.set_str("tone", tone);
	}
	node(NodeKind::Meter, props, Vec::new(), None)
}

fn spark_chart_props(block: &JobWidgetSparkBlock, role: &str, size: &str, grow: bool) -> Props {
	let mut props = Props::new();
	props.set_str("role", role);
	props.set_str("kind", "bars");
	props.set_str("size", size);
	let series: Vec<serde_json::Value> = block
		.values
		.iter()
		.enumerate()
		.map(|(index, value)| {
			let mut item = serde_json::Map::new();
			item.insert("value".into(), serde_json::json!(value));
			if let Some(tip) = block.tips.get(index).map(plain_line).filter(|tip| !tip.is_empty()) {
				item.insert("title".into(), serde_json::json!(tip));
			}
			serde_json::Value::Object(item)
		})
		.collect();
	props.set_prop("series", serde_json::Value::Array(series));
	props.set_f64("min", block.min);
	props.set_f64("max", block.max);
	let marks: Vec<serde_json::Value> = block
		.marks
		.iter()
		.map(|mark| {
			let mut item = serde_json::Map::new();
			item.insert("value".into(), serde_json::json!(mark.value));
			if let Some(tone) = mark.tone.as_deref().filter(|tone| !tone.is_empty()) {
				item.insert("tone".into(), serde_json::json!(tone));
			}
			let title = plain_line(&mark.label);
			if !title.is_empty() {
				item.insert("title".into(), serde_json::json!(title));
			}
			serde_json::Value::Object(item)
		})
		.collect();
	if !marks.is_empty() {
		props.set_prop("marks", serde_json::Value::Array(marks));
	}
	props.set_str("token", tone_token(block.tone.as_deref().unwrap_or(DEFAULT_TONE)));
	props.set_str("summary", format!("{} {}", plain_line(&block.label), plain_line(&block.last)));
	props.set_bool("grow", grow);
	props
}

fn describe_spark(block: &JobWidgetSparkBlock) -> crate::native::node::NativeNode {
	if block.values.is_empty() {
		return labelled(&block.label, waiting(), "omp.job.widget.spark");
	}
	let label = plain_line(&block.label);
	let last = plain_line(&block.last);
	let mut chart_props = spark_chart_props(block, "omp.job.widget.spark.chart", "md", true);
	chart_props.set_str("role", "omp.job.widget.spark.chart");
	let mut side_children = vec![ntext(
		NodeText::Spans(vec![nspan(last.clone(), Some("num strong"))]),
		Props::new(),
	)];
	if !label.is_empty() && !last.ends_with(&label) {
		let mut label_props = Props::new();
		label_props.set_str("wrap", "none");
		label_props.set_str("truncate", "end");
		side_children.push(ntext(NodeText::Spans(vec![nspan(label, Some("muted"))]), label_props));
	}
	for mark in &block.marks {
		let mut mark_props = Props::new();
		mark_props.set_str("wrap", "none");
		mark_props.set_str("truncate", "end");
		let tone = tone_token(mark.tone.as_deref().unwrap_or("muted"));
		side_children.push(ntext(
			NodeText::Spans(vec![nspan("┄ ", Some(tone)), nspan(plain_line(&mark.label), Some("dim"))]),
			mark_props,
		));
	}
	let mut side_props = Props::new();
	side_props.set_str("role", "omp.job.widget.side");
	side_props.set_str("gap", "none");
	let mut row_props = Props::new();
	row_props.set_str("role", "omp.job.widget.spark");
	row_props.set_str("gap", "sm");
	row_props.set_str("align", "center");
	row(
		vec![
			keyed(node(NodeKind::Chart, chart_props, Vec::new(), None), "chart"),
			keyed(col(side_children, side_props), "side"),
		],
		row_props,
	)
}

fn describe_bars(block: &JobWidgetBarsBlock) -> crate::native::node::NativeNode {
	let rows: Vec<crate::native::node::NativeNode> = block
		.items
		.iter()
		.enumerate()
		.map(|(index, item)| {
			let meter = meter_node(item.value, item.tone.as_deref(), &item.text);
			keyed(labelled(&item.label, meter, "omp.job.widget.bar"), index.to_string())
		})
		.collect();
	let mut caption_props = Props::new();
	caption_props.set_str("role", "omp.job.widget.caption");
	caption_props.set_str("wrap", "none");
	caption_props.set_str("truncate", "end");
	let mut children = vec![ntext(NodeText::Spans(vec![nspan(plain_line(&block.label), Some("dim"))]), caption_props)];
	if rows.is_empty() {
		children.push(waiting());
	} else {
		children.extend(rows);
	}
	let mut props = Props::new();
	props.set_str("role", "omp.job.widget.bars");
	props.set_str("gap", "xs");
	col(children, props)
}

fn stat_spans(block: &JobWidgetStatBlock) -> Vec<Span> {
	let mut spans = Vec::new();
	for (index, item) in block.items.iter().enumerate() {
		if index > 0 {
			spans.push(nspan(" · ", Some("dim")));
		}
		spans.push(nspan(format!("{} ", plain_line(&item.label)), Some("dim")));
		let tone = item.tone.as_deref().map(tone_token).unwrap_or("toolOutput");
		spans.push(nspan(plain_line(&item.text), Some(tone)));
		if let Some(trend) = &item.trend {
			spans.push(nspan(format!(" {}", plain_line(&trend.text)), Some(trend.tone.as_str())));
		}
	}
	spans
}

fn describe_block(block: &JobWidgetBlock) -> crate::native::node::NativeNode {
	match block {
		JobWidgetBlock::Spark(block) => describe_spark(block),
		JobWidgetBlock::Bars(block) => describe_bars(block),
		JobWidgetBlock::Meter(block) => {
			let meter = {
				let mut props = Props::new();
				props.set_f64("value", fraction(block.value, 1.0));
				props.set_str("label", plain_line(&block.text));
				props.set_str("size", "sm");
				props.set_bool("grow", true);
				if let Some(tone) = block.tone.as_deref().filter(|tone| !tone.is_empty()) {
					props.set_str("tone", tone);
				}
				node(NodeKind::Meter, props, Vec::new(), None)
			};
			labelled(&block.label, meter, "omp.job.widget.meter")
		}
		JobWidgetBlock::Stat(block) => {
			let mut props = Props::new();
			props.set_str("role", "omp.job.widget.stat");
			props.set_str("wrap", "none");
			props.set_str("truncate", "end");
			ntext(NodeText::Spans(stat_spans(block)), props)
		}
		JobWidgetBlock::Grid(block) => {
			let mut caption_props = Props::new();
			caption_props.set_str("role", "omp.job.widget.caption");
			caption_props.set_str("wrap", "none");
			caption_props.set_str("truncate", "end");
			let mut children = vec![ntext(
				NodeText::Spans(vec![nspan(plain_line(&block.label), Some("dim"))]),
				caption_props,
			)];
			if block.items.is_empty() {
				children.push(waiting());
			} else {
				let badges: Vec<crate::native::node::NativeNode> = block
					.items
					.iter()
					.enumerate()
					.map(|(index, item)| {
						let mut props = Props::new();
						props.set_str("text", plain_line(&item.label));
						props.set_str("tone", item.tone.clone());
						props.set_str("title", plain_line(&item.text));
						node(NodeKind::Badge, props, Vec::new(), Some(index.to_string().as_str()))
					})
					.collect();
				let mut row_props = Props::new();
				row_props.set_str("role", "omp.job.widget.cells");
				row_props.set_str("gap", "xs");
				row_props.set_bool("wrap", true);
				children.push(row(badges, row_props));
			}
			let mut props = Props::new();
			props.set_str("role", "omp.job.widget.grid");
			props.set_str("gap", "xs");
			col(children, props)
		}
		JobWidgetBlock::Text(text) => {
			let mut props = Props::new();
			props.set_str("role", "omp.job.widget.text");
			props.set_str("wrap", "none");
			props.set_str("truncate", "end");
			ntext(NodeText::Spans(vec![nspan(plain_line(text), Some("muted"))]), props)
		}
	}
}

/// Jobs as one frame of equal tiles (role `omp.job.tiles`) that wraps onto more
/// rows when the frame runs out of width.
pub fn describe_job_tiles(tiles: &[JobTile]) -> crate::native::node::NativeNode {
	let children: Vec<crate::native::node::NativeNode> = tiles
		.iter()
		.map(|tile| keyed(describe_job_tile(tile), stable_key(&tile.id)))
		.collect();
	let mut props = Props::new();
	props.set_str("role", "omp.job.tiles");
	props.set_str("gap", "none");
	props.set_bool("wrap", true);
	row(children, props)
}

/// One job as a tile (role `omp.job.tile`).
pub fn describe_job_tile(tile: &JobTile) -> crate::native::node::NativeNode {
	let view = tile.widget.as_ref().filter(|view| view.state == JobWidgetState::Live);
	let glance = view.and_then(job_widget_glance);
	let title = tile_title(tile);
	let id = plain_line(&tile.id);
	let pulse = tile.live && tile.status == JobTileStatus::Running;
	let clock = tile.duration_ms.map(|ms| elapsed(ms, !tile.live));
	let percent = tile.completion_percent.filter(|value| value.is_finite()).map(|value| value.round() as i64);

	let mut dot_span = Span::styled(tile.status.glyph(), tile.status.tone());
	dot_span.fx = pulse.then(|| "pulse".to_string());
	let mut dot_props = Props::new();
	dot_props.set_str("role", "omp.job.tile.dot");
	dot_props.set_str("wrap", "none");
	let mut title_props = Props::new();
	title_props.set_str("role", "omp.job.tile.title");
	title_props.set_str("wrap", "none");
	title_props.set_str("truncate", "end");
	title_props.set_bool("grow", true);
	let mut head_children = vec![
		ntext(NodeText::Spans(vec![dot_span]), dot_props),
		ntext(NodeText::Spans(vec![nspan(title.clone(), None)]), title_props),
	];
	if id != title {
		let mut id_props = Props::new();
		id_props.set_str("role", "omp.job.tile.id");
		id_props.set_str("wrap", "none");
		head_children.push(ntext(NodeText::Spans(vec![nspan(id, Some("dim"))]), id_props));
	}
	if let Some(percent) = percent {
		let mut ring_props = Props::new();
		ring_props.set_str("role", "omp.job.tile.ring");
		ring_props.set_str("style", "ring");
		ring_props.set_str("size", "sm");
		ring_props.set_f64("value", fraction(percent as f64, 100.0));
		ring_props.set_str("label", format!("{percent}%"));
		ring_props.set_str("title", format!("The agent's own completion estimate: {percent}%"));
		head_children.push(keyed(node(NodeKind::Meter, ring_props, Vec::new(), None), "ring"));
	}
	if let Some(clock) = clock {
		head_children.push(keyed(clock, "clock"));
	}
	let mut head_props = Props::new();
	head_props.set_str("role", "omp.job.tile.head");
	head_props.set_str("gap", "xs");
	head_props.set_str("align", "center");
	let head = row(head_children, head_props);

	let figure = glance.map(|glance| {
		let mut props = Props::new();
		props.set_str("role", "omp.job.tile.figure");
		props.set_str("wrap", "none");
		props.set_str("truncate", "end");
		ntext(NodeText::Spans(figure_spans(&glance)), props)
	});
	let visual = view.and_then(|view| view.blocks.first()).and_then(tile_visual);
	let mut children: Vec<crate::native::node::NativeNode> = vec![keyed(head, "head")];
	if let Some(figure) = figure {
		children.push(keyed(figure, "figure"));
	}
	if let Some(visual) = visual {
		children.push(keyed(visual, "visual"));
	}
	let lead = glance.is_none() && visual.is_none();
	for note in tile_notes(tile, view, lead) {
		children.push(note);
	}
	let mut props = Props::new();
	props.set_str("role", "omp.job.tile");
	props.set_tone(tile.status.card_tone());
	props.set_str("gap", "xs");
	col(children, props)
}

/// A subagent's name; any other job's widget title, else its label's first line, else its id.
fn tile_title(tile: &JobTile) -> String {
	if tile.r#type == JobTileType::Task {
		return plain_line(&tile.id);
	}
	let widget_title = tile.widget.as_ref().map(|view| plain_line(&view.title)).unwrap_or_default();
	if !widget_title.is_empty() {
		return widget_title;
	}
	let first_line = tile.label.split('\n').next().unwrap_or("");
	let first_line = plain_line(first_line);
	if !first_line.is_empty() {
		return first_line;
	}
	plain_line(&tile.id)
}

fn figure_spans(glance: &JobWidgetGlance) -> Vec<Span> {
	let (value, unit) = match FIGURE_NUMBER_RE.captures(&glance.text) {
		Some(captures) => (
			captures.get(1).map(|m| m.as_str().to_string()).unwrap_or_else(|| glance.text.clone()),
			captures.get(2).map(|m| m.as_str()).unwrap_or("").replacen('/', "/ ", 1),
		),
		None => (glance.text.clone(), String::new()),
	};
	let unit = if unit.starts_with("/ ") || unit == "/" { unit } else { unit };
	let label = match glance.label.as_deref() {
		Some(label) if !label.is_empty() && !unit.to_lowercase().contains(&label.to_lowercase()) => {
			format!("{label} ")
		}
		_ => String::new(),
	};
	let mut spans = Vec::new();
	if !label.is_empty() {
		spans.push(nspan(label, Some("dim")));
	}
	spans.push(nspan(value, None));
	if !unit.is_empty() {
		spans.push(nspan(format!(" {unit}"), Some("dim")));
	}
	if let Some(trend) = &glance.trend {
		spans.push(nspan(format!(" {}", plain_line(&trend.text)), Some(trend.tone.as_str())));
	}
	spans
}

fn tile_visual(block: Option<&JobWidgetBlock>) -> Option<crate::native::node::NativeNode> {
	match block? {
		JobWidgetBlock::Spark(block) => {
			if block.values.is_empty() {
				return None;
			}
			let props = spark_chart_props(block, "omp.job.tile.chart", "sm", false);
			Some(node(NodeKind::Chart, props, Vec::new(), None))
		}
		JobWidgetBlock::Meter(block) => Some(tile_meter(fraction(block.value, 1.0), block.tone.as_deref())),
		JobWidgetBlock::Bars(block) => {
			let mut rows: Vec<crate::native::node::NativeNode> = block
				.items
				.iter()
				.take(TILE_BARS)
				.enumerate()
				.map(|(index, item)| {
					let mut key_props = Props::new();
					key_props.set_str("role", "omp.job.tile.key");
					key_props.set_str("wrap", "none");
					key_props.set_str("truncate", "end");
					let mut num_props = Props::new();
					num_props.set_str("role", "omp.job.tile.num");
					num_props.set_str("wrap", "none");
					let mut row_props = Props::new();
					row_props.set_str("role", "omp.job.tile.bar");
					row_props.set_str("gap", "sm");
					row_props.set_str("align", "center");
					keyed(
						row(
							vec![
								ntext(NodeText::Spans(vec![nspan(plain_line(&item.label), Some("dim"))]), key_props),
								tile_meter(fraction(item.value, block.max), item.tone.as_deref()),
								ntext(NodeText::Spans(vec![nspan(plain_line(&item.text), None)]), num_props),
							],
							row_props,
						),
						index.to_string(),
					)
				})
				.collect();
			let rest = block.items.len().saturating_sub(TILE_BARS);
			if rest > 0 {
				let mut more_props = Props::new();
				more_props.set_str("role", "omp.job.tile.more");
				more_props.set_str("wrap", "none");
				rows.push(ntext(NodeText::Spans(vec![nspan(format!("+{rest} more"), Some("dim"))]), more_props));
			}
			if rows.is_empty() {
				None
			} else {
				let mut props = Props::new();
				props.set_str("role", "omp.job.tile.bars");
				props.set_str("gap", "xs");
				Some(col(rows, props))
			}
		}
		JobWidgetBlock::Grid(block) => {
			if block.items.is_empty() {
				return None;
			}
			let mut spans = Vec::new();
			for item in &block.items {
				if !spans.is_empty() {
					spans.push(nspan("   ", None));
				}
				spans.push(nspan("●\u{a0}", Some(tone_token(&item.tone))));
				spans.push(nspan(plain_line(&item.label).replace(' ', "\u{a0}"), Some("dim")));
			}
			let mut props = Props::new();
			props.set_str("role", "omp.job.tile.grid");
			props.set_str("wrap", "word");
			Some(ntext(NodeText::Spans(spans), props))
		}
		_ => None,
	}
}

fn tile_meter(value: f64, tone: Option<&str>) -> crate::native::node::NativeNode {
	let mut props = Props::new();
	props.set_str("role", "omp.job.tile.meter");
	props.set_f64("value", fraction(value, 1.0));
	props.set_str("size", "sm");
	if let Some(tone) = tone {
		props.set_str("tone", tone);
	}
	props.set_bool("grow", true);
	node(NodeKind::Meter, props, Vec::new(), None)
}

fn tile_notes(tile: &JobTile, view: Option<&JobWidgetView>, lead: bool) -> Vec<crate::native::node::NativeNode> {
	let stage = plain_line(tile.widget.as_ref().and_then(|view| view.stage.as_deref()).or(tile.stage.as_deref()).unwrap_or(""));
	let failed = match tile.exit_code {
		Some(code) if code != 0 => format!("exit {code}"),
		_ => String::new(),
	};
	let mut notes: Vec<crate::native::node::NativeNode> = Vec::new();
	if let Some(view) = view {
		for block in &view.blocks {
			match block {
				JobWidgetBlock::Stat(stat) if !stat.items.is_empty() => {
					let mut props = Props::new();
					props.set_str("role", "omp.job.tile.stats");
					props.set_str("wrap", "none");
					props.set_str("truncate", "end");
					notes.push(ntext(NodeText::Spans(stat_spans(stat)), props));
				}
				JobWidgetBlock::Text(text) => {
					let text = plain_line(text);
					if !text.is_empty() && text != stage {
						let mut props = Props::new();
						props.set_str("role", "omp.job.tile.note");
						props.set_str("wrap", "none");
						props.set_str("truncate", "end");
						notes.push(ntext(NodeText::Spans(vec![nspan(text, Some("dim"))]), props));
					}
				}
				_ => {}
			}
		}
	}
	if !stage.is_empty() || !failed.is_empty() {
		let mut spans = Vec::new();
		if !failed.is_empty() {
			spans.push(nspan(failed, Some("error")));
		}
		if !failed.is_empty() && !stage.is_empty() {
			spans.push(nspan(" · ", Some("dim")));
		}
		if !stage.is_empty() {
			let text = if lead { truncate_to_width(&stage, TILE_LEAD_CELLS, Ellipsis::Unicode) } else { stage.clone() };
			spans.push(nspan(text, if lead { None } else { Some("dim") }));
		}
		if lead {
			let mut props = Props::new();
			props.set_str("role", "omp.job.tile.lead");
			props.set_str("wrap", "word");
			notes.insert(0, ntext(NodeText::Spans(spans), props));
		} else {
			let mut props = Props::new();
			props.set_str("role", "omp.job.tile.stage");
			props.set_str("wrap", "none");
			props.set_str("truncate", "end");
			notes.push(ntext(NodeText::Spans(spans), props));
		}
	}
	notes.into_iter().enumerate().map(|(index, note)| keyed(note, format!("note-{index}"))).collect()
}

/// The HUD jobs pill's hover card: up to four running jobs as tiles, then a count of the rest.
pub fn describe_job_widgets_hovercard(entries: &[JobWidgetEntry]) -> crate::native::node::NativeNode {
	if entries.is_empty() {
		return describe_job_widget(&JobWidgetView {
			title: "Jobs".into(),
			headline: None,
			stage: None,
			blocks: vec![],
			state: JobWidgetState::Fallback,
			updated_at: 0,
		});
	}
	let shown = entries.len().min(HOVERCARD_JOBS);
	let tiles: Vec<JobTile> = entries[..shown]
		.iter()
		.map(|entry| JobTile {
			id: entry.id.clone(),
			r#type: JobTileType::from_wire(&entry.r#type),
			status: JobTileStatus::Running,
			label: entry.id.clone(),
			duration_ms: None,
			live: true,
			widget: Some(entry.view.clone()),
			completion_percent: None,
			stage: None,
			exit_code: None,
		})
		.collect();
	let mut children = vec![describe_job_tiles(&tiles)];
	let rest = entries.len() - shown;
	if rest > 0 {
		let mut props = Props::new();
		props.set_str("role", "omp.jobs.hovercard.more");
		props.set_str("wrap", "none");
		children.push(ntext(
			NodeText::Spans(vec![nspan(
				format!("+{rest} more {}", if rest == 1 { "job" } else { "jobs" }),
				Some("dim"),
			)]),
			props,
		));
	}
	let mut props = Props::new();
	props.set_str("role", "omp.jobs.hovercard");
	props.set_str("gap", "md");
	col(children, props)
}

/// Plain render call/result helpers for widget-bearing tools.
pub fn render_call(title: Option<&str>) -> String { title.map(plain_line).unwrap_or_else(|| "Job".into()) }
pub fn render_result(view: Option<&JobWidgetView>) -> Vec<String> {
	match view { Some(v) => render_job_widget_lines(v), None => vec!["No widget".into()] }
}
pub fn describe_call(title: Option<&str>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let t = title.map(plain_line).unwrap_or_else(|| "Job".into());
	NativeToolView { head: Some(NodeText::Spans(tool_head(&t, &[]))), ..NativeToolView::default() }
}
pub fn describe_result(view: Option<&JobWidgetView>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span};
	use crate::tools::native_view::{note_text, tool_head, NativeToolView};
	let Some(v) = view else { return NativeToolView { head: Some(NodeText::Spans(tool_head("Job", &[]))), body: Some(vec![note_text("No widget", "dim", None)]), ..NativeToolView::default() }; };
	let head = tool_head(&plain_line(&v.title), &[]);
	NativeToolView { head: Some(NodeText::Spans(head)), body: Some(vec![describe_job_widget(v)]), ..NativeToolView::default() }
}

/// Themed render of job widget lines.
pub fn render_job_widget_lines_themed(view: &JobWidgetView, theme: &crate::theme::Theme) -> Vec<String> {
	let mut lines = Vec::new();
	let mut head = theme.fg("accent", &plain_line(&view.title)).unwrap_or_else(|_| plain_line(&view.title));
	if let Some(stage) = &view.stage { if !stage.trim().is_empty() { head.push_str(&theme.fg("dim", &format!(" · {}", plain_line(stage))).unwrap_or_else(|_| format!(" · {}", plain_line(stage)))); } }
	if let Some(g) = job_widget_glance(view) {
		head.push_str(&theme.fg("dim", " — ").unwrap_or_else(|_| " — ".into()));
		head.push_str(&theme.fg("toolOutput", &plain_line(&g.text)).unwrap_or_else(|_| plain_line(&g.text)));
		if let Some(t) = &g.trend { let tone = t.tone.as_str(); head.push(' '); head.push_str(&theme.fg(tone, &plain_line(&t.text)).unwrap_or_else(|_| plain_line(&t.text))); }
	}
	lines.push(head);
	for b in &view.blocks {
		let line = match b {
			JobWidgetBlock::Spark(s) => theme.fg("muted", &format!("spark: {} points", s.values.len())).unwrap_or_else(|_| format!("spark: {} points", s.values.len())),
			JobWidgetBlock::Bars(bk) => theme.fg("muted", &format!("bars: {}", bk.items.len())).unwrap_or_else(|_| format!("bars: {}", bk.items.len())),
			JobWidgetBlock::Meter(m) => format!("{} {}", theme.fg("accent", &plain_line(&m.label)).unwrap_or_else(|_| plain_line(&m.label)), theme.fg("toolOutput", &m.text).unwrap_or_else(|_| m.text.clone())),
			JobWidgetBlock::Stat(s) => s.items.iter().map(|i| format!("{} {}", theme.fg("dim", &plain_line(&i.label)).unwrap_or_else(|_| plain_line(&i.label)), theme.fg("toolOutput", &plain_line(&i.text)).unwrap_or_else(|_| plain_line(&i.text)))).collect::<Vec<_>>().join(&theme.fg("dim", " · ").unwrap_or_else(|_| " · ".into())),
			JobWidgetBlock::Grid(g) => theme.fg("muted", &format!("grid: {} items", g.items.len())).unwrap_or_else(|_| format!("grid: {} items", g.items.len())),
			JobWidgetBlock::Text(t) => theme.fg("toolOutput", &plain_line(t)).unwrap_or_else(|_| plain_line(t)),
		};
		lines.push(line);
	}
	lines
}
pub fn render_result_themed(view: Option<&JobWidgetView>, theme: &crate::theme::Theme) -> Vec<String> {
	match view { Some(v) => render_job_widget_lines_themed(v, theme), None => vec![theme.fg("dim", "No widget").unwrap_or_else(|_| "No widget".into())] }
}
#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn widget_helpers() {
		let v = JobWidgetView { title: "build".into(), headline: None, stage: None, blocks: vec![JobWidgetBlock::Text("ok".into())], state: JobWidgetState::Live, updated_at: 1 };
		let lines = render_job_widget_lines(&v);
		assert!(!lines.is_empty());
		let _ = describe_job_widget(&v);
		let r = render_result(Some(&v));
		assert!(r[0].contains("build"));
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	fn sample_view() -> JobWidgetView {
		JobWidgetView {
			title: "loadtest".into(),
			headline: Some(JobWidgetHeadline {
				text: "186 tok/s".into(),
				label: Some("decode".into()),
				trend: Some(JobWidgetTrend {
					text: "▲ 15%".into(),
					tone: TrendTone::Success,
				}),
			}),
			stage: None,
			blocks: vec![],
			state: JobWidgetState::Live,
			updated_at: 10,
		}
	}

	#[test]
	fn glance_and_summary() {
		let v = sample_view();
		let g = job_widget_glance(&v).unwrap();
		assert_eq!(g.text, "186 tok/s");
		assert_eq!(g.label.as_deref(), Some("decode"));
		let s = job_widget_summary(&v);
		assert!(s.contains("loadtest"));
		assert!(s.contains("186 tok/s"));
		assert!(s.contains("▲ 15%"));
	}

	#[test]
	fn latest_live() {
		let a = JobWidgetEntry {
			id: "a".into(),
			r#type: "bash".into(),
			view: JobWidgetView {
				state: JobWidgetState::Fallback,
				updated_at: 20,
				title: "a".into(),
				headline: None,
				stage: None,
				blocks: vec![],
			},
		};
		let b = JobWidgetEntry {
			id: "b".into(),
			r#type: "bash".into(),
			view: sample_view(),
		};
		assert_eq!(latest_job_widget(&[a, b]).unwrap().id, "b");
	}
}
