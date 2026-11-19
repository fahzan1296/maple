//! Draws a [`ChartSpec`] as a standalone SVG document colored only with figure
//! tokens — `var(--fg)`, `var(--muted)`, `var(--border)`, `var(--accent)`,
//! `var(--surface)`, `var(--c1)`…`var(--c6)` — on a transparent background.
//!
//! Reference: `l/src/charts/chart-svg.ts`.
//!
//! Deviations forced by Rust:
//! - TypeScript interfaces become structs and enums; `Scale`'s `at` is a
//!   boxed closure. `Layer`'s optional fields are `Option`.
//! - `titleFigure`'s lookbehind regex is a hand-rolled scanner, and string
//!   indices are `char` offsets rather than UTF-16 units.
//! - `toLocaleString("en-US")` grouping reuses `chart_plan::group_digits`.

use super::chart_plan::{
	group_digits, interval_middle, is_condition_pair, percent_text, reference_factors, target_standing,
	ChartAnnotation, ChartAxis, ChartGroup, ChartKind, ChartLadder, ChartPoint, ChartSeries, ChartSpec, ChartTarget, FlowTotal,
	Shading,
	Polarity, Standing,
};
use super::table_data::{cell_status, Dimension, Status};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const WIDTH: f64 = 720.0;
const PAD: f64 = 12.0;
const LABEL_SIZE: f64 = 13.0;
const TICK_SIZE: f64 = 11.0;
/// Mean advance per em of the UI sans, for character budgets; `measure` estimates real widths.
const ADVANCE: f64 = 0.55;
/// The UI sans, best first.
const FONT: &str =
	"Geist, -apple-system, system-ui, 'SF Pro Text', Inter, 'Segoe UI', Roboto, 'Noto Sans', 'DejaVu Sans', Arial, 'Helvetica Neue', sans-serif";
/// Series colors in draw order.
const SERIES: [&str; 6] = ["c1", "c2", "c3", "c4", "c5", "c6"];
/// Corner radius of bars, tracks and cells.
const RADIUS: f64 = 3.0;
/// Opacity of gridlines over the border color: hairlines behind the marks.
const GRID_OPACITY: f64 = 0.5;
/// Ratio between line series' peaks from which they stop sharing an axis.
const SPLIT_SPREAD: f64 = 8.0;
/// Most categories whose every sample draws as a dot on its line.
const MAX_DOTS: usize = 16;
/// Spread (max/min of positive values) from which an axis turns logarithmic.
const LOG_SPREAD: f64 = 100.0;
/// Spread from which dots and intervals take a log axis.
const SPAN_LOG: f64 = 10.0;
/// Most decades a positional log axis ticks at 1, 2 and 5 times each power of ten.
const FINE_DECADES: f64 = 2.5;
/// Typical ratio between two interval series of one unit from which they are different quantities.
const BAND_SPREAD: f64 = 30.0;
/// Log-axis ticks for durations from a second up, in seconds.
const CLOCK_TICKS: [f64; 21] = [
	1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1200.0, 1800.0, 3600.0, 7200.0, 18_000.0,
	43_200.0, 86_400.0, 172_800.0, 604_800.0, 1_209_600.0, 2_592_000.0,
];
/// Smallest jump from the rest to an outlier tier that breaks the axis.
const TIER_GAP: f64 = 8.0;
/// Fewest values below an outlier tier that make breaking the axis worth it.
const MIN_BELOW_TIER: usize = 3;
/// Most of the values an outlier tier may hold.
const MAX_TIER_SHARE: f64 = 1.0 / 3.0;
/// Share of a broken axis' length the values below the break get.
const CUT_AT: f64 = 0.7;
/// Width of the gap parting a broken axis' segments, in user units.
const CUT_GAP: f64 = 10.0;
/// Share of its largest value a dumbbell's smallest must reach for its axis to leave zero out.
const ZOOM_SHARE: f64 = 0.6;
/// How far past the rest a clipped diverging axis reaches.
const CLIP_REACH: f64 = 1.25;
/// Length of a clipped bar between its break and its end.
const CLIP_INSET: f64 = 16.0;
const MAX_LABEL_CHARS: usize = 28;
/// Widest each line of a category name wrapped to two sets.
const LABEL_WIDTH: f64 = 200.0;
/// Size of a name wrapped over two lines.
const WRAP_SIZE: f64 = 12.0;
/// Baseline distance between a wrapped name's two lines.
const WRAP_LINE: f64 = 13.0;
/// Least row height that holds a wrapped name.
const WRAP_ROW: f64 = 30.0;
/// Most categories whose names wrap.
const MAX_WRAP_ROWS: usize = 12;
/// Widest a category name sets on one line.
const WIDE_LABEL_WIDTH: f64 = 260.0;
/// Longest group name in a label column.
const MAX_GROUP_CHARS: usize = 20;
/// Vertical space parting consecutive groups of category rows.
const GROUP_GAP: f64 = 8.0;
/// Semibold text sets this much wider than `measure` estimates for regular text.
const SEMIBOLD_WIDTH: f64 = 1.06;
/// Opacity of marks beside an emphasized one.
const RECEDE: f64 = 0.4;
/// Categories beyond which grouped bars pack tighter.
const DENSE_ROWS: usize = 12;
/// Share of a stacked bar's whole its parts may miss by rounding before the rest draws as a track.
const MIN_REST: f64 = 0.01;
/// Least length of a drawn bar.
const MIN_BAR: f64 = 2.5;
/// Fill opacity of an estimated bar inside its outline.
const ESTIMATE_OPACITY: f64 = 0.4;
/// Opacity of a range bar's span past its low end.
const RANGE_OPACITY: f64 = 0.35;
/// Most panels across one band of small multiples drawn as panels.
const PANELS_ACROSS: usize = 3;
/// Factors a change axis rounds its ends out to.
const FACTOR_STOPS: [f64; 6] = [1.25, 1.5, 2.0, 3.0, 5.0, 10.0];
/// Height of the title line.
const TITLE_LINE: f64 = 18.0;
/// Space between the header and the plot under it.
const HEADER_GAP: f64 = 10.0;
/// Height of one wrapped legend line.
const LEGEND_LINE: f64 = 18.0;
/// Heat level (0–1) from which a cell prints its figure in `ink`.
const INK_LEVEL: f64 = 0.58;
/// Fill opacity of the faintest heat cell.
const HEAT_FLOOR: f64 = 0.09;
/// Mask id behind `ink`.
const INK_MASK: &str = "chart-ink";
/// Longest callout text.
const MAX_CALLOUT_CHARS: usize = 28;
/// Longest legend name.
const MAX_LEGEND_CHARS: usize = 32;
/// Longest name a line carries at its end instead of a legend.
const MAX_DIRECT_LABEL: f64 = 130.0;
/// Width the status badge before a category name takes.
const STATUS_WIDTH: f64 = 14.0;
/// Room a `best` tag takes after its value.
const BEST_WIDTH: f64 = 34.0;
/// Most of the width right of the label column a figures column grows to.
const FIGURE_SHARE: f64 = 0.48;
/// Least share of a small-multiples column its bars keep.
const MIN_BAR_SHARE: f64 = 0.4;
/// Share of a dense grouped chart's cells at zero from which their figures drop.
const ZERO_QUIET: f64 = 0.2;
/// Factor within which a move reads as no change.
const STILL: f64 = 1.03;
/// Space between a transition's figures and its move.
const MOVE_GAP: &str = "  ";
/// Opacity of a transition's connector under its end dot.
const MOVE_OPACITY: f64 = 0.55;
/// Widest figures column of a transition.
const MAX_MOVE_COLUMN: f64 = 250.0;
/// Factor from which a chart writes its moves as factors.
const FACTOR_MOVE: f64 = 10.0;
/// Whole counts below this move by differences.
const MAX_COUNTED: f64 = 100.0;
/// Height of a ladder row.
const LADDER_ROW: f64 = 34.0;
/// Fill opacity of a ladder's body.
const LADDER_BODY: f64 = 0.4;
/// Height of a histogram's plot.
const HISTOGRAM_HEIGHT: f64 = 170.0;
/// Share of a bin's width its columns leave empty.
const BIN_GAP: f64 = 0.28;
/// Opacity step of each further part sharing one outcome color.
const TONE_STEP: f64 = 0.4;
/// Height of a timeline's strip.
const STRIP: f64 = 30.0;
/// Most label lanes under a timeline's strip.
const MAX_LANES: usize = 4;
/// Height of one label lane under a timeline's strip.
const LANE: f64 = 16.0;
/// Share of a timeline's run from which a segment leads in the accent color.
const LEADING_SEGMENT: f64 = 0.15;
/// Most segments a timeline leads with.
const MAX_LEADING: usize = 3;
/// Share of a timeline's run from which a stretch's label claims a lane.
const LABEL_FIRST: f64 = 0.05;
/// Factor within which every move of a `change` reads as a percent.
const PERCENT_REACH: f64 = 3.0;
/// Least room between two factor tick labels.
const FACTOR_TICK_GAP: f64 = 56.0;
/// Least reach of a percent axis toward each side its moves take.
const MIN_PERCENT_REACH: f64 = 10.0;
/// Plot height a slope chart gives each category.
const SLOPE_ROW: f64 = 26.0;
/// Least and most plot height of a slope chart.
const MIN_SLOPE_HEIGHT: f64 = 150.0;
const MAX_SLOPE_HEIGHT: f64 = 300.0;
/// Widest end label column of a slope chart.
const MAX_SLOPE_LABEL: f64 = 230.0;
/// Widest a stat tile grows.
const MAX_TILE: f64 = 300.0;
/// Space between stat tiles.
const TILE_GAP: f64 = 24.0;
/// Size of a stat tile's factor.
const FACTOR_SIZE: f64 = 24.0;
/// Furthest past its target a bullet axis reaches.
const TARGET_REACH: f64 = 2.0;
/// Least reach of a bullet axis where a value runs off it.
const OVERRUN_ROOM: f64 = 1.25;
/// Opacity of a bullet row's track.
const TRACK_OPACITY: f64 = 0.3;
/// Characters a matrix column's heading takes per line.
const MATRIX_HEAD_CHARS: usize = 11;
/// Height of one matrix heading line.
const MATRIX_LINE: f64 = 12.0;

/// A drawn layer: its SVG elements and the height they span from `top`.
struct Layer {
	height: f64,
	body: String,
	legend: Option<Vec<LegendItem>>,
	ramp: Option<RampKey>,
	notes: Option<Vec<String>>,
}

impl Layer {
	fn plain(height: f64, body: String) -> Self {
		Layer { height, body, legend: None, ramp: None, notes: None }
	}
}

#[derive(Clone)]
/// A legend entry: a name beside a filled dot of `color`, or a hollow ring.
struct LegendItem {
	name: String,
	color: String,
	ring: bool,
	tones: Option<Vec<String>>,
	opacity: Option<f64>,
}

/// A heat scale key.
struct RampKey {
	color: String,
	low: String,
	high: String,
}

/// Map a value to a coordinate, with tick values for the axis.
struct Scale {
	at: Box<dyn Fn(f64) -> f64>,
	ticks: Vec<f64>,
	log: bool,
	cut: Option<Cut>,
}

impl Scale {
	fn new(at: Box<dyn Fn(f64) -> f64>, ticks: Vec<f64>, log: bool, cut: Option<Cut>) -> Self {
		Scale { at, ticks, log, cut }
	}
	fn at(&self, value: f64) -> f64 {
		(self.at)(value)
	}
}

#[derive(Clone)]
/// Where a broken axis parts its segments.
struct Cut {
	below: f64,
	start: f64,
	end: f64,
}

/// A few values standing far above the rest.
struct Tier {
	rest: f64,
	low: f64,
	high: f64,
}

#[derive(Clone, Copy)]
enum Anchor {
	Start,
	Middle,
	End,
}

struct TextStyle {
	size: Option<f64>,
	fill: String,
	anchor: Option<Anchor>,
	weight: Option<u32>,
	italic: bool,
}

impl TextStyle {
	fn new(fill: &str) -> Self {
		TextStyle { size: None, fill: fill.to_string(), anchor: None, weight: None, italic: false }
	}
}

/// A stretch of one `text` set apart from the rest.
struct TextRun {
	content: String,
	fill: Option<String>,
	weight: Option<u32>,
}

fn max_all(values: &[f64]) -> f64 {
	values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn min_all(values: &[f64]) -> f64 {
	values.iter().copied().fold(f64::INFINITY, f64::min)
}

fn max_from(from: f64, values: &[f64]) -> f64 {
	values.iter().copied().fold(from, f64::max)
}

fn status_color(status: Status) -> &'static str {
	match status {
		Status::Good => "success",
		Status::Bad => "error",
		Status::Warn => "warning",
		Status::Pending => "muted",
	}
}

fn token(name: &str) -> String {
	format!("var(--{name})")
}

/// `raw` safe as XML text or a double-quoted attribute value.
fn escape_xml(raw: &str) -> String {
	let mut out = String::with_capacity(raw.len());
	for ch in raw.chars() {
		match ch {
			'&' => out.push_str("&amp;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'"' => out.push_str("&quot;"),
			_ => out.push(ch),
		}
	}
	out
}

fn num(value: f64) -> String {
	let rounded = (value * 10.0).round() / 10.0;
	if rounded == 0.0 {
		"0".to_string()
	} else {
		format!("{rounded}")
	}
}

/// Narrow and wide glyphs of a UI sans, for `measure`.
const NARROW: &str = "iljtfrI.,:;'!|()[] ";
const WIDE: &str = "mwMW%@—";

/// Estimated width of `content` set in the chart's sans at `size`.
fn measure(content: &str, size: f64) -> f64 {
	let mut em = 0.0;
	for ch in content.chars() {
		if NARROW.contains(ch) {
			em += 0.3;
		} else if WIDE.contains(ch) {
			em += 0.85;
		} else if ch.is_ascii_digit() {
			em += 0.58;
		} else if ch.is_ascii_uppercase() {
			em += 0.66;
		} else if (ch as u32) >= 0x1100 {
			em += 1.0;
		} else {
			em += 0.53;
		}
	}
	em * size
}

fn clip(content: &str, chars: usize) -> String {
	let count = content.chars().count();
	if count <= chars {
		return content.to_string();
	}
	let mut out: String = content.chars().take(chars.saturating_sub(1).max(1)).collect();
	out.push('…');
	out
}

/// `content` clipped to fit `width` user units at `size`.
fn fit_text(content: &str, width: f64, size: f64) -> String {
	let mut chars = content.chars().count();
	while chars > 1 && measure(&clip(content, chars), size) > width {
		chars -= 1;
	}
	clip(content, chars)
}

/// Category names clipped to `chars`, keeping their start; clashing identifiers keep both ends.
fn clip_names(names: &[String], chars: usize) -> Vec<String> {
	let clipped: Vec<String> = names.iter().map(|name| clip(name, chars)).collect();
	let mut owners: HashMap<String, String> = HashMap::new();
	let mut clashing: HashSet<String> = HashSet::new();
	for (at, label) in clipped.iter().enumerate() {
		match owners.get(label) {
			None => {
				owners.insert(label.clone(), names[at].clone());
			}
			Some(owner) if owner != &names[at] => {
				clashing.insert(label.clone());
			}
			_ => {}
		}
	}
	if clashing.is_empty() || chars < 7 {
		return clipped;
	}
	let head = chars / 2;
	clipped
		.iter()
		.enumerate()
		.map(|(at, label)| {
			let name = &names[at];
			if !clashing.contains(label) || name.chars().any(char::is_whitespace) {
				return label.clone();
			}
			let letters: Vec<char> = name.chars().collect();
			let tail = letters.len().saturating_sub(chars - 1 - head);
			let mut out: String = letters[..head.min(letters.len())].iter().collect();
			out.push('…');
			out.extend(letters[tail.min(letters.len())..].iter());
			out
		})
		.collect()
}

/// Text vertically centred on `y`.
fn text(x: f64, y: f64, content: &str, style: &TextStyle) -> String {
	text_runs(x, y, &[TextRun { content: content.to_string(), fill: None, weight: None }], style)
}

fn text_runs(x: f64, y: f64, runs: &[TextRun], style: &TextStyle) -> String {
	if !runs.iter().any(|run| !run.content.is_empty()) {
		return String::new();
	}
	let size = style.size.unwrap_or(LABEL_SIZE);
	let mut attrs: Vec<String> = vec![
		format!("x=\"{}\"", num(x)),
		format!("y=\"{}\"", num(y + size * 0.35)),
		format!("fill=\"{}\"", token(&style.fill)),
	];
	if size != LABEL_SIZE {
		attrs.push(format!("font-size=\"{size}\""));
	}
	if let Some(anchor) = style.anchor {
		if !matches!(anchor, Anchor::Start) {
			attrs.push(format!(
				"text-anchor=\"{}\"",
				match anchor {
					Anchor::Start => "start",
					Anchor::Middle => "middle",
					Anchor::End => "end",
				}
			));
		}
	}
	if let Some(weight) = style.weight {
		attrs.push(format!("font-weight=\"{weight}\""));
	}
	if style.italic {
		attrs.push("font-style=\"italic\"".to_string());
	}
	let body: Vec<String> = runs
		.iter()
		.filter(|run| !run.content.is_empty())
		.map(|run| {
			let mut own = String::new();
			if let Some(fill) = &run.fill {
				own.push_str(&format!(" fill=\"{}\"", token(fill)));
			}
			if let Some(weight) = run.weight {
				own.push_str(&format!(" font-weight=\"{weight}\""));
			}
			if own.is_empty() {
				escape_xml(&run.content)
			} else {
				format!("<tspan{own}>{}</tspan>", escape_xml(&run.content))
			}
		})
		.collect();
	format!("<text {}>{}</text>", attrs.join(" "), body.join(""))
}

/// Text in a dark ink that reads on a strong fill of a series color.
fn ink(x: f64, y: f64, content: &str, style: &TextStyle) -> String {
	if content.is_empty() {
		return String::new();
	}
	let mut surface = style.clone_style();
	surface.fill = "surface".to_string();
	let cover = text(x, y, content, &surface)
		.replacen("<text ", &format!("<text mask=\"url(#{})\" ", INK_MASK), 1);
	let mut fg = style.clone_style();
	fg.fill = "fg".to_string();
	format!("{}{}{}", text(x, y, content, &fg), cover, cover)
}

impl TextStyle {
	fn clone_style(&self) -> Self {
		TextStyle {
			size: self.size,
			fill: self.fill.clone(),
			anchor: self.anchor,
			weight: self.weight,
			italic: self.italic,
		}
	}
}

/// The mask `ink` draws through.
const INK_DEFS: &str = "<defs><mask id=\"chart-ink\"><rect x=\"-10000\" y=\"-10000\" width=\"20000\" height=\"20000\" fill=\"var(--fg)\"/></mask></defs>";

/// A soft `surface` band behind a highlighted row or column.
fn band(x: f64, y: f64, width: f64, height: f64) -> String {
	rect(x, y, width, height, "surface", 0.55, 4.0)
}

/// A dashed reference rule over the gridlines.
fn reference_rule(x1: f64, y1: f64, x2: f64, y2: f64) -> String {
	format!(
		"<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\" stroke-dasharray=\"3 3\" stroke-opacity=\"0.6\"/>",
		num(x1),
		num(y1),
		num(x2),
		num(y2),
		token("fg")
	)
}

/// A callout: semibold `note` at `x`, joined to its mark by a dotted leader.
fn callout(from: f64, y: f64, x: f64, note: &str) -> String {
	let leader = if x - 4.0 > from + 2.0 {
		format!(
			"<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\" stroke-dasharray=\"1 2\"/>",
			num(from),
			num(y),
			num(x - 4.0),
			num(y),
			token("muted")
		)
	} else {
		String::new()
	};
	let mut style = TextStyle::new("fg");
	style.size = Some(TICK_SIZE);
	style.weight = Some(600);
	format!("{leader}{}", text(x, y, note, &style))
}

/// A status word or prose standing where a cell holds no number.
fn note_text(x: f64, y: f64, note: &str, centred: bool) -> String {
	let said = cell_status(note);
	let status = said.is_some() || SHORT_WORD_RE.is_match(note);
	let fill = match said {
		Some(state) => status_color(state).to_string(),
		None if status => "warning".to_string(),
		None => "muted".to_string(),
	};
	let mut style = TextStyle::new(&fill);
	style.size = Some(TICK_SIZE);
	style.italic = !status;
	style.anchor = if centred { Some(Anchor::Middle) } else { None };
	text(x, y, note, &style)
}

static SHORT_WORD_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new("^[A-Z0-9 _/!-]{1,12}$").expect("valid SHORT_WORD regex"));

/// A rectangle with corners rounded by `corner`.
fn rect(x: f64, y: f64, width: f64, height: f64, fill: &str, opacity: f64, corner: f64) -> String {
	let w = width.max(0.0);
	let radius = corner.min(w / 2.0).min(height / 2.0);
	let rounded = if radius >= 0.5 { format!(" rx=\"{}\"", num(radius)) } else { String::new() };
	let alpha = if opacity < 1.0 {
		format!(" fill-opacity=\"{}\"", (opacity * 100.0).round() / 100.0)
	} else {
		String::new()
	};
	format!(
		"<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rounded} fill=\"{}\"{alpha}/>",
		num(x),
		num(y),
		num(w),
		num(height),
		token(fill)
	)
}

/// A round-capped line.
fn line(x1: f64, y1: f64, x2: f64, y2: f64, stroke: &str, width: f64, opacity: f64) -> String {
	let alpha = if opacity < 1.0 {
		format!(" stroke-opacity=\"{}\"", (opacity * 100.0).round() / 100.0)
	} else {
		String::new()
	};
	format!(
		"<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\"{alpha}/>",
		num(x1),
		num(y1),
		num(x2),
		num(y2),
		token(stroke),
		width
	)
}

/// A gridline: a hairline that recedes behind the marks.
fn rule(x1: f64, y1: f64, x2: f64, y2: f64) -> String {
	line(x1, y1, x2, y2, "border", 1.0, GRID_OPACITY)
}

/// A rectangle's outline with rounded corners.
fn outline(x: f64, y: f64, width: f64, height: f64, stroke: &str) -> String {
	format!(
		"<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\"/>",
		num(x),
		num(y),
		num(width),
		num(height),
		token(stroke)
	)
}

/// The `best` mark beside a series' best value.
fn best_tag(x: f64, y: f64, size: f64) -> String {
	let mut style = TextStyle::new("success");
	style.size = Some(size - 1.5);
	text(x, y, "best", &style)
}

/// A hollow circle.
fn ring(x: f64, y: f64, radius: f64, stroke: &str) -> String {
	format!(
		"<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.75\"/>",
		num(x),
		num(y),
		radius,
		token(stroke)
	)
}

fn dot(x: f64, y: f64, radius: f64, fill: &str, opacity: f64) -> String {
	let alpha = if opacity < 1.0 {
		format!(" fill-opacity=\"{}\"", (opacity * 100.0).round() / 100.0)
	} else {
		String::new()
	};
	format!(
		"<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"{alpha}/>",
		num(x),
		num(y),
		radius,
		token(fill)
	)
}

/// Two slanted strokes centred on `x`, `y`.
fn break_mark(x: f64, y: f64, half: f64, stroke: &str) -> String {
	let slant = half * 0.55;
	[-2.5, 2.5]
		.map(|offset| line(x + offset - slant, y + half, x + offset + slant, y - half, stroke, 1.25, 1.0))
		.join("")
}

/// A bar from `zero` to `end` crossing a broken axis.
fn cut_bar(cut: &Cut, zero: f64, end: f64, y: f64, height: f64, fill: &str, opacity: f64) -> String {
	format!(
		"{}{}{}",
		rect(zero, y, cut.start - zero, height, fill, opacity, RADIUS),
		rect(cut.end, y, (end - cut.end).max(1.0), height, fill, opacity, RADIUS),
		break_mark((cut.start + cut.end) / 2.0, y + height / 2.0, height / 2.0 + 1.0, "muted")
	)
}


fn or_one(value: f64) -> f64 {
	if value == 0.0 {
		1.0
	} else {
		value
	}
}

fn precision(value: f64, digits: usize) -> f64 {
	let text = format!("{:.*e}", digits - 1, value);
	text.parse().unwrap_or(value)
}

/// 1, 2, 2.5 or 5 times a power of ten, at least `raw`.
fn nice_step(raw: f64) -> f64 {
	let base = if raw == 0.0 { 1.0 } else { raw };
	let power = 10f64.powf(base.log10().floor());
	let fraction = raw / power;
	(if fraction <= 1.0 {
		1.0
	} else if fraction <= 2.0 {
		2.0
	} else if fraction <= 2.5 {
		2.5
	} else if fraction <= 5.0 {
		5.0
	} else {
		10.0
	}) * power
}

/// Nice tick spacing for a linear axis over `min`…`max`.
fn linear_ticks(min: f64, max: f64, integral: bool) -> (f64, f64, f64) {
	let mut best: Option<(f64, f64, f64, f64)> = None;
	for intervals in [4.0, 5.0, 3.0, 6.0] {
		let step = nice_step((max - min) / intervals);
		if integral && step.fract() != 0.0 {
			continue;
		}
		let low = (min / step).floor() * step;
		let high = (max / step).ceil() * step;
		let count = ((high - low) / step).round();
		if count < 2.0 || count > 7.0 {
			continue;
		}
		let waste = (high - low - (max - min)) / if high - low == 0.0 { 1.0 } else { high - low };
		let quarter = ((step / 10f64.powf(step.log10().floor())) - 2.5).abs() < 1e-9;
		let cost = waste + 0.03 * ((count - 4.5).abs() - 0.5).max(0.0) + if quarter { 0.08 } else { 0.0 };
	if best.is_none() || cost < best.unwrap().3 - 1e-9 {
			best = Some((low, high, step, cost));
		}
	}
	if let Some((low, high, step, _)) = best {
		return (low, high, step);
	}
	let step = if integral {
		nice_step((max - min) / 4.0).ceil().max(1.0)
	} else {
		nice_step((max - min) / 4.0)
	};
	((min / step).floor() * step, (max / step).ceil() * step, step)
}

/// Whether `values` are all positive and span `LOG_SPREAD`× or more.
fn log_spread(values: &[f64]) -> bool {
	let min = min_all(values);
	min > 0.0 && max_all(values) / min >= LOG_SPREAD
}

/// A value axis over `values` from `from` to `to`.
fn value_scale(values: &[f64], from: f64, to: f64, zero: bool, breakable: bool, unit: f64) -> Scale {
	let mut min = min_all(values);
	let mut max = max_all(values);
	if !min.is_finite() || !max.is_finite() {
		min = 0.0;
		max = 1.0;
	}
	if breakable {
		if let Some(tier) = outlier_tier(values, LOG_SPREAD) {
			return broken_scale(&tier, from, to, true);
		}
	}
	if log_spread(values) {
		let low = min.log10().floor();
		let high = max.log10().ceil();
		let mut ticks: Vec<f64> = Vec::new();
		let stride = (((high - low) / 6.0).ceil() as i64).max(1);
		let mut power = low as i64;
		while power <= high as i64 {
			ticks.push(10f64.powi(power as i32));
			power += stride;
		}
		let span = if high - low == 0.0 { 1.0 } else { high - low };
		return Scale::new(
			Box::new(move |value| {
				from + (value.max(10f64.powf(low)).log10() - low) / span * (to - from)
			}),
			ticks,
			true,
			None,
		);
	}
	if zero {
		min = min.min(0.0);
		max = max.max(0.0);
	} else if min > 0.0 && min < max * 0.35 {
		min = 0.0;
	}
	if min == max {
		max = if min == 0.0 { 1.0 } else { min + min.abs() * 0.5 };
		if !zero {
			min -= min.abs() * 0.5;
		}
	}
	let nice = linear_ticks(min / unit, max / unit, values.iter().all(|value| (value / unit).fract() == 0.0));
	let low = nice.0 * unit;
	let high = nice.1 * unit;
	let step = nice.2 * unit;
	let mut ticks: Vec<f64> = Vec::new();
	let mut tick = low;
	while tick <= high + step / 2.0 {
		ticks.push(precision(tick, 12));
		tick += step;
	}
	Scale::new(
		Box::new(move |value| from + ((value - low) / or_one(high - low)) * (to - from)),
		ticks,
		false,
		None,
	)
}

/// A positional axis for dots and intervals.
fn span_scale(values: &[f64], from: f64, to: f64, marks: &[f64], dim: Option<Dimension>, unit: f64) -> Scale {
	let reach: Vec<f64> = values.iter().chain(marks.iter()).copied().collect();
	let min = min_all(&reach);
	let max = max_all(&reach);
	let values_min = min_all(values);
	let values_max = max_all(values);
	if !(values_min > 0.0) || !(min > 0.0) || values_max / values_min < SPAN_LOG {
		return value_scale(&reach, from, to, false, false, unit);
	}
	let fine = (max / min).log10() <= FINE_DECADES;
	let steps: &[f64] = if fine { &[1.0, 2.0, 5.0] } else { &[1.0] };
	let mut grid: Vec<f64> = Vec::new();
	let low_power = min.log10().floor() as i64;
	let high_power = max.log10().ceil() as i64;
	for power in low_power..=high_power {
		for step in steps {
			let tick = precision(step * 10f64.powi(power as i32), 6);
			if dim != Some(Dimension::Duration) || tick < 1.0 || tick > CLOCK_TICKS[CLOCK_TICKS.len() - 1] {
				grid.push(tick);
			}
		}
	}
	if dim == Some(Dimension::Duration) {
		if fine {
			grid.extend(CLOCK_TICKS.iter().copied());
		} else {
			grid.extend(CLOCK_TICKS.iter().step_by(3).copied());
		}
	}
	grid.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	let low = grid
		.iter()
		.filter(|tick| **tick <= min * (1.0 + 1e-9))
		.copied()
		.fold(f64::NEG_INFINITY, f64::max);
	let high = grid
		.iter()
		.filter(|tick| **tick >= max * (1.0 - 1e-9))
		.copied()
		.fold(f64::INFINITY, f64::min);
	let all: Vec<f64> = grid.iter().copied().filter(|tick| *tick >= low && *tick <= high).collect();
	let stride = (all.len() as f64 / 7.0).ceil().max(1.0) as usize;
	let ticks: Vec<f64> = all
		.iter()
		.enumerate()
		.filter(|(at, _)| at % stride == 0 || *at == all.len() - 1)
		.map(|(_, tick)| *tick)
		.collect();
	let a = low.log10();
	let b = high.log10();
	Scale::new(
		Box::new(move |value| from + ((value.max(low).log10() - a) / or_one(b - a)) * (to - from)),
		ticks,
		true,
		None,
	)
}

/// The few values standing far above the rest, at the widest such jump.
fn outlier_tier(values: &[f64], rest_spread: f64) -> Option<Tier> {
	if values.iter().any(|value| *value < 0.0) {
		return None;
	}
	let mut sorted: Vec<f64> = values.iter().copied().filter(|value| *value > 0.0).collect();
	sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
	let most = (values.len() as f64 * MAX_TIER_SHARE).floor() as usize;
	let mut best: Option<Tier> = None;
	let mut gap = TIER_GAP;
	for k in 1..=most {
		if k + MIN_BELOW_TIER > sorted.len() {
			break;
		}
		let ratio = sorted[k - 1] / sorted[k];
		if ratio >= gap {
			gap = ratio;
			best = Some(Tier { rest: sorted[k], low: sorted[k - 1], high: sorted[0] });
		}
	}
	let best = best?;
	if best.rest / sorted[sorted.len() - 1] >= rest_spread {
		return None;
	}
	Some(best)
}

