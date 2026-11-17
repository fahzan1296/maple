//! Which chart a table becomes and the data it plots: the chart kind, the
//! label and measure columns, and the [`ChartSpec`] a renderer draws.
//!
//! Reference: `l/src/charts/chart-plan.ts`.
//!
//! Deviations forced by Rust:
//! - TypeScript union and optional object types become enums and `Option`
//!   fields; `Readonly<Record<number, Polarity>>` becomes `HashMap`.
//! - JavaScript lookahead regexes (`BOUND`, `QUANTILE_HEADER`,
//!   `QUANTILE_LIST`) are hand-rolled scanners; `Date.parse` in
//!   `chronological` is a small ISO-8601 parser; `toLocaleString` grouping
//!   is hand-rolled. Rust's `\b`/`\w` are Unicode-aware where JavaScript's
//!   are ASCII.

use super::table_data::{
	cell_status, is_monotonic, is_number, parse_cell, written_slack, Cell, CellKind, ColumnRole, Dimension,
	NumberCell, Status, TableAnalysis, TableColumn,
};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Which way a measure improves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Polarity {
	Higher,
	Lower,
}

/// Which way a table's alternatives run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rivals {
	Rows,
	Columns,
}

/// How a heatmap shades its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shading {
	Shared,
	Series,
	Row,
}

/// Which side of its target a value must stay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bound {
	Max,
	Min,
}

/// Chart kinds, by the table shapes they fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChartKind {
	Bar,
	Paired,
	Grouped,
	Line,
	Heatmap,
	Multiples,
	Share,
	Diverging,
	Scatter,
	Change,
	Progress,
	Stacked,
	Timeline,
	Waterfall,
	Dots,
	Range,
	Slope,
	Butterfly,
	Bullet,
	Ladder,
	Histogram,
	Stat,
}

/// What to draw from a table.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartPlan {
	pub kind: ChartKind,
	/// Column naming each category (or holding the x values of a line).
	pub label: Option<usize>,
	/// Columns joined to the label to tell rows apart.
	pub qualifiers: Option<Vec<usize>>,
	/// Naming column whose values nest the rows.
	pub group: Option<usize>,
	/// Plotted columns, in order.
	pub series: Vec<usize>,
	/// `change` over metric pairs: the baseline column of each series.
	pub baselines: Option<Vec<usize>>,
	/// Each data row is its own series.
	pub transpose: bool,
	pub shading: Option<Shading>,
	/// Rows whose `column` cell reads `value` are the reference.
	pub reference: Option<RowReference>,
	/// Rows ranked by a plotted column instead of table order.
	pub order: Option<RowOrder>,
	/// `stacked`: the column the plotted parts make up.
	pub whole: Option<usize>,
	/// `stacked`/`histogram`: each bar spans 100% of its category's total.
	pub normalize: Option<bool>,
	/// `timeline`: the column holding each segment's start.
	pub offsets: Option<usize>,
	/// `waterfall`: the row of a cap the steps build toward.
	pub limit: Option<usize>,
	/// `bullet`: the column holding each row's target.
	pub target: Option<usize>,
	/// Which way each column improves, by column index.
	pub polarity: Option<HashMap<usize, Polarity>>,
	/// Which way each row's metric improves, by row index.
	pub row_polarity: Option<HashMap<usize, Polarity>>,
	/// What competes, so each contest's best value is marked.
	pub rivals: Option<Rivals>,
	/// Rows the table is about, by row index.
	pub focus: Option<Vec<usize>>,
	/// Column whose status cells badge each row's name.
	pub status: Option<usize>,
	/// The chart's title.
	pub title: Option<String>,
}

impl ChartPlan {
	fn new(kind: ChartKind, label: Option<usize>, series: Vec<usize>, transpose: bool) -> Self {
		ChartPlan {
			kind,
			label,
			qualifiers: None,
			group: None,
			series,
			baselines: None,
			transpose,
			shading: None,
			reference: None,
			order: None,
			whole: None,
			normalize: None,
			offsets: None,
			limit: None,
			target: None,
			polarity: None,
			row_polarity: None,
			rivals: None,
			focus: None,
			status: None,
			title: None,
		}
	}
}

/// A naming cell marking reference rows.
#[derive(Clone, Debug, PartialEq)]
pub struct RowReference {
	pub column: usize,
	pub value: String,
}

/// Rows sorted by a column's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
	Descending,
	Ascending,
}

/// Rows sorted by `column`'s values, largest or smallest first.
#[derive(Clone, Debug, PartialEq)]
pub struct RowOrder {
	pub column: usize,
	pub direction: SortDirection,
}

/// One plotted value.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartPoint {
	/// Value in base units (seconds, bytes, …).
	pub value: f64,
	/// The number as the table wrote it, shown as the value label.
	pub text: String,
	/// The table bolded the cell.
	pub emphasis: bool,
	/// Upper end of a range cell.
	pub upper: Option<f64>,
	/// The spread a `± e` cell states around the value.
	pub error: Option<f64>,
}

/// A row's target in a `bullet` chart.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartTarget {
	pub value: f64,
	pub text: String,
	pub emphasis: bool,
	pub upper: Option<f64>,
	pub error: Option<f64>,
	pub bound: Option<Bound>,
}

impl ChartTarget {
	fn from_point(point: &ChartPoint, bound: Option<Bound>) -> Self {
		ChartTarget {
			value: point.value,
			text: point.text.clone(),
			emphasis: point.emphasis,
			upper: point.upper,
			error: point.error,
			bound,
		}
	}
}

/// One plotted series.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSeries {
	pub name: String,
	pub dim: Dimension,
	/// Currency symbol or rate unit the axis prints.
	pub unit: String,
	/// One per category; `None` where the cell holds no number of this dimension.
	pub points: Vec<Option<ChartPoint>>,
	/// Status words standing in for a value, one per category.
	pub notes: Vec<Option<String>>,
	pub polarity: Option<Polarity>,
	/// Category of the series' best value, marked when its rows are rivals.
	pub best: Option<usize>,
}

/// Consecutive categories sharing a leading name.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartGroup {
	pub name: String,
	pub members: Vec<String>,
}

/// Numeric x positions of a line, the x measure of a scatter, or segment starts.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartAxis {
	pub name: String,
	pub dim: Dimension,
	pub unit: String,
	pub values: Vec<f64>,
}

/// `waterfall`: the bar the steps build to.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowTotal {
	pub name: String,
	pub point: ChartPoint,
}

/// One measurement's quantiles in a `ladder` chart.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartLadder {
	pub name: String,
	pub series: Vec<usize>,
}

/// A status column of a [`ChartSpec::matrix`].
#[derive(Clone, Debug, PartialEq)]
pub struct StatusColumn {
	pub name: String,
	pub statuses: Vec<Option<Status>>,
}

/// A fact a chart could state as its title.
#[derive(Clone, Debug, PartialEq)]
pub struct Takeaway {
	pub key: String,
	pub text: String,
	pub striking: bool,
}

/// A mark a planner lays over a chart.
#[derive(Clone, Debug, PartialEq)]
pub enum ChartAnnotation {
	Reference {
		value: f64,
		label: String,
		series: Option<usize>,
	},
	Highlight {
		category: usize,
	},
	Callout {
		category: usize,
		series: Option<usize>,
		text: String,
	},
}

/// The data a chart draws, independent of how it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSpec {
	pub kind: ChartKind,
	pub categories: Vec<String>,
	pub groups: Option<Vec<ChartGroup>>,
	pub series: Vec<ChartSeries>,
	pub x: Option<ChartAxis>,
	pub caption: Option<String>,
	pub axis: String,
	pub baseline: Option<String>,
	pub rival: Option<bool>,
	pub shading: Option<Shading>,
	pub reference: Option<Vec<usize>>,
	pub whole: Option<ChartSeries>,
	pub normalized: Option<bool>,
	pub tones: Option<Vec<Option<Status>>>,
	pub total: Option<FlowTotal>,
	pub estimate: Option<ChartSeries>,
	pub targets: Option<Vec<Option<ChartTarget>>>,
	pub target_name: Option<String>,
	pub title: Option<String>,
	pub subtitle: Option<String>,
	pub transition: Option<bool>,
	pub annotations: Option<Vec<ChartAnnotation>>,
	pub transposed: Option<bool>,
	pub category_polarity: Option<Vec<Option<Polarity>>>,
	pub leaders: Option<Vec<Option<usize>>>,
	pub status: Option<Vec<Option<Status>>>,
	pub status_name: Option<String>,
	pub matrix: Option<Vec<StatusColumn>>,
	pub ladders: Option<Vec<ChartLadder>>,
}

impl ChartSpec {
	fn base(kind: ChartKind) -> Self {
		ChartSpec {
			kind,
			categories: Vec::new(),
			groups: None,
			series: Vec::new(),
			x: None,
			caption: None,
			axis: String::new(),
			baseline: None,
			rival: None,
			shading: None,
			reference: None,
			whole: None,
			normalized: None,
			tones: None,
			total: None,
			estimate: None,
			targets: None,
			target_name: None,
			title: None,
			subtitle: None,
			transition: None,
			annotations: None,
			transposed: None,
			category_polarity: None,
			leaders: None,
			status: None,
			status_name: None,
			matrix: None,
			ladders: None,
		}
	}
}

/// One quantile of a ladder.
#[derive(Clone, Debug, PartialEq)]
pub struct Rung {
	pub column: TableColumn,
	/// Which listed value of each cell; absent when the column holds the quantile alone.
	pub slot: Option<usize>,
	/// How many values each cell of a listing column holds.
	pub slots: Option<usize>,
	/// The quantile as the header writes it.
	pub name: String,
	/// Where it ranks: 0 the minimum, 50 the median, 100 the maximum.
	pub rank: f64,
	/// The unit the header gives bare numbers.
	pub scale: Option<(Dimension, f64)>,
}

/// Quantiles of one measurement per row, lowest rank first, one dimension.
#[derive(Clone, Debug, PartialEq)]
pub struct Ladder {
	/// The measurement; empty when its headers name nothing but quantiles.
	pub name: String,
	pub rungs: Vec<Rung>,
	pub dim: Dimension,
	pub unit: String,
}

const BEFORE: &str = r"\b(?:before|old|baseline|base|main|master|prev|previous|original|orig|cold|was|from|v1|stock|default|unpatched|without)\b";
const AFTER: &str = r"\b(?:after|new|patched|optimi[sz]ed|fixed|warm|now|to|v2|with|proposed|branch|pr|ours)\b";
const STRONG_BEFORE: &str = r"\b(?:before|old|baseline|base|main|master|prev|previous|original|orig|cold|v1|stock|default|unpatched)\b";
const STRONG_AFTER: &str = r"\b(?:after|new|patched|optimi[sz]ed|fixed|warm|now|v2|proposed|branch|pr|ours)\b";
const WEAK_CONDITION: &str = r"^(?:was|from|without|to|with)$";
const DELTA: &str = r"(?:Δ|delta|change|diff|improvement|speedup|reduction|savings|saved|gain|regression|% chg)";
const AGGREGATE: &str = r"^(?:total|sum|cumulative|cum\.?|running total|avg|average|mean)\b";
const ARROW: &str = r"\s*(?:→|->|⇒)\s*";
const SEQUENCE_LABEL: &str = r"^(?:(?:run|day|week|month|iter(?:ation)?|round|step|phase|epoch|v|version|n\s*=|wave|attempt|pass|batch)\s*\d+(?:\.\d+)*|q[1-4]|#\s*\d{1,2})\b";
const MONTH: &str = r"^(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\b";
const METRIC_HEADER: &str = r"^(?:metrics?|measures?|stats?|statistics?|quantit(?:y|ies))$";
const COUNT_HEADER: &str = r"^(?:count|counts|cnt|n|#|number|num|items|tally|freq|frequency)$";
const BUCKET_HEADER: &str = r"^(?:status|state|verdict|outcome|result|severity|priority|bucket|cohort|category|class|type|kind|tier|level|grade|cause|reason|disposition|resolution)$";
const RIVALS: &str = r"(?:^|[^\w])(?:models?|config(?:uration)?s?|arms?|variants?|approach(?:es)?|options?|plans?|polic(?:y|ies)|providers?|modes?|strateg(?:y|ies)|candidates?|settings?|shapes?|backends?|engines?|librar(?:y|ies)|implementations?|vendors?|tools?|methods?|algorithms?|setups?|runs?)(?![\w])";
const STEP_HEADER: &str = r"^(?:(?:ci|pipeline|build|job|run)\s+)?(?:steps?|stages?|phases?)(?:\s+name)?$";
const OFFSET_HEADER: &str = r"^(?:(?:byte|vertex|index|file|start)\s+)?(?:offset|start|begin|addr(?:ess)?)(?:\s*\(.*\))?$";
const PART_HEADER: &str = r"^(?:buckets?|(?:line\s+)?items?|components?|categor(?:y|ies)|expenses?|costs?|allocations?|use of funds|parts?|breakdown)$";
const PER_UNIT: &str = r"/|\bper\b|\bprice\b|\brate\b|\bavg\b|\baverage\b|\bmean\b|\beach\b|\bunit\b|\bmedian\b|\bp\d{2}\b";
const LIMIT_ROW: &str = r"\b(?:cap|limit|budget|ceiling|allowance|quota)\b";
const TREE_NAME: &str = r"^(?:\s|&nbsp;|\u00a0)*[└├↳┗┣╰]|^(?:&nbsp;|\u00a0){2,}";
const TOTAL_NAME: &str = r"^(?:total|overall|grand|sum)(?:\b|_)";
const WHOLE_NAME: &str = r"\b(?:total|overall|all|whole|end[- ]to[- ]end|wall(?:[- ]?clock)?|combined|sum)\b";
const VARIANT_NAME: &str = r"\b(?:before|after|old|new|baseline|current|cold|warm)\b";
const AMOUNT_HEADER: &str = r"\b(?:tokens?|lines?|loc|sloc|chars?|characters|words|bytes|calls|requests|cost|spend|budget|amount)\b";
const FOCUS_NAME: &str = r"\((?:current|ours|recommended|chosen|selected|winner|best|shipped|proposed|pick)\b[^)]*\)|(?:^|[^\w])(?:ours|recommended)(?![\w])|[✅⭐★]";
const IN_USE: &str = r"\((?:shipped|current|default|in use|deployed|prod(?:uction)?)\b[^)]*\)";
const POSITIONS: &str = r"(?:^|[^\w])(?:lines?|line ?(?:no|nos|numbers?)|cases?|ids?|rows?|cols?|columns?|offsets?|pages?|pp|ind(?:ex|ices)|positions?|range|span|issues?|#|no\.?)(?![\w])";
const STAGE_HEADER: &str = r"\b(?:(?:iter(?:ation)?|round|run|pass|step|stage|phase|attempt|v)\s*\d+|r\d+|now|after|then|final|latest|current|best|new|fixed|patched|optimi[sz]ed|today)\b";
const ROW_BEFORE: &str = r"\b(?:before|old|baseline|original|previous|prev|stock|unpatched|default|main|master|current|cold)\b";
const ROW_AFTER: &str = r"\b(?:after|new|patched|fixed|optimi[sz]ed|proposed|pr|branch|head|ours|warm)\b";
const IDENTIFIER: &str = r"^`|^[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+$";
const SWITCH: &str = r"^(?:on|off|enabled|disabled|with|without|yes|no)\b";
const SWITCHED_OFF: &str = r"^(?:off|disabled|without|no)\b";
const ORDER_HEADER: &str = r"^(?:runs?|steps?|stages?|phases?|rounds?|iter(?:ation)?s?|epochs?|turns?|attempts?|waves?|versions?|releases?|builds?|commits?|frames?|days?|weeks?|months?|years?|quarters?|dates?|time)\b";
const TARGET_WORD: &str = r"^(?:target|budget|limit|sla|slo|goal)$";
const CEILING_WORD: &str = r"^(?:budget|limit|cap|ceiling|quota|allowance)$";
const FLOOR_SIGN: &str = r"^(?:≥|>=|>|at least|no less than)$";
const SECOND_TARGET: &str = r"\d[^/]*\/\s*(?:≤|<=|<|≥|>=|>)?\s*[~≈]?\s*[$€£¥]?\s*\.?\d";
const DESCRIPTION: &str = r" [—–] | - |: | \(";
const NAME_JOIN: &str = " · ";
const NEST_SEPARATORS: [&str; 9] = [" — ", " – ", ": ", " · ", ", ", " ", "/", "-", "_"];

static BEFORE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){BEFORE}")).expect("valid BEFORE regex"));
static AFTER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){AFTER}")).expect("valid AFTER regex"));
static STRONG_BEFORE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){STRONG_BEFORE}")).expect("valid STRONG_BEFORE regex"));
static STRONG_AFTER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){STRONG_AFTER}")).expect("valid STRONG_AFTER regex"));
static WEAK_CONDITION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){WEAK_CONDITION}")).expect("valid WEAK_CONDITION regex"));
static DELTA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){DELTA}")).expect("valid DELTA regex"));
static AGGREGATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){AGGREGATE}")).expect("valid AGGREGATE regex"));
static ARROW_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(ARROW).expect("valid ARROW regex"));
static SEQUENCE_LABEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){SEQUENCE_LABEL}")).expect("valid SEQUENCE_LABEL regex"));
static MONTH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){MONTH}")).expect("valid MONTH regex"));
static METRIC_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){METRIC_HEADER}")).expect("valid METRIC_HEADER regex"));
static COUNT_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){COUNT_HEADER}")).expect("valid COUNT_HEADER regex"));
static BUCKET_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){BUCKET_HEADER}")).expect("valid BUCKET_HEADER regex"));
static RIVALS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){RIVALS}")).expect("valid RIVALS regex"));
static STEP_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){STEP_HEADER}")).expect("valid STEP_HEADER regex"));
static OFFSET_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){OFFSET_HEADER}")).expect("valid OFFSET_HEADER regex"));
static PART_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){PART_HEADER}")).expect("valid PART_HEADER regex"));
static PER_UNIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){PER_UNIT}")).expect("valid PER_UNIT regex"));
static LIMIT_ROW_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){LIMIT_ROW}")).expect("valid LIMIT_ROW regex"));
static TREE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){TREE_NAME}")).expect("valid TREE_NAME regex"));
static TOTAL_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){TOTAL_NAME}")).expect("valid TOTAL_NAME regex"));
static WHOLE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){WHOLE_NAME}")).expect("valid WHOLE_NAME regex"));
static VARIANT_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){VARIANT_NAME}")).expect("valid VARIANT_NAME regex"));
static AMOUNT_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){AMOUNT_HEADER}")).expect("valid AMOUNT_HEADER regex"));
static FOCUS_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){FOCUS_NAME}")).expect("valid FOCUS_NAME regex"));
static IN_USE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){IN_USE}")).expect("valid IN_USE regex"));
static POSITIONS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){POSITIONS}")).expect("valid POSITIONS regex"));
static STAGE_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){STAGE_HEADER}")).expect("valid STAGE_HEADER regex"));
static ROW_BEFORE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){ROW_BEFORE}")).expect("valid ROW_BEFORE regex"));
static ROW_AFTER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){ROW_AFTER}")).expect("valid ROW_AFTER regex"));
static IDENTIFIER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(IDENTIFIER).expect("valid IDENTIFIER regex"));
static SWITCH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){SWITCH}")).expect("valid SWITCH regex"));
static SWITCHED_OFF_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){SWITCHED_OFF}")).expect("valid SWITCHED_OFF regex"));
static ORDER_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){ORDER_HEADER}")).expect("valid ORDER_HEADER regex"));
static TARGET_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){TARGET_WORD}")).expect("valid TARGET_WORD regex"));
static CEILING_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){CEILING_WORD}")).expect("valid CEILING_WORD regex"));
static FLOOR_SIGN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("(?i){FLOOR_SIGN}")).expect("valid FLOOR_SIGN regex"));
static SECOND_TARGET_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(SECOND_TARGET).expect("valid SECOND_TARGET regex"));
static DESCRIPTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(DESCRIPTION).expect("valid DESCRIPTION regex"));
static TARGET_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?iu)^(?:[§\p{L}][\p{L}\p{N}§.-]*\s+)?(?<word>target|budget|limit|cap|ceiling|threshold|sla|slo|goal|quota|allowance|bar)(?:\s+[\p{L}\p{N}]+)?(?:\s*\([^)]*\))?$")
		.expect("valid TARGET_HEADER regex")
});
static STATED_REFERENCE_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)\b(?<lead>vs\.?|versus|budget|limit|cap|target|quota|threshold)\s+(?:of\s+)?(?<after>[~≈]?\s?[$€£]?\d[\d,]*(?:\.\d+)?\s?(?:[a-z%×µμ]+(?:\/[a-z]+)?)?)|(?<before>[$€£]?\d[\d,]*(?:\.\d+)?\s?[a-z%×]{0,3})\s+(?:[a-z]+\s+)?(?<word>budget|limit|cap|quota|threshold|target)\b")
		.expect("valid STATED_REFERENCE regex")
});
static QUANTILE_ITEM_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)^(?:p\d{1,4}(?:\.\d+)?|median|med|min|minimum|max|maximum)$").expect("valid QUANTILE_ITEM regex")
});
static QUANTILE_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)(?:^|[^\w.])(?<word>p\d{1,4}(?:\.\d+)?|median|med|min|minimum|max|maximum)(?![\w.])")
		.expect("valid QUANTILE_HEADER regex")
});
static QUANTILE_LIST_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)(?:^|[^\w.])(?<list>(?:p\d{1,4}(?:\.\d+)?|median|med|min|minimum|max|maximum|mean|avg|average)(?:\s*\/\s*(?:p\d{1,4}(?:\.\d+)?|median|med|min|minimum|max|maximum|mean|avg|average)){1,3})(?![\w.])")
		.expect("valid QUANTILE_LIST regex")
});
static REFERENCE_ROW_RE: LazyLock<Regex> = LazyLock::new(|| {
	let words = REFERENCE_WORDS.join("|");
	Regex::new(&format!(r"(?i)^(?<lead>{words})\b|\((?<marked>{words})\)")).expect("valid REFERENCE_ROW regex")
});
/// A bound's sign, without Rust's unsupported lookahead; the number it must lead is checked by hand.
static BOUND_SIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"(?i)(≤|<=|<|≥|>=|>|\bat most\b|\bat least\b|\bunder\b|\bbelow\b|\bup to\b|\bno more than\b|\bno less than\b)")
		.expect("valid BOUND_SIGN regex")
});
static NUMBER_START_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^\s*[~≈]?\s*[$€£¥]?\s*\.?\d").expect("valid NUMBER_START regex"));
static OPPOSED_RE: LazyLock<Vec<(Regex, Regex)>> = LazyLock::new(|| {
	vec![
		(Regex::new(r"(?i)\b(?:removed|removals?|deletions?|deleted|dels)\b").unwrap(), Regex::new(r"(?i)\b(?:added|additions?|insertions?|inserted|adds)\b").unwrap()),
		(Regex::new(r"(?i)\b(?:loss(?:es)?|lost)\b").unwrap(), Regex::new(r"(?i)\b(?:wins?|won|gain(?:s|ed)?)\b").unwrap()),
		(Regex::new(r"(?i)\b(?:closed|deleted)\b").unwrap(), Regex::new(r"(?i)\b(?:opened|created)\b").unwrap()),
		(Regex::new(r"(?i)\b(?:outflows?|debits?|sells?|downvotes?)\b").unwrap(), Regex::new(r"(?i)\b(?:inflows?|credits?|buys?|upvotes?)\b").unwrap()),
	]
});
static DOT_DIMS: LazyLock<HashSet<Dimension>> = LazyLock::new(|| {
	[Dimension::Duration, Dimension::Bytes, Dimension::Currency, Dimension::Rate, Dimension::Ratio]
		.into_iter()
		.collect()
});
static MATRIX_KINDS: LazyLock<HashSet<ChartKind>> = LazyLock::new(|| {
	[ChartKind::Bar, ChartKind::Grouped, ChartKind::Paired, ChartKind::Progress, ChartKind::Diverging]
		.into_iter()
		.collect()
});

/// Distinct series colors: the most segments a stacked bar tells apart.
const SERIES_COLORS: usize = 6;
/// Most panels a small-multiples grid stays legible with.
pub const MAX_PANELS: usize = 6;
/// Most lines one line chart keeps apart.
pub const MAX_LINES: usize = 6;
const MIN_LINE_ROWS: usize = 4;
const MIN_SCATTER_ROWS: usize = 5;
const MAX_GROUPED: usize = 4;
const DECADES: f64 = 100.0;
const ROW_SPREAD: f64 = 8.0;
const QUANTITY_SPREAD: f64 = 30.0;
const ROW_CONTRAST: f64 = 1.5;
/// Words naming a reference row, strongest first.
const REFERENCE_WORDS: [&str; 10] = [
	"baseline", "control", "reference", "before", "original", "stock", "default", "main", "master", "current",
];
const MAX_TRACKS: usize = 3;
const NOTABLE_FACTOR: f64 = 1.5;
const MAX_NAME_PART: usize = 40;
const MAX_LABEL: usize = 28;
const MIN_NEST_HEAD: usize = 3;
const NEST_SHARE: f64 = 0.6;
const PROSE_CELL: usize = 40;
const MAX_PROSE_COUNT: f64 = 5.0;
const MAX_SHARE_PARTS: usize = 6;
/// Most segments one share bar keeps legible.
pub const MAX_SHARE_SEGMENTS: usize = 10;
const APPROX_SUM: f64 = 0.02;
const TONED_SHARE: f64 = 2.0 / 3.0;
const MAX_STEPS: usize = 12;
const MAX_SEGMENTS: usize = 40;
const MOST_OF_RUN: f64 = 0.9;
const RANKED_RISES: usize = 8;
const CONTIGUOUS: f64 = 0.75;
const BOUND_SHARE: f64 = 0.5;
const CLOSE_SHARE: f64 = 0.9;
const MAX_LADDERS: usize = 3;
const ORDERED_SHARE: f64 = 0.8;
const MIN_BINS: usize = 3;
const MAX_BINS: usize = 24;
const MAX_DISTRIBUTIONS: usize = 4;
const SHARE_SLACK: f64 = 3.0;
const RANGE_SHARE: f64 = 0.5;
const MAX_RANGES: usize = 4;
const DOT_SPREAD: f64 = 10.0;
const MAX_DOT_ROWS: usize = 20;
const DOT_CLUMP: f64 = 0.2;
const REFERENCE_REACH: f64 = 50.0;
const MIX_SPREAD: f64 = 1.5;
const MAX_MATRIX: usize = 4;
const VISIBLE_MOVE: f64 = 0.25;
const MAX_RIVALS: usize = 16;
const MAX_TAKEAWAYS: usize = 8;
const TAKEAWAY_SERIES: usize = 4;
const OUTLIER: f64 = 2.0;
const STRIKING: f64 = 3.0;
const STRIKING_MOVE: f64 = 2.0;
const DOMINANT_PART: f64 = 0.6;
const DOMINANT_PAIR: f64 = 0.75;
const SECOND_PART: f64 = 0.15;
const STILL_FACTOR: f64 = 1.03;
const CONSENSUS_WEIGHT: f64 = 20.0;
const RIVALRY_WEIGHT: f64 = 4.0;

/// Opposing amount words: the losing side first.
static OPPOSED: LazyLock<Vec<(Regex, Regex)>> = LazyLock::new(|| {
	vec![
		(
			Regex::new(r"\b(?:loss(?:es)?|lost)\b").expect("valid regex"),
			Regex::new(r"\b(?:wins?|won|gain(?:s|ed)?)\b").expect("valid regex"),
		),
		(
			Regex::new(r"\b(?:closed|deleted)\b").expect("valid regex"),
			Regex::new(r"\b(?:opened|created)\b").expect("valid regex"),
		),
		(
			Regex::new(r"\b(?:outflows?|debits?|sells?|downvotes?)\b").expect("valid regex"),
			Regex::new(r"\b(?:inflows?|credits?|buys?|upvotes?)\b").expect("valid regex"),
		),
	]
});

/// Dimensions of amounts that can stand back to back: tallies, sizes, money and time.
static OPPOSED_DIMS: LazyLock<HashSet<Dimension>> = LazyLock::new(|| {
	[Dimension::Count, Dimension::Bytes, Dimension::Currency, Dimension::Duration]
		.into_iter()
		.collect()
});
const MAX_STAGES: usize = 4;
const MIN_PEAK_RISE: f64 = 1.25;
const PEAK_RISE: f64 = 1.5;
const MIN_PEAK_SAMPLES: usize = 4;
const MAJORITY: f64 = 0.5;
const SHIFT_POINTS: f64 = 10.0;
const MAX_STATS: usize = 4;
const WIDE_MOVES: f64 = 12.0;

/// The values of `column` over `rows` that read as numbers.
fn column_values(column: &TableColumn, rows: &[usize]) -> Vec<f64> {
	let mut values = Vec::new();
	for &row in rows {
		if let Some(cell) = column.cells.get(row) {
			if is_number(cell) {
				values.push(cell.as_number().expect("number cell").value);
			}
		}
	}
	values
}

/// A numeric cell as a plotted point labelled with its written figure.
fn point(cell: &NumberCell) -> ChartPoint {
	ChartPoint {
		value: cell.value,
		text: cell.figure.clone(),
		emphasis: cell.emphasis,
		upper: cell.upper,
		error: cell.error,
	}
}

/// A naming cell's text; a blank or a dash names nothing.
fn name_cell(column: &TableColumn, row: usize) -> String {
	let Some(cell) = column.cells.get(row) else {
		return String::new();
	};
	if cell.kind() == CellKind::Missing && !cell.text().chars().any(|c| c.is_alphanumeric() || c.is_numeric()) {
		return String::new();
	}
	cell.text().to_string()
}

/// Whether `dim` is one whose rows add up along one axis.
fn flow_dim(dim: Dimension) -> bool {
	matches!(dim, Dimension::Count | Dimension::Duration | Dimension::Bytes | Dimension::Currency)
}

fn is_word_dot(c: char) -> bool {
	c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// Match one quantile word at byte `at`, longest first, with word boundaries.
fn quantile_word_at(text: &str, at: usize) -> Option<(String, usize)> {
	let rest = &text[at..];
	let lower = rest.to_ascii_lowercase();
	let mut best: Option<(String, usize)> = None;
	if lower.starts_with('p') {
		let bytes = lower.as_bytes();
		let mut end = 1;
		while end < bytes.len() && end < 5 && bytes[end].is_ascii_digit() {
			end += 1;
		}
		if end > 1 {
			if end < bytes.len() && bytes[end] == b'.' {
				let mut frac = end + 1;
				while frac < bytes.len() && bytes[frac].is_ascii_digit() {
					frac += 1;
				}
				if frac > end + 1 {
					end = frac;
				}
			}
			best = Some((rest[..end].to_string(), at + end));
		}
	}
	for word in ["minimum", "maximum", "median", "average", "med", "min", "max", "avg", "mean"] {
		if lower.starts_with(word) {
			let end = at + word.len();
			if best.as_ref().map_or(true, |(_, best_end)| end > *best_end) {
				best = Some((rest[..word.len()].to_string(), end));
			}
			break;
		}
	}
	let (word, end) = best?;
	let before_ok = at == 0 || !text[..at].chars().next_back().map_or(false, is_word_dot);
	let after_ok = text[end..].chars().next().map_or(true, |c| !is_word_dot(c));
	if before_ok && after_ok {
		Some((word, end))
	} else {
		None
	}
}

/// Every quantile word a header names, whole and case-insensitively.
fn quantile_headers(header: &str) -> Vec<String> {
	let mut found = Vec::new();
	let mut at = 0;
	while at < header.len() {
		if let Some((word, end)) = quantile_word_at(header, at) {
			found.push(word);
			at = end;
		} else {
			at += header[at..].chars().next().map_or(1, char::len_utf8);
		}
	}
	found
}

/// A header's slash list of two to four quantiles or means, lowest first, and its byte span.
fn quantile_list(header: &str) -> Option<(Vec<String>, usize, usize)> {
	let mut at = 0;
	while at < header.len() {
		let before_ok = at == 0 || !header[..at].chars().next_back().map_or(false, is_word_dot);
		if before_ok {
			if let Some((first, first_end)) = quantile_word_at(header, at) {
				let mut words = vec![first];
				let mut end = first_end;
				while words.len() < 4 {
					let mut cursor = end;
					while cursor < header.len() && header[cursor..].starts_with(' ') {
						cursor += 1;
					}
					if cursor >= header.len() || !header[cursor..].starts_with('/') {
						break;
					}
					let mut slash = cursor + 1;
					while slash < header.len() && header[slash..].starts_with(' ') {
						slash += 1;
					}
					let Some((word, word_end)) = quantile_word_at(header, slash) else { break };
					words.push(word);
					end = word_end;
				}
				if words.len() >= 2 {
					return Some((words, at, end));
				}
			}
		}
		at += header[at..].chars().next().map_or(1, char::len_utf8);
	}
	None
}

/// A bound written before a number, wherever it starts: `(start, end, sign)`.
fn bound_match(text: &str) -> Option<(usize, usize, String)> {
	let captures = BOUND_SIGN_RE.captures(text)?;
	let sign = captures.get(0)?;
	let after = &text[sign.end()..];
	let spaces = after.len() - after.trim_start().len();
	if !NUMBER_START_RE.is_match(after) {
		return None;
	}
	Some((sign.start(), sign.end() + spaces, sign.as_str().to_string()))
}

fn is_integer(value: f64) -> bool {
	value.is_finite() && value.fract() == 0.0
}

/// `value` with exactly `decimals` fraction digits, grouped in thousands when asked.
fn locale_fixed(value: f64, decimals: usize, group: bool) -> String {
	let mut text = format!("{:.*}", decimals, value);
	if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.') {
		text = text[1..].to_string();
	}
	let (sign, body) = if let Some(stripped) = text.strip_prefix('-') {
		("-", stripped)
	} else {
		("", text.as_str())
	};
	let (int_part, frac_part) = match body.split_once('.') {
		Some((int_part, frac_part)) => (int_part, Some(frac_part)),
		None => (body, None),
	};
	let int_part = if group { group_digits(int_part) } else { int_part.to_string() };
	let mut out = String::new();
	out.push_str(sign);
	out.push_str(&int_part);
	if let Some(frac_part) = frac_part {
		out.push('.');
		out.push_str(frac_part);
	}
	out
}

