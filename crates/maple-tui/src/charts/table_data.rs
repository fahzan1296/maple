//! Numeric reading of GFM table cells and columns, the input to chart
//! planning: what number a cell holds (in base units, with its dimension and
//! annotations) and which role each column plays — a measure to plot, a
//! category label, an ordered x axis, or a row index to ignore.
//!
//! Reference: `l/src/charts/table-data.ts`.
//!
//! Deviations forced by Rust:
//! - TypeScript union types (`Dimension`, `CellFit`, `ColumnRole`, `Status`,
//!   `Cell`) become enums; `Cell` is `Number(NumberCell)` / `Other(OtherCell)`.
//! - `withMeasures` has no default argument, so the label defaults to
//!   `table.label` in [`with_measures`] and [`with_measures_labeled`] takes it
//!   explicitly.
//! - The reference's lookaround/backreference regexes (JavaScript-only) are
//!   hand-rolled scanners with identical accept/reject behaviour; `regex` is
//!   used for the rest. See `plain_cell`, `match_integer`, `match_cell`.

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// What a number measures; values of one dimension share an axis after unit normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dimension {
	/// A plain count.
	Count,
	/// Percent points.
	Percent,
	/// Seconds.
	Duration,
	/// Bytes.
	Bytes,
	/// Money, keeping its currency symbol.
	Currency,
	/// A bare multiplier (`4.1×`).
	Ratio,
	/// A written `a/b` fraction.
	Fraction,
	/// A unit per something (`tok/s`).
	Rate,
}

/// How cleanly a cell reads as its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellFit {
	/// Bare (`12 ms`).
	Pure,
	/// With a short note (`17 (21.5%)`, `16 pass`).
	Note,
	/// A range (`24–72 h`).
	Range,
	/// A transition (`12 → 15`).
	Arrow,
	/// A fraction (`5/8`).
	Fraction,
	/// A number leading prose (`~1,400w, nearly ready`).
	Prose,
}

/// What a column contributes to a chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnRole {
	/// Values to plot.
	Measure,
	/// Category names.
	Label,
	/// Dates or times.
	Temporal,
	/// An ordered step (`Run`, `Day`, `Workers`).
	Sequence,
	/// A row number or identifier never plotted.
	Index,
}

/// What a status cell says about its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
	/// Passed or landed.
	Good,
	/// Failed or broke.
	Bad,
	/// Partly or flaky.
	Warn,
	/// Not settled yet.
	Pending,
}

impl Dimension {
	/// The reference's lowercase name for this dimension.
	pub fn as_str(&self) -> &'static str {
		match self {
			Dimension::Count => "count",
			Dimension::Percent => "percent",
			Dimension::Duration => "duration",
			Dimension::Bytes => "bytes",
			Dimension::Currency => "currency",
			Dimension::Ratio => "ratio",
			Dimension::Fraction => "fraction",
			Dimension::Rate => "rate",
		}
	}
}

/// The kind of a non-numeric cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtherKind {
	/// Stands for "no value".
	Missing,
	/// A date.
	Date,
	/// A time of day.
	Time,
	/// A version string.
	Version,
	/// A row identifier or code.
	Id,
	/// Anything else.
	Text,
}

/// A cell that reads as a number.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberCell {
	/// Value in base units: seconds, bytes, a plain count, percent points.
	pub value: f64,
	/// Dominant dimension of the value.
	pub dim: Dimension,
	/// Unit or currency symbol as written (`ms`, `KiB`, `$`, `tok/s`); empty for bare numbers.
	pub unit: String,
	/// How cleanly the cell reads.
	pub fit: CellFit,
	/// Written as an estimate or bound (`~`, `≈`, `<`, `≥`).
	pub approx: bool,
	/// Written with an explicit sign (`+12%`, `-3`).
	pub signed: bool,
	/// The cell was bolded.
	pub emphasis: bool,
	/// The cell text without Markdown decoration.
	pub text: String,
	/// The number as written, with approximation, sign, currency and unit.
	pub figure: String,
	/// `a → b`: `b`, in the same base units.
	pub to: Option<f64>,
	/// `a → b`: `b` as written.
	pub to_figure: Option<String>,
	/// `a–b`: `b`, in the same base units.
	pub upper: Option<f64>,
	/// `a/b`: `b`.
	pub denominator: Option<f64>,
	/// `a ± e`: `e`, in the same base units.
	pub error: Option<f64>,
	/// A slash list of measurements, the first included, in base units.
	pub values: Option<Vec<f64>>,
}

/// A cell that is not a quantity.
#[derive(Clone, Debug, PartialEq)]
pub struct OtherCell {
	/// Which kind of non-quantity.
	pub kind: OtherKind,
	/// The cell text without Markdown decoration.
	pub text: String,
	/// The cell was bolded.
	pub emphasis: bool,
}

/// A parsed table cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
	/// A cell that reads as a number.
	Number(NumberCell),
	/// A cell that is not a quantity.
	Other(OtherCell),
}

impl Cell {
	/// The discriminant of the cell.
	pub fn kind(&self) -> CellKind {
		match self {
			Cell::Number(_) => CellKind::Number,
			Cell::Other(other) => match other.kind {
				OtherKind::Missing => CellKind::Missing,
				OtherKind::Date => CellKind::Date,
				OtherKind::Time => CellKind::Time,
				OtherKind::Version => CellKind::Version,
				OtherKind::Id => CellKind::Id,
				OtherKind::Text => CellKind::Text,
			},
		}
	}

	/// The cell text without Markdown decoration.
	pub fn text(&self) -> &str {
		match self {
			Cell::Number(cell) => &cell.text,
			Cell::Other(cell) => &cell.text,
		}
	}

	/// Whether the cell was bolded.
	pub fn emphasis(&self) -> bool {
		match self {
			Cell::Number(cell) => cell.emphasis,
			Cell::Other(cell) => cell.emphasis,
		}
	}

	/// The number a cell holds, when it reads as one.
	pub fn as_number(&self) -> Option<&NumberCell> {
		match self {
			Cell::Number(cell) => Some(cell),
			Cell::Other(_) => None,
		}
	}
}

/// Discriminant shared by every [`Cell`] variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
	/// A number.
	Number,
	/// No value.
	Missing,
	/// A date.
	Date,
	/// A time of day.
	Time,
	/// A version string.
	Version,
	/// An identifier or code.
	Id,
	/// Plain text.
	Text,
}

/// A table column read for charting.
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
	/// Column position in the source table.
	pub index: usize,
	/// Header text without Markdown decoration.
	pub header: String,
	/// What the column contributes.
	pub role: ColumnRole,
	/// Parsed cells, one per row.
	pub cells: Vec<Cell>,
	/// Dominant dimension of the numeric cells.
	pub dim: Option<Dimension>,
	/// Representative unit of the dominant dimension; empty when bare.
	pub unit: String,
	/// Numeric cells disagree on dimension.
	pub mixed: bool,
	/// Numeric cells are mostly scores out of a total, read as percent of it.
	pub scores: bool,
	/// Per row, the status its cell leads with, when most cells lead with one.
	pub statuses: Option<Vec<Option<Status>>>,
	/// Every numeric data cell holds the same value: an echoed setting.
	pub constant: bool,
	/// Share of numeric cells written with an explicit sign.
	pub signed_share: f64,
	/// Share of numeric cells written as `a → b`.
	pub arrow_share: f64,
	/// Index of the measure column this one restates, when the values show it.
	pub restates: Option<usize>,
}

/// A column before its data rows are known (no `constant` flag yet).
#[derive(Clone, Debug, PartialEq)]
pub struct DraftColumn {
	/// Column position in the source table.
	pub index: usize,
	/// Header text without Markdown decoration.
	pub header: String,
	/// What the column contributes.
	pub role: ColumnRole,
	/// Parsed cells, one per row.
	pub cells: Vec<Cell>,
	/// Dominant dimension of the numeric cells.
	pub dim: Option<Dimension>,
	/// Representative unit of the dominant dimension; empty when bare.
	pub unit: String,
	/// Numeric cells disagree on dimension.
	pub mixed: bool,
	/// Numeric cells are mostly scores out of a total, read as percent of it.
	pub scores: bool,
	/// Per row, the status its cell leads with, when most cells lead with one.
	pub statuses: Option<Vec<Option<Status>>>,
	/// Share of numeric cells written with an explicit sign.
	pub signed_share: f64,
	/// Share of numeric cells written as `a → b`.
	pub arrow_share: f64,
}

/// Measure columns that add up, read from the values.
#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
	/// Column the parts make up; absent when only a judge says the parts form a whole.
	pub whole: Option<usize>,
	/// Leaf part columns, in table order.
	pub parts: Vec<usize>,
	/// The parts sum to the whole within the written precision in every row.
	pub exact: bool,
}

/// A table read for charting.
#[derive(Clone, Debug, PartialEq)]
pub struct TableAnalysis {
	/// Every column, in table order.
	pub columns: Vec<TableColumn>,
	/// Category labels.
	pub label: Option<TableColumn>,
	/// Plottable measure columns, constant ones excluded.
	pub measures: Vec<TableColumn>,
	/// Data rows, in table order, without total/summary rows.
	pub rows: Vec<usize>,
	/// Total/summary rows, kept out of the scale.
	pub totals: Vec<usize>,
	/// Measure columns adding up to a whole, when the values show it.
	pub composition: Option<Composition>,
}

/// Inline Markdown stripped from a cell.
#[derive(Clone, Debug, PartialEq)]
pub struct PlainCell {
	/// The remaining text.
	pub text: String,
	/// The cell carried bold or italic emphasis.
	pub emphasis: bool,
}

/** Cell texts that stand for "no value". */
const MISSING: [&str; 17] = [
	"", "-", "—", "–", "−", "n/a", "na", "none", "?", "…", "...", "null", "nan", "tbd", "·", "x", "✗",
];

/// Leading status words, matched whole and case-insensitively.
static STATUS_WORDS: LazyLock<HashMap<&'static str, Status>> = LazyLock::new(|| {
	HashMap::from([
		("ok", Status::Good),
		("pass", Status::Good),
		("passed", Status::Good),
		("passes", Status::Good),
		("passing", Status::Good),
		("success", Status::Good),
		("succeeded", Status::Good),
		("done", Status::Good),
		("complete", Status::Good),
		("completed", Status::Good),
		("landed", Status::Good),
		("merged", Status::Good),
		("fixed", Status::Good),
		("works", Status::Good),
		("working", Status::Good),
		("green", Status::Good),
		("fail", Status::Bad),
		("failed", Status::Bad),
		("fails", Status::Bad),
		("failing", Status::Bad),
		("failure", Status::Bad),
		("error", Status::Bad),
		("errored", Status::Bad),
		("broken", Status::Bad),
		("timeout", Status::Bad),
		("timed out", Status::Bad),
		("crash", Status::Bad),
		("crashed", Status::Bad),
		("oom", Status::Bad),
		("panic", Status::Bad),
		("red", Status::Bad),
		("blocked", Status::Bad),
		("truncated", Status::Bad),
		("partial", Status::Warn),
		("flaky", Status::Warn),
		("degraded", Status::Warn),
		("pending", Status::Pending),
		("running", Status::Pending),
		("waiting", Status::Pending),
		("queued", Status::Pending),
		("skipped", Status::Pending),
		("wip", Status::Pending),
		("in progress", Status::Pending),
	])
});

/// Leading status marks.
const STATUS_MARKS: [(char, Status); 12] = [
	('✅', Status::Good),
	('✓', Status::Good),
	('✔', Status::Good),
	('🟢', Status::Good),
	('❌', Status::Bad),
	('✗', Status::Bad),
	('✘', Status::Bad),
	('🔴', Status::Bad),
	('⚠', Status::Warn),
	('🟡', Status::Warn),
	('⏳', Status::Pending),
	('🕐', Status::Pending),
];

/// The `STATUS_WORDS` keys, longest first, joined as one anchored alternation.
static STATUS_WORD: LazyLock<Regex> = LazyLock::new(|| {
	let mut words: Vec<&'static str> = STATUS_WORDS.keys().copied().collect();
	words.sort_by(|a, b| b.len().cmp(&a.len()));
	let pattern = format!("^(?:{})", words.join("|"));
	Regex::new(&pattern).expect("valid status word regex")
});

/// Status and trend marks that decorate a value without changing it.
static MARKS: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new("[✅❌⚠🟢🔴🟡✓✗✔★⭐↑↓⬆⬇▲▼\u{FE0E}\u{FE0F}]").expect("valid marks regex")
});

/// A run of whitespace, collapsed by [`plain_cell`].
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

/// Footnote marks after a value (`165*`, `12 ms†`): the value stands as written.
static FOOTNOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[*†‡]+$").expect("valid footnote regex"));

/// A tilde before words hedges the verdict, read as `warn`.
static HEDGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^~\s*\p{L}").expect("valid hedge regex"));

/// HTML tags a Markdown cell may carry.
static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)<\/?(?:a|abbr|b|br|code|del|details|div|em|i|img|ins|kbd|mark|p|s|small|span|strong|sub|summary|sup|u)\b[^>]*>")
		.expect("valid html tag regex")
});

/// A Markdown code span, masked before other markup is stripped.
static CODE_SPAN: LazyLock<Regex> = LazyLock::new(|| Regex::new("`([^`]*)`").expect("valid code span regex"));