/// A linear axis from zero over the rest of `tier`, then a zoomed second segment.
fn broken_scale(tier: &Tier, from: f64, to: f64, nice: bool) -> Scale {
	let middle = from + (to - from) * CUT_AT;
	let sign = (to - from).signum();
	let start = middle - sign * CUT_GAP / 2.0;
	let end = middle + sign * CUT_GAP / 2.0;
	let step = nice_step(tier.rest / 4.0);
	let top = if nice { (tier.rest / step).ceil() * step } else { tier.rest };
	let spread = tier.high - tier.low;
	let zoom = nice_step((spread.max(tier.high * 0.1)) / 2.0);
	let floor = (tier.low - (spread * 0.25).max(tier.high * 0.05)).min(tier.high / 2.0);
	let low = top.max(if nice { (floor / zoom).floor() * zoom } else { floor });
	let high = if nice { (tier.high / zoom).ceil() * zoom } else { tier.high };
	let mut ticks: Vec<f64> = Vec::new();
	let mut tick = 0.0;
	while tick <= top + step / 2.0 {
		ticks.push(precision(tick, 12));
		tick += step;
	}
	ticks.push(high);
	Scale::new(
		Box::new(move |value| {
			if value <= top {
				from + (value.max(0.0) / top) * (start - from)
			} else {
				end + ((value.max(low) - low) / or_one(high - low)) * (to - end)
			}
		}),
		ticks,
		false,
		Some(Cut { below: top, start, end }),
	)
}

/// Bar ends from `from` to `to` for a panel without an axis.
fn panel_scale(values: &[f64], from: f64, to: f64) -> Scale {
	if let Some(tier) = outlier_tier(values, LOG_SPREAD) {
		return broken_scale(&tier, from, to, false);
	}
	let max = max_all(values);
	let min = values.iter().fold(0.0f64, |acc, value| acc.min(*value));
	let span = if max.is_finite() && max > min { max - min } else { 0.0 };
	Scale::new(
		Box::new(move |value| from + (if span != 0.0 { (value - min) / span } else { 1.0 }) * (to - from)),
		Vec::new(),
		false,
		None,
	)
}

/// Where a linear heat ramp starts.
fn heat_floor(values: &[f64]) -> f64 {
	let min = min_all(values);
	let max = max_all(values);
	if !(min > 0.0 && min >= max * 0.5 && max > min) {
		return 0.0;
	}
	let step = nice_step((max - min) / 4.0);
	(((min - (max - min) / 3.0) / step).floor() * step).max(0.0)
}

/// Whether `shade` maps `values` logarithmically.
fn log_shaded(values: &[f64]) -> bool {
	let positive: Vec<f64> = values.iter().copied().filter(|value| *value > 0.0).collect();
	if positive.len() < 2 || values.iter().any(|value| *value < 0.0) {
		return false;
	}
	max_all(&positive) / min_all(&positive) >= LOG_SPREAD || outlier_tier(values, LOG_SPREAD).is_some()
}

/// Fill strength 0–1 of heat cells.
fn shade(values: &[f64]) -> Box<dyn Fn(f64) -> f64> {
	let lowest = min_all(values);
	if lowest < 0.0 {
		let span = max_all(values) - lowest;
		return Box::new(move |value| if span != 0.0 { (value - lowest) / span } else { 1.0 });
	}
	let positive: Vec<f64> = values.iter().copied().filter(|value| *value > 0.0).collect();
	let max = max_all(&positive);
	let min = min_all(&positive);
	if positive.len() < 2 || max == min {
		return Box::new(|value| if value > 0.0 { 1.0 } else { 0.0 });
	}
	if log_shaded(values) {
		let low = min.log10();
		let span = max.log10() - low;
		return Box::new(move |value| {
			if value > 0.0 {
				0.1 + (0.9 * (value.log10() - low)) / span
			} else {
				0.0
			}
		});
	}
	let floor = heat_floor(values);
	Box::new(move |value| (value - floor).max(0.0) / (max - floor))
}

/// Gridlines and tick labels under a horizontal value axis.
fn draw_value_axis(scale: &Scale, series: &ChartSeries, top: f64, bottom: f64) -> String {
	let mut out = String::new();
	let format = tick_format(scale, series);
	for tick in &scale.ticks {
		let x = scale.at(*tick);
		out.push_str(&if *tick == 0.0 && !scale.log {
			line(x, top, x, bottom, "border", 1.0, 1.0)
		} else {
			rule(x, top, x, bottom)
		});
		if let Some(cut) = &scale.cut {
			if *tick == cut.below {
				continue;
			}
		}
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::Middle);
		out.push_str(&text(x, bottom + 12.0, &format(*tick), &style));
	}
	if let Some(cut) = &scale.cut {
		out.push_str(&break_mark((cut.start + cut.end) / 2.0, bottom + 12.0, 5.0, "muted"));
	}
	out
}

static BINARY_SIZE_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new("(?i)[KMGTP]iB").expect("valid BINARY_SIZE regex"));

/// Whether `axis` holds sizes the table wrote in binary units.
fn binary_bytes(axis: &ChartSeries) -> bool {
	axis.dim == Dimension::Bytes
		&& axis.points.iter().flatten().any(|point| BINARY_SIZE_RE.is_match(&point.text))
}

/// One tick unit for an axis of `series` over `values`, in base units.
fn tick_unit(series: &ChartSeries, values: &[f64]) -> f64 {
	if !binary_bytes(series) {
		return 1.0;
	}
	let max = max_from(0.0, &values.iter().map(|value| value.abs()).collect::<Vec<f64>>());
	let mut unit = 1.0;
	while unit * 1024.0 <= max {
		unit *= 1024.0;
	}
	unit
}

/// Durations' units: each one's length in seconds and the least magnitude that reads in it.
const DURATION_UNITS: [(f64, &str, f64); 7] = [
	(1e-9, "ns", 0.0),
	(1e-6, "µs", 1e-6),
	(1e-3, "ms", 1e-3),
	(1.0, "s", 1.0),
	(60.0, "m", 120.0),
	(3600.0, "h", 7200.0),
	(86_400.0, "d", 172_800.0),
];

/// The `DURATION_UNITS` entry `magnitude` reads in; zero reads in seconds.
fn duration_unit(magnitude: f64) -> (f64, &'static str, f64) {
	if magnitude == 0.0 {
		return DURATION_UNITS[3];
	}
	DURATION_UNITS
		.iter()
		.rev()
		.find(|(_, _, from)| magnitude >= *from)
		.copied()
		.unwrap_or(DURATION_UNITS[0])
}

/// A magnitude for `format_value` that reads `ticks` in the largest clean unit.
fn duration_magnitude(ticks: &[f64], magnitude: f64) -> f64 {
	let clean = |seconds: f64| {
		ticks.iter().all(|tick| {
			let count = tick / seconds;
			(count * 2.0 - (count * 2.0).round()).abs() < 1e-6 && count.abs() < 10_000.0
		})
	};
	let own = DURATION_UNITS
		.iter()
		.position(|unit| *unit == duration_unit(magnitude))
		.unwrap_or(3);
	for index in (3..=own).rev() {
		let (seconds, _, from) = DURATION_UNITS[index];
		if clean(seconds) {
			return seconds.max(from);
		}
	}
	magnitude
}

/// A point's written figure, or its value formatted when the table wrote none.
fn point_text(series: &ChartSeries, at: usize) -> String {
	let Some(point) = series.points.get(at) else {
		return String::new();
	};
	let Some(point) = point else {
		return String::new();
	};
	if !point.text.is_empty() {
		return point.text.clone();
	}
	let unit = if binary_bytes(series) { "iB" } else { series.unit.as_str() };
	format_value(point.value, series.dim, unit, point.value.abs())
}

/// A value in base units as an axis label.
pub fn format_value(value: f64, dim: Dimension, unit: &str, magnitude: f64) -> String {
	match dim {
		Dimension::Percent => format!("{}%", compact(value, magnitude)),
		Dimension::Ratio => format!("{}×", compact(value, magnitude)),
		Dimension::Currency => format!("{}{}", if value < 0.0 { "-" } else { "" }, unit) + &compact(value.abs(), magnitude),
		Dimension::Duration => {
			let (divisor, suffix, _) = duration_unit(magnitude);
			format!("{}{}", compact(value / divisor, (value / divisor).abs()), suffix)
		}
		Dimension::Bytes => {
			let binary = unit == "iB";
			let base: f64 = if binary { 1024.0 } else { 1000.0 };
			let units = if binary {
				["B", "KiB", "MiB", "GiB", "TiB"]
			} else {
				["B", "KB", "MB", "GB", "TB"]
			};
			let mut index: usize = 0;
			while magnitude >= base.powi(index as i32 + 1) && index < units.len() - 1 {
				index += 1;
			}
			format!("{} {}", compact(value / base.powi(index as i32), (value / base.powi(index as i32)).abs()), units[index])
		}
		_ => compact(value, magnitude),
	}
}

/// `1234567` → `1.2M`, `0.0123` → `0.012`.
fn compact(value: f64, magnitude: f64) -> String {
	if value == 0.0 {
		return "0".to_string();
	}
	if magnitude >= 1e9 {
		return format!("{}B", trim(value / 1e9));
	}
	if magnitude >= 1e6 {
		return format!("{}M", trim(value / 1e6));
	}
	if magnitude >= 1e4 {
		return format!("{}k", trim(value / 1e3));
	}
	trim(value)
}

/// `value` to three significant digits or so; thousands take a separator.
fn trim(value: f64) -> String {
	let magnitude = value.abs();
	if magnitude >= 1000.0 {
		return group_digits(&format!("{}", value.round() as i64));
	}
	let digits = if magnitude >= 100.0 {
		0
	} else if magnitude >= 10.0 {
		1
	} else if magnitude >= 1.0 {
		2
	} else {
		3
	};
	let rounded = format!("{:.*}", digits, value);
	rounded.parse::<f64>().map(|parsed| format!("{parsed}")).unwrap_or(rounded)
}

static ESTIMATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^\\s*[~≈]").expect("valid ESTIMATE regex"));

/// Indices of `entry`'s estimated figures, only where they are the exceptions.
fn estimated_points(entry: &ChartSeries) -> HashSet<usize> {
	let estimated = |point: &Option<ChartPoint>| {
		point.as_ref().map_or(false, |point| ESTIMATE_RE.is_match(&point.text))
	};
	let marked: Vec<usize> = entry
		.points
		.iter()
		.enumerate()
		.filter(|(_, point)| estimated(point) && !point.as_ref().map_or(false, |point| point.emphasis))
		.map(|(at, _)| at)
		.collect();
	let measured = entry
		.points
		.iter()
		.filter(|point| point.is_some() && !estimated(point))
		.count();
	if marked.len() <= measured {
		marked.into_iter().collect()
	} else {
		HashSet::new()
	}
}

/// A value's bar: a `rect`, or with `estimate` a faint fill inside a dashed outline.
fn bar_rect(x: f64, y: f64, width: f64, height: f64, fill: &str, opacity: f64, estimate: bool) -> String {
	if !estimate {
		return rect(x, y, width, height, fill, opacity, RADIUS);
	}
	let radius = RADIUS.min(width / 2.0).min(height / 2.0);
	format!(
		"{}{}",
		rect(x, y, width, height, fill, opacity * ESTIMATE_OPACITY, RADIUS),
		format!(
			"<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"none\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"1\" stroke-dasharray=\"3 2\"/>",
			num(x + 0.5),
			num(y + 0.5),
			num((width - 1.0).max(0.0)),
			num(height - 1.0),
			num(radius),
			token(fill),
			(opacity * 100.0).round() / 100.0
		)
	)
}

/// The color a move by `factor` takes for a measure of `polarity`.
fn move_tone(polarity: Option<Polarity>, factor: f64) -> Option<&'static str> {
	let polarity = polarity?;
	if !factor.is_finite() || factor < 0.0 {
		return None;
	}
	if factor > 0.0 && factor.abs().ln() < STILL.ln() {
		return None;
	}
	Some(if (factor > 1.0) == (polarity == Polarity::Higher) { "success" } else { "error" })
}

/// Labels for a linear axis' ticks in one unit.
fn tick_format<'a>(scale: &Scale, axis: &'a ChartSeries) -> Box<dyn Fn(f64) -> String + 'a> {
	let unit = if binary_bytes(axis) { "iB" } else { axis.unit.as_str() };
	let dim = axis.dim;
	let format = move |ticks: &[f64], magnitude: f64| -> String {
		let first = ticks[0];
		format_value(
			first,
			dim,
			unit,
			if dim == Dimension::Duration { duration_magnitude(ticks, magnitude) } else { magnitude },
		)
	};
	let counted = dim != Dimension::Duration && dim != Dimension::Bytes;
	if scale.log {
		return Box::new(move |tick| {
			let magnitude = if counted && tick.abs() >= 1000.0 { tick.abs().max(1e4) } else { tick.abs() };
			format(&[tick], magnitude)
		});
	}
	let magnitude = max_from(0.0, &scale.ticks.iter().map(|tick| tick.abs()).collect::<Vec<f64>>());
	if let Some(cut) = &scale.cut {
		let below = cut.below;
		let ticks = scale.ticks.clone();
		return Box::new(move |tick| {
			let mut all = vec![tick];
			all.extend(ticks.iter().copied());
			if tick <= below {
				format(&all, below)
			} else {
				format(&all, magnitude)
			}
		});
	}
	let ticks = scale.ticks.clone();
	Box::new(move |tick| {
		let mut all = vec![tick];
		all.extend(ticks.iter().copied());
		format(&all, magnitude)
	})
}

struct TitleLine {
	content: String,
	start: usize,
	kept: usize,
}

struct MatrixColumn {
	lines: Vec<String>,
	width: f64,
}

struct StackBand {
	rows: (usize, usize),
	groups: (usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveStyle {
	Difference,
	Percent,
	Factor,
}

struct Stretch {
	first: usize,
	last: usize,
	name: String,
	value: f64,
	figure: String,
	group: bool,
}

#[derive(Clone)]
struct LaneItem {
	leader: f64,
	lead: bool,
	value: f64,
	group: bool,
	variants: Vec<(String, String)>,
}

struct LaneLabel {
	leader: f64,
	lead: bool,
	value: f64,
	group: bool,
	lane: usize,
	x0: f64,
	x1: f64,
	name: String,
	figure: String,
}

struct EndLabel {
	x: f64,
	y: f64,
	text: String,
	color: String,
	name: Option<String>,
}
#[derive(Clone)]

struct StageLabel {
	y: f64,
	text: String,
	names: Vec<usize>,
}

struct SpotBox {
	x0: f64,
	y0: f64,
	x1: f64,
	y1: f64,
}

fn kind_name(kind: ChartKind) -> &'static str {
	match kind {
		ChartKind::Bar => "bar",
		ChartKind::Paired => "paired",
		ChartKind::Grouped => "grouped",
		ChartKind::Line => "line",
		ChartKind::Heatmap => "heatmap",
		ChartKind::Multiples => "multiples",
		ChartKind::Share => "share",
		ChartKind::Diverging => "diverging",
		ChartKind::Scatter => "scatter",
		ChartKind::Change => "change",
		ChartKind::Progress => "progress",
		ChartKind::Stacked => "stacked",
		ChartKind::Timeline => "timeline",
		ChartKind::Waterfall => "waterfall",
		ChartKind::Dots => "dots",
		ChartKind::Range => "range",
		ChartKind::Slope => "slope",
		ChartKind::Butterfly => "butterfly",
		ChartKind::Bullet => "bullet",
		ChartKind::Ladder => "ladder",
		ChartKind::Histogram => "histogram",
		ChartKind::Stat => "stat",
	}
}

/// The SVG document for `spec`, labeled for screen readers with `chart_alt`.
pub fn render_chart_svg(spec: &ChartSpec) -> String {
	let layer = draw_chart(spec);
	let header = draw_header(spec, &layer);
	let (head_height, head_body) = matrix_head(spec);
	let top = PAD + header.height + head_height;
	let mut height = top + layer.height;
	let mut key = String::new();
	if spec.status_name.is_some() && spec.status.as_ref().map_or(false, |states| states.iter().any(|state| state.is_some())) {
		let shown: Vec<Status> = [Status::Good, Status::Warn, Status::Bad, Status::Pending]
			.into_iter()
			.filter(|state| spec.status.as_ref().map_or(false, |states| states.iter().any(|held| held.as_ref() == Some(state))))
			.collect();
		let y = height + 14.0;
		let mut dots = String::new();
		for (at, state) in shown.iter().enumerate() {
			dots.push_str(&dot(PAD + 4.0 + at as f64 * 12.0, y, 4.0, status_color(*state), 1.0));
		}
		key = dots;
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		key.push_str(&text(PAD + shown.len() as f64 * 12.0 + 4.0, y, &clip(spec.status_name.as_deref().unwrap_or(""), 60), &style));
		height += 20.0;
	}
	let mut caption = String::new();
	if let Some(caption_text) = &spec.caption {
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		caption = text(PAD, height + 14.0, &clip(caption_text, ((WIDTH - 2.0 * PAD) / (TICK_SIZE * ADVANCE)).floor() as usize), &style);
		height += 20.0;
	}
	height = (height + PAD).ceil();
	let mut out = String::new();
	out.push_str(&format!(
		"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{height}\" viewBox=\"0 0 {WIDTH} {height}\" role=\"img\" aria-label=\"{}\" font-family=\"{}\" font-size=\"{}\">",
		escape_xml(&chart_alt(spec)),
		FONT,
		LABEL_SIZE
	));
	if layer.body.contains(&format!("url(#{INK_MASK})")) {
		out.push_str(INK_DEFS);
	}
	out.push_str(&format!("<g transform=\"translate(0 {PAD})\">{}</g>", header.body));
	if !head_body.is_empty() {
		out.push_str(&format!("<g transform=\"translate(0 {})\">{}</g>", num(PAD + header.height), head_body));
	}
	out.push_str(&format!("<g transform=\"translate(0 {})\">{}</g>", num(top), layer.body));
	out.push_str(&key);
	out.push_str(&caption);
	out.push_str("</svg>");
	out
}

/// One-line description of the chart, for screen readers.
fn chart_alt(spec: &ChartSpec) -> String {
	let joint = match spec.kind {
		ChartKind::Stacked => " + ",
		ChartKind::Slope => " → ",
		ChartKind::Butterfly => " vs ",
		_ => ", ",
	};
	let names: Vec<&str> = spec.series.iter().map(|entry| entry.name.as_str()).collect();
	let kind = match spec.kind {
		ChartKind::Multiples => "small multiples".to_string(),
		_ if spec.normalized == Some(true) => "100% stacked".to_string(),
		ChartKind::Dots => "dot".to_string(),
		ChartKind::Ladder => "quantile".to_string(),
		ChartKind::Stat => "comparison".to_string(),
		other => kind_name(other).to_string(),
	};
	let mut goals: HashSet<String> = HashSet::new();
	if let Some(targets) = &spec.targets {
		for target in targets.iter().flatten() {
			goals.insert(target.text.clone());
		}
	}
	let word = spec.target_name.clone().unwrap_or_else(|| "target".to_string());
	let versus = if let Some(baseline) = &spec.baseline {
		format!(" vs {baseline}")
	} else if spec.kind == ChartKind::Bullet {
		if goals.len() == 1 {
			format!(" vs {} {word}", goals.iter().next().map(String::as_str).unwrap_or(""))
		} else {
			format!(" vs each row's {word}")
		}
	} else if let Some(whole) = &spec.whole {
		format!(" = {}", whole.name)
	} else if spec.kind == ChartKind::Waterfall {
		if let Some(total) = &spec.total {
			format!(" to {}", total.name)
		} else {
			String::new()
		}
	} else {
		String::new()
	};
	let lead = spec.title.as_ref().map_or(String::new(), |title| format!("{title}. "));
	let axis = if spec.axis.is_empty() { String::new() } else { format!(" by {}", spec.axis) };
	format!("{lead}{kind} chart of {}{versus}{axis}", names.join(joint))
}

/// The header over a chart: title, its key figure in the accent color, the muted
/// encoding note, and the layer's key right-aligned on the first line.
fn draw_header(spec: &ChartSpec, layer: &Layer) -> Layer {
	let span = WIDTH - 2.0 * PAD;
	let items = layer.legend.clone().unwrap_or_default();
	let key_width = if let Some(ramp) = &layer.ramp {
		ramp_width(ramp)
	} else if !items.is_empty() {
		legend_width(&items)
	} else {
		0.0
	};
	let subtitle = spec.subtitle.clone().or_else(|| layer.notes.as_ref().map(|notes| notes.join(" · ")));
	let full_title = spec.title.clone().unwrap_or_default();
	if full_title.is_empty() && subtitle.is_none() && key_width == 0.0 {
		return Layer::plain(0.0, String::new());
	}
	let title_size = LABEL_SIZE * SEMIBOLD_WIDTH;
	let inline = key_width == 0.0 || full_title.is_empty() || measure(&full_title, title_size) + 24.0 + key_width <= span;
	let room = if inline && key_width != 0.0 { span - key_width - 24.0 } else { span };
	let lines = if full_title.is_empty() { Vec::new() } else { title_lines(&full_title, room) };
	let figure = title_figure(&full_title);
	let mut parts: Vec<String> = Vec::new();
	for (at, line) in lines.iter().enumerate() {
		let (from, to) = match figure {
			Some((figure_from, figure_to)) => (
				figure_from.saturating_sub(line.start),
				line.kept.min(figure_to.saturating_sub(line.start)),
			),
			None => (0, 0),
		};
		let runs = if to > from {
			vec![
				TextRun { content: line.content[..from].to_string(), fill: None, weight: None },
				TextRun { content: line.content[from..to].to_string(), fill: Some("accent".to_string()), weight: None },
				TextRun { content: line.content[to..].to_string(), fill: None, weight: None },
			]
		} else {
			vec![TextRun { content: line.content.clone(), fill: None, weight: None }]
		};
		let mut style = TextStyle::new("fg");
		style.weight = Some(600);
		parts.push(text_runs(PAD, 8.0 + at as f64 * TITLE_LINE, &runs, &style));
	}
	let mut height = lines.len().max(1) as f64 * TITLE_LINE;
	if let Some(subtitle) = &subtitle {
		let last = lines.last();
		let after = last.map_or(0.0, |line| measure(&line.content, title_size) + 12.0);
		let line_room = if lines.len() > 1 { span } else { room };
		let beside = last.is_none() || after + measure(subtitle, TICK_SIZE) <= line_room;
		let y = if beside { 8.0 + lines.len().saturating_sub(1) as f64 * TITLE_LINE } else { 8.0 + lines.len() as f64 * TITLE_LINE };
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		let said = fit_text(subtitle, if beside { line_room - after } else { span }, TICK_SIZE);
		parts.push(text(PAD + if beside { after } else { 0.0 }, y, &said, &style));
		if !beside {
			height += TITLE_LINE - 2.0;
		}
	}
	if let Some(ramp) = &layer.ramp {
		let y = if inline { 8.0 } else { height + 8.0 };
		parts.push(draw_ramp(ramp, if inline { WIDTH - PAD - key_width } else { PAD }, y));
		if !inline {
			height += LEGEND_LINE;
		}
	} else if !items.is_empty() {
		if inline {
			parts.push(draw_legend(&items, WIDTH - PAD - key_width, WIDTH - PAD, 8.0).body);
		} else {
			let legend = draw_legend(&items, PAD, WIDTH - PAD, height + 8.0);
			parts.push(legend.body);
			height = legend.height;
		}
	}
	Layer::plain(height + HEADER_GAP, parts.join(""))
}

/// A title in at most two lines of `width`: whole when it fits, else broken at
/// the last space that fits the first line, the second clipped if still too long.
fn title_lines(title: &str, width: f64) -> Vec<TitleLine> {
	let size = LABEL_SIZE * SEMIBOLD_WIDTH;
	if measure(title, size) <= width {
		return vec![TitleLine { content: title.to_string(), start: 0, kept: title.chars().count() }];
	}
	let mut at = title.rfind(' ').map(|index| index as i64).unwrap_or(0);
	while at > 0 && measure(&title[..at as usize], size) > width {
		at = title[..at as usize].rfind(' ').map(|index| index as i64).unwrap_or(0);
	}
	if at <= 0 {
		let content = fit_text(title, width, size);
		let kept = if content.ends_with('…') { content.chars().count() - 1 } else { content.chars().count() };
		return vec![TitleLine { content, start: 0, kept }];
	}
	let rest = &title[at as usize + 1..];
	let second = fit_text(rest, width, size);
	let kept = if second == rest { rest.chars().count() } else { second.chars().count().saturating_sub(1) };
	vec![
		TitleLine { content: title[..at as usize].to_string(), start: 0, kept: at as usize },
		TitleLine { content: second, start: at as usize + 1, kept },
	]
}

fn is_js_word(ch: char) -> bool {
	ch.is_ascii_alphanumeric() || ch == '_'
}

/// The stretch of `title` holding its key figure, as `[start, end)`.
fn title_figure(title: &str) -> Option<(usize, usize)> {
	let chars: Vec<char> = title.chars().collect();
	let mut found: Vec<(usize, usize, String)> = Vec::new();
	let mut at = 0;
	while at < chars.len() {
		match match_figure(&chars, at) {
			Some((end, content)) => {
				found.push((at, end, content));
				at = end;
			}
			None => at += 1,
		}
	}
	let pick = found
		.iter()
		.find(|(_, _, content)| content.contains('×'))
		.or_else(|| {
			found.iter().find(|(_, _, content)| {
				(content.starts_with('+') || content.starts_with('−') || content.starts_with('-')) && content.ends_with('%')
			})
		})
		.or_else(|| found.iter().find(|(start, _, _)| *start > 0 && chars[start - 1] == '('))
		.or_else(|| {
			found.iter().find(|(_, _, content)| {
				let last = content.chars().last().unwrap_or(' ');
				(!last.is_ascii_digit() && last != ',' && last != '.' && !last.is_whitespace() && last != '–' && last != '-')
					|| content.starts_with('$')
					|| content.starts_with('€')
					|| content.starts_with('£')
			})
		});
	pick.map(|(start, end, _)| (*start, *end))
}

fn match_figure(chars: &[char], start: usize) -> Option<(usize, String)> {
	let len = chars.len();
	let mut i = start;
	if i > 0 {
		let prev = chars[i - 1];
		if is_js_word(prev) || prev == '.' || prev == '-' || prev == '−' {
			return None;
		}
	}
	if i < len && (chars[i] == '+' || chars[i] == '−' || chars[i] == '-') {
		i += 1;
	}
	if i < len && (chars[i] == '$' || chars[i] == '€' || chars[i] == '£') {
		i += 1;
	}
	if i >= len || !chars[i].is_ascii_digit() {
		return None;
	}
	while i < len && (chars[i].is_ascii_digit() || chars[i] == ',') {
		i += 1;
	}
	if i + 1 < len && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
		i += 1;
		while i < len && chars[i].is_ascii_digit() {
			i += 1;
		}
	}
		let _after_number = i;
	let mut j = i;
	if j < len && chars[j].is_whitespace() {
		j += 1;
	}
	if j < len && (chars[j] == '–' || chars[j] == '-' || chars[j] == '/') {
		j += 1;
		if j < len && chars[j].is_whitespace() {
			j += 1;
		}
		if j < len && chars[j].is_ascii_digit() {
			while j < len && (chars[j].is_ascii_digit() || chars[j] == ',') {
				j += 1;
			}
			if j + 1 < len && chars[j] == '.' && chars[j + 1].is_ascii_digit() {
				j += 1;
				while j < len && chars[j].is_ascii_digit() {
					j += 1;
				}
			}
			i = j;
		}
	}
	let after_range = i;
	let mut k = i;
	let mut matched = false;
	if k < len && chars[k] == '×' {
		k += 1;
		let mut m = k;
		if m < len && chars[m].is_whitespace() {
			m += 1;
		}
		if m + 4 <= len && chars[m..m + 4].iter().collect::<String>() == "more" {
			k = m + 4;
		} else if m + 4 <= len && chars[m..m + 4].iter().collect::<String>() == "less" {
			k = m + 4;
		}
		matched = true;
	} else if k < len && chars[k] == '%' {
		k += 1;
		matched = true;
	} else {
		let mut u = k;
		if u < len && chars[u].is_whitespace() {
			u += 1;
		}
		let word: String = chars[u..(u + 5).min(len)].iter().collect();
		if word.starts_with("tok/s") {
			u += 5;
			matched = true;
		} else if chars[u..(u + 3).min(len)].iter().collect::<String>() == "min" {
			u += 3;
			matched = true;
		} else if u < len
			&& (chars[u] == 'n' || chars[u] == 'µ' || chars[u] == 'm')
			&& u + 1 < len
			&& chars[u + 1] == 's'
		{
			u += 2;
			matched = true;
		} else if u < len && chars[u] == 's' {
			u += 1;
			matched = true;
		} else if u < len && chars[u] == 'h' {
			u += 1;
			matched = true;
		} else if u + 2 < len && "KMGT".contains(chars[u]) && chars[u + 1] == 'i' && chars[u + 2] == 'B' {
			u += 3;
			matched = true;
		} else if u + 1 < len && "KMGT".contains(chars[u]) && chars[u + 1] == 'B' {
			u += 2;
			matched = true;
		} else if u < len && chars[u] == 'B' {
			u += 1;
			matched = true;
		} else if u < len && chars[u] == 'k' {
			u += 1;
			matched = true;
		} else if u < len && chars[u] == 'M' {
			u += 1;
			matched = true;
		}
		if matched {
			if u < len && is_js_word(chars[u]) {
				matched = false;
			} else {
				k = u;
			}
		}
	}
	if matched {
		i = k;
	} else {
		i = after_range;
	}
	let content: String = chars[start..i].iter().collect();
	if content.is_empty() {
		return None;
	}
	Some((i, content))
}

fn draw_chart(spec: &ChartSpec) -> Layer {
	match spec.kind {
		ChartKind::Line => draw_line(spec),
		ChartKind::Heatmap => draw_grid(spec, true),
		ChartKind::Multiples => {
			if spec.series.len() > PANELS_ACROSS && spec.categories.len() <= SERIES.len() {
				draw_panels(spec)
			} else {
				draw_grid(spec, false)
			}
		}
		ChartKind::Change => draw_change(spec),
		ChartKind::Share => draw_share(spec),
		ChartKind::Scatter => draw_scatter(spec),
		ChartKind::Stacked => draw_stacked(spec),
		ChartKind::Waterfall => draw_waterfall(spec),
		ChartKind::Timeline => draw_timeline(spec),
		ChartKind::Dots => draw_dots(spec),
		ChartKind::Range => draw_range(spec),
		ChartKind::Slope => draw_slope(spec),
		ChartKind::Butterfly => draw_butterfly(spec),
		ChartKind::Bullet => draw_bullet(spec),
		ChartKind::Ladder => draw_ladder(spec),
		ChartKind::Histogram => draw_histogram(spec),
		ChartKind::Stat => draw_stat(spec),
		_ => draw_bars(spec),
	}
}

/// Row and bar heights for `count` categories.
fn bar_metrics(count: usize, grouped: usize) -> (f64, f64, f64) {
	if grouped > 1 {
		if count > DENSE_ROWS {
			return (grouped as f64 * 9.0 + 9.0, 7.0, 9.0);
		}
		let bar = if count <= 6 { 10.0 } else { 8.0 };
		return (grouped as f64 * (bar + 2.0) + 10.0, bar, bar + 2.0);
	}
	let bar = if count <= 5 {
		18.0
	} else if count <= 10 {
		16.0
	} else if count <= 20 {
		14.0
	} else {
		11.0
	};
	(bar + if count <= 10 { 10.0 } else { 8.0 }, bar, 0.0)
}