pub fn group_digits(int_part: &str) -> String {
	let mut out = String::new();
	let digits: Vec<char> = int_part.chars().collect();
	for (at, digit) in digits.iter().enumerate() {
		if at > 0 && (digits.len() - at) % 3 == 0 {
			out.push(',');
		}
		out.push(*digit);
	}
	out
}

/// The plural of a row noun.
fn plural(noun: &str) -> String {
	if noun.ends_with('s') || noun.ends_with('x') || noun.ends_with("ch") || noun.ends_with("sh") {
		format!("{noun}es")
	} else if noun.ends_with('y') && !noun[..noun.len() - 1].ends_with(['a', 'e', 'i', 'o', 'u']) {
		format!("{}ies", &noun[..noun.len() - 1])
	} else {
		format!("{noun}s")
	}
}

/// `a`, `a and b`, `a, b and c`.
fn list(names: &[String]) -> String {
	if names.len() == 1 {
		names[0].clone()
	} else {
		format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1])
	}
}

/// A share in percent as a chart writes it.
pub fn percent_text(percent: f64) -> String {
	if percent <= 0.0 || percent >= 100.0 {
		return format!("{}%", percent.clamp(0.0, 100.0).round());
	}
	if percent < 0.05 {
		return "<0.1%".to_string();
	}
	if percent >= 99.95 {
		return ">99.9%".to_string();
	}
	if percent < 1.0 || percent > 99.0 {
		format!("{:.1}%", percent)
	} else {
		format!("{}%", percent.round())
	}
}

/// `48`, `2.1`: whole from 10, one decimal below.
fn round_factor(factor: f64) -> String {
	if factor >= 10.0 {
		group_digits(&format!("{}", factor.round() as i64))
	} else {
		let rounded = (factor * 10.0).round() / 10.0;
		if rounded.fract() == 0.0 {
			format!("{}", rounded as i64)
		} else {
			format!("{rounded:.1}")
		}
	}
}

/// A factor as people say it.
fn factor_words(factor: f64) -> String {
	if factor == 0.0 {
		return "−100%".to_string();
	}
	if factor >= 2.0 {
		return format!("{}× more", round_factor(factor));
	}
	if factor <= 0.5 {
		return format!("{}× less", round_factor(1.0 / factor));
	}
	let percent = ((factor - 1.0) * 100.0).round();
	if percent == 0.0 {
		"same".to_string()
	} else if percent > 0.0 {
		format!("+{percent}%")
	} else {
		format!("−{}%", percent.abs())
	}
}

/// The middle of a value written `a–b`.
pub fn interval_middle(point: &ChartPoint) -> f64 {
	match point.upper {
		None => point.value,
		Some(upper) => {
			if point.value > 0.0 {
				(point.value * upper).sqrt()
			} else {
				(point.value + upper) / 2.0
			}
		}
	}
}

/// Largest over smallest of `values`; 1 when any is zero or negative.
fn spread_of(values: &[f64]) -> f64 {
	if values.len() < 2 || values.iter().any(|&value| value <= 0.0) {
		return 1.0;
	}
	values.iter().copied().fold(f64::MIN, f64::max) / values.iter().copied().fold(f64::MAX, f64::min)
}

/// Largest over smallest of the values (range ends included) in `columns` over `rows`.
fn value_spread(columns: &[TableColumn], rows: &[usize]) -> f64 {
	let mut values = Vec::new();
	for column in columns {
		for &row in rows {
			if let Some(cell) = column.cells.get(row) {
				if let Some(number) = cell.as_number() {
					if Some(number.dim) == column.dim {
						values.push(number.value);
						values.push(number.upper.unwrap_or(number.value));
					}
				}
			}
		}
	}
	spread_of(&values)
}

/// The widest typical ratio between two columns of `grid`.
fn pair_spread(grid: &[Vec<f64>]) -> f64 {
	let width = grid.first().map_or(0, |row| row.len());
	let mut widest: f64 = 1.0;
	for a in 0..width {
		for b in (a + 1)..width {
			let mut ratios: Vec<f64> = grid
				.iter()
				.filter_map(|values| {
					let x = values.get(a)?;
					let y = values.get(b)?;
					if x.is_finite() && y.is_finite() && *x != 0.0 && *y != 0.0 {
						Some(x / y)
					} else {
						None
					}
				})
				.collect();
			if ratios.len() < 2 {
				continue;
			}
			ratios.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
			let middle = ratios[ratios.len() / 2];
			widest = widest.max(middle).max(1.0 / middle);
		}
	}
	widest
}

/// How dots would sit for `grid`: `apart`, `clumped`, or `None`.
fn dot_layout(grid: &[Vec<f64>]) -> Option<&'static str> {
	if pair_spread(grid) >= QUANTITY_SPREAD {
		return None;
	}
	let values: Vec<f64> = grid.iter().flatten().copied().filter(|v| v.is_finite()).collect();
	if values.len() < 2 {
		return None;
	}
	let max = values.iter().copied().fold(f64::MIN, f64::max);
	let min = values.iter().copied().fold(f64::MAX, f64::min);
	let whole = (max / min).ln();
	let mut spans: Vec<f64> = grid
		.iter()
		.map(|row| row.iter().copied().filter(|v| v.is_finite()).collect::<Vec<_>>())
		.filter(|row| row.len() >= 2)
		.map(|row| {
			let max = row.iter().copied().fold(f64::MIN, f64::max);
			let min = row.iter().copied().fold(f64::MAX, f64::min);
			(max / min).ln()
		})
		.collect();
	spans.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	let typical = spans.get(spans.len() / 2).copied().unwrap_or(0.0);
	if whole > 0.0 && typical < DOT_CLUMP * whole {
		Some("clumped")
	} else {
		Some("apart")
	}
}

/// Present points of `points`, largest first.
fn by_value(points: &[Option<ChartPoint>]) -> Vec<(usize, ChartPoint)> {
	let mut ranked: Vec<(usize, ChartPoint)> = points
		.iter()
		.enumerate()
		.filter_map(|(at, point)| point.clone().map(|point| (at, point)))
		.collect();
	ranked.sort_by(|a, b| b.1.value.partial_cmp(&a.1.value).unwrap_or(std::cmp::Ordering::Equal));
	ranked
}

/// What a sentence calls one category.
fn row_noun(spec: &ChartSpec) -> String {
	let axis = &spec.axis;
	let chars: Vec<char> = axis.chars().collect();
	if chars.is_empty() || chars.len() > 16 || !chars.iter().all(|c| c.is_ascii_alphabetic()) {
		return "row".to_string();
	}
	let lower = axis.to_lowercase();
	if lower.len() >= 2 && lower.ends_with('s') && !lower[..lower.len() - 1].ends_with('s') {
		lower[..lower.len() - 1].to_string()
	} else {
		lower
	}
}

fn words(header: &str) -> Vec<String> {
	header
		.to_lowercase()
		.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
		.filter(|word| !word.is_empty())
		.map(str::to_string)
		.collect()
}

/// Whether `text` holds `word` as a whole word.
fn has_word(text: &str, word: &str) -> bool {
	let lower = text.to_lowercase();
	let mut from = 0;
	while let Some(at) = lower[from..].find(word) {
		let start = from + at;
		let end = start + word.len();
		let before_ok = lower[..start].chars().next_back().map_or(true, |c| !c.is_ascii_alphanumeric() && c != '_');
		let after_ok = lower[end..].chars().next().map_or(true, |c| !c.is_ascii_alphanumeric() && c != '_');
		if before_ok && after_ok {
			return true;
		}
		from = start + word.len();
	}
	false
}

/// The direction of a condition pair: `first` names the before state.
pub fn is_transition(first: &str, second: &str) -> bool {
	let before = BEFORE_RE.is_match(first) && !BEFORE_RE.is_match(second);
	let after = AFTER_RE.is_match(second) && !AFTER_RE.is_match(first);
	if !before && !after {
		return false;
	}
	if (before && STRONG_BEFORE_RE.is_match(first)) || (after && STRONG_AFTER_RE.is_match(second)) {
		return true;
	}
	let subject = |header: &str| {
		header
			.to_lowercase()
			.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
			.filter(|word| !word.is_empty() && !WEAK_CONDITION_RE.is_match(word))
			.map(str::to_string)
			.collect::<Vec<_>>()
	};
	let words = subject(second);
	subject(first).iter().any(|word| words.contains(word))
}

/// Whether two headers name one quantity under two conditions.
pub fn is_condition_pair(first: &str, second: &str) -> bool {
	let a = words(first);
	let b = words(second);
	if a.join(" ") == b.join(" ") {
		return false;
	}
	if is_transition(first, second) {
		return true;
	}
	let shared = a.iter().filter(|word| b.contains(word)).count();
	shared > 0 && shared < a.len().max(b.len())
}

/// What a chart calls each of its rows.
struct RowNames {
	names: Vec<String>,
	axis: String,
	groups: Option<Vec<ChartGroup>>,
}

/// Names of `rows` from the `namers` cells, in table order.
fn name_rows(namers: &[TableColumn], group: Option<&TableColumn>, rows: &[usize], nest: bool) -> RowNames {
	let join = |columns: &[TableColumn], row: usize| -> String {
		columns
			.iter()
			.map(|column| name_cell(column, row))
			.filter(|name| !name.is_empty())
			.collect::<Vec<_>>()
			.join(NAME_JOIN)
	};
	let full: Vec<String> = rows
		.iter()
		.enumerate()
		.map(|(at, &row)| join(namers, row).is_empty().then(|| format!("#{}", at + 1)).unwrap_or_else(|| join(namers, row)))
		.collect();
	let axis = namers
		.iter()
		.map(|column| column.header.clone())
		.filter(|header| !header.is_empty())
		.collect::<Vec<_>>()
		.join(NAME_JOIN);
	let rest: Vec<TableColumn> = namers
		.iter()
		.filter(|column| group.map_or(true, |group| column.index != group.index))
		.cloned()
		.collect();
	if group.is_none() || rest.is_empty() {
		let nested = if nest { nest_names(&full) } else { None };
		return match nested {
			Some(groups) => RowNames { names: full, axis, groups: Some(groups) },
			None => RowNames { names: tidy_names(&full), axis, groups: None },
		};
	}
	let group = group.expect("group column");
	let mut groups: Vec<ChartGroup> = Vec::new();
	for &row in rows {
		let name = name_cell(group, row);
		let member = join(&rest, row);
		if groups.last().map_or(false, |last| last.name == name) {
			groups.last_mut().expect("last group").members.push(member);
		} else {
			groups.push(ChartGroup { name, members: vec![member] });
		}
	}
	if groups.len() == rows.len() {
		return RowNames { names: tidy_names(&full), axis, groups: None };
	}
	let heads = tidy_names(&groups.iter().map(|group| group.name.clone()).collect::<Vec<_>>());
	RowNames {
		names: full,
		axis,
		groups: Some(
			groups
				.into_iter()
				.enumerate()
				.map(|(at, group)| ChartGroup { name: heads[at].clone(), members: tidy_names(&group.members) })
				.collect(),
		),
	}
}

/// Row names nested under a leading part they share, when that reads as an outline.
fn nest_names(names: &[String]) -> Option<Vec<ChartGroup>> {
	if names.len() < 3 {
		return None;
	}
	let mut best: Option<(Vec<ChartGroup>, usize)> = None;
	for separator in NEST_SEPARATORS {
		let mut runs: Vec<(Option<String>, Vec<String>, Vec<String>)> = Vec::new();
		let mut seen: HashSet<String> = HashSet::new();
		let mut valid = true;
		for name in names {
			let at = name.find(separator);
			let split = match at {
				Some(at) if at >= MIN_NEST_HEAD => name[at + separator.len()..].trim().to_string(),
				_ => String::new(),
			};
			let torn = at.map_or(false, |at| name[..at].ends_with([',', ';', ':', '(']))
				|| split.chars().next().map_or(false, |c| c.is_ascii_digit());
			let head = if !split.is_empty() && !torn { at.map(|at| name[..at].to_string()) } else { None };
			let member = if head.is_some() { split.clone() } else { String::new() };
			let nested_run = head.is_some()
				&& !member.is_empty()
				&& runs.last().map_or(false, |(last_head, _, _)| *last_head == head);
			if nested_run {
				let (_, members, run_names) = runs.last_mut().expect("last run");
				members.push(member);
				run_names.push(name.clone());
				continue;
			}
			if head.is_some() && !member.is_empty() {
				if seen.contains(head.as_ref().expect("head")) {
					valid = false;
					break;
				}
				seen.insert(head.clone().expect("head"));
			}
			runs.push((if member.is_empty() { None } else { head }, vec![member], vec![name.clone()]));
		}
		if !valid {
			continue;
		}
		let mut groups: Vec<ChartGroup> = Vec::new();
		let mut nested = 0;
		let mut named = 0;
		for (head, members, run_names) in runs {
			if head.is_some() && members.len() >= 2 {
				groups.push(ChartGroup { name: head.expect("head"), members: tidy_names(&members) });
				nested += members.len();
				named += 1;
				continue;
			}
			let unnamed = run_names;
			if groups.last().map_or(false, |last| last.name.is_empty()) {
				groups.last_mut().expect("last group").members.extend(unnamed);
			} else {
				groups.push(ChartGroup { name: String::new(), members: unnamed });
			}
		}
		if named == 0
			|| (nested as f64) < names.len() as f64 * NEST_SHARE
			|| (named == 1 && nested == names.len())
		{
			continue;
		}
		if best.as_ref().map_or(true, |(_, best_nested)| nested > *best_nested) {
			best = Some((groups, nested));
		}
	}
	best.map(|(groups, _)| groups)
}

fn description_index(name: &str) -> Option<usize> {
	DESCRIPTION_RE.find(name).map(|found| found.start())
}

/// The longest start (or end) every name shares, ending at a separator.
fn shared_edge(names: &[String], edge: &str) -> String {
	let read = |name: &str| -> Vec<char> {
		if edge == "start" {
			name.chars().collect()
		} else {
			name.chars().rev().collect()
		}
	};
	let first = read(&names[0]);
	let mut length = first.len();
	for name in names {
		let other = read(name);
		let mut at = 0;
		while at < length && at < other.len() && other[at] == first[at] {
			at += 1;
		}
		length = at;
	}
	while length > 0 && !['/', ':', '_', '-'].contains(&first[length - 1]) {
		length -= 1;
	}
	let shared: String = first[..length].iter().collect();
	if edge == "start" {
		shared
	} else {
		shared.chars().rev().collect()
	}
}

/// Shorter names that still tell `names` apart, for labels.
fn tidy_names(names: &[String]) -> Vec<String> {
	let usable = |list: &[String]| -> bool {
		let numbers_only = list
			.iter()
			.all(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_digit() || " \t.,:/+-–".contains(c)));
		list.iter().all(|name| name.chars().count() >= 2)
			&& list.iter().collect::<HashSet<_>>().len() == list.len()
			&& !numbers_only
	};
	let mut tidy = names.to_vec();
	if tidy.len() < 2 || !usable(&tidy) {
		return tidy;
	}
	let path_like = |name: &str| -> bool {
		name.contains('/')
			&& !name.contains(' ')
			&& name
				.match_indices('/')
				.all(|(at, _)| at > 0 && at + 1 < name.len())
	};
	if tidy.iter().all(|name| path_like(name)) {
		let tails: Vec<String> = tidy
			.iter()
			.map(|name| name.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string())
			.collect();
		if usable(&tails) {
			tidy = tails;
		}
	}
	if tidy.len() >= 3 && tidy.iter().all(|name| !name.contains(' ')) {
		let prefix = shared_edge(&tidy, "start");
		let trimmed: Vec<String> = tidy
			.iter()
			.map(|name| name[prefix.len()..].trim_start_matches([' ', ',', ';', ':', '·', '/', '_', '-']).to_string())
			.collect();
		if prefix.chars().count() >= 3 && usable(&trimmed) {
			tidy = trimmed;
		}
		let suffix = shared_edge(&tidy, "end");
		let cut: Vec<String> = tidy
			.iter()
			.map(|name| {
				let end = name.len() - suffix.len();
				name[..end].trim_end_matches([' ', ',', ';', ':', '·', '/', '_', '-']).to_string()
			})
			.collect();
		if suffix.chars().count() >= 3 && usable(&cut) {
			tidy = cut;
		}
	}
	let long: Vec<&String> = tidy.iter().filter(|name| name.chars().count() > MAX_LABEL).collect();
	let described = long
		.iter()
		.filter(|name| description_index(name).map_or(false, |at| at >= 2))
		.count();
	if described * 2 <= long.len() {
		return tidy;
	}
	let short: Vec<String> = tidy
		.iter()
		.map(|name| match description_index(name) {
			Some(at) if at >= 2 => name[..at].trim_end_matches([' ', ',', ';', ':', '·', '—', '–', '-']).to_string(),
			_ => name.clone(),
		})
		.collect();
	if usable(&short) {
		short
	} else {
		tidy
	}
}

/// Parse a date or time as an ordering key; `None` when unreadable.
fn parse_date(text: &str) -> Option<f64> {
	let text = text.trim();
	let digits: Vec<u32> = text
		.chars()
		.take_while(|c| c.is_ascii_digit())
		.filter_map(|c| c.to_digit(10))
		.collect();
	if digits.len() == 4 {
		let year: u32 = text[..4].parse().ok()?;
		let rest = &text[4..];
		let (month, day, time) = if let Some(rest) = rest.strip_prefix('-') {
			let month: u32 = rest.get(0..2)?.parse().ok()?;
			let day: u32 = rest.get(3..5)?.parse().ok()?;
			(month, day, rest.get(6..).unwrap_or(""))
		} else if let Some(rest) = rest.strip_prefix('/') {
			let month: u32 = rest.get(0..2)?.parse().ok()?;
			let day: u32 = rest.get(3..5)?.parse().ok()?;
			(month, day, rest.get(6..).unwrap_or(""))
		} else {
			(1, 1, "")
		};
		let mut seconds = days_from_civil(year, month, day) as f64 * 86_400.0;
		if let Some(time) = time.strip_prefix('T').or_else(|| time.strip_prefix(' ')) {
			let time = time.trim_end_matches('Z');
			let mut parts = time.split(':');
			let hour: f64 = parts.next()?.parse().ok()?;
			let minute: f64 = parts.next().unwrap_or("0").parse().ok()?;
			let second: f64 = parts.next().unwrap_or("0").parse().ok()?;
			seconds += hour * 3600.0 + minute * 60.0 + second;
		}
		return Some(seconds);
	}
	let _ = digits;
	None
}

fn days_from_civil(year: u32, month: u32, day: u32) -> i64 {
	let y = if month <= 2 { year as i64 - 1 } else { year as i64 };
	let era = if y >= 0 { y } else { y - 399 } / 400;
	let yoe = y - era * 400;
	let mp = (month as i64 + 9) % 12;
	let doy = (153 * mp + 2) / 5 + day as i64 - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	era * 146_097 + doe - 719_468
}

/// `rows` in time order when `label` holds dates.
fn chronological(rows: &[usize], label: Option<&TableColumn>) -> Vec<usize> {
	let Some(label) = label else { return rows.to_vec() };
	if label.role != super::table_data::ColumnRole::Temporal {
		return rows.to_vec();
	}
	let mut times: Vec<(usize, f64)> = Vec::new();
	for &row in rows {
		let Some(cell) = label.cells.get(row) else { return rows.to_vec() };
		match parse_date(cell.text()) {
			Some(time) => times.push((row, time)),
			None => return rows.to_vec(),
		}
	}
	times.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
	times.into_iter().map(|(row, _)| row).collect()
}

/// `rows` with each `group` value gathered into one run, runs in order of first appearance.
fn regroup(rows: &[usize], group: Option<&TableColumn>) -> Vec<usize> {
	let Some(group) = group else { return rows.to_vec() };
	let mut order: HashMap<String, usize> = HashMap::new();
	for &row in rows {
		let name = name_cell(group, row);
		let next = order.len();
		order.entry(name).or_insert(next);
	}
	let mut sorted = rows.to_vec();
	sorted.sort_by_key(|&row| order.get(&name_cell(group, row)).copied().unwrap_or(usize::MAX));
	sorted
}

/// Columns that may join `label` in naming rows.
pub fn naming_columns(table: &TableAnalysis, label: Option<&TableColumn>) -> Vec<TableColumn> {
	let rows = &table.rows;
	table
		.columns
		.iter()
		.filter(|column| {
			label.map_or(true, |label| column.index != label.index)
				&& (column.role == super::table_data::ColumnRole::Label
					|| column.role == super::table_data::ColumnRole::Temporal)
				&& rows.iter().all(|&row| name_cell(column, row).chars().count() <= MAX_NAME_PART)
				&& rows
					.iter()
					.map(|&row| name_cell(column, row))
					.collect::<HashSet<_>>()
					.len()
					> 1
		})
		.cloned()
		.collect()
}

/// The local guess at the columns naming rows beside `label`.
pub fn guess_naming(
	table: &TableAnalysis,
	label: Option<&TableColumn>,
	spare: Option<&[TableColumn]>,
) -> (Vec<usize>, Option<usize>) {
	let spare_owned;
	let spare = match spare {
		Some(spare) => spare,
		None => {
			spare_owned = naming_columns(table, label);
			spare_owned.as_slice()
		}
	};
	let rows = &table.rows;
	let Some(label) = label else { return (Vec::new(), None) };
	let clashes = |columns: &[TableColumn]| -> usize {
		rows
			.iter()
			.map(|&row| {
				columns
					.iter()
					.map(|column| name_cell(column, row))
					.collect::<Vec<_>>()
					.join("\0")
			})
			.collect::<HashSet<_>>()
			.len();
		rows.len() - rows
			.iter()
			.map(|&row| {
				columns
					.iter()
					.map(|column| name_cell(column, row))
					.collect::<Vec<_>>()
					.join("\0")
			})
			.collect::<HashSet<_>>()
			.len()
	};
	let mut namers = vec![label.clone()];
	let mut clash = clashes(&namers);
	while clash > 0 {
		let mut candidates: Vec<(TableColumn, usize, usize)> = spare
			.iter()
			.filter(|column| !namers.iter().any(|namer| namer.index == column.index))
			.map(|column| {
				let mut extended = namers.clone();
				extended.push(column.clone());
				let distinct = rows
					.iter()
					.map(|&row| name_cell(column, row))
					.collect::<HashSet<_>>()
					.len();
				(column.clone(), clashes(&extended), distinct)
			})
			.filter(|(_, candidate_clash, _)| *candidate_clash < clash)
			.collect();
		candidates.sort_by_key(|(_, candidate_clash, distinct)| (*candidate_clash, *distinct));
		let Some((next, _, _)) = candidates.first().cloned() else { break };
		namers.push(next);
		clash = clashes(&namers);
	}
	let qualifiers: Vec<usize> = namers[1..].iter().map(|column| column.index).collect();
	if qualifiers.is_empty() {
		return (qualifiers, None);
	}
	namers.sort_by_key(|column| column.index);
	let group = namers
		.iter()
		.find(|column| in_runs(&rows.iter().map(|&row| name_cell(column, row)).collect::<Vec<_>>()))
		.map(|column| column.index);
	(qualifiers, group)
}

/// Each distinct value in one consecutive run, one run at least two long.
fn in_runs(values: &[String]) -> bool {
	let mut seen: HashSet<&str> = HashSet::new();
	let mut repeats = false;
	for (at, value) in values.iter().enumerate() {
		if at > 0 && value == &values[at - 1] {
			repeats = true;
		} else if seen.contains(value.as_str()) {
			return false;
		} else {
			seen.insert(value.as_str());
		}
	}
	repeats
}

/// The local guess at the reference rows.
fn guess_reference(
	table: &TableAnalysis,
	label: Option<&TableColumn>,
	qualifiers: &[usize],
) -> Option<RowReference> {
	let label = label?;
	let mut columns: Vec<&TableColumn> = qualifiers
		.iter()
		.rev()
		.filter_map(|index| table.columns.get(*index))
		.collect();
	columns.push(label);
	for column in columns {
		let mut best: Option<(String, usize)> = None;
		let mut tied = false;
		let mut seen: HashSet<String> = HashSet::new();
		let mut values: Vec<String> = Vec::new();
		for &row in &table.rows {
			let value = name_cell(column, row);
			if seen.insert(value.clone()) {
				values.push(value);
			}
		}
		for value in values {
			let Some(captures) = REFERENCE_ROW_RE.captures(&value) else { continue };
			let name = captures
				.name("lead")
				.or_else(|| captures.name("marked"))
				.map(|found| found.as_str().to_lowercase())?;
			let Some(rank) = REFERENCE_WORDS.iter().position(|word| *word == name) else {
				continue;
			};
			if best.as_ref().map_or(false, |(_, best_rank)| rank == *best_rank) {
				tied = true;
			}
			if best.as_ref().map_or(true, |(_, best_rank)| rank < *best_rank) {
				best = Some((value, rank));
				tied = false;
			}
		}
		let Some((value, _)) = best else { continue };
		if tied {
			continue;
		}
		let marked: Vec<usize> = table
			.rows
			.iter()
			.copied()
			.filter(|&row| name_cell(column, row) == value)
			.collect();
		let owners: HashSet<String> = marked.iter().map(|&row| name_cell(label, row)).collect();
		let ok = if column.index == label.index {
			marked.len() == 1
		} else {
			owners.len() == marked.len() && owners.len() >= 2
		};
		if ok {
			return Some(RowReference { column: column.index, value });
		}
	}
	None
}

/// How the label column orders the rows.
fn axis_order(table: &TableAnalysis, label: Option<&TableColumn>) -> Option<&'static str> {
	let label = label?;
	if label.role == super::table_data::ColumnRole::Temporal {
		return Some("temporal");
	}
	if label.role == super::table_data::ColumnRole::Sequence {
		return is_monotonic(&column_values(label, &table.rows)).then_some("sequence");
	}
	let names: Vec<String> = table.rows.iter().map(|&row| label.cells[row].text().to_string()).collect();
	let mostly = |pattern: &Regex| -> bool {
		names.iter().filter(|name| pattern.is_match(name)).count() as f64 >= names.len() as f64 * 0.8
	};
	if mostly(&SEQUENCE_LABEL_RE) {
		return Some("sequence");
	}
	if mostly(&MONTH_RE) {
		return Some("temporal");
	}
	let values = column_values(label, &table.rows);
	if values.len() == names.len() && values.len() >= 3 && is_monotonic(&values) {
		return Some("numeric");
	}
	None
}

/// Whether `table`'s rows lie along an ordered axis.
fn ordered_rows(table: &TableAnalysis, label: Option<&TableColumn>) -> bool {
	if axis_order(table, label).is_some() {
		return true;
	}
	if let Some(label) = label {
		if ORDER_HEADER_RE.is_match(&label.header) {
			return true;
		}
	}
	table.columns.iter().any(|column| {
		if let Some(label) = label {
			if column.index >= label.index {
				return false;
			}
		}
		if column.role != super::table_data::ColumnRole::Measure
			&& column.role != super::table_data::ColumnRole::Sequence
		{
			return false;
		}
		let values = column_values(column, &table.rows);
		values.len() == table.rows.len() && is_monotonic(&values) && values[1] > values[0]
	})
}

/// A takeaway with the weight ranking it among a chart's others.
#[derive(Clone)]
struct Fact {
	key: String,
	text: String,
	striking: bool,
	weight: f64,
}

/// The local guess at what `plan`'s rows and measures mean.
struct Meaning {
	polarity: Option<HashMap<usize, Polarity>>,
	row_polarity: Option<HashMap<usize, Polarity>>,
	rivals: Option<Rivals>,
	focus: Option<Vec<usize>>,
	status: Option<usize>,
}

/// The local best guess at a chart for `table`, or `None` when no chart fits.
pub fn plan_chart(table: &TableAnalysis) -> Option<ChartPlan> {
	let guessed = guess_shape(table);
	let drawn = guessed.as_ref().and_then(|shape| build_chart(table, shape));
	let shape = match drawn.as_ref() {
		Some(spec) if worth_charting(spec) => guessed.clone(),
		_ => stat_shape(table, guessed.as_ref()).or(guessed.clone()),
	};
	let shape = shape?;
	let meaning = guess_meaning(table, &shape);
	let mut plan = ChartPlan {
		polarity: meaning.polarity,
		row_polarity: meaning.row_polarity,
		rivals: meaning.rivals,
		focus: meaning.focus,
		status: meaning.status,
		..shape
	};
	if let Some(lead) = table_takeaways(table, &plan, &[plan.rivals], "drawn")
		.into_iter()
		.find(|fact| fact.striking)
	{
		plan.title = Some(lead.text);
	}
	Some(plan)
}

/// Whether a model should pick `table`'s chart.
pub fn should_judge(table: &TableAnalysis, guess: Option<&ChartPlan>) -> bool {
	let Some(guess) = guess else { return false };
	if table.rows.len() < 3 {
		return false;
	}
	if table.measures.len() >= 2 || !leading_measures(table, guess).is_empty() {
		return true;
	}
	build_chart(table, guess).map_or(false, |spec| worth_charting(&spec))
}

/// Measure columns before `guess`'s text label column.
pub fn leading_measures(table: &TableAnalysis, guess: &ChartPlan) -> Vec<TableColumn> {
	let Some(label) = guess.label.and_then(|index| table.columns.get(index)) else {
		return Vec::new();
	};
	if label.role != super::table_data::ColumnRole::Label {
		return Vec::new();
	}
	table
		.measures
		.iter()
		.filter(|column| column.index < label.index)
		.cloned()
		.collect()
}

/// Whether ranking a chart's drawn measures side by side adds facts its own leave out.
fn side_by_side(spec: &ChartSpec) -> bool {
	if matches!(spec.kind, ChartKind::Scatter | ChartKind::Line | ChartKind::Stacked) {
		return true;
	}
	matches!(spec.kind, ChartKind::Range) && spec.transition != Some(true)
}

/// What a chart draws, by name.
pub fn drawn_names(spec: &ChartSpec) -> HashSet<String> {
	let mut names: HashSet<String> = spec.series.iter().map(|entry| entry.name.clone()).collect();
	for name in [spec.x.as_ref().map(|x| &x.name), spec.whole.as_ref().map(|w| &w.name), spec.baseline.as_ref()] {
		if let Some(name) = name {
			if !name.is_empty() && *name != spec.axis {
				names.insert(name.clone());
			}
		}
	}
	names
}