/// A backslash escape (`\$12.50`, `\*`), masked before other markup is stripped.
static ESCAPE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\\([!-/:-@\[-`{-~])").expect("valid escape regex"));

/// An inline link, replaced by its text.
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!?\[([^\]]*)\]\([^)]*\)").expect("valid link regex"));

/// A strikethrough span.
static STRIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~~(.+?)~~").expect("valid strike regex"));

/// A `<br>` tag.
static BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("valid br regex"));

/// A masked span placeholder.
static PLACEHOLDER: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\u{0000}([0-9]+)\u{0000}").expect("valid placeholder regex"));

/// A date written as `2024-01-02`, `01/02/2024`, or `Jan 2`.
static DATE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^[0-9]{4}-[0-9]{2}(?:-[0-9]{2})?(?:[ T][0-9]{1,2}:[0-9]{2}(?::[0-9]{2})?)?\b|^[0-9]{1,2}/[0-9]{1,2}/[0-9]{2,4}$|^(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?\s+[0-9]{1,2}\b")
		.expect("valid date regex")
});

/// A time of day.
static TIME: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^[0-9]{1,2}:[0-9]{2}(?::[0-9]{2})?(?:\.[0-9]+)?\s*(?:am|pm)?$").expect("valid time regex")
});

/// A version string.
static VERSION: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?i)^v?[0-9]+\.[0-9]+\.[0-9]+|^v[0-9]").expect("valid version regex"));

/// A whole number written with a leading zero: a code, never a quantity.
static ZERO_PADDED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0[0-9]").expect("valid zero padded regex"));

/// A cell that is one Markdown link, bold or not, and nothing else.
static LINKED: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^\s*(?:\*\*|__)?\[[^\]]*\]\([^)]*\)(?:\*\*|__)?\s*$").expect("valid linked regex")
});

/// A compound duration: `1d 2h 3m 4s`.
static COMPOUND_DURATION: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^(?:([0-9]+(?:\.[0-9]+)?)\s*d)?\s*(?:([0-9]+(?:\.[0-9]+)?)\s*h)?\s*(?:([0-9]+(?:\.[0-9]+)?)\s*m(?:in)?)?\s*(?:([0-9]+(?:\.[0-9]+)?)\s*s)?$")
		.expect("valid compound duration regex")
});

/// Text after a list or a sum that only annotates it.
static SHORT_NOTE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^$|^\(.*\)$|^[A-Za-z][A-Za-z0-9_-]{0,11}$").expect("valid short note regex"));

/// Row labels that summarize the rows above them.
static TOTAL_ROW: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^(?:(?:total|totals|sum|overall|grand total|subtotal|σ)\b|(?:average|avg|mean|median|all|combined|net)\s*(?:\(.*\))?:?$)")
		.expect("valid total row regex")
});

/// Summary labels that say nothing else.
static BARE_TOTAL: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^(?:total|totals|sum|grand total|sub-?total|σ|average|avg|mean|median)\s*(?:\(.*\))?:?$")
		.expect("valid bare total regex")
});

/// Header names of ordered steps.
static SEQUENCE_HEADER: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^(?:step|phase|stage|round|tier|level|wave|pass|attempt|iter|iteration|epoch|run|batch|day|week|month|year|quarter|n|workers?|threads?|concurrency|jobs|depth|k)$")
		.expect("valid sequence header regex")
});

/// A header with its numbers blanked.
static SWEEP_STEP: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"[0-9]+(?:\.[0-9]+)?").expect("valid sweep step regex"));

/// Headers of a size on disk or in memory.
static SIZE_HEADER: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)\b(?:sizes?|disk|du|rss|memory|mem|heap|storage|footprint|bytes)\b").expect("valid size header regex")
});

/// A whole that parts may fall within.
static WHOLE_HEADER: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?i)\b(?:total|all|sum|overall)\b").expect("valid whole header regex"));

/// The word a bare count names after its figure.
static COUNTED_NOUN: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^\s*([A-Za-z][A-Za-z-]*)").expect("valid counted noun regex"));

/// An elapsed time written on a clock face.
static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^(?:([0-9]{1,2}):)?([0-9]{1,3}):([0-9]{2}(?:\.[0-9]+)?)$").expect("valid clock regex")
});

/// A span of whole numbers written `a–b`, or one number.
static SPAN: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^([0-9]+)(?:\s*[-–—]\s*([0-9]+))?$").expect("valid span regex"));

/// A spaced slash between list items.
static LIST_SLASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+\/\s+").expect("valid list slash regex"));

/// A spaced slash naming two values rather than a path or a score.
static PAIR_SLASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\S \/ \S").expect("valid pair slash regex"));

/// A fraction whose denominator is a whole number after a slash.
static WHOLE_DENOMINATOR: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"/\s*[0-9][0-9,]*$").expect("valid whole denominator regex"));

/// Header names of row numbers and identifiers: never plotted, never a
/// category axis when text exists.
static INDEX_HEADER: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^(?:#|no\.?|n°|id|ids|rank|row|idx|index|line|lines?\s*#|ln|pr|pr\s*#|issue|commit|sha|hash|port|pid|code|exit\s*code|status|version|ver|ref|offset|priority|prio)$|\s(?:id|ids|version|ver|sha|hash|pid|port)$/i")
		.expect("valid index header regex")
});

/// Unit → dimension and factor to base units, matched case-insensitively.
static UNITS: LazyLock<HashMap<&'static str, (Dimension, f64)>> = LazyLock::new(|| {
	HashMap::from([
		("%", (Dimension::Percent, 1.0)),
		("pp", (Dimension::Percent, 1.0)),
		("pct", (Dimension::Percent, 1.0)),
		("ns", (Dimension::Duration, 1e-9)),
		("µs", (Dimension::Duration, 1e-6)),
		("μs", (Dimension::Duration, 1e-6)),
		("us", (Dimension::Duration, 1e-6)),
		("ms", (Dimension::Duration, 1e-3)),
		("s", (Dimension::Duration, 1.0)),
		("sec", (Dimension::Duration, 1.0)),
		("secs", (Dimension::Duration, 1.0)),
		("seconds", (Dimension::Duration, 1.0)),
		("min", (Dimension::Duration, 60.0)),
		("mins", (Dimension::Duration, 60.0)),
		("minutes", (Dimension::Duration, 60.0)),
		("h", (Dimension::Duration, 3600.0)),
		("hr", (Dimension::Duration, 3600.0)),
		("hrs", (Dimension::Duration, 3600.0)),
		("hours", (Dimension::Duration, 3600.0)),
		("d", (Dimension::Duration, 86_400.0)),
		("days", (Dimension::Duration, 86_400.0)),
		("bytes", (Dimension::Bytes, 1.0)),
		("kb", (Dimension::Bytes, 1e3)),
		("kib", (Dimension::Bytes, 1024.0)),
		("mb", (Dimension::Bytes, 1e6)),
		("mib", (Dimension::Bytes, 1024.0 * 1024.0)),
		("gb", (Dimension::Bytes, 1e9)),
		("gib", (Dimension::Bytes, 1024.0 * 1024.0 * 1024.0)),
		("tb", (Dimension::Bytes, 1e12)),
		("tib", (Dimension::Bytes, 1024.0 * 1024.0 * 1024.0 * 1024.0)),
		("x", (Dimension::Ratio, 1.0)),
		("×", (Dimension::Ratio, 1.0)),
		("k", (Dimension::Count, 1e3)),
		("bn", (Dimension::Count, 1e9)),
	])
});

/// Units whose case decides their meaning.
static CASED_UNITS: LazyLock<HashMap<&'static str, (Dimension, f64)>> = LazyLock::new(|| {
	HashMap::from([
		("B", (Dimension::Bytes, 1.0)),
		("b", (Dimension::Count, 1e9)),
		("M", (Dimension::Count, 1e6)),
		("m", (Dimension::Duration, 60.0)),
		("G", (Dimension::Count, 1e9)),
		("T", (Dimension::Count, 1e12)),
	])
});

/// Bare magnitude suffixes of a `du -h` size, as powers of 1024.
static BINARY_POWER: LazyLock<HashMap<&'static str, u32>> =
	LazyLock::new(|| HashMap::from([("K", 1), ("k", 1), ("M", 2), ("G", 3), ("T", 4)]));

/// Most values one slash list holds; longer lists are prose.
const MAX_LISTED_VALUES: usize = 4;

/// Spread within which a bare slash list repeats one measurement.
const ALIKE_SPREAD: f64 = 3.0;

/// Relative slack within which a total row's figure matches the sum above it.
const APPROX_TOTAL: f64 = 0.005;

/// Most columns searched for parts summing to a whole.
const MAX_COMPOSED: usize = 8;

/// Most parts one whole splits into directly.
const MAX_PARTS: usize = 4;

/// Share of its total that parts falling within it must make up in one row.
const MIN_PART_SHARE: f64 = 0.25;

/// Most relative rounding a row's quotient may carry.
const LOOSE_QUOTIENT: f64 = 0.05;

/// Most relative rounding a row may carry to prove a proportion.
const TIGHT: f64 = 0.015;

/// Fewest items a slash list must hold to split into rows.
const MIN_LISTED: usize = 3;

/// Fewest items a slash list splits into when its name cell leads.
const MIN_LISTED_PAIR: usize = 2;

/// Fewest numeric columns listing values in a row it splits.
const MIN_LISTS: usize = 2;

/// Fewest spans that tile a numbering.
const MIN_TILES: usize = 3;

/// JavaScript `\s`: Unicode White_Space plus BOM.
fn is_js_ws(c: char) -> bool {
	c.is_whitespace() || c == '\u{FEFF}'
}

/// JavaScript `\w`: ASCII word character.
fn is_word_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || c == '_'
}

/// Length of `text` in UTF-16 code units, as JavaScript's `String.length` counts.
fn utf16_len(text: &str) -> usize {
	text.encode_utf16().count()
}

/// Skip JavaScript whitespace.
fn skip_ws(text: &str, mut pos: usize) -> usize {
	while let Some(c) = text[pos..].chars().next() {
		if !is_js_ws(c) {
			break;
		}
		pos += c.len_utf8();
	}
	pos
}

/// Byte length of the ASCII digit run starting at `at`.
fn digit_run(text: &str, at: usize) -> usize {
	let mut end = at;
	while let Some(c) = text[end..].chars().next() {
		if !c.is_ascii_digit() {
			break;
		}
		end += 1;
	}
	end - at
}

/// The value of a written number, its comma or space group separators removed.
fn to_number(written: &str) -> f64 {
	let cleaned: String = written.chars().filter(|c| *c != ',' && !is_js_ws(*c)).collect();
	if cleaned.is_empty() {
		return 0.0;
	}
	cleaned.parse::<f64>().unwrap_or(f64::NAN)
}

/// The whole number starting at `at`, as JavaScript's `INTEGER` matches it.
///
/// Reproduces `\d{1,3}(?:,\d{3})+|\d{1,3}(?:[ ]\d{3})+(?!\d)|\d+`, whose
/// negative lookahead Rust's `regex` cannot express.
fn match_integer(text: &str, at: usize) -> Option<usize> {
	let run = digit_run(text, at);
	if run == 0 {
		return None;
	}
	for first in (1..=3).rev() {
		if first > run {
			continue;
		}
		let mut end = at + first;
		let mut groups = 0;
		while text[end..].starts_with(',') && digit_run(text, end + 1) >= 3 {
			end += 4;
			groups += 1;
		}
		if groups > 0 {
			return Some(end);
		}
	}
	for first in (1..=3).rev() {
		if first > run {
			continue;
		}
		let mut probe = at + first;
		let mut ends: Vec<usize> = Vec::new();
		while let Some(c) = text[probe..].chars().next() {
			if !matches!(c, ' ' | '\u{00A0}' | '\u{202F}') || digit_run(text, probe + c.len_utf8()) < 3 {
				break;
			}
			probe += c.len_utf8() + 3;
			ends.push(probe);
		}
		for end in ends.into_iter().rev() {
			if !text[end..].starts_with(|c: char| c.is_ascii_digit()) {
				return Some(end);
			}
		}
	}
	Some(at + run)
}

/// The number starting at `at`, as JavaScript's `NUMBER` matches it.
fn match_number(text: &str, at: usize) -> Option<usize> {
	if let Some(mut end) = match_integer(text, at) {
		if text[end..].starts_with('.') && digit_run(text, end + 1) > 0 {
			end += 1 + digit_run(text, end + 1);
		}
		if matches!(text[end..].chars().next(), Some('e') | Some('E')) {
			let mut exponent = end + 1;
			if matches!(text[exponent..].chars().next(), Some('+') | Some('-')) {
				exponent += 1;
			}
			let digits = digit_run(text, exponent);
			if digits > 0 {
				end = exponent + digits;
			}
		}
		return Some(end);
	}
	if text[at..].starts_with('.') && digit_run(text, at + 1) > 0 {
		return Some(at + 1 + digit_run(text, at + 1));
	}
	None
}

/// Whether `text` is exactly one whole number.
fn is_whole(text: &str) -> bool {
	match_integer(text, 0) == Some(text.len())
}

/// Dimension and base factor of `unit`, cased spellings first.
fn known_unit(unit: &str) -> Option<(Dimension, f64)> {
	if let Some(found) = CASED_UNITS.get(unit) {
		return Some(*found);
	}
	UNITS.get(unit.to_lowercase().as_str()).copied()
}

/// Strip inline Markdown from a cell: links keep their text; bold, italics,
/// strikethrough, code spans and `<br>` keep their content.
pub fn plain_cell(markdown: &str) -> PlainCell {
	let replaced = markdown.replace("\\|", "|");
	let emphasis = has_emphasis(&replaced);
	let mut spans: Vec<String> = Vec::new();
	let text = CODE_SPAN
		.replace_all(&replaced, |caps: &regex::Captures| {
			let code = caps.get(1).map_or("", |m| m.as_str()).to_string();
			let at = spans.len();
			spans.push(code);
			format!("\u{0}{at}\u{0}")
		})
		.into_owned();
	let text = ESCAPE
		.replace_all(&text, |caps: &regex::Captures| {
			let escaped = caps.get(1).map_or("", |m| m.as_str()).to_string();
			let at = spans.len();
			spans.push(escaped);
			format!("\u{0}{at}\u{0}")
		})
		.into_owned();
	let text = LINK.replace_all(&text, "${1}").into_owned();
	let text = strip_bold(&text);
	let text = strip_italic(&text, '*');
	let text = strip_italic(&text, '_');
	let text = STRIKE.replace_all(&text, "${1}").into_owned();
	let text = BR.replace_all(&text, " ").into_owned();
	let text = HTML_TAG.replace_all(&text, "").into_owned();
	let text = PLACEHOLDER
		.replace_all(&text, |caps: &regex::Captures| {
			caps.get(1)
				.and_then(|m| m.as_str().parse::<usize>().ok())
				.and_then(|at| spans.get(at).cloned())
				.unwrap_or_default()
		})
		.into_owned();
	let text = WHITESPACE.replace_all(&text, " ").trim().to_string();
	PlainCell { text, emphasis }
}

/// Whether `text` carries a closed `**…**` or `__…__` pair.
fn has_emphasis(text: &str) -> bool {
	for delim in ["**", "__"] {
		let mut from = 0;
		while let Some(found) = text[from..].find(delim) {
			let start = from + found;
			let after = start + 2;
			from = after;
			let Some(next) = text[after..].chars().next() else {
				continue;
			};
			if is_js_ws(next) {
				continue;
			}
			let line_end = text[after..].find('\n').map(|at| after + at).unwrap_or(text.len());
			let mut search = after;
			while let Some(found) = text[search..line_end].find(delim) {
				let close = search + found;
				if close > after {
					let previous = text[..close].chars().next_back().expect("close follows content");
					if !is_js_ws(previous) {
						return true;
					}
				}
				search = close + 2;
			}
		}
	}
	false
}

/// Replace `**bold**` and `__bold__` with their content.
fn strip_bold(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut at = 0;
	while at < text.len() {
		let delim = if text[at..].starts_with("**") {
			Some("**")
		} else if text[at..].starts_with("__") {
			Some("__")
		} else {
			None
		};
		if let Some(delim) = delim {
			if let Some(close) = find_delimiter(text, at + 2, delim) {
				out.push_str(&text[at + 2..close]);
				at = close + delim.len();
				continue;
			}
		}
		let c = text[at..].chars().next().expect("at is a char boundary");
		out.push(c);
		at += c.len_utf8();
	}
	out
}

/// First `delim` after `from` with at least one character between, on one line.
fn find_delimiter(text: &str, from: usize, delim: &str) -> Option<usize> {
	let line_end = text[from..].find('\n').map(|at| from + at).unwrap_or(text.len());
	let mut search = from;
	while let Some(found) = text[search..line_end].find(delim) {
		let close = search + found;
		if close > from {
			return Some(close);
		}
		search = close + delim.len();
	}
	None
}

/// Replace `*italic*` or `_italic_` with its content.
fn strip_italic(text: &str, marker: char) -> String {
	let mut out = String::with_capacity(text.len());
	let mut at = 0;
	while at < text.len() {
		if text[at..].starts_with(marker) {
			let preceded_ok = at == 0 || {
				let previous = text[..at].chars().next_back().expect("at is a char boundary");
				if marker == '*' {
					!is_word_char(previous) && previous != '*'
				} else {
					!is_word_char(previous)
				}
			};
			let after = at + marker.len_utf8();
			let followed_ok = text[after..].chars().next().map_or(false, |c| !is_js_ws(c));
			if preceded_ok && followed_ok {
				if let Some(close) = find_italic_close(text, after, marker) {
					out.push_str(&text[after..close]);
					at = close + marker.len_utf8();
					continue;
				}
			}
		}
		let c = text[at..].chars().next().expect("at is a char boundary");
		out.push(c);
		at += c.len_utf8();
	}
	out
}

/// First closing `marker` after `from`: content of at least one character, no
/// newline, not preceded by whitespace, and not followed by a word character
/// (or `*` for `*` italics).
fn find_italic_close(text: &str, from: usize, marker: char) -> Option<usize> {
	let line_end = text[from..].find('\n').map(|at| from + at).unwrap_or(text.len());
	let mut search = from;
	while let Some(found) = text[search..line_end].find(marker) {
		let close = search + found;
		if close > from {
			let previous = text[..close].chars().next_back().expect("close follows content");
			let followed_ok = match text[close + marker.len_utf8()..].chars().next() {
				None => true,
				Some(c) => {
					if marker == '*' {
						!is_word_char(c) && c != '*'
					} else {
						!is_word_char(c)
					}
				}
			};
			if !is_js_ws(previous) && followed_ok {
				return Some(close);
			}
		}
		search = close + marker.len_utf8();
	}
	None
}

/// `#12`, a bare hex hash, a hash leading its subject, `PR #3`, `L42`.
fn identifier_match(text: &str) -> bool {
	let lower = text.to_ascii_lowercase();
	if lower.starts_with('#') && digit_run(text, 1) > 0 {
		return true;
	}
	if (7..=40).contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit()) {
		return true;
	}
	if let Some(whitespace) = text.find(|c: char| is_js_ws(c)) {
		let hash = &text[..whitespace];
		if (7..=40).contains(&hash.len())
			&& hash.chars().all(|c| c.is_ascii_hexdigit())
			&& hash.chars().any(|c| c.is_ascii_digit())
			&& hash.chars().any(|c| ('a'..='f').contains(&c))
		{
			return true;
		}
	}
	if lower.starts_with("pr") {
		let mut at = skip_ws(text, 2);
		if text[at..].starts_with('#') {
			at += 1;
		}
		if digit_run(text, at) > 0 {
			return true;
		}
	}
	if lower.starts_with('l') {
		let digits = digit_run(text, 1);
		if digits > 0 {
			let end = 1 + digits;
			if end == text.len() || !is_word_char(text[end..].chars().next().expect("end is a char boundary")) {
				return true;
			}
		}
	}
	false
}

/// A leading count before a status word (`3 ok`).
static LEADING_COUNT: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^[0-9]+\s+").expect("valid leading count regex"));

/// A short word after a value (`pass`).
static SHORT_WORD: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_-]{0,11}$").expect("valid short word regex"));

/// The status a cell's text leads with, or `None` for any other text.
pub fn cell_status(text: &str) -> Option<Status> {
	let trimmed = plain_cell(text).text.trim().to_string();
	for (mark, status) in STATUS_MARKS {
		if trimmed.starts_with(mark) {
			return Some(status);
		}
	}
	let candidate = LEADING_COUNT.replace(&trimmed, "").to_string();
	let found = STATUS_WORD.find(&candidate)?;
	if found.start() != 0 {
		return None;
	}
	let end = found.end();
	if let Some(c) = candidate[end..].chars().next() {
		if is_word_char(c) || c == '-' {
			return None;
		}
	}
	STATUS_WORDS.get(candidate[..end].to_lowercase().as_str()).copied()
}

fn base_cell(text: &str, emphasis: bool) -> NumberCell {
	NumberCell {
		value: 0.0,
		dim: Dimension::Count,
		unit: String::new(),
		fit: CellFit::Pure,
		approx: false,
		signed: false,
		emphasis,
		text: text.to_string(),
		figure: String::new(),
		to: None,
		to_figure: None,
		upper: None,
		denominator: None,
		error: None,
		values: None,
	}
}

/// The groups of a `CELL` match, parsed by hand (the reference pattern uses a
/// lookahead Rust's `regex` cannot express).
struct CellParts {
	approx: bool,
	sign: Option<char>,
	cur: Option<char>,
	num: String,
	num_start: usize,
	num_end: usize,
	plus: bool,
	unit: String,
	rest: String,
}

/// The unit group of a `CELL` match, ending at `pos`.
fn match_cell_unit(text: &str, pos: usize) -> usize {
	let Some(c) = text[pos..].chars().next() else {
		return pos;
	};
	if c == '%' {
		return pos + 1;
	}
	if c == '/' && text[pos + 1..].starts_with('s') {
		return pos + 2;
	}
	if c.is_ascii_alphabetic() || c == 'µ' || c == 'μ' || c == '×' {
		let mut end = pos + c.len_utf8();
		while let Some(inner) = text[end..].chars().next() {
			if inner.is_ascii_alphabetic() || inner == 'µ' || inner == 'μ' || inner == '×' {
				end += inner.len_utf8();
			} else {
				break;
			}
		}
		if text[end..].starts_with('/') {
			let mut tail = end + 1;
			while let Some(inner) = text[tail..].chars().next() {
				if inner.is_ascii_alphabetic() {
					tail += inner.len_utf8();
				} else {
					break;
				}
			}
			if tail > end + 1 {
				return tail;
			}
		}
		return end;
	}
	pos
}

/// Parse `text` as a `CELL`: approximation, sign, currency, number, trailing
/// `+`, unit, and the rest.
fn match_cell(text: &str) -> Option<CellParts> {
	let mut pos = 0;
	let approx = if text.starts_with("<=") || text.starts_with(">=") {
		pos += 2;
		true
	} else {
		match text[pos..].chars().next() {
			Some(c @ ('~' | '≈' | '<' | '>' | '≤' | '≥')) => {
				pos += c.len_utf8();
				true
			}
			_ => false,
		}
	};
	pos = skip_ws(text, pos);
	let sign = match text[pos..].chars().next() {
		Some(c @ ('+' | '-' | '−' | '±')) => {
			pos += c.len_utf8();
			Some(c)
		}
		_ => None,
	};
	pos = skip_ws(text, pos);
	let cur = match text[pos..].chars().next() {
		Some(c @ ('$' | '€' | '£' | '¥')) => {
			pos += c.len_utf8();
			Some(c)
		}
		_ => None,
	};
	pos = skip_ws(text, pos);
	let num_end = match_number(text, pos)?;
	let num = text[pos..num_end].to_string();
	let num_start = pos;
	pos = num_end;
	let mut plus = false;
	if text[pos..].starts_with('+') {
		let after = skip_ws(text, pos + 1);
		if matches!(text[after..].chars().next(), Some(c) if c.is_ascii_alphabetic() || c == 'µ' || c == 'μ' || c == '%')
		{
			plus = true;
			pos += 1;
		}
	}
	pos = skip_ws(text, pos);
	let unit_end = match_cell_unit(text, pos);
	let unit = text[pos..unit_end].to_string();
	pos = unit_end;
	Some(CellParts {
		approx,
		sign,
		cur,
		num,
		num_start,
		num_end,
		plus,
		unit,
		rest: text[pos..].to_string(),
	})
}

/// A number with an optional trailing unit, as the tail patterns match them.
struct TailMatch {
	num: String,
	unit: String,
	full: String,
	sign: Option<char>,
}

/// The unit of a range or transition tail: `%` or letters with `×`.
fn match_tail_unit(text: &str, pos: usize) -> usize {
	let Some(c) = text[pos..].chars().next() else {
		return pos;
	};
	if c == '%' {
		return pos + 1;
	}
	if c.is_ascii_alphabetic() || c == 'µ' || c == 'μ' || c == '×' {
		let mut end = pos + c.len_utf8();
		while let Some(inner) = text[end..].chars().next() {
			if inner.is_ascii_alphabetic() || inner == 'µ' || inner == 'μ' || inner == '×' {
				end += inner.len_utf8();
			} else {
				break;
			}
		}
		return end;
	}
	pos
}

/// The stated spread after a value: `± 0.69`, `±.074`, `(±0.16)`, `± 2.9 t/s`.
fn error_tail(rest: &str) -> Option<TailMatch> {
	let mut pos = 0;
	if rest[pos..].starts_with('(') {
		pos += 1;
	}
	pos = skip_ws(rest, pos);
	if !rest[pos..].starts_with('±') {
		return None;
	}
	pos += '±'.len_utf8();
	pos = skip_ws(rest, pos);
	let num_end = match_number(rest, pos)?;
	let num = rest[pos..num_end].to_string();
	pos = num_end;
	pos = skip_ws(rest, pos);
	let unit_end = match_error_unit(rest, pos);
	let unit = rest[pos..unit_end].to_string();
	pos = unit_end;
	pos = skip_ws(rest, pos);
	if rest[pos..].starts_with(')') {
		pos += 1;
	}
	if pos != rest.len() {
		return None;
	}
	Some(TailMatch { num, unit, full: rest.to_string(), sign: None })
}

/// The unit of an error tail: `%` or letters, optionally per letters.
fn match_error_unit(text: &str, pos: usize) -> usize {
	let Some(c) = text[pos..].chars().next() else {
		return pos;
	};
	if c == '%' {
		return pos + 1;
	}
	if c.is_ascii_alphabetic() || c == 'µ' || c == 'μ' {
		let mut end = pos + c.len_utf8();
		while let Some(inner) = text[end..].chars().next() {
			if inner.is_ascii_alphabetic() || inner == 'µ' || inner == 'μ' {
				end += inner.len_utf8();
			} else {
				break;
			}
		}
		if text[end..].starts_with('/') {
			let mut tail = end + 1;
			while let Some(inner) = text[tail..].chars().next() {
				if inner.is_ascii_alphabetic() {
					tail += inner.len_utf8();
				} else {
					break;
				}
			}
			if tail > end + 1 {
				return tail;
			}
		}
		return end;
	}
	pos
}

/// `24–72 h`: a dash then the range's end and its unit.
fn range_tail(rest: &str) -> Option<TailMatch> {
	let mut pos = 0;
	let dash = rest.chars().next()?;
	if !matches!(dash, '-' | '–' | '—') {
		return None;
	}
	pos += dash.len_utf8();
	pos = skip_ws(rest, pos);
	if matches!(rest[pos..].chars().next(), Some('~') | Some('≈')) {
		pos += 1;
	}
	pos = skip_ws(rest, pos);
	if let Some(c) = rest[pos..].chars().next() {
		if matches!(c, '$' | '€' | '£' | '¥') {
			pos += c.len_utf8();
		}
	}
	pos = skip_ws(rest, pos);
	let num_end = match_number(rest, pos)?;
	let num = rest[pos..num_end].to_string();
	pos = num_end;
	pos = skip_ws(rest, pos);
	let unit_end = match_tail_unit(rest, pos);
	let unit = rest[pos..unit_end].to_string();
	pos = unit_end;
	Some(TailMatch { num, unit, full: rest[..pos].to_string(), sign: None })
}

/// `12 → 15`: an arrow then the value it moves to.
fn arrow_tail(rest: &str) -> Option<TailMatch> {
	let mut pos = 0;
	if rest.starts_with("->") {
		pos += 2;
	} else if let Some(c) = rest.chars().next() {
		if matches!(c, '→' | '⇒') {
			pos += c.len_utf8();
		} else {
			return None;
		}
	} else {
		return None;
	}
	pos = skip_ws(rest, pos);
	if matches!(rest[pos..].chars().next(), Some('~') | Some('≈')) {
		pos += 1;
	}
	pos = skip_ws(rest, pos);
	let sign = match rest[pos..].chars().next() {
		Some(c @ ('+' | '-' | '−')) => {
			pos += c.len_utf8();
			Some(c)
		}
		_ => None,
	};
	pos = skip_ws(rest, pos);
	if let Some(c) = rest[pos..].chars().next() {
		if matches!(c, '$' | '€' | '£' | '¥') {
			pos += c.len_utf8();
		}
	}
	pos = skip_ws(rest, pos);
	let num_end = match_number(rest, pos)?;
	let num = rest[pos..num_end].to_string();
	pos = num_end;
	pos = skip_ws(rest, pos);
	let unit_end = match_tail_unit(rest, pos);
	let unit = rest[pos..unit_end].to_string();
	pos = unit_end;
	Some(TailMatch { num, unit, full: rest[..pos].to_string(), sign })
}

/// `a/b`: the two numbers and what follows.
fn fraction_match(text: &str) -> Option<(String, String, String)> {
	let first_end = match_number(text, 0)?;
	let first = text[..first_end].to_string();
	let mut pos = skip_ws(text, first_end);
	if !text[pos..].starts_with('/') {
		return None;
	}
	pos += 1;
	pos = skip_ws(text, pos);
	let second_end = match_number(text, pos)?;
	let second = text[pos..second_end].to_string();
	if text[second_end..].starts_with('/')
		|| text[second_end..].starts_with(|c: char| c.is_ascii_digit())
	{
		return None;
	}
	Some((first, second, text[second_end..].to_string()))
}

/// Parse the first number of a (Markdown) cell, or classify why it is not one.
pub fn parse_cell(markdown: &str) -> Cell {
	let plain = plain_cell(markdown);
	let text = WHITESPACE
		.replace_all(&MARKS.replace_all(&plain.text, ""), " ")
		.trim()
		.to_string();
	let emphasis = plain.emphasis;
	let other = |kind: OtherKind| Cell::Other(OtherCell { kind, text: plain.text.clone(), emphasis });
	if MISSING.contains(&text.to_lowercase().as_str()) {
		return other(OtherKind::Missing);
	}
	if DATE.is_match(&text) {
		return other(OtherKind::Date);
	}
	if TIME.is_match(&text) {
		return other(OtherKind::Time);
	}
	if VERSION.is_match(&text) {
		return other(OtherKind::Version);
	}
	if identifier_match(&text) || ZERO_PADDED.is_match(&text) {
		return other(OtherKind::Id);
	}
	if LINKED.is_match(markdown) && text.starts_with(|c: char| c.is_ascii_digit()) {
		return other(OtherKind::Id);
	}

	if let Some(cell) = list_cell(&text, &plain) {
		return Cell::Number(cell);
	}
	if let Some(cell) = sum_cell(&text, &plain) {
		return Cell::Number(cell);
	}
	if let Some((first, second, rest)) = fraction_match(&text) {
		return Cell::Number(NumberCell {
			value: to_number(&first),
			dim: Dimension::Fraction,
			unit: "/".to_string(),
			fit: if rest.trim().is_empty() { CellFit::Fraction } else { CellFit::Note },
			figure: text[..text.len() - rest.len()].trim().to_string(),
			denominator: Some(to_number(&second)),
			..base_cell(&plain.text, emphasis)
		});
	}
	if let Some(caps) = COMPOUND_DURATION.captures(&text) {
		let present = (1..=4).filter(|at| caps.get(*at).is_some()).count();
		if present >= 2 {
			let parts: Vec<f64> = (1..=4)
				.map(|at| caps.get(at).map_or(0.0, |m| to_number(m.as_str())))
				.collect();
			let value = parts[0] * 86_400.0 + parts[1] * 3600.0 + parts[2] * 60.0 + parts[3];
			return Cell::Number(NumberCell {
				value,
				dim: Dimension::Duration,
				unit: "s".to_string(),
				fit: CellFit::Pure,
				figure: text.clone(),
				..base_cell(&plain.text, emphasis)
			});
		}
	}

	let Some(parts) = match_cell(&text) else {
		return other(OtherKind::Text);
	};
	let mut unit = parts.unit.clone();
	let mut rest = parts.rest.trim().to_string();
	let mut dim = if parts.cur.is_some() { Dimension::Currency } else { Dimension::Count };
	let mut factor = 1.0;
	if !unit.is_empty() {
		if let Some((known_dim, known_factor)) = known_unit(&unit) {
			dim = known_dim;
			factor = known_factor;
		} else if unit.contains('/') {
			dim = Dimension::Rate;
		} else {
			rest = format!("{}{} {}", if parts.plus { "+" } else { "" }, unit, rest)
				.trim()
				.to_string();
			unit.clear();
		}
	}
	let figure_end = if !unit.is_empty() {
		text.len() - parts.rest.len()
	} else {
		parts.num_start + parts.num.len()
	};
	let figure = text[..figure_end].trim().to_string();
	if let Some(cur) = parts.cur {
		unit = cur.to_string();
	}
	let range = range_tail(&rest);
	let range_unit = range
		.as_ref()
		.and_then(|tail| if tail.unit.is_empty() { None } else { known_unit(&tail.unit) });
	if unit.is_empty() {
		if let Some((known_dim, known_factor)) = range_unit {
			dim = known_dim;
			factor = known_factor;
			unit = range.as_ref().expect("range unit implies a match").unit.clone();
		}
	}
	if parts.cur.is_some() && dim == Dimension::Count {
		dim = Dimension::Currency;
		unit = parts.cur.expect("checked").to_string();
	}
	let negative = parts.sign == Some('-') || parts.sign == Some('−');
	let value = to_number(&parts.num) * factor * if negative { -1.0 } else { 1.0 };
	let mut cell = NumberCell {
		value,
		dim,
		unit: unit.clone(),
		figure: figure.clone(),
		approx: parts.approx || (parts.plus && !unit.is_empty()) || rest == "+",
		signed: parts.sign.is_some(),
		..base_cell(&plain.text, emphasis)
	};
	if rest.is_empty() || rest == "+" || FOOTNOTE.is_match(&rest) {
		cell.fit = CellFit::Pure;
		return Cell::Number(cell);
	}
	if let Some(spread) = error_tail(&rest) {
		let tail = if spread.unit.is_empty() {
			None
		} else {
			known_unit(&spread.unit)
				.or_else(|| spread.unit.contains('/').then_some((Dimension::Rate, 1.0)))
		};
		let own = if unit.is_empty() {
			tail.map(|(tail_dim, tail_factor)| (tail_dim, spread.unit.clone(), value * tail_factor))
		} else {
			None
		};
		let error = to_number(&spread.num)
			* if tail.is_some() && own.is_some() {
				tail.expect("checked").1
			} else {
				factor
			};
		cell.dim = own.as_ref().map_or(cell.dim, |(tail_dim, _, _)| *tail_dim);
		cell.unit = own.as_ref().map_or(cell.unit, |(_, unit, _)| unit.clone());
		cell.value = own.as_ref().map_or(cell.value, |(_, _, value)| *value);
		cell.fit = CellFit::Note;
		cell.error = Some(error);
		cell.figure = format!(
			"{} ± {}{}",
			figure,
			spread.num,
			if spread.unit.is_empty() { String::new() } else { format!(" {}", spread.unit) }
		);
		return Cell::Number(cell);
	}
	if let Some(range) = range {
		let upper = to_number(&range.num) * tail_factor(Some(range.unit.as_str()), factor);
		cell.fit = CellFit::Range;
		cell.upper = Some(upper);
		cell.figure = format!("{}{}", figure, range.full.trim());
		return Cell::Number(cell);
	}
	if let Some(arrow) = arrow_tail(&rest) {
		let sign = if arrow.sign == Some('-') || arrow.sign == Some('−') { -1.0 } else { 1.0 };
		let to = sign * to_number(&arrow.num) * tail_factor(Some(arrow.unit.as_str()), factor);
		cell.fit = CellFit::Arrow;
		cell.to = Some(to);
		cell.to_figure = Some(
			if arrow.full.starts_with("->") {
				arrow.full[2..].trim_start().to_string()
			} else {
				let stripped = &arrow.full[arrow.full.chars().next().map_or(0, |c| c.len_utf8())..];
				stripped.trim_start().to_string()
			},
		);
		return Cell::Number(cell);
	}
	cell.fit = if (rest.starts_with('(') && rest.ends_with(')')) || SHORT_WORD.is_match(&rest) {
		CellFit::Note
	} else {
		CellFit::Prose
	};
	Cell::Number(cell)
}

/// Factor of a unit written after the second number of a range or transition.
fn tail_factor(unit: Option<&str>, fallback: f64) -> f64 {
	match unit {
		None => fallback,
		Some(unit) => known_unit(unit).map_or(fallback, |(_, factor)| factor),
	}
}

/// A number of a list or sum: its value as written, its unit and its marks.
#[derive(Clone)]
struct Item {
	value: f64,
	unit: String,
	approx: bool,
	signed: bool,
	currency: String,
}

/// One `ITEM` match, as the sticky reference pattern matches it.
struct ItemMatch {
	num_start: usize,
	num_end: usize,
	match_end: usize,
	num: String,
	unit: String,
	approx: bool,
	signed: bool,
	negative: bool,
	currency: String,
}

/// Which separator joins two items.
#[derive(Clone, Copy)]
enum Sep {
	/// A slash between list items.
	List,
	/// A plus between sum terms.
	Sum,
	/// An equals before a sum's total.
	Total,
}

/// The items of `text` from its start joined by `separator`.
struct ReadItems {
	items: Vec<Item>,
	separators: Vec<String>,
	end: usize,
}

/// The unit group of an `ITEM`: `%` or letters, optionally per letters.
fn match_item_unit(text: &str, pos: usize) -> usize {
	let Some(c) = text[pos..].chars().next() else {
		return pos;
	};
	if c == '%' {
		return pos + 1;
	}
	if c.is_ascii_alphabetic() || c == 'µ' || c == 'μ' || c == '×' {
		let mut end = pos + c.len_utf8();
		while let Some(inner) = text[end..].chars().next() {
			if inner.is_ascii_alphabetic() || inner == 'µ' || inner == 'μ' || inner == '×' {
				end += inner.len_utf8();
			} else {
				break;
			}
		}
		if text[end..].starts_with('/') {
			let mut tail = end + 1;
			while let Some(inner) = text[tail..].chars().next() {
				if inner.is_ascii_alphabetic() {
					tail += inner.len_utf8();
				} else {
					break;
				}
			}
			if tail > end + 1 {
				return tail;
			}
		}
		return end;
	}
	pos
}

/// Match one `ITEM` sticky at `at`.
fn match_item(text: &str, at: usize) -> Option<ItemMatch> {
	let mut pos = at;
	let approx = matches!(text[pos..].chars().next(), Some('~') | Some('≈'));
	if approx {
		pos += text[pos..].chars().next().expect("checked").len_utf8();
	}
	pos = skip_ws(text, pos);
	let sign = match text[pos..].chars().next() {
		Some(c @ ('+' | '-' | '−')) => {
			pos += c.len_utf8();
			Some(c)
		}
		_ => None,
	};
	pos = skip_ws(text, pos);
	let currency = match text[pos..].chars().next() {
		Some(c @ ('$' | '€' | '£' | '¥')) => {
			pos += c.len_utf8();
			c.to_string()
		}
		_ => String::new(),
	};
	pos = skip_ws(text, pos);
	let num_end = match_number(text, pos)?;
	let num = text[pos..num_end].to_string();
	let num_start = pos;
	pos = num_end;
	pos = skip_ws(text, pos);
	let unit_end = match_item_unit(text, pos);
	let unit = text[pos..unit_end].to_string();
	pos = unit_end;
	Some(ItemMatch {
		num_start,
		num_end,
		match_end: pos,
		num,
		unit,
		approx,
		signed: sign.is_some(),
		negative: sign == Some('-') || sign == Some('−'),
		currency,
	})
}

/// Match a separator sticky at `at`, returning where it ends.
fn match_separator(text: &str, at: usize, sep: Sep) -> Option<usize> {
	let mut pos = skip_ws(text, at);
	let marker = match sep {
		Sep::List => '/',
		Sep::Sum => '+',
		Sep::Total => '=',
	};
	if !text[pos..].starts_with(marker) {
		return None;
	}
	pos += marker.len_utf8();
	Some(skip_ws(text, pos))
}

/// The items of `text` from its start joined by `separator` (two at least).
fn read_items(text: &str, sep: Sep, max: usize) -> Option<ReadItems> {
	let mut items: Vec<Item> = Vec::new();
	let mut separators: Vec<String> = Vec::new();
	let mut end = 0;
	let mut at = 0;
	while items.len() < max {
		let Some(matched) = match_item(text, at) else {
			break;
		};
		let noun = !matched.unit.is_empty() && known_unit(&matched.unit).is_none() && !matched.unit.contains('/');
		items.push(Item {
			value: to_number(&matched.num) * if matched.negative { -1.0 } else { 1.0 },
			unit: if noun { String::new() } else { matched.unit.clone() },
			approx: matched.approx,
			signed: matched.signed,
			currency: matched.currency.clone(),
		});
		end = if noun { matched.num_end } else { matched.match_end };
		at = end;
		if noun {
			break;
		}
		let Some(separator_end) = match_separator(text, at, sep) else {
			break;
		};
		separators.push(text[at..separator_end].to_string());
		at = separator_end;
	}
	if items.len() < 2 {
		return None;
	}
	Some(ReadItems { separators: separators[..items.len() - 1].to_vec(), items, end })
}

/// Dimension and per-item factor of items read as one quantity.
fn item_units(items: &[Item]) -> Option<(Dimension, Vec<f64>, String)> {
	let mut dim: Option<Dimension> = None;
	let mut written: Vec<Option<f64>> = Vec::new();
	let mut carried: Vec<Option<f64>> = Vec::new();
	for item in items {
		let known = if item.unit.is_empty() { None } else { known_unit(&item.unit) };
		if !item.currency.is_empty() && known.is_some() && known.expect("checked").0 != Dimension::Count {
			return None;
		}
		let item_dim = if !item.currency.is_empty() {
			Some(Dimension::Currency)
		} else if !item.unit.is_empty() {
			Some(known.map_or(Dimension::Rate, |(known_dim, _)| known_dim))
		} else {
			None
		};
		if let (Some(item_dim), Some(current)) = (item_dim, dim) {
			if item_dim != current {
				return None;
			}
		}
		dim = dim.or(item_dim);
		written.push(
			if !item.unit.is_empty() || !item.currency.is_empty() {
				Some(known.map_or(1.0, |(_, factor)| factor))
			} else {
				None
			},
		);
		carried.push(
			if !item.unit.is_empty() && item.currency.is_empty() && known.is_some() {
				let (known_dim, factor) = known.expect("checked");
				if known_dim != Dimension::Count {
					Some(factor)
				} else {
					None
				}
			} else {
				None
			},
		);
	}
	let count = items.len();
	let mut factors = vec![1.0; count];
	for at in (0..count).rev() {
		factors[at] = match written[at] {
			Some(factor) => factor,
		None => {
				let next_marked = items
					.get(at + 1)
					.map_or(false, |item| !item.unit.is_empty() || !item.currency.is_empty());
				if next_marked {
					carried[at + 1].unwrap_or(1.0)
				} else if at + 1 < count {
					factors[at + 1]
				} else {
					1.0
				}
			}
		};
	}
	let lead = items
		.iter()
		.find(|item| !item.currency.is_empty())
		.or_else(|| items.iter().find(|item| !item.unit.is_empty()));
	let unit = lead
		.map(|item| if !item.currency.is_empty() { item.currency.clone() } else { item.unit.clone() })
		.unwrap_or_default();
	Some((dim.unwrap_or(Dimension::Count), factors, unit))
}

/// A slash list of measurements as its first value with every value kept.
fn list_cell(text: &str, base: &PlainCell) -> Option<NumberCell> {
	let read = read_items(text, Sep::List, MAX_LISTED_VALUES + 1)?;
	if read.items.len() > MAX_LISTED_VALUES {
		return None;
	}
	let rest = text[read.end..].trim();
	if rest.starts_with(|c: char| c.is_ascii_digit() || c == '/') {
		return None;
	}
	let tight = read.separators.iter().filter(|separator| separator.as_str() == "/").count();
	if tight > 0 && tight < read.separators.len() {
		return None;
	}
	let marked: Vec<bool> = read
		.items
		.iter()
		.map(|item| !item.unit.is_empty() || !item.currency.is_empty())
		.collect();
	let last = read.items.last()?;
	let carried = marked[..marked.len() - 1].iter().all(|mark| !*mark)
		&& last.currency.is_empty()
		&& known_unit(&last.unit).map_or(true, |(dim, _)| dim != Dimension::Count);
	let slash = text.find('/').map_or(0, |at| at + 1);
	let whole = is_whole(text[slash..read.end].trim());
	let first = &read.items[0];
	let second = &read.items[1];
	let score = whole && first.value >= 0.0 && first.value <= second.value;
	let pair =
		read.items.len() == 2 && marked.iter().all(|mark| !*mark) && read.separators[0] != "/" && !score;
	if !pair && (!marked.last().copied().unwrap_or(false) || !(marked.iter().all(|mark| *mark) || carried)) {
		return None;
	}
	let (dim, factors, unit) = item_units(&read.items)?;
	let values: Vec<f64> = read
		.items
		.iter()
		.enumerate()
		.map(|(at, item)| item.value * factors[at])
		.collect();
	Some(NumberCell {
		value: values[0],
		dim,
		unit,
		fit: if SHORT_NOTE.is_match(rest) { CellFit::Note } else { CellFit::Prose },
		figure: text[..read.end].trim().to_string(),
		approx: read.items.iter().any(|item| item.approx),
		signed: read.items[0].signed,
		values: Some(values),
		..base_cell(&base.text, base.emphasis)
	})
}

/// A sum of one quantity as its total.
fn sum_cell(text: &str, base: &PlainCell) -> Option<NumberCell> {
	let read = read_items(text, Sep::Sum, usize::MAX)?;
	if read.items[0].signed {
		return None;
	}
	let mut end = read.end;
	let mut total: Option<Item> = None;
	if let Some(equals_end) = match_separator(text, end, Sep::Total) {
		if let Some(matched) = match_item(text, equals_end) {
			total = Some(Item {
				value: to_number(&matched.num),
				unit: if !matched.currency.is_empty() { matched.currency.clone() } else { matched.unit.clone() },
				approx: matched.approx,
				signed: false,
				currency: matched.currency.clone(),
			});
			end = matched.match_end;
		}
	}
	let rest = text[end..].trim();
	if rest.starts_with(|c: char| c.is_ascii_digit() || c == '=' || c == '+') {
		return None;
	}
	let terms: Vec<Item> = match total {
		Some(total) => vec![total],
		None => read.items.clone(),
	};
	let (dim, factors, unit) = item_units(&terms)?;
	let value: f64 = terms.iter().enumerate().map(|(at, item)| item.value * factors[at]).sum();
	Some(NumberCell {
		value,
		dim,
		unit,
		fit: if SHORT_NOTE.is_match(rest) { CellFit::Note } else { CellFit::Prose },
		figure: text[..end].trim().to_string(),
		approx: terms.iter().any(|item| item.approx) || read.items.iter().any(|item| item.approx),
		signed: false,
		..base_cell(&base.text, base.emphasis)
	})
}

/// Whether `cell` reads as a number.
pub fn is_number(cell: &Cell) -> bool {
	matches!(cell, Cell::Number(_))
}

/// Strictly increasing or strictly decreasing.
pub fn is_monotonic(values: &[f64]) -> bool {
	if values.len() < 2 {
		return false;
	}
	let up = values[1] > values[0];
	for at in 1..values.len() {
		let step = values[at] - values[at - 1];
		if step == 0.0 || (step > 0.0) != up {
			return false;
		}
	}
	true
}

/// Non-decreasing with at least one repeat: ordered buckets.
fn is_ordered(values: &[f64]) -> bool {
	if values.len() < 3
		|| values.iter().map(|value| value.to_bits()).collect::<HashSet<_>>().len() == values.len()
	{
		return false;
	}
	values.iter().enumerate().all(|(at, value)| at == 0 || *value >= values[at - 1])
}

/// Half the last written digit of `cell`, in base units.
pub fn written_slack(cell: &NumberCell) -> f64 {
	if cell.denominator.is_some() && cell.dim == Dimension::Percent {
		return 0.0;
	}
	let Some((whole, decimals)) = written_parts(&cell.figure) else {
		return 0.0;
	};
	let written = to_number(&format!(
		"{}{}",
		whole,
		decimals.as_ref().map_or(String::new(), |d| format!(".{d}"))
	));
	let factor = if written != 0.0 { cell.value.abs() / written } else { 1.0 };
	let decimals = decimals.map_or(0, |d| d.len());
	if cell.dim == Dimension::Count && decimals == 0 && factor == 1.0 && !cell.approx {
		return 0.0;
	}
	0.5 * 10f64.powi(-(decimals as i32)) * factor
}

/// The first number of a figure, its whole part and its decimals apart.
fn written_parts(figure: &str) -> Option<(String, Option<String>)> {
	let mut at = 0;
	while at < figure.len() {
		if let Some(end) = match_integer(figure, at) {
			let whole = figure[at..end].to_string();
			let mut decimals = None;
			if figure[end..].starts_with('.') {
				let digits = digit_run(figure, end + 1);
				if digits > 0 {
					decimals = Some(figure[end + 1..end + 1 + digits].to_string());
				}
			}
			return Some((whole, decimals));
		}
		at += figure[at..].chars().next().expect("at is a char boundary").len_utf8();
	}
	None
}

/// The cell of `column` at `row` as a plain number of the column's dimension.
fn plain_value(column: &TableColumn, row: usize) -> Option<&NumberCell> {
	let cell = column.cells.get(row)?;
	let number = cell.as_number()?;
	if Some(number.dim) != column.dim || number.upper.is_some() || number.to.is_some() {
		return None;
	}
	Some(number)
}

/// How many of `rows` the table bolded in `column`.
fn emphasized(column: &TableColumn, rows: &[usize]) -> usize {
	rows.iter().filter(|row| column.cells.get(**row).map_or(false, |cell| cell.emphasis())).count()
}

/// `b = k·a` for one `k` in every row holding both.
fn proportional(a: &TableColumn, b: &TableColumn, rows: &[usize]) -> bool {
	let pairs: Vec<(&NumberCell, &NumberCell)> = rows
		.iter()
		.filter_map(|row| Some((plain_value(a, *row)?, plain_value(b, *row)?)))
		.collect();
	if pairs.len() < 3 || pairs.iter().any(|(x, y)| x.value <= 0.0 || y.value <= 0.0) {
		return false;
	}
	let xs: Vec<f64> = pairs.iter().map(|(x, _)| x.value).collect();
	if xs.iter().cloned().fold(f64::MIN, f64::max) < 1.5 * xs.iter().cloned().fold(f64::MAX, f64::min) {
		return false;
	}
	let mut low: f64 = 0.0;
	let mut high = f64::INFINITY;
	let mut tight = 0;
	for (x, y) in &pairs {
		let (sx, sy) = (written_slack(x), written_slack(y));
		if sx >= x.value {
			return false;
		}
		low = low.max((y.value - sy) / (x.value + sx));
		high = high.min((y.value + sy) / (x.value - sx));
		if sx / x.value + sy / y.value <= TIGHT {
			tight += 1;
		}
	}
	low <= high * (1.0 + 1e-9) && tight >= 3
}

/// `total` steps by `step` from row to row, over three or more steps.
fn running_total(total: &TableColumn, step: &TableColumn, rows: &[usize]) -> bool {
	if total.dim != step.dim || rows.len() < 4 {
		return false;
	}
	let at = |column: &TableColumn, row: usize| plain_value(column, row).map(|cell| cell.value);
	let fits = |offset: bool| {
		rows[1..].iter().enumerate().all(|(index, row)| {
			let now = at(total, *row);
			let before = at(total, rows[index]);
			let by = at(step, if offset { *row } else { rows[index] });
			now.is_some() && before.is_some() && by.map_or(false, |by| by != 0.0) && now == before.map(|b| b + by.unwrap_or(0.0))
		})
	};
	fits(false) || fits(true)
}

/// `ratio = a ÷ b` in every row holding all three.
fn quotient(ratio: &TableColumn, a: &TableColumn, b: &TableColumn, rows: &[usize]) -> bool {
	if !matches!(ratio.dim, Some(Dimension::Count) | Some(Dimension::Ratio))
		|| a.dim != b.dim
		|| a.unit != b.unit
	{
		return false;
	}
	let mut tight = 0;
	let mut matched: Vec<(f64, f64, f64)> = Vec::new();
	for row in rows {
		let (Some(r), Some(x), Some(y)) = (plain_value(ratio, *row), plain_value(a, *row), plain_value(b, *row))
		else {
			continue;
		};
		if x.value <= 0.0 || y.value <= 0.0 || r.value <= 0.0 {
			return false;
		}
		let exact = x.value / y.value;
		let slack = written_slack(r)
			+ exact * (written_slack(x) / x.value + written_slack(y) / y.value)
			+ 1e-9 * exact;
		if (r.value - exact).abs() > slack {
			return false;
		}
		if slack <= LOOSE_QUOTIENT * exact {
			tight += 1;
		}
		matched.push((r.value, x.value, y.value));
	}
	if tight < 3 {
		return false;
	}
	if ratio.dim != a.dim {
		return true;
	}
	let pick = |values: &(f64, f64, f64), at: usize| match at {
		0 => values.0,
		1 => values.1,
		_ => values.2,
	};
	let spread = |at: usize| {
		matched.iter().map(|values| pick(values, at)).fold(f64::MIN, f64::max)
			/ matched.iter().map(|values| pick(values, at)).fold(f64::MAX, f64::min)
	};
	spread(0) < spread(1) && spread(0) < spread(2)
}

/// Columns restating another column's values.
fn restated_columns(measures: &[TableColumn], rows: &[usize]) -> HashMap<usize, usize> {
	let mut restated: HashMap<usize, usize> = HashMap::new();
	let share = |column: &TableColumn| {
		if column.dim != Some(Dimension::Percent) || column.scores {
			return false;
		}
		let sum: f64 = rows.iter().map(|row| plain_value(column, *row).map_or(0.0, |cell| cell.value)).sum();
		(97.0..=103.0).contains(&sum)
	};
	for (at, a) in measures.iter().enumerate() {
		for b in &measures[at + 1..] {
			if restated.contains_key(&a.index) || restated.contains_key(&b.index) || !proportional(a, b, rows) {
				continue;
			}
			let bolder = emphasized(b, rows) > emphasized(a, rows);
			if (share(b) && !share(a)) || bolder {
				restated.insert(a.index, b.index);
			} else {
				restated.insert(b.index, a.index);
			}
		}
	}
	for column in measures {
		if restated.contains_key(&column.index) {
			continue;
		}
		let source = measures
			.iter()
			.find(|other| !std::ptr::eq(*other, column) && running_total(column, *other, rows));
		if let Some(source) = source {
			restated.insert(column.index, source.index);
		}
	}
	let kept: Vec<&TableColumn> = measures.iter().filter(|column| !restated.contains_key(&column.index)).collect();
	for ratio in &kept {
		if restated.contains_key(&ratio.index) {
			continue;
		}
		let terms: Vec<&TableColumn> = kept
			.iter()
			.copied()
			.filter(|column| !std::ptr::eq(*column, *ratio) && !restated.contains_key(&column.index))
			.collect();
		let over = (0..terms.len())
			.flat_map(|a| (0..terms.len()).map(move |b| (a, b)))
			.find(|(a, b)| a != b && quotient(ratio, terms[*a], terms[*b], rows));
		if let Some((a, _)) = over {
			restated.insert(ratio.index, terms[a].index);
		}
	}
	restated
}

/// Subsets of `items` from `min` to `max` long, smallest first.
fn subsets<T: Clone>(items: &[T], min: usize, max: usize) -> Vec<Vec<T>> {
	fn grow<T: Clone>(
		items: &[T],
		start: usize,
		picked: &mut Vec<T>,
		min: usize,
		max: usize,
		out: &mut Vec<Vec<T>>,
	) {
		if picked.len() >= min {
			out.push(picked.clone());
		}
		if picked.len() == max {
			return;
		}
		for at in start..items.len() {
			picked.push(items[at].clone());
			grow(items, at + 1, picked, min, max, out);
			picked.pop();
		}
	}
	let mut out = Vec::new();
	grow(items, 0, &mut Vec::new(), min, max, &mut out);
	out.sort_by_key(|subset| subset.len());
	out
}

/// `parts` sum to `whole`, or stay within it, in every row holding all of them.
fn adds(whole: &TableColumn, parts: &[TableColumn], rows: &[usize], exact: bool) -> bool {
	let mut held = 0;
	let mut short = false;
	let mut used: HashSet<usize> = HashSet::new();
	for row in rows {
		let Some(total) = plain_value(whole, *row) else {
			continue;
		};
		let cells: Vec<Option<&NumberCell>> = parts.iter().map(|part| plain_value(part, *row)).collect();
		if cells.iter().any(|cell| cell.is_none()) {
			continue;
		}
		held += 1;
		let sum: f64 = cells.iter().map(|cell| cell.unwrap_or_else(|| panic!("checked")).value).sum();
		let slack = written_slack(total)
			+ cells
				.iter()
				.map(|cell| written_slack(cell.unwrap_or_else(|| panic!("checked"))))
				.sum::<f64>()
			+ 1e-9 * total.value;
		if exact {
			if (sum - total.value).abs() > slack {
				return false;
			}
		} else {
			if sum > total.value + slack {
				return false;
			}
			if sum < total.value - slack {
				short = true;
			}
		}
		for (at, cell) in cells.iter().enumerate() {
			if cell.map_or(false, |cell| cell.value > 0.0) {
				used.insert(parts[at].index);
			}
		}
	}
	held >= 3 && used.len() == parts.len() && (exact || short)
}

/// The parts `measures` add up to.
fn find_composition(measures: &[TableColumn], rows: &[usize]) -> Option<Composition> {
	let pool: Vec<TableColumn> = measures
		.iter()
		.filter(|column| {
			!column.mixed
				&& column.arrow_share == 0.0
				&& rows.iter().all(|row| {
					let cell = column.cells.get(*row);
					cell.map_or(true, |cell| {
						cell.as_number().map_or(true, |number| number.value >= 0.0 && number.upper.is_none())
					})
				})
		})
		.take(MAX_COMPOSED)
		.cloned()
		.collect();
	let mut sums: HashMap<usize, Vec<TableColumn>> = HashMap::new();
	let mut order: Vec<usize> = Vec::new();
	for whole in &pool {
		let others: Vec<TableColumn> = pool
			.iter()
			.filter(|column| column.index != whole.index && column.dim == whole.dim)
			.cloned()
			.collect();
		for subset in subsets(&others, 2, MAX_PARTS) {
			if sums.contains_key(&whole.index) {
				break;
			}
			if adds(whole, &subset, rows, true) {
				sums.insert(whole.index, subset);
				order.push(whole.index);
			}
		}
	}
	if !sums.is_empty() {
		let mut active: HashSet<usize> = HashSet::new();
		fn leaves(
			index: usize,
			sums: &HashMap<usize, Vec<TableColumn>>,
			active: &mut HashSet<usize>,
		) -> Option<Vec<usize>> {
			if active.contains(&index) {
				return None;
			}
			let Some(parts) = sums.get(&index) else {
				return Some(vec![index]);
			};
			active.insert(index);
			let mut branches = Vec::new();
			for part in parts {
				match leaves(part.index, sums, active) {
					Some(branch) => branches.push(branch),
					None => {
						active.remove(&index);
						return None;
					}
				}
			}
			active.remove(&index);
			Some(branches.into_iter().flatten().collect())
		}
		let mut expanded: HashMap<usize, Option<Vec<usize>>> = HashMap::new();
		for index in &order {
			expanded.insert(*index, leaves(*index, &sums, &mut active));
		}
		if expanded.values().any(|parts| parts.is_none()) {
			return None;
		}
		let nested: HashSet<usize> = sums
			.values()
			.flat_map(|parts| parts.iter().map(|part| part.index))
			.collect();
		let mut roots: Vec<usize> = order.iter().copied().filter(|index| !nested.contains(index)).collect();
		roots.sort_by_key(|index| {
			let len = expanded.get(index).and_then(|parts| parts.as_ref()).map_or(0, |parts| parts.len());
			std::cmp::Reverse(len)
		});
		let root = roots.first().copied()?;
		let mut parts = expanded.get(&root).and_then(|parts| parts.clone())?;
		parts.sort_unstable();
		return Some(Composition { whole: Some(root), parts, exact: true });
	}
	let whole = pool.iter().find(|column| WHOLE_HEADER.is_match(&column.header))?;
	let parts: Vec<TableColumn> = pool
		.iter()
		.filter(|column| {
			column.index != whole.index
				&& column.dim == whole.dim
				&& !WHOLE_HEADER.is_match(&column.header)
				&& column.signed_share < 0.5
		})
		.cloned()
		.collect();
	if parts.len() < 2 || parts.len() > MAX_PARTS || !adds(whole, &parts, rows, false) {
		return None;
	}
	let share = |row: usize| {
		let total = plain_value(whole, row).map_or(0.0, |cell| cell.value);
		let sum: f64 = parts.iter().map(|part| plain_value(part, row).map_or(0.0, |cell| cell.value)).sum();
		if total > 0.0 { sum / total } else { 0.0 }
	};
	if !rows.iter().any(|row| share(*row) >= MIN_PART_SHARE) {
		return None;
	}
	Some(Composition { whole: Some(whole.index), parts: parts.iter().map(|part| part.index).collect(), exact: false })
}

/// `columns` with constant measures that are one step of a sweep kept as measures.
fn keep_sweeps(columns: Vec<TableColumn>) -> Vec<TableColumn> {
	let varying: HashSet<String> = columns
		.iter()
		.filter(|column| column.role == ColumnRole::Measure && !column.constant && column.header.chars().any(|c| c.is_ascii_digit()))
		.map(|column| SWEEP_STEP.replace_all(&column.header, "#").to_string())
		.collect();
	columns
		.into_iter()
		.map(|column| {
			if column.constant
				&& column.role == ColumnRole::Measure
				&& varying.contains(&SWEEP_STEP.replace_all(&column.header, "#").to_string())
			{
				TableColumn { constant: false, ..column }
			} else {
				column
			}
		})
		.collect()
}

/// A column with its `constant` flag decided from the data rows.
fn finish_column(column: DraftColumn, rows: &[usize]) -> TableColumn {
	let mut values: HashSet<u64> = HashSet::new();
	let mut count = 0;
	for row in rows {
		if let Some(cell) = column.cells.get(*row) {
			if let Some(number) = cell.as_number() {
				values.insert(number.value.to_bits());
				count += 1;
			}
		}
	}
	TableColumn {
		index: column.index,
		header: column.header,
		role: column.role,
		cells: column.cells,
		dim: column.dim,
		unit: column.unit,
		mixed: column.mixed,
		scores: column.scores,
		statuses: column.statuses,
		constant: count > 0 && values.len() <= 1,
		signed_share: column.signed_share,
		arrow_share: column.arrow_share,
		restates: None,
	}
}

/// Whether `cells` are spans of whole numbers laid end to end in order.
fn tiles_spans(cells: &[&Cell]) -> bool {
	if cells.len() < MIN_TILES {
		return false;
	}
	let mut end: Option<f64> = None;
	let mut ranged = false;
	for cell in cells {
		let marks = MARKS.replace_all(cell.text(), "");
		let marks = marks.trim();
		let Some(caps) = SPAN.captures(marks) else {
			return false;
		};
		let first: f64 = caps.get(1).map_or(0.0, |m| m.as_str().parse().unwrap_or(0.0));
		let last = match caps.get(2) {
			Some(m) => m.as_str().parse().unwrap_or(first),
			None => first,
		};
		if last < first || end.map_or(false, |end| first != end + 1.0) {
			return false;
		}
		ranged = ranged || last > first;
		end = Some(last);
	}
	ranged
}

/// A lone `m` means minutes beside other durations and millions otherwise.
fn resolve_minutes(cells: Vec<Cell>) -> Vec<Cell> {
	let durations: Vec<&NumberCell> = cells
		.iter()
		.filter_map(|cell| cell.as_number())
		.filter(|number| number.dim == Dimension::Duration)
		.collect();
	if durations.is_empty() || !durations.iter().all(|number| number.unit == "m") {
		return cells;
	}
	let rescale = |seconds: Option<f64>| seconds.map(|value| (value / 60.0) * 1e6);
	cells
		.into_iter()
		.map(|cell| {
			if let Cell::Number(mut number) = cell {
				if number.unit == "m" {
					number.dim = Dimension::Count;
					number.value = rescale(Some(number.value)).unwrap_or(number.value);
					number.to = rescale(number.to);
					number.upper = rescale(number.upper);
				}
				Cell::Number(number)
			} else {
				cell
			}
		})
		.collect()
}

/// Counts of another thing than most of the column's read as text.
fn other_nouns(cells: Vec<Cell>, header: &str) -> Vec<Cell> {
	let nouns: Vec<Option<String>> = cells
		.iter()
		.map(|cell| {
			let number = cell.as_number()?;
			if number.dim != Dimension::Count
				|| !number.unit.is_empty()
				|| number.fit == CellFit::Pure
				|| cell_status(cell.text()).is_some()
			{
				return None;
			}
			let at = cell.text().find(&number.figure)? + number.figure.len();
			let word = COUNTED_NOUN.captures(&cell.text()[at..])?.get(1)?.as_str();
			let lower = word.to_lowercase();
			Some(if lower.ends_with("es") {
				lower[..lower.len() - 2].to_string()
			} else if lower.ends_with('s') {
				lower[..lower.len() - 1].to_string()
			} else {
				lower
			})
		})
		.collect();
	let mut tally: Vec<(String, usize)> = Vec::new();
	for noun in nouns.iter().flatten() {
		if let Some(entry) = tally.iter_mut().find(|(key, _)| key == noun) {
			entry.1 += 1;
		} else {
			tally.push((noun.clone(), 1));
		}
	}
	let mut best = (String::new(), 0usize);
	for (noun, count) in &tally {
		if *count > best.1 {
			best = (noun.clone(), *count);
		}
	}
	let number_count = cells.iter().filter(|cell| is_number(cell)).count();
	if best.1 < 2 || best.1 * 2 <= number_count {
		return cells;
	}
	let named = header.to_lowercase();
	let count_of = |noun: &str| tally.iter().find(|(key, _)| key == noun).map_or(0, |(_, count)| *count);
	cells
		.into_iter()
		.enumerate()
		.map(|(at, cell)| {
			let replace = match &nouns[at] {
				Some(noun) => *noun != best.0 && count_of(noun) >= 2 && !named.contains(noun.as_str()),
				None => false,
			};
			if replace {
				Cell::Other(OtherCell { kind: OtherKind::Text, text: cell.text().to_string(), emphasis: cell.emphasis() })
			} else {
				cell
			}
		})
		.collect()
}

/// Bare magnitudes read by what they stand beside.
fn resolve_magnitudes(cells: Vec<Cell>, header: &str) -> Vec<Cell> {
	let numbers: Vec<&NumberCell> = cells.iter().filter_map(|cell| cell.as_number()).collect();
	let suffixed_indices: HashSet<usize> = cells
		.iter()
		.enumerate()
		.filter(|(_, cell)| {
			cell.as_number()
				.map_or(false, |number| number.dim == Dimension::Count && BINARY_POWER.contains_key(number.unit.as_str()))
		})
		.map(|(at, _)| at)
		.collect();
	if suffixed_indices.is_empty() {
		return cells;
	}
	let rescale = |value: Option<f64>, from: f64, to: f64| value.map(|value| (value / from) * to);
	if SIZE_HEADER.is_match(header)
		&& numbers.iter().all(|number| number.dim == Dimension::Count || number.dim == Dimension::Bytes)
	{
		return cells
			.into_iter()
			.enumerate()
			.map(|(at, cell)| {
				if let Cell::Number(number) = &cell {
					if suffixed_indices.contains(&at) {
						let from = known_unit(&number.unit).map_or(1.0, |(_, factor)| factor);
						let to = 1024f64.powi(BINARY_POWER.get(number.unit.as_str()).copied().unwrap_or(0) as i32);
						return Cell::Number(NumberCell {
							dim: Dimension::Bytes,
							value: rescale(Some(number.value), from, to).unwrap_or(number.value),
							to: rescale(number.to, from, to),
							upper: rescale(number.upper, from, to),
							..number.clone()
						});
					}
				}
				cell
			})
			.collect();
	}
	let bytes: Vec<&NumberCell> = numbers.iter().copied().filter(|number| number.dim == Dimension::Bytes).collect();
	let any_millions = cells
		.iter()
		.enumerate()
		.filter(|(at, _)| suffixed_indices.contains(at))
		.filter_map(|(_, cell)| cell.as_number())
		.any(|number| number.unit == "M");
	if bytes.is_empty()
		|| !any_millions
		|| !bytes
			.iter()
			.all(|number| number.unit == "B" && number.value.abs() < 1000.0)
	{
		return cells;
	}
	cells
		.into_iter()
		.map(|cell| {
			if let Cell::Number(number) = &cell {
				if number.dim == Dimension::Bytes {
					return Cell::Number(NumberCell {
						dim: Dimension::Count,
						value: number.value * 1e9,
						to: rescale(number.to, 1.0, 1e9),
						upper: rescale(number.upper, 1.0, 1e9),
						..number.clone()
					});
				}
			}
			cell
		})
		.collect()
}

/// `4:14` beside `8s` and `24s` is four minutes fourteen seconds.
fn resolve_clock(cells: Vec<Cell>) -> Vec<Cell> {
	if !cells
		.iter()
		.any(|cell| matches!(cell, Cell::Number(number) if number.dim == Dimension::Duration))
	{
		return cells;
	}
	cells
		.into_iter()
		.map(|cell| {
			let Cell::Other(other) = &cell else {
				return cell;
			};
			if other.kind != OtherKind::Time {
				return cell;
			}
			let text = MARKS.replace_all(&other.text, "").trim().to_string();
			let Some(caps) = CLOCK.captures(&text) else {
				return cell;
			};
			let value = caps.get(1).map_or(0.0, |m| to_number(m.as_str())) * 3600.0
				+ caps.get(2).map_or(0.0, |m| to_number(m.as_str())) * 60.0
				+ caps.get(3).map_or(0.0, |m| to_number(m.as_str()));
			Cell::Number(NumberCell {
				value,
				dim: Dimension::Duration,
				unit: "s".to_string(),
				fit: CellFit::Pure,
				approx: false,
				signed: false,
				emphasis: other.emphasis,
				text: other.text.clone(),
				figure: text,
				..base_cell(&other.text, other.emphasis)
			})
		})
		.collect()
}

/// Read one column's role and dominant dimension.
fn read_column(index: usize, header: &str, parsed: &[Vec<Cell>]) -> DraftColumn {
	let collected: Vec<Cell> = parsed
		.iter()
		.map(|row| {
			row.get(index).cloned().unwrap_or(Cell::Other(OtherCell {
				kind: OtherKind::Missing,
				text: String::new(),
				emphasis: false,
			}))
		})
		.collect();
	let cells = other_nouns(
		resolve_clock(resolve_magnitudes(resolve_minutes(collected), header)),
		header,
	);
	let present: Vec<&Cell> = cells.iter().filter(|cell| cell.kind() != CellKind::Missing).collect();
	let numbers: Vec<&Cell> = present.iter().copied().filter(|cell| is_number(cell)).collect();
	let strict: Vec<&&Cell> = numbers.iter().filter(|cell| cell.as_number().map_or(false, |n| n.fit != CellFit::Prose)).collect();
	let total = present.len().max(1);
	let number_share = numbers.len() as f64 / total as f64;
	let strict_share = strict.len() as f64 / total as f64;
	let temporal_share = present
		.iter()
		.filter(|cell| cell.kind() == CellKind::Date || cell.kind() == CellKind::Time)
		.count() as f64
		/ total as f64;

	let mut groups: Vec<(Dimension, Vec<NumberCell>)> = Vec::new();
	for cell in &numbers {
		let number = cell.as_number().map_or_else(|| panic!("filtered"), |n| n.clone());
		if let Some(group) = groups.iter_mut().find(|(dim, _)| *dim == number.dim) {
			group.1.push(number);
		} else {
			groups.push((number.dim, vec![number]));
		}
	}
	let mut dominant: Option<&Vec<NumberCell>> = None;
	for (_, group) in &groups {
		if dominant.map_or(true, |current| group.len() > current.len()) {
			dominant = Some(group);
		}
	}
	let dim = dominant.and_then(|group| group.first()).map(|number| number.dim);
	let mixed = !numbers.is_empty() && dominant.map_or(0, |group| group.len()) as f64 / (numbers.len() as f64) < 0.8;
	let scored = numbers
		.iter()
		.filter(|cell| {
			cell.as_number()
				.map_or(false, |number| number.dim == Dimension::Percent && number.denominator.is_some())
		})
		.count();
	let unit = match dim {
		Some(Dimension::Currency) | Some(Dimension::Rate) => {
			dominant.and_then(|group| group.first()).map_or(String::new(), |number| number.unit.clone())
		}
		_ => String::new(),
	};

	let statuses: Vec<Option<Status>> = cells.iter().map(|cell| cell_status(cell.text())).collect();
	let written = cells.iter().filter(|cell| !cell.text().trim().is_empty()).count();
	let status_share = if written > 0 {
		statuses.iter().filter(|status| status.is_some()).count() as f64 / written as f64
	} else {
		0.0
	};

	let trimmed = header.trim();
	let sequential = index == 0
		&& numbers.len() >= 3
		&& numbers.len() == present.len()
		&& numbers.iter().enumerate().all(|(at, cell)| {
			let number = cell.as_number().map_or_else(|| panic!("filtered"), |n| n);
			number.dim == Dimension::Count
				&& number.value == numbers[0].as_number().map_or(0.0, |n| n.value) + at as f64
		});
	let bare = numbers.iter().all(|cell| {
		cell.as_number()
			.map_or(false, |number| number.dim == Dimension::Count && number.unit.is_empty())
	});
	let tallies = !numbers.is_empty()
		&& bare
		&& cells.iter().enumerate().all(|(at, cell)| {
			!is_number(cell) || statuses.get(at).copied().flatten().is_some()
		});
	let role = if temporal_share >= 0.8 {
		ColumnRole::Temporal
	} else if tallies {
		ColumnRole::Label
	} else if number_share >= 0.8 && (sequential || (bare && INDEX_HEADER.is_match(trimmed))) {
		ColumnRole::Index
	} else if index > 0 && bare && tiles_spans(&present) {
		ColumnRole::Index
	} else if number_share >= 0.8
		&& bare
		&& SEQUENCE_HEADER.is_match(trimmed)
		&& (is_monotonic(&numbers.iter().map(|cell| cell.as_number().map_or(0.0, |n| n.value)).collect::<Vec<_>>())
			|| is_ordered(&numbers.iter().map(|cell| cell.as_number().map_or(0.0, |n| n.value)).collect::<Vec<_>>()))
	{
		ColumnRole::Sequence
	} else if number_share >= 0.8 && strict_share >= 0.6 {
		ColumnRole::Measure
	} else if number_share >= 0.6
		&& strict_share >= 0.5
		&& present
			.iter()
			.all(|cell| is_number(cell) || utf16_len(cell.text()) <= 24)
	{
		ColumnRole::Measure
	} else {
		ColumnRole::Label
	};

	let statuses = if status_share >= 0.6 {
		Some(
			statuses
				.into_iter()
				.enumerate()
				.map(|(at, status)| {
					status.or_else(|| {
						if HEDGE.is_match(cells.get(at).map_or("", |cell| cell.text())) {
							Some(Status::Warn)
						} else {
							None
						}
					})
				})
				.collect(),
		)
	} else {
		None
	};
	DraftColumn {
		index,
		header: header.to_string(),
		role,
		statuses,
		dim,
		unit,
		mixed,
		scores: !numbers.is_empty() && scored as f64 / numbers.len() as f64 >= 0.8,
		signed_share: if numbers.is_empty() {
			0.0
		} else {
			numbers
				.iter()
				.filter(|cell| cell.as_number().map_or(false, |number| number.signed))
				.count() as f64
				/ numbers.len() as f64
		},
		arrow_share: if numbers.is_empty() {
			0.0
		} else {
			numbers
				.iter()
				.filter(|cell| cell.as_number().map_or(false, |number| number.to.is_some()))
				.count() as f64
				/ numbers.len() as f64
		},
		cells,
	}
}

/// Leading text columns written as an outline repeat their parent's name.
fn fill_down(columns: Vec<DraftColumn>) -> Vec<DraftColumn> {
	let first_measure = columns.iter().position(|column| column.role == ColumnRole::Measure);
	columns
		.into_iter()
		.map(|column| {
			if column.role != ColumnRole::Label
				|| first_measure.map_or(false, |first| column.index > first)
			{
				return column;
			}
			let blank = |cell: &Cell| cell.kind() == CellKind::Missing && cell.text().is_empty();
			let cells = &column.cells;
			if cells.len() < 3 || !blank(&cells[0]) || !cells.iter().any(blank) {
				return column;
			}
			let mut filled: Vec<Cell> = Vec::with_capacity(cells.len());
			for cell in cells {
				if blank(cell) {
					let last = filled.last().cloned().unwrap_or_else(|| cell.clone());
					filled.push(last);
				} else {
					filled.push(cell.clone());
				}
			}
			DraftColumn { cells: filled, ..column }
		})
		.collect()
}

/// How an `a/b` cell reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FractionKind {
	/// A score out of a whole.
	Score,
	/// A list like `327 / 333 / 339`.
	List,
	/// A plain fraction.
	Fraction,
}