/// A value as a dot, or a range as a capsule spanning it, on `scale`.
fn mark(scale: &Scale, point: &ChartPoint, y: f64, radius: f64, fill: &str, opacity: f64, hollow: bool) -> String {
	let from = scale.at(point.value);
	let to = point.upper.map_or(from, |upper| scale.at(upper));
	if (to - from).abs() < 1.0 {
		return if hollow { ring(from, y, radius - 0.75, fill) } else { dot(from, y, radius, fill, opacity) };
	}
	let x = from.min(to) - radius;
	let width = (to - from).abs() + 2.0 * radius;
	if !hollow {
		return rect(x, y - radius, width, 2.0 * radius, fill, opacity, radius);
	}
	let inset = 0.875;
	format!(
		"<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.75\"/>",
		num(x + inset),
		num(y - radius + inset),
		num(width - 2.0 * inset),
		num(2.0 * radius - 2.0 * inset),
		num(radius - inset),
		token(fill)
	)
}

/// The spread a `± e` point states: a hairline with ticks either side.
fn whisker(scale: &Scale, point: &ChartPoint, y: f64, half: f64, stroke: &str) -> String {
	let error = point.error.unwrap_or(0.0);
	let from = scale.at(point.value - error);
	let to = scale.at(point.value + error);
	if to - from < 2.0 {
		return String::new();
	}
	format!(
		"{}{}{}",
		line(from, y, to, y, stroke, 1.25, 0.8),
		line(from, y - half, from, y + half, stroke, 1.25, 0.8),
		line(to, y - half, to, y + half, stroke, 1.25, 0.8)
	)
}

/// The reference annotations of `spec` (a stated budget, price or 1×).
fn reference_marks(spec: &ChartSpec) -> Vec<ChartAnnotation> {
	spec.annotations
		.as_ref()
		.map(|marks| marks.iter().filter(|mark| matches!(mark, ChartAnnotation::Reference { .. })).cloned().collect())
		.unwrap_or_default()
}

/// Dashed reference rules at `marks` across `top`…`bottom`, each labelled above the plot.
fn reference_lines(marks: &[ChartAnnotation], scale: &Scale, left: f64, right: f64, top: f64, bottom: f64) -> String {
	let mut parts: Vec<String> = Vec::new();
	for mark in marks {
		let (value, label) = match mark {
			ChartAnnotation::Reference { value, label, .. } => (*value, label.clone()),
			_ => continue,
		};
		let x = scale.at(value);
		if x < left - 0.5 || x > right + 0.5 {
			continue;
		}
		parts.push(reference_rule(x, top - 2.0, x, bottom));
		let label = clip(&label, MAX_CALLOUT_CHARS);
		let half = measure(&label, 10.0) / 2.0;
		let mut style = TextStyle::new("fg");
		style.size = Some(10.0);
		style.anchor = Some(Anchor::Middle);
		parts.push(text((x.max(left + half)).min(right - half), top - 9.0, &label, &style));
	}
	parts.join("")
}

/// First and last category index of the group holding category `at`.
fn group_span(spec: &ChartSpec, at: usize) -> (usize, usize) {
	let mut first = 0;
	for group in spec.groups.as_deref().unwrap_or(&[]) {
		let last = first + group.members.len() - 1;
		if at <= last {
			return (first, last);
		}
		first = last + 1;
	}
	(0, spec.categories.len().saturating_sub(1))
}

/// Reference rules at each reference category's value of `entry`, through its group's rows.
fn reference_rules(spec: &ChartSpec, entry: &ChartSeries, tops: &[f64], row_height: f64, scale: &Scale) -> (String, Vec<Option<f64>>) {
	let mut parts: Vec<String> = Vec::new();
	let mut xs: Vec<Option<f64>> = vec![None; spec.categories.len()];
	for index in spec.reference.as_deref().unwrap_or(&[]) {
		let Some(point) = entry.points.get(*index).and_then(|point| point.as_ref()) else { continue };
		let (first, last) = group_span(spec, *index);
		let x = scale.at(point.value);
		for slot in xs.iter_mut().take(last + 1).skip(first) {
			*slot = Some(x);
		}
		parts.push(reference_rule(x, tops[first] + 2.0, x, tops[last] + row_height - 2.0));
	}
	(parts.join(""), xs)
}

/// Each name as the lines its label sets in.
fn label_lines(names: &[String], wrap: bool) -> Vec<Vec<String>> {
	let wide = (WIDE_LABEL_WIDTH / (LABEL_SIZE * ADVANCE)).floor() as usize;
	let clipped = clip_names(names, wide);
	names
		.iter()
		.enumerate()
		.map(|(at, name)| {
			if name.chars().count() <= MAX_LABEL_CHARS || measure(name, LABEL_SIZE) <= WIDE_LABEL_WIDTH {
				return vec![name.clone()];
			}
			if !wrap {
				return vec![fit_text(&clipped[at], WIDE_LABEL_WIDTH, LABEL_SIZE)];
			}
			let lines = wrap_label(name, LABEL_WIDTH, WRAP_SIZE);
			if lines.len() > 1 {
				lines
			} else {
				vec![fit_text(name, LABEL_WIDTH, LABEL_SIZE)]
			}
		})
		.collect()
}

/// `name` over two lines of at most `width` at `size`, split where they balance best.
fn wrap_label(name: &str, width: f64, size: f64) -> Vec<String> {
	let chars: Vec<char> = name.chars().collect();
	let mut best: Option<(Vec<String>, f64)> = None;
	for at in 1..chars.len() {
		let before = chars[at - 1];
		let after = chars[at];
		let space = before == ' ';
		let camel = before.is_ascii_lowercase() && after.is_ascii_uppercase();
		if !space && !camel && !"/._-,:+".contains(before) && after != '(' {
			continue;
		}
		let first: String = chars[..at].iter().collect::<String>().trim_end().to_string();
		let second: String = chars[at..].iter().collect::<String>().trim_start().to_string();
		let first_width = measure(&first, size);
		if first.is_empty() || second.is_empty() || first_width > width {
			continue;
		}
		let cost = first_width.max(measure(&second, size)) + if space { 0.0 } else { 8.0 };
		if best.as_ref().map_or(true, |(_, seen)| cost < *seen) {
			best = Some((vec![first, fit_text(&second, width, size)], cost));
		}
	}
	best.map(|(lines, _)| lines).unwrap_or_else(|| vec![fit_text(name, width, size)])
}

/// Each category's label lines: its own name, or its member name beside a group.
fn category_lines(spec: &ChartSpec) -> Vec<Vec<String>> {
	let names: Vec<String> = match &spec.groups {
		Some(groups) => groups.iter().flat_map(|group| group.members.clone()).collect(),
		None => spec.categories.clone(),
	};
	label_lines(&names, spec.categories.len() <= MAX_WRAP_ROWS)
}

/// `row` grown to `WRAP_ROW` where a category's name wraps.
fn label_row(spec: &ChartSpec, row: f64) -> f64 {
	if row < WRAP_ROW && category_lines(spec).iter().any(|lines| lines.len() > 1) {
		WRAP_ROW
	} else {
		row
	}
}

/// A label's lines from `x`, centred on `y` as a block.
fn label_text(x: f64, y: f64, lines: &[String], style: &TextStyle) -> String {
	if lines.len() == 1 {
		return text(x, y, &lines[0], style);
	}
	lines
		.iter()
		.enumerate()
		.map(|(at, line)| {
			let mut own = style.clone_style();
			own.size = Some(WRAP_SIZE);
			text(x, y + (at as f64 - (lines.len() - 1) as f64 / 2.0) * WRAP_LINE, line, &own)
		})
		.collect::<Vec<String>>()
		.join("")
}

/// Category names down the label column, centred on their rows from `tops`.
fn category_labels(spec: &ChartSpec, tops: &[f64], row_height: f64, strong: &HashSet<usize>) -> String {
	let middle = |at: usize| tops[at] + row_height / 2.0;
	let status = spec.status.as_deref();
	let matrix = matrix_dots(spec, &middle);
	let badges = if status.map_or(false, |states| states.iter().any(|state| state.is_some())) { STATUS_WIDTH } else { 0.0 };
	let badge = |x: f64, at: usize| -> String {
		match status.and_then(|states| states[at]) {
			Some(state) => dot(x + 4.0, middle(at), 4.0, status_color(state), 1.0),
			None => String::new(),
		}
	};
	let lines = category_lines(spec);
	let Some(groups) = spec.groups.as_ref() else {
		let mut out = matrix;
		for (at, name) in lines.iter().enumerate() {
			let mut style = TextStyle::new("fg");
			style.weight = if strong.contains(&at) { Some(600) } else { None };
			out.push_str(&badge(PAD, at));
			out.push_str(&label_text(PAD + badges, middle(at), name, &style));
		}
		return out;
	};
	let member_x = PAD + group_name_width(groups) + 10.0;
	let heads = clip_names(&groups.iter().map(|group| group.name.clone()).collect::<Vec<String>>(), group_chars(groups));
	let mut parts: Vec<String> = vec![matrix];
	let mut at = 0;
	for (index, group) in groups.iter().enumerate() {
		let mut style = TextStyle::new("fg");
		style.weight = Some(600);
		parts.push(text(PAD, middle(at), &heads[index], &style));
		let end = at + group.members.len();
		while at < end {
			let mut style = TextStyle::new("fg");
			style.weight = if strong.contains(&at) { Some(600) } else { None };
			parts.push(badge(member_x, at));
			parts.push(label_text(member_x + badges, middle(at), &lines[at], &style));
			at += 1;
		}
	}
	parts.join("")
}

/// Each matrix column's heading, wrapped, and the width it takes.
fn matrix_columns(spec: &ChartSpec) -> Vec<MatrixColumn> {
	spec.matrix
		.as_deref()
		.unwrap_or(&[])
		.iter()
		.map(|column| {
			let lines = wrap_header(&column.name, MATRIX_HEAD_CHARS as f64 * TICK_SIZE * ADVANCE, TICK_SIZE);
			let width = lines.iter().map(|line| measure(line, TICK_SIZE)).fold(STATUS_WIDTH, f64::max) + 12.0;
			MatrixColumn { lines, width }
		})
		.collect()
}

/// Each matrix column's centre; the matrix closes the label column.
fn matrix_centres(spec: &ChartSpec, columns: &[MatrixColumn]) -> Vec<f64> {
	let mut x = PAD + label_column_width(spec) - columns.iter().map(|column| column.width).sum::<f64>();
	columns
		.iter()
		.map(|column| {
			x += column.width;
			x - column.width / 2.0
		})
		.collect()
}

/// The matrix columns' headings, a muted row between the header and the plot.
fn matrix_head(spec: &ChartSpec) -> (f64, String) {
	let columns = matrix_columns(spec);
	if columns.is_empty() {
		return (0.0, String::new());
	}
	let lines = columns.iter().map(|column| column.lines.len()).max().unwrap_or(0);
	let centres = matrix_centres(spec, &columns);
	let mut body: Vec<String> = Vec::new();
	for (index, column) in columns.iter().enumerate() {
		for (at, line) in column.lines.iter().enumerate() {
			let mut style = TextStyle::new("muted");
			style.size = Some(TICK_SIZE);
			style.anchor = Some(Anchor::Middle);
			body.push(text(centres[index], (lines - column.lines.len() + at) as f64 * MATRIX_LINE + 6.0, line, &style));
		}
	}
	if spec.series.len() == 1 {
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		body.push(text(PAD + label_column_width(spec) + 10.0, (lines - 1) as f64 * MATRIX_LINE + 6.0, &clip(&spec.series[0].name, 40), &style));
	}
	(lines as f64 * MATRIX_LINE + 4.0, body.join(""))
}

/// The matrix's dots, closing the label column.
fn matrix_dots(spec: &ChartSpec, middle: &dyn Fn(usize) -> f64) -> String {
	let columns = matrix_columns(spec);
	let centres = matrix_centres(spec, &columns);
	let mut out = String::new();
	for (index, column) in spec.matrix.as_deref().unwrap_or(&[]).iter().enumerate() {
		for (at, state) in column.statuses.iter().enumerate() {
			match state {
				Some(state) => out.push_str(&dot(centres[index], middle(at), 4.5, status_color(*state), 1.0)),
				None => out.push_str(&dot(centres[index], middle(at), 1.5, "muted", 0.6)),
			}
		}
	}
	out
}

/// Characters a group name may take.
fn group_chars(groups: &[ChartGroup]) -> usize {
	let widest = groups
		.iter()
		.flat_map(|group| group.members.iter())
		.map(|member| member.chars().count().min(MAX_LABEL_CHARS))
		.max()
		.unwrap_or(0);
	MAX_GROUP_CHARS.max(MAX_LABEL_CHARS + MAX_GROUP_CHARS - widest)
}

/// Top of each category's row from `top`, a `GROUP_GAP` parting consecutive groups.
fn row_tops(spec: &ChartSpec, top: f64, row_height: f64) -> Vec<f64> {
	let sizes: Vec<usize> = match spec.groups.as_deref() {
		Some(groups) => groups.iter().map(|group| group.members.len()).collect(),
		None => vec![spec.categories.len()],
	};
	let mut tops: Vec<f64> = Vec::new();
	let mut y = top;
	for (index, size) in sizes.iter().enumerate() {
		if index > 0 {
			y += GROUP_GAP;
		}
		for _ in 0..*size {
			tops.push(y);
			y += row_height;
		}
	}
	tops.push(y);
	tops
}

/// Width of the category label column.
fn label_column_width(spec: &ChartSpec) -> f64 {
	let widest = category_lines(spec)
		.iter()
		.flat_map(|lines| {
			let size = if lines.len() > 1 { WRAP_SIZE } else { LABEL_SIZE };
			lines.iter().map(move |line| measure(line, size))
		})
		.fold(0.0f64, f64::max);
	let groups = spec.groups.as_deref();
	let width = match groups {
		Some(groups) => group_name_width(groups) + 10.0 + widest,
		None => widest,
	};
	let matrix = matrix_columns(spec).iter().map(|column| column.width).sum::<f64>();
	(4.0 * LABEL_SIZE * ADVANCE).max(width) + if spec.status.as_ref().map_or(false, |states| states.iter().any(|state| state.is_some())) { STATUS_WIDTH } else { 0.0 } + matrix
}

/// Width of the widest semibold group name.
fn group_name_width(groups: &[ChartGroup]) -> f64 {
	let heads = clip_names(&groups.iter().map(|group| group.name.clone()).collect::<Vec<String>>(), group_chars(groups));
	heads.iter().map(|name| measure(name, LABEL_SIZE) * SEMIBOLD_WIDTH).fold(0.0f64, f64::max)
}

/// A column heading in one line of `width` at `size`, or two split where they balance.
fn wrap_header(name: &str, width: f64, size: f64) -> Vec<String> {
	if measure(name, size) <= width {
		vec![name.to_string()]
	} else {
		wrap_label(name, width, size)
	}
}

/// Width of one legend entry: its mark (and tone dots), name, and the gap before the next.
fn legend_item_width(item: &LegendItem) -> f64 {
	12.0 + 9.0 * item.tones.as_deref().map_or(0, |tones| tones.len()) as f64 + measure(&clip(&item.name, MAX_LEGEND_CHARS), TICK_SIZE) + 16.0
}

/// Width of `items` on one line, without the trailing gap.
fn legend_width(items: &[LegendItem]) -> f64 {
	items.iter().map(legend_item_width).sum::<f64>() - 16.0
}

/// Dot-and-name legend wrapping across `left`…`right`, its first line centred on `y`.
fn draw_legend(items: &[LegendItem], left: f64, right: f64, y: f64) -> Layer {
	let mut parts: Vec<String> = Vec::new();
	let labels = clip_names(&items.iter().map(|item| item.name.clone()).collect::<Vec<String>>(), MAX_LEGEND_CHARS);
	let mut x = left;
	let mut line_y = y;
	for (at, item) in items.iter().enumerate() {
		let width = legend_item_width(item);
		if x + width - 16.0 > right + 0.5 && x > left {
			x = left;
			line_y += LEGEND_LINE;
		}
		parts.push(if item.ring { ring(x + 4.0, line_y, 3.25, &item.color) } else { dot(x + 4.0, line_y, 4.0, &item.color, item.opacity.unwrap_or(1.0)) });
		let tones = item.tones.as_deref().unwrap_or(&[]);
		for (slot, tone) in tones.iter().enumerate() {
			parts.push(dot(x + 13.0 + slot as f64 * 9.0, line_y, 4.0, tone, item.opacity.unwrap_or(1.0)));
		}
		parts.push(text(x + 12.0 + 9.0 * tones.len() as f64, line_y, &labels[at], &TextStyle::new("muted")));
		x += width;
	}
	Layer::plain(line_y + 8.0, parts.join(""))
}

/// Width of a heat ramp key: low figure, five swatches, high figure.
fn ramp_width(ramp: &RampKey) -> f64 {
	measure(&ramp.low, 10.0) + 6.0 + 5.0 * 13.0 + 4.0 + measure(&ramp.high, 10.0)
}

/// A heat scale key from `x`, centred on `y`.
fn draw_ramp(ramp: &RampKey, x: f64, y: f64) -> String {
	let mut parts: Vec<String> = vec![text(x, y, &ramp.low, &TextStyle::new("muted"))];
	let mut style = TextStyle::new("muted");
	style.size = Some(10.0);
	parts[0] = text(x, y, &ramp.low, &style);
	let mut at = x + measure(&ramp.low, 10.0) + 6.0;
	for step in 0..5 {
		parts.push(rect(at, y - 5.0, 11.0, 10.0, &ramp.color, HEAT_FLOOR + (1.0 - HEAT_FLOOR) * step as f64 / 4.0, 2.0));
		at += 13.0;
	}
	parts.push(text(at + 4.0, y, &ramp.high, &style));
	parts.join("")
}

/// Categories a planner highlighted, in range.
fn highlighted(spec: &ChartSpec) -> Vec<usize> {
	spec.annotations
		.as_ref()
		.map(|marks| {
			marks
				.iter()
				.filter_map(|mark| match mark {
					ChartAnnotation::Highlight { category } if *category < spec.categories.len() => Some(*category),
					_ => None,
				})
				.collect()
		})
		.unwrap_or_default()
}

/// Categories whose labels set semibold: highlighted ones, and rows the table bolded a figure in.
fn strong_categories(spec: &ChartSpec) -> HashSet<usize> {
	let bold: Vec<usize> = (0..spec.categories.len())
		.filter(|at| spec.series.iter().any(|entry| entry.points.get(*at).and_then(|point| point.as_ref()).map_or(false, |point| point.emphasis)))
		.collect();
	let mut out = HashSet::new();
	out.extend(highlighted(spec));
	if bold.len() * 2 <= spec.categories.len() {
		out.extend(bold);
	}
	out
}

/// Callout text per category (the first per category wins).
fn callout_texts(spec: &ChartSpec) -> HashMap<usize, String> {
	let mut texts: HashMap<usize, String> = HashMap::new();
	for mark in spec.annotations.as_deref().unwrap_or(&[]) {
		if let ChartAnnotation::Callout { category, text, .. } = mark {
			if *category < spec.categories.len() && !texts.contains_key(category) {
				texts.insert(*category, clip(text, MAX_CALLOUT_CHARS));
			}
		}
	}
	texts
}

/// A factor as a move: `155× less`, `1.2× more` in the factor style, `−31%` in percent.
fn factor_move(factor: f64, style: MoveStyle) -> String {
	if factor == 0.0 {
		return if matches!(style, MoveStyle::Factor) { "to zero".to_string() } else { "−100%".to_string() };
	}
	let percent = ((factor - 1.0) * 100.0).round();
	if percent == 0.0 || (matches!(style, MoveStyle::Factor) && factor.abs().ln() < STILL.ln()) {
		return "same".to_string();
	}
	if matches!(style, MoveStyle::Factor) {
		if factor > 1.0 {
			format!("{}× more", compact_factor(factor))
		} else {
			format!("{}× less", compact_factor(1.0 / factor))
		}
	} else if percent > 0.0 {
		format!("+{}%", percent.abs())
	} else {
		format!("−{}%", percent.abs())
	}
}

/// `48`, `2.1`, `1.25`, `35k`: whole above 10, one decimal above 2, two below.
fn compact_factor(factor: f64) -> String {
	if factor >= 10.0 {
		let rounded = factor.round();
		return compact(rounded, rounded.abs());
	}
	let digits = if factor >= 2.0 { 1 } else { 2 };
	let rounded = format!("{:.*}", digits, factor);
	rounded.parse::<f64>().map(|parsed| format!("{parsed}")).unwrap_or(rounded)
}

/// A factor as people say it: `48× less`, `2.1× more`, or a percent within 2×.
fn factor_text(point: &ChartPoint) -> String {
	let style = if point.value >= 2.0 || (point.value > 0.0 && point.value <= 0.5) { MoveStyle::Factor } else { MoveStyle::Percent };
	factor_move(point.value, style)
}

/// `log10` of the first `FACTOR_STOPS` entry at or past `10^log`; 0 stays 0.
fn round_factor(log: f64) -> f64 {
	if log <= 0.0 {
		return 0.0;
	}
	match FACTOR_STOPS.iter().find(|factor| factor.log10() >= log - 1e-9) {
		Some(stop) => stop.log10(),
		None => (log - 1e-9).ceil(),
	}
}

/// A value's factor of its reference row, one way for a whole column.
fn reference_text(factor: f64, ratio: bool) -> String {
	if !ratio {
		return factor_move(factor, MoveStyle::Percent);
	}
	if factor > 0.0 && factor.abs().ln() < STILL.ln() {
		return "same".to_string();
	}
	if factor >= 10.0 {
		let rounded = factor.round();
		format!("{}×", compact(rounded, rounded.abs()))
	} else {
		let digits = if factor >= 1.0 { 1 } else { 2 };
		let rounded = format!("{:.*}", digits, factor);
		format!("{}×", rounded.parse::<f64>().map(|parsed| format!("{parsed}")).unwrap_or(rounded))
	}
}

fn widest(labels: &[String], size: f64) -> f64 {
	labels.iter().map(|label| measure(label, size)).fold(0.0f64, f64::max)
}