/// Facts about `table` a chart could state as its headline under `plan`'s meaning.
pub fn table_takeaways(
	table: &TableAnalysis,
	plan: &ChartPlan,
	contests: &[Option<Rivals>],
	scope: &str,
) -> Vec<Takeaway> {
	let measures: Vec<TableColumn> = table
		.measures
		.iter()
		.filter(|column| Some(column.index) != plan.label)
		.cloned()
		.collect();
	let wide = !matches!(
		plan.kind,
		ChartKind::Change | ChartKind::Slope | ChartKind::Bullet | ChartKind::Ladder | ChartKind::Histogram
	) && !plan.transpose;
	let mut facts: Vec<Fact> = Vec::new();
	for &rivals in contests {
		let mut with_rivals = plan.clone();
		with_rivals.rivals = rivals;
		let drawn = build_chart(table, &with_rivals);
		if let Some(drawn) = &drawn {
			facts.extend(chart_facts(drawn));
		}
		let mut beside: Vec<TableColumn> = Vec::new();
		if scope == "table" {
			if measures.len() > plan.series.len() {
				beside = measures.clone();
			}
		} else if let Some(drawn) = &drawn {
			if side_by_side(drawn) {
				let names = drawn_names(drawn);
				beside = measures
					.iter()
					.filter(|column| names.contains(&column.header))
					.cloned()
					.collect();
			}
		}
		let contest = if scope == "drawn" && drawn.as_ref().map_or(false, |d| d.kind == ChartKind::Stacked) {
			None
		} else {
			rivals
		};
		let side = if wide && beside.len() >= 2 {
			let mut side_plan = plan.clone();
			side_plan.rivals = contest;
			side_plan.kind = ChartKind::Grouped;
			side_plan.series = beside.iter().map(|column| column.index).collect();
			build_chart(table, &side_plan)
		} else {
			None
		};
		if let Some(side) = &side {
			facts.extend(chart_facts(side));
		}
	}
	rank_facts(facts)
}

/// A measure's polarity from the unit its values are written in.
fn dim_polarity(dim: Option<Dimension>, unit: &str, change: bool, scores: bool) -> Option<Polarity> {
	if scores {
		return Some(Polarity::Higher);
	}
	if dim == Some(Dimension::Rate) && unit.ends_with("/s") {
		return Some(Polarity::Higher);
	}
	if dim == Some(Dimension::Duration) {
		return Some(Polarity::Lower);
	}
	if matches!(dim, Some(Dimension::Currency) | Some(Dimension::Bytes)) && change {
		return Some(Polarity::Lower);
	}
	None
}

/// Whether `plan`'s rows may each be a metric of their own.
pub fn rows_are_metrics(table: &TableAnalysis, plan: &ChartPlan) -> bool {
	plan.kind == ChartKind::Change
		|| plan.transpose
		|| plan
			.series
			.iter()
			.any(|index| table.columns.get(*index).map_or(false, |column| column.mixed))
}

/// Rows the table singles out.
fn guess_focus(table: &TableAnalysis, label: Option<&TableColumn>) -> Vec<usize> {
	let rows = &table.rows;
	let Some(label) = label else { return Vec::new() };
	if rows.len() < 2 {
		return Vec::new();
	}
	let bolded: Vec<usize> = rows
		.iter()
		.copied()
		.filter(|&row| {
			if label.cells[row].emphasis() {
				return true;
			}
			let numbers: Vec<&NumberCell> = table.measures.iter().filter_map(|c| c.cells[row].as_number()).collect();
			numbers.len() >= 2 && numbers.iter().filter(|cell| cell.emphasis).count() * 2 > numbers.len()
		})
		.collect();
	if !bolded.is_empty() && bolded.len() * 3 <= rows.len() + 1 {
		return bolded;
	}
	let named: Vec<usize> = rows
		.iter()
		.copied()
		.filter(|&row| FOCUS_NAME_RE.is_match(label.cells[row].text()))
		.collect();
	if !named.is_empty() && named.len() * 3 <= rows.len() + 1 {
		named
	} else {
		Vec::new()
	}
}

/// A text or measure column whose cells lead with statuses that are not all alike.
fn guess_status(table: &TableAnalysis, plan: &ChartPlan) -> Option<usize> {
	let mut naming: HashSet<usize> = HashSet::new();
	if let Some(label) = plan.label {
		naming.insert(label);
	}
	if let Some(qualifiers) = &plan.qualifiers {
		for index in qualifiers {
			naming.insert(*index);
		}
	}
	table.columns.iter().find_map(|column| {
		if naming.contains(&column.index) {
			return None;
		}
		let statuses = column.statuses.as_ref()?;
		let mut seen: HashSet<Status> = HashSet::new();
		for &row in &table.rows {
			if let Some(status) = statuses.get(row).copied().flatten() {
				seen.insert(status);
			}
		}
		(seen.len() >= 2).then_some(column.index)
	})
}

/// The local guess at what `plan`'s rows and measures mean.
pub fn guess_meaning(table: &TableAnalysis, plan: &ChartPlan) -> Meaning {
	let label = plan.label.and_then(|index| table.columns.get(index));
	let change = plan.kind == ChartKind::Change
		|| plan.kind == ChartKind::Stat
		|| table.columns.iter().any(|column| BEFORE_RE.is_match(&column.header));
	let mut polarity: HashMap<usize, Polarity> = HashMap::new();
	for column in &table.columns {
		if label.map_or(false, |label| label.index == column.index)
			|| column.role != super::table_data::ColumnRole::Measure
			|| DELTA_RE.is_match(&column.header)
			|| column.signed_share >= 0.5
		{
			continue;
		}
		if let Some(guessed) = dim_polarity(column.dim, &column.unit, change, column.scores) {
			polarity.insert(column.index, guessed);
		}
	}
	let mut row_polarity: HashMap<usize, Polarity> = HashMap::new();
	if let Some(label) = label {
		if rows_are_metrics(table, plan) {
			for row in table.rows.iter().chain(table.totals.iter()).copied() {
				let first = plan
					.series
					.iter()
					.filter_map(|index| table.columns.get(*index))
					.filter_map(|column| column.cells.get(row))
					.find(|cell| is_number(cell))
					.and_then(|cell| cell.as_number());
				if let Some(first) = first {
					if let Some(guessed) = dim_polarity(Some(first.dim), &first.unit, change, false) {
						row_polarity.insert(row, guessed);
					}
				}
			}
		}
	}
	Meaning {
		polarity: Some(polarity),
		row_polarity: Some(row_polarity),
		rivals: if label.map_or(false, |label| RIVALS_RE.is_match(&label.header)) {
			Some(Rivals::Rows)
		} else {
			None
		},
		focus: Some(guess_focus(table, label)),
		status: guess_status(table, plan),
	}
}

/// The amount column a share column restates, when it is an amount that builds up.
fn amount_behind(table: &TableAnalysis, share: &TableColumn) -> Option<TableColumn> {
	if share.dim != Some(Dimension::Percent) || share.scores {
		return None;
	}
	table
		.columns
		.iter()
		.find(|column| {
			column.restates == Some(share.index)
				&& column.dim.map_or(false, flow_dim)
		})
		.cloned()
}

/// Whether `column`'s counts split one total among its rows.
fn splits_total(table: &TableAnalysis, label: Option<&TableColumn>, column: &TableColumn) -> bool {
	let rows = &table.rows;
	if !label.map_or(false, |label| label.role == super::table_data::ColumnRole::Label)
		|| column.dim != Some(Dimension::Count)
		|| !column.unit.is_empty()
		|| column.scores
	{
		return false;
	}
	if rows.len() < 3 || rows.len() > MAX_SHARE_PARTS {
		return false;
	}
	let values = column_values(column, rows);
	if values.len() != rows.len() || values.iter().any(|value| *value < 0.0 || !is_integer(*value)) {
		return false;
	}
	if values.iter().filter(|value| **value > 0.0).count() < 2 {
		return false;
	}
	let sum: f64 = values.iter().sum();
	let approx = rows.iter().any(|&row| {
		column.cells[row].as_number().map_or(false, |cell| cell.approx)
	});
	let totalled = table.totals.iter().any(|&row| {
		column.cells[row].as_number().map_or(false, |cell| {
			(cell.value - sum).abs() <= if approx || cell.approx { APPROX_SUM * sum } else { 0.5 }
		})
	});
	let label_header = &label.expect("label role checked").header;
	totalled
		|| ((label_header.is_empty() || BUCKET_HEADER_RE.is_match(label_header))
			&& (column.header.is_empty() || COUNT_HEADER_RE.is_match(&column.header)))
}

/// Same-unit count or percent measures whose headers each lead with an outcome.
pub fn outcome_columns(measures: &[TableColumn]) -> Option<Vec<TableColumn>> {
	let first = measures.iter().find(|column| cell_status(&column.header).is_some())?;
	if !matches!(first.dim, Some(Dimension::Count) | Some(Dimension::Percent)) || first.scores {
		return None;
	}
	let parts: Vec<TableColumn> = measures
		.iter()
		.filter(|column| {
			column.dim == first.dim
				&& column.unit == first.unit
				&& !column.scores
				&& column.signed_share == 0.0
				&& cell_status(&column.header).is_some()
		})
		.cloned()
		.collect();
	if parts.len() >= 2 && outcome_tones(&parts.iter().map(|column| column.header.clone()).collect::<Vec<_>>()).is_some() {
		Some(parts)
	} else {
		None
	}
}

/// The outcome each of `names` leads with, when enough of them do.
pub fn outcome_tones(names: &[String]) -> Option<Vec<Option<Status>>> {
	let tones: Vec<Option<Status>> = names.iter().map(|name| cell_status(name)).collect();
	let said = tones.iter().filter(|tone| tone.is_some()).count();
	if said < 2 || (said as f64) < TONED_SHARE * tones.len() as f64 {
		return None;
	}
	let has_good = tones.iter().any(|tone| *tone == Some(Status::Good));
	let mixed = tones.iter().any(|tone| *tone != Some(Status::Good));
	if has_good && mixed {
		Some(tones)
	} else {
		None
	}
}

/// Rows read as one run of steps.
#[derive(Clone, Debug, PartialEq)]
struct Flow {
	kind: ChartKind,
	column: TableColumn,
	offsets: Option<usize>,
	limit: Option<usize>,
}

/// The cell of `column` at `row` as one plain number of the column's dimension.
fn flow_cell<'a>(column: &'a TableColumn, row: usize) -> Option<&'a NumberCell> {
	let cell = column.cells.get(row)?.as_number()?;
	if Some(cell.dim) == column.dim && cell.upper.is_none() && cell.to.is_none() {
		Some(cell)
	} else {
		None
	}
}

/// `cells` sum to `total` within the precision they were written in.
fn adds_up(cells: &[&NumberCell], total: &NumberCell) -> bool {
	let sum: f64 = cells.iter().map(|cell| cell.value).sum();
	let slack: f64 = cells.iter().map(|cell| written_slack(cell)).sum::<f64>() + written_slack(total);
	(sum - total.value).abs() <= slack + 1e-9 * total.value.abs()
}

/// `offsets` places each row of `size` on one axis.
fn laid_out(offsets: &TableColumn, size: &TableColumn, rows: &[usize]) -> bool {
	if offsets.dim != size.dim {
		return false;
	}
	let starts: Vec<Option<&NumberCell>> = rows.iter().map(|&row| flow_cell(offsets, row)).collect();
	let sizes: Vec<Option<&NumberCell>> = rows.iter().map(|&row| flow_cell(size, row)).collect();
	if starts.iter().any(|cell| cell.is_none()) || sizes.iter().any(|cell| cell.is_none() || cell.unwrap().value < 0.0) {
		return false;
	}
	let mut contiguous = 0.0;
	for at in 1..rows.len() {
		let start = starts[at].unwrap();
		let previous_start = starts[at - 1].unwrap();
		let previous_size = sizes[at - 1].unwrap();
		let end = previous_start.value + previous_size.value;
		let slack = written_slack(start) + written_slack(previous_size) + written_slack(previous_start);
		if start.value < previous_start.value {
			return false;
		}
		if (start.value - end).abs() <= slack {
			contiguous += 1.0;
		}
	}
	contiguous >= CONTIGUOUS * (rows.len() - 1) as f64
}

/// `running` totals `step` row by row, each row's value its end.
fn ends_of(running: &TableColumn, step: &TableColumn, rows: &[usize]) -> bool {
	let mut sum: f64 = 0.0;
	for &row in rows {
		let Some(total) = flow_cell(running, row) else { return false };
		let Some(by) = flow_cell(step, row) else { return false };
		sum += by.value;
		if (total.value - sum).abs() > written_slack(total) + 1e-9 * sum.abs() {
			return false;
		}
	}
	true
}

/// Rows over more than one level of parts.
fn nested(cells: &[&NumberCell], names: &[String]) -> bool {
	if names.iter().any(|name| TREE_NAME_RE.is_match(name)) {
		return true;
	}
	for (at, parent) in cells.iter().enumerate() {
		if !WHOLE_NAME_RE.is_match(names.get(at).map_or("", String::as_str)) {
			continue;
		}
		for direction in [1isize, -1] {
			let mut run: Vec<&NumberCell> = Vec::new();
			let mut next = at as isize + direction;
			while next >= 0 && next < cells.len() as isize {
				run.push(cells[next as usize]);
				if run.len() >= 2 && adds_up(&run, parent) {
					return true;
				}
				next += direction;
			}
		}
	}
	false
}

/// `column` holds amounts that add up along one axis.
fn builds_up(column: &TableColumn) -> bool {
	match column.dim {
		Some(dim) if flow_dim(dim) => dim != Dimension::Count || AMOUNT_HEADER_RE.is_match(&column.header),
		_ => false,
	}
}

/// Rows that lie end to end on one axis, read from the values and headers.
fn guess_flow(table: &TableAnalysis, label: Option<&TableColumn>, measures: &[TableColumn]) -> Option<Flow> {
	let rows = &table.rows;
	let Some(label) = label else { return None };
	if label.role != super::table_data::ColumnRole::Label || rows.len() < 3 {
		return None;
	}
	if rows.len() <= MAX_SEGMENTS {
		for offsets in &table.columns {
			if offsets.index == label.index || !OFFSET_HEADER_RE.is_match(&offsets.header) {
				continue;
			}
			let size = measures
				.iter()
				.find(|column| column.index != offsets.index && laid_out(offsets, column, rows));
			if let Some(size) = size {
				return Some(Flow {
					kind: ChartKind::Timeline,
					column: size.clone(),
					offsets: Some(offsets.index),
					limit: None,
				});
			}
		}
	}
	let mut steps: Vec<&TableColumn> = measures
		.iter()
		.filter(|column| {
			!measures
				.iter()
				.any(|other| other.index != column.index && ends_of(column, other, rows))
		})
		.collect();
	let led = steps.len() > 1;
	if led {
		let lead = steps[0];
		let rest = &steps[1..];
		let shared = rows.len() <= MAX_SHARE_PARTS && rest.iter().any(|other| other.dim == Some(Dimension::Percent));
		steps = if !shared && builds_up(lead) && lead.dim != Some(Dimension::Duration) && !rest.iter().any(|other| builds_up(other)) {
			vec![lead]
		} else {
			Vec::new()
		};
	}
	if steps.len() != 1 {
		return None;
	}
	let column = steps[0].clone();
	let running = steps.len() < measures.len()
		|| table
			.columns
			.iter()
			.any(|other| other.restates == Some(column.index) && ends_of(other, &column, rows));
	if column.dim.map_or(true, |dim| !flow_dim(dim)) || column.scores || column.mixed || PER_UNIT_RE.is_match(&column.header) {
		return None;
	}
	let cells: Vec<Option<&NumberCell>> = rows.iter().map(|&row| flow_cell(&column, row)).collect();
	if cells.iter().any(|cell| cell.is_none()) {
		return None;
	}
	let values: Vec<f64> = cells.iter().map(|cell| cell.unwrap().value).collect();
	let negative = values.iter().any(|value| *value < 0.0);
	let total_row = table.totals.iter().find(|&&row| flow_cell(&column, row).is_some()).copied();
	let sums = total_row.is_some() && adds_up(&cells.iter().map(|cell| cell.unwrap()).collect::<Vec<_>>(), flow_cell(&column, total_row.unwrap()).unwrap());
	if (total_row.is_some() || led) && !sums {
		return None;
	}
	let last = *rows.last().expect("rows checked");
	let capped = column.dim != Some(Dimension::Duration)
		&& !negative
		&& !sums
		&& rows.len() >= 4
		&& LIMIT_ROW_RE.is_match(label.cells[last].text())
		&& values[values.len() - 1] >= values[..values.len() - 1]
			.iter()
			.copied()
			.fold(f64::MIN, f64::max);
	let run: Vec<&NumberCell> = if capped { cells[..cells.len() - 1].iter().map(|cell| cell.unwrap()).collect() } else { cells.iter().map(|cell| cell.unwrap()).collect() };
	if run.len() > MAX_STEPS {
		return None;
	}
	let sizes: Vec<f64> = run.iter().map(|cell| cell.value.abs()).collect();
	let names: Vec<String> = rows.iter().map(|&row| label.cells[row].text().to_string()).collect();
	if sizes.iter().copied().fold(f64::MIN, f64::max) > MOST_OF_RUN * sizes.iter().sum::<f64>()
		|| names.iter().any(|name| TOTAL_NAME_RE.is_match(name))
		|| (!sums && nested(&run, &names))
	{
		return None;
	}
	if column.dim == Some(Dimension::Duration) {
		let variants = names.iter().filter(|name| VARIANT_NAME_RE.is_match(name)).count();
	if negative || variants >= 2 || WHOLE_NAME_RE.is_match(&column.header) {
			return None;
		}
		return if STEP_HEADER_RE.is_match(&label.header) || sums || running {
			Some(Flow { kind: ChartKind::Timeline, column, offsets: None, limit: None })
		} else {
			None
		};
	}
	if !builds_up(&column) {
		return None;
	}
	let rises = values
		.iter()
		.enumerate()
		.filter(|(at, value)| *at > 0 && **value > values[at - 1])
		.count();
	let parts = !negative && PART_HEADER_RE.is_match(&label.header) && rises * RANKED_RISES > values.len() - 1;
	if !(sums || capped || running || parts) {
		return None;
	}
	Some(Flow {
		kind: ChartKind::Waterfall,
		column,
		offsets: None,
		limit: capped.then_some(last),
	})
}

/// A measured column and the column of targets it is held to.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetColumns {
	pub measured: TableColumn,
	pub target: TableColumn,
}

/// The column of targets `table` holds its measure to, read from headers and cells.
pub fn find_target(table: &TableAnalysis) -> Option<TargetColumns> {
	let rows: Vec<usize> = table.rows.iter().chain(table.totals.iter()).copied().collect();
	let bounded = |column: &TableColumn, written: &[usize]| -> bool {
		written
			.iter()
			.filter(|row| bound_match(&column.cells[**row].text()).is_some())
			.count() as f64
			>= BOUND_SHARE * written.len() as f64
	};
	for target in &table.columns {
		let Some(captures) = TARGET_HEADER_RE.captures(&target.header) else { continue };
		let Some(word) = captures.name("word").map(|found| found.as_str().to_string()) else {
			continue;
		};
		if Some(target.index) == table.label.as_ref().map(|label| label.index) || target.index == 0 {
			continue;
		}
		let written: Vec<usize> = rows
			.iter()
			.copied()
			.filter(|&row| target.cells[row].kind() != CellKind::Missing)
			.collect();
		let stated = written
			.iter()
			.filter(|&&row| target_cell(&target.cells[row].text()).is_some())
			.count();
		if written.len() < 2 || stated * 2 < written.len() {
			continue;
		}
		if !(TARGET_WORD_RE.is_match(&word) || bounded(target, &written)) {
			continue;
		}
		let distance = |column: &TableColumn| -> usize {
			if column.index < target.index {
				target.index - column.index
			} else {
				table.columns.len() + column.index
			}
		};
		let mut candidates: Vec<&TableColumn> = table
			.columns
			.iter()
			.filter(|column| {
				column.index != target.index
					&& Some(column.index) != table.label.as_ref().map(|label| label.index)
					&& (column.role == super::table_data::ColumnRole::Measure
						|| column.role == super::table_data::ColumnRole::Label)
					&& !TARGET_HEADER_RE.is_match(&column.header)
					&& !bounded(column, &written)
			})
			.collect();
		candidates.sort_by_key(|column| distance(column));
		for measured in candidates {
			let mut both = 0;
			let mut alike = 0;
			for &row in &rows {
				let value = measured_cell(measured.cells.get(row));
				let goal = target_cell(&target.cells[row].text());
				let (Some(value), Some(goal)) = (value, goal) else { continue };
				both += 1;
				if comparable(&value, &goal.0) {
					alike += 1;
				}
			}
			if alike >= 2 && alike * 2 >= rows.len() && alike * 2 >= both {
				return Some(TargetColumns { measured: measured.clone(), target: target.clone() });
			}
		}
	}
	None
}

/// Whether a measured cell compares with its target.
fn comparable(measured: &NumberCell, target: &NumberCell) -> bool {
	let unit = (measured.dim != Dimension::Currency && measured.dim != Dimension::Rate)
		|| measured.unit == target.unit;
	measured.dim == target.dim && unit && (!measured.signed || target.signed)
}

/// A measured cell's value: its number, a range, or a rising slash list.
fn measured_cell(cell: Option<&Cell>) -> Option<NumberCell> {
	let cell = cell?.as_number()?;
	if cell.dim == Dimension::Fraction {
		return None;
	}
	let values = cell.values.as_ref()?;
	let high = values.last()?;
	if *high >= values[0] {
		let mut cell = cell.clone();
		cell.upper = Some(*high);
		cell.fit = super::table_data::CellFit::Range;
		Some(cell)
	} else {
		None
	}
}

/// A target cell's number and bound.
fn target_cell(written: &str) -> Option<(NumberCell, Option<Bound>, String)> {
	let bound = bound_match(written);
	let rest = match &bound {
		Some((_, end, _)) => &written[*end..],
		None => written,
	};
	if SECOND_TARGET_RE.is_match(rest) {
		return None;
	}
	let parsed = parse_cell(rest);
	let number = parsed.as_number()?.clone();
	if number.dim == Dimension::Fraction || number.to.is_some() {
		return None;
	}
	let Some((_, _, sign)) = bound else {
		return Some((number.clone(), None, number.figure.clone()));
	};
	let floor = FLOOR_SIGN_RE.is_match(&sign);
	let bound = if floor { Bound::Min } else { Bound::Max };
	Some((
		number.clone(),
		Some(bound),
		format!("{} {}", if floor { "≥" } else { "≤" }, number.figure),
	))
}

/// How `point` stands against `target`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
	pub met: bool,
	pub close: bool,
	pub share: Option<f64>,
}

/// How `point` stands against `target`.
pub fn target_standing(point: &ChartPoint, target: &ChartTarget) -> Option<Standing> {
	let high = point.upper.unwrap_or(point.value);
	let scale = target.upper.unwrap_or(target.value);
	if let Some(upper) = target.upper {
		let met = point.value >= target.value && high <= upper;
		return Some(Standing { met, close: false, share: if scale > 0.0 { Some(high / scale) } else { None } });
	}
	let bound = target.bound?;
	let worst = if bound == Bound::Max { high } else { point.value };
	let share = if target.value > 0.0 { Some(worst / target.value) } else { None };
	let met = if bound == Bound::Max { worst <= target.value } else { worst >= target.value };
	let close = met
		&& share.map_or(false, |share| {
			if bound == Bound::Max {
				share >= CLOSE_SHARE
			} else {
				share <= 1.0 / CLOSE_SHARE
			}
		});
	Some(Standing { met, close, share })
}

/// Whether the table's prose carries it.
pub fn prose_led(table: &TableAnalysis, columns: &[TableColumn]) -> bool {
	let rows = &table.rows;
	let mut prose = 0;
	for column in &table.columns {
		if column.role != super::table_data::ColumnRole::Label
			|| Some(column.index) == table.label.as_ref().map(|label| label.index)
		{
			continue;
		}
		let mut lengths: Vec<usize> = rows.iter().map(|&row| column.cells[row].text().chars().count()).collect();
		lengths.sort();
		prose += lengths.get(lengths.len() / 2).copied().unwrap_or(0);
	}
	prose >= PROSE_CELL
		&& columns.iter().all(|column| {
			column_values(column, rows)
				.iter()
				.all(|value| is_integer(*value) && value.abs() <= MAX_PROSE_COUNT)
		})
}

/// The local guess at a chart's kind, label and series for `table`.
fn guess_shape(table: &TableAnalysis) -> Option<ChartPlan> {
	let rows = &table.rows;
	let mut label = table.label.clone();
	if let Some(held) = find_target(table) {
		let naming = guess_naming(table, label.as_ref(), None);
		return Some(ChartPlan {
			kind: ChartKind::Bullet,
			label: label.as_ref().map(|label| label.index),
			qualifiers: Some(naming.0),
			group: naming.1,
			series: vec![held.measured.index],
			target: Some(held.target.index),
			transpose: false,
			..ChartPlan::new(ChartKind::Bullet, None, Vec::new(), false)
		});
	}
	if let Some(ladders) = table_ladders(table, label.as_ref().map(|label| label.index)) {
		let naming = guess_naming(table, label.as_ref(), None);
		return Some(ChartPlan {
			kind: ChartKind::Ladder,
			label: label.as_ref().map(|label| label.index),
			qualifiers: Some(naming.0),
			group: naming.1,
			series: ladder_columns(&ladders),
			transpose: false,
			..ChartPlan::new(ChartKind::Ladder, None, Vec::new(), false)
		});
	}
	if let Some(change) = change_columns(table) {
		let naming = guess_naming(table, label.as_ref(), None);
		return Some(ChartPlan {
			kind: ChartKind::Change,
			label: label.as_ref().map(|label| label.index),
			qualifiers: Some(naming.0),
			group: naming.1,
			series: change.iter().map(|column| column.index).collect(),
			transpose: false,
			..ChartPlan::new(ChartKind::Change, None, Vec::new(), false)
		});
	}
	let pairs = metric_pairs(&table.measures);
	if pairs.len() >= 2 {
		let naming = guess_naming(table, label.as_ref(), None);
		return Some(ChartPlan {
			kind: ChartKind::Change,
			label: label.as_ref().map(|label| label.index),
			qualifiers: Some(naming.0),
			group: naming.1,
			series: pairs.iter().map(|(_, after)| after.index).collect(),
			baselines: Some(pairs.iter().map(|(before, _)| before.index).collect()),
			transpose: false,
			..ChartPlan::new(ChartKind::Change, None, Vec::new(), false)
		});
	}
	let mut measures = drop_aggregates(&table.measures, rows);
	if measures.is_empty() || rows.len() < 2 {
		return None;
	}
	let mut order = axis_order(table, label.as_ref());
	if order.is_none()
		&& label.as_ref().map_or(true, |label| label.role != super::table_data::ColumnRole::Label)
		&& measures.len() >= 2
		&& measures[0].index == 0
	{
		let xs = column_values(&measures[0], rows);
		if xs.len() == rows.len() && is_monotonic(&xs) {
			label = Some(measures[0].clone());
			measures = measures[1..].to_vec();
			if rows.len() >= 4 {
				order = Some("numeric");
			}
		}
	}
	if prose_led(table, &measures) {
		return None;
	}
	if measures.len() == 1
		&& measures[0].arrow_share < 0.6
		&& label.as_ref().map_or(false, |label| label.role == super::table_data::ColumnRole::Label)
		&& METRIC_HEADER_RE.is_match(label.as_ref().expect("label role checked").header.trim())
	{
		return None;
	}
	let naming = guess_naming(table, label.as_ref(), None);
	let reference = guess_reference(table, label.as_ref(), &naming.0);
	let make = |kind: ChartKind, series: &[TableColumn], transpose: bool| -> ChartPlan {
		ChartPlan {
			kind,
			label: label.as_ref().map(|label| label.index),
			qualifiers: Some(naming.0.clone()),
			group: naming.1,
			reference: reference.clone(),
			series: series.iter().map(|column| column.index).collect(),
			transpose,
			..ChartPlan::new(kind, None, Vec::new(), false)
		}
	};
	if let Some(histogram) = histogram_columns(table, &measures) {
		let naming = guess_naming(table, Some(&histogram.bins), None);
		return Some(ChartPlan {
			label: Some(histogram.bins.index),
			qualifiers: Some(naming.0),
			group: naming.1,
			..make(ChartKind::Histogram, &histogram.counts, false)
		});
	}
	if measures.len() == 1 && measures[0].arrow_share >= 0.6 {
		return Some(make(
			if measures[0].mixed { ChartKind::Change } else { ChartKind::Paired },
			&measures,
			false,
		));
	}
	let arrows: Vec<TableColumn> = measures.iter().filter(|column| column.arrow_share >= 0.6).cloned().collect();
	if arrows.len() >= 2 {
		return Some(make(ChartKind::Change, &arrows, false));
	}
	if let Some(composition) = &table.composition {
		if composition.parts.iter().all(|index| Some(*index) != label.as_ref().map(|label| label.index)) {
			let parts: Vec<TableColumn> = composition
				.parts
				.iter()
				.filter_map(|index| table.columns.get(*index))
				.cloned()
				.collect();
			return Some(ChartPlan { whole: composition.whole, ..make(ChartKind::Stacked, &parts, false) });
		}
	}
	if let Some(outcomes) = outcome_columns(&measures) {
		return Some(make(ChartKind::Stacked, &outcomes, false));
	}
	let flowing: Vec<TableColumn> = if rows.len() > MAX_SHARE_PARTS {
		measures
			.iter()
			.map(|column| amount_behind(table, column).unwrap_or_else(|| column.clone()))
			.collect()
	} else {
		measures.clone()
	};
	if let Some(flow) = guess_flow(table, label.as_ref(), &flowing) {
		return Some(ChartPlan {
			offsets: flow.offsets,
			limit: flow.limit,
			..make(flow.kind, std::slice::from_ref(&flow.column), false)
		});
	}
	let mixed: Vec<TableColumn> = measures.iter().filter(|column| column.mixed).cloned().collect();
	if !mixed.is_empty() && mixed.len() * 2 >= measures.len() {
		return Some(make(ChartKind::Multiples, &measures, true));
	}
	let ranges = range_columns(&measures, rows);
	if let Some(ranges) = &ranges {
		if ranges.len() * 2 > measures.len() {
			return Some(make(ChartKind::Range, ranges, false));
		}
	}
	let dims: HashSet<Option<Dimension>> = measures.iter().map(|column| column.dim).collect();
	if let Some(stages) = stage_columns(&measures, rows) {
		return Some(make(ChartKind::Slope, &stages, false));
	}
	if let Some((loss, gain)) = opposed_columns(&measures) {
		return Some(make(ChartKind::Butterfly, &[loss, gain], false));
	}
	if measures.len() >= 2 && measures.len() <= 4 {
		let before = measures.iter().find(|column| BEFORE_RE.is_match(&column.header)).cloned();
		let after = before.as_ref().and_then(|before| {
			measures
				.iter()
				.find(|column| column.index != before.index && column.dim == before.dim && AFTER_RE.is_match(&column.header))
				.cloned()
		});
		if let (Some(before), Some(after)) = (before, after) {
			if before_after(table, &[before.clone(), after.clone()]).is_some() {
				let low = before.index.min(after.index);
				let high = before.index.max(after.index);
				let filtered: Vec<TableColumn> = measures
					.iter()
					.filter(|column| column.dim == before.dim && column.index >= low && column.index <= high)
					.cloned()
					.collect();
				return Some(make(ChartKind::Change, &filtered, false));
			}
			return Some(make(ChartKind::Paired, &[before, after], false));
		}
	}
	if rows.len() == 2 && measures.len() == 1 {
		return None;
	}
	if order.is_some() && rows.len() >= MIN_LINE_ROWS {
		let lead = measures[0].dim;
		let picked: Vec<TableColumn> = measures.iter().filter(|column| column.dim == lead).take(MAX_LINES).cloned().collect();
		return Some(make(ChartKind::Line, &picked, false));
	}
	if measures.len() == 1 && measures[0].scores {
		return Some(make(ChartKind::Progress, &measures, false));
	}
	if measures.len() <= 2 {
		let delta = measures
			.iter()
			.find(|column| {
				(column.signed_share >= 0.6 || DELTA_RE.is_match(&column.header))
					&& column_values(column, rows).iter().any(|value| *value < 0.0)
			})
			.cloned();
		if let Some(delta) = delta {
			return Some(make(ChartKind::Diverging, std::slice::from_ref(&delta), false));
		}
	}
	let share = measures
		.iter()
		.find(|column| column.dim == Some(Dimension::Percent) && !column.scores)
		.cloned();
	if let Some(share) = share {
		if rows.len() <= MAX_SHARE_SEGMENTS && (measures.len() == 1 || (measures.len() == 2 && dims.len() == 2)) {
			let values = column_values(&share, rows);
			let sum: f64 = values.iter().sum();
			if values.iter().all(|value| *value >= 0.0) && sum >= 97.0 && sum <= 103.0 {
				return Some(make(ChartKind::Share, std::slice::from_ref(&share), false));
			}
		}
	}
	if measures.len() == 1 && splits_total(table, label.as_ref(), &measures[0]) {
		return Some(make(ChartKind::Share, &measures, false));
	}
	if dots_fit(&measures, rows) {
		return Some(make(ChartKind::Dots, &measures, false));
	}
	if dims.len() == 1 && measures.len() >= 4 && rows.len() >= 3 {
		return Some(ChartPlan {
			shading: Some(guess_shading(&measures, rows)),
			..make(ChartKind::Heatmap, &measures, false)
		});
	}
	if label.is_none() && measures.len() == 2 && rows.len() >= 6 {
		return Some(make(ChartKind::Scatter, &measures, false));
	}
	if dims.len() >= 2
		|| (!DOT_DIMS.contains(&measures[0].dim.unwrap_or(Dimension::Count)) && guess_shading(&measures, rows) == Shading::Series)
	{
		let picked: Vec<TableColumn> = measures.iter().take(MAX_PANELS).cloned().collect();
		return Some(make(ChartKind::Multiples, &picked, false));
	}
	if measures.len() == 2 {
		let kind = if is_condition_pair(&measures[0].header, &measures[1].header) {
			ChartKind::Paired
		} else {
			ChartKind::Grouped
		};
		return Some(make(kind, &measures, false));
	}
	if measures.len() >= 3 {
		return Some(make(ChartKind::Grouped, &measures, false));
	}
	Some(make(ChartKind::Bar, &measures, false))
}