/// How an `a/b` cell reads: score, list, plain fraction, or nothing.
fn fraction_kind(cell: Option<&Cell>) -> Option<FractionKind> {
	let number = cell?.as_number()?;
	if number.dim != Dimension::Fraction {
		return None;
}
	let denominator = number.denominator?;
	if denominator == 0.0 {
		return None;
	}
	let after = number.text.find(&number.figure)?;
	if number.text[after + number.figure.len()..].trim_start().starts_with('/') {
		return Some(FractionKind::List);
	}
	let whole = WHOLE_DENOMINATOR.is_match(&number.figure);
	if whole && number.value >= 0.0 && number.value <= denominator {
		Some(FractionKind::Score)
	} else {
		Some(FractionKind::Fraction)
	}
}

/// A bare slash list of three or more values unlike each other.
fn unlike_list(cell: &Cell) -> bool {
	let Some(number) = cell.as_number() else {
		return false;
	};
	if number.dim != Dimension::Fraction {
		return false;
	}
	let Some(read) = read_items(&MARKS.replace_all(&number.text, "").trim().to_string(), Sep::List, MAX_LISTED_VALUES)
	else {
		return false;
	};
	if read.items.len() < 3 || read.items.iter().any(|item| !item.unit.is_empty() || !item.currency.is_empty()) {
		return false;
	}
	let values: Vec<f64> = read.items.iter().map(|item| item.value).collect();
	values.iter().any(|value| *value <= 0.0)
		|| values.iter().cloned().fold(f64::MIN, f64::max) > ALIKE_SPREAD * values.iter().cloned().fold(f64::MAX, f64::min)
}