/// Horizontal bars: bar, grouped, paired, diverging, progress.
fn draw_bars(spec: &ChartSpec) -> Layer {
	let kind = spec.kind;
	let categories = &spec.categories;
	let series = &spec.series;
	let tracks = kind == ChartKind::Progress;
	let grouped = kind == ChartKind::Grouped || (tracks && series.len() > 1);
	let paired = kind == ChartKind::Paired;
	let single = series.len() == 1;
	let diverging = kind == ChartKind::Diverging && single;
	let values: Vec<f64> = if tracks {
		vec![0.0, 100.0]
	} else {
		series
			.iter()
			.flat_map(|entry| entry.points.iter())
			.flat_map(|point| match point {
				None => Vec::new(),
				Some(point) => {
					if let Some(upper) = point.upper {
						vec![point.value, upper]
					} else if let Some(error) = point.error {
						vec![point.value - error, point.value + error]
					} else {
						vec![point.value]
					}
				}
			})
			.collect()
	};
	let label_width = label_column_width(spec);
	let referenced: Vec<usize> = if kind == ChartKind::Bar { spec.reference.as_deref().unwrap_or(&[]).to_vec() } else { Vec::new() };
	let factors = if referenced.is_empty() { Vec::new() } else { reference_factors(spec, &series[0]) };
	let versus = |at: usize| -> String {
		let Some(Some(factor)) = factors.get(at) else { return String::new() };
		factor_text(&ChartPoint { value: *factor, text: String::new(), emphasis: false, upper: None, error: None })
	};
	let moves: Option<Vec<String>> = if paired && spec.transition == Some(true) { Some(pair_moves(&series[0], &series[1])) } else { None };
	let ratios: Option<Vec<String>> = if paired && spec.transition != Some(true) && log_spread(&values) {
		Some(
			categories
				.iter()
				.enumerate()
				.map(|(at, _)| {
					let a = series[0].points.get(at).and_then(|point| point.as_ref());
					let b = series[1].points.get(at).and_then(|point| point.as_ref());
					match (a, b) {
						(Some(a), Some(b)) if a.value > 0.0 && b.value > 0.0 => {
							let ratio = a.value.max(b.value) / a.value.min(b.value);
							if ratio >= 10.0 {
								format!("{}×", group_digits(&format!("{}", ratio.round() as i64)))
							} else {
								format!("{}×", (ratio * 10.0).round() / 10.0)
							}
						}
						_ => String::new(),
					}
				})
				.collect(),
		)
	} else {
		None
	};
	let value_texts: Vec<String> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| {
			if paired {
				let mut joined = [point_text(&series[0], at), point_text(&series[1], at)]
					.into_iter()
					.filter(|text| !text.is_empty())
					.collect::<Vec<String>>()
					.join(if spec.transition == Some(true) { " → " } else { " · " });
				if let Some(moves) = &moves {
					if let Some(move_text) = moves.get(at).filter(|move_text| !move_text.is_empty()) {
						joined.push_str(MOVE_GAP);
						joined.push_str(move_text);
					}
				}
				if let Some(ratios) = &ratios {
					if let Some(ratio) = ratios.get(at).filter(|ratio| !ratio.is_empty()) {
						joined.push_str(MOVE_GAP);
						joined.push_str(ratio);
					}
				}
				joined
			} else if single {
				[point_text(&series[0], at), versus(at)].into_iter().filter(|text| !text.is_empty()).collect::<Vec<String>>().join(" · ")
			} else {
				String::new()
			}
		})
		.collect();
	let callouts = callout_texts(spec);
	let callout_width = if callouts.is_empty() { 0.0 } else { widest(&callouts.values().cloned().collect::<Vec<String>>(), TICK_SIZE) * SEMIBOLD_WIDTH + 24.0 };
	let signed = |negative: bool| -> f64 {
		widest(
			&categories
				.iter()
				.enumerate()
				.filter_map(|(at, _)| {
					let point = series[0].points.get(at).and_then(|point| point.as_ref());
					point.filter(|point| (point.value < 0.0) == negative).map(|_| value_texts[at].clone())
				})
				.collect::<Vec<String>>(),
			TICK_SIZE,
		)
	};
	let left_room = if diverging && values.iter().any(|value| *value < 0.0) { signed(true) + 8.0 } else { 0.0 };
	let tagged = if series.iter().any(|entry| entry.best.is_some()) || spec.leaders.as_ref().map_or(false, |leaders| leaders.iter().any(|lead| lead.is_some())) {
		BEST_WIDTH
	} else {
		0.0
	};
	let value_width = (if diverging {
		signed(false) + 8.0
	} else if grouped {
		widest(
			&series
				.iter()
				.flat_map(|entry| entry.points.iter())
				.map(|point| clip(point.as_ref().map_or("", |point| point.text.as_str()), 12))
				.collect::<Vec<String>>(),
			10.0,
		) + 6.0
	} else {
		let cap = if moves.is_some() {
			MAX_MOVE_COLUMN.max((WIDTH - 2.0 * PAD - label_width - 10.0) * FIGURE_SHARE)
		} else {
			180.0
		};
		cap.min(widest(&value_texts, TICK_SIZE) + 8.0)
	}) + if paired { 0.0 } else { tagged };
	let left = PAD + label_width + 10.0 + left_room;
	let right = WIDTH - PAD - value_width - callout_width;
	let tier = if diverging { outlier_tier(&values.iter().map(|value| value.abs()).collect::<Vec<f64>>(), f64::INFINITY) } else { None };
	let limit = tier.as_ref().map_or(f64::INFINITY, |tier| tier.rest * CLIP_REACH);
	let clamp = move |value: f64| value.max(-limit).min(limit);
	let clustered = paired && values.iter().copied().fold(f64::INFINITY, f64::min) >= ZOOM_SHARE * values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
	let axis = value_scale(
		&if tier.is_some() { values.iter().map(|value| clamp(*value)).collect::<Vec<f64>>() } else { values.clone() },
		left,
		right,
		!clustered,
		kind == ChartKind::Bar || kind == ChartKind::Grouped,
		tick_unit(&series[0], &values),
	);
	let scale = match &tier {
		Some(_) => {
			let ticks = axis.ticks.clone();
			let log = axis.log;
			let cut = axis.cut.clone();
			Scale::new(Box::new(move |value| axis.at(clamp(value))), ticks, log, cut)
		}
		None => axis,
	};
	let (row, bar_height, step) = bar_metrics(categories.len(), if grouped { series.len() } else { 1 });
	let row_height = label_row(spec, row);
	let references = reference_marks(spec);
	let top = if references.is_empty() { 0.0 } else { 16.0 };
	let tops = row_tops(spec, top, row_height);
	let grid_top = top;
	let grid_bottom = *tops.last().unwrap_or(&0.0);
	let strong = strong_categories(spec);
	let mut parts: Vec<String> = Vec::new();
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	parts.push(draw_value_axis(&scale, &series[0], grid_top, grid_bottom));
	parts.push(category_labels(spec, &tops, row_height, &strong));
	parts.push(reference_lines(&references, &scale, left, right, grid_top, grid_bottom));
	let zero = if scale.log { left } else { scale.at(0.0) };
	let reference = if referenced.is_empty() { None } else { Some(reference_rules(spec, &series[0], &tops, row_height, &scale)) };
	let emphasized = series.iter().any(|entry| entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis)));
	let estimates: Vec<HashSet<usize>> = series.iter().map(|entry| estimated_points(entry)).collect();
	let plotted = series.iter().flat_map(|entry| entry.points.iter()).filter(|point| point.is_some()).count();
	let quiet_zeros = grouped
		&& categories.len() > DENSE_ROWS
		&& series
			.iter()
			.flat_map(|entry| entry.points.iter())
			.filter(|point| point.as_ref().map_or(false, |point| point.value == 0.0))
			.count() as f64
			>= ZERO_QUIET * plotted as f64;
	let polarity = if paired && spec.transition == Some(true) { series[1].polarity.or(series[0].polarity) } else { None };
	let toned = polarity.is_some()
		&& categories.iter().enumerate().any(|(at, _)| {
			let a = series[0].points.get(at).and_then(|point| point.as_ref());
			let b = series[1].points.get(at).and_then(|point| point.as_ref());
			a.zip(b).map_or(false, |(a, b)| pair_tone(&series[0], &series[1], a, b).is_some())
		});
	for (at, _) in categories.iter().enumerate() {
		let row_top = tops[at];
		let middle = row_top + row_height / 2.0;
		if paired {
			let a = series[0].points.get(at).and_then(|point| point.as_ref());
			let b = series[1].points.get(at).and_then(|point| point.as_ref());
			let radius = if bar_height >= 14.0 { 5.0 } else { 4.0 };
			let center = |point: &ChartPoint| (scale.at(point.value) + scale.at(point.upper.unwrap_or(point.value))) / 2.0;
			let tone = if spec.transition == Some(true) { a.zip(b).and_then(|(a, b)| pair_tone(&series[0], &series[1], a, b)) } else { None };
			let end = tone.unwrap_or(if toned { "muted" } else { SERIES[0] });
			if let (Some(a), Some(b)) = (a, b) {
				let (from, to) = (center(a), center(b));
				if spec.transition != Some(true) {
					parts.push(line(from, middle, to, middle, "border", 2.5, 1.0));
				} else {
					parts.push(rect(from.min(to), middle - 1.5, (to - from).abs().max(0.5), 3.0, end, MOVE_OPACITY, 1.5));
				}
			}
			let still = a.zip(b).map_or(false, |(a, b)| (center(a) - center(b)).abs() < radius);
			if let Some(a) = a {
				if !(spec.transition == Some(true) && still) {
					parts.push(mark(&scale, a, middle, radius, if spec.transition == Some(true) { "muted" } else { SERIES[0] }, 1.0, spec.transition == Some(true)));
				}
			}
			if let Some(b) = b {
				parts.push(mark(&scale, b, middle, radius, if spec.transition == Some(true) { end } else { SERIES[1] }, 1.0, false));
			}
			if let Some(a) = a {
				if spec.transition == Some(true) && still {
					parts.push(ring(center(a), middle, radius + 2.5, "muted"));
				}
			}
			let strong_pair = a.map_or(false, |point| point.emphasis) || b.map_or(false, |point| point.emphasis);
			let from_text = point_text(&series[0], at);
			let to_text = point_text(&series[1], at);
			if spec.transition == Some(true) && !from_text.is_empty() && !to_text.is_empty() {
				let move_text = moves.as_ref().and_then(|moves| moves.get(at)).filter(|move_text| !move_text.is_empty());
				let mut kept_text = to_text.clone();
				if let Some(move_text) = move_text {
					kept_text.push_str(MOVE_GAP);
					kept_text.push_str(move_text);
				}
				let kept = measure(&kept_text, TICK_SIZE) * SEMIBOLD_WIDTH;
				let room = value_width - 8.0 - kept - measure(" → ", TICK_SIZE);
				let fits = measure(&value_texts[at], TICK_SIZE) + 8.0 <= value_width;
				let mut head = if fits { from_text.clone() } else { fit_text(&from_text, room, TICK_SIZE) };
				head.push_str(" → ");
				let tail = right + 8.0 + measure(&head, TICK_SIZE);
				let mut style = TextStyle::new("muted");
				style.size = Some(TICK_SIZE);
				parts.push(text(right + 8.0, middle, &head, &style));
				let mut style = TextStyle::new("fg");
				style.size = Some(TICK_SIZE);
				style.weight = if strong_pair { Some(600) } else { None };
				parts.push(text(tail, middle, &to_text, &style));
				if let Some(move_text) = move_text {
					let mut prefix = to_text.clone();
					prefix.push_str(MOVE_GAP);
					let mut style = TextStyle::new(tone.unwrap_or("muted"));
					style.size = Some(TICK_SIZE);
					style.weight = if tone.is_some() { Some(600) } else { None };
					parts.push(text(tail + measure(&prefix, TICK_SIZE), middle, move_text, &style));
				}
			} else {
				let mut runs: Vec<TextRun> = Vec::new();
				if !from_text.is_empty() {
					runs.push(TextRun {
						content: from_text,
						fill: Some(if spec.transition == Some(true) { "muted".to_string() } else { SERIES[0].to_string() }),
						weight: None,
					});
				}
				if !to_text.is_empty() {
					if !runs.is_empty() {
						runs.push(TextRun { content: " · ".to_string(), fill: None, weight: None });
					}
					runs.push(TextRun { content: to_text, fill: Some(SERIES[1].to_string()), weight: None });
				}
				if let Some(ratio) = ratios.as_ref().and_then(|ratios| ratios.get(at)).filter(|ratio| !ratio.is_empty()) {
					runs.push(TextRun { content: format!("\u{2002}{ratio}"), fill: Some("fg".to_string()), weight: None });
				}
				let mut style = TextStyle::new("muted");
				style.size = Some(TICK_SIZE);
				style.weight = if strong_pair { Some(600) } else { None };
				parts.push(text_runs(right + 8.0, middle, &runs, &style));
			}
			continue;
		}
		for (index, entry) in series.iter().enumerate() {
			let point = entry.points.get(at).and_then(|point| point.as_ref());
			let y = if grouped { row_top + 5.0 + index as f64 * step } else { middle - bar_height / 2.0 };
			let context = emphasized && point.map_or(true, |point| !point.emphasis);
			let gain = if diverging { point.and_then(|point| move_tone(entry.polarity, if point.value < 0.0 { 0.5 } else { 2.0 })) } else { None };
			let fill = if single && context {
				"muted"
			} else {
				gain.unwrap_or(if diverging && point.map_or(false, |point| point.value < 0.0) { SERIES[1] } else { SERIES[index % SERIES.len()] })
			};
			let opacity = if context { if single { 0.55 } else { RECEDE } } else { 1.0 };
			if tracks {
				parts.push(rect(left, y, scale.at(100.0) - left, bar_height, "border", 0.35, RADIUS));
			}
			let Some(point) = point else {
				if let Some(note) = entry.notes.get(at).and_then(|note| note.clone()) {
					parts.push(note_text(left + 4.0, y + bar_height / 2.0, &clip(&note, 24), false));
				}
				continue;
			};
			let end = scale.at(point.value);
			if scale.log {
				parts.push(line(left, y + bar_height / 2.0, end, y + bar_height / 2.0, fill, 2.0, 0.4 * opacity));
				parts.push(mark(&scale, point, y + bar_height / 2.0, (bar_height / 2.6).min(6.0), fill, opacity, false));
			} else if let Some(cut) = &scale.cut {
				if point.value > cut.below {
					parts.push(cut_bar(cut, zero, end, y, bar_height, fill, opacity));
				} else {
					parts.push(bar_rect(zero.min(end), y, (end - zero).abs().max(MIN_BAR), bar_height, fill, opacity, estimates[index].contains(&at)));
				}
			} else if point.value.abs() > limit {
				let toward = (zero - end).signum();
				let cut = end + toward * CLIP_INSET;
				parts.push(rect(end.min(cut - toward * (CUT_GAP / 2.0)), y, CLIP_INSET - CUT_GAP / 2.0, bar_height, fill, opacity, RADIUS));
				parts.push(rect(zero.min(cut + toward * (CUT_GAP / 2.0)), y, (zero - cut).abs() - CUT_GAP / 2.0, bar_height, fill, opacity, RADIUS));
				parts.push(break_mark(cut, y + bar_height / 2.0, bar_height / 2.0 + 1.0, "muted"));
			} else {
				parts.push(bar_rect(zero.min(end), y, (end - zero).abs().max(MIN_BAR), bar_height, fill, opacity, estimates[index].contains(&at)));
				if let Some(upper) = point.upper {
					if upper > point.value {
						parts.push(rect(end, y, scale.at(upper) - end, bar_height, fill, RANGE_OPACITY * opacity, RADIUS));
					}
				}
			}
			if point.error.is_some() {
				parts.push(whisker(&scale, point, y + bar_height / 2.0, (bar_height / 2.0 - 1.0).max(3.0), "fg"));
			}
			if grouped {
				let label_x = if tracks {
					scale.at(100.0) + 4.0
				} else {
					let reach = point.upper.unwrap_or(point.value + point.error.unwrap_or(0.0));
					end.max(scale.at(reach)) + 4.0
				};
				let best = entry.best == Some(at) || spec.leaders.as_ref().and_then(|leaders| leaders.get(at)).copied().flatten() == Some(index);
				let label = if quiet_zeros && point.value == 0.0 && !best { String::new() } else { clip(&point.text, 12) };
				let size = if step < 10.0 { 9.0 } else { 10.0 };
				let mut style = TextStyle::new(if point.emphasis || best { "fg" } else { "muted" });
				style.size = Some(size);
				style.weight = if point.emphasis || best { Some(600) } else { None };
				parts.push(text(label_x, y + bar_height / 2.0, &label, &style));
				if best {
					parts.push(best_tag(label_x + measure(&label, size) * SEMIBOLD_WIDTH + 4.0, y + bar_height / 2.0, size));
				}
			}
		}
		if single {
			let point = series[0].points.get(at).and_then(|point| point.as_ref());
			let Some(point) = point else { continue };
			let mut end = scale.at(point.value);
			if let Some(upper) = point.upper {
				end = end.max(scale.at(upper));
			}
			if let Some(error) = point.error {
				end = end.max(scale.at(point.value + error));
			}
			let label = value_texts[at].clone();
			let best = series[0].best == Some(at);
			let mut style = TextStyle::new(if point.emphasis || best { "fg" } else { "muted" });
			style.size = Some(TICK_SIZE);
			style.weight = if point.emphasis || best { Some(600) } else { None };
			if diverging && point.value < 0.0 {
				style.anchor = Some(Anchor::End);
				parts.push(text(scale.at(point.value) - 6.0, middle, &label, &style));
			} else {
				let x = if tracks {
					right + 8.0
				} else {
					let reference_x = reference.as_ref().and_then(|(_, ats)| ats.get(at).copied().flatten()).unwrap_or(zero);
					end.max(zero).max(reference_x) + if scale.log { 10.0 } else { 6.0 }
				};
				parts.push(text(x, middle, &label, &style));
				if best {
					parts.push(best_tag(x + measure(&label, TICK_SIZE) * SEMIBOLD_WIDTH + 6.0, middle, TICK_SIZE));
				}
			}
		}
	}
	if diverging && !scale.log {
		parts.push(line(zero, grid_top - 4.0, zero, grid_bottom + 4.0, "fg", 1.25, 0.6));
	}
	if let Some((body, _)) = &reference {
		parts.push(body.clone());
	}
	for (at, note) in callouts.iter() {
		let middle = tops[*at] + row_height / 2.0;
		let point = series[0].points.get(*at).and_then(|point| point.as_ref());
		let reach = if paired {
			right + 8.0 + measure(&value_texts[*at], TICK_SIZE)
		} else if point.is_some() && single && !tracks {
			let reference_x = reference.as_ref().and_then(|(_, ats)| ats.get(*at).copied().flatten()).unwrap_or(zero);
			scale.at(point.unwrap().value).max(zero).max(reference_x) + if scale.log { 10.0 } else { 6.0 } + measure(&value_texts[*at], TICK_SIZE)
		} else {
			right + 8.0 + if single { measure(&value_texts[*at], TICK_SIZE) } else { 0.0 }
		};
		parts.push(callout(reach + 6.0, middle, WIDTH - PAD - callout_width + 18.0, note));
	}
	let end_tones: Vec<&'static str> = if toned {
		["success", "error", "muted"]
			.into_iter()
			.filter(|color| {
				categories.iter().enumerate().any(|(at, _)| {
					let a = series[0].points.get(at).and_then(|point| point.as_ref());
					let b = series[1].points.get(at).and_then(|point| point.as_ref());
					let tone = match (a, b) {
						(Some(a), Some(b)) => pair_tone(&series[0], &series[1], a, b),
						_ => None,
					};
					tone == if *color == "muted" { None } else { Some(*color) }
				})
			})
			.collect()
	} else {
		Vec::new()
	};
	let legend: Vec<LegendItem> = if single && !paired {
		Vec::new()
	} else if paired && spec.transition == Some(true) {
		vec![
			LegendItem { name: series[0].name.clone(), color: "muted".to_string(), ring: true, tones: None, opacity: None },
			LegendItem {
				name: series[1].name.clone(),
				color: end_tones.first().copied().unwrap_or("fg").to_string(),
				ring: false,
				tones: if end_tones.len() > 1 { Some(end_tones[1..].iter().map(|tone| tone.to_string()).collect()) } else { None },
				opacity: None,
			},
		]
	} else {
		series
			.iter()
			.enumerate()
			.map(|(at, entry)| LegendItem {
				name: entry.name.clone(),
				color: SERIES[at % SERIES.len()].to_string(),
				ring: false,
				tones: None,
				opacity: None,
			})
			.collect()
	};
	let mut notes: Vec<String> = Vec::new();
	if toned {
		if let Some(polarity) = polarity {
			notes.push(format!("{polarity:?} is better"));
		}
	}
	if scale.log && !tracks {
		notes.push("log scale".to_string());
	} else if scale.cut.is_some() || tier.is_some() {
		notes.push("broken axis".to_string());
	}
	Layer { height: grid_bottom + 20.0, body: parts.join(""), legend: Some(legend), ramp: None, notes: Some(notes) }
}

/// How `entry`'s moves from each pair's first point to its second are written.
fn move_style(entry: &ChartSeries, pairs: &[(&ChartPoint, &ChartPoint)]) -> MoveStyle {
	if entry.dim == Dimension::Percent {
		return MoveStyle::Difference;
	}
	if entry.dim == Dimension::Count
		&& entry.unit.is_empty()
		&& pairs.iter().all(|(a, b)| (a.value.fract() == 0.0 && a.value < MAX_COUNTED) && (b.value.fract() == 0.0 && b.value < MAX_COUNTED))
	{
		return MoveStyle::Difference;
	}
	let far = pairs.iter().any(|(a, b)| {
		a.value > 0.0 && (b.value == 0.0 || (b.value / a.value).abs().log10() >= FACTOR_MOVE.log10())
	});
	if far {
		MoveStyle::Factor
	} else {
		MoveStyle::Percent
	}
}

/// The move from `a` to `b` of `entry`'s measure in `style`.
fn move_text(a: &ChartPoint, b: &ChartPoint, entry: &ChartSeries, style: MoveStyle) -> String {
	if matches!(style, MoveStyle::Difference) {
		let difference = b.value - a.value;
		let size = difference.abs();
		if precision(size, 6) == 0.0 {
			return "same".to_string();
		}
		let sign = if difference > 0.0 { "+" } else { "−" };
		return if entry.dim == Dimension::Percent {
			format!("{sign}{} pp", trim(size))
		} else {
			format!("{sign}{}", format_value(size, entry.dim, &entry.unit, size.abs()))
		};
	}
	if a.value > 0.0 && b.value >= 0.0 {
		factor_move(b.value / a.value, style)
	} else {
		String::new()
	}
}

/// Each category's move from `before` to `after`.
fn pair_moves(before: &ChartSeries, after: &ChartSeries) -> Vec<String> {
	let pairs: Vec<(&ChartPoint, &ChartPoint)> = before
		.points
		.iter()
		.enumerate()
		.filter_map(|(at, a)| Some((a.as_ref()?, after.points.get(at)?.as_ref()?)))
		.collect();
	let style = move_style(after, &pairs);
	before
		.points
		.iter()
		.enumerate()
		.map(|(at, a)| match (a.as_ref(), after.points.get(at).and_then(|b| b.as_ref())) {
			(Some(a), Some(b)) => move_text(a, b, after, style),
			_ => String::new(),
		})
		.collect()
}

/// The tone of a transition from `a` to `b`, the end's before the start's.
fn pair_tone(before: &ChartSeries, after: &ChartSeries, a: &ChartPoint, b: &ChartPoint) -> Option<&'static str> {
	if a.value > 0.0 {
		move_tone(after.polarity.or(before.polarity), b.value / a.value)
	} else {
		None
	}
}

/// Dots at each value, ranged points as capsules, figures above or below.
fn draw_dots(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let references = reference_marks(spec);
	let mut values: Vec<f64> = Vec::new();
	for entry in series {
		for point in &entry.points {
			let Some(point) = point else { continue };
			values.push(point.value);
			values.push(point.upper.unwrap_or(point.value));
			if let Some(error) = point.error {
				values.push(point.value - error);
				values.push(point.value + error);
			}
		}
	}
	let left = PAD + label_column_width(spec) + 10.0;
	let baseline = spec.baseline.is_some();
	let figure_of = |point: &ChartPoint| {
		if baseline {
			factor_text(point)
		} else {
			clip(&point.text, 12)
		}
	};
	let figures: Vec<String> = series
		.iter()
		.flat_map(|entry| entry.points.iter().map(|point| point.as_ref().map_or(String::new(), |point| figure_of(point))))
		.collect();
	let figure_reach = figures.iter().map(|figure| measure(figure, 10.0) / 2.0).fold(8.0f64, f64::max);
	let right = WIDTH - PAD - figure_reach;
	let scale = span_scale(&values, left, right, &reference_values(&references), Some(series[0].dim), tick_unit(&series[0], &values));
	let row_height = label_row(spec, if categories.len() <= 10 { 36.0 } else if categories.len() <= 16 { 28.0 } else { 20.0 });
	let labelled = row_height >= 28.0;
	let top = if references.is_empty() { 0.0 } else { 16.0 };
	let tops = row_tops(spec, top, row_height);
	let bottom = *tops.last().unwrap_or(&0.0);
	let mut parts: Vec<String> = Vec::new();
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	parts.push(draw_value_axis(&scale, &series[0], top, bottom));
	parts.push(category_labels(spec, &tops, row_height, &strong_categories(spec)));
	parts.push(reference_lines(&references, &scale, left, right, top, bottom));
	let emphasized = series.iter().any(|entry| entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis)));
	let radius: f64 = if row_height >= 28.0 { 5.0 } else { 4.0 };
	for (at, _) in categories.iter().enumerate() {
		let y = tops[at] + row_height / 2.0 + if labelled { 2.0 } else { 0.0 };
		let present: Vec<(usize, &ChartSeries, &ChartPoint)> = series
			.iter()
			.enumerate()
			.filter_map(|(index, entry)| entry.points.get(at).and_then(|point| point.as_ref()).map(|point| (index, entry, point)))
			.collect();
		if present.is_empty() {
			let note = series.iter().find_map(|entry| entry.notes.get(at).and_then(|note| note.clone()));
			if let Some(note) = note {
				parts.push(note_text(left + 4.0, y, &clip(&note, 24), false));
			}
			continue;
		}
		let mut xs: Vec<f64> = Vec::new();
		for (_, _, point) in &present {
			xs.push(scale.at(point.value));
			xs.push(scale.at(point.upper.unwrap_or(point.value)));
		}
		let low = min_all(&xs);
		let high = max_all(&xs);
		if high - low >= 1.0 {
			parts.push(line(low, y, high, y, "muted", 2.0, 0.35));
		}
		let best = |index: usize, entry: &ChartSeries| {
			entry.best == Some(at) || spec.leaders.as_ref().and_then(|leaders| leaders.get(at)).copied().flatten() == Some(index)
		};
		let ranged = present.len() > 1 && present.iter().any(|(_, _, point)| point.upper.is_some());
		let thin = if ranged { radius.min(3.5) } else { radius };
		let stagger = if ranged { 2.0 * thin + 1.5 } else { 0.0 };
		let spread = stagger * (present.len() - 1) as f64 / 2.0;
		for (slot, (index, entry, point)) in present.iter().enumerate() {
			let context = emphasized && !point.emphasis;
			let dot_y = y - spread + slot as f64 * stagger;
			let size = if point.upper.is_none() { radius } else { thin };
			if point.error.is_some() {
				parts.push(whisker(&scale, point, dot_y, radius + 1.0, SERIES[*index % SERIES.len()]));
			}
			parts.push(mark(&scale, point, dot_y, size, SERIES[*index % SERIES.len()], if context { RECEDE } else { 1.0 }, false));
			if best(*index, entry) {
				let from = scale.at(point.value);
				let to = scale.at(point.upper.unwrap_or(point.value));
				if from == to {
					parts.push(ring(from, dot_y, radius + 3.0, "success"));
				} else {
					parts.push(mark(&scale, point, dot_y, size + 2.5, "success", 1.0, true));
				}
			}
		}
		if !labelled {
			continue;
		}
		let highest = present.iter().map(|(_, _, point)| point.value).fold(f64::NEG_INFINITY, f64::max);
		let lowest = present.iter().map(|(_, _, point)| point.value).fold(f64::INFINITY, f64::min);
		let rank = |item: &(usize, &ChartSeries, &ChartPoint)| {
			if best(item.0, item.1) || item.2.emphasis {
				0
			} else if item.2.value == highest || item.2.value == lowest {
				1
			} else {
				2
			}
		};
		let mut ordered = present.clone();
		ordered.sort_by_key(|item| rank(item));
		let mut taken: Vec<(f64, f64, i32)> = Vec::new();
		for item in ordered {
			let label = figure_of(item.2);
			let x = (scale.at(item.2.value) + scale.at(item.2.upper.unwrap_or(item.2.value))) / 2.0;
			let half = measure(&label, 10.0) / 2.0 + 2.0;
			let from = (x - half).max(left - 6.0).min(WIDTH - PAD - 2.0 * half);
			let level = [-1i32, 1].into_iter().find(|level| {
				taken.iter().all(|(other_from, other_to, other_level)| {
					*other_level != *level || *other_to <= from || *other_from >= from + 2.0 * half
				})
			});
			let Some(level) = level else { continue };
			taken.push((from, from + 2.0 * half, level));
			let strong = rank(&item) == 0;
			let tone = if baseline {
				move_tone(item.1.polarity.or(spec.category_polarity.as_ref().and_then(|p| p.get(at)).copied().flatten()), item.2.value)
			} else {
				None
			};
			let mut style = TextStyle::new(tone.unwrap_or(if strong { "fg" } else { "muted" }));
			style.size = Some(10.0);
			style.weight = if strong { Some(600) } else { None };
			style.anchor = Some(Anchor::Middle);
			parts.push(text(from + half, y + level as f64 * ((if ranged { thin + 6.0 } else { radius + 7.0 }) + spread), &label, &style));
		}
	}
	let ringed = categories.iter().enumerate().any(|(at, _)| {
		series.iter().enumerate().any(|(index, entry)| {
			entry.points.get(at).and_then(|point| point.as_ref()).is_some() && best_at(entry, spec, at, index)
		})
	});
	let mut legend: Vec<LegendItem> = series
		.iter()
		.enumerate()
		.map(|(at, entry)| LegendItem {
			name: entry.name.clone(),
			color: SERIES[at % SERIES.len()].to_string(),
			ring: false,
			tones: None,
			opacity: None,
		})
		.collect();
	if ringed {
		legend.push(LegendItem { name: "best".to_string(), color: "success".to_string(), ring: true, tones: None, opacity: None });
	}
	let mut notes: Vec<String> = Vec::new();
	if baseline {
		notes.push("factor".to_string());
	}
	if scale.log {
		notes.push("log scale".to_string());
	}
	Layer { height: bottom + 20.0, body: parts.join(""), legend: Some(legend), ramp: None, notes: Some(notes) }
}

fn best_at(entry: &ChartSeries, spec: &ChartSpec, at: usize, index: usize) -> bool {
	entry.best == Some(at) || spec.leaders.as_ref().and_then(|leaders| leaders.get(at)).copied().flatten() == Some(index)
}

fn reference_values(marks: &[ChartAnnotation]) -> Vec<f64> {
	marks
		.iter()
		.filter_map(|mark| match mark {
			ChartAnnotation::Reference { value, .. } => Some(*value),
			_ => None,
		})
		.collect()
}

/// Floating intervals from each value's low end to its high end, one band per unit.
fn draw_range(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let references = reference_marks(spec);
	let typical = |a: &ChartSeries, b: &ChartSeries| {
		let mut ratios: Vec<f64> = categories
			.iter()
			.enumerate()
			.filter_map(|(at, _)| {
				let x = a.points.get(at).and_then(|point| point.as_ref());
				let y = b.points.get(at).and_then(|point| point.as_ref());
				match (x, y) {
					(Some(x), Some(y)) if interval_middle(x) > 0.0 => Some(interval_middle(y) / interval_middle(x)),
					_ => None,
				}
			})
			.collect();
		ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		ratios.get(ratios.len() >> 1).copied().unwrap_or(1.0)
	};
	let mut bands: Vec<Vec<usize>> = Vec::new();
	for (index, entry) in series.iter().enumerate() {
		let mut placed = false;
		for band in bands.iter_mut() {
			let lead = &series[band[0]];
			let ratio = typical(lead, entry);
			if lead.dim == entry.dim && lead.unit == entry.unit && ratio < BAND_SPREAD && ratio > 1.0 / BAND_SPREAD {
				band.push(index);
				placed = true;
				break;
			}
		}
		if !placed {
			bands.push(vec![index]);
		}
	}
	let titled = bands.len() > 1;
	let transition = spec.transition == Some(true);
	let color = |index: usize| {
		if transition {
			if index == 0 {
				"muted"
			} else {
				SERIES[(index - 1) % SERIES.len()]
			}
		} else {
			SERIES[index % SERIES.len()]
		}
	};
	let figure = |index: usize, at: usize| {
		let own = point_text(&series[index], at);
		let central = if index == 0 {
			spec.estimate.as_ref().and_then(|estimate| estimate.points.get(at)).and_then(|point| point.as_ref()).map_or(String::new(), |point| point.text.clone())
		} else {
			String::new()
		};
		[central, own].into_iter().filter(|text| !text.is_empty()).collect::<Vec<String>>().join(" · ")
	};
	let figure_width = series
		.iter()
		.enumerate()
		.flat_map(|(index, _)| categories.iter().enumerate().map(move |(at, _)| measure(&figure(index, at), TICK_SIZE)))
		.fold(0.0f64, f64::max);
	let paired = |members: &[usize]| {
		if members.len() != 2 {
			return false;
		}
		let first = series[members[0]].name.trim().to_string();
		let second = series[members[1]].name.trim().to_string();
		(transition && members[0] == 0) || is_condition_pair(&first, &second) || (!first.contains(char::is_whitespace) && !second.contains(char::is_whitespace))
	};
	let referenced = spec.reference.as_deref().map_or(false, |reference| !reference.is_empty()) && bands.iter().any(|members| members.len() == 1);
	let factor_width = if bands.iter().any(|members| paired(members)) || referenced { 84.0 } else { 0.0 };
	let left = PAD + label_column_width(spec) + 10.0;
	let right = WIDTH - PAD - figure_width - 8.0 - factor_width;
	let strong = strong_categories(spec);
	let bold = |index: usize, at: usize| {
		series[index].points.get(at).and_then(|point| point.as_ref()).map_or(false, |point| point.emphasis)
			|| (index == 0
				&& spec.estimate.as_ref().and_then(|estimate| estimate.points.get(at)).and_then(|point| point.as_ref()).map_or(false, |point| point.emphasis))
	};
	let emphasized = series.iter().enumerate().any(|(index, _)| categories.iter().enumerate().any(|(at, _)| bold(index, at)));
	let mut parts: Vec<String> = Vec::new();
	let mut notes: Vec<String> = Vec::new();
	let mut y = 0.0;
	for members in &bands {
		let entries: Vec<&ChartSeries> = members.iter().map(|index| &series[*index]).collect();
		let marks: Vec<ChartAnnotation> = references
			.iter()
			.filter(|mark| match mark {
				ChartAnnotation::Reference { series, .. } => members.contains(&series.unwrap_or(0)),
				_ => false,
			})
			.cloned()
			.collect();
		let mut values: Vec<f64> = Vec::new();
		for entry in &entries {
			for point in &entry.points {
				let Some(point) = point else { continue };
				values.push(point.value);
				values.push(point.upper.unwrap_or(point.value));
			}
		}
		if members.contains(&0) {
			if let Some(estimate) = &spec.estimate {
				for point in &estimate.points {
					if let Some(point) = point {
						values.push(point.value);
					}
				}
			}
		}
		let scale = span_scale(&values, left, right, &reference_values(&marks), Some(entries[0].dim), tick_unit(entries[0], &values));
		if scale.log && !notes.iter().any(|note| note == "log scale") {
			notes.push("log scale".to_string());
		}
		if titled {
			let names: Vec<String> = entries.iter().map(|entry| entry.name.clone()).collect();
			let mut style = TextStyle::new("fg");
			style.weight = Some(600);
			parts.push(text(PAD, y + 8.0, &clip(&names.join(" · "), 60), &style));
			y += TITLE_LINE + 4.0;
		}
		if !marks.is_empty() {
			y += 16.0;
		}
		let (row, bar, step) = bar_metrics(categories.len(), members.len());
		let row_height = label_row(spec, row);
		let tops = row_tops(spec, y, row_height);
		let bottom = *tops.last().unwrap_or(&0.0);
		for at in highlighted(spec) {
			parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
		}
		parts.push(draw_value_axis(&scale, entries[0], y, bottom));
		parts.push(category_labels(spec, &tops, row_height, &strong));
		parts.push(reference_lines(&marks, &scale, left, right, y, bottom));
		let lone = if members.len() == 1 { Some(&series[members[0]]) } else { None };
		let base = |at: usize| -> Option<usize> {
			if lone.is_none() {
				return None;
			}
			spec.reference.as_deref()?.iter().copied().find(|index| group_span(spec, *index) == group_span(spec, at))
		};
		let pair = paired(members);
		let factors: Vec<Option<f64>> = categories
			.iter()
			.enumerate()
			.map(|(at, _)| {
				let reference = base(at);
				let (a, b) = if pair {
					(
						series[members[0]].points.get(at).and_then(|point| point.as_ref()),
						series[members[1]].points.get(at).and_then(|point| point.as_ref()),
					)
				} else {
					let a = match reference {
						Some(reference) if reference != at => lone.and_then(|lone| lone.points.get(reference).and_then(|point| point.as_ref())),
						_ => None,
					};
					let b = lone.and_then(|lone| lone.points.get(at).and_then(|point| point.as_ref()));
					(a, b)
				};
				match (a, b) {
					(Some(a), Some(b)) if interval_middle(a) > 0.0 && interval_middle(b) > 0.0 => {
						Some(interval_middle(b) / interval_middle(a))
					}
					_ => None,
				}
			})
			.collect();
		let moves = (transition && members[0] == 0) || lone.is_some();
		let known: Vec<f64> = factors.iter().copied().flatten().collect();
		let one_way = !known.is_empty() && (known.iter().all(|factor| *factor >= 1.0) || known.iter().all(|factor| *factor < 1.0));
		if pair && !moves && one_way {
			let a = series[members[0]].name.clone();
			let b = series[members[1]].name.clone();
			notes.push(if known[0] >= 1.0 { format!("{b} ÷ {a}") } else { format!("{a} ÷ {b}") });
		}
		if let Some(lone) = lone {
			for index in spec.reference.as_deref().unwrap_or(&[]) {
				let Some(point) = lone.points.get(*index).and_then(|point| point.as_ref()) else { continue };
				let (first, last) = group_span(spec, *index);
				let x = scale.at(interval_middle(point));
				parts.push(reference_rule(x, tops[first] + 2.0, x, tops[last] + row_height - 2.0));
			}
		}
		for (at, _) in categories.iter().enumerate() {
			let row_top = tops[at];
			let middle = row_top + row_height / 2.0;
			for (slot, index) in members.iter().enumerate() {
				let entry = &series[*index];
				let point = entry.points.get(at).and_then(|point| point.as_ref());
				let centre = if members.len() > 1 { row_top + 5.0 + slot as f64 * step + bar / 2.0 } else { middle };
				let Some(point) = point else {
					let note = entry.notes.get(at).and_then(|note| note.clone());
					if let Some(note) = note {
						parts.push(note_text(left + 4.0, centre, &clip(&note, 24), false));
					}
					continue;
				};
				let context = emphasized && !bold(*index, at);
				let fill = color(*index);
				let opacity = if context {
					RECEDE
				} else if transition && *index == 0 {
					0.6
				} else {
					1.0
		};
				let from = scale.at(point.value);
				let to = scale.at(point.upper.unwrap_or(point.value));
				if to - from < bar {
					parts.push(dot((from + to) / 2.0, centre, (bar / 2.0).min(6.0), fill, opacity));
				} else {
					parts.push(rect(from, centre - bar / 2.0, to - from, bar, fill, opacity, 0.0));
				}
				let central = if *index == 0 {
					spec.estimate.as_ref().and_then(|estimate| estimate.points.get(at)).and_then(|point| point.as_ref())
				} else {
					None
				};
				if let Some(central) = central {
					let x = scale.at(central.value);
					parts.push(line(x, centre - bar / 2.0 - 3.0, x, centre + bar / 2.0 + 3.0, "fg", 2.0, 1.0));
				}
				let end = to.max(from + bar / 2.0) + 6.0;
				let own = point_text(entry, at);
				let head = central.map_or(String::new(), |central| format!("{} · ", central.text));
				let size = if members.len() > 1 && step < 12.0 { 10.0 } else { TICK_SIZE };
				if !head.is_empty() {
					let mut style = TextStyle::new("fg");
					style.size = Some(size);
					style.weight = Some(600);
					parts.push(text(end, centre, &head, &style));
				}
				let mut style = TextStyle::new(if point.emphasis { "fg" } else { "muted" });
				style.size = Some(size);
				style.weight = if point.emphasis { Some(600) } else { None };
				parts.push(text(end + if head.is_empty() { 0.0 } else { measure(&head, size) * SEMIBOLD_WIDTH }, centre, &own, &style));
			}
			let Some(factor) = factors[at] else { continue };
			let polarity = series[*members.get(1).unwrap_or(&members[0])].polarity.or(series[members[0]].polarity);
			let tone = if moves { move_tone(polarity, factor) } else { None };
			let said = if moves {
				factor_text(&ChartPoint { value: factor, text: String::new(), emphasis: false, upper: None, error: None })
			} else {
				format!("{}×", compact_factor(if factor >= 1.0 { factor } else { 1.0 / factor }))
			};
			if !moves && !one_way {
				parts.push(dot(
					WIDTH - PAD - measure(&said, TICK_SIZE) * SEMIBOLD_WIDTH - 8.0,
					middle,
					3.5,
					color(members[if factor >= 1.0 { 1 } else { 0 }]),
					1.0,
				));
			}
			let mut style = TextStyle::new(tone.unwrap_or("fg"));
			style.size = Some(TICK_SIZE);
			style.weight = Some(600);
			style.anchor = Some(Anchor::End);
			parts.push(text(WIDTH - PAD, middle, &clip(&said, 14), &style));
		}
		y = bottom + 20.0 + if titled { 12.0 } else { 0.0 };
	}
	let mut legend: Vec<LegendItem> = if (series.len() < 2 && spec.estimate.is_none()) || (titled && bands.iter().all(|members| members.len() == 1)) {
		Vec::new()
	} else {
		series
			.iter()
			.enumerate()
			.map(|(index, entry)| LegendItem {
				name: entry.name.clone(),
				color: color(index).to_string(),
				ring: false,
				tones: None,
				opacity: None,
			})
			.collect()
	};
	if let Some(estimate) = &spec.estimate {
		legend.insert(0, LegendItem { name: estimate.name.clone(), color: "fg".to_string(), ring: false, tones: None, opacity: None });
	}
	Layer {
		height: y - if titled { 12.0 } else { 0.0 },
		body: parts.join(""),
		legend: Some(legend),
		ramp: None,
		notes: Some(notes),
	}
}