/// The `bullet` chart of `plan`: its one series measured against its target row by row.
fn bullet_chart(
	table: &TableAnalysis,
	plan: &ChartPlan,
	namers: &[TableColumn],
	group: Option<&TableColumn>,
) -> Option<ChartSpec> {
	let measured = plan.series.first().and_then(|index| table.columns.get(*index))?;
	let target = plan.target.and_then(|index| table.columns.get(index))?;
	let word = TARGET_HEADER_RE
		.captures(&target.header)
		.and_then(|captures| captures.name("word"))
		.map(|found| found.as_str().to_lowercase())?;
	let mut all_rows: Vec<usize> = table.rows.clone();
	all_rows.extend(table.totals.iter().copied());
	all_rows.sort();
	let rows = regroup(&all_rows, group);
	let polarity = plan.polarity.as_ref().and_then(|polarity| polarity.get(&measured.index)).copied();
	let mut points: Vec<Option<ChartPoint>> = Vec::new();
	let mut notes: Vec<Option<String>> = Vec::new();
	let mut targets: Vec<Option<ChartTarget>> = Vec::new();
	let mut lead: Option<NumberCell> = None;
	for &row in &rows {
		let cell = measured.cells.get(row);
		let value = measured_cell(cell);
		let goal = target_cell(&target.cells[row].text());
		let (Some(value), Some((goal_cell, goal_bound, goal_text))) = (value, goal) else {
			points.push(None);
			targets.push(None);
			notes.push(cell.and_then(|cell| {
				if cell.kind() != CellKind::Missing { Some(cell.text().to_string()) } else { None }
			}));
			continue;
		};
		if lead.is_none() {
			lead = Some(value.clone());
		}
		let implied = plan
			.row_polarity
			.as_ref()
			.and_then(|row_polarity| row_polarity.get(&row))
		.copied()
		.or(polarity);
		let bound = if goal_cell.upper.is_some() {
			None
		} else {
			goal_bound.or_else(|| {
				if CEILING_WORD_RE.is_match(&word) || implied == Some(Polarity::Lower) {
					Some(Bound::Max)
				} else if implied == Some(Polarity::Higher) {
					Some(Bound::Min)
				} else {
					None
				}
			})
		};
		points.push(Some(point(&value)));
		let text = if goal_bound.is_some() || bound.is_none() {
			goal_text
		} else {
			format!("{} {}", if bound == Some(Bound::Max) { "≤" } else { "≥" }, goal_text)
		};
		let mut chart_target = ChartTarget::from_point(&point(&goal_cell), bound);
		chart_target.text = text;
		targets.push(Some(chart_target));
		notes.push(None);
	}
	let lead = lead?;
	if points.iter().filter(|point| point.is_some()).count() < 2 {
		return None;
	}
	let naming = name_rows(namers, group, &rows, true);
	let spec = ChartSpec {
		kind: ChartKind::Bullet,
		categories: naming.names,
		groups: naming.groups,
		series: vec![ChartSeries {
			name: measured.header.clone(),
			dim: lead.dim,
			unit: if matches!(lead.dim, Dimension::Currency | Dimension::Rate) { lead.unit.clone() } else { String::new() },
			points,
			notes,
			polarity,
			best: None,
		}],
		targets: Some(targets),
		target_name: Some(if word == "bar" {
			"target".to_string()
		} else if word == "sla" || word == "slo" {
			word.to_uppercase()
		} else {
			word
		}),
		axis: naming.axis,
		..ChartSpec::base(ChartKind::Bullet)
	};
	with_meaning(table, plan, &spec, Some(&rows))
}

/// The columns `ladders` read their quantiles from, each once, in ladder order.
pub fn ladder_columns(ladders: &[Ladder]) -> Vec<usize> {
	let mut seen: HashSet<usize> = HashSet::new();
	let mut out = Vec::new();
	for ladder in ladders {
		for rung in &ladder.rungs {
			if seen.insert(rung.column.index) {
				out.push(rung.column.index);
			}
		}
	}
	out
}

/// `table`'s quantile ladders beside `label`, when they dominate what the table measures.
fn table_ladders(table: &TableAnalysis, label: Option<usize>) -> Option<Vec<Ladder>> {
	if table.rows.len() < 2 || find_target(table).is_some() {
		return None;
	}
	let sources: Vec<TableColumn> = table
		.columns
		.iter()
		.filter(|column| {
			Some(column.index) != label
				&& (column.role == ColumnRole::Measure || column.role == ColumnRole::Label)
		})
		.cloned()
		.collect();
	let ladders = quantile_ladders(&sources, &table.rows)?;
	let held = ladder_columns(&ladders);
	let mut measured: HashSet<usize> = table.measures.iter().map(|column| column.index).collect();
	measured.extend(held.iter().copied());
	if let Some(label) = label {
		measured.remove(&label);
	}
	if held.len() * 2 >= measured.len() {
		Some(ladders)
	} else {
		None
	}
}

/// Where quantile `word` ranks (0–100): `p999` is 99.9; a mean stands in the middle.
fn quantile_rank(word: &str) -> f64 {
	let lower = word.to_lowercase();
	if lower.starts_with("min") {
		return 0.0;
	}
	if lower.starts_with("max") {
		return 100.0;
	}
	let digit = lower.as_bytes().get(1).map_or(false, |byte| byte.is_ascii_digit());
	if !lower.starts_with('p') || !digit {
		return 50.0;
	}
	let mut rank: f64 = lower[1..].parse().unwrap_or(50.0);
	while rank > 100.0 {
		rank /= 10.0;
	}
	rank
}

/// What a header names besides its quantiles, and the unit it gives bare numbers.
fn ladder_subject(rest: &str) -> (String, Option<(Dimension, f64)>) {
	let mut scale: Option<(Dimension, f64)> = None;
	let mut words: Vec<String> = Vec::new();
	for raw in rest.replace(['(', ')', '[', ']', ',', ':', ';'], " ").split_whitespace() {
		let parsed = parse_cell(&format!("1 {raw}"));
		let unit = match parsed.as_number() {
			Some(unit) if !unit.unit.is_empty() && unit.dim != Dimension::Count => unit,
			_ => {
				words.push(raw.to_string());
				continue;
			}
		};
		scale = Some(scale.unwrap_or((unit.dim, unit.value)));
	}
	(words.join(" "), scale)
}

/// The quantile ladders among `measures`, lowest rank first, one dimension.
pub fn quantile_ladders(measures: &[TableColumn], rows: &[usize]) -> Option<Vec<Ladder>> {
	let mut drafts: Vec<(String, Vec<Rung>)> = Vec::new();
	let mut previous: Option<usize> = None;
	for column in measures {
		let list = QUANTILE_LIST_RE.captures(&column.header);
		let words: Vec<String> = match &list {
			Some(captures) => captures
				.name("list")
				.expect("list group")
				.as_str()
				.split('/')
				.map(str::trim)
				.map(str::to_string)
				.collect(),
			None => Vec::new(),
		};
		if let Some(captures) = &list {
			if words.iter().any(|word| QUANTILE_ITEM_RE.is_match(word)) {
				let subject_text = captures.name("list").expect("list group").as_str();
				let subject = ladder_subject(&column.header.replacen(subject_text, " ", 1));
				let rungs = words
					.iter()
					.enumerate()
					.map(|(slot, word)| Rung {
						column: column.clone(),
						slot: Some(slot),
						slots: Some(words.len()),
						name: word.clone(),
						rank: quantile_rank(word),
						scale: subject.1,
					})
					.collect();
				drafts.push((subject.0, rungs));
				previous = None;
				continue;
			}
		}
		let found: Vec<String> = QUANTILE_HEADER_RE
			.captures_iter(&column.header)
			.filter_map(|captures| captures.name("word").map(|word| word.as_str().to_string()))
			.collect();
		if found.len() != 1 {
			previous = None;
			continue;
		}
		let word = found[0].clone();
		let subject = ladder_subject(&column.header.replacen(&word, " ", 1));
		let rung = Rung {
			column: column.clone(),
			slot: None,
			slots: None,
			name: word,
			rank: quantile_rank(&found[0]),
			scale: subject.1,
		};
		let draft_index = if subject.0.is_empty() && previous.is_some() {
			previous
		} else {
			drafts
				.iter()
				.position(|(name, rungs)| *name == subject.0 && rungs.iter().all(|rung| rung.slot.is_none()))
		};
		match draft_index {
			Some(index) => drafts[index].1.push(rung),
			None => drafts.push((subject.0.clone(), vec![rung])),
		}
		previous = Some(draft_index.unwrap_or(drafts.len() - 1));
	}
	let mut ladders: Vec<Ladder> = Vec::new();
	for (name, mut rungs) in drafts {
		rungs.sort_by(|a, b| a.rank.partial_cmp(&b.rank).unwrap_or(std::cmp::Ordering::Equal));
		if rungs.len() < 2 || rungs.windows(2).any(|pair| pair[0].rank == pair[1].rank) {
			continue;
		}
		if rungs.iter().all(|rung| rung.rank == 0.0 || rung.rank == 100.0) {
			continue;
		}
		let read: Vec<Vec<Option<(ChartPoint, Dimension, String)>>> = rows
			.iter()
			.map(|&row| rungs.iter().map(|rung| rung_value(rung, row)).collect())
			.collect();
		let mut dims: HashSet<(Dimension, String)> = HashSet::new();
		for values in &read {
			for value in values.iter().flatten() {
				dims.insert((value.1, value.2.clone()));
			}
		}
		if dims.len() != 1 {
			continue;
		}
		let held: Vec<Vec<f64>> = read
			.iter()
			.map(|values| {
				values
					.iter()
					.filter_map(|value| value.as_ref().map(|(point, _, _)| point.value))
					.collect()
			})
			.collect();
		let full: Vec<&Vec<f64>> = held.iter().filter(|values| values.len() >= 2).collect();
		let ordered = full
			.iter()
			.filter(|values| values.iter().enumerate().all(|(at, value)| at == 0 || *value >= values[at - 1]))
			.count();
		if full.len() < 2 || (ordered as f64) < ORDERED_SHARE * full.len() as f64 {
			continue;
		}
		let first = read.iter().flatten().find(|value| value.is_some()).expect("a read value");
		let (dim, unit) = (first.as_ref().expect("read value").1, first.as_ref().expect("read value").2.clone());
		ladders.push(Ladder { name, rungs, dim, unit });
	}
	let first_dim = ladders.first().map(|ladder| (ladder.dim, ladder.unit.clone()));
	let kept: Vec<Ladder> = ladders
		.into_iter()
		.filter(|ladder| Some((ladder.dim, ladder.unit.clone())) == first_dim)
		.collect();
	if kept.is_empty() {
		None
	} else {
		Some(kept.into_iter().take(MAX_LADDERS).collect())
	}
}

/// `rung`'s value at `row` as a plotted point, its dimension and unit.
fn rung_value(rung: &Rung, row: usize) -> Option<(ChartPoint, Dimension, String)> {
	let cell = rung.column.cells.get(row)?;
	let read: Option<(f64, Dimension, String, String)> = if let Some(slot) = rung.slot {
		let items: Vec<&str> = cell.text().split('/').map(str::trim).collect();
		let mut listed: Option<Vec<f64>> = cell.as_number().and_then(|number| number.values.clone());
		if listed.is_none() {
			listed = Some(
				items
					.iter()
					.map(|item| {
						let own = parse_cell(item);
						match own.as_number() {
							Some(number) if number.fit == super::table_data::CellFit::Pure && number.dim == Dimension::Count => {
								number.value
							}
							_ => f64::NAN,
						}
					})
					.collect(),
			);
		}
		let listed = listed?;
		let value = *listed.get(slot)?;
		if listed.len() != rung.slots.unwrap_or(0) || !value.is_finite() {
			return None;
		}
		let number = cell.as_number();
		let measured = number.map_or(false, |number| number.values.is_some());
		let dim = if measured { number.expect("measured").dim } else { Dimension::Count };
		let unit = if measured { number.expect("measured").unit.clone() } else { String::new() };
		let text = items.get(slot)?.trim().to_string();
		Some((value, dim, unit, text))
	} else {
		let number = cell.as_number()?;
		if number.upper.is_none()
			&& number.to.is_none()
			&& Some(number.dim) == rung.column.dim
			&& number.dim != Dimension::Fraction
		{
			Some((number.value, number.dim, number.unit.clone(), number.figure.clone()))
		} else {
			None
		}
	};
	let (mut value, mut dim, unit, text) = read?;
	if dim == Dimension::Count && unit.is_empty() {
		if let Some((scale_dim, factor)) = rung.scale {
			dim = scale_dim;
			value *= factor;
		}
	}
	Some((
		ChartPoint { value, text, emphasis: cell.emphasis(), upper: None, error: None },
		dim,
		if matches!(dim, Dimension::Currency | Dimension::Rate) { unit } else { String::new() },
	))
}

/// A `ladder` chart over the table's rows: one series per quantile.
fn ladder_chart(
	table: &TableAnalysis,
	plan: &ChartPlan,
	namers: &[TableColumn],
	group: Option<&TableColumn>,
	ladders: &[Ladder],
) -> Option<ChartSpec> {
	let label = plan.label.and_then(|index| table.columns.get(index));
	let rows = rank(&regroup(&table.rows, group), group, plan.order.as_ref(), table);
	let naming = name_rows(namers, group, &rows, true);
	let mut series: Vec<ChartSeries> = Vec::new();
	let mut drawn: Vec<ChartLadder> = Vec::new();
	for ladder in ladders {
		let start = series.len();
		for rung in &ladder.rungs {
			series.push(ChartSeries {
				name: rung.name.clone(),
				dim: ladder.dim,
				unit: ladder.unit.clone(),
				points: rows.iter().map(|&row| rung_value(rung, row).map(|(point, _, _)| point)).collect(),
				notes: rows.iter().map(|_| None).collect(),
				polarity: plan.polarity.as_ref().and_then(|polarity| polarity.get(&rung.column.index)).copied(),
				best: None,
			});
		}
		drawn.push(ChartLadder {
			name: ladder.name.clone(),
			series: ladder.rungs.iter().enumerate().map(|(at, _)| start + at).collect(),
		});
	}
	let present = rows
		.iter()
		.enumerate()
		.filter(|(at, _)| series.iter().any(|entry| entry.points.get(*at).is_some_and(|point| point.is_some())))
		.count();
	if present < 2 {
		return None;
	}
	let columns: Vec<TableColumn> = {
		let mut seen: HashSet<usize> = HashSet::new();
		let mut out = Vec::new();
		for ladder in ladders {
			for rung in &ladder.rungs {
				if seen.insert(rung.column.index) {
					out.push(rung.column.clone());
				}
			}
		}
		out
	};
	let transition = (drawn.len() == 2 && is_transition(&drawn[0].name, &drawn[1].name)).then_some(true);
	with_meaning(
		table,
		plan,
		&ChartSpec {
			kind: ChartKind::Ladder,
			categories: naming.names,
			groups: naming.groups,
			series,
			axis: naming.axis,
			caption: totals_caption(table, label, &columns, false),
			ladders: Some(drawn),
			transition,
			..ChartSpec::base(ChartKind::Ladder)
		},
		Some(&rows),
	)
}

/// The interval a bin cell covers: `2–5` → [2, 5], open ends to ±∞.
fn bin_of(cell: Option<&Cell>) -> Option<(f64, f64)> {
	let cell = cell?.as_number()?;
	if cell.signed || cell.to.is_some() || cell.values.is_some() || cell.denominator.is_some() {
		return None;
	}
	if cell.fit == super::table_data::CellFit::Range {
		return Some((cell.value, cell.upper.unwrap_or(cell.value)));
	}
	if cell.fit != super::table_data::CellFit::Pure && cell.fit != super::table_data::CellFit::Note {
		return None;
}
	let text = cell.text.trim();
	if text.starts_with('<') || text.starts_with('≤') {
		return Some((f64::NEG_INFINITY, cell.value));
	}
	if text.starts_with('>') || text.starts_with('≥') || text.ends_with('+') {
		return Some((cell.value, f64::INFINITY));
	}
	Some((cell.value, cell.value))
}

/// `rows` as ordered bins of `column`, ascending or descending, meeting end to start.
pub fn bin_rows(column: &TableColumn, rows: &[usize]) -> Option<Vec<usize>> {
	if rows.len() < MIN_BINS || rows.len() > MAX_BINS {
		return None;
	}
	let bins: Vec<(f64, f64)> = rows
		.iter()
		.map(|&row| bin_of(column.cells.get(row)))
		.collect::<Option<Vec<_>>>()?;
	if bins.iter().all(|(low, high)| low == high) {
		return None;
	}
	let ascending = bins[0].0 < bins[bins.len() - 1].0;
	let order: Vec<usize> = if ascending { rows.to_vec() } else { rows.iter().rev().copied().collect() };
	let sorted: Vec<(f64, f64)> = if ascending { bins } else { bins.into_iter().rev().collect() };
	let whole = sorted
		.iter()
		.all(|(low, high)| (!low.is_finite() || low.fract() == 0.0) && (!high.is_finite() || high.fract() == 0.0));
	for at in 1..sorted.len() {
		let (low, high) = sorted[at - 1];
		let next_low = sorted[at].0;
		let cell = &column.cells[order[at]];
		let step = if whole {
			1.0
		} else if let Some(number) = cell.as_number() {
			2.0 * written_slack(number)
		} else {
			0.0
		};
		let gap = next_low - high;
		if !(next_low > low) || !(gap >= 0.0) || gap > step + 1e-9 * high.abs() {
			return None;
		}
	}
	Some(order)
}

/// The columns of `measures` (but `bins`) counting how often each bin occurs.
fn frequency_columns(measures: &[TableColumn], bins: &TableColumn, rows: &[usize]) -> Option<Vec<TableColumn>> {
	let counted: Vec<&TableColumn> = measures.iter().filter(|column| column.index != bins.index).collect();
	if counted.is_empty() || counted.len() > MAX_DISTRIBUTIONS {
		return None;
	}
	let frequencies = counted.iter().all(|column| {
		if column.scores || column.mixed || PER_UNIT_RE.is_match(&column.header) || DELTA_RE.is_match(&column.header) {
			return false;
		}
		let (subject, scale) = ladder_subject(&column.header);
		if scale.is_some() && subject.is_empty() {
			return false;
		}
		let values = column_values(column, rows);
		if values.len() < rows.len().saturating_sub(1) || values.iter().any(|value| *value < 0.0) {
			return false;
		}
		if column.dim == Some(Dimension::Count) {
			values.iter().all(|value| is_integer(*value))
		} else {
			let sum: f64 = values.iter().sum();
			column.dim == Some(Dimension::Percent) && (sum - 100.0).abs() <= SHARE_SLACK
		}
	});
	if !frequencies {
		return None;
	}
	let dims: HashSet<Option<Dimension>> = counted.iter().map(|column| column.dim).collect();
	if dims.len() == 1 {
		Some(counted.into_iter().cloned().collect())
	} else {
		None
	}
}

/// The column whose rows are ordered bins, with the measures counting each bin.
struct HistogramColumns {
	bins: TableColumn,
	counts: Vec<TableColumn>,
}

/// The column whose rows are `table`'s ordered bins, with the measures counting each bin.
pub fn histogram_columns(table: &TableAnalysis, measures: &[TableColumn]) -> Option<HistogramColumns> {
	let bins = table
		.label
		.as_ref()
		.and_then(|label| bin_rows(label, &table.rows).map(|_| label.clone()))
		.or_else(|| {
			table
				.columns
				.first()
				.and_then(|column| bin_rows(column, &table.rows).map(|_| column.clone()))
		})?;
	let counts = frequency_columns(measures, &bins, &table.rows)?;
	Some(HistogramColumns { bins, counts })
}

/// `series` of a histogram as shares of each one's total.
fn as_shares(series: &[ChartSeries]) -> Vec<ChartSeries> {
	series
		.iter()
		.map(|entry| {
			let total: f64 = entry.points.iter().map(|point| point.as_ref().map_or(0.0, |point| point.value.max(0.0))).sum();
			ChartSeries {
				dim: Dimension::Percent,
				unit: String::new(),
				points: entry
					.points
					.iter()
					.map(|point| {
						point.as_ref().and_then(|point| {
							if total > 0.0 {
								let value = (100.0 * point.value) / total;
								Some(ChartPoint { value, text: percent_text(value), ..point.clone() })
							} else {
								None
							}
						})
					})
					.collect(),
				..entry.clone()
			}
		})
		.collect()
}

/// Share of `column`'s numbers of its own dimension written as an `a–b` range running up.
fn column_range_share(column: &TableColumn, rows: &[usize]) -> f64 {
	let numbers: Vec<&NumberCell> = rows
		.iter()
		.filter_map(|&row| column.cells.get(row))
		.filter_map(|cell| cell.as_number())
		.filter(|number| Some(number.dim) == column.dim)
		.collect();
	if numbers.is_empty() {
		return 0.0;
	}
	numbers
		.iter()
		.filter(|number| number.upper.map_or(false, |upper| upper > number.value))
		.count() as f64
		/ numbers.len() as f64
}

/// Share of the plotted values written as `a–b` ranges running up.
fn interval_share(series: &[ChartSeries]) -> f64 {
	let points: Vec<&ChartPoint> = series.iter().flat_map(|entry| entry.points.iter().flatten()).collect();
	if points.is_empty() {
		return 0.0;
	}
	points
		.iter()
		.filter(|point| point.upper.map_or(false, |upper| upper > point.value))
		.count() as f64
		/ points.len() as f64
}

/// The same-unit measures plotted as intervals when `a–b` values dominate.
pub fn range_columns(measures: &[TableColumn], rows: &[usize]) -> Option<Vec<TableColumn>> {
	let key = |column: &TableColumn| format!("{:?}:{}", column.dim, column.unit);
	let mut ranged: HashSet<String> = HashSet::new();
	for column in measures {
		if !(column.dim == Some(Dimension::Count) && POSITIONS_RE.is_match(&column.header))
			&& column_range_share(column, rows) >= RANGE_SHARE
		{
			ranged.insert(key(column));
		}
	}
	if ranged.is_empty() {
		return None;
}
	let columns: Vec<TableColumn> = measures
		.iter()
		.filter(|column| ranged.contains(&key(column)))
		.take(MAX_RANGES)
		.cloned()
		.collect();
	let mut cells: Vec<&NumberCell> = Vec::new();
	for column in &columns {
		for &row in rows {
			if let Some(cell) = column.cells.get(row) {
				if let Some(number) = cell.as_number() {
					if Some(number.dim) == column.dim {
						cells.push(number);
					}
				}
			}
		}
	}
	let spans = cells.iter().filter(|cell| cell.upper.map_or(false, |upper| upper > cell.value)).count();
	if (spans as f64) >= cells.len() as f64 * RANGE_SHARE {
		Some(columns)
	} else {
		None
	}
}

/// Whether `columns` over `rows` read best as dots.
pub fn dots_fit(columns: &[TableColumn], rows: &[usize]) -> bool {
	let first = match columns.first() {
		Some(first) => first,
		None => return false,
	};
	let first_dim = match first.dim {
		Some(dim) => dim,
		None => return false,
	};
	if !DOT_DIMS.contains(&first_dim)
		|| columns.iter().any(|column| {
			column.dim != first.dim
				|| ((first_dim == Dimension::Currency || first_dim == Dimension::Rate) && column.unit != first.unit)
		})
		|| columns.len() < 3
		|| columns.len() > SERIES_COLORS
		|| rows.len() < 3
		|| rows.len() > MAX_DOT_ROWS
		|| value_spread(columns, rows) < DOT_SPREAD
	{
		return false;
	}
	let grid: Vec<Vec<f64>> = rows
		.iter()
		.map(|&row| {
			columns
				.iter()
				.map(|column| {
					column
						.cells
						.get(row)
						.and_then(|cell| cell.as_number())
						.filter(|number| Some(number.dim) == column.dim && number.value > 0.0)
						.map_or(f64::NAN, |number| number.value)
				})
				.collect()
		})
		.collect();
	match dot_layout(&grid) {
		Some("apart") => true,
		Some("clumped") => dot_baseline(columns).is_some(),
		_ => false,
	}
}

/// The column the others are read against: the one in use, or the baseline.
fn dot_baseline(columns: &[TableColumn]) -> Option<TableColumn> {
	let baseline = columns
		.iter()
		.find(|column| IN_USE_RE.is_match(&column.header))
		.or_else(|| {
			columns
				.iter()
				.find(|column| columns.iter().all(|other| other.index == column.index || is_transition(&column.header, &other.header)))
		})
		.cloned()?;
	if sole_mark(columns, &baseline) {
		Some(baseline)
	} else {
		None
	}
}

/// Whether no other of `columns` shares a reference word marking `baseline`.
fn sole_mark(columns: &[TableColumn], baseline: &TableColumn) -> bool {
	let lower = baseline.header.to_lowercase();
	let marks: Vec<String> = BEFORE_RE.find_iter(&lower).map(|found| found.as_str().to_string()).collect();
	columns.iter().all(|column| {
		column.index == baseline.index
			|| !marks.iter().any(|mark| has_word(&column.header.to_lowercase(), mark))
	})
}

/// Among rival columns of one polarity, the one best in most rows.
fn leading_column(table: &TableAnalysis, plan: &ChartPlan, columns: &[TableColumn]) -> Option<TableColumn> {
	let first = columns.first()?;
	let polarity = plan.polarity.as_ref().and_then(|polarity| polarity.get(&first.index)).copied()?;
	if plan.rivals != Some(Rivals::Columns)
		|| columns
			.iter()
			.any(|column| plan.polarity.as_ref().and_then(|polarity| polarity.get(&column.index)).copied() != Some(polarity))
	{
		return None;
	}
	let mut wins: HashMap<usize, usize> = HashMap::new();
	for &row in &table.rows {
		let mut best: Option<(usize, f64)> = None;
		for column in columns {
			let Some(cell) = column.cells.get(row).and_then(|cell| cell.as_number()) else {
				continue;
			};
			let better = match best {
				None => true,
				Some((_, value)) => {
					if polarity == Polarity::Lower {
						cell.value < value
					} else {
						cell.value > value
					}
				}
			};
			if better {
				best = Some((column.index, cell.value));
			}
		}
		if let Some((index, _)) = best {
			*wins.entry(index).or_insert(0) += 1;
		}
	}
	let mut ranked: Vec<(usize, usize)> = wins.into_iter().collect();
	ranked.sort_by(|a, b| b.1.cmp(&a.1));
	match ranked.first() {
		Some((index, wins)) if *wins > ranked.get(1).map_or(0, |(_, wins)| *wins) => {
			columns.iter().find(|column| column.index == *index).cloned()
		}
		_ => None,
	}
}

/// A column as a series: its cells of the column's dimension, other cells as gaps.
fn column_series(column: &TableColumn, rows: &[usize], name: String) -> ChartSeries {
	let mut points: Vec<Option<ChartPoint>> = Vec::new();
	let mut notes: Vec<Option<String>> = Vec::new();
	for &row in rows {
		let cell = column.cells.get(row);
		let fits = cell.map_or(false, |cell| is_number(cell) && cell.as_number().map_or(false, |number| Some(number.dim) == column.dim));
		let moved = fits
			&& cell
				.and_then(|cell| cell.as_number())
				.map_or(false, |number| number.to.is_some() && column.arrow_share < 0.6);
		points.push(if moved {
			cell.and_then(|cell| cell.as_number()).and_then(|number| {
				number.to.map(|to| ChartPoint {
					value: to,
					text: format!("{} → {}", number.figure, number.to_figure.clone().unwrap_or_default()),
					emphasis: number.emphasis,
					upper: None,
					error: None,
				})
			})
		} else if fits {
			cell.and_then(|cell| cell.as_number()).map(|number| point(number))
		} else {
			None
		});
		notes.push(match cell {
			Some(cell) if !fits && cell.kind() != CellKind::Missing && !is_number(cell) => Some(cell.text().to_string()),
			_ => None,
		});
	}
	ChartSeries {
		name,
		dim: column.dim.unwrap_or(Dimension::Count),
		unit: column.unit.clone(),
		points,
		notes,
		polarity: None,
		best: None,
	}
}

/// One row of a metric-per-row table as a series over the plotted columns.
fn row_series(name: &str, columns: &[TableColumn], row: usize) -> ChartSeries {
	let cells: Vec<Option<&Cell>> = columns.iter().map(|column| column.cells.get(row)).collect();
	let mut tally: HashMap<Dimension, usize> = HashMap::new();
	for cell in cells.iter().flatten() {
		if let Some(number) = cell.as_number() {
			*tally.entry(number.dim).or_insert(0) += 1;
		}
	}
	let mut dim = Dimension::Count;
	let mut best = 0;
	for (key, count) in &tally {
		if *count > best {
			dim = *key;
			best = *count;
		}
	}
	let unit_cell = cells.iter().flatten().find_map(|cell| cell.as_number()).filter(|number| number.dim == dim);
	ChartSeries {
		name: name.to_string(),
		dim,
		unit: if matches!(dim, Dimension::Currency | Dimension::Rate) {
			unit_cell.map_or(String::new(), |number| number.unit.clone())
		} else {
			String::new()
		},
		points: cells
			.iter()
			.map(|cell| {
				cell.and_then(|cell| cell.as_number())
					.filter(|number| number.dim == dim)
					.map(|number| point(number))
			})
			.collect(),
		notes: cells
			.iter()
			.map(|cell| match cell {
				Some(cell) if !is_number(cell) && cell.kind() != CellKind::Missing => Some(cell.text().to_string()),
				_ => None,
			})
			.collect(),
		polarity: None,
		best: None,
	}
}

/// `Total: 2,964 · 2,310` for each summary row, over the plotted columns.
fn totals_caption(table: &TableAnalysis, label: Option<&TableColumn>, columns: &[TableColumn], transpose: bool) -> Option<String> {
	if transpose || label.is_none() || table.totals.is_empty() {
		return None;
	}
	let label = label.expect("label checked");
	let mut lines: Vec<String> = Vec::new();
	for &row in table.totals.iter().take(2) {
		let values: Vec<String> = columns
			.iter()
			.filter_map(|column| column.cells.get(row))
			.map(|cell| cell.text().to_string())
			.filter(|text| !text.is_empty())
			.collect();
		if !values.is_empty() {
			lines.push(format!("{}: {}", label.cells[row].text(), values.join(" · ")));
		}
	}
	if lines.is_empty() { None } else { Some(lines.join("   ")) }
}