/// A spaced bare `a / b` read as the pair of values it lists.
fn as_pair(cell: &Cell) -> Cell {
	let Some(number) = cell.as_number() else {
		return cell.clone();
	};
	if number.denominator.is_none()
		|| number.fit != CellFit::Fraction
		|| !PAIR_SLASH.is_match(&number.figure)
	{
		return cell.clone();
	}
	Cell::Number(NumberCell {
		dim: Dimension::Count,
		unit: String::new(),
		fit: CellFit::Note,
		denominator: None,
		values: Some(vec![number.value, number.denominator.unwrap_or(0.0)]),
		..number.clone()
	})
}

/// A score as the percent of its denominator it reached.
fn as_score(cell: &Cell) -> Cell {
	let Some(number) = cell.as_number() else {
		return cell.clone();
	};
	let Some(denominator) = number.denominator else {
		return cell.clone();
	};
	if denominator == 0.0 || fraction_kind(Some(cell)) != Some(FractionKind::Score) {
		return cell.clone();
	}
	Cell::Number(NumberCell {
		dim: Dimension::Percent,
		value: (100.0 * number.value) / denominator,
		..number.clone()
	})
}

/// Two or more copies of one table set side by side, read as one table.
fn unstack(headers: &[String], rows: &[Vec<String>]) -> Option<(usize, Vec<Vec<String>>)> {
	for width in 2..headers.len() {
		if width * 2 > headers.len() || headers.len() % width != 0 {
			continue;
		}
		if !headers[..width].iter().any(|name| !name.is_empty()) {
			continue;
		}
		if !headers.iter().enumerate().all(|(at, name)| name == &headers[at % width]) {
			continue;
		}
		let mut stacked: Vec<Vec<String>> = Vec::new();
		let mut start = 0;
		while start < headers.len() {
			for row in rows {
				let block: Vec<String> = (0..width)
					.map(|at| row.get(start + at).cloned().unwrap_or_default())
					.collect();
				if block.iter().any(|cell| !plain_cell(cell).text.is_empty()) {
					stacked.push(block);
				}
			}
			start += width;
		}
		return Some((width, stacked));
	}
	None
}