/// A figure's trailing unit (`3.7ms` → `ms`), the chars the reference's unit regex matches.
fn figure_unit(figure: &str) -> &str {
	let cut = figure
		.char_indices()
		.rev()
		.find(|(_, ch)| ch.is_ascii_digit() || *ch == ' ' || *ch == '.' || *ch == ',')
		.map_or(0, |(index, ch)| index + ch.len_utf8());
	&figure[cut..]
}

/// Whether `name` is a ladder's middle quantile.
fn is_median_name(name: &str) -> bool {
	const MEDIAN: [&str; 6] = ["p50", "median", "med", "mean", "avg", "average"];
	MEDIAN.iter().any(|median| name.eq_ignore_ascii_case(median))
}

/// Quantile ladders: one thick band per ladder, its middle quantile drawn as a dot.
fn draw_ladder(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let ladders: Vec<ChartLadder> = spec.ladders.clone().unwrap_or_else(|| {
		vec![ChartLadder { name: String::new(), series: (0..series.len()).collect() }]
	});
	let values: Vec<f64> = series
		.iter()
		.flat_map(|entry| entry.points.iter().flatten().map(|point| point.value))
		.collect();
	// Figures share their unit: written once, after the last (`0.65 / 0.87 / 3.7ms`).
	let figures = |members: &[usize], at: usize| {
		let texts: Vec<String> = members
			.iter()
			.filter_map(|index| {
				series
					.get(*index)
					.and_then(|entry| entry.points.get(at))
					.and_then(|point| point.as_ref())
					.map(|_| point_text(&series[*index], at))
			})
			.collect();
		let unit = figure_unit(texts.last().map_or("", String::as_str));
		let bare = |figure: &str| {
			if unit.is_empty() {
				figure.to_string()
			} else {
				figure[..figure.len() - unit.len()].trim().to_string()
			}
		};
		let shared = !unit.is_empty()
			&& texts.iter().all(|figure| figure.ends_with(unit) && bare(figure).ends_with(|ch: char| ch.is_ascii_digit()));
		let written: Vec<String> = texts
			.iter()
			.enumerate()
			.map(|(slot, figure)| {
				if shared && slot < texts.len() - 1 {
					bare(figure)
				} else {
					figure.clone()
				}
			})
			.collect();
		clip(&written.join(" / "), 32)
	};
	let figure_width = ladders
		.iter()
		.flat_map(|ladder| categories.iter().enumerate().map(move |(at, _)| measure(&figures(&ladder.series, at), 10.0)))
		.fold(0.0f64, f64::max);
	let left = PAD + label_column_width(spec) + 10.0;
	let right = WIDTH - PAD - figure_width - 12.0;
	let scale = span_scale(&values, left, right, &[], Some(series[0].dim), tick_unit(&series[0], &values));
	let stacked = ladders.len() > 1;
	let metrics = bar_metrics(categories.len(), ladders.len());
	let row_height = label_row(spec, if stacked { metrics.0 + 6.0 } else { metrics.0.max(if categories.len() <= 12 { LADDER_ROW } else { 24.0 }) });
	let step = if stacked { metrics.2 + 3.0 } else { 0.0 };
	let thick = if stacked { metrics.1 } else { 12.0f64.min(metrics.1) };
	// The first row whose marks stand far enough apart for every name carries the quantile names (the first
	// ladder's), else the row whose marks stand furthest apart.
	let lead = ladders[0].series.clone();
	let spacing: Vec<f64> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| {
			let xs: Vec<f64> = lead
				.iter()
				.filter_map(|index| series[*index].points.get(at).and_then(|point| point.as_ref()).map(|point| scale.at(point.value)))
				.collect();
			if xs.len() < lead.len() {
				-1.0
			} else {
				xs.windows(2).map(|pair| pair[1] - pair[0]).fold(f64::INFINITY, f64::min)
			}
		})
		.collect();
	let room = lead.iter().map(|index| measure(&clip(&series[*index].name, 8), 10.0)).fold(0.0f64, f64::max) + 4.0;
	let named = match spacing.iter().position(|gap| *gap >= room) {
		Some(at) => at,
		None => spacing
			.iter()
			.enumerate()
			.max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
			.map(|(at, _)| at)
			.unwrap_or(0),
	};
	let top = 14.0;
	let tops = row_tops(spec, top, row_height);
	let bottom = *tops.last().unwrap_or(&0.0);
	let mut parts: Vec<String> = Vec::new();
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	parts.push(draw_value_axis(&scale, &series[0], top, bottom));
	let strong = strong_categories(spec);
	parts.push(category_labels(spec, &tops, row_height, &strong));
	// Before and after (`Baseline` | `Final`): the start recedes in muted, the lead color passes to the end.
	let color_of = |slot: usize| {
		if spec.transition == Some(true) {
			if slot == 0 {
				"muted"
			} else {
				SERIES[(slot - 1) % SERIES.len()]
			}
		} else {
			SERIES[slot % SERIES.len()]
		}
	};
	for (at, _) in categories.iter().enumerate() {
		let row_top = tops[at];
		for (slot, ladder) in ladders.iter().enumerate() {
			let color = color_of(slot);
			let y = if stacked { row_top + 6.0 + slot as f64 * step + thick / 2.0 } else { row_top + row_height / 2.0 + 4.0 };
			let marks: Vec<(usize, f64, &ChartPoint)> = ladder
				.series
				.iter()
				.filter_map(|index| series[*index].points.get(at).and_then(|point| point.as_ref()).map(|point| (*index, scale.at(point.value), point)))
				.collect();
			if marks.is_empty() {
				continue;
			}
			// The middle quantile: the median (or a mean beside quantiles), else the lowest one shown.
			let middle = marks.iter().position(|(index, _, _)| is_median_name(&series[*index].name)).unwrap_or(0);
			let low = marks[0].1;
			let high = marks[marks.len() - 1].1;
			if high - low >= 1.0 {
				parts.push(line(low, y, high, y, color, 1.5, 0.55));
			}
			let centre = marks[middle];
			let body = marks.get(middle + 1);
			if let Some(body) = body {
				if body.1 - centre.1 >= 1.0 {
					parts.push(rect(centre.1, y - thick / 2.0, body.1 - centre.1, thick, color, LADDER_BODY, 2.0));
				}
			}
			for (order, (_, x, _)) in marks.iter().enumerate() {
				if order == middle {
					continue;
				}
				parts.push(line(*x, y - thick / 2.0 - 1.0, *x, y + thick / 2.0 + 1.0, color, 2.0, 1.0));
			}
			let radius = (thick / 2.0 + 1.0).min(5.5);
			parts.push(format!(
				r#"<circle cx="{}" cy="{}" r="{}" fill="{}" stroke="{}" stroke-width="1.5"/>"#,
				num(centre.1),
				num(y),
				num(radius),
				token(color),
				token("surface")
			));
			let said = figures(&ladder.series, at);
			let emphasis = strong.contains(&at) || marks.iter().any(|(_, _, point)| point.emphasis);
			let mut style = TextStyle::new(if emphasis { "fg" } else { "muted" });
			style.size = Some(10.0);
			style.weight = if emphasis { Some(600) } else { None };
			parts.push(text((high.max(centre.1 + 6.0) + 8.0).max(high), y, &said, &style));
			if at != named || slot != 0 {
				continue;
			}
			// Quantile names over this row's marks, each where it clears the one before.
			let mut clear = f64::NEG_INFINITY;
			for (index, x, _) in &marks {
				let name = clip(&series[*index].name, 8);
				let half = measure(&name, 10.0) / 2.0;
				if *x - half < clear {
					continue;
				}
				let mut style = TextStyle::new("muted");
				style.size = Some(10.0);
				style.anchor = Some(Anchor::Middle);
				parts.push(text(*x, y - thick / 2.0 - 8.0, &name, &style));
				clear = *x + half + 4.0;
			}
		}
	}
	let legend: Vec<LegendItem> = if stacked {
		ladders
			.iter()
			.enumerate()
			.map(|(slot, ladder)| LegendItem {
				name: if ladder.name.is_empty() { format!("#{}", slot + 1) } else { ladder.name.clone() },
				color: color_of(slot).to_string(),
				ring: false,
				tones: None,
				opacity: None,
			})
			.collect()
	} else {
		Vec::new()
	};
	Layer {
		height: bottom + 20.0,
		body: parts.join(""),
		legend: Some(legend),
		ramp: None,
		notes: Some(if scale.log { vec!["log scale".to_string()] } else { Vec::new() }),
	}
}

/// Counts (or shares) over ordered bins as columns rising from zero in bin order.
fn draw_histogram(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let mut values = vec![0.0];
	for entry in series {
		for point in &entry.points {
			if let Some(point) = point {
				values.push(point.value);
			}
		}
	}
	let unit = tick_unit(&series[0], &values);
	let probe = value_scale(&values, 0.0, 1.0, true, false, unit);
	let format = tick_format(&probe, &series[0]);
	let tick_width = probe.ticks.iter().map(|tick| measure(&format(*tick), TICK_SIZE)).fold(0.0f64, f64::max);
	let left = PAD + tick_width + 8.0;
	let right = WIDTH - PAD;
	let plot_top = 18.0;
	let plot_bottom = plot_top + HISTOGRAM_HEIGHT;
	let scale = value_scale(&values, plot_bottom, plot_top, true, false, unit);
	let bin_width = (right - left) / categories.len() as f64;
	let single = series.len() == 1;
	let inner = if single { bin_width - 3.0 } else { bin_width * (1.0 - BIN_GAP) };
	let column_width = inner / series.len() as f64;
	let mut parts: Vec<String> = Vec::new();
	for at in highlighted(spec) {
		parts.push(band(left + at as f64 * bin_width, plot_top - 16.0, bin_width, plot_bottom - plot_top + 36.0));
	}
	for tick in &probe.ticks {
		let y = scale.at(*tick);
		if *tick == 0.0 {
			parts.push(line(left, y, right, y, "border", 1.0, 1.0));
		} else {
			parts.push(rule(left, y, right, y));
		}
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::End);
		parts.push(text(left - 8.0, y, &format(*tick), &style));
	}
	let emphasized = series.iter().any(|entry| entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis)));
	let strong = strong_categories(spec);
	let label_size = if column_width >= 34.0 { TICK_SIZE } else { 10.0 };
	for (at, name) in categories.iter().enumerate() {
		let start = left + at as f64 * bin_width + (bin_width - inner) / 2.0;
		for (index, entry) in series.iter().enumerate() {
			let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) else { continue };
			let x = start + index as f64 * column_width;
			// A zero or near-zero bin stays visible as a stub.
			let y = scale.at(point.value.max(0.0)).min(plot_bottom - MIN_BAR);
			let context = emphasized && !point.emphasis;
			let width = (column_width - if single { 0.0 } else { 2.0 }).max(1.0);
			let color = SERIES[index % SERIES.len()];
			parts.push(bar_rect(x, y, width, plot_bottom - y, color, if context { RECEDE + 0.15 } else { 1.0 }, false));
			let figure = point_text(entry, at);
			if measure(&figure, label_size) <= column_width + if single { 0.0 } else { 4.0 } {
				let mut style = TextStyle::new(if point.emphasis { "fg" } else { "muted" });
				style.size = Some(label_size);
				style.weight = if point.emphasis { Some(600) } else { None };
				style.anchor = Some(Anchor::Middle);
				parts.push(text(x + width / 2.0, y - 8.0, &figure, &style));
			}
		}
		let mut style = TextStyle::new(if strong.contains(&at) { "fg" } else { "muted" });
		style.size = Some(TICK_SIZE);
		style.weight = if strong.contains(&at) { Some(600) } else { None };
		style.anchor = Some(Anchor::Middle);
		parts.push(text(
			left + (at as f64 + 0.5) * bin_width,
			plot_bottom + 13.0,
			&fit_text(name, bin_width - 4.0, TICK_SIZE),
			&style,
		));
	}
	let mut height = plot_bottom + 22.0;
	if !spec.axis.is_empty() {
		let name = clip(&spec.axis, 60);
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::Middle);
		parts.push(text((left + right) / 2.0, height + 8.0, &name, &style));
		height += 18.0;
	}
	let legend: Vec<LegendItem> = if single {
		Vec::new()
	} else {
		series
			.iter()
			.enumerate()
			.map(|(at, entry)| LegendItem {
				name: entry.name.clone(),
				color: SERIES[at % SERIES.len()].to_string(),
				ring: false,
				tones: None,
				opacity: None,
			})
			.collect()
	};
	let mut notes: Vec<String> = Vec::new();
	if single {
		notes.push(series[0].name.clone());
	}
	if spec.normalized == Some(true) {
		notes.push("each as shares of its total".to_string());
	}
	Layer { height, body: parts.join(""), legend: Some(legend), ramp: None, notes: Some(notes) }
}

/// Fill of part `at` of a whole: its outcome's status color where the parts are toned.
fn part_color(spec: &ChartSpec, at: usize) -> &'static str {
	match &spec.tones {
		None => SERIES[at % SERIES.len()],
		Some(tones) => tones.get(at).copied().flatten().map(status_color).unwrap_or("muted"),
	}
}

/// Opacity step of each further part sharing one outcome color, so they stay apart.

/// Opacity of part `at` of a toned whole.
fn part_opacity(spec: &ChartSpec, at: usize) -> f64 {
	let Some(tones) = &spec.tones else { return 1.0 };
	let Some(tone) = tones.get(at).copied().flatten() else { return 1.0 };
	let repeats = tones.iter().take(at).filter(|other| **other == Some(tone)).count();
	(1.0 - TONE_STEP * repeats as f64).max(0.35)
}

/// Consecutive groups gathered while their bar ends stay within `SPLIT_SPREAD`× of each other.
fn stack_bands(spec: &ChartSpec, ends: &[f64]) -> Vec<StackBand> {
	let runs: Vec<usize> = spec
		.groups
		.as_ref()
		.map(|groups| groups.iter().map(|group| group.members.len()).collect())
		.unwrap_or_else(|| vec![ends.len()]);
	let mut bands: Vec<(StackBand, f64, f64)> = Vec::new();
	let mut row = 0;
	for (index, size) in runs.iter().enumerate() {
		let peak = ends[row..row + size].iter().copied().fold(f64::NEG_INFINITY, f64::max);
		let mut merge = false;
		if let Some(last) = bands.last_mut() {
			let low = last.1.min(peak);
			let high = last.2.max(peak);
			if peak == 0.0 || high <= low * SPLIT_SPREAD {
				last.0.rows.1 = row + size;
				last.0.groups.1 = index + 1;
				if peak > 0.0 {
					last.1 = low;
					last.2 = high;
				}
				merge = true;
			}
		}
		if !merge {
			bands.push((StackBand { rows: (row, row + size), groups: (index, index + 1) }, peak, peak));
		}
		row += size;
	}
	if bands.len() > 3 {
		return vec![StackBand { rows: (0, ends.len()), groups: (0, runs.len()) }];
	}
	bands.into_iter().map(|(band, _, _)| band).collect()
}

/// Each category's parts as touching segments of one bar, its whole written past its end.
fn draw_stacked(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let whole = spec.whole.as_ref();
	let normalized = spec.normalized == Some(true);
	let colors: Vec<&'static str> = series.iter().enumerate().map(|(at, _)| part_color(spec, at)).collect();
	let shades: Vec<f64> = series.iter().enumerate().map(|(at, _)| part_opacity(spec, at)).collect();
	let sums: Vec<f64> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| series.iter().map(|entry| entry.points.get(at).and_then(|point| point.as_ref()).map_or(0.0, |point| point.value)).sum())
		.collect();
	// A whole the parts reach within rounding leaves no rest worth a track.
	let ends: Vec<f64> = sums
		.iter()
		.enumerate()
		.map(|(at, sum)| {
			let total = whole.and_then(|whole| whole.points.get(at)).and_then(|point| point.as_ref()).map_or(0.0, |point| point.value);
			if total > sum * (1.0 + MIN_REST) { total } else { *sum }
		})
		.collect();
	let totals: Vec<String> = sums
		.iter()
		.enumerate()
		.map(|(at, sum)| {
			whole
				.and_then(|whole| whole.points.get(at))
				.and_then(|point| point.as_ref())
				.map_or_else(|| format_value(*sum, series[0].dim, if binary_bytes(&series[0]) { "iB" } else { &series[0].unit }, sum.abs()), |point| point.text.clone())
		})
		.collect();
	let rested = ends.iter().zip(sums.iter()).any(|(end, sum)| end > sum);
	let left = PAD + label_column_width(spec) + 10.0;
	let right = WIDTH - PAD - totals.iter().map(|label| measure(label, TICK_SIZE)).fold(0.0f64, f64::max).min(120.0) - 8.0;
	let (row, bar_height, _) = bar_metrics(categories.len(), 1);
	let row_height = label_row(spec, row);
	let strong = strong_categories(spec);
	let emphasized = series.iter().any(|entry| entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis)));
	let banded: Vec<f64> = if normalized { ends.iter().map(|end| if *end > 0.0 { 100.0 } else { 0.0 }).collect() } else { ends.clone() };
	let bands = stack_bands(spec, &banded);
	let mut parts: Vec<String> = Vec::new();
	let mut top = 0.0;
	for stack in &bands {
		let first = stack.rows.0;
		let last = stack.rows.1;
		let view = ChartSpec {
			categories: categories[first..last].to_vec(),
			groups: spec.groups.as_ref().map(|groups| groups[stack.groups.0..stack.groups.1].to_vec()),
			..spec.clone()
		};
		let tops = row_tops(&view, top, row_height);
		let bottom = *tops.last().unwrap_or(&0.0);
		let mut extent = vec![0.0];
		if !normalized {
			extent.extend_from_slice(&ends[first..last]);
		} else {
			extent.push(100.0);
		}
		let scale = value_scale(&extent, left, right, true, false, if normalized { 1.0 } else { tick_unit(&series[0], &extent) });
		let local = |set: &HashSet<usize>| -> HashSet<usize> {
			set.iter().copied().filter(|at| *at >= first && *at < last).map(|at| at - first).collect()
		};
		for at in highlighted(spec) {
			if at >= first && at < last {
				parts.push(band(PAD - 4.0, tops[at - first], WIDTH - 2.0 * PAD + 8.0, row_height));
			}
		}
		let percent_axis = ChartSeries { dim: Dimension::Percent, unit: String::new(), ..series[0].clone() };
		parts.push(draw_value_axis(&scale, if normalized { &percent_axis } else { &series[0] }, top, bottom));
		parts.push(category_labels(&view, &tops, row_height, &local(&strong)));
		for at in first..last {
			let y = tops[at - first] + (row_height - bar_height) / 2.0;
			let end = ends[at];
			// A mix places each part at its percent of the row's total.
			let place = |value: f64| if normalized { scale.at(100.0 * value / end) } else { scale.at(value) };
			// A row holding none of the parts draws nothing but its note.
			if end == 0.0 {
				let note = series.iter().find_map(|entry| entry.notes.get(at).and_then(|note| note.clone()));
				if let Some(note) = note {
					parts.push(note_text(left + 4.0, y + bar_height / 2.0, &clip(&note, 24), false));
				}
				continue;
			}
			let mut sum = 0.0;
			for (index, entry) in series.iter().enumerate() {
				let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) else { continue };
				if point.value <= 0.0 {
					continue;
				}
				let from = place(sum);
				sum += point.value;
				// Segments part with a sliver of the surface behind them.
				let width = (place(sum) - from - 1.5).max(1.0);
				let opacity = (if emphasized && !point.emphasis { RECEDE } else { 1.0 }) * shades[index];
				parts.push(rect(from, y, width, bar_height, colors[index], opacity, 0.0));
				let label = if normalized {
					percent_text(100.0 * point.value / end)
				} else if bar_height >= 14.0 {
					point.text.clone()
				} else {
					String::new()
				};
				if !label.is_empty() && point.value < end && measure(&label, TICK_SIZE) * SEMIBOLD_WIDTH + 10.0 <= width {
					let mut style = TextStyle::new("fg");
					style.size = Some(TICK_SIZE);
					style.anchor = Some(Anchor::Middle);
					style.weight = Some(600);
					let middle = from + width / 2.0;
					let said = y + bar_height / 2.0;
					if opacity >= INK_LEVEL {
						parts.push(ink(middle, said, &label, &style));
					} else {
						parts.push(text(middle, said, &label, &style));
					}
				}
			}
			if end > sums[at] {
				parts.push(rect(place(sum), y, place(end) - place(sum), bar_height, "border", 0.5, 0.0));
			}
			let bold = whole
				.and_then(|whole| whole.points.get(at))
				.and_then(|point| point.as_ref())
				.map_or(false, |point| point.emphasis);
			let mut style = TextStyle::new(if bold { "fg" } else { "muted" });
			style.size = Some(TICK_SIZE);
			style.weight = if bold { Some(600) } else { None };
			parts.push(text(place(end) + 6.0, y + bar_height / 2.0, &totals[at], &style));
		}
		top = bottom + 26.0;
	}
	let mut legend: Vec<LegendItem> = series
		.iter()
		.enumerate()
		.map(|(at, entry)| LegendItem { name: entry.name.clone(), color: colors[at].to_string(), ring: false, tones: None, opacity: Some(shades[at]) })
		.collect();
	if rested && whole.is_some() {
		legend.push(LegendItem { name: format!("rest of {}", whole.unwrap().name), color: "border".to_string(), ring: false, tones: None, opacity: None });
	}
	let notes: Vec<String> = if normalized {
		vec!["% of row total".to_string()]
	} else if bands.len() > 1 {
		vec!["scaled per group".to_string()]
	} else {
		Vec::new()
	};
	Layer { height: top - 6.0, body: parts.join(""), legend: Some(legend), ramp: None, notes: Some(notes) }
}

/// Bars against targets: a faint track to the target, the bar success where it keeps the target's side.
fn draw_bullet(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let entry = &spec.series[0];
	let targets: &[Option<ChartTarget>] = spec.targets.as_deref().unwrap_or(&[]);
	let word = spec.target_name.clone().unwrap_or_else(|| "target".to_string());
	let known: Vec<&ChartTarget> = targets.iter().flatten().collect();
	let first = known.first().copied();
	let shared = first.is_some()
		&& known.iter().all(|target| {
			target.value == first.unwrap().value && target.upper == first.unwrap().upper && target.text == first.unwrap().text
		});
	let goal = if shared { first.unwrap().upper.unwrap_or(first.unwrap().value) } else { 100.0 };
	let reach = goal * TARGET_REACH;
	// Positions: in the unit when every row shares one target, else as a percent of each row's own.
	let position = |at: usize, value: f64| -> f64 {
		let Some(target) = targets.get(at).and_then(|target| target.as_ref()) else { return value };
		if shared {
			return value;
		}
		let scale = target.upper.unwrap_or(target.value);
		if scale > 0.0 {
			100.0 * value / scale
		} else if value > 0.0 {
			f64::INFINITY
		} else {
			0.0
		}
	};
	let standings: Vec<Option<Standing>> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| {
			let point = entry.points.get(at).and_then(|point| point.as_ref());
			let target = targets.get(at).and_then(|target| target.as_ref());
			match (point, target) {
				(Some(point), Some(target)) => target_standing(point, target),
				_ => None,
			}
		})
		.collect();
	let tone = |at: usize| -> &'static str {
		match standings.get(at).and_then(|standing| *standing) {
			None => SERIES[0],
			Some(standing) => {
				if !standing.met {
					"error"
				} else if standing.close {
					"warning"
				} else {
					"success"
				}
			}
		}
	};
	// Each row's share of its target; over a zero target only whether it met it.
	let share_text = |at: usize| -> String {
		if shared {
			return String::new();
		}
		let Some(standing) = standings.get(at).and_then(|standing| *standing) else { return String::new() };
		let Some(share) = standing.share else { return if standing.met { "met".to_string() } else { "over".to_string() } };
		if share >= TARGET_REACH {
			compact_factor(share)
		} else if share < 0.01 {
			percent_text(share * 100.0)
		} else {
			format!("{}%", (share * 100.0).round())
		}
	};
	let figure = |at: usize| {
		let own = point_text(entry, at);
		let target = targets.get(at).and_then(|target| target.as_ref());
		if shared || target.is_none() {
			own
		} else {
			format!("{own}  {}", target.unwrap().text)
		}
	};
	let share_width = categories
		.iter()
		.enumerate()
		.map(|(at, _)| measure(&share_text(at), TICK_SIZE) * SEMIBOLD_WIDTH)
		.fold(0.0f64, f64::max);
	let figure_width = (categories
		.iter()
		.enumerate()
		.map(|(at, _)| measure(&figure(at), TICK_SIZE))
		.fold(0.0f64, f64::max))
		.min(220.0);
	let left = PAD + label_column_width(spec) + 10.0;
	let right = WIDTH - PAD - figure_width - 10.0 - if share_width > 0.0 { share_width + 8.0 } else { 0.0 };
	let ends: Vec<f64> = categories
		.iter()
		.enumerate()
		.flat_map(|(at, _)| {
			let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) else { return Vec::new() };
			let Some(target) = targets.get(at).and_then(|target| target.as_ref()) else { return Vec::new() };
			let _ = target;
			vec![position(at, point.value), position(at, point.upper.unwrap_or(point.value))]
		})
		.collect();
	// Values off the axis need only a stretch past the target to run off; the axis spans the rest.
	let within: Vec<f64> = ends.iter().copied().filter(|value| *value <= reach).collect();
	let room = if ends.len() > within.len() { vec![goal * OVERRUN_ROOM] } else { Vec::new() };
	let mut extent = vec![0.0, goal];
	extent.extend(within.iter().copied());
	extent.extend(room.iter().copied());
	let scale = value_scale(&extent, left, right, true, false, if shared { tick_unit(entry, &extent) } else { 1.0 });
	let last = reach.min(scale.ticks.last().copied().unwrap_or(reach));
	let end = scale.at(last);
	let x = |at: usize, value: f64| scale.at(position(at, value).min(last));
	let metrics = bar_metrics(categories.len(), 1);
	let row_height = label_row(spec, metrics.0);
	let bar = metrics.1;
	let estimates = estimated_points(entry);
	let top = 16.0;
	let tops = row_tops(spec, top, row_height);
	let bottom = *tops.last().unwrap_or(&0.0);
	let strong = strong_categories(spec);
	let mut parts: Vec<String> = Vec::new();
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	let percent_axis = ChartSeries { dim: Dimension::Percent, unit: String::new(), ..entry.clone() };
	parts.push(draw_value_axis(&scale, if shared { entry } else { &percent_axis }, top, bottom));
	parts.push(category_labels(spec, &tops, row_height, &strong));
	let rule = if shared { format!("{} {}", first.unwrap().text, word) } else { word.clone() };
	parts.push(reference_lines(
		&[ChartAnnotation::Reference { value: goal, label: rule, series: None }],
		&scale,
		left,
		right,
		top,
		bottom,
	));
	let emphasized = entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis));
	for (at, _) in categories.iter().enumerate() {
		let middle = tops[at] + row_height / 2.0;
		let y = middle - bar / 2.0;
		let point = entry.points.get(at).and_then(|point| point.as_ref());
		let target = targets.get(at).and_then(|target| target.as_ref());
		let (Some(point), Some(target)) = (point, target) else {
			let note = entry.notes.get(at).and_then(|note| note.clone());
			if let Some(note) = note {
				parts.push(note_text(left + 4.0, middle, &clip(&note, 40), false));
			}
			continue;
		};
		let zero = scale.at(0.0);
		let scale_of = target.upper.unwrap_or(target.value);
		parts.push(rect(zero, y, x(at, scale_of) - zero, bar, "border", TRACK_OPACITY, 0.0));
		if let Some(upper) = target.upper {
			parts.push(rect(x(at, target.value), y - 2.0, x(at, upper) - x(at, target.value), bar + 4.0, "success", 0.18, 0.0));
		}
		let fill = tone(at);
		let opacity = if emphasized && !point.emphasis { RECEDE } else { 1.0 };
		let low = x(at, point.value);
		let high = x(at, point.upper.unwrap_or(point.value));
		parts.push(bar_rect(zero, y, (low - zero).max(MIN_BAR), bar, fill, opacity, estimates.contains(&at)));
		if high > low {
			parts.push(rect(low, y, high - low, bar, fill, RANGE_OPACITY * opacity, 0.0));
		}
		// Past the axis' end: the bar runs off it, the break marked where it leaves.
		if position(at, point.upper.unwrap_or(point.value)) > last {
			parts.push(break_mark(end - 6.0, middle, bar / 2.0 + 2.0, "fg"));
		}
		let own = point_text(entry, at);
		// One target: the figure follows its bar, clear of the target's rule.
		let rule_x = scale.at(goal);
		let after = high.max(low) + if position(at, point.upper.unwrap_or(point.value)) > last { 10.0 } else { 6.0 };
		let mut text_x = if !shared {
			right + 10.0
		} else if after <= rule_x + 2.0 && after + measure(&own, TICK_SIZE) >= rule_x - 2.0 {
			rule_x + 6.0
		} else {
			after
		};
		let share = share_text(at);
		if share_width > 0.0 {
			let mut style = TextStyle::new(if fill == SERIES[0] { "fg" } else { fill });
			style.size = Some(TICK_SIZE);
			style.weight = Some(600);
			style.anchor = Some(Anchor::End);
			parts.push(text(text_x + share_width, middle, &share, &style));
			text_x += share_width + 8.0;
		}
		// The figure, then (each row held to its own) its target, muted: one line of runs.
		let standing = standings[at];
		let mut runs = vec![TextRun {
			content: own,
			fill: if point.emphasis || standing.map_or(true, |standing| !standing.met) { Some("fg".to_string()) } else { None },
			weight: if point.emphasis { Some(600) } else { None },
		}];
		if !shared {
			runs.push(TextRun { content: format!("\u{2002}{}", clip(&target.text, 18)), fill: None, weight: None });
		}
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		parts.push(text_runs(text_x, middle, &runs, &style));
	}
	let shown: HashSet<&str> = categories.iter().enumerate().map(|(at, _)| tone(at)).collect();
	let mut legend: Vec<LegendItem> = vec![
		LegendItem { name: format!("within {word}"), color: "success".to_string(), ring: false, tones: None, opacity: None },
		LegendItem { name: "within 10%".to_string(), color: "warning".to_string(), ring: false, tones: None, opacity: None },
		LegendItem { name: format!("misses {word}"), color: "error".to_string(), ring: false, tones: None, opacity: None },
	]
	.into_iter()
	.filter(|item| shown.contains(item.color.as_str()))
		.collect();
	if legend.len() <= 1 {
		legend.clear();
	}
	// A spread's two ends (`p50 / p99`), named under the figures they explain.
	let spread: Vec<&str> = entry.name.split('/').map(str::trim).collect();
	if spread.len() == 2 && entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.upper.is_some())) {
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::End);
		parts.push(text(
			WIDTH - PAD,
			bottom + 12.0,
			&clip(&format!("bar to {}, faint to {}", spread[0], spread[1]), 40),
			&style,
		));
	}
	Layer { height: bottom + 20.0, body: parts.join(""), legend: Some(legend), ramp: None, notes: Some(Vec::new()) }
}