/// The nearest kind `kind` draws as for `series`.
fn fit_kind(kind: ChartKind, series: &[ChartSeries], has_x: bool) -> ChartKind {
	if matches!(kind, ChartKind::Multiples | ChartKind::Stat) {
		return kind;
	}
	if kind == ChartKind::Scatter && has_x {
		return kind;
	}
	if kind == ChartKind::Range {
		return kind;
	}
	if kind == ChartKind::Share {
		return if series.first().map_or(0, |entry| entry.points.len()) <= MAX_SHARE_SEGMENTS {
			kind
		} else {
			ChartKind::Bar
		};
	}
	let units: HashSet<String> = series.iter().map(|entry| format!("{:?}:{}", entry.dim, entry.unit)).collect();
	if units.len() > 1 {
		return ChartKind::Multiples;
	}
	if kind == ChartKind::Ladder {
		return fit_kind(ChartKind::Dots, series, has_x);
	}
	if kind == ChartKind::Histogram {
		return if series.len() <= MAX_DISTRIBUTIONS { kind } else { ChartKind::Heatmap };
	}
	if kind == ChartKind::Dots {
		return if series.len() == 1 {
			ChartKind::Bar
		} else if series.len() <= SERIES_COLORS {
			kind
		} else {
			ChartKind::Heatmap
		};
	}
	if kind == ChartKind::Slope && series.len() >= 2 && series.len() <= MAX_STAGES {
		return kind;
	}
	if kind == ChartKind::Butterfly && series.len() == 2 {
		return kind;
	}
	if matches!(kind, ChartKind::Diverging | ChartKind::Line | ChartKind::Heatmap) {
		return kind;
	}
	if matches!(kind, ChartKind::Timeline | ChartKind::Waterfall)
		&& series.len() == 1
		&& series[0]
			.points
			.iter()
			.all(|point| point.as_ref().map_or(false, |point| kind == ChartKind::Waterfall || point.value >= 0.0))
	{
		return kind;
	}
	if kind == ChartKind::Stacked
		&& series.len() >= 2
		&& series.len() <= SERIES_COLORS
		&& series.iter().all(|entry| entry.points.iter().all(|point| point.as_ref().map_or(true, |point| point.value >= 0.0)))
	{
		return kind;
	}
	if kind == ChartKind::Progress
		&& series.len() <= MAX_TRACKS
		&& series.iter().all(|entry| {
			entry.dim == Dimension::Percent
				&& entry.points.iter().all(|point| point.as_ref().map_or(true, |point| point.value >= 0.0 && point.value <= 100.0))
		}) {
		return kind;
	}
	let count = series.len();
	if count == 1 {
		ChartKind::Bar
	} else if count == 2 && kind == ChartKind::Paired {
		ChartKind::Paired
	} else if count <= MAX_GROUPED {
		ChartKind::Grouped
	} else {
		ChartKind::Heatmap
	}
}

/// Whether `spec` is worth drawing at all.
pub fn worth_charting(spec: &ChartSpec) -> bool {
	if spec.kind == ChartKind::Stat {
		return stat_story(&spec.series).is_some();
	}
	if spec.kind == ChartKind::Change {
		return spec.categories.len() >= 3
			&& spec.series.iter().any(|entry| {
				entry.points.iter().any(|point| {
					point.as_ref().map_or(false, |point| point.value.abs().ln().abs() >= NOTABLE_FACTOR.ln())
				})
			});
	}
	if spec.kind == ChartKind::Share && spec.series[0].dim != Dimension::Percent {
		return spec.categories.len() >= 3;
	}
	if spec.kind == ChartKind::Bullet {
		let targets = spec.targets.as_deref().unwrap_or(&[]);
		return targets
			.iter()
			.enumerate()
			.filter(|(at, target)| target.is_some() && spec.series[0].points.get(*at).map_or(false, |point| point.is_some()))
			.count() >= 3;
	}
	if spec.kind == ChartKind::Range {
		return spec.categories.len() >= 4 || (spec.categories.len() == 3 && spec.series.len() >= 2);
	}
	if spec.kind == ChartKind::Ladder {
		return spec.categories.len() >= 3 || (spec.categories.len() == 2 && spec.series.len() >= 4);
	}
	if matches!(spec.kind, ChartKind::Slope | ChartKind::Butterfly) {
		return spec.categories.len() >= 3;
	}
	let moved = spec
		.categories
		.iter()
		.enumerate()
		.filter(|(at, _)| spec.series.iter().all(|entry| entry.points.get(*at).map_or(false, |point| point.is_some())))
		.count();
	if spec.kind == ChartKind::Paired && spec.transition == Some(true) && moved < 3 {
		return false;
	}
	if spec.kind == ChartKind::Paired && spec.transition == Some(true) && spec.categories.len() == 3 {
		let values: Vec<f64> = spec
			.series
			.iter()
			.flat_map(|entry| entry.points.iter().flatten().filter(|point| point.value > 0.0).map(|point| point.value))
			.collect();
		let moves = spec.categories.iter().enumerate().any(|(at, _)| {
			let a = spec.series[0].points.get(at).and_then(|point| point.as_ref());
			let b = spec.series.get(1).and_then(|entry| entry.points.get(at)).and_then(|point| point.as_ref());
			a.map_or(false, |a| {
				b.map_or(false, |b| {
					a.value > 0.0 && b.value > 0.0 && (b.value / a.value).ln().abs() >= NOTABLE_FACTOR.ln()
				})
			})
		});
		if moves && values.iter().copied().fold(f64::MIN, f64::max) >= DECADES * values.iter().copied().fold(f64::MAX, f64::min) {
			return true;
		}
	}
	if spec.kind == ChartKind::Diverging {
		let values: Vec<f64> = spec.series[0]
			.points
			.iter()
			.flatten()
			.filter(|point| point.value != 0.0)
			.map(|point| point.value)
			.collect();
		let sizes: Vec<f64> = values.iter().map(|value| value.abs()).collect();
		let split = values.iter().any(|value| *value > 0.0) && values.iter().any(|value| *value < 0.0);
		let tallies = sizes.iter().all(|size| is_integer(*size) && *size <= MAX_PROSE_COUNT);
		return values.len() >= 4
			&& !tallies
			&& (split || values.len() >= 9 || sizes.iter().copied().fold(f64::MIN, f64::max) >= 3.0 * sizes.iter().copied().fold(f64::MAX, f64::min));
	}
	let bars = if spec.kind == ChartKind::Multiples {
		spec.categories.len().max(spec.series.len())
	} else if spec.kind == ChartKind::Waterfall {
		spec.categories.len() + 1
	} else {
		spec.categories.len()
	};
	if bars < 4 {
		return false;
	}
	let uniform = spec.kind != ChartKind::Share
		&& spec.series.iter().all(|entry| {
			let values: Vec<f64> = entry.points.iter().flatten().map(|point| point.value).collect();
			let mut tally: HashMap<u64, usize> = HashMap::new();
			for value in &values {
				*tally.entry(value.to_bits()).or_insert(0) += 1;
			}
			values.len() >= 4 && tally.values().copied().max().unwrap_or(0) >= values.len() - 1
		});
	if uniform {
		return false;
	}
	let mut points = 0;
	let mut spread: f64 = 1.0;
	for entry in &spec.series {
		let positive: Vec<f64> = entry.points.iter().flatten().filter(|point| point.value > 0.0).map(|point| point.value).collect();
		points += entry.points.iter().flatten().count();
		if positive.len() >= 2 {
			spread = spread.max(positive.iter().copied().fold(f64::MIN, f64::max) / positive.iter().copied().fold(f64::MAX, f64::min));
		}
	}
	points >= 9 || spread >= 3.0
}

/// The data `plan` draws from `table`, or `None` when it leaves too little.
pub fn build_chart(table: &TableAnalysis, plan: &ChartPlan) -> Option<ChartSpec> {
	let label = plan.label.and_then(|index| table.columns.get(index));
	let ordered = matches!(plan.kind, ChartKind::Line | ChartKind::Scatter) && ordered_rows(table, label);
	let long_enough = table.rows.len() >= MIN_LINE_ROWS;
	if plan.kind == ChartKind::Line && !plan.transpose && !(ordered && long_enough) {
		if ordered {
			return None;
		}
		let across = plan.series.iter().all(|&index| {
			let header = table.columns.get(index).map_or("", |column| column.header.as_str());
			matches!(parse_cell(header).kind(), CellKind::Date)
				|| MONTH_RE.is_match(header)
				|| SEQUENCE_LABEL_RE.is_match(header)
		});
		let mut next = plan.clone();
		next.kind = if across { ChartKind::Multiples } else { ChartKind::Grouped };
		next.transpose = across;
		return build_chart(table, &next);
	}
	if plan.kind == ChartKind::Scatter && table.rows.len() < MIN_SCATTER_ROWS {
		let mut next = plan.clone();
		next.kind = if ordered && long_enough { ChartKind::Line } else { ChartKind::Grouped };
		return build_chart(table, &next);
	}
	let columns: Vec<TableColumn> = plan.series.iter().filter_map(|&index| table.columns.get(index).cloned()).collect();
	if columns.is_empty() {
		return None;
	}
	let mut wanted: HashSet<usize> = HashSet::new();
	if let Some(label) = plan.label {
		wanted.insert(label);
	}
	if let Some(qualifiers) = &plan.qualifiers {
		wanted.extend(qualifiers.iter().copied());
	}
	if let Some(group) = plan.group {
		wanted.insert(group);
	}
	let namers: Vec<TableColumn> = table.columns.iter().filter(|column| wanted.contains(&column.index)).cloned().collect();
	let grouper = plan.group.and_then(|index| table.columns.get(index));
	let flowing = matches!(plan.kind, ChartKind::Timeline | ChartKind::Waterfall);
	let group = if plan.kind == ChartKind::Line
		|| flowing
		|| plan.transpose
		|| !namers.iter().any(|column| Some(column.index) != plan.group)
	{
		None
	} else {
		grouper.map(|column| column.clone())
	};
	let bullet = if plan.kind == ChartKind::Bullet && !plan.transpose {
		bullet_chart(table, plan, &namers, group.as_ref())
	} else {
		None
	};
	if let Some(spec) = bullet {
		return Some(spec);
	}
	if plan.kind == ChartKind::Change && !plan.transpose {
		if let Some(baselines) = &plan.baselines {
			if baselines.len() == columns.len() {
				let baseline_columns: Vec<TableColumn> =
					baselines.iter().filter_map(|&index| table.columns.get(index).cloned()).collect();
				return change_chart(table, plan, &namers, group.as_ref(), &columns, Some(&baseline_columns));
			}
		}
		return change_chart(table, plan, &namers, group.as_ref(), &columns, None);
	}
	let ladders = if plan.kind == ChartKind::Ladder && !plan.transpose {
		quantile_ladders(&columns, &table.rows)
	} else {
		None
	};
	if let Some(ladders) = ladders {
		return ladder_chart(table, plan, &namers, group.as_ref(), &ladders);
	}
	let arrows: Vec<TableColumn> = columns.iter().filter(|column| column.arrow_share >= 0.6).cloned().collect();
	if arrows.len() >= 2 && !plan.transpose {
		return change_chart(table, plan, &namers, group.as_ref(), &arrows, None);
	}
	if arrows.len() == 1
		&& plan.kind != ChartKind::Paired
		&& plan.kind != ChartKind::Range
		&& !plan.transpose
	{
		let mut next = plan.clone();
		next.kind = if arrows[0].mixed { ChartKind::Change } else { ChartKind::Paired };
		next.series = vec![arrows[0].index];
		return build_chart(table, &next);
	}
	let factored: Option<Vec<TableColumn>> = if plan.transpose || range_columns(&columns, &table.rows).is_some() {
		None
	} else if plan.kind == ChartKind::Paired && columns.len() <= 2 {
		before_after(table, &columns)
	} else if matches!(plan.kind, ChartKind::Paired | ChartKind::Grouped | ChartKind::Dots) {
		against_baseline(table, plan, &columns)
	} else {
		None
	};
	let factors = factored.and_then(|factored| change_chart(table, plan, &namers, group.as_ref(), &factored, None));
	if factors.is_some() {
		return factors;
	}
	let signed: Option<TableColumn> = if plan.kind == ChartKind::Diverging
		&& plan.order.is_none()
		&& !plan.transpose
		&& group.is_none()
		&& axis_order(table, label).is_none()
		&& !label.map_or(false, |label| STEP_HEADER_RE.is_match(&label.header))
	{
		columns
			.iter()
			.find(|column| column_values(column, &table.rows).iter().any(|value| *value < 0.0))
			.cloned()
	} else {
		None
	};
	let ranked: Option<RowOrder> = if plan.order.is_some() && !plan.transpose && plan.kind != ChartKind::Scatter && !flowing {
		plan.order.clone()
	} else if let Some(signed) = &signed {
		Some(RowOrder { column: signed.index, direction: SortDirection::Descending })
	} else {
		None
	};
	let bins = if plan.kind == ChartKind::Histogram {
		label.and_then(|label| bin_rows(label, &table.rows))
	} else {
		None
	};
	let regrouped = regroup(&table.rows, group.as_ref());
	let listed: Vec<usize> = if plan.kind == ChartKind::Line {
		chronological(&table.rows, label)
	} else if flowing {
		table.rows.iter().copied().filter(|&row| Some(row) != plan.limit).collect()
	} else if let Some(bins) = bins {
		bins
	} else {
		rank(&regrouped, group.as_ref(), ranked.as_ref(), table)
	};
	let unplotted: Vec<usize> = if plan.kind == ChartKind::Paired && !plan.transpose && columns.len() == 2 {
		listed
			.iter()
			.copied()
			.filter(|&row| {
				columns.iter().all(|column| {
					column.cells.get(row).map_or(true, |cell| {
						!is_number(cell) || cell.as_number().map_or(true, |number| Some(number.dim) != column.dim)
					})
				}) && columns
					.iter()
					.any(|column| column.cells.get(row).map_or(false, |cell| cell.kind() != CellKind::Missing))
			})
			.collect()
	} else {
		Vec::new()
	};
	let rows: Vec<usize> = if !unplotted.is_empty() && unplotted.len() < listed.len() - 1 {
		listed.iter().copied().filter(|row| !unplotted.contains(row)).collect()
	} else {
		listed.clone()
	};
	let skipped: Option<String> = if rows == listed {
		None
	} else {
		Some(format!("Not plotted: {}", name_rows(&namers, None, &unplotted, false).names.join(", ")))
	};
	let nest = !plan.transpose && plan.kind != ChartKind::Line && plan.kind != ChartKind::Scatter;
	let naming = name_rows(&namers, group.as_ref(), &rows, nest);
	let mut categories: Vec<String>;
	let mut series: Vec<ChartSeries>;
	if plan.transpose {
		categories = columns.iter().map(|column| column.header.clone()).collect();
		series = rows
			.iter()
			.enumerate()
			.map(|(at, &row)| {
				let mut entry = row_series(naming.names.get(at).map_or("", String::as_str), &columns, row);
				entry.polarity = plan.row_polarity.as_ref().and_then(|polarity| polarity.get(&row)).copied();
				entry
			})
			.collect();
	} else if plan.kind == ChartKind::Paired && columns.len() == 1 {
		let column = &columns[0];
		let (from, to) = arrow_names(&column.header);
		let mut before = column_series(column, &rows, from);
		before.polarity = plan.polarity.as_ref().and_then(|polarity| polarity.get(&column.index)).copied();
		let mut after = before.clone();
		after.name = to;
		after.points = rows
			.iter()
			.map(|&row| {
				column.cells.get(row).and_then(|cell| cell.as_number()).and_then(|number| {
					number.to.map(|to| ChartPoint {
						value: to,
						text: number.to_figure.clone().unwrap_or_default(),
						emphasis: number.emphasis,
						upper: None,
						error: None,
					})
				})
			})
			.collect();
		categories = naming.names.clone();
		series = vec![before, after];
	} else {
		categories = naming.names.clone();
		series = columns
			.iter()
			.map(|column| {
				let mut entry = column_series(column, &rows, column.header.clone());
				entry.polarity = if column.arrow_share >= 0.6 {
					None
				} else {
					plan.polarity.as_ref().and_then(|polarity| polarity.get(&column.index)).copied()
				};
				entry
			})
			.collect();
	}
	let mut x: Option<ChartAxis> = None;
	if plan.kind == ChartKind::Scatter && series.len() >= 2 {
		let xs = series.remove(0);
		x = Some(ChartAxis {
			name: xs.name.clone(),
			dim: xs.dim,
			unit: xs.unit.clone(),
			values: xs.points.iter().map(|point| point.as_ref().map_or(f64::NAN, |point| point.value)).collect(),
		});
		let second = series.remove(0);
		series = vec![second];
	} else if plan.kind == ChartKind::Line && label.is_some() && !plan.transpose {
	let values: Vec<f64> = rows
			.iter()
			.map(|&row| {
				label
					.and_then(|label| label.cells.get(row))
					.and_then(|cell| cell.as_number())
					.map_or(f64::NAN, |number| number.value)
			})
			.collect();
		if values.iter().all(|value| value.is_finite()) && is_monotonic(&values) {
			let first = label
				.and_then(|label| label.cells.get(rows[0]))
				.and_then(|cell| cell.as_number());
			x = Some(ChartAxis {
				name: label.map_or(String::new(), |label| label.header.clone()),
				dim: first.map_or(Dimension::Count, |number| number.dim),
				unit: label.map_or(String::new(), |label| label.unit.clone()),
				values,
			});
		}
	}
	series.retain(|entry| entry.points.iter().filter(|point| point.is_some()).count() >= 2);
	if categories.len() < 2 || series.is_empty() {
		return None;
	}
	if plan.kind == ChartKind::Dots && !plan.transpose {
		let unit = format!("{:?}:{}", series[0].dim, series[0].unit);
		let shared: Vec<ChartSeries> = series
			.iter()
			.filter(|entry| format!("{:?}:{}", entry.dim, entry.unit) == unit)
			.cloned()
			.collect();
		if shared.len() >= 2 {
			series = shared;
		}
	}
	let intervals = if plan.transpose { None } else { fold_estimate(&series) };
	let lead = intervals.as_ref().map(|(entries, _)| &entries[0]);
	let mut estimate: Option<ChartSeries> = None;
	let mut kind = if plan.transpose && plan.kind != ChartKind::Stat {
		ChartKind::Multiples
	} else {
		fit_kind(plan.kind, &series, x.is_some())
	};
	if let Some((interval_series, interval_estimate)) = &intervals {
		let rangeish = matches!(
			kind,
			ChartKind::Range | ChartKind::Bar | ChartKind::Grouped | ChartKind::Paired | ChartKind::Dots
		);
		if rangeish
			&& interval_share(interval_series) >= RANGE_SHARE
			&& interval_series.len() <= MAX_RANGES
			&& !lead.map_or(false, |lead| lead.dim == Dimension::Count && POSITIONS_RE.is_match(&lead.name))
		{
			kind = ChartKind::Range;
			series = interval_series.clone();
			estimate = interval_estimate.clone();
			let first = series.first().cloned();
			let second = series.get(1).cloned();
			if let (Some(first), Some(second)) = (first, second) {
				if first.dim == second.dim && first.unit == second.unit && series.len() > 2 {
					series.retain(|entry| {
						!DELTA_RE.is_match(&entry.name) || entry.name == first.name || entry.name == second.name
					});
				}
			}
		}
	}
	if kind == ChartKind::Range && intervals.is_none() {
		kind = fit_kind(ChartKind::Grouped, &series, x.is_some());
	}
	let layout: Option<&'static str> = if matches!(kind, ChartKind::Grouped | ChartKind::Dots) {
		let grid: Vec<Vec<f64>> = categories
			.iter()
			.enumerate()
			.map(|(at, _)| {
				series
					.iter()
					.map(|entry| {
						entry
							.points
							.get(at)
							.and_then(|point| point.as_ref())
							.map_or(f64::NAN, |point| if point.value > 0.0 { point.value } else { f64::NAN })
					})
					.collect()
			})
			.collect();
		dot_layout(&grid)
	} else {
		None
	};
	let plotted: Vec<TableColumn> = if layout.is_some() {
		columns
			.iter()
			.filter(|column| series.iter().any(|entry| entry.name == column.header))
			.cloned()
			.collect()
	} else {
		Vec::new()
	};
	if kind == ChartKind::Grouped
		&& !plan.transpose
		&& DOT_DIMS.contains(&series[0].dim)
		&& (layout == Some("apart") || (layout == Some("clumped") && dot_baseline(&plotted).is_some()))
		&& spread_of(
			&series
				.iter()
				.flat_map(|entry| entry.points.iter().flatten().flat_map(|point| [point.value, point.upper.unwrap_or(point.value)]))
				.collect::<Vec<_>>(),
		) >= if series.len() >= 3 { DOT_SPREAD } else { DECADES }
	{
		kind = ChartKind::Dots;
	} else if kind == ChartKind::Grouped
		&& !plan.transpose
		&& !DOT_DIMS.contains(&series[0].dim)
		&& pair_spread(
			&categories
				.iter()
				.enumerate()
				.map(|(at, _)| {
					series
						.iter()
						.map(|entry| {
							entry
								.points
								.get(at)
								.and_then(|point| point.as_ref())
								.map_or(f64::NAN, |point| if point.value > 0.0 { point.value } else { f64::NAN })
						})
						.collect()
				})
				.collect::<Vec<_>>(),
		) >= QUANTITY_SPREAD
	{
		kind = ChartKind::Multiples;
	}
	let baseline = if kind == ChartKind::Dots && layout == Some("clumped") {
		dot_baseline(&plotted)
	} else {
		None
	};
	let base = baseline
	.as_ref()
		.filter(|_| series.len() >= 3)
		.and_then(|baseline| series.iter().find(|entry| entry.name == baseline.header).cloned());
	if let Some(base) = &base {
		series = series
			.iter()
			.filter(|entry| entry.name != base.name)
			.map(|entry| {
				let points = entry
					.points
					.iter()
					.enumerate()
					.map(|(at, point)| {
						let from = base.points.get(at).and_then(|point| point.as_ref());
						match (point.as_ref(), from) {
							(Some(point), Some(from)) if from.value > 0.0 => Some(ChartPoint {
								value: point.value / from.value,
								text: format!("{} → {}", from.text, point.text),
								emphasis: point.emphasis || from.emphasis,
								upper: None,
								error: point.error.map(|error| error / from.value),
							}),
							_ => None,
						}
					})
					.collect();
				ChartSeries { dim: Dimension::Ratio, unit: String::new(), points, ..entry.clone() }
			})
			.collect();
	}
	if kind == ChartKind::Heatmap && plan.shading == Some(Shading::Series) && series.len() <= MAX_PANELS {
		kind = ChartKind::Multiples;
	}
	let totals: Vec<f64> = series
		.iter()
		.map(|entry| entry.points.iter().map(|point| point.as_ref().map_or(0.0, |point| point.value.max(0.0))).sum())
		.collect();
	let shares = kind == ChartKind::Histogram
		&& series.iter().all(|entry| entry.dim == Dimension::Count)
		&& (plan.normalize.unwrap_or(false)
			|| (series.len() >= 2
				&& totals.iter().copied().fold(f64::MIN, f64::max) >= MIX_SPREAD * totals.iter().copied().fold(f64::MAX, f64::min)));
	if shares {
		series = as_shares(&series);
	}
	if kind == ChartKind::Share {
		series = series.into_iter().take(1).collect();
	}
	if kind == ChartKind::Diverging {
		series = vec![series
			.iter()
			.find(|entry| entry.points.iter().any(|point| point.as_ref().map_or(false, |point| point.value < 0.0)))
			.cloned()
			.unwrap_or_else(|| series[0].clone())];
	}
	let whole_column = plan.whole.and_then(|index| table.columns.get(index));
	let whole = if kind == ChartKind::Stacked
		&& whole_column.map_or(false, |column| column.dim == series.first().map(|entry| entry.dim))
	{
		whole_column.map(|column| column_series(column, &rows, column.header.clone()))
	} else {
		None
	};
	let tones = if kind == ChartKind::Stacked {
		outcome_tones(&series.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>())
	} else if kind == ChartKind::Share {
		outcome_tones(&categories)
	} else {
		None
	};
	let normalized = kind == ChartKind::Stacked
		&& (plan.normalize.unwrap_or(false)
			|| (tones.is_some() && total_spread(&series, whole.as_ref()) >= MIX_SPREAD));
	let transition = (if kind == ChartKind::Paired {
		series.len() == 2
	} else {
		kind == ChartKind::Range
			&& series.len() >= 2
			&& series[0].dim == series[1].dim
			&& series[0].unit == series[1].unit
	}) && ((plan.kind == ChartKind::Paired && columns.len() == 1) || is_transition(&series[0].name, &series[1].name));
	if kind == ChartKind::Paired && !transition {
		series = series.iter().map(|entry| ChartSeries { polarity: None, ..entry.clone() }).collect();
	}
	let contest = !matches!(
		kind,
		ChartKind::Line
			| ChartKind::Paired
			| ChartKind::Stat
			| ChartKind::Share
			| ChartKind::Stacked
			| ChartKind::Timeline
			| ChartKind::Waterfall
			| ChartKind::Range
			| ChartKind::Slope
			| ChartKind::Butterfly
			| ChartKind::Histogram
	) && base.is_none();
	let flow = if matches!(kind, ChartKind::Timeline | ChartKind::Waterfall) {
		Some(flow_spec(table, plan, &rows, &series[0]))
	} else {
		None
	};
	let mut leaders: Option<Vec<Option<usize>>> = None;
	if contest && plan.transpose {
		let rivals: Vec<bool> = columns
			.iter()
			.map(|column| !DELTA_RE.is_match(&column.header) && column.signed_share < 0.5)
			.collect();
		series = series
			.iter()
			.map(|entry| ChartSeries { best: best_point(entry, Some(&rivals), 2), ..entry.clone() })
			.collect();
	} else if contest && plan.rivals == Some(Rivals::Rows) {
		series = series.iter().map(|entry| ChartSeries { best: best_point(entry, None, 3), ..entry.clone() }).collect();
	} else if contest && plan.rivals == Some(Rivals::Columns) {
		leaders = column_leaders(&categories, &series);
	}
	let stated = if matches!(kind, ChartKind::Range | ChartKind::Dots) {
		Some(stated_references(table, &series, base.as_ref().map(|base| base.name.as_str())))
	} else {
		None
	};
	if stated.as_ref().map_or(false, |stated| !stated.restating.is_empty() && series.len() > stated.restating.len()) {
		let restating = stated.as_ref().expect("stated").restating.clone();
		series.retain(|entry| !restating.contains(&entry.name));
	}
	let caption = if flow.as_ref().map_or(false, |flow| flow.total.is_some()) {
		None
	} else {
		let parts: Vec<String> = [totals_caption(table, label, &columns, plan.transpose), skipped].into_iter().flatten().collect();
		if parts.is_empty() { None } else { Some(parts.join(" · ")) }
	};
	let spec = ChartSpec {
		kind,
		transition: transition.then_some(true),
		transposed: plan.transpose.then_some(true),
		categories,
		groups: if plan.transpose { None } else { naming.groups.clone() },
		series,
		x: flow.as_ref().and_then(|flow| flow.x.clone()).or(x),
		caption,
		axis: naming.axis.clone(),
		shading: plan.shading,
		reference: if kind == ChartKind::Stat {
			Some(vec![stat_baseline(&naming.names)])
		} else if plan.transpose {
			None
		} else {
			reference_rows(table, plan.reference.as_ref(), group.as_ref(), &rows)
		},
		whole,
		normalized: (normalized || shares).then_some(true),
		tones,
		total: flow.as_ref().and_then(|flow| flow.total.clone()),
		annotations: flow
			.as_ref()
			.and_then(|flow| flow.annotations.clone())
			.or_else(|| stated.as_ref().map(|stated| stated.marks.clone()).filter(|marks| !marks.is_empty())),
		leaders,
		estimate,
		baseline: base.as_ref().map(|base| base.name.clone()),
		..ChartSpec::base(kind)
	};
	with_meaning(table, plan, &spec, if plan.transpose { None } else { Some(&rows) })
}

/// A line's peak, for a callout: where it rises most.
#[derive(Clone, Debug)]
struct Peak {
	at: usize,
	rise: f64,
	point: ChartPoint,
	striking: bool,
	series: Option<usize>,
}

/// Reference marks and ratio series a stated rule restates.
#[derive(Clone, Debug)]
struct StatedReferences {
	marks: Vec<ChartAnnotation>,
	restating: Vec<String>,
}

/// What a timeline or waterfall adds to its spec.
#[derive(Clone, Debug)]
struct FlowSpec {
	x: Option<ChartAxis>,
	total: Option<FlowTotal>,
	annotations: Option<Vec<ChartAnnotation>>,
}

/// `series` with the first interval series leading, and an estimate series taken out.
fn fold_estimate(series: &[ChartSeries]) -> Option<(Vec<ChartSeries>, Option<ChartSeries>)> {
	let ranged = series
		.iter()
		.find(|entry| interval_share(std::slice::from_ref(entry)) >= RANGE_SHARE)?;
	let inside = |entry: &ChartSeries| -> bool {
		if std::ptr::eq(entry, ranged) || entry.dim != ranged.dim || entry.unit != ranged.unit {
			return false;
		}
		let mut held = 0;
		for (at, point) in entry.points.iter().enumerate() {
			let Some(point) = point.as_ref() else { continue };
			let Some(span) = ranged.points.get(at).and_then(|point| point.as_ref()) else {
				return false;
			};
			let Some(upper) = span.upper else { return false };
			if point.upper.is_some() || point.value < span.value || point.value > upper {
				return false;
			}
			held += 1;
		}
		held >= 2
	};
	let estimate = series.iter().find(|entry| inside(entry)).cloned();
	let folded: Vec<ChartSeries> = std::iter::once(ranged.clone())
		.chain(
			series
				.iter()
				.filter(|entry| !std::ptr::eq(*entry, ranged) && !estimate.as_ref().map_or(false, |found| std::ptr::eq(*entry, found)))
				.cloned(),
		)
		.collect();
	Some((folded, estimate))
}

/// Reference rules a `range` or `dots` chart reads its values against.
fn stated_references(table: &TableAnalysis, series: &[ChartSeries], baseline: Option<&str>) -> StatedReferences {
	let mut marks: Vec<ChartAnnotation> = Vec::new();
	let mut restating: Vec<String> = Vec::new();
	let ends = |entry: &ChartSeries| -> Vec<f64> {
		entry
			.points
			.iter()
			.flatten()
			.flat_map(|point| [point.value, point.upper.unwrap_or(point.value)])
			.collect()
	};
	let parity = series.iter().position(|entry| {
		let values = ends(entry);
		entry.dim == Dimension::Ratio
			&& values.iter().copied().fold(f64::MAX, f64::min) < 1.0
			&& values.iter().copied().fold(f64::MIN, f64::max) > 1.0
	});
	if let Some(baseline) = baseline {
		marks.push(ChartAnnotation::Reference { value: 1.0, label: baseline.to_string(), series: Some(0) });
	} else if let Some(parity) = parity {
		marks.push(ChartAnnotation::Reference { value: 1.0, label: "1×".to_string(), series: Some(parity) });
	}
	let mut headers: HashSet<String> = HashSet::new();
	for column in &table.columns {
		headers.insert(column.header.clone());
	}
	let mut texts: Vec<String> = headers.iter().cloned().collect();
	for column in &table.columns {
		for &row in &table.rows {
			if let Some(cell) = column.cells.get(row) {
				texts.push(cell.text().to_string());
			}
		}
	}
	for source in texts {
		let Some(captures) = STATED_REFERENCE_RE.captures(&source) else { continue };
		let figure = captures.name("after").or_else(|| captures.name("before")).map(|found| found.as_str().trim().to_string());
		let Some(figure) = figure else { continue };
		let lead = captures.name("lead").map(|found| found.as_str()).unwrap_or("");
		if lead.eq_ignore_ascii_case("vs") || lead.eq_ignore_ascii_case("vs.") || lead.eq_ignore_ascii_case("versus") {
			if !headers.contains(&source) {
				continue;
			}
		}
		let parsed = parse_cell(&figure);
		let Some(cell) = parsed.as_number() else { continue };
		if cell.fit != super::table_data::CellFit::Pure || cell.value <= 0.0 {
			continue;
		}
		let at = series.iter().position(|entry| {
			entry.dim == cell.dim && (entry.dim != Dimension::Currency || entry.unit == cell.unit)
		});
		let values = at.map_or(Vec::new(), |at| ends(&series[at]));
		let low = values.iter().copied().fold(f64::MAX, f64::min);
		let high = values.iter().copied().fold(f64::MIN, f64::max);
		if values.is_empty()
			|| cell.value * REFERENCE_REACH < low
			|| cell.value > high * REFERENCE_REACH
			|| marks.iter().any(|mark| match mark {
				ChartAnnotation::Reference { value, series, .. } => *series == at && *value == cell.value,
				_ => false,
			}) {
			continue;
		}
		let word = captures.name("word").or_else(|| captures.name("lead")).map(|found| found.as_str()).unwrap_or("");
		let label = if word.eq_ignore_ascii_case("vs") || word.eq_ignore_ascii_case("vs.") || word.eq_ignore_ascii_case("versus") {
			cell.figure.clone()
		} else {
			format!("{} {}", cell.figure, word.to_lowercase())
		};
		marks.push(ChartAnnotation::Reference { value: cell.value, label, series: at });
		for entry in series {
			if entry.dim == Dimension::Ratio && entry.name == source {
				restating.push(entry.name.clone());
			}
		}
	}
	StatedReferences { marks, restating }
}