/// Rows folding several rows into one, split back into one row per item.
fn unlist(rows: &[Vec<String>]) -> Vec<Vec<String>> {
	let mut out: Vec<Vec<String>> = Vec::new();
	for row in rows {
		let plains: Vec<PlainCell> = row.iter().map(|cell| plain_cell(cell)).collect();
		let lists: Vec<Vec<&str>> = plains.iter().map(|plain| LIST_SLASH.split(plain.text.as_str()).collect()).collect();
		let size = lists.iter().map(|list| list.len()).max().unwrap_or(0);
		if size < MIN_LISTED_PAIR {
			out.push(row.clone());
			continue;
		}
		let numeric: Vec<bool> = lists
			.iter()
			.map(|list| list.iter().all(|item| listed_number(item)))
			.collect();
		let name = lists
			.iter()
			.enumerate()
			.find(|(at, list)| list.len() == size && !numeric[*at])
			.map(|(at, _)| at);
		let fits = lists.iter().enumerate().all(|(at, list)| {
			at == name.unwrap_or(usize::MAX)
				|| (list.len() == size && numeric[at])
				|| (list.len() != size && (!numeric[at] || list.first().map_or(true, |first| first.is_empty())))
		});
		let listed = lists
			.iter()
			.enumerate()
			.filter(|(at, list)| list.len() == size && numeric[*at])
			.count();
		if name.is_none() || !fits || listed < MIN_LISTS {
			out.push(row.clone());
			continue;
		}
		let name = name.unwrap_or(0);
		if size < MIN_LISTED
			&& lists
				.iter()
				.enumerate()
				.any(|(at, list)| at < name && list.len() == size)
		{
			out.push(row.clone());
			continue;
		}
		let mut items: Vec<String> = lists[name].iter().map(|item| item.to_string()).collect();
		let first = items[0].clone();
		let last = items[size - 1].clone();
		let before = match first.rfind(' ') {
			Some(at) => first[..at + 1].to_string(),
			None => String::new(),
		};
		let aside = aside_index(&last);
		let after = aside.map_or(String::new(), |at| last[at..].to_string());
		items[0] = first[before.len()..].to_string();
		items[size - 1] = last[..last.len() - after.len()].to_string();
		for (index, item) in items.iter().enumerate() {
			let split: Vec<String> = row
				.iter()
				.enumerate()
				.map(|(at, cell)| {
					if at == name {
						format!("{before}{item}{after}")
					} else if lists[at].len() == size {
						lists[at][index].to_string()
					} else {
						cell.clone()
					}
				})
				.collect();
			out.push(split);
		}
	}
	out
}