/// A dotted hairline down from a waterfall step's end at `x`, from `y1` to `y2`.
fn connector(x: f64, y1: f64, y2: f64) -> String {
	if y2 <= y1 {
		return String::new();
	}
	format!(
		r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1" stroke-dasharray="2 2" stroke-opacity="0.8"/>"#,
		num(x),
		num(y1),
		num(x),
		num(y2),
		token("muted")
	)
}

/// Steps adding to a running sum, each bar from the last sum to the new one, the total at the end.
fn draw_waterfall(spec: &ChartSpec) -> Layer {
	let entry = &spec.series[0];
	let categories = &spec.categories;
	let steps: Vec<f64> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| entry.points.get(at).and_then(|point| point.as_ref()).map_or(0.0, |point| point.value))
		.collect();
	let mut run = 0.0;
	let ends: Vec<f64> = steps.iter().map(|value| {
		run += *value;
		run
	}).collect();
	let starts: Vec<f64> = ends.iter().zip(steps.iter()).map(|(end, value)| end - value).collect();
	let total = spec.total.clone().unwrap_or_else(|| FlowTotal {
		name: "Total".to_string(),
		point: ChartPoint { value: ends.last().copied().unwrap_or(0.0), text: String::new(), emphasis: false, upper: None, error: None },
	});
	let references: Vec<ChartAnnotation> = spec
		.annotations
		.as_deref()
		.map(|marks| marks.iter().filter(|mark| matches!(mark, ChartAnnotation::Reference { .. })).cloned().collect())
		.unwrap_or_default();
	let cap = references.first();
	let format = |value: f64| format_value(value, entry.dim, &entry.unit, value.abs());
	let total_text = if total.point.text.is_empty() { format(total.point.value) } else { total.point.text.clone() };
	let spare = cap.map_or(0.0, |cap| match cap {
		ChartAnnotation::Reference { value, .. } => *value - total.point.value,
		_ => 0.0,
	});
	let spare_text = cap.map_or_else(String::new, |_| format!("{} {}", format(spare.abs()), if spare >= 0.0 { "to spare" } else { "over" }));
	// Shares of a sum only read when every step adds to it.
	let rising = steps.iter().all(|value| *value >= 0.0) && total.point.value > 0.0;
	let share = |at: usize| -> String {
		if !rising {
			return String::new();
		}
		let percent = (100.0 * steps[at]) / total.point.value;
		if percent < 1.0 {
			"<1%".to_string()
		} else {
			format!("{}%", percent.round())
		}
	};
	let view = ChartSpec {
		categories: categories.iter().cloned().chain(std::iter::once(total.name.clone())).collect(),
		groups: spec.groups.as_ref().map(|groups| {
			let mut groups = groups.clone();
			groups.push(ChartGroup { name: String::new(), members: vec![total.name.clone()] });
			groups
		}),
		status: spec.status.as_ref().map(|status| {
			let mut status = status.clone();
			status.push(None);
			status
		}),
		..spec.clone()
	};
	let figure_width = |at: usize| {
		let shared = share(at);
		measure(&point_text(entry, at), TICK_SIZE) + if shared.is_empty() { 0.0 } else { measure(&format!(" · {shared}"), TICK_SIZE) }
	};
	let mut value_width = categories
		.iter()
		.enumerate()
		.map(|(at, _)| figure_width(at))
		.chain(std::iter::once(
			measure(&total_text, TICK_SIZE) * SEMIBOLD_WIDTH + if spare_text.is_empty() { 0.0 } else { measure(&format!(" · {spare_text}"), TICK_SIZE) },
		))
		.fold(0.0f64, f64::max)
		+ 8.0;
	value_width = value_width.min(200.0);
	let left = PAD + label_column_width(&view) + 10.0;
	let right = WIDTH - PAD - value_width;
	let mut extent = vec![0.0];
	extent.extend(starts.iter().copied());
	extent.extend(ends.iter().copied());
	extent.push(total.point.value);
	extent.extend(references.iter().filter_map(|mark| match mark {
		ChartAnnotation::Reference { value, .. } => Some(*value),
		_ => None,
	}));
	let scale = value_scale(&extent, left, right, true, false, 1.0);
	let count = categories.len() + 1;
	let (row, bar_height, _) = bar_metrics(count, 1);
	let row_height = label_row(&view, row);
	let top = if references.is_empty() { 0.0 } else { 16.0 };
	let tops = row_tops(&view, top, row_height);
	let bottom = *tops.last().unwrap_or(&0.0);
	let bar_top = |at: usize| tops[at] + (row_height - bar_height) / 2.0;
	let mut strong = strong_categories(spec);
	strong.insert(categories.len());
	let mut parts: Vec<String> = Vec::new();
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	parts.push(draw_value_axis(&scale, entry, top, bottom));
	parts.push(category_labels(&view, &tops, row_height, &strong));
	for mark in &references {
		let ChartAnnotation::Reference { value, label, .. } = mark else { continue };
		let x = scale.at(*value);
		parts.push(reference_rule(x, top - 2.0, x, bottom));
		let label = clip(label, MAX_CALLOUT_CHARS);
		let half = measure(&label, 10.0) / 2.0;
		let mut style = TextStyle::new("fg");
		style.size = Some(10.0);
		style.anchor = Some(Anchor::Middle);
		parts.push(text(x.max(left + half).min(WIDTH - PAD - half), top - 9.0, &label, &style));
	}
	let emphasized = entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis));
	let fall = move_tone(entry.polarity, 0.5).unwrap_or(SERIES[1]);
	for (at, _) in categories.iter().enumerate() {
		let point = entry.points.get(at).and_then(|point| point.as_ref());
		let y = bar_top(at);
		let from = scale.at(starts[at].min(ends[at]));
		let to = scale.at(starts[at].max(ends[at]));
		let context = emphasized && !point.map_or(false, |point| point.emphasis);
		parts.push(rect(
			from,
			y,
			(to - from).max(1.0),
			bar_height,
			if context { "muted" } else if steps[at] < 0.0 { fall } else { SERIES[0] },
			if context { 0.55 } else { 1.0 },
			0.0,
		));
		// The running sum carries down to where the next step (or the total) starts.
		let x = scale.at(ends[at]);
		parts.push(connector(x, y + bar_height, bar_top(at + 1)));
		let figure = point_text(entry, at);
		let mut style = TextStyle::new(if point.map_or(false, |point| point.emphasis) { "fg" } else { "muted" });
		style.size = Some(TICK_SIZE);
		style.weight = if point.map_or(false, |point| point.emphasis) { Some(600) } else { None };
		parts.push(text(to + 6.0, y + bar_height / 2.0, &figure, &style));
		let shared = share(at);
		if !shared.is_empty() {
			let mut style = TextStyle::new("muted");
			style.size = Some(TICK_SIZE);
			let emphasis = point.map_or(false, |point| point.emphasis);
			parts.push(text(
				to + 6.0 + measure(&figure, TICK_SIZE) * if emphasis { SEMIBOLD_WIDTH } else { 1.0 } + 4.0,
				y + bar_height / 2.0,
				&format!("· {shared}"),
				&style,
			));
		}
	}
	let y = bar_top(categories.len());
	let zero = scale.at(0.0);
	let end = scale.at(total.point.value);
	parts.push(rect(zero.min(end), y, (end - zero).abs().max(1.0), bar_height, "muted", 1.0, 0.0));
	// The headroom left under the cap, or the overrun past it, as a span of the total's row.
	let cap_x = cap.map_or(end, |cap| match cap {
		ChartAnnotation::Reference { value, .. } => scale.at(*value),
		_ => end,
	});
	if let Some(_) = cap {
		if spare > 0.0 {
			parts.push(rect(end, y, cap_x - end, bar_height, "success", 0.18, 0.0));
		}
		if spare < 0.0 {
			parts.push(rect(cap_x, y, end - cap_x, bar_height, "error", 0.85, 0.0));
		}
	}
	let figure_x = zero.max(end) + 6.0;
	let mut style = TextStyle::new("fg");
	style.size = Some(TICK_SIZE);
	style.weight = Some(600);
	parts.push(text(figure_x, y + bar_height / 2.0, &total_text, &style));
	if !spare_text.is_empty() {
		let after = figure_x + measure(&total_text, TICK_SIZE) * SEMIBOLD_WIDTH + 6.0;
		// Inside the headroom span when it holds the words; else clear of the cap's rule.
		let inside = spare > 0.0 && after + measure(&spare_text, TICK_SIZE) <= cap_x - 4.0;
		let mut style = TextStyle::new(if spare >= 0.0 { "success" } else { "error" });
		style.size = Some(TICK_SIZE);
		style.weight = Some(600);
		parts.push(text(if inside || spare < 0.0 { after } else { cap_x + 6.0 }, y + bar_height / 2.0, &spare_text, &style));
	}
	Layer::plain(bottom + 20.0, parts.join(""))
}

/// A grouped category's own name: its member name, or the group's where the group alone names it.
fn member_name(spec: &ChartSpec, at: usize) -> String {
	let mut first = 0;
	if let Some(groups) = &spec.groups {
		for group in groups {
			if at < first + group.members.len() {
				return group.members.get(at - first).cloned().filter(|name| !name.is_empty()).unwrap_or_else(|| group.name.clone());
			}
			first += group.members.len();
		}
	}
	spec.categories.get(at).cloned().unwrap_or_default()
}

/// A duration as people say it: `48s`, `6m 36s`, `2h 5m`.
fn clock_text(seconds: f64) -> String {
	if seconds < 120.0 {
		return format_value(seconds, Dimension::Duration, "", seconds.abs());
	}
	let whole = seconds.round();
	let hours = (whole / 3600.0).floor();
	let minutes = ((whole % 3600.0) / 60.0).floor();
	let rest = whole % 60.0;
	if hours > 0.0 {
		return if minutes > 0.0 { format!("{hours:.0}h {minutes:.0}m") } else { format!("{hours:.0}h") };
	}
	if rest > 0.0 { format!("{minutes:.0}m {rest:.0}s") } else { format!("{minutes:.0}m") }
}

fn timeline_fit(options: &[String], x0: f64, x1: f64, lead: bool, size: f64) -> Option<String> {
	options
		.iter()
		.find(|option| measure(option, size) * if lead { SEMIBOLD_WIDTH } else { 1.0 } <= x1 - x0 - 10.0)
		.cloned()
}

fn timeline_inside(parts: &mut Vec<String>, even: bool, x0: f64, content: &str, lead: bool, size: f64) {
	let mut style = TextStyle::new("fg");
	style.size = Some(size);
	style.weight = if lead { Some(600) } else { None };
	if lead || even {
		parts.push(ink(x0 + 5.0, STRIP / 2.0 + 18.0, content, &style));
	} else {
		parts.push(text(x0 + 5.0, STRIP / 2.0 + 18.0, content, &style));
	}
}

/// Rows laid end to end on one axis as touching segments of one strip.
fn draw_timeline(spec: &ChartSpec) -> Layer {
	let entry = &spec.series[0];
	let categories = &spec.categories;
	let sizes: Vec<f64> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| entry.points.get(at).and_then(|point| point.as_ref()).map_or(0.0, |point| point.value.max(0.0)))
		.collect();
	let cumulative: Vec<f64> = {
		let mut run = 0.0;
		sizes
			.iter()
			.map(|size| {
				run += *size;
				run - *size
			})
			.collect()
	};
	let starts: Vec<f64> = spec.x.as_ref().map_or(cumulative, |axis| axis.values.clone());
	let ends: Vec<f64> = starts.iter().zip(sizes.iter()).map(|(start, size)| start + size).collect();
	let from = starts.iter().copied().fold(f64::INFINITY, f64::min).min(0.0);
	let to = ends.iter().copied().fold(f64::NEG_INFINITY, f64::max);
	let span = (to - from).max(1.0);
	let duration = entry.dim == Dimension::Duration;
	let format = |value: f64| if duration { clock_text(value) } else { format_value(value, entry.dim, &entry.unit, value.abs()) };
	// Parts at their offsets end where the last one does; a run of steps at its total.
    let end_text = if spec.x.is_some() {
		format(to)
	} else {
		spec.total
			.as_ref()
			.map_or_else(|| format(spec.total.as_ref().map_or(to, |total| total.point.value)), |total| {
				if total.point.text.is_empty() { format(total.point.value) } else { total.point.text.clone() }
		})
	};
	let left = PAD;
	let right = WIDTH - PAD - measure(&end_text, TICK_SIZE) * SEMIBOLD_WIDTH - 8.0;
	let at = |value: f64| left + ((value - from) / span) * (right - left);
	// Stretches: named groups as one, loose rows on their own.
	let mut stretches: Vec<Stretch> = Vec::new();
	let mut row = 0;
	let groups = spec.groups.clone().unwrap_or_else(|| vec![ChartGroup { name: String::new(), members: categories.clone() }]);
	for group in &groups {
		let first = row;
		row += group.members.len();
		if !group.name.is_empty() && group.members.len() > 1 {
			let value: f64 = sizes[first..row].iter().sum();
			stretches.push(Stretch { first, last: row - 1, name: group.name.clone(), value, figure: format(value), group: true });
		} else {
			for index in first..row {
				stretches.push(Stretch {
					first: index,
					last: index,
					name: categories.get(index).cloned().unwrap_or_default(),
					value: sizes[index],
					figure: point_text(entry, index),
					group: false,
				});
			}
		}
	}
	let total = sizes.iter().sum::<f64>().max(1.0);
	let share = |stretch: &Stretch| -> String {
		let percent = (100.0 * stretch.value) / total;
		if percent < 1.0 { "<1%".to_string() } else { format!("{}%", percent.round()) }
	};
	let strong = strong_categories(spec);
	let marked: Vec<usize> = stretches
		.iter()
		.enumerate()
		.filter(|(_, stretch)| strong.iter().any(|index| *index >= stretch.first && *index <= stretch.last))
		.map(|(index, _)| index)
		.collect();
	let leading: HashSet<usize> = if !marked.is_empty() {
		marked.into_iter().collect()
	} else {
		let mut by_size: Vec<usize> = (0..stretches.len()).collect();
		by_size.sort_by(|a, b| stretches[*b].value.partial_cmp(&stretches[*a].value).unwrap_or(std::cmp::Ordering::Equal));
		by_size.into_iter().take(MAX_LEADING).filter(|index| stretches[*index].value / total >= LEADING_SEGMENT).collect()
	};
	let mut parts: Vec<String> = Vec::new();
	let strip_top = 18.0;
	let strip_bottom = strip_top + STRIP;
	let middle = strip_top + STRIP / 2.0;
	// Axis over the strip: tick labels and short ticks, the strip itself the baseline.
	let unit = if duration { if to >= 7200.0 { 3600.0 } else if to >= 120.0 { 60.0 } else { 1.0 } } else { 1.0 };
	let step = nice_step(span / unit / 5.0) * unit;
	let mut tick = (from / step).ceil() * step;
	while tick <= to + 1e-9 {
		let x = at(tick);
		let label = format_value((tick * 1e12).round() / 1e12, entry.dim, &entry.unit, to);
		parts.push(rule(x, 12.0, x, strip_top - 2.0));
		// The first tick sits at the margin: its label starts there rather than hanging off the canvas.
		let edge = x - measure(&label, TICK_SIZE) / 2.0 < 2.0;
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(if edge { Anchor::Start } else { Anchor::Middle });
		parts.push(text(if edge { x - 1.0 } else { x }, 5.0, &label, &style));
		tick += step;
	}
	// Segments: leading stretches in the lead color, the rest alternating grays — or, where none leads,
	// every stretch in alternating strengths of the lead color; a sliver parts neighbours.
	let even = leading.is_empty();
	let fill = |index: usize| -> (&'static str, f64) {
		let odd = index % 2 == 1;
		if leading.contains(&index) {
			(SERIES[0], 1.0)
		} else if even {
			(SERIES[0], if odd { 0.7 } else { 1.0 })
		} else {
			("muted", if odd { 0.3 } else { 0.5 })
		}
	};
	for (index, stretch) in stretches.iter().enumerate() {
		let (color, opacity) = fill(index);
		for member in stretch.first..=stretch.last {
			let x0 = at(starts[member]);
			let width = (at(ends[member]) - x0 - if sizes[member] > 0.0 { 1.0 } else { 0.0 }).max(1.0);
			parts.push(rect(x0, strip_top, width, STRIP, color, opacity, 2.0));
		}
	}
	{
		let mut style = TextStyle::new("fg");
		style.size = Some(TICK_SIZE);
		style.weight = Some(600);
		parts.push(text(at(to) + 8.0, middle, &end_text, &style));
	}
	// Names inside their segments where they fit (with the figure and share, or less); the rest go to lanes.
	let mut pending: Vec<usize> = Vec::new();
	for (index, stretch) in stretches.iter().enumerate() {
		let lead = leading.contains(&index);
		let x0 = at(starts[stretch.first]);
		let x1 = at(ends[stretch.last]);
		let named = format!("{} {}", stretch.name, stretch.figure);
		let options = if lead {
			vec![format!("{named} · {}", share(stretch)), named.clone(), stretch.name.clone()]
		} else {
			vec![named.clone(), stretch.name.clone()]
		};
		let size = if lead { TICK_SIZE + 1.0 } else { TICK_SIZE };
		if stretch.group {
			let mut members: Vec<(usize, String)> = Vec::new();
			for member in stretch.first..=stretch.last {
				let options = vec![member_name(spec, member)];
				if let Some(name) = timeline_fit(&options, at(starts[member]), at(ends[member]), lead, 10.0) {
					members.push((member, name));
				}
			}
			// A group naming at most one member inside is better named itself, where its name fits.
			let own = if members.len() <= 1 { timeline_fit(&options, x0, x1, lead, size) } else { None };
			if let Some(own) = own {
				// The slivers between its members close behind the group's name.
				let (color, opacity) = fill(index);
				for member in stretch.first..stretch.last {
					parts.push(rect(at(ends[member]) - 1.0, middle - 8.0, 1.0, 16.0, color, opacity, 0.0));
				}
				timeline_inside(&mut parts, even, x0, &own, lead, size);
			} else {
				for (member, name) in members {
					timeline_inside(&mut parts, even, at(starts[member]), &name, lead, 10.0);
				}
				pending.push(index);
			}
			continue;
		}
		let own = timeline_fit(&options, x0, x1, lead, size);
		if let Some(own) = own {
			timeline_inside(&mut parts, even, x0, &own, lead, size);
		} else {
			pending.push(index);
		}
	}
	// Groups span a bracket under their rows, whether or not their name finds a lane.
	for (index, stretch) in stretches.iter().enumerate() {
		if !stretch.group {
			continue;
		}
		let x0 = at(starts[stretch.first]);
		let x1 = at(ends[stretch.last]);
		parts.push(line(x0 + 1.0, strip_bottom + 4.0, x1 - 1.0, strip_bottom + 4.0, if leading.contains(&index) { SERIES[0] } else { "muted" }, 1.5, 1.0));
	}
	let items: Vec<LaneItem> = pending
		.iter()
		.map(|index| {
			let stretch = &stretches[*index];
			let lead = leading.contains(index);
			let x0 = at(starts[stretch.first]);
			let x1 = at(ends[stretch.last]);
			let leader = if stretch.group { (x0 + x1) / 2.0 } else { (x0 + 1.0).max(((x0 + x1) / 2.0).min(x0 + 6.0)) };
			LaneItem {
				leader,
				lead,
				value: stretch.value,
				group: stretch.group,
				variants: vec![
					(stretch.name.clone(), if lead { format!("{} · {}", stretch.figure, share(stretch)) } else { stretch.figure.clone() }),
					(stretch.name.clone(), String::new()),
					(clip(&stretch.name, 12), String::new()),
				],
			}
		})
		.collect();
	let labels = place_lanes(&items, total);
	let mut lanes = 0;
	for label in &labels {
		lanes = lanes.max(label.lane + 1);
		let y = strip_bottom + LANE * (label.lane as f64 + 1.0);
		parts.push(line(label.leader, strip_bottom + if label.group { 5.0 } else { 1.0 }, label.leader, y - 6.0, "muted", 1.0, 0.7));
		let mut style = TextStyle::new("fg");
		style.size = Some(TICK_SIZE);
		style.weight = if label.lead { Some(600) } else { None };
		parts.push(text(label.x0, y, &label.name, &style));
		if !label.figure.is_empty() {
			let mut style = TextStyle::new("muted");
			style.size = Some(TICK_SIZE);
			parts.push(text(
				label.x0 + measure(&label.name, TICK_SIZE) * if label.lead { SEMIBOLD_WIDTH } else { 1.0 } + 6.0,
				y,
				&label.figure,
				&style,
			));
		}
	}
	let notes = if spec.x.is_some() { vec![format!("at {}", spec.x.as_ref().unwrap().name)] } else { vec!["end to end, in table order".to_string()] };
	Layer {
		height: strip_bottom + LANE * lanes as f64 + if lanes > 0 { 8.0 } else { 4.0 },
		body: parts.join(""),
		legend: None,
		ramp: None,
		notes: Some(notes),
	}
}

/// Lanes for `items` of a run of `total`: each in turn, in the shallowest lane where its box fits.
fn place_lanes(items: &[LaneItem], total: f64) -> Vec<LaneLabel> {
	let first: HashSet<usize> = items
		.iter()
		.enumerate()
		.filter(|(_, item)| item.lead || item.value / total >= LABEL_FIRST)
		.map(|(index, _)| index)
		.collect();
	let mut orders: Vec<Vec<usize>> = Vec::new();
	let mut by_leader_desc: Vec<usize> = (0..items.len()).collect();
	by_leader_desc.sort_by(|a, b| items[*b].leader.partial_cmp(&items[*a].leader).unwrap_or(std::cmp::Ordering::Equal));
	orders.push(by_leader_desc);
	let mut by_first: Vec<usize> = (0..items.len()).collect();
	by_first.sort_by(|a, b| {
		(!first.contains(b)) as i32 - (!first.contains(a)) as i32;
		items[*b].leader.partial_cmp(&items[*a].leader).unwrap_or(std::cmp::Ordering::Equal)
	});
	orders.push(by_first);
	let mut by_leader_asc: Vec<usize> = (0..items.len()).collect();
	by_leader_asc.sort_by(|a, b| items[*a].leader.partial_cmp(&items[*b].leader).unwrap_or(std::cmp::Ordering::Equal));
	orders.push(by_leader_asc);
	let mut by_value: Vec<usize> = (0..items.len()).collect();
	by_value.sort_by(|a, b| items[*b].value.partial_cmp(&items[*a].value).unwrap_or(std::cmp::Ordering::Equal));
	orders.push(by_value);
	let mut best: Vec<LaneLabel> = Vec::new();
	let mut best_score = [-1i64, -1, -1];
	for order in &orders {
		for terse in [false, true] {
			for inward in [false, true] {
				let mut placed: Vec<LaneLabel> = Vec::new();
				for index in order {
					let item = &items[*index];
					let variants: Vec<&(String, String)> = if terse && !first.contains(index) {
						item.variants.iter().filter(|(_, figure)| figure.is_empty()).collect()
					} else {
						item.variants.iter().collect()
					};
					for (name, figure) in variants {
						let width = measure(name, TICK_SIZE) * if item.lead { SEMIBOLD_WIDTH } else { 1.0 }
							+ if figure.is_empty() { 0.0 } else { 6.0 + measure(figure, TICK_SIZE) };
						// Right of the leader (from the margin at the left edge), else left of it.
						let mut anchors = vec![item.leader - 2.0, item.leader + 2.0 - width];
						anchors[0] = anchors[0].max(PAD);
						anchors.retain(|start| *start >= PAD && start + width <= WIDTH - PAD);
						if inward && item.leader > WIDTH / 2.0 {
							anchors.reverse();
						}
						let mut label: Option<LaneLabel> = None;
						for lane in 0..MAX_LANES {
							let x0 = anchors.iter().copied().find(|start| {
								placed.iter().all(|other| {
									if other.lane == lane {
										start + width + 8.0 < other.x0 || start - 8.0 > other.x1
									} else if other.lane < lane {
										item.leader < other.x0 - 3.0 || item.leader > other.x1 + 3.0
									} else {
										other.leader < start - 3.0 || other.leader > start + width + 3.0
									}
								})
							});
							if let Some(x0) = x0 {
								label = Some(LaneLabel { leader: item.leader, lead: item.lead, value: item.value, group: item.group, lane, x0, x1: x0 + width, name: name.clone(), figure: figure.clone() });
								break;
							}
						}
						if let Some(label) = label {
							placed.push(label);
							break;
						}
					}
				}
				let score = [
					placed.len() as i64,
                    placed.iter().map(|label| label.value as i64).sum(),
					placed.iter().filter(|label| !label.figure.is_empty()).count() as i64,
				];
				let mut at = None;
				for (slot, value) in score.iter().enumerate() {
					if *value != best_score[slot] {
						at = Some((*value > best_score[slot], slot));
						break;
					}
				}
				if let Some((better, _)) = at {
					if better {
						best = placed;
						best_score = score;
					}
				}
			}
		}
	}
	best
}