/// `spec` with what `plan` says the table means: title, focus highlights, status badges.
fn with_meaning(table: &TableAnalysis, plan: &ChartPlan, spec: &ChartSpec, order: Option<&[usize]>) -> Option<ChartSpec> {
	let focus: Vec<usize> = match order {
		Some(order) => plan
			.focus
			.as_ref()
			.map_or(Vec::new(), |focus| {
				focus
					.iter()
					.filter_map(|&row| order.iter().position(|&candidate| candidate == row))
					.collect()
			}),
		None => Vec::new(),
	};
	let matrix = match order {
		Some(order) if MATRIX_KINDS.contains(&spec.kind) => status_matrix(table, plan, order),
		_ => None,
	};
	let column = if plan.status.is_none() || order.is_none() || matrix.is_some() {
		None
	} else {
		table.columns.get(plan.status.expect("status index"))
	};
	let statuses = column.and_then(|column| column.statuses.clone());
	let mut marks: Vec<ChartAnnotation> = spec.annotations.clone().unwrap_or_default();
	marks.extend(focus.into_iter().map(|category| ChartAnnotation::Highlight { category }));
	marks.extend(story_callouts(spec));
	Some(ChartSpec {
		title: plan.title.clone(),
		annotations: if marks.is_empty() { None } else { Some(marks) },
		status: statuses.as_ref().map(|statuses| {
			order
				.expect("status order")
				.iter()
				.map(|&row| statuses.get(row).copied().flatten())
				.collect()
		}),
		status_name: statuses.as_ref().map(|_| column.expect("statuses").header.clone()),
		matrix,
		..spec.clone()
	})
}

/// Callouts stating what a line's marks leave unwritten: its peak.
fn story_callouts(spec: &ChartSpec) -> Vec<ChartAnnotation> {
	if spec.kind != ChartKind::Line {
		return Vec::new();
	}
	let mut peaks: Vec<Peak> = Vec::new();
	for (series, entry) in spec.series.iter().enumerate() {
		if let Some(mut peak) = line_peak(spec, entry) {
			peak.series = Some(series);
			peaks.push(peak);
		}
	}
	let pick = peaks
		.iter()
		.find(|peak| peak.point.emphasis)
		.cloned()
		.or_else(|| {
			let mut sorted = peaks.clone();
			sorted.sort_by(|a, b| b.rise.partial_cmp(&a.rise).unwrap_or(std::cmp::Ordering::Equal));
			sorted.into_iter().next()
		});
	pick.map_or(Vec::new(), |peak| {
		vec![ChartAnnotation::Callout {
			category: peak.at,
			series: peak.series,
			text: format!("peak {}", peak.point.text),
		}]
	})
}

/// Largest over smallest positive category total of a stack.
fn total_spread(series: &[ChartSeries], whole: Option<&ChartSeries>) -> f64 {
	let totals: Vec<f64> = (0..series.first().map_or(0, |entry| entry.points.len()))
		.filter_map(|at| {
			let written = whole
				.and_then(|whole| whole.points.get(at))
				.and_then(|point| point.as_ref())
				.map_or(0.0, |point| point.value);
			let summed: f64 = series
				.iter()
				.map(|entry| entry.points.get(at).and_then(|point| point.as_ref()).map_or(0.0, |point| point.value.max(0.0)))
				.sum();
			let total = if written != 0.0 { written } else { summed };
			if total > 0.0 { Some(total) } else { None }
		})
		.collect();
	if totals.is_empty() {
		1.0
	} else {
		totals.iter().copied().fold(f64::MIN, f64::max) / totals.iter().copied().fold(f64::MAX, f64::min)
	}
}

/// The status columns of `table` that neither name nor plot `plan`'s rows.
fn status_matrix(table: &TableAnalysis, plan: &ChartPlan, order: &[usize]) -> Option<Vec<StatusColumn>> {
	let mut taken: HashSet<usize> = HashSet::new();
	if let Some(label) = plan.label {
		taken.insert(label);
	}
	if let Some(qualifiers) = &plan.qualifiers {
		taken.extend(qualifiers.iter().copied());
	}
	if let Some(group) = plan.group {
		taken.insert(group);
	}
	taken.extend(plan.series.iter().copied());
	let columns: Vec<&TableColumn> = table
		.columns
		.iter()
		.filter(|column| column.statuses.is_some() && !taken.contains(&column.index))
		.collect();
	if columns.len() < 2 || columns.len() > MAX_MATRIX {
		return None;
	}
	let mut seen: HashSet<Status> = HashSet::new();
	for column in &columns {
		let statuses = column.statuses.as_ref().expect("statuses");
		for &row in order {
			if let Some(status) = statuses.get(row).copied().flatten() {
				seen.insert(status);
			}
		}
	}
	if seen.len() < 2 {
		return None;
	}
	Some(
		columns
			.into_iter()
			.map(|column| StatusColumn {
				name: column.header.clone(),
				statuses: order
					.iter()
					.map(|&row| column.statuses.as_ref().expect("statuses").get(row).copied().flatten())
					.collect(),
			})
			.collect(),
	)
}

/// What a `timeline` or `waterfall` adds: starts, the total, and the cap rule.
fn flow_spec(table: &TableAnalysis, plan: &ChartPlan, rows: &[usize], entry: &ChartSeries) -> FlowSpec {
	let column = plan.series.first().and_then(|&index| table.columns.get(index));
	let label = plan.label.and_then(|index| table.columns.get(index));
	let offsets = if plan.kind == ChartKind::Timeline {
		plan.offsets.and_then(|index| table.columns.get(index))
	} else {
		None
	};
	let starts: Option<Vec<f64>> = offsets.map(|offsets| {
		rows.iter()
			.map(|&row| flow_cell(offsets, row).map_or(f64::NAN, |cell| cell.value))
			.collect()
	});
	let x = match (offsets, starts) {
		(Some(offsets), Some(starts)) if starts.iter().all(|value| value.is_finite()) => Some(ChartAxis {
			name: offsets.header.clone(),
			dim: offsets.dim.unwrap_or(entry.dim),
			unit: offsets.unit.clone(),
			values: starts,
		}),
		_ => None,
	};
	let total_cell = column.and_then(|column| {
		table.totals
			.iter()
			.copied()
			.find(|&at| flow_cell(column, at).is_some())
			.and_then(|at| flow_cell(column, at).map(|cell| (at, cell)))
	});
	let values: Vec<&ChartPoint> = entry.points.iter().flatten().collect();
	let total = match (total_cell, label) {
		(Some((row, cell)), Some(label)) => FlowTotal { name: label.cells[row].text().to_string(), point: point(cell) },
		_ => FlowTotal {
			name: "Total".to_string(),
			point: ChartPoint {
				value: values.iter().map(|point| point.value).sum(),
				text: if entry.dim == Dimension::Duration { String::new() } else { sum_figure(&values) },
				emphasis: false,
				upper: None,
				error: None,
			},
		},
	};
	let cap = if plan.kind == ChartKind::Waterfall {
		plan.limit.and_then(|limit| column.and_then(|column| flow_cell(column, limit)))
	} else {
		None
	};
	let cap_name = match (label, plan.limit) {
		(Some(label), Some(limit)) => label.cells[limit].text().to_string(),
		_ => String::new(),
	};
	FlowSpec {
		x,
		total: Some(total),
		annotations: cap.map(|cap| {
			vec![ChartAnnotation::Reference {
				value: cap.value,
				label: format!("{} {}", cap_name, cap.figure).trim().to_string(),
				series: None,
			}]
		}),
	}
}

static SUM_FIGURE_RE: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^(?<approx>[~≈≤≥<>]?)\s*(?<sign>[+\-−]?)\s*(?<currency>[$€£¥]?)\s*(?<int>\d[\d,]*)(?:\.(?<frac>\d+))?(?<spaces>\s*)(?<unit>[A-Za-zµ]*)$")
		.expect("valid sum figure regex")
});

/// The sum of `points` written like their figures, when all share a currency, unit and scale.
fn sum_figure(points: &[&ChartPoint]) -> String {
	let read: Vec<Option<regex::Captures<'_>>> = points
		.iter()
		.map(|point| SUM_FIGURE_RE.captures(point.text.trim()))
		.collect();
	if read.is_empty() || read.iter().any(|found| found.is_none()) {
		return String::new();
	}
	let matches: Vec<&regex::Captures<'_>> = read.iter().map(|found| found.as_ref().expect("matched")).collect();
	let first = matches[0];
	let scale = |found: &regex::Captures, at: usize| -> Option<f64> {
		let int = found.name("int")?.as_str().replace(',', "");
		let frac = found.name("frac").map_or("0", |found| found.as_str());
		let written: f64 = format!("{int}.{frac}").parse().ok()?;
		if written == 0.0 {
			None
		} else {
			Some(points[at].value.abs() / written)
		}
	};
	let factor = matches.iter().enumerate().find_map(|(at, found)| scale(found, at)).filter(|scale| scale.is_finite()).unwrap_or(1.0);
	let alike = matches.iter().enumerate().all(|(at, found)| {
		found.name("currency").map_or("", |found| found.as_str()) == first.name("currency").map_or("", |found| found.as_str())
			&& found.name("unit").map_or("", |found| found.as_str()) == first.name("unit").map_or("", |found| found.as_str())
			&& match scale(found, at) {
				Some(scale) => (scale / factor - 1.0).abs() <= 1e-6,
				None => true,
			}
	});
	if !alike {
		return String::new();
	}
	let decimals = matches
		.iter()
		.map(|found| found.name("frac").map_or(0, |frac| frac.as_str().len()))
		.max()
		.unwrap_or(0);
	let sum: f64 = points.iter().map(|point| point.value).sum();
	let grouped = matches.iter().any(|found| found.name("int").map_or(false, |int| int.as_str().contains(','))) || sum.abs() >= 10_000.0;
	let digits = locale_fixed(sum.abs(), decimals, grouped);
	let approx = if matches.iter().any(|found| found.name("approx").map_or(false, |approx| !approx.as_str().is_empty())) {
		"~"
	} else {
		""
	};
	format!(
		"{}{}{}{}{}{}",
		approx,
		if sum < 0.0 { "−" } else { "" },
		first.name("currency").map_or("", |found| found.as_str()),
		digits,
		first.name("spaces").map_or("", |found| found.as_str()),
		first.name("unit").map_or("", |found| found.as_str())
	)
}

/// `rows` sorted by `order`'s column within each run of one `group` value.
fn rank(rows: &[usize], group: Option<&TableColumn>, order: Option<&RowOrder>, table: &TableAnalysis) -> Vec<usize> {
	let Some(order) = order else { return rows.to_vec() };
	let Some(column) = table.columns.get(order.column) else { return rows.to_vec() };
	let sign = if order.direction == SortDirection::Descending { -1.0 } else { 1.0 };
	let value = |row: usize| {
		column
			.cells
			.get(row)
			.and_then(|cell| cell.as_number())
			.filter(|number| Some(number.dim) == column.dim)
			.map_or(f64::NAN, |number| number.value)
	};
	let mut runs: Vec<Vec<usize>> = Vec::new();
	for &row in rows {
		let same = runs.last().map_or(false, |last| {
			group.map_or(true, |group| name_cell(group, last[0]) == name_cell(group, row))
		});
		if same {
			runs.last_mut().expect("last run").push(row);
		} else {
			runs.push(vec![row]);
		}
	}
	runs.into_iter()
		.flat_map(|mut run| {
			run.sort_by(|&a, &b| {
				let x = value(a);
				let y = value(b);
				if x.is_nan() || y.is_nan() {
					if x.is_nan() && y.is_nan() {
						std::cmp::Ordering::Equal
					} else if x.is_nan() {
						std::cmp::Ordering::Greater
					} else {
						std::cmp::Ordering::Less
					}
				} else {
					(sign * (x - y)).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
				}
			});
			run
		})
		.collect()
}

/// Positions in `rows` of the rows `reference` marks, one per group run.
fn reference_rows(
	table: &TableAnalysis,
	reference: Option<&RowReference>,
	group: Option<&TableColumn>,
	rows: &[usize],
) -> Option<Vec<usize>> {
	let reference = reference?;
	let column = table.columns.get(reference.column)?;
	let mut marked: Vec<usize> = Vec::new();
	let mut seen: HashSet<String> = HashSet::new();
	for (at, &row) in rows.iter().enumerate() {
		if name_cell(column, row) != reference.value {
			continue;
		}
		let run = group.map_or_else(String::new, |group| name_cell(group, row));
		if seen.contains(&run) {
			return None;
		}
		seen.insert(run);
		marked.push(at);
	}
	if marked.is_empty() { None } else { Some(marked) }
}

/// The baseline and compared column of a dumbbell whose factors span decades.
fn before_after(table: &TableAnalysis, columns: &[TableColumn]) -> Option<Vec<TableColumn>> {
	let a = columns.first()?;
	if columns.len() > 2 {
		return None;
	}
	let pair: Vec<TableColumn> = if columns.len() == 1 {
		if a.arrow_share >= 0.6 { vec![a.clone()] } else { return None; }
	} else {
		let b = &columns[1];
		if a.dim != b.dim {
			return None;
		}
		if is_transition(&a.header, &b.header) {
			vec![a.clone(), b.clone()]
		} else if is_transition(&b.header, &a.header) {
			vec![b.clone(), a.clone()]
		} else {
			return None;
		}
	};
	let from = pair[0].clone();
	let to = pair.get(1).cloned();
	let mut moves: Vec<(f64, f64)> = Vec::new();
	for &row in &table.rows {
		let Some(start) = from.cells.get(row).and_then(|cell| cell.as_number()) else { continue };
		if let Some(to) = &to {
			if let Some(end) = to.cells.get(row).and_then(|cell| cell.as_number()) {
				moves.push((start.value, end.value));
			}
		} else if let Some(end) = start.to {
			moves.push((start.value, end));
		}
	}
	let positive: Vec<f64> = moves.iter().flat_map(|(start, end)| [*start, *end]).filter(|value| *value > 0.0).collect();
	let spread = if positive.len() >= 4 {
		positive.iter().copied().fold(f64::MIN, f64::max) / positive.iter().copied().fold(f64::MAX, f64::min)
	} else {
		1.0
	};
	if spread < DECADES {
		return None;
	}
	let measured = pair
		.iter()
		.all(|column| column.dim.map_or(false, |dim| DOT_DIMS.contains(&dim)) && column.unit == from.unit);
	let reach = moves
		.iter()
		.filter_map(|(start, end)| if *start > 0.0 && *end > 0.0 { Some((end / start).abs().ln()) } else { None })
		.fold(0.0_f64, f64::max);
	if measured && reach >= VISIBLE_MOVE * spread.ln() {
		None
	} else {
		Some(pair)
	}
}

/// The plotted columns baseline first when one is the baseline every other moves from.
fn against_baseline(table: &TableAnalysis, plan: &ChartPlan, columns: &[TableColumn]) -> Option<Vec<TableColumn>> {
	if columns.len() < 2
		|| columns.len() > MAX_GROUPED
		|| columns.iter().map(|column| column.dim).collect::<HashSet<_>>().len() > 1
	{
		return None;
	}
	let marked = columns
		.iter()
		.find(|column| columns.iter().all(|other| other.index == column.index || is_transition(&column.header, &other.header)));
	let baseline = match marked {
		Some(marked) if sole_mark(columns, marked) => Some(marked.clone()),
		Some(marked) => leading_column(table, plan, columns),
		None => None,
	};
	let baseline = baseline?;
	let grid: Vec<Vec<f64>> = columns
		.iter()
		.map(|column| {
			table.rows
				.iter()
				.map(|&row| {
					column
						.cells
						.get(row)
						.and_then(|cell| cell.as_number())
						.filter(|number| number.value > 0.0)
						.map_or(f64::NAN, |number| number.value)
				})
				.collect()
		})
		.collect();
	if pair_spread(&grid) < ROW_SPREAD {
		return None;
	}
	let mut ordered = vec![baseline.clone()];
	ordered.extend(columns.iter().filter(|column| column.index != baseline.index).cloned());
	Some(ordered)
}

/// Measure columns pairing a before condition with another over the same metric.
fn metric_pairs(measures: &[TableColumn]) -> Vec<(TableColumn, TableColumn)> {
	let pairs: Vec<(TableColumn, TableColumn)> = Vec::new();
	let mut pairs = pairs;
	for before in measures {
		if !BEFORE_RE.is_match(&before.header) {
			continue;
		}
		let subject: Vec<String> = words(&before.header)
			.into_iter()
			.filter(|word| !BEFORE_RE.is_match(word))
			.collect();
		if subject.is_empty() {
			continue;
		}
		let partners: Vec<&TableColumn> = measures
			.iter()
			.filter(|column| {
				if column.index == before.index || column.dim != before.dim || BEFORE_RE.is_match(&column.header) {
					return false;
				}
				let own = words(&column.header);
				let extra = own.iter().filter(|word| !subject.contains(word)).count();
				subject.iter().all(|word| own.contains(word)) && extra >= 1 && extra <= 2
			})
			.collect();
		if partners.len() > 1 {
			return Vec::new();
		}
		if let Some(after) = partners.first() {
			if !pairs.iter().any(|(_, taken)| taken.index == after.index) {
				pairs.push((before.clone(), (*after).clone()));
			}
		}
	}
	pairs
}


/// Three to [`MAX_STAGES`] adjacent same-unit measures holding one quantity at
/// successive stages of a run; `None` when no such run exists.
fn stage_columns(measures: &[TableColumn], rows: &[usize]) -> Option<Vec<TableColumn>> {
	let near = |first: &TableColumn, next: &TableColumn| -> bool {
		let mut ratios: Vec<f64> = rows
			.iter()
			.flat_map(|&row| {
				let a = first.cells.get(row).and_then(|cell| cell.as_number());
				let b = next.cells.get(row).and_then(|cell| cell.as_number());
				if a.map_or(false, |a| a.value > 0.0) && b.map_or(false, |b| b.value > 0.0) {
					let a = a.expect("checked");
					let b = b.expect("checked");
					Some(a.value.max(b.value) / a.value.min(b.value))
				} else {
					None
				}
			})
			.collect();
		ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		!ratios.is_empty() && ratios[ratios.len() >> 1] < QUANTITY_SPREAD
	};
	for (at, first) in measures.iter().enumerate() {
		if !BEFORE_RE.is_match(&first.header) || first.mixed || DELTA_RE.is_match(&first.header) || first.arrow_share >= 0.6 {
			continue;
		}
		let mut run = vec![first.clone()];
		for next in measures.iter().skip(at + 1) {
			if run.len() == MAX_STAGES || next.index != run.last().expect("run").index + 1 {
				break;
			}
			if next.dim != first.dim
				|| next.unit != first.unit
				|| next.mixed
				|| next.arrow_share >= 0.6
				|| BEFORE_RE.is_match(&next.header)
				|| DELTA_RE.is_match(&next.header)
				|| AGGREGATE_RE.is_match(&next.header)
				|| !STAGE_HEADER_RE.is_match(&next.header)
				|| !near(first, next)
			{
				break;
			}
			run.push(next.clone());
		}
		if run.len() >= 3 {
			return Some(run);
		}
	}
	None
}

/// Two same-unit measures of opposing amounts, the losing side first; `None`
/// when no such pair exists.
fn opposed_columns(measures: &[TableColumn]) -> Option<(TableColumn, TableColumn)> {
	let plain: Vec<&TableColumn> = measures
		.iter()
		.filter(|column| {
			column.dim.is_some()
				&& OPPOSED_DIMS.contains(&column.dim.expect("dim"))
				&& !column.mixed
				&& !column.scores
				&& column.arrow_share < 0.6
				&& (!DELTA_RE.is_match(&column.header)
					|| OPPOSED.iter().any(|(_, gains)| gains.is_match(&column.header)))
				&& !AGGREGATE_RE.is_match(&column.header)
		})
		.collect();
	for (loses, gains) in OPPOSED.iter() {
		let loss = plain.iter().find(|column| loses.is_match(&column.header) && !gains.is_match(&column.header));
		let gain = plain.iter().find(|column| gains.is_match(&column.header) && !loses.is_match(&column.header));
		if let (Some(loss), Some(gain)) = (loss, gain) {
			if loss.dim == gain.dim && loss.unit == gain.unit {
				return Some(((*loss).clone(), (*gain).clone()));
			}
		}
	}
	None
}

/// Per category, the series with its best value among the rival columns;
/// `None` where tied or missing.
fn column_leaders(categories: &[String], series: &[ChartSeries]) -> Option<Vec<Option<usize>>> {
	let mut kinds: HashMap<String, Vec<usize>> = HashMap::new();
	for (index, entry) in series.iter().enumerate() {
		let Some(polarity) = entry.polarity else { continue };
		kinds
			.entry(format!("{}:{}:{}", match polarity { Polarity::Higher => "higher", Polarity::Lower => "lower" }, entry.dim.as_str(), entry.unit))
			.or_default()
			.push(index);
	}
	let mut rivals: Vec<&Vec<usize>> = kinds.values().collect();
	rivals.sort_by(|a, b| b.len().cmp(&a.len()));
	let rivals = rivals.first()?;
	if rivals.len() < 2 {
		return None;
	}
	let sign = if series[*rivals.first().expect("rivals")].polarity == Some(Polarity::Higher) { 1.0 } else { -1.0 };
	Some(
		categories
			.iter()
			.enumerate()
			.map(|(at, _)| {
				let mut ranked: Vec<(usize, f64)> = rivals
					.iter()
					.copied()
					.filter_map(|index| {
						series[index]
							.points
							.get(at)
							.and_then(|point| point.as_ref())
							.map(|point| (index, sign * point.value))
					})
					.collect();
				ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
				if ranked.len() >= 2 && ranked[0].1 != ranked[1].1 {
					Some(ranked[0].0)
				} else {
					None
				}
			})
			.collect(),
	)
}

/// The category holding `series`' best value by its polarity among `rivals`
/// (all by default): one of at least `least` values, not tied with another.
fn best_point(series: &ChartSeries, rivals: Option<&[bool]>, least: usize) -> Option<usize> {
	let polarity = series.polarity?;
	let sign = if polarity == Polarity::Higher { 1.0 } else { -1.0 };
	let mut ranked: Vec<(usize, f64)> = series
		.points
		.iter()
		.enumerate()
		.flat_map(|(at, point)| {
			let point = point.as_ref()?;
			if rivals.map_or(true, |rivals| rivals.get(at).copied().unwrap_or(true)) {
				Some((at, sign * point.value))
			} else {
				None
			}
		})
		.collect();
	ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
	if ranked.len() < least || ranked.len() > MAX_RIVALS {
		return None;
	}
	if ranked[0].1 == ranked[1].1 {
		return None;
	}
	Some(ranked[0].0)
}


/// Facts `spec` could state as its headline, unranked.
fn chart_facts(spec: &ChartSpec) -> Vec<Fact> {
	let mut facts: Vec<Fact> = Vec::new();
	let name = |at: usize| spec.categories.get(at).cloned().unwrap_or_default();
	if spec.kind == ChartKind::Change || (spec.kind == ChartKind::Dots && spec.baseline.is_some()) {
		for entry in spec.series.iter().take(TAKEAWAY_SERIES) {
			let mut moves: Vec<(usize, ChartPoint)> = entry
				.points
				.iter()
				.enumerate()
				.filter_map(|(at, point)| point.as_ref().filter(|point| point.value > 0.0).map(|point| (at, point.clone())))
				.collect();
			moves.sort_by(|a, b| {
				(b.1.value.ln().abs())
					.partial_cmp(&a.1.value.ln().abs())
					.unwrap_or(std::cmp::Ordering::Equal)
			});
			let lead = if spec.series.len() > 1 { format!("{} ", entry.name) } else { String::new() };
			for (at, point) in moves.iter().take(2) {
				let words = factor_words(point.value);
				if words == "same" {
					continue;
				}
				let weight = point.value.ln().abs();
				facts.push(Fact {
					key: format!("move:{}:{}", entry.name, name(*at)),
					text: format!(
						"{}: {}{}{} ({})",
						name(*at),
						lead,
						words,
						if spec.rival.is_some() {
							format!(" vs {}", spec.baseline.clone().unwrap_or_default())
						} else {
							String::new()
						},
						point.text
					),
					weight,
					striking: weight >= STRIKING_MOVE.ln(),
				});
			}
			if spec.rival.is_some() {
				if let Some(gap) = rival_gap(spec, entry) {
					facts.push(gap);
				}
				continue;
			}
			let factors: Vec<Option<f64>> = entry
				.points
				.iter()
				.map(|point| point.as_ref().map(|point| point.value))
				.collect();
			let polarities: Vec<Option<Polarity>> = entry
				.points
				.iter()
				.enumerate()
				.map(|(at, _)| {
					spec.category_polarity
						.as_ref()
						.and_then(|polarity| polarity.get(at).copied().flatten())
						.or(entry.polarity)
				})
				.collect();
			let span = if spec.series.len() > 1 { format!(" on {}", entry.name) } else { String::new() };
			let consensus = consensus_move(
				spec,
				&factors,
				Some(&polarities),
				&format!("every {}", row_noun(spec)),
				&span,
			);
			if let Some(consensus) = consensus {
				facts.push(Fact { key: format!("moves:{}", entry.name), ..consensus });
			}
		}
	} else if (spec.kind == ChartKind::Paired || spec.kind == ChartKind::Range) && spec.series.len() == 2 {
		let before = spec.series[0].clone();
		let after = spec.series[1].clone();
		if (spec.kind == ChartKind::Range && spec.transition.unwrap_or(false))
			|| (spec.kind == ChartKind::Paired && is_transition(&before.name, &after.name))
		{
			let value = |point: &ChartPoint| if spec.kind == ChartKind::Range { interval_middle(point) } else { point.value };
			let factors: Vec<Option<f64>> = spec
				.categories
				.iter()
				.enumerate()
				.map(|(at, _)| {
					let a = before.points.get(at).and_then(|point| point.as_ref())?;
					let b = after.points.get(at).and_then(|point| point.as_ref())?;
					if value(a) > 0.0 {
						Some(value(b) / value(a))
					} else if value(b) == 0.0 {
						Some(1.0)
					} else {
						None
					}
				})
				.collect();
			for (at, factor) in factors.iter().enumerate() {
				let Some(factor) = factor else { continue };
				let words = factor_words(*factor);
				if words == "same" {
					continue;
				}
				facts.push(Fact {
					key: format!("move:{}", name(at)),
					text: format!(
						"{}: {} → {} ({})",
						name(at),
						before.points[at].as_ref().map_or("", |point| &point.text),
						after.points[at].as_ref().map_or("", |point| &point.text),
						words
					),
					weight: factor.max(1e-9).ln().abs(),
					striking: false,
				});
			}
			let polarity = after.polarity.or(before.polarity);
			let polarities: Vec<Option<Polarity>> = factors.iter().map(|_| polarity).collect();
			let consensus = consensus_move(
				spec,
				&factors,
				Some(&polarities),
				&format!("every {}", row_noun(spec)),
				&format!(" from {} to {}", before.name, after.name),
			)
			.or_else(|| {
				split_move(spec, &factors, Some(&polarities), &|at| {
					format!(
						"{} → {}",
						before.points.get(at).and_then(|point| point.as_ref()).map_or("", |point| &point.text),
						after.points.get(at).and_then(|point| point.as_ref()).map_or("", |point| &point.text)
					)
				})
			});
			if let Some(consensus) = consensus {
				facts.push(consensus);
			}
		} else {
			facts.extend(peer_facts(spec, &before, &after));
		}
	} else if spec.kind == ChartKind::Line {
		for entry in spec.series.iter().take(TAKEAWAY_SERIES) {
			let present: Vec<(usize, ChartPoint)> = entry
				.points
				.iter()
				.enumerate()
				.filter_map(|(at, point)| point.as_ref().map(|point| (at, point.clone())))
				.collect();
			let first = present.first().cloned();
			let last = present.last().cloned();
			let (Some(first), Some(last)) = (first, last) else { continue };
			if first.0 == last.0 || first.1.value <= 0.0 {
				continue;
		}
			let factor = last.1.value / first.1.value;
			facts.push(Fact {
				key: format!("trend:{}", entry.name),
				text: format!(
					"{}: {} → {} from {} to {} ({})",
					entry.name, first.1.text, last.1.text, name(first.0), name(last.0), factor_words(factor)
				),
				weight: factor.ln().abs(),
				striking: false,
			});
		}
		facts.extend(peak_facts(spec));
	} else if spec.kind == ChartKind::Stacked {
		if let Some(whole) = spec.whole.as_ref() {
			facts.extend(extremes(spec, whole));
		}
		let sums: Vec<f64> = spec
			.series
			.iter()
			.map(|entry| entry.points.iter().map(|point| point.as_ref().map_or(0.0, |point| point.value)).sum())
			.collect();
		let grand = spec
			.whole
			.as_ref()
			.map_or_else(|| sums.iter().sum::<f64>(), |whole| {
				whole.points.iter().map(|point| point.as_ref().map_or(0.0, |point| point.value)).sum()
			});
		let lead = sums
			.iter()
			.enumerate()
			.max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
			.map(|(at, _)| at)
			.unwrap_or(0);
		let part = if grand > 0.0 { sums[lead] / grand } else { 0.0 };
		if part >= DOMINANT_PART {
			facts.push(Fact {
				key: format!("part:{}", spec.series[lead].name),
				text: format!(
					"{} is {}% of {}",
					spec.series[lead].name,
					(part * 100.0).round() as i64,
					spec.whole.as_ref().map_or("the total", |whole| &whole.name)
				),
				weight: 1.0 + part,
				striking: false,
			});
		}
		if spec.normalized == Some(true) {
			if let Some(tones) = spec.tones.as_ref() {
				for (index, entry) in spec.series.iter().enumerate() {
					if tones.get(index).copied().flatten() == Some(Status::Good)
						|| !(sums[index] > 0.0)
						|| !(grand > 0.0)
					{
						continue;
					}
					let rates: Vec<f64> = spec
						.categories
						.iter()
						.enumerate()
						.map(|(at, _)| {
							let total = spec
								.whole
								.as_ref()
								.and_then(|whole| whole.points.get(at))
								.and_then(|point| point.as_ref())
								.map_or_else(
									|| {
										spec.series
											.iter()
											.map(|other| {
												other.points.get(at).and_then(|point| point.as_ref()).map_or(0.0, |point| point.value)
											})
											.sum()
									},
									|point| point.value,
								);
							if total > 0.0 {
								entry.points.get(at).and_then(|point| point.as_ref()).map_or(0.0, |point| point.value) / total
							} else {
								0.0
							}
						})
						.collect();
					let top = rates
						.iter()
						.enumerate()
						.max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
						.map(|(at, _)| at)
						.unwrap_or(0);
					facts.push(Fact {
						key: format!("mix:{}", entry.name),
						text: format!(
							"{} has the highest {} rate ({})",
							name(top),
							entry.name,
							percent_text(rates[top] * 100.0)
						),
						weight: 1.0 + (rates[top] / (sums[index] / grand)).ln(),
						striking: false,
					});
				}
			}
		}
	} else if (spec.kind == ChartKind::Timeline || spec.kind == ChartKind::Waterfall) && spec.total.is_some() {
		let total = spec.total.as_ref().expect("total").point.clone();
		facts.extend(flow_facts(spec, &total));
	} else if spec.kind == ChartKind::Bullet && spec.targets.is_some() {
		let targets = spec.targets.as_ref().expect("targets");
		facts.extend(target_facts(spec, targets));
	} else if spec.kind == ChartKind::Stat {
		facts.extend(stat_facts(spec));
	} else if spec.kind == ChartKind::Slope && spec.series.len() >= 2 {
		facts.extend(slope_facts(spec));
	} else if spec.kind == ChartKind::Ladder {
		facts.extend(ladder_facts(spec));
	} else if spec.kind == ChartKind::Histogram {
		facts.extend(histogram_facts(spec));
	} else if spec.kind == ChartKind::Butterfly && spec.series.len() == 2 {
		facts.extend(butterfly_facts(spec));
	} else if spec.kind == ChartKind::Share {
		let top = by_value(&spec.series[0].points).into_iter().next();
		let total: f64 = spec.series[0]
			.points
			.iter()
			.map(|point| point.as_ref().map_or(0.0, |point| point.value.max(0.0)))
			.sum();
		let share = match &top {
			Some((_, point)) if spec.series[0].dim != Dimension::Percent && total > 0.0 => {
				format!("{}, {}", point.text, percent_text((100.0 * point.value) / total))
			}
			Some((_, point)) => point.text.clone(),
			None => String::new(),
		};
		let majority = majority_part(spec, &spec.series[0]);
		if let Some(majority) = majority {
			facts.push(majority);
		} else if let Some((at, _)) = top {
			facts.push(Fact {
				key: "share:top".to_string(),
				text: format!("{} is the largest share ({})", name(at), share),
				weight: 1.0,
				striking: false,
			});
		}
	} else {
		for (index, entry) in spec.series.iter().take(TAKEAWAY_SERIES).enumerate() {
			for fact in extremes(spec, entry) {
				facts.push(Fact { weight: fact.weight - 0.2 * index as f64, ..fact });
			}
		}
		facts.extend(rivalry(spec));
		facts.extend(reference_moves(spec));
	}
	facts
}