/// A list item that is one number, as written.
fn listed_number(item: &str) -> bool {
	let mut rest = item.trim();
	if rest.starts_with('~') || rest.starts_with('≈') {
		rest = &rest[1..];
	}
	if let Some(stripped) = rest.strip_prefix(['$', '€', '£', '¥']) {
		rest = stripped;
	}
	let Some(end) = match_integer(rest, 0) else {
		return false;
	};
	if end != rest.len() {
		let tail = &rest[end..];
		let tail = tail.strip_prefix('.').map_or(tail, |tail| {
			let digits = digit_run(tail, 0);
			if digits > 0 { &tail[digits..] } else { tail }
		});
		let tail = tail.trim_start();
		if tail.is_empty() {
			return true;
		}
		if tail == "%" {
			return true;
		}
		let mut chars = tail.chars();
		return chars.next().map_or(false, |c| c.is_ascii_alphabetic() || c == 'µ' || c == 'μ')
			&& chars.all(|c| c.is_ascii_alphabetic() || c == 'µ' || c == 'μ');
	}
	true
}

/// Where a list item's aside begins: whitespace before a symbol.
fn aside_index(last: &str) -> Option<usize> {
	let chars: Vec<(usize, char)> = last.char_indices().collect();
	let mut at = 0;
	while at < chars.len() {
		if is_js_ws(chars[at].1) {
			let start = chars[at].0;
			let mut end = at;
			while end < chars.len() && is_js_ws(chars[end].1) {
				end += 1;
			}
			if end < chars.len() {
				let next = chars[end].1;
				if !next.is_alphabetic() && !next.is_numeric() && !is_js_ws(next) {
					return Some(start);
				}
			}
			at = end;
		} else {
			at += 1;
		}
	}
	None
}