/// A matrix of rows by series: `heat` tinted cells, `bars` one column of bars per series.
fn draw_grid(spec: &ChartSpec, heat: bool) -> Layer {
	let series = &spec.series;
	let label_width = label_column_width(spec);
	let left = PAD + label_width + 10.0;
	let cell_width = (WIDTH - PAD - left) / series.len() as f64;
	let count = spec.categories.len();
	let row_height = label_row(spec, if heat { if count <= 8 { 26.0 } else if count <= 16 { 22.0 } else { 20.0 } } else if count <= 8 { 24.0 } else { 20.0 });
	let header_chars = ((cell_width - 6.0) / (TICK_SIZE * ADVANCE)).floor().max(4.0) as usize;
	let per_series = spec.shading == Some(Shading::Series);
	let mut parts: Vec<String> = Vec::new();
	let headers: Vec<Vec<String>> = series
		.iter()
		.map(|entry| wrap_header(&entry.name, cell_width - 6.0, TICK_SIZE * SEMIBOLD_WIDTH))
		.collect();
	let header_lines = headers.iter().map(|lines| lines.len()).fold(0usize, usize::max);
	for (index, entry) in series.iter().enumerate() {
		let x = left + index as f64 * cell_width;
		for (line, content) in headers[index].iter().enumerate() {
			// A one-line header sits on the line nearest the cells.
			let y = 6.0 + (header_lines - headers[index].len() + line) as f64 * 13.0;
			if heat {
				let mut style = TextStyle::new(if per_series { "fg" } else { "muted" });
				style.size = Some(TICK_SIZE);
				style.anchor = Some(Anchor::Middle);
				style.weight = if per_series { Some(600) } else { None };
				parts.push(text(x + cell_width / 2.0, y, content, &style));
			} else {
				let mut style = TextStyle::new("fg");
				style.size = Some(TICK_SIZE);
				style.weight = Some(600);
				parts.push(text(x + 2.0, y, content, &style));
			}
		}
		let _ = entry;
	}
	let top = 18.0 + (header_lines - 1) as f64 * 13.0;
	let tops = row_tops(spec, top, row_height);
	let bottom = *tops.last().unwrap_or(&0.0);
	// Heatmaps share one scale unless shaded per series or per row; small multiples scale each series on its own.
	let all: Vec<f64> = series.iter().flat_map(|entry| entry.points.iter().flatten().map(|point| point.value)).collect();
	let shared = shade(&all);
	let per_row = heat && spec.shading == Some(Shading::Row);
	let by_row: Vec<Box<dyn Fn(f64) -> f64>> = spec
		.categories
		.iter()
		.enumerate()
		.map(|(row, _)| shade(&series.iter().flat_map(|entry| entry.points.get(row).and_then(|point| point.as_ref()).map(|point| vec![point.value]).unwrap_or_default()).collect::<Vec<f64>>()))
		.collect();
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	parts.push(category_labels(spec, &tops, row_height, &strong_categories(spec)));
	// Whether any column printed factors of a reference row, which the header then names.
	let mut factor_noted = false;
	for (index, entry) in series.iter().enumerate() {
		let x = left + index as f64 * cell_width;
		let values: Vec<f64> = entry.points.iter().flatten().map(|point| point.value).collect();
		let own = shade(&values);
		// Bars reach their stated spread (`32 ± 7`) too.
		let reach: Vec<f64> = entry.points.iter().flatten().map(|point| point.value + point.error.unwrap_or(0.0)).collect();
		// Bars beside reference rows print their factor of their group's reference.
		let all_factors: Vec<Option<f64>> = if heat { Vec::new() } else { reference_factors(spec, entry) };
		let ratios = all_factors.iter().any(|factor| matches!(factor, Some(factor) if *factor >= 2.0 || *factor <= 0.5));
		let leader_at = |row: usize| spec.leaders.as_ref().and_then(|leaders| leaders.get(row)).copied().flatten() == Some(index);
		// Room past each bar for its figure, factor and `best` tag.
		let need = |best: bool, factored: bool| {
			entry
				.points
				.iter()
				.enumerate()
				.map(|(row, point)| {
					let factor = all_factors.get(row).copied().flatten();
					measure(point.as_ref().map_or("", |point| &point.text), TICK_SIZE) * SEMIBOLD_WIDTH
						+ if factored && factor.is_some() { measure(&reference_text(factor.unwrap(), ratios), TICK_SIZE - 1.0) + 6.0 } else { 0.0 }
						+ if best && (entry.best == Some(row) || leader_at(row)) { BEST_WIDTH } else { 0.0 }
				})
				.fold(0.0f64, f64::max)
				+ 18.0
		};
		let fits = |best: bool, factored: bool| cell_width - need(best, factored) >= MIN_BAR_SHARE * cell_width;
		let factored = fits(false, true);
		let tagged = fits(true, factored);
		let factors = if factored { all_factors.clone() } else { Vec::new() };
		let factor_texts: Vec<String> = factors.iter().map(|factor| factor.map_or_else(String::new, |factor| reference_text(factor, ratios))).collect();
		if factor_texts.iter().any(|text| !text.is_empty()) {
			factor_noted = true;
		}
		let bar_space = (MIN_BAR_SHARE * cell_width).max(cell_width - need(tagged, factored));
		let bars = panel_scale(if heat { &values } else { &reach }, x + 2.0, x + 2.0 + bar_space);
		let reference = if heat { None } else { Some(reference_rules(spec, entry, &tops, row_height, &bars)) };
		let emphasized = entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis));
		// Columns are told apart by their headings: one hue for all, so color stays free for emphasis.
		let color = SERIES[0];
		let estimated = estimated_points(entry);
		for (row, point) in entry.points.iter().enumerate() {
			let Some(point) = point.as_ref() else {
				let note = entry.notes.get(row).and_then(|note| note.clone());
				if let Some(note) = note {
					parts.push(note_text(if heat { x + cell_width / 2.0 } else { x + 4.0 }, tops[row] + row_height / 2.0, &clip(&note, header_chars), heat));
				}
				continue;
			};
			let y = tops[row];
			let middle = y + row_height / 2.0;
			let best = entry.best == Some(row) || leader_at(row);
			if heat {
				let level = if per_series { own(point.value) } else if per_row { by_row[row](point.value) } else { shared(point.value) };
				parts.push(rect(x + 1.5, y + 1.5, cell_width - 3.0, row_height - 3.0, color, HEAT_FLOOR + (1.0 - HEAT_FLOOR) * level, 4.0));
				// The best cell of a column is outlined in the success color.
				if best {
					parts.push(outline(x + 1.5, y + 1.5, cell_width - 3.0, row_height - 3.0, "success"));
				}
				let mut style = TextStyle::new(if point.value == 0.0 { "muted" } else { "fg" });
				style.size = Some(TICK_SIZE);
				style.anchor = Some(Anchor::Middle);
				style.weight = if point.emphasis || best || level >= INK_LEVEL { Some(600) } else { None };
				let label = clip(&point.text, header_chars);
				if level >= INK_LEVEL {
					parts.push(ink(x + cell_width / 2.0, middle, &label, &style));
				} else {
					parts.push(text(x + cell_width / 2.0, middle, &label, &style));
				}
				continue;
			}
			let end = (x + 2.0 + MIN_BAR).max(bars.at(point.value));
			let thick = 14.0f64.min(row_height - 10.0);
			// Rows beside the bolded ones recede to gray.
			let context = emphasized && !point.emphasis;
			if let Some(cut) = &bars.cut {
				if point.value > cut.below {
					parts.push(cut_bar(cut, x + 2.0, end, middle - thick / 2.0, thick, if context { "muted" } else { color }, if context { 0.55 } else { 1.0 }));
				} else {
					parts.push(bar_rect(x + 2.0, middle - thick / 2.0, end - x - 2.0, thick, if context { "muted" } else { color }, if context { 0.55 } else { 1.0 }, estimated.contains(&row)));
				}
			} else {
				parts.push(bar_rect(x + 2.0, middle - thick / 2.0, end - x - 2.0, thick, if context { "muted" } else { color }, if context { 0.55 } else { 1.0 }, estimated.contains(&row)));
			}
			let spread = match point.error {
				Some(_) if bars.cut.is_none() => whisker(&bars, point, middle, thick / 2.0 - 1.0, "fg"),
				_ => String::new(),
			};
			// A figure clears its group's reference rule and the spread's whisker.
			let reference_x = reference.as_ref().and_then(|(_, ats)| ats[row]).unwrap_or(end);
			let figure_x = end.max(reference_x).max(if spread.is_empty() { end } else { bars.at(point.value + point.error.unwrap()) }) + 4.0;
			let mut style = TextStyle::new(if point.emphasis || best { "fg" } else { "muted" });
			style.size = Some(TICK_SIZE);
			style.weight = if point.emphasis || best { Some(600) } else { None };
			parts.push(text(figure_x, middle, &point.text, &style));
			let mut after = figure_x + measure(&point.text, TICK_SIZE) * if point.emphasis || best { SEMIBOLD_WIDTH } else { 1.0 } + 5.0;
			if !factor_texts[row].is_empty() {
				let tone = move_tone(entry.polarity, factors[row].unwrap()).unwrap_or("muted");
				let mut style = TextStyle::new(tone);
				style.size = Some(TICK_SIZE - 1.0);
				parts.push(text(after, middle, &factor_texts[row], &style));
				after += measure(&factor_texts[row], TICK_SIZE - 1.0) + 6.0;
			}
			if best && tagged {
				parts.push(best_tag(after, middle, TICK_SIZE));
			}
		}
		if let Some((body, _)) = &reference {
			parts.push(body.clone());
		}
	}
	if !heat {
		for index in 1..series.len() {
			let x = left + index as f64 * cell_width - 3.0;
			parts.push(rule(x, top, x, bottom));
		}
		// Factors name what they are factors of, where every group compares with a row of one name.
		let bases: HashSet<String> = spec
			.reference
			.as_deref()
			.map(|reference| reference.iter().map(|at| clip(&member_name(spec, *at), 24)).collect())
			.unwrap_or_default();
		let mut notes = vec!["scaled per column".to_string()];
		if factor_noted && bases.len() == 1 {
			notes.push(format!("vs {}", bases.iter().next().unwrap()));
		}
		return Layer { height: bottom + 4.0, body: parts.join(""), legend: None, ramp: None, notes: Some(notes) };
	}
	let log_shade = log_shaded(&all);
	let outlined = series.iter().any(|entry| entry.best.is_some())
		|| spec.leaders.as_deref().map_or(false, |leaders| leaders.iter().any(|lead| lead.is_some()));
	let mut notes: Vec<String> = Vec::new();
	if per_series {
		notes.push("shaded per column".to_string());
	} else if per_row {
		notes.push("shaded per row".to_string());
	} else if log_shade {
		notes.push("log shading".to_string());
	}
	if outlined {
		notes.push("best outlined".to_string());
	}
	// The faintest swatch stands for where the shading starts.
	let least = all.iter().copied().fold(f64::INFINITY, f64::min);
	let ramp = if per_series || per_row || all.is_empty() {
		None
	} else {
		let low = if log_shade {
			all.iter().copied().filter(|value| *value > 0.0).fold(f64::INFINITY, f64::min)
		} else {
			least.min(heat_floor(&all))
		};
		Some(RampKey {
			color: SERIES[0].to_string(),
			low: format_value(low, series[0].dim, &series[0].unit, low.abs()),
			high: format_value(all.iter().copied().fold(f64::NEG_INFINITY, f64::max), series[0].dim, &series[0].unit, all.iter().copied().fold(f64::NEG_INFINITY, f64::max).abs()),
		})
	};
	Layer { height: bottom + 4.0, body: parts.join(""), legend: None, ramp, notes: Some(notes) }
}

/// Small multiples as bands of panels, one per series, each scaling its own bars.
fn draw_panels(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let mut parts: Vec<String> = Vec::new();
	let bands = (series.len() + PANELS_ACROSS - 1) / PANELS_ACROSS;
	let across = (series.len() + bands - 1) / bands;
	let gap = 18.0;
	let panel_width = (WIDTH - 2.0 * PAD - gap * (across as f64 - 1.0)) / across as f64;
	let panel_height = 20.0 + categories.len() as f64 * 14.0 + 14.0;
	let top = 0.0;
	let title_chars = (panel_width / (TICK_SIZE * ADVANCE)).floor() as usize;
	let panel_spec = ChartSpec { groups: None, ..spec.clone() };
	for (index, entry) in series.iter().enumerate() {
		let x = PAD + (index % across) as f64 * (panel_width + gap);
		let y = top + (index / across) as f64 * panel_height;
		let mut style = TextStyle::new("fg");
		style.size = Some(TICK_SIZE);
		style.weight = Some(600);
		parts.push(text(x, y + 6.0, &clip(&entry.name, title_chars), &style));
		let text_width = entry
			.points
			.iter()
			.enumerate()
			.map(|(at, _)| measure(&point_text(entry, at), 10.0))
			.fold(0.0f64, f64::max);
		let bar_space = (panel_width - text_width - 6.0 - if entry.best.is_some() { BEST_WIDTH } else { 0.0 }).max(0.0);
		let reach: Vec<f64> = entry.points.iter().flatten().map(|point| point.value + point.error.unwrap_or(0.0)).collect();
		let scale = panel_scale(&reach, x, x + bar_space);
		// Categories are colors here, never nested: a reference rule runs through the whole panel.
		let rows: Vec<f64> = categories.iter().enumerate().map(|(at, _)| y + 20.0 + at as f64 * 14.0).collect();
		let (reference_body, reference_at) = reference_rules(&panel_spec, entry, &rows, 14.0, &scale);
		let emphasized = entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.emphasis));
		for (at, point) in entry.points.iter().enumerate() {
			let middle = y + 27.0 + at as f64 * 14.0;
			let Some(point) = point.as_ref() else {
				let note = entry.notes.get(at).and_then(|note| note.clone());
				if let Some(note) = note {
					parts.push(note_text(x, middle, &clip(&note, title_chars), false));
				}
				continue;
			};
			let end = (x + MIN_BAR).max(scale.at(point.value));
			let color = SERIES[at % SERIES.len()];
			let opacity = if emphasized && !point.emphasis { RECEDE } else { 1.0 };
			if let Some(cut) = &scale.cut {
				if point.value > cut.below {
					parts.push(cut_bar(cut, x, end, middle - 5.0, 10.0, color, opacity));
				} else {
					parts.push(rect(x, middle - 5.0, end - x, 10.0, color, opacity, 0.0));
				}
			} else {
				parts.push(rect(x, middle - 5.0, end - x, 10.0, color, opacity, 0.0));
			}
			let spread = match point.error {
                Some(_) if scale.cut.is_none() => whisker(&scale, point, middle, 4.0, "fg"),
				_ => String::new(),
			};
			let best = entry.best == Some(at);
			let figure_x = end.max(reference_at[at].unwrap_or(end)).max(if spread.is_empty() { end } else { scale.at(point.value + point.error.unwrap()) }) + 4.0;
			let mut style = TextStyle::new(if point.emphasis || best { "fg" } else { "muted" });
			style.size = Some(10.0);
			style.weight = if point.emphasis || best { Some(600) } else { None };
			parts.push(text(figure_x, middle, &point_text(entry, at), &style));
			if best {
				parts.push(best_tag(figure_x + measure(&point_text(entry, at), 10.0) * SEMIBOLD_WIDTH + 4.0, middle, 10.0));
			}
		}
		parts.push(reference_body);
	}
	let legend: Vec<LegendItem> = categories
		.iter()
		.enumerate()
		.map(|(at, name)| LegendItem { name: name.clone(), color: SERIES[at % SERIES.len()].to_string(), ring: false, tones: None, opacity: None })
		.collect();
	Layer {
		height: top + bands as f64 * panel_height - 10.0,
		body: parts.join(""),
		legend: Some(legend),
		ramp: None,
		notes: Some(vec!["scaled per panel".to_string()]),
	}
}

/// Each metric's move from the baseline as a bar from no change.
fn draw_change(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let single = series.len() == 1;
	let points: Vec<&ChartPoint> = series.iter().flat_map(|entry| entry.points.iter().flatten()).collect();
	let style = if points.iter().all(|point| point.value <= PERCENT_REACH && (point.value == 0.0 || point.value >= 1.0 / PERCENT_REACH)) {
		MoveStyle::Percent
	} else {
		MoveStyle::Factor
	};
	let logs: Vec<f64> = points.iter().filter(|point| point.value > 0.0).map(|point| point.value.log10()).collect();
	let least_log = logs.iter().copied().fold(f64::INFINITY, f64::min).min(0.0);
	let mut low = -round_factor(-least_log);
	let mut high = round_factor(logs.iter().copied().fold(f64::NEG_INFINITY, f64::max).max(0.0));
	// A drop to zero has no factor: on the log axis its bar runs off the axis' end through a break.
	if points.iter().any(|point| point.value == 0.0) {
		low = low.min(-(2.0f64.log10()));
	}
	if low == high {
		high = FACTOR_STOPS[0].log10();
	}
	let mut parts: Vec<String> = Vec::new();
	let top = 0.0;
	let factors: Vec<String> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| {
			let point = series[0].points.get(at).and_then(|point| point.as_ref());
			if single && point.is_some() { factor_move(point.unwrap().value, style) } else { String::new() }
		})
		.collect();
	let factor_width = factors.iter().map(|label| measure(label, TICK_SIZE)).fold(0.0f64, f64::max);
	let figures: Vec<String> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| if single { series[0].points.get(at).and_then(|point| point.as_ref()).map_or(String::new(), |point| point.text.clone()) } else { String::new() })
		.collect();
	let figure_width = figures.iter().map(|label| measure(label, TICK_SIZE)).fold(0.0f64, f64::max);
	let label_width = label_column_width(spec);
	// Long figures take up to FIGURE_SHARE of the room right of the labels.
	let value_width = if single {
		((WIDTH - 2.0 * PAD - label_width - 10.0) * FIGURE_SHARE).max(280.0).min(factor_width + 10.0 + figure_width) + 8.0
	} else {
		points.iter().map(|point| measure(&factor_move(point.value, style), 10.0)).fold(0.0f64, f64::max) + 8.0
	};
	let left = PAD + label_width + 10.0;
	let right = WIDTH - PAD - value_width;
	let metrics = bar_metrics(categories.len(), series.len());
	let row_height = label_row(spec, metrics.0);
	let bar_height = metrics.1;
	let tops = row_tops(spec, top, row_height);
	let grid_bottom = *tops.last().unwrap_or(&0.0);
	// Where a factor lands, and the ticks.
	let mut ticks: Vec<(f64, String)> = Vec::new();
	let x: Box<dyn Fn(f64) -> f64> = if matches!(style, MoveStyle::Percent) {
		// Moves as written (whole percents): a `same` never stretches the axis to a side of its own.
		let percents: Vec<f64> = points.iter().map(|point| ((point.value - 1.0) * 100.0).round()).collect();
		let mut extent = percents.clone();
		if percents.iter().any(|percent| *percent < 0.0) {
			extent.push(-MIN_PERCENT_REACH);
		}
		if percents.iter().any(|percent| *percent > 0.0) {
			extent.push(MIN_PERCENT_REACH);
		}
		let scale = value_scale(&extent, left, right, true, false, 1.0);
		for tick in &scale.ticks {
			let label = if *tick == 0.0 {
				"0%".to_string()
			} else {
				format!("{}{}%", if *tick > 0.0 { "+" } else { "−" }, compact(tick.abs(), tick.abs()))
			};
			ticks.push((*tick / 100.0 + 1.0, label));
		}
		Box::new(move |factor| scale.at((factor - 1.0) * 100.0))
	} else {
		let at = |log: f64| left + ((log - low) / (high - low)) * (right - left);
		let decade = (right - left) / (high - low);
		let mut logs = vec![0.0f64];
		for end in [low, high] {
			if end.abs() * decade >= FACTOR_TICK_GAP {
				logs.push(end);
			}
		}
		let mut power = low.ceil() as i64;
		while power <= high.floor() as i64 {
			let at_power = power as f64;
			if logs.iter().all(|tick| (at_power - tick).abs() * decade >= FACTOR_TICK_GAP) {
				logs.push(at_power);
			}
			power += 1;
		}
		for log in logs {
			let label = if log == 0.0 {
				"1×".to_string()
			} else {
				format!("{}× {}", compact_factor(10.0f64.powf(log.abs())), if log < 0.0 { "less" } else { "more" })
			};
			ticks.push((10.0f64.powf(log), label));
		}
		Box::new(move |factor| if factor > 0.0 { at(factor.log10()) } else { at(low) })
	};
	for (tick, label) in &ticks {
		parts.push(rule(x(*tick), top, x(*tick), grid_bottom));
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::Middle);
		parts.push(text(x(*tick), grid_bottom + 12.0, label, &style));
	}
	// Neutral bars share one color; beside toned ones they step back.
	let toned = categories.iter().enumerate().any(|(row, _)| {
		series.iter().any(|entry| {
			entry.points.get(row).and_then(|point| point.as_ref()).map_or(false, |point| {
				move_tone(spec.category_polarity.as_ref().and_then(|polarity| polarity.get(row)).copied().flatten().or(entry.polarity), point.value).is_some()
			})
		})
	});
	let emphasized = points.iter().any(|point| point.emphasis);
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	parts.push(category_labels(spec, &tops, row_height, &strong_categories(spec)));
	for (row, _) in categories.iter().enumerate() {
		let row_top = tops[row];
		let middle = row_top + row_height / 2.0;
		for (index, entry) in series.iter().enumerate() {
			let Some(point) = entry.points.get(row).and_then(|point| point.as_ref()) else { continue };
			let y = if single { middle - bar_height / 2.0 } else { row_top + 5.0 + index as f64 * metrics.2 };
			let end = x(point.value);
			// A metric with a polarity reads its move as an improvement or a regression.
			let tone = move_tone(spec.category_polarity.as_ref().and_then(|polarity| polarity.get(row)).copied().flatten().or(entry.polarity), point.value);
			// One series takes the move's tone (neutral otherwise); several keep their own colors.
			let color = if single { tone.unwrap_or(if toned { "muted" } else { SERIES[0] }) } else { SERIES[index % SERIES.len()] };
			let opacity = if emphasized && !point.emphasis { RECEDE } else { 1.0 };
			if matches!(style, MoveStyle::Factor) && point.value == 0.0 {
				let cut = end + CLIP_INSET;
				parts.push(rect(end, y, CLIP_INSET - CUT_GAP / 2.0, bar_height, color, opacity, 0.0));
				parts.push(rect(cut + CUT_GAP / 2.0, y, x(1.0) - cut - CUT_GAP / 2.0, bar_height, color, opacity, 0.0));
				parts.push(break_mark(cut, y + bar_height / 2.0, bar_height / 2.0 + 1.0, "muted"));
			} else {
				parts.push(rect(x(1.0).min(end), y, (end - x(1.0)).abs().max(1.0), bar_height, color, opacity, 0.0));
			}
			if !single {
				let mut move_style = TextStyle::new(tone.unwrap_or("muted"));
				move_style.size = Some(10.0);
				parts.push(text(right + 8.0, y + bar_height / 2.0, &factor_move(point.value, style), &move_style));
			}
		}
	if single {
			let point = series[0].points.get(row).and_then(|point| point.as_ref());
			let strong = if point.map_or(false, |point| point.emphasis) { Some(600) } else { None };
			let tone = point.and_then(|point| move_tone(spec.category_polarity.as_ref().and_then(|polarity| polarity.get(row)).copied().flatten().or(series[0].polarity), point.value));
			let mut text_style = TextStyle::new(tone.unwrap_or("fg"));
			text_style.size = Some(TICK_SIZE);
			text_style.weight = strong;
			parts.push(text(right + 8.0, middle, &factors[row], &text_style));
			let mut figure_style = TextStyle::new("muted");
			figure_style.size = Some(TICK_SIZE);
			parts.push(text(
				right + 18.0 + factor_width,
				middle,
				&fit_text(&figures[row], value_width - 17.0 - factor_width, TICK_SIZE),
				&figure_style,
			));
		}
	}
	parts.push(line(x(1.0), top - 2.0, x(1.0), grid_bottom + 2.0, "muted", 1.5, 1.0));
	let versus = format!("change vs {}", spec.baseline.clone().unwrap_or_else(|| "baseline".to_string()));
	let notes = if matches!(style, MoveStyle::Percent) { vec![versus] } else { vec![versus, "log scale".to_string()] };
	let legend: Vec<LegendItem> = if single {
		Vec::new()
	} else {
		series
			.iter()
			.enumerate()
			.map(|(at, entry)| LegendItem { name: entry.name.clone(), color: SERIES[at % SERIES.len()].to_string(), ring: false, tones: None, opacity: None })
			.collect()
	};
	Layer { height: grid_bottom + 20.0, body: parts.join(""), legend: Some(legend), ramp: None, notes: Some(notes) }
}

/// A labelled item with a vertical position, for `spread_apart`.
trait YSpan {
	fn y(&self) -> f64;
	fn set_y(&mut self, y: f64);
}

impl YSpan for StageLabel {
	fn y(&self) -> f64 { self.y }
	fn set_y(&mut self, y: f64) { self.y = y; }
}

impl YSpan for EndLabel {
	fn y(&self) -> f64 { self.y }
	fn set_y(&mut self, y: f64) { self.y = y; }
}

/// `items` sorted by `y` and nudged at least `gap` apart, kept between `top` and `bottom` where they fit.
fn spread_apart<T: YSpan>(items: &mut [T], top: f64, bottom: f64, gap: f64) {
	items.sort_by(|a, b| a.y().partial_cmp(&b.y()).unwrap_or(std::cmp::Ordering::Equal));
	for at in 1..items.len() {
		let low = items[at - 1].y() + gap;
		let y = items[at].y().max(low);
		items[at].set_y(y);
	}
	for at in (0..items.len()).rev() {
		let limit = if at == items.len() - 1 { bottom } else { items[at + 1].y() - gap };
		let y = items[at].y().clamp(top, limit);
		items[at].set_y(y);
	}
}

/// Values across ordered stages as lines from each stage to the next.
fn draw_slope(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let first = &series[0];
	let last = &series[series.len() - 1];
	let strong = strong_categories(spec);
	let steps: Vec<Vec<Option<&'static str>>> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| {
			series[1..]
				.iter()
				.enumerate()
				.map(|(index, entry)| {
					let a = series[index].points.get(at).and_then(|point| point.as_ref());
					let b = entry.points.get(at).and_then(|point| point.as_ref());
					match (a, b) {
						(Some(a), Some(b)) => pair_tone(&series[index], entry, a, b),
						_ => None,
					}
				})
				.collect()
		})
		.collect();
	let toned = steps.iter().any(|tones| tones.iter().any(|tone| tone.is_some()));
	let own = |at: usize| {
		if toned {
			"muted"
		} else if categories.len() <= SERIES.len() {
			SERIES[at % SERIES.len()]
		} else if strong.contains(&at) {
			SERIES[0]
		} else {
			"muted"
		}
	};
	let ends: Vec<Option<(&ChartPoint, &ChartPoint)>> = categories
		.iter()
		.enumerate()
		.map(|(at, _)| match (first.points.get(at).and_then(|point| point.as_ref()), last.points.get(at).and_then(|point| point.as_ref())) {
			(Some(a), Some(b)) => Some((a, b)),
			_ => None,
		})
		.collect();
	let pairs: Vec<(&ChartPoint, &ChartPoint)> = ends.iter().copied().flatten().collect();
	let style = move_style(last, &pairs);
	let nets: Vec<String> = ends
		.iter()
		.map(|pair| pair.map_or_else(String::new, |(a, b)| move_text(a, b, last, style)))
		.collect();
	let net_tone: Vec<Option<&'static str>> = ends
		.iter()
		.map(|pair| pair.and_then(|(a, b)| pair_tone(first, last, a, b)))
		.collect();
	// Categories sharing a figure at a stage share one label.
	let labels_at = |entry: &ChartSeries| {
		let mut by_text: Vec<(String, Vec<usize>)> = Vec::new();
		for (at, point) in entry.points.iter().enumerate() {
			let Some(point) = point.as_ref() else { continue };
			if let Some(group) = by_text.iter_mut().find(|(text, _)| *text == point.text) {
				group.1.push(at);
			} else {
				by_text.push((point.text.clone(), vec![at]));
			}
		}
		by_text.into_iter().map(|(text, names)| StageLabel { y: 0.0, text, names }).collect::<Vec<StageLabel>>()
	};
	let name_list = |names: &[usize]| names.iter().map(|at| categories.get(*at).cloned().unwrap_or_default()).collect::<Vec<String>>().join(", ");
	let net_of = |names: &[usize]| -> String {
		if names.is_empty() {
			return String::new();
		}
		let first_net = &nets[names[0]];
		if names.iter().all(|at| nets[*at] == *first_net) { first_net.clone() } else { String::new() }
	};
	let lefts = labels_at(first);
	let rights = labels_at(last);
	// A little slack past the estimate keeps a name that just fits from clipping.
	let left_width = lefts
		.iter()
		.map(|label| measure(&name_list(&label.names), TICK_SIZE) + 6.0 + measure(&label.text, TICK_SIZE))
		.fold(0.0f64, f64::max)
		.min(MAX_SLOPE_LABEL)
		+ 6.0;
	let right_width = rights
		.iter()
		.map(|label| {
			let net = net_of(&label.names);
			measure(&label.text, TICK_SIZE) * SEMIBOLD_WIDTH
				+ 6.0
				+ measure(&name_list(&label.names), TICK_SIZE)
				+ if net.is_empty() { 0.0 } else { 8.0 + measure(&net, TICK_SIZE) * SEMIBOLD_WIDTH }
		})
		.fold(0.0f64, f64::max)
		.min(MAX_SLOPE_LABEL);
	let x0 = PAD + left_width + 12.0;
	let x1 = WIDTH - PAD - right_width - 12.0;
	let xs: Vec<f64> = series
		.iter()
		.enumerate()
		.map(|(index, _)| x0 + (index as f64 / (series.len() - 1) as f64) * (x1 - x0))
		.collect();
	let plot_top = 30.0;
	let plot_height = (categories.len() as f64 * SLOPE_ROW).clamp(MIN_SLOPE_HEIGHT, MAX_SLOPE_HEIGHT);
	let plot_bottom = plot_top + plot_height;
	let values: Vec<f64> = series.iter().flat_map(|entry| entry.points.iter().flatten().map(|point| point.value)).collect();
	let scale = value_scale(&values, plot_bottom, plot_top, false, false, 1.0);
	let mut parts: Vec<String> = Vec::new();
	for (index, entry) in series.iter().enumerate() {
		parts.push(rule(xs[index], plot_top - 10.0, xs[index], plot_bottom + 6.0));
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::Middle);
		parts.push(text(xs[index], 8.0, &clip(&entry.name, 24), &style));
	}
	// Lines first, emphasized ones last so they cross over the rest; dots over every line.
	let mut order: Vec<usize> = (0..categories.len()).collect();
	order.sort_by_key(|at| strong.contains(at));
	let mut dots: Vec<String> = Vec::new();
	for at in order {
		let width = if strong.contains(&at) { 3.0 } else { 2.25 };
		for (index, entry) in series.iter().enumerate() {
			let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) else { continue };
			let x = xs[index];
			let y = scale.at(point.value);
			let before = if index > 0 { series[index - 1].points.get(at).and_then(|point| point.as_ref()) } else { None };
			let color = steps[at].get(index - 1).copied().flatten().unwrap_or_else(|| own(at));
			if let Some(before) = before {
				parts.push(line(xs[index - 1], scale.at(before.value), x, y, color, width, 0.85));
			}
			if index == 0 {
				dots.push(ring(x, y, 3.5, if toned { "muted" } else { own(at) }));
			} else {
				dots.push(dot(x, y, 3.5, color, 1.0));
			}
		}
	}
	parts.extend(dots);
	// Middle stages print their figures small, above their dots.
	for (index, entry) in series.iter().enumerate() {
		if index == 0 || index == series.len() - 1 {
			continue;
		}
		// Figures whose dots nearly meet join one label over the topmost, top to bottom.
		let mut natural: Vec<StageLabel> = labels_at(entry)
			.into_iter()
			.map(|mut label| {
				label.y = scale.at(entry.points[label.names[0]].as_ref().unwrap().value) - 9.0;
				label
			})
			.collect();
		natural.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
		let mut labels: Vec<StageLabel> = Vec::new();
		for mut label in natural {
			if let Some(above) = labels.last() {
				if label.y - above.y < 11.0 {
					let above = labels.last_mut().expect("label present");
					above.text = format!("{} · {}", above.text, label.text);
					above.names.append(&mut label.names);
					continue;
				}
			}
			labels.push(label);
		}
		spread_apart(&mut labels, plot_top - 8.0, plot_bottom, 11.0);
		for label in &labels {
			let mut style = TextStyle::new("muted");
			style.size = Some(10.0);
			style.anchor = Some(Anchor::Middle);
			parts.push(text(xs[index], label.y, &label.text, &style));
		}
	}
	let place = |labels: &mut Vec<StageLabel>, entry: &ChartSeries| {
		for label in labels.iter_mut() {
			label.y = scale.at(entry.points[label.names[0]].as_ref().unwrap().value);
		}
		spread_apart(labels, plot_top - 6.0, plot_bottom + 6.0, TICK_SIZE + 3.0);
		labels.clone()
	};
	let name_color = |names: &[usize]| {
		if toned || names.len() > 1 {
			"fg"
		} else {
			own(names[0])
		}
	};
	for label in place(&mut lefts.clone(), first) {
		let figure_width = measure(&label.text, TICK_SIZE);
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::End);
		parts.push(text(x0 - 12.0, label.y, &label.text, &style));
		let mut style = TextStyle::new(name_color(&label.names));
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::End);
		style.weight = if label.names.iter().any(|at| strong.contains(at)) { Some(600) } else { None };
		parts.push(text(
			x0 - 18.0 - figure_width,
			label.y,
			&fit_text(&name_list(&label.names), x0 - 18.0 - figure_width - PAD, TICK_SIZE),
			&style,
		));
	}
	for label in place(&mut rights.clone(), last) {
		let net = net_of(&label.names);
		let tone = if label.names.iter().all(|at| net_tone[*at] == net_tone[label.names[0]]) { net_tone[label.names[0]] } else { None };
		let mut x = x1 + 12.0;
		let mut style = TextStyle::new("fg");
		style.size = Some(TICK_SIZE);
		style.weight = Some(600);
		parts.push(text(x, label.y, &label.text, &style));
		x += measure(&label.text, TICK_SIZE) * SEMIBOLD_WIDTH + 6.0;
		let net_width = if net.is_empty() { 0.0 } else { 8.0 + measure(&net, TICK_SIZE) * SEMIBOLD_WIDTH };
		let names = fit_text(&name_list(&label.names), WIDTH - PAD - x - net_width, TICK_SIZE);
		let mut style = TextStyle::new(name_color(&label.names));
		style.size = Some(TICK_SIZE);
		style.weight = if label.names.iter().any(|at| strong.contains(at)) { Some(600) } else { None };
		parts.push(text(x, label.y, &names, &style));
		if !net.is_empty() {
			let mut style = TextStyle::new(tone.unwrap_or("muted"));
			style.size = Some(TICK_SIZE);
			style.weight = if tone.is_some() { Some(600) } else { None };
			parts.push(text(x + measure(&names, TICK_SIZE) + 8.0, label.y, &net, &style));
		}
	}
	let polarity = last.polarity.or(first.polarity);
	let mut notes: Vec<String> = Vec::new();
	if toned {
		let word = match polarity {
			Some(Polarity::Higher) => "higher",
			Some(Polarity::Lower) => "lower",
			None => "higher",
		};
		notes.push(format!("{word} is better"));
	}
	if scale.log {
		notes.push("log scale".to_string());
	}
	Layer::plain(plot_bottom + 10.0, parts.join(""))
}