static MIDDLE_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?i)^(?:p50|median|mean|avg)\b").expect("valid middle regex"));

static WHOLE_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?i)^(?:share|%|percent(?:age)?)\s+of\s+").expect("valid whole regex"));

/// A slope's stages as takeaways.
fn slope_facts(spec: &ChartSpec) -> Vec<Fact> {
	let first = spec.series.first().cloned();
	let previous = spec.series.len().checked_sub(2).and_then(|at| spec.series.get(at).cloned());
	let last = spec.series.last().cloned();
	let (Some(first), Some(previous), Some(last)) = (first, previous, last) else {
		return Vec::new();
	};
	let polarity = last.polarity.or(first.polarity);
	let factors: Vec<Option<f64>> = spec
		.categories
		.iter()
		.enumerate()
		.map(|(at, _)| {
			let a = first.points.get(at).and_then(|point| point.as_ref())?;
			let b = last.points.get(at).and_then(|point| point.as_ref())?;
			if a.value > 0.0 {
				Some(b.value / a.value)
			} else if b.value == 0.0 {
				Some(1.0)
			} else {
				None
			}
		})
		.collect();
	let polarities: Vec<Option<Polarity>> = factors.iter().map(|_| polarity).collect();
	let moves = consensus_move(
		spec,
		&factors,
		Some(&polarities),
		&format!("every {}", row_noun(spec)),
		&format!(" from {} to {}", first.name, last.name),
	)
	.or_else(|| {
		split_move(spec, &factors, Some(&polarities), &|at| {
			format!(
				"{} → {}",
				first.points.get(at).and_then(|point| point.as_ref()).map_or("", |point| &point.text),
				last.points.get(at).and_then(|point| point.as_ref()).map_or("", |point| &point.text)
			)
		})
	});
	let turns: Vec<(usize, f64)> = spec
		.categories
		.iter()
		.enumerate()
		.flat_map(|(at, _)| {
			let a = spec.series.len().checked_sub(3).and_then(|at| spec.series.get(at))?;
			let a = a.points.get(at).and_then(|point| point.as_ref())?;
			let b = previous.points.get(at).and_then(|point| point.as_ref())?;
			let c = last.points.get(at).and_then(|point| point.as_ref())?;
			if a.value <= 0.0 || b.value <= 0.0 || c.value <= 0.0 {
				return None;
			}
			let before = b.value / a.value;
			let after = c.value / b.value;
			let turned = (before < 1.0 && after > 1.0) || (before > 1.0 && after < 1.0);
			if turned && after.abs().ln().abs() >= STRIKING_MOVE.ln() {
				Some((at, after))
			} else {
				None
			}
		})
		.collect();
	if turns.len() != 1 {
		return moves.into_iter().collect();
	}
	let (at, after) = turns[0];
	let verb = match polarity {
		Some(polarity) if (after > 1.0) == (polarity == Polarity::Higher) => "improved",
		Some(_) => "regressed",
		None if after > 1.0 => "rose",
		None => "fell",
	};
	let turn = format!(
		"{} {} from {} to {} at {}",
		spec.categories.get(at).cloned().unwrap_or_default(),
		verb,
		previous.points.get(at).and_then(|point| point.as_ref()).map_or("", |point| &point.text),
		last.points.get(at).and_then(|point| point.as_ref()).map_or("", |point| &point.text),
		last.name
	);
	let weight = CONSENSUS_WEIGHT + after.abs().ln().abs();
	let named = moves
		.as_ref()
		.map_or(false, |moves| moves.text.contains("; ") && !moves.text.contains(", none "));
	if moves.is_none() || named {
		let mut facts = moves.into_iter().collect::<Vec<_>>();
		facts.push(Fact {
			key: format!("turn:{}", spec.categories.get(at).cloned().unwrap_or_default()),
			text: turn,
			weight: weight - 1.0,
			striking: false,
		});
		return facts;
	}
	let moves = moves.expect("moves");
	let head = moves
		.text
		.split(", none ")
		.next()
		.unwrap_or("")
		.split("; ")
		.next()
		.unwrap_or("")
		.to_string();
	vec![Fact { key: "moves".to_string(), text: format!("{}; {}", head, turn), weight, striking: true }]
}

/// A quantile ladder's takeaways.
fn ladder_facts(spec: &ChartSpec) -> Vec<Fact> {
	let Some(ladder) = spec.ladders.as_ref().and_then(|ladders| ladders.first()) else {
		return Vec::new();
	};
	if ladder.series.len() < 2 {
		return Vec::new();
	}
	let rungs: Vec<&ChartSeries> = ladder.series.iter().filter_map(|&index| spec.series.get(index)).collect();
	if rungs.is_empty() {
		return Vec::new();
	}
	let middle = rungs
		.iter()
		.find(|entry| MIDDLE_RE.is_match(&entry.name))
		.copied()
		.unwrap_or(rungs[0]);
	let top = rungs.last().copied().unwrap_or(middle);
	let mut facts: Vec<Fact> = extremes(spec, middle)
		.into_iter()
		.filter(|fact| !fact.key.starts_with("outlier:"))
		.collect();
	if !std::ptr::eq(top, middle) {
		let mut tails: Vec<(usize, f64)> = spec
			.categories
			.iter()
			.enumerate()
			.flat_map(|(at, _)| {
				let m = middle.points.get(at).and_then(|point| point.as_ref())?;
				let t = top.points.get(at).and_then(|point| point.as_ref())?;
				if m.value > 0.0 {
					Some((at, t.value / m.value))
				} else {
					None
				}
			})
			.collect();
		tails.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
		if let Some(&(at, ratio)) = tails.first() {
			if ratio >= STRIKING && tails.len() >= 2 {
				let next = tails.get(1).map(|&(_, ratio)| ratio);
				let apart = next.map_or(false, |next| ratio >= OUTLIER * next);
				facts.push(Fact {
					key: format!("tail:{}", ladder.name),
					text: format!(
						"{} has the longest tail ({} {}× its {})",
						spec.categories.get(at).cloned().unwrap_or_default(),
						top.name,
						round_factor(ratio),
						middle.name
					),
					weight: (ratio / next.unwrap_or(1.0)).ln() + 1.0,
					striking: apart,
				});
			}
		}
	}
	let after = if spec.transition == Some(true) {
		spec.ladders.as_ref().and_then(|ladders| ladders.get(1))
	} else {
		None
	};
	if let Some(after) = after {
	if let Some(moved) = after
			.series
			.iter()
			.filter_map(|&index| spec.series.get(index))
			.find(|entry| entry.name == middle.name)
		{
			let factors: Vec<Option<f64>> = middle
				.points
				.iter()
				.enumerate()
				.map(|(at, point)| {
					let point = point.as_ref()?;
					let end = moved.points.get(at).and_then(|point| point.as_ref())?;
					if point.value > 0.0 {
						Some(end.value / point.value)
					} else {
						None
					}
				})
				.collect();
			let polarities: Vec<Option<Polarity>> = factors.iter().map(|_| moved.polarity.or(middle.polarity)).collect();
			if let Some(move_) = consensus_move(
				spec,
				&factors,
				Some(&polarities),
				&format!("every {}'s {}", row_noun(spec), middle.name),
				&format!(" from {} to {}", ladder.name, after.name),
			) {
				facts.push(move_);
			}
		}
	}
	facts
}

/// A histogram's takeaways.
fn histogram_facts(spec: &ChartSpec) -> Vec<Fact> {
	let bin = |at: usize| {
		if spec.axis.is_empty() {
			spec.categories.get(at).cloned().unwrap_or_default()
		} else {
			format!("{} = {}", spec.axis, spec.categories.get(at).cloned().unwrap_or_default())
		}
	};
	let shares: Vec<Vec<Option<f64>>> = spec
		.series
		.iter()
		.map(|entry| {
			let total: f64 = entry
				.points
				.iter()
				.map(|point| point.as_ref().map_or(0.0, |point| point.value.max(0.0)))
				.sum();
			entry
				.points
				.iter()
				.map(|point| {
					point.as_ref().and_then(|point| {
						if total > 0.0 {
							Some(100.0 * point.value.max(0.0) / total)
						} else {
							None
						}
					})
				})
				.collect()
		})
		.collect();
	let mut facts: Vec<Fact> = Vec::new();
	if let Some(entry) = spec.series.first() {
		let ranked: Vec<(usize, f64)> = shares[0]
			.iter()
			.enumerate()
			.filter_map(|(at, share)| share.map(|share| (at, share)))
			.collect();
		let mut ranked = ranked;
		ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
		if let Some(&(at, share)) = ranked.first() {
			if share >= MAJORITY * 100.0 {
				let whole = WHOLE_RE.replace(&entry.name, "").to_string();
				facts.push(Fact {
					key: format!("mode:{}", entry.name),
					text: format!("{} holds {} of {}", bin(at), percent_text(share), whole),
					weight: 1.0 + share / 100.0,
					striking: ranked.get(1).map_or(false, |&(_, next)| share >= 2.0 * next),
				});
			}
		}
	}
	if spec.series.len() == 2 {
		let from = &spec.series[0];
		let to = &spec.series[1];
		let mut shifts: Vec<(usize, f64, f64)> = spec
			.categories
			.iter()
			.enumerate()
			.filter_map(|(at, _)| {
				let a = shares[0].get(at).copied().flatten()?;
				let b = shares[1].get(at).copied().flatten()?;
				Some((at, a, b))
			})
			.collect();
		shifts.sort_by(|x, y| {
			(y.2 - y.1)
				.abs()
				.partial_cmp(&(x.2 - x.1).abs())
				.unwrap_or(std::cmp::Ordering::Equal)
		});
		if let Some(&(at, a, b)) = shifts.first() {
			if (b - a).abs() >= 1.0 {
				let written = |entry: &ChartSeries, share: f64| {
					if entry.dim == Dimension::Percent {
						entry.points.get(at).and_then(|point| point.as_ref()).map_or(String::new(), |point| point.text.clone())
					} else {
						percent_text(share)
					}
				};
				facts.push(Fact {
					key: format!("shift:{}", spec.categories.get(at).cloned().unwrap_or_default()),
					text: format!("{}: {} → {} from {} to {}", bin(at), written(from, a), written(to, b), from.name, to.name),
					weight: CONSENSUS_WEIGHT + (b - a).abs() / 100.0,
					striking: (b - a).abs() >= SHIFT_POINTS,
				});
			}
		}
	}
	facts
}

/// A butterfly's takeaway.
fn butterfly_facts(spec: &ChartSpec) -> Vec<Fact> {
	let sums: Vec<f64> = spec
		.series
		.iter()
		.map(|entry| entry.points.iter().map(|point| point.as_ref().map_or(0.0, |point| point.value.max(0.0))).sum())
		.collect();
	let (low, high) = if sums[0] > sums[1] { (1, 0) } else { (0, 1) };
	let big = &spec.series[high];
	let small = &spec.series[low];
	if !(sums[high] > 0.0) {
		return Vec::new();
	}
	let figure = |entry: &ChartSeries, sum: f64| {
		let points: Vec<&ChartPoint> = entry.points.iter().flatten().collect();
		if sum_figure(&points).is_empty() {
			locale_fixed(sum, 0, true)
		} else {
			sum_figure(&points)
		}
	};
	let part = majority_part(spec, big);
	let lead = part.as_ref().map_or(String::new(), |part| {
		let text = match part.text.find(" of ") {
			Some(at) => format!("{} of {}", &part.text[..at], big.name),
			None => part.text.clone(),
		};
		format!("; {}", text)
	});
	let factor = if sums[low] > 0.0 { sums[high] / sums[low] } else { f64::INFINITY };
	vec![Fact {
		key: "net".to_string(),
		text: format!("{} {} vs {} {}{}", big.name, figure(big, sums[high]), small.name, figure(small, sums[low]), lead),
		weight: CONSENSUS_WEIGHT + factor.ln().min(5.0),
		striking: factor >= STRIKING_MOVE,
	}]
}

/// The largest of a share's parts when it makes up a [`MAJORITY`] of their sum.
fn majority_part(spec: &ChartSpec, entry: &ChartSeries) -> Option<Fact> {
	if entry.points.iter().any(|point| point.as_ref().map_or(true, |point| point.value < 0.0)) {
		return None;
	}
	let ranked = by_value(&entry.points);
	let total: f64 = entry.points.iter().map(|point| point.as_ref().map_or(0.0, |point| point.value)).sum();
	let top = ranked.first()?.clone();
	let next = ranked.get(1).cloned();
	if !(total > 0.0) || top.1.value / total < MAJORITY {
		return None;
	}
	let part = top.1.value / total;
	let sum = if entry.dim == Dimension::Percent {
		String::new()
	} else {
		let points: Vec<&ChartPoint> = entry.points.iter().flatten().collect();
		sum_figure(&points)
	};
	let whole = if sum.len() >= 4 && sum.chars().all(|character| character.is_ascii_digit()) {
		locale_fixed(sum.parse().unwrap_or(0.0), 0, true)
	} else if sum.is_empty() {
		"the total".to_string()
	} else {
		sum
	};
	Some(Fact {
		key: "share:top".to_string(),
		text: format!(
			"{} is {} of {}",
			spec.categories.get(top.0).cloned().unwrap_or_default(),
			percent_text(part * 100.0),
			whole
		),
		weight: 1.0 + part,
		striking: next.map_or(false, |(_, point)| top.1.value >= 2.0 * point.value),
	})
}

/// A line chart's interior peaks as takeaways.
fn peak_facts(spec: &ChartSpec) -> Vec<Fact> {
	let mut peaks: Vec<(Peak, &ChartSeries, f64)> = Vec::new();
	for entry in spec.series.iter().take(TAKEAWAY_SERIES) {
		if let Some(peak) = line_peak(spec, entry) {
			let weight = peak.rise.ln() + 1.0 + if peak.point.emphasis { 0.5 } else { 0.0 };
			peaks.push((peak, entry, weight));
		}
	}
	let mut facts: Vec<Fact> = Vec::new();
	let mut ats: Vec<usize> = peaks.iter().map(|(peak, _, _)| peak.at).collect();
	ats.sort_unstable();
	ats.dedup();
	for at in ats {
		let shared: Vec<&(Peak, &ChartSeries, f64)> = peaks.iter().filter(|(peak, _, _)| peak.at == at).collect();
		let where_ = spec.categories.get(at).cloned().unwrap_or_default();
		if shared.len() == 1 {
			let (peak, entry, weight) = shared[0];
			facts.push(Fact {
				key: format!("peak:{}", entry.name),
				text: format!("{} peaked at {} in {}", entry.name, peak.point.text, where_),
				weight: *weight,
				striking: peak.striking,
			});
			continue;
		}
		let named = if shared.len() == 2 {
			let names: Vec<String> = shared
				.iter()
				.map(|(peak, entry, _)| format!("{} ({})", entry.name, peak.point.text))
				.collect();
			list(&names)
		} else if shared.len() == spec.series.len() {
			format!("all {} measures", shared.len())
		} else {
			format!("{} of {} measures", shared.len(), spec.series.len())
		};
		let weight = shared.iter().map(|(_, _, weight)| *weight).fold(f64::MIN, f64::max) + 0.5;
		facts.push(Fact {
			key: format!("peak:@{}", where_),
			text: format!(
				"{} was the peak {} {}",
				where_,
				if shared.len() == 2 { "of" } else { "on" },
				named
			),
			weight,
			striking: shared.iter().any(|(peak, _, _)| peak.striking),
		});
	}
	facts
}

/// The highest sample of a line when it lies inside the line and stands above both ends.
fn line_peak(spec: &ChartSpec, entry: &ChartSeries) -> Option<Peak> {
	let mut order: Vec<usize> = (0..spec.categories.len()).collect();
	if let Some(x) = spec.x.as_ref() {
		order.sort_by(|&a, &b| {
			x.values
				.get(a)
				.unwrap_or(&0.0)
				.partial_cmp(x.values.get(b).unwrap_or(&0.0))
				.unwrap_or(std::cmp::Ordering::Equal)
		});
	}
	let present: Vec<(usize, &ChartPoint)> = order
		.iter()
		.filter_map(|&at| entry.points.get(at).and_then(|point| point.as_ref()).map(|point| (at, point)))
		.collect();
	if present.len() < 3 {
		return None;
	}
	let top = present
		.iter()
		.copied()
		.max_by(|a, b| a.1.value.partial_cmp(&b.1.value).unwrap_or(std::cmp::Ordering::Equal))?;
	let ends = present.first()?.1.value.max(present.last()?.1.value);
	if top.0 == present.first()?.0 || top.0 == present.last()?.0 || ends <= 0.0 {
		return None;
	}
	let rise = top.1.value / ends;
	let striking = rise >= PEAK_RISE && present.len() >= MIN_PEAK_SAMPLES;
	if rise >= MIN_PEAK_RISE {
		Some(Peak { at: top.0, point: top.1.clone(), rise, striking, series: None })
	} else {
		None
	}
}

/// `facts` strongest first, one per key, at most [`MAX_TAKEAWAYS`].
fn rank_facts(mut facts: Vec<Fact>) -> Vec<Takeaway> {
	facts.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
	let mut seen: HashSet<String> = HashSet::new();
	facts
		.into_iter()
		.filter(|fact| seen.insert(fact.key.clone()))
		.take(MAX_TAKEAWAYS)
		.map(|fact| Takeaway { key: fact.key, text: fact.text, striking: fact.striking })
		.collect()
}

/// Facts across rivals' best values.
fn rivalry(spec: &ChartSpec) -> Vec<Fact> {
	let mut facts: Vec<Fact> = Vec::new();
	let mut bests: Vec<(usize, Vec<&ChartSeries>)> = Vec::new();
	for entry in &spec.series {
		let Some(best) = entry.best else { continue };
		if let Some(slot) = bests.iter_mut().find(|(at, _)| *at == best) {
			slot.1.push(entry);
		} else {
			bests.push((best, vec![entry]));
		}
	}
	for (at, won) in &bests {
		if won.len() < 2 {
			continue;
		}
		let measures = if won.len() <= 3 {
			let names: Vec<String> = won.iter().map(|entry| entry.name.clone()).collect();
			list(&names)
		} else if won.len() == spec.series.len() {
			format!("all {} measures", won.len())
		} else {
			format!("{} of {} measures", won.len(), spec.series.len())
		};
		let conditions = won.iter().all(|entry| entry.dim == won[0].dim && entry.unit == won[0].unit)
			&& is_transition(&won[0].name, &won.last().expect("won").name);
		facts.push(Fact {
			key: format!("sweep:{}", spec.categories.get(*at).cloned().unwrap_or_default()),
			text: format!("{} is best on {}", spec.categories.get(*at).cloned().unwrap_or_default(), measures),
			weight: if conditions { 1.0 } else { RIVALRY_WEIGHT + 0.3 * won.len() as f64 },
			striking: !conditions,
		});
	}
	for gain in spec.series.iter().filter(|entry| entry.polarity == Some(Polarity::Higher) && entry.best.is_some()) {
		let top = gain.best.expect("best");
		for price in spec.series.iter().filter(|entry| entry.polarity == Some(Polarity::Lower)) {
			let costliest = by_value(&price.points);
			let Some(&(costliest_at, _)) = costliest.first() else { continue };
			if costliest_at != top {
				continue;
			}
			if costliest.get(1).map_or(false, |(_, point)| point.value == costliest[0].1.value) {
				continue;
			}
			facts.push(Fact {
				key: format!("tradeoff:{}:{}", gain.name, price.name),
				text: format!(
					"{} has the highest {} ({}) but also the highest {} ({})",
					spec.categories.get(top).cloned().unwrap_or_default(),
					gain.name,
					gain.points.get(top).and_then(|point| point.as_ref()).map_or("", |point| &point.text),
					price.name,
					price.points.get(top).and_then(|point| point.as_ref()).map_or("", |point| &point.text)
				),
				weight: RIVALRY_WEIGHT + 0.7,
				striking: true,
			});
		}
	}
	let leaders = spec.leaders.clone().unwrap_or_default();
	let decided: Vec<usize> = leaders.iter().flatten().copied().collect();
	if decided.len() >= 3 {
		let mut wins: Vec<(usize, usize)> = Vec::new();
		for &lead in &decided {
			if let Some(slot) = wins.iter_mut().find(|(index, _)| *index == lead) {
				slot.1 += 1;
			} else {
				wins.push((lead, 1));
			}
		}
		wins.sort_by(|a, b| b.1.cmp(&a.1));
		let &(index, count) = wins.first().expect("wins");
		if count * 3 >= decided.len() * 2 {
			let text = if count == leaders.len() {
				format!("{} is best in every row", spec.series[index].name)
			} else {
				format!("{} is best in {} of {} rows", spec.series[index].name, count, leaders.len())
			};
			facts.push(Fact {
				key: format!("leader:{}", spec.series[index].name),
				text,
				weight: RIVALRY_WEIGHT + count as f64 / leaders.len() as f64,
				striking: true,
			});
		}
	}
	facts
}

/// One series of a rival-referenced `change` behind or ahead of that rival on every category.
fn rival_gap(spec: &ChartSpec, entry: &ChartSeries) -> Option<Fact> {
	let gaps: Vec<(f64, Option<Polarity>)> = entry
		.points
		.iter()
		.enumerate()
		.filter_map(|(at, point)| {
			let point = point.as_ref()?;
			if point.value > 0.0 {
				Some((
					point.value,
					spec.category_polarity
						.as_ref()
						.and_then(|polarity| polarity.get(at).copied().flatten())
						.or(entry.polarity),
				))
			} else {
				None
			}
		})
		.collect();
	if gaps.len() < 3 || gaps.len() < spec.categories.len() {
		return None;
	}
	let behind = gaps
		.iter()
		.all(|(factor, polarity)| polarity.map_or(false, |polarity| (*factor > 1.0) != (polarity == Polarity::Higher)));
	let ahead = gaps
		.iter()
		.all(|(factor, polarity)| polarity.map_or(false, |polarity| (*factor > 1.0) == (polarity == Polarity::Higher)));
	let verb = if behind {
		"trails"
	} else if ahead {
		"beats"
	} else {
		return None;
	};
	if gaps.iter().any(|(factor, _)| factor.abs().ln().abs() < STILL_FACTOR.ln()) {
		return None;
	}
	let mut sorted: Vec<f64> = gaps.iter().map(|(factor, _)| *factor).collect();
	sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	let median = sorted[sorted.len() >> 1];
	Some(Fact {
		key: format!("gap:{}", entry.name),
		text: format!(
			"{} {} {} on every {} ({})",
			entry.name,
			verb,
			spec.baseline.clone().unwrap_or_default(),
			row_noun(spec),
			factor_range(&sorted)
		),
		weight: RIVALRY_WEIGHT + median.abs().ln().abs(),
		striking: false,
	})
}

/// Every category moving one way by more than [`STILL_FACTOR` (three at least).
fn consensus_move(
	spec: &ChartSpec,
	factors: &[Option<f64>],
	polarities: Option<&[Option<Polarity>]>,
	subject: &str,
	span: &str,
) -> Option<Fact> {
	let moved: Vec<(usize, f64)> = factors
		.iter()
		.enumerate()
		.filter_map(|(at, factor)| {
			let factor = (*factor)?;
			if factor > 0.0 && factor.abs().ln().abs() >= STILL_FACTOR.ln() {
				Some((at, factor))
			} else {
				None
			}
		})
		.collect();
	let measured = factors.iter().filter(|factor| factor.map_or(false, |factor| factor > 0.0)).count();
	if moved.len() < 3 || moved.len() < measured {
		return None;
	}
	let down = moved.iter().all(|(_, factor)| *factor < 1.0);
	if !down && !moved.iter().all(|(_, factor)| *factor > 1.0) {
		return None;
	}
	let tones: Vec<Option<bool>> = moved
		.iter()
		.map(|(at, factor)| {
			polarities
				.and_then(|polarities| polarities.get(*at).copied().flatten())
				.map(|polarity| (*factor > 1.0) == (polarity == Polarity::Higher))
		})
		.collect();
	let verb = if tones.iter().all(|tone| *tone == Some(true)) {
		"improved"
	} else if tones.iter().all(|tone| *tone == Some(false)) {
		"regressed"
	} else if down {
		"fell"
	} else {
		"rose"
	};
	let mut sorted: Vec<f64> = moved.iter().map(|(_, factor)| *factor).collect();
	sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	let median = sorted[sorted.len() / 2];
	let mut chars = subject.chars();
	let head = match chars.next() {
		Some(character) => character.to_uppercase().collect::<String>() + chars.as_str(),
		None => String::new(),
	};
	Some(Fact {
		key: "moves".to_string(),
		text: format!("{} {}{} ({})", head, verb, span, factor_range(&sorted)),
		weight: CONSENSUS_WEIGHT + median.abs().ln().abs(),
		striking: true,
	})
}

/// Every category but one moving one way and that one the other, or most moving one way and none the other.
fn split_move(
	spec: &ChartSpec,
	factors: &[Option<f64>],
	polarities: Option<&[Option<Polarity>]>,
	move_text: &dyn Fn(usize) -> String,
) -> Option<Fact> {
	let moved: Vec<(usize, f64)> = factors
		.iter()
		.enumerate()
		.filter_map(|(at, factor)| {
			let factor = (*factor)?;
			if factor > 0.0 && factor.abs().ln().abs() >= STILL_FACTOR.ln() {
				Some((at, factor))
			} else {
				None
			}
		})
		.collect();
	let toned = moved
		.iter()
		.all(|(at, _)| polarities.map_or(false, |polarities| polarities.get(*at).copied().flatten().is_some()));
	let side = |item: &(usize, f64)| -> bool {
		if toned {
			let polarity = polarities.expect("toned")[item.0];
			(item.1 > 1.0) == (polarity == Some(Polarity::Higher))
		} else {
			item.1 < 1.0
		}
	};
	let (verbs, others): (&str, &str) = if toned { ("improved", "regressed") } else { ("fell", "rose") };
	let ahead: Vec<(usize, f64)> = moved.iter().filter(|item| side(item)).copied().collect();
	let behind: Vec<(usize, f64)> = moved.iter().filter(|item| !side(item)).copied().collect();
	let (major, minor, verb, counter) = if ahead.len() >= behind.len() {
		(ahead, behind, verbs, others)
	} else {
		(behind, ahead, others, verbs)
	};
	let total = factors.iter().filter(|factor| factor.map_or(false, |factor| factor > 0.0)).count();
	if major.len() < 2 || minor.len() > 1 || total * 3 < spec.categories.len() * 2 {
		return None;
	}
	if minor.is_empty() && (moved.len() == total || major.len() * 2 <= total) {
		return None;
	}
	let nouns = plural(&row_noun(spec));
	let strength = |item: &(usize, f64)| item.1.abs().ln().abs();
	if minor.len() == 1 {
		let odd = minor[0];
		return Some(Fact {
			key: "moves".to_string(),
			text: format!(
				"{} of {} {} {}; {} {} ({})",
				major.len(),
				total,
				nouns,
				verb,
				spec.categories.get(odd.0).cloned().unwrap_or_default(),
				counter,
				move_text(odd.0)
			),
			weight: CONSENSUS_WEIGHT - 1.0 + strength(&odd),
			striking: true,
		});
	}
	let lead = major
		.iter()
		.max_by(|a, b| strength(a).partial_cmp(&strength(b)).unwrap_or(std::cmp::Ordering::Equal))?
		.clone();
	Some(Fact {
		key: "moves".to_string(),
		text: format!(
			"{} of {} {} {}, none {}; {} most ({})",
			major.len(),
			total,
			nouns,
			verb,
			counter,
			spec.categories.get(lead.0).cloned().unwrap_or_default(),
			move_text(lead.0)
		),
		weight: CONSENSUS_WEIGHT - 1.0 + strength(&lead),
		striking: true,
	})
}

/// Factors all on one side of 1, as people say their span.
fn factor_range(sorted: &[f64]) -> String {
	if sorted.is_empty() {
		return String::new();
	}
	let down = sorted[sorted.len() - 1] < 1.0;
	let first = sorted[0];
	let last = sorted[sorted.len() - 1];
	let (weak, strong) = if down { (1.0 / last, 1.0 / first) } else { (first, last) };
	let strongest = if down { first } else { last };
	if strong / weak >= WIDE_MOVES {
		return format!("up to {}", factor_words(strongest));
	}
	let low = factor_words(if down { last } else { first });
	let high = factor_words(strongest);
	if low == high {
		return high;
	}
	if down && weak < 2.0 && strong < 10.0 {
		return format!(
			"−{}% to −{}%",
			((1.0 - last) * 100.0).round() as i64,
			((1.0 - first) * 100.0).round() as i64
		);
	}
	if strong >= 2.0 {
		return format!(
			"{}–{}× {}",
			round_factor(weak),
			round_factor(strong),
			if down { "less" } else { "more" }
		);
	}
	format!("{} to {}", low, high)
}

/// Each category's value of `entry` as a factor of its group's reference category.
pub fn reference_factors(spec: &ChartSpec, entry: &ChartSeries) -> Vec<Option<f64>> {
	let mut group_of: Vec<usize> = Vec::new();
	let groups: Vec<ChartGroup> = spec.groups.clone().unwrap_or_else(|| {
		vec![ChartGroup { name: String::new(), members: spec.categories.clone() }]
	});
	for (index, group) in groups.iter().enumerate() {
		for _ in &group.members {
			group_of.push(index);
		}
	}
	spec.categories
		.iter()
		.enumerate()
		.map(|(at, _)| {
			let index = spec.reference.as_ref().and_then(|reference| {
				reference
					.iter()
					.find(|&&index| group_of.get(index) == group_of.get(at))
					.copied()
			});
			let base = index.and_then(|index| entry.points.get(index)).and_then(|point| point.as_ref());
			let point = entry.points.get(at).and_then(|point| point.as_ref());
			if index.is_none() || index == Some(at) || base.is_none() || point.is_none() || base.expect("base").value <= 0.0 {
				None
		} else {
				Some(point.expect("point").value / base.expect("base").value)
			}
		})
		.collect()
}

/// The whole chart moving one way from its groups' reference rows.
fn reference_moves(spec: &ChartSpec) -> Vec<Fact> {
	let Some(reference) = spec.reference.as_ref().and_then(|reference| reference.first().copied()) else {
		return Vec::new();
	};
	let Some(references) = spec.reference.as_ref() else {
		return Vec::new();
	};
	if references.len() < 2 {
		return Vec::new();
	}
	let mut start = 0;
	let mut name = spec.categories.get(reference).cloned().unwrap_or_default();
	if let Some(groups) = spec.groups.as_ref() {
		for group in groups {
			if reference < start + group.members.len() {
				name = group.members.get(reference - start).cloned().unwrap_or_else(|| name.clone());
				break;
			}
			start += group.members.len();
		}
	}
	let mut moves: Vec<(&ChartSeries, Fact)> = Vec::new();
	for entry in spec.series.iter().take(TAKEAWAY_SERIES) {
		let mut axis_spec = spec.clone();
		axis_spec.axis = spec.axis.split(NAME_JOIN).last().unwrap_or(&spec.axis).to_string();
		let subject = format!("every {}", row_noun(&axis_spec));
		let polarities: Vec<Option<Polarity>> = spec.categories.iter().map(|_| entry.polarity).collect();
		if let Some(fact) = consensus_move(
			spec,
			&reference_factors(spec, entry),
			Some(&polarities),
			&subject,
			&format!(" vs {} on {}", name, entry.name),
		) {
			moves.push((entry, fact));
		}
	}
	if moves.len() < 2 {
		return moves
			.into_iter()
			.map(|(entry, mut fact)| {
				fact.key = format!("reference:{}", entry.name);
				fact
			})
			.collect();
	}
	let verb = |fact: &Fact| fact.text.split(" vs ").next().unwrap_or("").to_string();
	let first_verb = verb(&moves[0].1);
	let together: Vec<(&ChartSeries, Fact)> = moves.iter().filter(|(_, fact)| verb(fact) == first_verb).cloned().collect();
	if together.len() < 2 {
		return moves
			.into_iter()
			.map(|(entry, mut fact)| {
				fact.key = format!("reference:{}", entry.name);
				fact
			})
			.collect();
	}
	let names: Vec<String> = together.iter().map(|(entry, _)| entry.name.clone()).collect();
	let weight = together.iter().map(|(_, fact)| fact.weight).fold(f64::MIN, f64::max) + 0.1;
	vec![Fact {
		key: "reference".to_string(),
		text: format!("{} vs {} on {}", first_verb, name, list(&names)),
		weight,
		striking: true,
	}]
}