/// Read a table's columns and rows for charting.
pub fn analyze_table(header: &[String], rows: &[Vec<String>]) -> TableAnalysis {
	let mut headers: Vec<String> = header.iter().map(|cell| plain_cell(cell).text).collect();
	let mut owned: Vec<Vec<String>> = rows.to_vec();
	if let Some((width, stacked)) = unstack(&headers, &owned) {
		headers.truncate(width);
		owned = stacked;
	}
	let rows = unlist(&owned);
	let raw: Vec<Vec<Cell>> = rows
		.iter()
		.map(|row| {
			headers
				.iter()
				.enumerate()
				.map(|(index, _)| row.get(index).map_or_else(|| parse_cell(""), |cell| parse_cell(cell)))
				.collect()
		})
		.collect();
	let paired: Vec<bool> = raw
		.iter()
		.map(|cells| {
			let first = cells.iter().position(|cell| is_number(cell)).unwrap_or(0);
			cells[..first].iter().any(|cell| cell.kind() == CellKind::Text && PAIR_SLASH.is_match(cell.text()))
		})
		.collect();
	let scoring: Vec<bool> = headers
		.iter()
		.enumerate()
		.map(|(index, text)| {
			!PAIR_SLASH.is_match(text)
				&& raw.iter().enumerate().all(|(row, cells)| {
					paired[row] || fraction_kind(cells.get(index)) != Some(FractionKind::Fraction)
				})
		})
		.collect();
	let pairing: Vec<bool> = headers
		.iter()
		.enumerate()
		.map(|(index, _)| {
			raw.iter().any(|cells| {
				cells.get(index).map_or(false, |cell| {
					cell.as_number()
						.map_or(false, |number| number.values.is_some() && number.unit.is_empty())
				})
			})
		})
		.collect();
	let parsed: Vec<Vec<Cell>> = raw
		.iter()
		.zip(paired.iter())
		.map(|(cells, paired)| {
			cells
				.iter()
				.enumerate()
				.map(|(index, cell)| {
					if !PAIR_SLASH.is_match(&headers[index]) && unlike_list(cell) {
						Cell::Other(OtherCell {
							kind: OtherKind::Text,
							text: cell.text().to_string(),
							emphasis: cell.emphasis(),
						})
					} else if pairing[index] {
						as_pair(cell)
					} else if scoring[index] && !*paired {
						as_score(cell)
					} else {
						cell.clone()
					}
				})
				.collect()
		})
		.collect();
	let mut draft = fill_down(
		headers
			.iter()
			.enumerate()
			.map(|(index, text)| read_column(index, text, &parsed))
			.collect(),
	);
	let unnamed = !draft.iter().any(|column| column.role == ColumnRole::Label || column.role == ColumnRole::Temporal);
	if let Some(lead) = draft.first().cloned() {
		if lead.role == ColumnRole::Measure
			&& unnamed
			&& lead.cells.iter().any(|cell| {
				cell.kind() != CellKind::Missing
					&& (!is_number(cell)
						|| cell.as_number().map_or(false, |number| {
							number.fit == CellFit::Note || number.fit == CellFit::Prose
						}))
			}) {
			draft[0].role = ColumnRole::Label;
		} else if unnamed {
			let worded = draft.iter().position(|column| {
				column.role == ColumnRole::Measure
					&& column.mixed
					&& column.cells.iter().filter(|cell| {
						is_number(cell)
							&& cell.as_number().map_or(false, |number| number.fit != CellFit::Pure)
					}).count() * 2 > column.cells.len()
			});
			if let Some(worded) = worded {
				draft[worded].role = ColumnRole::Label;
			}
		}
	}
	let label_index = draft
		.iter()
		.filter(|column| column.role == ColumnRole::Label || column.role == ColumnRole::Temporal)
		.find(|column| column.cells.iter().any(|cell| worded_cell(cell)))
		.or_else(|| {
			draft
				.iter()
				.find(|column| column.role == ColumnRole::Label || column.role == ColumnRole::Temporal)
		})
		.or_else(|| draft.iter().find(|column| column.role == ColumnRole::Sequence))
		.or_else(|| draft.iter().find(|column| column.role == ColumnRole::Index))
		.map(|column| column.index);
	let mut totals: Vec<usize> = Vec::new();
	let mut data: Vec<usize> = Vec::new();
	let names: Vec<String> = rows
		.iter()
		.enumerate()
		.map(|(row, _)| {
			label_index.map_or_else(String::new, |index| {
				draft
					.get(index)
					.and_then(|column| column.cells.get(row))
					.map_or_else(String::new, |cell| MARKS.replace_all(cell.text(), "").trim().to_string())
			})
		})
		.collect();
	let summary: Vec<bool> = names.iter().map(|name| label_index.is_some() && TOTAL_ROW.is_match(name)).collect();
	let sums_above = |row: usize| -> bool {
		draft.iter().any(|column| {
			let Some(total) = column.cells.get(row).and_then(|cell| cell.as_number()) else {
				return false;
			};
			if column.role != ColumnRole::Measure || total.value <= 0.0 {
				return false;
			}
			let mut sum = 0.0;
			for above in (0..row).rev() {
				let Some(cell) = column.cells.get(above).and_then(|cell| cell.as_number()) else {
					return false;
				};
				if cell.dim != total.dim {
					return false;
				}
				sum += cell.value;
				if above < row - 1 && (sum - total.value).abs() <= total.value * APPROX_TOTAL {
					return true;
				}
			}
			false
		})
	};
	for row in 0..rows.len() {
		let summarizes = summary[row]
			&& (summary[row + 1..].iter().all(|value| *value)
				|| summary[..row].iter().all(|value| *value)
				|| BARE_TOTAL.is_match(&names[row])
				|| sums_above(row));
		if summarizes {
			totals.push(row);
		} else {
			data.push(row);
		}
	}
	let finished = keep_sweeps(
		draft
			.into_iter()
			.map(|column| finish_column(column, &data))
			.collect(),
	);
	let restated = restated_columns(
		&finished
			.iter()
			.filter(|column| column.role == ColumnRole::Measure && !column.constant)
			.cloned()
			.collect::<Vec<_>>(),
		&data,
	);
	let columns: Vec<TableColumn> = finished
		.into_iter()
		.map(|column| match restated.get(&column.index) {
			Some(source) => TableColumn { restates: Some(*source), ..column },
			None => column,
		})
		.collect();
	let measures: Vec<TableColumn> = columns
		.iter()
		.filter(|column| column.role == ColumnRole::Measure && !column.constant && column.restates.is_none())
		.cloned()
		.collect();
	let composition = find_composition(&measures, &data);
	TableAnalysis {
		label: label_index.map(|index| columns[index].clone()),
		columns,
		measures,
		rows: data,
		totals,
		composition,
	}
}