/// Two opposing amounts per category as bars from one spine.
fn draw_butterfly(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let loss = &series[0];
	let gain = &series[1];
	let sides = [loss, gain];
	let size = |point: Option<&ChartPoint>| point.map_or(0.0, |point| point.value.abs());
	let nets: Vec<f64> = categories.iter().enumerate().map(|(at, _)| size(gain.points.get(at).and_then(|point| point.as_ref())) - size(loss.points.get(at).and_then(|point| point.as_ref()))).collect();
	let whole = sides.iter().all(|entry| entry.points.iter().all(|point| point.as_ref().map_or(true, |point| point.value.fract() == 0.0)));
	let net_text = |net: f64| {
		if net == 0.0 {
			return "±0".to_string();
		}
		let sign = if net > 0.0 { "+" } else { "−" };
		let magnitude = net.abs();
		let body = if whole { group_digits(&format!("{:.0}", magnitude)) } else { format_value(magnitude, gain.dim, &gain.unit, magnitude) };
		format!("{sign}{body}")
	};
	let figures: Vec<Vec<String>> = sides.iter().map(|entry| categories.iter().enumerate().map(|(at, _)| point_text(entry, at)).collect()).collect();
	let widest = |labels: &[String]| labels.iter().map(|label| measure(label, TICK_SIZE)).fold(0.0f64, f64::max);
	let net_width = measure("net", TICK_SIZE).max(widest(&nets.iter().map(|net| net_text(*net)).collect::<Vec<String>>()) * SEMIBOLD_WIDTH) + 12.0;
	let left = PAD + label_column_width(spec) + 10.0 + widest(&figures[0]) + 8.0;
	let right = WIDTH - PAD - net_width - widest(&figures[1]) - 8.0;
	let spine = (left + right) / 2.0;
	let reach = sides.iter().flat_map(|entry| entry.points.iter()).map(|point| point.as_ref().map_or(0.0, |point| point.value.abs())).fold(1.0f64, f64::max);
	let span = |value: f64| (value.abs() / reach) * (right - spine);
	let (row, bar_height, _) = bar_metrics(categories.len(), 1);
	let row_height = label_row(spec, row);
	let top = 18.0;
	let tops = row_tops(spec, top, row_height);
	let bottom = *tops.last().unwrap_or(&top);
	let estimates: Vec<HashSet<usize>> = sides.iter().map(|entry| estimated_points(entry)).collect();
	let mut parts: Vec<String> = Vec::new();
	for at in highlighted(spec) {
		parts.push(band(PAD - 4.0, tops[at], WIDTH - 2.0 * PAD + 8.0, row_height));
	}
	parts.push(category_labels(spec, &tops, row_height, &strong_categories(spec)));
	let mut muted_end = TextStyle::new("muted");
	muted_end.size = Some(TICK_SIZE);
	muted_end.anchor = Some(Anchor::End);
	let mut muted_start = TextStyle::new("muted");
	muted_start.size = Some(TICK_SIZE);
	parts.push(text(spine - 8.0, 6.0, &clip(&loss.name, 24), &muted_end));
	parts.push(text(spine + 8.0, 6.0, &clip(&gain.name, 24), &muted_start));
	parts.push(text(WIDTH - PAD, 6.0, "net", &muted_end));
	let emphasized = sides.iter().any(|entry| entry.points.iter().flatten().any(|point| point.emphasis));
	for (at, _) in categories.iter().enumerate() {
		let middle = tops[at] + row_height / 2.0;
		let y = middle - bar_height / 2.0;
		for (index, entry) in sides.iter().enumerate() {
			let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) else { continue };
			let length = MIN_BAR.max(span(point.value));
			let opacity = if emphasized && !point.emphasis { RECEDE } else { 1.0 };
			let weight = if point.emphasis { Some(600) } else { None };
			let estimate = estimates[index].contains(&at);
			if index == 0 {
				parts.push(bar_rect(spine - length, y, length, bar_height, "error", opacity, estimate));
				let mut style = TextStyle::new("muted");
				style.size = Some(TICK_SIZE);
				style.anchor = Some(Anchor::End);
				style.weight = weight;
				parts.push(text(spine - length - 6.0, middle, &figures[0][at], &style));
			} else {
				parts.push(bar_rect(spine, y, length, bar_height, "success", opacity, estimate));
				let mut style = TextStyle::new("muted");
				style.size = Some(TICK_SIZE);
				style.weight = weight;
				parts.push(text(spine + length + 6.0, middle, &figures[1][at], &style));
			}
		}
		let net = nets[at];
		let mut style = TextStyle::new(if net > 0.0 { "success" } else if net < 0.0 { "error" } else { "muted" });
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::End);
		style.weight = Some(600);
		parts.push(text(WIDTH - PAD, middle, &net_text(net), &style));
	}
	parts.push(line(spine, top - 4.0, spine, bottom + 4.0, "fg", 1.25, 0.6));
	Layer::plain(bottom + 8.0, parts.join(""))
}


/// Two conditions compared metric by metric (`stat`).
fn draw_stat(spec: &ChartSpec) -> Layer {
	let series = &spec.series;
	let categories = &spec.categories;
	let base = spec.reference.as_ref().map_or(0, |reference| reference.first().copied().unwrap_or(0));
	let other = 1 - base;
	let tile = (WIDTH - 2.0 * PAD - TILE_GAP * (series.len() as f64 - 1.0)) / series.len() as f64;
	let tile = tile.min(MAX_TILE);
	let mut parts: Vec<String> = Vec::new();
	let bar_top = 50.0;
	let bar_height = 10.0;
	let bar_step = 18.0;
	let height = bar_top + bar_step + bar_height;
	for (index, entry) in series.iter().enumerate() {
		let left = PAD + index as f64 * (tile + TILE_GAP);
		if index > 0 {
			parts.push(rule(left - TILE_GAP / 2.0, 0.0, left - TILE_GAP / 2.0, height));
		}
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		parts.push(text(left, 7.0, &fit_text(&entry.name, tile, TICK_SIZE), &style));
		let from = entry.points.get(base).and_then(|point| point.as_ref());
		let to = entry.points.get(other).and_then(|point| point.as_ref());
		let (Some(from), Some(to)) = (from, to) else { continue };
		let ratio = if from.value > 0.0 { ChartPoint { value: to.value / from.value, text: String::new(), emphasis: false, upper: None, error: None } } else { continue };
		let words = factor_text(&ratio);
		let size = FACTOR_SIZE.min((FACTOR_SIZE * tile) / (1.0f64).max(measure(&words, FACTOR_SIZE) * SEMIBOLD_WIDTH));
		let tone = move_tone(entry.polarity, ratio.value);
		let mut style = TextStyle::new(tone.unwrap_or("fg"));
		style.size = Some(size);
		style.weight = Some(600);
		parts.push(text(left, 30.0, &words, &style));
		// Figures show whole (`2,426 / 2,368 ms`) up to half the tile; the bars keep the rest.
		let figures: Vec<String> = [base, other].iter().map(|at| fit_text(&point_text(entry, *at), tile / 2.0, TICK_SIZE * SEMIBOLD_WIDTH)).collect();
		let figure_width = figures.iter().map(|figure| measure(figure, TICK_SIZE) * SEMIBOLD_WIDTH).fold(0.0f64, f64::max) + 8.0;
		let span = (tile - figure_width).max(24.0);
		let peak = from.value.max(to.value).max(1.0);
		let estimated = estimated_points(entry);
		for (at, point) in [from, to].into_iter().enumerate() {
			let y = bar_top + at as f64 * bar_step;
			let length = MIN_BAR.max((point.value.max(0.0) / peak) * span);
			let category_at = if at == 0 { base } else { other };
			parts.push(bar_rect(left, y, length, bar_height, if at == 0 { "muted" } else { SERIES[0] }, if at == 0 { 0.55 } else { 1.0 }, estimated.contains(&category_at)));
			let mut style = TextStyle::new(if at == 0 { "muted" } else { "fg" });
			style.size = Some(TICK_SIZE);
			style.weight = if at == 0 { None } else { Some(600) };
			parts.push(text(left + length + 6.0, y + bar_height / 2.0, &figures[at], &style));
		}
	}
	Layer {
		height,
		body: parts.join(""),
		legend: Some(vec![
			LegendItem { name: categories.get(base).cloned().unwrap_or_default(), color: "muted".to_string(), ring: false, tones: None, opacity: Some(0.55) },
			LegendItem { name: categories.get(other).cloned().unwrap_or_default(), color: SERIES[0].to_string(), ring: false, tones: None, opacity: None },
		]),
		ramp: None,
		notes: None,
	}
}

/// One bar split into the categories' shares.
fn draw_share(spec: &ChartSpec) -> Layer {
	let entry = &spec.series[0];
	let total = entry.points.iter().map(|point| point.as_ref().map_or(0.0, |point| point.value.max(0.0))).sum::<f64>().max(1.0);
	let counts = entry.dim != Dimension::Percent;
	let shares: Vec<String> = entry.points.iter().map(|point| match point.as_ref() {
		Some(point) if counts => percent_text((100.0 * point.value.max(0.0)) / total),
		_ => point.as_ref().map_or_else(String::new, |point| point.text.clone()),
	}).collect();
	let mut parts: Vec<String> = Vec::new();
	let bar_width = WIDTH - 2.0 * PAD;
	let mut x = PAD;
	for (at, point) in entry.points.iter().enumerate() {
		let Some(point) = point.as_ref() else { continue };
		if point.value <= 0.0 {
			continue;
		}
		let width = (point.value / total) * bar_width;
		// Segments part with a sliver of the canvas behind them.
		let opacity = part_opacity(spec, at);
		parts.push(rect(x, 0.0, (width - 2.0).max(1.0), 24.0, part_color(spec, at), opacity, 4.0));
		// A segment wide enough carries its figure inside, in ink to read on the solid color.
		let figure = &shares[at];
		let mut style = TextStyle::new("fg");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::Middle);
		style.weight = Some(600);
		if !figure.is_empty() && measure(figure, TICK_SIZE) * SEMIBOLD_WIDTH + 12.0 <= width - 2.0 {
			parts.push(if opacity >= INK_LEVEL { ink(x + (width - 2.0) / 2.0, 12.0, figure, &style) } else { text(x + (width - 2.0) / 2.0, 12.0, figure, &style) });
		}
		x += width;
	}
	let columns = if spec.categories.len() > 5 { 2 } else { 1 };
	let column_width = bar_width / columns as f64;
	let per_column = (spec.categories.len() as f64 / columns as f64).ceil() as usize;
	let names = clip_names(&spec.categories, ((column_width - 90.0) / (LABEL_SIZE * ADVANCE)).floor() as usize);
	for (at, name) in names.iter().enumerate() {
		let column = at / per_column;
		let lx = PAD + column as f64 * column_width;
		let ly = 42.0 + (at % per_column) as f64 * 20.0;
		let point = entry.points.get(at).and_then(|point| point.as_ref());
		parts.push(dot(lx + 5.0, ly, 4.5, part_color(spec, at), part_opacity(spec, at)));
		let style = TextStyle::new("fg");
		parts.push(text(lx + 16.0, ly, name, &style));
		let figure = match point {
			Some(point) if counts => format!("{} · {}", point.text, shares[at]),
			Some(point) => point.text.clone(),
			None => String::new(),
		};
		let mut style = TextStyle::new(if point.map_or(false, |point| point.emphasis) { "fg" } else { "muted" });
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::End);
		style.weight = if point.map_or(false, |point| point.emphasis) { Some(600) } else { None };
		parts.push(text(lx + column_width - 12.0, ly, &figure, &style));
	}
	// The whole the counts split, unless the caption already prints the table's total row.
	let notes = if counts && spec.caption.is_none() { vec![format!("100% = {}", group_digits(&format!("{:.0}", total)))] } else { Vec::new() };
	Layer { height: 42.0 + per_column as f64 * 20.0 - 6.0, body: parts.join(""), legend: None, ramp: None, notes: if notes.is_empty() { None } else { Some(notes) } }
}

/// Series values over an ordered x, as smooth lines with one axis per magnitude panel.
fn draw_line(spec: &ChartSpec) -> Layer {
	let categories = &spec.categories;
	let series = &spec.series;
	let panels = magnitude_panels(series);
	let values: Vec<Vec<f64>> = panels
		.iter()
		.map(|panel| {
			panel
				.iter()
				.flat_map(|index| series[*index].points.iter().flatten().map(|point| point.value))
				.collect()
		})
		.collect();
	let mut tick_width = 0.0f64;
	for panel_values in &values {
		let scale = value_scale(panel_values, 0.0, 1.0, false, false, 1.0);
		let format = tick_format(&scale, &series[0]);
		for tick in &scale.ticks {
			tick_width = tick_width.max(measure(&format(*tick), TICK_SIZE));
		}
	}
	let position = |at: usize| spec.x.as_ref().map_or(at as f64, |axis| axis.values.get(at).copied().unwrap_or(at as f64));
	let mut order: Vec<usize> = (0..categories.len()).collect();
	order.sort_by(|a, b| position(*a).partial_cmp(&position(*b)).unwrap_or(std::cmp::Ordering::Equal));
	// The rightmost sample of each line carries its value label.
	let lasts: Vec<Option<usize>> = series
		.iter()
		.map(|entry| {
			order
				.iter()
				.rev()
				.find(|at| entry.points.get(**at).and_then(|point| point.as_ref()).is_some())
				.copied()
		})
		.collect();
	let direct = series.len() > 1 && series.iter().all(|entry| measure(&entry.name, TICK_SIZE) * SEMIBOLD_WIDTH <= MAX_DIRECT_LABEL);
	let end_width = lasts
		.iter()
		.enumerate()
		.map(|(index, last)| {
			let name = if direct { measure(&series[index].name, TICK_SIZE) * SEMIBOLD_WIDTH + 6.0 } else { 0.0 };
			let figure = last
				.and_then(|last| series[index].points.get(last).and_then(|point| point.as_ref()))
				.map_or(0.0, |point| measure(&point.text, TICK_SIZE));
			name + figure
		})
		.fold(0.0f64, f64::max);
	let left = PAD + tick_width + 8.0;
	let right = WIDTH - PAD - (if direct { 230.0f64 } else { 120.0 }).min(end_width + 10.0);
	let count = categories.len();
	let x_scale = match &spec.x {
		Some(axis) => value_scale(&axis.values, left, right, false, false, 1.0),
		None => Scale::new(
			Box::new(move |index: f64| {
				left + if count == 1 { 0.0 } else { (index / (count as f64 - 1.0)) * (right - left) }
			}),
			Vec::new(),
			false,
			None,
		),
	};
	let x_at = |at: usize| x_scale.at(position(at));
	let plot_height = if panels.len() == 1 { 180.0 } else { (200.0 / panels.len() as f64).round().max(72.0) };
	let dotted = categories.len() <= MAX_DOTS;
	let references: Vec<(f64, String)> = spec
		.annotations
		.as_deref()
		.unwrap_or(&[])
		.iter()
		.filter_map(|mark| match mark {
			ChartAnnotation::Reference { value, label, .. } => Some((*value, label.clone())),
			_ => None,
		})
		.collect();
	let callouts: Vec<(usize, usize, String)> = spec
		.annotations
		.as_deref()
		.unwrap_or(&[])
		.iter()
		.filter_map(|mark| match mark {
			ChartAnnotation::Callout { category, series, text } => Some((*category, series.unwrap_or(0), text.clone())),
			_ => None,
		})
		.collect();
	let mut parts: Vec<String> = Vec::new();
	let mut top = 0.0f64;
	let mut plot_bottom = 0.0f64;
	for (panel_index, panel) in panels.iter().enumerate() {
		// Lines without direct labels need a legend in each of several panels (one panel's goes in the header).
		if series.len() > 1 && !direct && panels.len() > 1 {
			let items: Vec<LegendItem> = panel
				.iter()
				.map(|index| LegendItem {
					name: series[*index].name.clone(),
					color: SERIES[*index % SERIES.len()].to_string(),
					ring: false,
					tones: None,
					opacity: None,
				})
				.collect();
			let legend = draw_legend(&items, PAD, WIDTH - PAD, 6.0);
			parts.push(format!("<g transform=\"translate(0 {})\">{}</g>", num(top), legend.body));
			top += legend.height + 4.0;
		}
		// A callout over a line of this panel sets its text above the plot, clear of the panel's legend.
		let room = if callouts.iter().any(|(_, index, _)| panel.contains(index)) { 24.0 } else { 0.0 };
		let plot_top = top + 6.0 + room;
		plot_bottom = plot_top + plot_height;
		let scale = value_scale(&values[panel_index], plot_bottom, plot_top, false, false, 1.0);
		let format = tick_format(&scale, &series[0]);
		// Short panels label every other gridline.
		let label_every = if plot_height / (scale.ticks.len().max(1) as f64 - 1.0).max(1.0) < 20.0 { 2 } else { 1 };
		for (at, tick) in scale.ticks.iter().enumerate() {
			let y = scale.at(*tick);
			parts.push(rule(left, y, right, y));
			if at % label_every == 0 {
				let mut style = TextStyle::new("muted");
				style.size = Some(TICK_SIZE);
				style.anchor = Some(Anchor::End);
				parts.push(text(left - 8.0, y, &format(*tick), &style));
			}
		}
		let mut ends: Vec<EndLabel> = Vec::new();
		for index in panel {
			let entry = &series[*index];
			let color = SERIES[*index % SERIES.len()];
			// Runs of consecutive samples; a gap in the table breaks the line.
			let mut runs: Vec<Vec<(f64, f64)>> = vec![Vec::new()];
			for at in &order {
				match entry.points.get(*at).and_then(|point| point.as_ref()) {
					Some(point) => runs.last_mut().expect("a run is open").push((x_at(*at), scale.at(point.value))),
					None => {
						if runs.last().is_some_and(|run| !run.is_empty()) {
							runs.push(Vec::new());
						}
					}
				}
			}
			// The area reads as an amount, so it shades only down to a zero baseline, never to a cut-off axis.
			if panel.len() == 1 && scale.ticks.first() == Some(&0.0) {
				// The fade's id names its color, so two charts inlined in one page agree on it.
				parts.push(format!("<defs><linearGradient id=\"fade-{color}\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">"));
				parts.push(format!("<stop offset=\"0\" stop-color=\"{}\" stop-opacity=\"0.26\"/>", token(color)));
				parts.push(format!("<stop offset=\"1\" stop-color=\"{}\" stop-opacity=\"0\"/>", token(color)));
				parts.push("</linearGradient></defs>".to_string());
				for run in &runs {
					if run.len() < 2 {
						continue;
					}
					let floor = format!(
						"L{} {}L{} {}Z",
						num(run[run.len() - 1].0),
						num(plot_bottom),
						num(run[0].0),
						num(plot_bottom)
					);
					parts.push(format!("<path d=\"{}{floor}\" fill=\"url(#fade-{color})\"/>", smooth_path(run)));
				}
			}
			for run in &runs {
				if run.len() < 2 {
					continue;
				}
				parts.push(format!(
					"<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2.25\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
					smooth_path(run),
					token(color)
				));
			}
			let last = lasts[*index];
			for (at, point) in entry.points.iter().enumerate() {
				let Some(point) = point.as_ref() else { continue };
				if !(dotted || point.emphasis || Some(at) == last) {
					continue;
				}
				let radius = if point.emphasis { 4.5 } else if Some(at) == last { 3.5 } else { 2.5 };
				parts.push(dot(x_at(at), scale.at(point.value), radius, color, 1.0));
			}
			if let Some(end) = last.and_then(|last| entry.points.get(last).and_then(|point| point.as_ref())) {
				ends.push(EndLabel {
					x: x_at(last.expect("the end sample exists")) + 8.0,
					y: scale.at(end.value),
					text: end.text.clone(),
					color: color.to_string(),
					name: if direct { Some(entry.name.clone()) } else { None },
				});
			}
			for (category, callout_series, callout_text) in &callouts {
				let point = if *callout_series == *index { entry.points.get(*category).and_then(|point| point.as_ref()) } else { None };
				let Some(point) = point else { continue };
				let label = clip(callout_text, MAX_CALLOUT_CHARS);
				let half = (measure(&label, TICK_SIZE) * SEMIBOLD_WIDTH) / 2.0;
				let px = x_at(*category);
				let py = scale.at(point.value);
				parts.push(line(px, py - 6.0, px, py - 16.0, "muted", 1.0, 1.0));
				let mut style = TextStyle::new("fg");
				style.size = Some(TICK_SIZE);
				style.anchor = Some(Anchor::Middle);
				style.weight = Some(600);
				parts.push(text(px.max(left + half).min(right - half), py - 23.0, &label, &style));
			}
		}
		if panel_index == 0 {
			for (value, label) in &references {
				let y = scale.at(*value);
				if y < plot_top - 0.5 || y > plot_bottom + 0.5 {
					continue;
				}
				parts.push(reference_rule(left, y, right, y));
				let mut style = TextStyle::new("fg");
				style.size = Some(10.0);
				style.anchor = Some(Anchor::End);
				parts.push(text(right, y - 7.0, &clip(label, MAX_CALLOUT_CHARS), &style));
			}
		}
		parts.push(spread_labels(&mut ends, plot_top - 4.0, plot_bottom + 4.0));
		top = plot_bottom + 22.0;
	}
	// Category labels left to right, skipping any that would touch the last one drawn.
	// A label too long for its slot wraps to a second line before it clips.
	let slot = (right - left) / (categories.len().max(1) as f64 - 1.0).max(1.0);
	let labels: Vec<Vec<String>> = categories.iter().map(|name| wrap_header(name, slot.max(60.0) - 6.0, TICK_SIZE)).collect();
	let mut drawn_end = f64::NEG_INFINITY;
	let mut wrapped = false;
	for at in &order {
		let lines = &labels[*at];
		let width = lines.iter().map(|label| measure(label, TICK_SIZE)).fold(0.0f64, f64::max);
		let center = x_at(*at).max(left + width / 2.0).min(WIDTH - PAD - width / 2.0);
		if center - width / 2.0 < drawn_end + 8.0 {
			continue;
		}
		for (line, label) in lines.iter().enumerate() {
			let mut style = TextStyle::new("muted");
			style.size = Some(TICK_SIZE);
			style.anchor = Some(Anchor::Middle);
			parts.push(text(center, plot_bottom + 14.0 + line as f64 * 12.0, label, &style));
		}
		wrapped = wrapped || lines.len() > 1;
		drawn_end = center + width / 2.0;
	}
	let legend: Vec<LegendItem> = if series.len() > 1 && !direct && panels.len() == 1 {
		series
			.iter()
			.enumerate()
			.map(|(at, entry)| LegendItem {
				name: entry.name.clone(),
				color: SERIES[at % SERIES.len()].to_string(),
				ring: false,
				tones: None,
				opacity: None,
			})
			.collect()
	} else {
		Vec::new()
	};
	Layer {
		height: plot_bottom + if wrapped { 36.0 } else { 24.0 },
		body: parts.join(""),
		legend: if legend.is_empty() { None } else { Some(legend) },
		ramp: None,
		notes: if panels.len() > 1 { Some(vec!["own axis per panel".to_string()]) } else { None },
	}
}

/// Series indices grouped into panels whose peaks stay within {@link SPLIT_SPREAD}× of the panel's
/// largest, panels and their series in table order. An all-zero series joins the smallest panel.
fn magnitude_panels(series: &[ChartSeries]) -> Vec<Vec<usize>> {
	let peaks: Vec<f64> = series
		.iter()
		.map(|entry| {
			entry
				.points
				.iter()
				.map(|point| point.as_ref().map_or(0.0, |point| point.value.abs()))
				.fold(0.0f64, f64::max)
		})
		.collect();
	let mut order: Vec<usize> = (0..series.len()).collect();
	order.sort_by(|a, b| peaks[*b].partial_cmp(&peaks[*a]).unwrap_or(std::cmp::Ordering::Equal));
	let mut panels: Vec<Vec<usize>> = Vec::new();
	let mut panel_peak = 0.0f64;
	for index in order {
		let peak = peaks[index];
		match panels.last_mut() {
			Some(last) if peak == 0.0 || panel_peak < peak * SPLIT_SPREAD => last.push(index),
			_ => {
				panels.push(vec![index]);
				panel_peak = peak;
			}
		}
	}
	for panel in &mut panels {
		panel.sort_unstable();
	}
	panels.sort_unstable_by_key(|panel| panel[0]);
	panels
}

/// A path through `points` (x ascending) as a monotone cubic (Steffen's method): smooth, yet never
/// overshooting a sample the way a Catmull-Rom curve would between a peak and a flat stretch.
fn smooth_path(points: &[(f64, f64)]) -> String {
	let n = points.len();
	let mut path = format!("M{} {}", num(points[0].0), num(points[0].1));
	if n < 3 {
		return if n == 2 { format!("{path}L{} {}", num(points[1].0), num(points[1].1)) } else { path };
	}
	let mut secants: Vec<f64> = Vec::with_capacity(n - 1);
	for at in 0..n - 1 {
		let h = points[at + 1].0 - points[at].0;
		secants.push(if h == 0.0 { 0.0 } else { (points[at + 1].1 - points[at].1) / h });
	}
	let mut tangents: Vec<f64> = vec![0.0];
	for at in 1..n - 1 {
		let h0 = points[at].0 - points[at - 1].0;
		let h1 = points[at + 1].0 - points[at].0;
		let (s0, s1) = (secants[at - 1], secants[at]);
		let weighted = (s0 * h1 + s1 * h0) / or_one(h0 + h1);
		tangents.push((s0.signum() + s1.signum()) * s0.abs().min(s1.abs()).min(0.5 * weighted.abs()));
	}
	tangents[0] = (3.0 * secants[0] - tangents[1]) / 2.0;
	tangents.push((3.0 * secants[n - 2] - tangents[n - 2]) / 2.0);
	for at in 0..n - 1 {
		let (x0, y0) = points[at];
		let (x1, y1) = points[at + 1];
		let third = (x1 - x0) / 3.0;
		path += &format!(
			"C{} {} {} {} {} {}",
			num(x0 + third),
			num(y0 + third * tangents[at]),
			num(x1 - third),
			num(y1 - third * tangents[at + 1]),
			num(x1),
			num(y1)
		);
	}
	path
}

/// End labels in their lines' colors, nudged apart so none overlap, kept between `top` and `bottom` where they fit.
fn spread_labels(labels: &mut [EndLabel], top: f64, bottom: f64) -> String {
	spread_apart(labels, top, bottom, TICK_SIZE + 2.0);
	labels
		.iter()
		.map(|label| {
			let mut style = TextStyle::new(&label.color);
			style.size = Some(TICK_SIZE);
			style.weight = Some(600);
			match &label.name {
				None => text(label.x, label.y, &label.text, &style),
				Some(name) => {
					let mark = text(label.x, label.y, name, &style);
					let offset = measure(name, TICK_SIZE) * SEMIBOLD_WIDTH + 6.0;
					let mut muted = TextStyle::new("muted");
					muted.size = Some(TICK_SIZE);
					mark + &text(label.x + offset, label.y, &label.text, &muted)
				}
			}
		})
		.collect()
}

/// One series against a measure, one dot per row.
fn draw_scatter(spec: &ChartSpec) -> Layer {
	let entry = &spec.series[0];
	let xs = spec.x.clone().unwrap_or(ChartAxis { name: String::new(), dim: Dimension::Count, unit: String::new(), values: Vec::new() });
	let ys: Vec<f64> = entry.points.iter().map(|point| point.as_ref().map_or(f64::NAN, |point| point.value)).collect();
	let finite = |values: &[f64]| -> Vec<f64> { values.iter().copied().filter(|value| value.is_finite()).collect() };
	let ys_finite = finite(&ys);
	let y_axis = value_scale(&ys_finite, 0.0, 1.0, false, false, 1.0);
	let y_format = tick_format(&y_axis, entry);
	let mut tick_width = 0.0f64;
	for tick in &y_axis.ticks {
		tick_width = tick_width.max(measure(&y_format(*tick), TICK_SIZE));
	}
	let left = PAD + tick_width + 8.0;
	let right = WIDTH - PAD - 24.0;
	let top = 18.0;
	let bottom = top + 200.0;
	// Ends inset from the plot's edges, so a dot on an outer tick stays clear of the axis labels.
	let x_scale = value_scale(&finite(&xs.values), left + 8.0, right - 8.0, false, false, 1.0);
	let y_scale = value_scale(&ys_finite, bottom - 6.0, top + 6.0, false, false, 1.0);
	// Axis titles in the text color: they name what the plot means, unlike the muted tick figures.
	let mut parts: Vec<String> = Vec::new();
	let mut title = TextStyle::new("fg");
	title.size = Some(TICK_SIZE);
	parts.push(text(left, 6.0, &entry.name, &title));
	for tick in &y_scale.ticks {
		parts.push(rule(left, y_scale.at(*tick), right, y_scale.at(*tick)));
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::End);
		parts.push(text(left - 8.0, y_scale.at(*tick), &y_format(*tick), &style));
	}
	// A positional axis of bare numbers reads in their own dimension, never in the series'.
	let x_axis = ChartSeries {
		name: xs.name.clone(),
		dim: xs.dim,
		unit: xs.unit.clone(),
		points: Vec::new(),
		notes: Vec::new(),
		polarity: None,
		best: None,
	};
	let x_format = tick_format(&x_scale, &x_axis);
	for tick in &x_scale.ticks {
		let mut style = TextStyle::new("muted");
		style.size = Some(TICK_SIZE);
		style.anchor = Some(Anchor::Middle);
		parts.push(text(x_scale.at(*tick), bottom + 14.0, &x_format(*tick), &style));
	}
	let mut title = TextStyle::new("fg");
	title.size = Some(TICK_SIZE);
	title.anchor = Some(Anchor::End);
	parts.push(text(right, bottom + 30.0, &xs.name, &title));
	let mut marks: Vec<(usize, &ChartPoint, f64, f64)> = Vec::new();
	for (at, point) in entry.points.iter().enumerate() {
		let Some(point) = point.as_ref() else { continue };
		let Some(xv) = xs.values.get(at).copied() else { continue };
		if !xv.is_finite() {
			continue;
		}
		marks.push((at, point, x_scale.at(xv), y_scale.at(point.value)));
	}
	let emphasized = marks.iter().any(|(_, point, _, _)| point.emphasis);
	// A highlighted item (the row the table is about) is ringed in the accent and named in full strength.
	let focus: HashSet<usize> = highlighted(spec).into_iter().collect();
	for (at, point, x, y) in &marks {
		let strong = !emphasized || point.emphasis || focus.contains(at);
		if focus.contains(at) {
			parts.push(ring(*x, *y, 8.5, "accent"));
		}
		parts.push(dot(*x, *y, if point.emphasis { 5.5 } else { 4.0 }, SERIES[0], if strong { 0.9 } else { RECEDE }));
	}
	// Each name takes the first spot around its dot clear of every dot and name placed so far; crowded ones go unnamed.
	let mut taken: Vec<SpotBox> =
		marks.iter().map(|(_, _, x, y)| SpotBox { x0: x - 5.0, y0: y - 5.0, x1: x + 5.0, y1: y + 5.0 }).collect();
	let lead = |(at, point, _, _): &(usize, &ChartPoint, f64, f64)| u8::from(focus.contains(at)) as i32 * 2 + i32::from(point.emphasis);
	let mut order: Vec<&(usize, &ChartPoint, f64, f64)> = marks.iter().collect();
	order.sort_by_key(|mark| -lead(*mark));
	let names = clip_names(&spec.categories, 26);
	if order.len() <= 40 {
		for (at, point, x, y) in order {
			let label = &names[*at];
			let strong = point.emphasis || focus.contains(at);
			let width = measure(label, TICK_SIZE) * if strong { SEMIBOLD_WIDTH } else { 1.0 };
			// A name clears its dot, and the accent ring around a highlighted one.
			let gap = if focus.contains(at) { 13.0 } else { 8.0 };
			let spots = [
				(*x + gap, *y, Anchor::Start),
				(*x - gap, *y, Anchor::End),
				(*x, *y - gap - 5.0, Anchor::Middle),
				(*x, *y + gap + 5.0, Anchor::Middle),
			];
			for (spot_x, spot_y, anchor) in spots {
				let x0 = match anchor {
					Anchor::Start => spot_x,
					Anchor::End => spot_x - width,
					Anchor::Middle => spot_x - width / 2.0,
				};
				let spot = SpotBox { x0: x0 - 1.0, y0: spot_y - 6.0, x1: x0 + width + 1.0, y1: spot_y + 6.0 };
				if spot.x0 < PAD || spot.x1 > WIDTH - PAD || spot.y0 < 0.0 || spot.y1 > bottom + 4.0 {
					continue;
				}
				if taken.iter().any(|other| spot.x0 < other.x1 && other.x0 < spot.x1 && spot.y0 < other.y1 && other.y0 < spot.y1) {
					continue;
				}
				taken.push(spot);
				// Names read in the text color unless bolded points single some out; then the rest recede with their dots.
				let mut style = TextStyle::new(if focus.contains(at) {
					"accent"
				} else if strong || !emphasized {
					"fg"
				} else {
					"muted"
				});
				style.size = Some(TICK_SIZE);
				style.anchor = Some(anchor);
				style.weight = if strong { Some(600) } else { None };
				parts.push(text(spot_x, spot_y, label, &style));
				break;
			}
		}
	}
	Layer { height: bottom + 36.0, body: parts.join(""), legend: None, ramp: None, notes: None }
}