/// A series' extremes as takeaways.
fn extremes(spec: &ChartSpec, entry: &ChartSeries) -> Vec<Fact> {
	let mut facts: Vec<Fact> = Vec::new();
	let name = |at: usize| spec.categories.get(at).cloned().unwrap_or_default();
	let ranked = by_value(&entry.points);
	if ranked.len() < 3 {
		return facts;
	}
	let top = ranked[0].clone();
	let second = ranked[1].clone();
	let bottom = ranked[ranked.len() - 1].clone();
	if let Some(groups) = spec.groups.as_ref() {
		if groups.len() >= 2 {
			let mut at = 0;
			let mut means: Vec<(String, String, f64)> = Vec::new();
			for group in groups {
				let mut values: Vec<f64> = Vec::new();
				for _ in &group.members {
					if let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) {
						values.push(point.value);
					}
					at += 1;
				}
				let mean = if values.is_empty() {
					f64::NAN
				} else {
					values.iter().sum::<f64>() / values.len() as f64
				};
				means.push((group.name.clone(), group.members.join("\0"), mean));
			}
			let mut sorted: Vec<(String, String, f64)> =
				means.into_iter().filter(|(_, _, mean)| mean.is_finite()).collect();
			sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
			if sorted.len() >= 2 && sorted[1].2 > 0.0 && sorted[0].2 / sorted[1].2 >= OUTLIER {
				let factor = sorted[0].2 / sorted[1].2;
				let alike = sorted[0].1 == sorted[1].1;
				facts.push(Fact {
					key: format!("outlier:{}", entry.name),
					text: format!(
						"{} has {}× the {} of {}",
						sorted[0].0, round_factor(factor), entry.name, sorted[1].0
					),
					weight: factor.ln() + 1.0,
					striking: factor >= STRIKING && alike && entry.polarity.is_none(),
				});
			}
		}
	}
	if second.1.value > 0.0 && top.1.value / second.1.value >= OUTLIER {
		let factor = top.1.value / second.1.value;
		facts.push(Fact {
			key: format!("outlier:{}", entry.name),
			text: format!(
				"{} has {}× the {} of the next ({} vs {})",
				name(top.0),
				round_factor(factor),
				entry.name,
				top.1.text,
				second.1.text
			),
			weight: factor.ln() + 1.0,
			striking: entry.best == Some(top.0) && factor >= STRIKING,
		});
	}
	let tied: Vec<&(usize, ChartPoint)> = ranked.iter().filter(|(_, point)| point.value == top.1.value).collect();
	if tied.len() >= 2 && tied.len() < ranked.len() {
		let names = if tied.len() == 2 {
			format!("{} and {}", name(tied[0].0), name(tied[1].0))
		} else {
			format!("{} rows", tied.len())
		};
		facts.push(Fact {
			key: format!("tie:{}", entry.name),
			text: format!("{} tie for the highest {} ({})", names, entry.name, top.1.text),
			weight: if entry.polarity == Some(Polarity::Higher) { 0.9 } else { 0.3 },
			striking: false,
		});
	} else if tied.len() == 1 {
		facts.push(Fact {
			key: format!("top:{}", entry.name),
			text: format!("{} has the highest {} ({})", name(top.0), entry.name, top.1.text),
			weight: if entry.polarity == Some(Polarity::Higher) { 0.9 } else { 0.3 },
			striking: false,
		});
	}
	if entry.polarity == Some(Polarity::Lower)
		&& ranked.iter().filter(|(_, point)| point.value == bottom.1.value).count() == 1
	{
		facts.push(Fact {
			key: format!("low:{}", entry.name),
			text: format!("{} has the lowest {} ({})", name(bottom.0), entry.name, bottom.1.text),
			weight: 0.9,
			striking: false,
		});
	}
	facts
}

/// Most rows missing their targets a takeaway names; past it, it names the worst.
const MAX_MISSES_NAMED: usize = 2;
/// Share of a column's values that must fall for it to read as a falling measure.
const FALL: f64 = 0.5;
/// Fewest values a stage column must hold inside and outside a run.
const MIN_CELLS: usize = 2;

static CHANGES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bchanges\b").expect("valid changes regex"));

/// What a run of steps is mostly made of.
fn flow_facts(spec: &ChartSpec, total: &ChartPoint) -> Vec<Fact> {
	let Some(entry) = spec.series.first() else { return Vec::new() };
	if total.value <= 0.0 {
		return Vec::new();
	}
	let mut at = 0;
	let mut units: Vec<(String, f64, bool)> = Vec::new();
	if let Some(groups) = spec.groups.as_ref() {
		for group in groups {
			let start = at;
			let mut sum = 0.0;
			for _ in &group.members {
				if let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) {
					sum += point.value;
				}
				at += 1;
			}
			if !group.name.is_empty() && group.members.len() > 1 {
				units.push((group.name.clone(), sum, false));
			} else {
				for (index, _) in group.members.iter().enumerate() {
					let point = entry.points.get(start + index).and_then(|point| point.as_ref());
					units.push((
						spec.categories.get(start + index).cloned().unwrap_or_default(),
						point.map_or(0.0, |point| point.value),
						point.map_or(false, |point| point.emphasis),
					));
				}
			}
		}
	} else {
		for (index, name) in spec.categories.iter().enumerate() {
			let point = entry.points.get(index).and_then(|point| point.as_ref());
			units.push((
				name.clone(),
				point.map_or(0.0, |point| point.value),
				point.map_or(false, |point| point.emphasis),
			));
		}
	}
	if units.iter().any(|(_, value, _)| *value < 0.0) {
		return Vec::new();
	}
	units.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
	let Some((first_name, first_value, first_bold)) = units.first().cloned() else {
		return Vec::new();
	};
	let Some((second_name, second_value, second_bold)) = units.get(1).cloned() else {
		return Vec::new();
	};
	let of = if total.text.is_empty() {
		"of the total".to_string()
	} else {
		format!("of {}", total.text)
	};
	let lead = first_value / total.value;
	let pair = (first_value + second_value) / total.value;
	let striking = units.len() >= 4;
	let named = lead < DOMINANT_PART || (first_bold && second_bold);
	if named && pair >= DOMINANT_PAIR && second_value / total.value >= SECOND_PART {
		return vec![Fact {
			key: format!("part:{}", first_name),
			text: format!(
				"{} and {} are {}% {}",
				first_name,
				second_name,
				(pair * 100.0).round() as i64,
				of
			),
			weight: 1.0 + pair,
			striking,
		}];
	}
	if lead < DOMINANT_PART {
		return Vec::new();
	}
	vec![Fact {
		key: format!("part:{}", first_name),
		text: format!("{} is {}% {}", first_name, (lead * 100.0).round() as i64, of),
		weight: 1.0 + lead,
		striking,
	}]
}

/// What a `bullet` chart says of its targets.
fn target_facts(spec: &ChartSpec, targets: &[Option<ChartTarget>]) -> Vec<Fact> {
	let Some(entry) = spec.series.first() else { return Vec::new() };
	let mut standings: Vec<(usize, &ChartPoint, &ChartTarget, Standing)> = Vec::new();
	for (at, target) in targets.iter().enumerate() {
		let Some(point) = entry.points.get(at).and_then(|point| point.as_ref()) else { continue };
		let Some(target) = target else { continue };
		let Some(standing) = target_standing(point, target) else { continue };
		standings.push((at, point, target, standing));
	}
	if standings.len() < 2 {
		return Vec::new();
	}
	let word = spec.target_name.clone().unwrap_or_else(|| "target".to_string());
	let said = |standing: &(usize, &ChartPoint, &ChartTarget, Standing)| {
		format!(
			"{} ({} vs {})",
			spec.categories.get(standing.0).cloned().unwrap_or_default(),
			standing.1.text,
			standing.2.text
		)
	};
	let reach = |standing: &(usize, &ChartPoint, &ChartTarget, Standing)| -> f64 {
		match standing.3.share {
			None => f64::INFINITY,
			Some(share) if standing.2.bound == Some(Bound::Min) => 1.0 / share.max(1e-9),
			Some(share) => share,
		}
	};
	let mut missed: Vec<(usize, &ChartPoint, &ChartTarget, Standing)> =
		standings.iter().filter(|standing| !standing.3.met).copied().collect();
	missed.sort_by(|a, b| reach(b).partial_cmp(&reach(a)).unwrap_or(std::cmp::Ordering::Equal));
	let count = standings.len();
	let text = if missed.is_empty() {
		let mut closest: Vec<(usize, &ChartPoint, &ChartTarget, Standing)> = standings
			.iter()
			.filter(|standing| standing.3.share.is_some() && standing.2.upper.is_none())
			.copied()
			.collect();
		closest.sort_by(|a, b| reach(b).partial_cmp(&reach(a)).unwrap_or(std::cmp::Ordering::Equal));
		let near = match closest.first() {
			None => String::new(),
			Some(standing) if standing.2.bound == Some(Bound::Max) => format!(
				"; closest: {} at {}%",
				spec.categories.get(standing.0).cloned().unwrap_or_default(),
				(standing.3.share.unwrap_or(0.0) * 100.0).round() as i64
			),
			Some(standing) => format!("; closest: {}", said(standing)),
		};
		let unchecked = if targets.len() > count { " checked" } else { "" };
		format!("All {}{} within {}{}", count, unchecked, word, near)
	} else if missed.len() == count {
		format!("None of {} within {}; worst: {}", count, word, said(&missed[0]))
	} else if missed.len() == 1 {
		let miss = missed[0];
		format!(
			"{} of {} within {}; {} misses ({} vs {})",
			count - 1,
			count,
			word,
			spec.categories.get(miss.0).cloned().unwrap_or_default(),
			miss.1.text,
			miss.2.text
		)
	} else if missed.len() <= MAX_MISSES_NAMED {
		let names: Vec<String> = missed
			.iter()
			.map(|standing| spec.categories.get(standing.0).cloned().unwrap_or_default())
			.collect();
		format!(
			"{} of {} within {}; {} miss",
			count - missed.len(),
			count,
			word,
			list(&names)
		)
	} else {
		format!("{} of {} within {}; worst: {}", count - missed.len(), count, word, said(&missed[0]))
	};
	vec![Fact { key: "target".to_string(), text, weight: CONSENSUS_WEIGHT, striking: true }]
}

/// Facts between two peers on one axis.
fn peer_facts(spec: &ChartSpec, first: &ChartSeries, second: &ChartSeries) -> Vec<Fact> {
	if first.dim != second.dim || first.unit != second.unit {
		return Vec::new();
	}
	let value = |point: &ChartPoint| if spec.kind == ChartKind::Range { interval_middle(point) } else { point.value };
	let both: Vec<(usize, &ChartPoint, &ChartPoint)> = spec
		.categories
		.iter()
		.enumerate()
		.filter_map(|(at, _)| {
			let a = first.points.get(at).and_then(|point| point.as_ref())?;
			let b = second.points.get(at).and_then(|point| point.as_ref())?;
			if value(a) > 0.0 && value(b) > 0.0 {
				Some((at, a, b))
			} else {
				None
			}
		})
		.collect();
	if both.len() < 3 || both.len() < spec.categories.len() {
		return Vec::new();
	}
	let noun = row_noun(spec);
	let differ: Vec<&(usize, &ChartPoint, &ChartPoint)> = both
		.iter()
		.filter(|(_, a, b)| a.text != b.text && value(*a) != value(*b))
		.collect();
	if differ.len() == 1 {
		let (at, a, b) = differ[0];
		let pair = if first.name.len() + second.name.len() <= MAX_LABEL {
			format!("{} and {}", first.name, second.name)
		} else {
			"Both".to_string()
		};
		return vec![Fact {
			key: format!("peers:{}", spec.categories.get(*at).cloned().unwrap_or_default()),
			text: format!(
				"{} match on every {} but {} ({} vs {})",
				pair,
				noun,
				spec.categories.get(*at).cloned().unwrap_or_default(),
				a.text,
				b.text
			),
			weight: RIVALRY_WEIGHT,
			striking: true,
		}];
	}
	let Some(polarity) = first.polarity.or(second.polarity) else {
		return Vec::new();
	};
	if first.polarity.is_some() && second.polarity.is_some() && first.polarity != second.polarity {
		return Vec::new();
	}
	let mut leads: Vec<f64> = both
		.iter()
		.map(|(_, a, b)| if polarity == Polarity::Higher { value(*a) / value(*b) } else { value(*b) / value(*a) })
		.collect();
	let ahead = leads.iter().all(|lead| *lead > 1.0);
	if !ahead && !leads.iter().all(|lead| *lead < 1.0) {
		return Vec::new();
	}
	leads = leads.into_iter().map(|lead| if ahead { lead } else { 1.0 / lead }).collect();
	leads.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	let (winner, loser) = if ahead { (first, second) } else { (second, first) };
	let low = leads[0];
	let high = *leads.last().expect("leads");
	let margin = if high < 2.0 {
		format!(
			"by {}–{}%",
			((low - 1.0) * 100.0).round() as i64,
			((high - 1.0) * 100.0).round() as i64
		)
	} else if round_factor(low) == round_factor(high) {
		format!("{}×", round_factor(high))
	} else {
		format!("{}–{}×", round_factor(low), round_factor(high))
	};
	vec![Fact {
		key: format!("peers:{}", winner.name),
		text: format!("{} beats {} on every {} ({})", winner.name, loser.name, noun, margin),
		weight: RIVALRY_WEIGHT + low.ln(),
		striking: low >= STRIKING_MOVE,
	}]
}

/// A `stat` plan for a small table comparing two conditions.
fn stat_shape(table: &TableAnalysis, guess: Option<&ChartPlan>) -> Option<ChartPlan> {
	let label_index = guess
		.and_then(|guess| guess.label)
		.or_else(|| table.label.as_ref().map(|label| label.index))
		.unwrap_or(usize::MAX);
	let label = table.columns.get(label_index)?;
	if label.role != ColumnRole::Label {
		return None;
	}
	if label.index > 0 && table.rows.iter().any(|&row| label.cells.get(row).map_or(false, |cell| is_number(cell))) {
		return None;
	}
	let plain = |cell: Option<&Cell>, dim: Option<Dimension>| -> bool {
		let Some(number) = cell.and_then(|cell| cell.as_number()) else { return false };
		Some(number.dim) == dim && number.value > 0.0 && number.upper.is_none() && number.to.is_none()
	};
	let measures: Vec<TableColumn> = table
		.measures
		.iter()
		.filter(|column| column.index != label.index && !DELTA_RE.is_match(&column.header))
		.cloned()
		.collect();
	if table.rows.len() == 2 {
		let first = table
			.rows
			.first()
			.and_then(|&row| label.cells.get(row))
			.map_or(String::new(), |cell| cell.text().trim().to_string());
		let second = table
			.rows
			.get(1)
			.and_then(|&row| label.cells.get(row))
			.map_or(String::new(), |cell| cell.text().trim().to_string());
		let conditions = condition_pair(&first, &second)
			|| condition_pair(&second, &first)
			|| (SWITCH_RE.is_match(&first) && SWITCH_RE.is_match(&second))
			|| RIVALS_RE.is_match(&label.header);
		let metrics: Vec<TableColumn> = measures
			.iter()
			.filter(|column| table.rows.iter().all(|&row| plain(column.cells.get(row), column.dim)))
			.cloned()
			.collect();
		if !conditions || metrics.is_empty() || metrics.len() > MAX_STATS || metrics.len() < measures.len() {
			return None;
		}
		let plan = ChartPlan {
			qualifiers: Some(Vec::new()),
			..ChartPlan::new(ChartKind::Stat, Some(label.index), metrics.iter().map(|column| column.index).collect(), false)
		};
		let spec = build_chart(table, &plan)?;
		stat_story(&spec.series)?;
		return Some(plan);
	}
	if table.rows.len() < 2 || table.rows.len() > 3 || measures.len() != 2 {
		return None;
	}
	let before = &measures[0];
	let after = &measures[1];
	if !is_transition(&before.header, &after.header) {
		return None;
	}
	if BUCKET_HEADER_RE.is_match(&label.header)
		|| table.rows.iter().any(|&row| {
			label
				.cells
				.get(row)
				.map_or(false, |cell| IDENTIFIER_RE.is_match(cell.text().trim()))
		}) {
		return None;
	}
	let dims: Vec<Option<Dimension>> = table
		.rows
		.iter()
		.map(|&row| before.cells.get(row).and_then(|cell| cell.as_number()).map(|number| number.dim))
		.collect();
	if !table
		.rows
		.iter()
		.enumerate()
		.all(|(at, &row)| dims[at].is_some() && plain(before.cells.get(row), dims[at]) && plain(after.cells.get(row), dims[at]))
	{
		return None;
	}
	let plan = ChartPlan {
		qualifiers: Some(Vec::new()),
		..ChartPlan::new(ChartKind::Stat, Some(label.index), vec![before.index, after.index], true)
	};
	let spec = build_chart(table, &plan)?;
	stat_story(&spec.series)?;
	Some(plan)
}

/// Whether two row names read as the before and after of one comparison.
fn condition_pair(before: &str, after: &str) -> bool {
	(ROW_BEFORE_RE.is_match(before) && !ROW_BEFORE_RE.is_match(after))
		|| (ROW_AFTER_RE.is_match(after) && !ROW_AFTER_RE.is_match(before))
}

/// Which of a comparison's two conditions the other reads against.
fn stat_baseline(names: &[String]) -> usize {
	let first = names.first().cloned().unwrap_or_default();
	let second = names.get(1).cloned().unwrap_or_default();
	let before = (condition_pair(&second, &first) && !condition_pair(&first, &second))
		|| (is_transition(&second, &first) && !is_transition(&first, &second))
		|| (SWITCHED_OFF_RE.is_match(&second) && !SWITCHED_OFF_RE.is_match(&first));
	if before {
		1
	} else {
		0
	}
}

/// The strongest move of a `stat` comparison's metrics.
fn stat_story(series: &[ChartSeries]) -> Option<f64> {
	if series.is_empty() || series.len() > MAX_STATS {
		return None;
	}
	let factors: Vec<f64> = series
		.iter()
		.filter_map(|entry| {
			let mut points = entry.points.iter();
			let a = points.next()?.as_ref()?;
			let b = points.next()?.as_ref()?;
			if entry.points.len() == 2 && a.value > 0.0 && b.value > 0.0 {
				Some(b.value / a.value)
			} else {
				None
			}
		})
		.collect();
	if factors.len() < series.len() {
		return None;
	}
	let strongest = factors.iter().copied().reduce(|best, factor| {
		if factor.abs().ln().abs() > best.abs().ln().abs() {
			factor
		} else {
			best
		}
	})?;
	if strongest.abs().ln().abs() >= STRIKING_MOVE.ln() {
		Some(strongest)
	} else {
		None
	}
}

/// Each metric of a `stat` comparison as a takeaway.
fn stat_facts(spec: &ChartSpec) -> Vec<Fact> {
	let base = spec.reference.as_ref().and_then(|reference| reference.first().copied()).unwrap_or(0);
	let other = 1 - base;
	let from = spec.categories.get(base).cloned().unwrap_or_default();
	let to = spec.categories.get(other).cloned().unwrap_or_default();
	let transition = is_transition(&from, &to);
	let moves: Vec<(&str, f64)> = spec
		.series
		.iter()
		.filter_map(|entry| {
			let a = entry.points.get(base).and_then(|point| point.as_ref())?;
			let b = entry.points.get(other).and_then(|point| point.as_ref())?;
			if a.value > 0.0 {
				Some((entry.name.as_str(), b.value / a.value))
			} else {
				None
			}
		})
		.collect();
	let strength = |factor: f64| factor.abs().ln().abs();
	let strongest = moves.iter().map(|(_, factor)| strength(*factor)).fold(0.0_f64, f64::max);
	let mut summary: Vec<Fact> = Vec::new();
	if moves.len() >= 2 && moves.len() == spec.series.len() {
		let mut sorted = moves.clone();
		sorted.sort_by(|a, b| strength(b.1).partial_cmp(&strength(a.1)).unwrap_or(std::cmp::Ordering::Equal));
		let text = format!(
			"{}: {}",
			to,
			sorted
				.iter()
				.map(|(name, factor)| {
					let words = factor_words(*factor);
					format!("{} {}", name, if words == "same" { "unchanged" } else { &words })
				})
				.collect::<Vec<_>>()
				.join(", ")
		);
		summary.push(Fact {
			key: "stat:all".to_string(),
			text,
			weight: strongest + 1.0,
			striking: strongest >= STRIKING_MOVE.ln(),
		});
	}
	let singles: Vec<Fact> = spec
		.series
		.iter()
		.filter_map(|entry| {
			let a = entry.points.get(base).and_then(|point| point.as_ref())?;
			let b = entry.points.get(other).and_then(|point| point.as_ref())?;
			if a.value <= 0.0 {
				return None;
			}
			let factor = b.value / a.value;
			let words = factor_words(factor);
			if words == "same" {
				return None;
			}
			Some(Fact {
				key: format!("stat:{}", entry.name),
				text: if transition {
					format!("{}: {} → {} ({})", entry.name, a.text, b.text, words)
				} else {
					format!("{}: {} {} vs {} {} ({})", entry.name, to, b.text, from, a.text, words)
				},
				weight: strength(factor),
				striking: strength(factor) >= STRIKING_MOVE.ln(),
			})
		})
		.collect();
	summary.into_iter().chain(singles).collect()
}

/// Baseline and compared columns of a before/after table a shared axis cannot hold.
fn change_columns(table: &TableAnalysis) -> Option<Vec<TableColumn>> {
	let label = table.label.as_ref()?;
	if label.role != ColumnRole::Label {
		return None;
	}
	let values: Vec<TableColumn> = table
		.columns
		.iter()
		.filter(|column| {
			*column != label
				&& (column.role == ColumnRole::Measure || column.role == ColumnRole::Label)
				&& !DELTA_RE.is_match(&column.header)
				&& !ARROW_RE.is_match(&column.header)
				&& column.arrow_share < 0.6
		})
		.cloned()
		.collect();
	let baseline = values.iter().find(|column| BEFORE_RE.is_match(&column.header))?.clone();
	let rows: Vec<usize> = table.rows.iter().chain(table.totals.iter()).copied().collect();
	let compared: Vec<TableColumn> = values
		.iter()
		.filter(|column| {
			*column != &baseline
				&& (column.role == ColumnRole::Measure || AFTER_RE.is_match(&column.header))
				&& rows
					.iter()
					.filter(|&&row| change_point(baseline.cells.get(row), column.cells.get(row)).is_some())
					.count()
					>= 2
		})
		.cloned()
		.collect();
	if !compared.iter().any(|column| AFTER_RE.is_match(&column.header)) {
		return None;
	}
	let mut columns = vec![baseline];
	columns.extend(compared);
	if columns.iter().all(|column| column.role == ColumnRole::Measure && !column.mixed) {
		return None;
	}
	Some(columns)
}

/// How a `change` pair reads a row.
enum ChangeAt {
	Arrow(TableColumn),
	Change(TableColumn, TableColumn),
}

/// One `change` series: its name, polarity, and how it reads each row.
struct ChangePair {
	name: String,
	polarity: Option<Polarity>,
	at: ChangeAt,
}

impl ChangePair {
	fn at(&self, row: usize) -> Option<ChartPoint> {
		match &self.at {
			ChangeAt::Arrow(column) => arrow_change(column.cells.get(row)),
			ChangeAt::Change(before, after) => change_point(before.cells.get(row), after.cells.get(row)),
		}
	}
}

/// A `change` over `columns`.
fn change_chart(
	table: &TableAnalysis,
	plan: &ChartPlan,
	namers: &[TableColumn],
	group: Option<&TableColumn>,
	columns: &[TableColumn],
	baselines: Option<&[TableColumn]>,
) -> Option<ChartSpec> {
	let baseline = columns.first()?.clone();
	let picked: Vec<TableColumn> = columns.iter().skip(1).cloned().collect();
	let values: Vec<TableColumn> = picked
		.iter()
		.filter(|column| !DELTA_RE.is_match(&CHANGES_RE.replace(&column.header, "")))
		.cloned()
		.collect();
	let compared = if values.is_empty() { picked.clone() } else { values };
	let (from, to) = arrow_names(&baseline.header);
	let improves = |column: &TableColumn, before: &TableColumn| {
		plan.polarity
			.as_ref()
			.and_then(|polarity| polarity.get(&column.index))
			.copied()
			.or_else(|| plan.polarity.as_ref().and_then(|polarity| polarity.get(&before.index)).copied())
	};
	let mut pairs: Vec<ChangePair>;
	let against: String;
	let mut rival = false;
	if let Some(baselines) = baselines {
		pairs = columns
			.iter()
			.enumerate()
			.map(|(at, column)| {
				let before = baselines.get(at).cloned().unwrap_or_else(|| baseline.clone());
				ChangePair {
					name: column.header.clone(),
					polarity: improves(column, &before),
					at: ChangeAt::Change(before, column.clone()),
				}
			})
			.collect();
		let first = baselines.first()?;
		against = BEFORE_RE
			.find(&first.header)
			.map(|found| found.as_str().to_string())
			.unwrap_or_else(|| first.header.clone());
	} else if !compared.is_empty() && columns.iter().all(|column| column.arrow_share >= 0.6) {
		pairs = columns
			.iter()
			.map(|column| ChangePair {
				name: arrow_subject(&column.header),
				polarity: improves(column, &baseline),
				at: ChangeAt::Arrow(column.clone()),
			})
			.collect();
		against = BEFORE_RE
			.find(&from)
			.map(|found| found.as_str().to_string())
			.unwrap_or_else(|| "before".to_string());
	} else if !compared.is_empty() {
		pairs = compared
			.iter()
			.map(|column| ChangePair {
				name: column.header.clone(),
				polarity: improves(column, &baseline),
				at: ChangeAt::Change(baseline.clone(), column.clone()),
			})
			.collect();
		against = baseline.header.clone();
		rival = !compared.iter().all(|column| is_transition(&baseline.header, &column.header));
	} else {
		pairs = vec![ChangePair {
			name: to.clone(),
			polarity: improves(&baseline, &baseline),
			at: ChangeAt::Arrow(baseline.clone()),
		}];
		against = from.clone();
	}
	let mut rows: Vec<usize> = table.rows.iter().chain(table.totals.iter()).copied().collect();
	rows.sort_unstable();
	let rows = regroup(&rows, group);
	let kept: Vec<usize> = rows
		.iter()
		.copied()
		.filter(|row| pairs.iter().any(|pair| pair.at(*row).is_some()))
		.collect();
	if kept.len() < 2 {
		return None;
	}
	let all = name_rows(namers, None, &rows, false).names;
	let skipped: Vec<String> = rows
		.iter()
		.enumerate()
		.filter(|(_, row)| !kept.contains(row))
		.map(|(at, _)| all.get(at).cloned().unwrap_or_default())
		.collect();
	let naming = name_rows(namers, group, &kept, true);
	let spec = ChartSpec {
		kind: ChartKind::Change,
		categories: naming.names,
		groups: naming.groups,
		series: pairs
			.iter()
			.map(|pair| ChartSeries {
				name: pair.name.clone(),
				dim: Dimension::Ratio,
				unit: String::new(),
				points: kept.iter().map(|&row| pair.at(row)).collect(),
				notes: kept.iter().map(|_| None).collect(),
				polarity: pair.polarity,
				best: None,
			})
			.collect(),
		caption: if skipped.is_empty() {
			None
		} else {
			Some(format!("Not plotted: {}", skipped.join(", ")))
		},
		axis: naming.axis,
		baseline: Some(against),
		rival: if rival { Some(true) } else { None },
		category_polarity: Some(
			kept
				.iter()
				.map(|&row| plan.row_polarity.as_ref().and_then(|polarity| polarity.get(&row)).copied())
				.collect(),
		),
		..ChartSpec::base(ChartKind::Change)
	};
	with_meaning(table, plan, &spec, Some(&kept))
}

/// What an `a → b` header's column measures.
fn arrow_subject(header: &str) -> String {
	let from = ARROW_RE.split(header).next().unwrap_or(header);
	let subject = BEFORE_RE
		.replace_all(from, "")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	if subject.is_empty() {
		header.to_string()
	} else {
		subject
	}
}

/// `to` as a factor of `from`, when both are numbers of one dimension.
fn change_point(from: Option<&Cell>, to: Option<&Cell>) -> Option<ChartPoint> {
	let from = from?.as_number()?;
	let to = to?.as_number()?;
	if from.dim != to.dim || from.value <= 0.0 || to.value < 0.0 {
		return None;
	}
	Some(ChartPoint {
		value: to.value / from.value,
		text: format!("{} → {}", from.figure, to.figure),
		emphasis: from.emphasis || to.emphasis,
		upper: None,
		error: None,
	})
}

/// An `a → b` cell as the factor `b ÷ a`.
fn arrow_change(cell: Option<&Cell>) -> Option<ChartPoint> {
	let cell = cell?.as_number()?;
	let to = cell.to?;
	if cell.value <= 0.0 || to < 0.0 {
		return None;
	}
	Some(ChartPoint {
		value: to / cell.value,
		text: format!("{} → {}", cell.figure, cell.to_figure.clone().unwrap_or_default()),
		emphasis: cell.emphasis,
		upper: None,
		error: None,
	})
}

/// The from and to names in an `a → b` header, else `before` and `after`.
fn arrow_names(header: &str) -> (String, String) {
	let mut parts = ARROW_RE.split(header);
	let from = parts.next().unwrap_or("");
	let to = parts.next();
	match to {
		Some(to) if !from.is_empty() && !to.is_empty() => (from.to_string(), to.to_string()),
		_ => ("before".to_string(), "after".to_string()),
	}
}

/// How a heatmap of `columns` over `rows` should shade.
fn guess_shading(columns: &[TableColumn], rows: &[usize]) -> Shading {
	let grid: Vec<Vec<f64>> = rows
		.iter()
		.map(|&row| {
			columns
				.iter()
				.map(|column| {
					column
						.cells
						.get(row)
						.and_then(|cell| cell.as_number())
						.filter(|number| number.value > 0.0)
						.map_or(f64::NAN, |number| number.value)
				})
				.collect()
		})
		.collect();
	if pair_spread(&grid) >= QUANTITY_SPREAD {
		return Shading::Series;
	}
	let transposed: Vec<Vec<f64>> = columns
		.iter()
		.enumerate()
		.map(|(at, _)| grid.iter().map(|values| *values.get(at).unwrap_or(&f64::NAN)).collect())
		.collect();
	if pair_spread(&transposed) < QUANTITY_SPREAD {
		return Shading::Shared;
	}
	let mut contrasts: Vec<f64> = grid
		.iter()
		.map(|values| values.iter().copied().filter(|value| value.is_finite()).collect::<Vec<_>>())
		.filter(|values| values.len() >= 2)
		.map(|values| {
			values.iter().copied().fold(f64::MIN, f64::max) / values.iter().copied().fold(f64::MAX, f64::min)
		})
		.collect();
	contrasts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
	if contrasts.get(contrasts.len() >> 1).copied().unwrap_or(1.0) >= ROW_CONTRAST {
		Shading::Row
	} else {
		Shading::Shared
	}
}

/// Same-unit aggregate columns restate the series beside them.
fn drop_aggregates(measures: &[TableColumn], rows: &[usize]) -> Vec<TableColumn> {
	measures
		.iter()
		.filter(|column| {
			if !AGGREGATE_RE.is_match(&column.header) {
				return true;
			}
			let others: Vec<&TableColumn> = measures
				.iter()
				.filter(|other| other.index != column.index && other.dim == column.dim)
				.collect();
			if others.len() < 2 {
				return true;
			}
			let mut ratios: Vec<f64> = rows
				.iter()
				.map(|&row| {
					let sum: f64 = others
						.iter()
						.map(|other| column_values(other, std::slice::from_ref(&row)).first().copied().unwrap_or(0.0))
						.sum();
					let value = column_values(column, std::slice::from_ref(&row)).first().copied().unwrap_or(f64::NAN);
					value / sum
				})
				.filter(|ratio| ratio.is_finite())
				.collect();
			ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
			(ratios.get(ratios.len() >> 1).copied().unwrap_or(0.0)) >= QUANTITY_SPREAD
		})
		.cloned()
		.collect()
}