/// A cell that names a row rather than coding it.
fn worded_cell(cell: &Cell) -> bool {
	cell.kind() != CellKind::Missing
		&& ((cell.kind() != CellKind::Id && !is_number(cell)) || cell.text().chars().any(is_js_ws))
}

/// `table` read with `measures` as its only measures and `table.label` naming its rows.
pub fn with_measures(table: &TableAnalysis, measures: &[TableColumn]) -> TableAnalysis {
	with_measures_labeled(table, measures, table.label.clone())
}

/// `table` read with `measures` as its only measures and `label` naming its rows.
pub fn with_measures_labeled(
	table: &TableAnalysis,
	measures: &[TableColumn],
	label: Option<TableColumn>,
) -> TableAnalysis {
	let parts: Vec<usize> = table.composition.as_ref().map_or_else(Vec::new, |composition| composition.parts.clone());
	let whole = !parts.is_empty() && parts.iter().all(|index| measures.iter().any(|column| column.index == *index));
	TableAnalysis {
		columns: table.columns.clone(),
		label,
		measures: measures.to_vec(),
		rows: table.rows.clone(),
		totals: table.totals.clone(),
		composition: if whole { table.composition.clone() } else { None },
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn plain_cell_strips_markdown() {
		assert_eq!(plain_cell("**4** ✅").text, "4 ✅");
		assert!(plain_cell("**4**").emphasis);
		assert_eq!(plain_cell("`a*b`").text, "a*b");
		assert_eq!(plain_cell("[site](https://x)").text, "site");
		assert_eq!(plain_cell("a  b").text, "a b");
	}

	#[test]
	fn cell_status_reads_leading_words() {
		let cases = [
			("ok", Some(Status::Good)),
			("FAIL", Some(Status::Bad)),
			("⚠️ slow", Some(Status::Warn)),
			("⏳", Some(Status::Pending)),
			("3 ok", Some(Status::Good)),
			("12 ms", None),
			("passedx", None),
		];
		for (text, expected) in cases {
			assert_eq!(cell_status(text), expected, "{text}");
		}
	}

	#[test]
	fn parse_cell_reads_numbers() {
		let number = |text: &str| match parse_cell(text) {
			Cell::Number(number) => number,
			other => panic!("{text} read as {other:?}"),
		};
		let plain = number("12 ms");
		assert!((plain.value - 0.012).abs() < 1e-12);
		assert_eq!(plain.dim, Dimension::Duration);
		assert_eq!(plain.unit, "ms");
		assert_eq!(plain.fit, CellFit::Pure);
		let estimate = number("~1,400");
		assert_eq!(estimate.value, 1400.0);
		assert!(estimate.approx);
		assert_eq!(estimate.dim, Dimension::Count);
		let money = number("$6.0k");
		assert_eq!(money.value, 6000.0);
		assert_eq!(money.dim, Dimension::Currency);
		assert_eq!(money.unit, "$");
		let range = number("24–72 h");
		assert_eq!(range.value, 24.0);
		assert_eq!(range.dim, Dimension::Duration);
		assert_eq!(range.upper, Some(72.0 * 3600.0));
		assert_eq!(range.fit, CellFit::Range);
		let arrow = number("12 → 15");
		assert_eq!(arrow.fit, CellFit::Arrow);
		assert_eq!(arrow.to, Some(15.0));
		let fraction = number("5/8");
		assert_eq!(fraction.dim, Dimension::Fraction);
		assert_eq!(fraction.denominator, Some(8.0));
		assert!(matches!(parse_cell("03"), Cell::Other(other) if other.kind == OtherKind::Id));
		assert!(matches!(parse_cell("2024-01-02"), Cell::Other(other) if other.kind == OtherKind::Date));
		assert!(matches!(parse_cell("n/a"), Cell::Other(other) if other.kind == OtherKind::Missing));
	}

	#[test]
	fn monotonic_and_written_slack() {
		assert!(is_monotonic(&[1.0, 2.0, 3.0]));
		assert!(is_monotonic(&[3.0, 2.0, 1.0]));
		assert!(!is_monotonic(&[1.0, 1.0]));
		assert!(!is_monotonic(&[1.0, 2.0, 1.0]));
		let mut cell = base_cell("38.5%", false);
		cell.figure = "38.5%".to_string();
		cell.value = 38.5;
		cell.dim = Dimension::Percent;
		assert!((written_slack(&cell) - 0.05).abs() < 1e-12);
		let mut exact = base_cell("46", false);
		exact.figure = "46".to_string();
		exact.value = 46.0;
		assert_eq!(written_slack(&exact), 0.0);
	}

	#[test]
	fn analyze_table_reads_roles_and_totals() {
		let header = vec!["Name".to_string(), "Latency".to_string(), "Delta".to_string()];
		let rows = vec![
			vec!["a".to_string(), "12 ms".to_string(), "1 ms".to_string()],
			vec!["b".to_string(), "15 ms".to_string(), "2 ms".to_string()],
			vec!["c".to_string(), "18 ms".to_string(), "3 ms".to_string()],
			vec!["Total".to_string(), "45 ms".to_string(), "6 ms".to_string()],
		];
		let table = analyze_table(&header, &rows);
		assert_eq!(table.rows, vec![0, 1, 2]);
		assert_eq!(table.totals, vec![3]);
		assert!(table.label.is_some());
		assert_eq!(table.measures.len(), 2);
		let only = with_measures(&table, &[table.measures[0].clone()]);
		assert_eq!(only.measures.len(), 1);
		assert!(only.label.is_some());
	}
}
