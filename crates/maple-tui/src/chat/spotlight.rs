//! The Spotlight experiment's trace on a tool result (`details.spotlight`).
//!
//! Reference: `l/src/chat/spotlight.ts`.
//!
//! What shaping did to the model-facing text, and, when it changed it, where the
//! original is and how Spotlight scored and kept its lines. A native transcript
//! draws it as a notice row under the tool card ([`SpotlightNotice`]); a click
//! on a "saved" or "held out" notice opens the sheet.
//!
//! Deviations:
//! - `isRecord` becomes `serde_json::Value::as_object`; `formatNumber` is
//!   [`crate::status_line::segments::format_number`], and the reference's
//!   `toLocaleString("en-US")` grouping is [`group_digits`].
//! - `handleNativeEvent(NativeUiEvent)` needs the native event type the crate
//!   keeps in its reconciler; this port exposes the action as
//!   [`SpotlightNotice::handle_action`] instead.
//! - `TspPropNode` is a plain [`serde_json::Value`] prop (`prop_node`).

use crate::component::Component;
use crate::native::describe::{col, node, prop_node, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text, Tone};
use crate::status_line::segments::format_number;

use crate::chat::transcript_actions::{
    has_transcript_actions, run_transcript_action, TranscriptAction,
};

/// Original lines a result's heat and the sheet cover; later lines are shown in
/// neither.
pub const SPOTLIGHT_VIEW_LINES: usize = 5_000;

/// One window of an original that several reads make up: where its lines sit in
/// the original and how they are numbered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightWindow {
    /// 0-based position of its first line in the original.
    pub at: usize,
    /// Its line count.
    pub lines: usize,
    /// Line number of its first line.
    pub start: u64,
    /// The file it shows.
    pub path: Option<String>,
    /// How the step that cut its lines by rank chose them.
    pub rank: Option<SpotlightRank>,
    /// The part read its file with no line selector.
    pub whole_file: bool,
}

/// Why shaping showed or hid one original line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotlightReason {
    /// Won on its score.
    Score,
    /// Structure kept regardless of score.
    Structure,
    /// Named by the intent or the conversation.
    Named,
    /// Context: a neighbour of a chosen line, or a lone gap filled.
    Context,
    /// Ranked below the cutoff or past the budget.
    BelowCutoff,
    /// Seen in an earlier call.
    Earlier,
    /// Folded by structure.
    Folded,
    /// Unknown.
    Unknown,
}

impl SpotlightReason {
    /// The trace character.
    pub fn as_char(self) -> char {
        match self {
            SpotlightReason::Score => 'a',
            SpotlightReason::Structure => 's',
            SpotlightReason::Named => 'n',
            SpotlightReason::Context => 'c',
            SpotlightReason::BelowCutoff => 'b',
            SpotlightReason::Earlier => 'r',
            SpotlightReason::Folded => 'f',
            SpotlightReason::Unknown => '-',
        }
    }

    /// Parse a trace character; `None` for anything outside the vocabulary.
    pub fn from_char(ch: char) -> Option<Self> {
        match ch {
            'a' => Some(SpotlightReason::Score),
            's' => Some(SpotlightReason::Structure),
            'n' => Some(SpotlightReason::Named),
            'c' => Some(SpotlightReason::Context),
            'b' => Some(SpotlightReason::BelowCutoff),
            'r' => Some(SpotlightReason::Earlier),
            'f' => Some(SpotlightReason::Folded),
            '-' => Some(SpotlightReason::Unknown),
            _ => None,
        }
    }

    /// Whether the reason shows the line.
    pub fn shows(self) -> bool {
        reason_shows(self)
    }
}

/// The last line kept on rank and the characters it cost.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpotlightRankLast {
    /// 1-based position in the original.
    pub line: u64,
    /// Characters it cost.
    pub chars: u64,
}

/// The ranking of the step that cut by rank.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightRank {
    /// The parts a score adds up, in order.
    pub parts: Vec<String>,
    /// The highest score a line can reach.
    pub max: f64,
    /// Per part, its value on every original line.
    pub values: Vec<String>,
    /// The lowest score the step kept on rank.
    pub cutoff: f64,
    /// Characters the step could show, when it had a budget.
    pub budget: Option<u64>,
    /// Characters it showed.
    pub used: Option<u64>,
    /// The last line kept on rank.
    pub last: Option<SpotlightRankLast>,
}

/// The original of a result whose text shaping changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightOriginal {
    /// Session artifact holding the original text (`artifact://N`).
    pub url: String,
    /// Its line count.
    pub lines: usize,
    /// Line number of its first line.
    pub start: u64,
    /// The file a `read` showed.
    pub path: Option<String>,
    /// The windows the original is made of, when the result joins several reads.
    pub windows: Vec<SpotlightWindow>,
    /// One character per original line: its rank among the scored lines,
    /// `0` (coldest) to `9` (hottest), `-` for a line it never scored.
    pub heat: String,
    /// Runs of original lines the model was shown verbatim, 1-based.
    pub kept: Vec<(u64, u64)>,
    /// One reason character per original line.
    pub why: Option<String>,
    /// How the step that cut by rank chose its lines.
    pub rank: Option<SpotlightRank>,
    /// One entry per line of the text the model saw.
    pub map: Vec<Option<LineRef>>,
    /// A held-out result's would-be text.
    pub shaped_url: Option<String>,
}

/// What one line of the text the model saw stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineRef {
    /// The original line it shows (1-based).
    Line(u64),
    /// The `[first, last]` run a marker stands for.
    Run(u64, u64),
    /// A line of its own (a shaping note).
    Own,
}

/// What a result's Spotlight requests sent besides the output lines they ranked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpotlightRequestParams {
    /// The user's first message of the session.
    pub task: String,
    /// Up to three earlier bash commands of the session, oldest first.
    pub commands: Vec<String>,
    /// The agent's text before the call.
    pub said: String,
    /// The call's intent argument.
    pub intent: String,
    /// The command, code or `read <path>` that produced the output.
    pub source: String,
    /// The probe the lines were ranked under.
    pub probe: String,
    /// The model.
    pub model: String,
    /// The thinking level.
    pub thinking: String,
}

/// `details.spotlight`: one tool result as shaping left it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotlightDetails {
    /// The tool name.
    pub tool: String,
    /// The shaping steps that ran (`none` for a passthrough).
    pub steps: Vec<String>,
    /// Characters in the original.
    pub in_chars: u64,
    /// Characters in the text the model saw.
    pub out_chars: u64,
    /// `o200k` tokens of the original.
    pub in_tokens: u64,
    /// `o200k` tokens of the text the model saw.
    pub out_tokens: u64,
    /// Size of the plain result for the same call.
    pub plain_chars: Option<u64>,
    /// Tokens of the plain result.
    pub plain_tokens: Option<u64>,
    /// `plainTokens - outTokens` for a changed result, else 0.
    pub saved_tokens: i64,
    /// The score-free coin skipped Spotlight.
    pub score_free: bool,
    /// The cut saved too few tokens against the plain result to apply.
    pub capped_chars: Option<u64>,
    /// Tokens of the would-be cut.
    pub capped_tokens: Option<u64>,
    /// Shaping would have changed the text, but the result was held out.
    pub holdout: bool,
    /// The old edit gate kept the result whole.
    pub gated: bool,
    /// Present only when shaping changed the text, or would have.
    pub original: Option<SpotlightOriginal>,
    /// Present with `original` when Spotlight ranked the result's lines.
    pub request: Option<SpotlightRequestParams>,
}

/// The windows an original is made of.
pub fn original_windows(original: &SpotlightOriginal) -> Vec<SpotlightWindow> {
    if !original.windows.is_empty() {
        return original.windows.clone();
    }
    vec![SpotlightWindow {
        at: 0,
        lines: original.lines,
        start: original.start,
        path: original.path.clone(),
        rank: original.rank.clone(),
        whole_file: false,
    }]
}

/// Whether a [`SpotlightReason`] shows the line.
pub fn reason_shows(reason: SpotlightReason) -> bool {
    matches!(
        reason,
        SpotlightReason::Score
            | SpotlightReason::Structure
            | SpotlightReason::Named
            | SpotlightReason::Context
    )
}

/// The reason for original line `i` (0-based), or `None` past the trace or
/// without one.
pub fn reason_at(why: Option<&str>, i: usize) -> Option<SpotlightReason> {
    let ch = why?.chars().nth(i)?;
    SpotlightReason::from_char(ch)
}

/// Levels of one [`encode_rank_part`] value: two base-36 digits.
const RANK_LEVELS: f64 = (36 * 36 - 1) as f64;

/// One [`SpotlightRank::values`] entry: two base-36 digits per line, each value
/// relative to `max`.
pub fn encode_rank_part(values: &[f64], max: f64) -> String {
    let n = values.len().min(SPOTLIGHT_VIEW_LINES);
    let mut out = String::with_capacity(n * 2);
    for value in values.iter().take(n) {
        let level = if max > 0.0 {
            ((value / max).clamp(0.0, 1.0) * RANK_LEVELS).round() as u64
        } else {
            0
        };
        out.push_str(&base36_two(level));
    }
    out
}

/// Two-digit base-36 form of `level`.
fn base36_two(level: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut value = level.min(RANK_LEVELS as u64);
    let low = (value % 36) as usize;
    value /= 36;
    let high = (value % 36) as usize;
    format!("{}{}", DIGITS[high] as char, DIGITS[low] as char)
}

/// The value of original line `i` (0-based) in an [`encode_rank_part`] string;
/// 0 past its end.
pub fn rank_part_at(encoded: &str, i: usize, max: f64) -> f64 {
    let start = 2 * i;
    let slice: String = encoded.chars().skip(start).take(2).collect();
    let level = u64::from_str_radix(&slice, 36).unwrap_or(0);
    (level as f64 / RANK_LEVELS) * max
}

/// The `heat` of per-line scores: each scored line's rank among them in ten
/// steps, ties sharing their mean place.
pub fn encode_heat(scores: &[Option<f64>]) -> String {
    let n = scores.len().min(SPOTLIGHT_VIEW_LINES);
    let mut order: Vec<(f64, usize)> = Vec::new();
    for (index, score) in scores.iter().take(n).enumerate() {
        if let Some(score) = score {
            order.push((*score, index));
        }
    }
    order.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut digits: Vec<char> = vec!['-'; n];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && order[j + 1].0 == order[i].0 {
            j += 1;
        }
        let digit = if order.len() > 1 {
            ((((i + j) as f64 / 2.0) / (order.len() - 1) as f64) * 9.0).round() as u32
        } else {
            9
        };
        for entry in &order[i..=j] {
            digits[entry.1] = char::from_digit(digit, 10).unwrap_or('9');
        }
        i = j + 1;
    }
    let text: String = digits.into_iter().collect();
    text.trim_end_matches('-').to_string()
}

/// The heat of original line `i` (0-based) in `[0, 1]`, or `None` when
/// Spotlight never scored it.
pub fn heat_at(heat: &str, i: usize) -> Option<f64> {
    let ch = heat.as_bytes().get(i).copied()?;
    if ch.is_ascii_digit() {
        Some((ch - b'0') as f64 / 9.0)
    } else {
        None
    }
}

/// `kept` of a per-line kept mask.
pub fn kept_ranges(kept: &[bool]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < kept.len() {
        if !kept[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < kept.len() && kept[j + 1] {
            j += 1;
        }
        out.push((i as u64 + 1, j as u64 + 1));
        i = j + 1;
    }
    out
}

/// Heat digit from which a line counts as hot.
const HOT_DIGIT: u8 = 9;

/// Whether original line `i` (0-based) scored hot.
pub fn spotlight_hot(heat: &str, i: usize) -> bool {
    heat.as_bytes()
        .get(i)
        .is_some_and(|ch| (b'0' + HOT_DIGIT..=b'9').contains(ch))
}

/// Tokens the shaped text saves against the plain result (negative: it costs
/// more), or against the original on traces without `plain_tokens`.
pub fn spotlight_saving(spotlight: &SpotlightDetails) -> i64 {
    match spotlight.plain_tokens {
        None => spotlight.saved_tokens,
        Some(plain) => plain as i64 - spotlight.out_tokens as i64,
    }
}

/// The trace a result's details carry, or `None` when the experiment did not
/// shape it.
pub fn spotlight_details(details: &serde_json::Value) -> Option<SpotlightDetails> {
    let trace = details.as_object()?.get("spotlight")?.as_object()?;
    let tool = trace.get("tool")?.as_str()?;
    let saved_tokens = trace.get("savedTokens")?.as_i64()?;
    let number = |key: &str| trace.get(key).and_then(serde_json::Value::as_u64);
    let flag = |key: &str| trace.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
    let strings = |key: &str| -> Vec<String> {
        trace
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(SpotlightDetails {
        tool: tool.to_string(),
        steps: strings("steps"),
        in_chars: number("inChars").unwrap_or(0),
        out_chars: number("outChars").unwrap_or(0),
        in_tokens: number("inTokens").unwrap_or(0),
        out_tokens: number("outTokens").unwrap_or(0),
        plain_chars: number("plainChars"),
        plain_tokens: number("plainTokens"),
        saved_tokens,
        score_free: flag("scoreFree"),
        capped_chars: number("cappedChars"),
        capped_tokens: number("cappedTokens"),
        holdout: flag("holdout"),
        gated: flag("gated"),
        original: trace.get("original").and_then(sp_original),
        request: trace.get("request").and_then(sp_request),
    })
}

fn sp_original(value: &serde_json::Value) -> Option<SpotlightOriginal> {
    let object = value.as_object()?;
    let kept = object
        .get("kept")
        .and_then(serde_json::Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(|run| {
                    let pair = run.as_array()?;
                    Some((pair.first()?.as_u64()?, pair.get(1)?.as_u64()?))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(SpotlightOriginal {
        url: object.get("url")?.as_str()?.to_string(),
        lines: object.get("lines")?.as_u64()? as usize,
        start: object.get("start").and_then(serde_json::Value::as_u64).unwrap_or(1),
        path: object.get("path").and_then(serde_json::Value::as_str).map(str::to_string),
        windows: object
            .get("windows")
            .and_then(serde_json::Value::as_array)
            .map(|windows| windows.iter().filter_map(sp_window).collect())
            .unwrap_or_default(),
        heat: object.get("heat").and_then(serde_json::Value::as_str)?.to_string(),
        kept,
        why: object.get("why").and_then(serde_json::Value::as_str).map(str::to_string),
        rank: object.get("rank").and_then(sp_rank),
        map: object
            .get("map")
            .and_then(serde_json::Value::as_array)
            .map(|entries| entries.iter().map(sp_line_ref).collect())
            .unwrap_or_default(),
        shaped_url: object.get("shapedUrl").and_then(serde_json::Value::as_str).map(str::to_string),
    })
}

fn sp_window(value: &serde_json::Value) -> Option<SpotlightWindow> {
    let object = value.as_object()?;
    Some(SpotlightWindow {
        at: object.get("at")?.as_u64()? as usize,
        lines: object.get("lines")?.as_u64()? as usize,
        start: object.get("start").and_then(serde_json::Value::as_u64).unwrap_or(1),
        path: object.get("path").and_then(serde_json::Value::as_str).map(str::to_string),
        rank: object.get("rank").and_then(sp_rank),
        whole_file: object
            .get("wholeFile")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

fn sp_rank(value: &serde_json::Value) -> Option<SpotlightRank> {
    let object = value.as_object()?;
    Some(SpotlightRank {
        parts: object
            .get("parts")
            .and_then(serde_json::Value::as_array)
            .map(|parts| parts.iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
        max: object.get("max").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
        values: object
            .get("values")
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        cutoff: object.get("cutoff").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
        budget: object.get("budget").and_then(serde_json::Value::as_u64),
        used: object.get("used").and_then(serde_json::Value::as_u64),
        last: object.get("last").and_then(|last| {
            Some(SpotlightRankLast {
                line: last.get("line")?.as_u64()?,
                chars: last.get("chars")?.as_u64()?,
            })
        }),
    })
}

fn sp_line_ref(value: &serde_json::Value) -> Option<LineRef> {
    if let Some(line) = value.as_u64() {
        return Some(LineRef::Line(line));
    }
    if let Some(pair) = value.as_array() {
        return Some(LineRef::Run(pair.first()?.as_u64()?, pair.get(1)?.as_u64()?));
    }
    if value.is_null() {
        return Some(LineRef::Own);
    }
    None
}

fn sp_request(value: &serde_json::Value) -> Option<SpotlightRequestParams> {
    let object = value.as_object()?;
    let field = |key: &str| object.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    Some(SpotlightRequestParams {
        task: field("task"),
        commands: object
            .get("commands")
            .and_then(serde_json::Value::as_array)
            .map(|commands| {
                commands
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        said: field("said"),
        intent: field("intent"),
        source: field("source"),
        probe: field("probe"),
        model: field("model"),
        thinking: field("thinking"),
    })
}

/// The tone the sheet and the notice give original line `i` (0-based).
pub fn spotlight_tone(original: &SpotlightOriginal, i: usize) -> Option<Tone> {
    let reason = reason_at(original.why.as_deref(), i)?;
    if reason == SpotlightReason::Unknown {
        return None;
    }
    if reason_shows(reason) {
        return Some(match reason {
            SpotlightReason::Score => Tone::Accent,
            SpotlightReason::Structure => Tone::Neutral,
            SpotlightReason::Named => Tone::User,
            _ => Tone::Info,
        });
    }
    if spotlight_hot(&original.heat, i) {
        Some(Tone::Error)
    } else {
        None
    }
}

/// At most this many bars in the notice card's chart.
const NOTICE_BARS: usize = 120;

/// Tones a notice card bar takes, the most notable first.
const NOTICE_TONES: [Tone; 6] = [
    Tone::Error,
    Tone::Accent,
    Tone::User,
    Tone::Info,
    Tone::Neutral,
    Tone::Muted,
];

/// The notice card's tone for original line `i`, `muted` for a plain hidden
/// line.
fn notice_tone(original: &SpotlightOriginal, i: usize) -> Option<Tone> {
    if let Some(tone) = spotlight_tone(original, i) {
        return Some(tone);
    }
    let reason = reason_at(original.why.as_deref(), i)?;
    if reason != SpotlightReason::Unknown && !reason_shows(reason) {
        Some(Tone::Muted)
    } else {
        None
    }
}

/// One bar of the notice card's chart.
#[derive(Clone, Debug, PartialEq)]
pub struct NoticeBar {
    /// The bar's value in `[0, 1]`.
    pub value: f64,
    /// Its tone, when any of its lines had one.
    pub tone: Option<Tone>,
}

/// The hover card of a changed result's notice: its heat as at most
/// [`NOTICE_BARS`] bars, then how many lines the model was shown.
pub fn spotlight_notice_card(original: &SpotlightOriginal, held_out: bool) -> serde_json::Value {
    let lines = original.heat.chars().count();
    let bars = NOTICE_BARS.min(lines);
    let mut series = Vec::new();
    for bar in 0..bars {
        let start = bar * lines / bars;
        let end = (bar + 1) * lines / bars;
        let mut value: f64 = 0.0;
        let mut best = NOTICE_TONES.len();
        for i in start..end {
            value = value.max(heat_at(&original.heat, i).unwrap_or(0.0));
            if let Some(tone) = notice_tone(original, i) {
                if let Some(index) = NOTICE_TONES.iter().position(|known| *known == tone) {
                    best = best.min(index);
                }
            }
        }
        series.push(NoticeBar {
            value,
            tone: NOTICE_TONES.get(best).copied(),
        });
    }
    let kept: u64 = original
        .kept
        .iter()
        .map(|(from, to)| to.saturating_sub(*from) + 1)
        .sum();
    let mut scored = 0u64;
    let mut hot = 0u64;
    let why_len = original.why.as_ref().map(|why| why.chars().count()).unwrap_or(0);
    for i in 0..why_len {
        if reason_at(original.why.as_deref(), i) == Some(SpotlightReason::Score) {
            scored += 1;
        } else if spotlight_tone(original, i) == Some(Tone::Error) {
            hot += 1;
        }
    }
    let mut facts = vec![format!(
        "{} {} of {} lines",
        if held_out { "would keep" } else { "kept" },
        group_digits(kept),
        group_digits(original.lines as u64)
    )];
    if scored > 0 {
        facts.push(format!("{} by score", group_digits(scored)));
    }
    if hot > 0 {
        facts.push(format!("{} cut though hot", group_digits(hot)));
    }

    let mut chart_props = Props::new();
    chart_props.set_str("kind", "bars");
    chart_props.set_str("size", "sm");
    chart_props.set_i64("min", 0);
    chart_props.set_i64("max", 1);
    chart_props.insert(
        "series".to_string(),
        serde_json::Value::Array(
            series
                .iter()
                .map(|bar| {
                    let mut value = serde_json::Map::new();
                    value.insert("value".into(), serde_json::json!(bar.value));
                    if let Some(tone) = bar.tone {
                        value.insert("tone".into(), serde_json::Value::String(tone.as_str().to_string()));
                    }
                    serde_json::Value::Object(value)
                })
                .collect(),
        ),
    );
    let chart = NativeNode::with_props(NodeKind::Chart, chart_props);
    let mut facts_props = Props::new();
    facts_props.set_tone(Tone::Muted);
    let facts_node = text(Text::plain(facts.join(" · ")), facts_props);
    let mut col_props = Props::new();
    col_props.set_str("gap", "xs");
    prop_node(&col(vec![chart, facts_node], col_props))
}

/// Thousands-grouped integer, the reference's `toLocaleString("en-US")`.
pub fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The notice's star when the model saw the original.
const STAR_OFF: &str = "\u{f155b}";
/// The notice's star when shaping changed what the model saw.
const STAR: &str = "\u{f005}";
/// The lit star past [`SHOOTING_STAR_TOKENS`] saved.
const SHOOTING_STAR: &str = "\u{f1742}";
/// Tokens saved past which the lit star shoots.
const SHOOTING_STAR_TOKENS: i64 = 10_000;

/// The notice's spans: Spotlight's star, then what shaping did to the result.
fn notice_spans(spotlight: &SpotlightDetails) -> Vec<crate::native::node::Span> {
    let saving = spotlight_saving(spotlight);
    if spotlight.holdout || spotlight.gated {
        let why = if spotlight.gated {
            "kept whole for editing"
        } else {
            "held out"
        };
        return vec![
            span(STAR_OFF, Some("icon")),
            span(
                format!(
                    "spotlight: {why} · would save {} toks",
                    format_number(saving.max(0) as f64)
                ),
                None,
            ),
        ];
    }
    if spotlight.original.is_none() {
        return vec![span(STAR_OFF, Some("icon"))];
    }
    let star = if saving > SHOOTING_STAR_TOKENS {
        SHOOTING_STAR
    } else {
        STAR
    };
    let mut star_span = span(star, Some("icon"));
    star_span.fx = Some("pulse".to_string());
    let label = if saving > 0 {
        format!("spotlight: saved {} toks", format_number(saving as f64))
    } else {
        format!("spotlight: reshaped, +{} toks", format_number(-saving as f64))
    };
    vec![star_span, span(label, None)]
}

/// The notice row under one tool card.
pub struct SpotlightNotice {
    tool_call_id: String,
    spotlight: SpotlightDetails,
    reduced: String,
    hidden: bool,
}

impl SpotlightNotice {
    /// A notice for call `tool_call_id`.
    pub fn new(tool_call_id: impl Into<String>, spotlight: SpotlightDetails, reduced: impl Into<String>) -> Self {
        SpotlightNotice {
            tool_call_id: tool_call_id.into(),
            spotlight,
            reduced: reduced.into(),
            hidden: false,
        }
    }

    /// Replace the trace and the text the model saw; false when unchanged.
    pub fn update(&mut self, spotlight: SpotlightDetails, reduced: String) -> bool {
        if spotlight == self.spotlight && reduced == self.reduced {
            return false;
        }
        self.spotlight = spotlight;
        self.reduced = reduced;
        true
    }

    /// Hide with the card it belongs to (tool activity hidden).
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    /// The call this notice belongs to.
    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    /// Whether the notice is hidden.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// The shaping trace.
    pub fn spotlight(&self) -> &SpotlightDetails {
        &self.spotlight
    }

    /// Run the `spotlight` action this notice's click stands for; false when
    /// there is no original to open or no host handles actions.
    pub fn handle_action(&self) -> bool {
        if self.spotlight.original.is_none() {
            return false;
        }
        run_transcript_action(TranscriptAction::Spotlight {
            tool_call_id: self.tool_call_id.clone(),
            spotlight: self.spotlight.clone(),
            reduced: self.reduced.clone(),
        })
    }
}

impl Component for SpotlightNotice {
    fn render(&mut self, _width: u16) -> Vec<String> {
        Vec::new()
    }
}

impl NativeDescribe for SpotlightNotice {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let original = self.spotlight.original.as_ref();
        let mut props = Props::new();
        props.set_str("role", "omp.tool.spotlight");
        let tone = match original {
            Some(_) if !self.spotlight.holdout && !self.spotlight.gated => Tone::Accent,
            _ => Tone::Muted,
        };
        props.set_tone(tone);
        props.insert(
            "spans".to_string(),
            serde_json::Value::Array(notice_spans(&self.spotlight).iter().map(|s| s.to_value()).collect()),
        );
        match original {
            Some(original) => {
                props.insert(
                    "hovercard".to_string(),
                    spotlight_notice_card(original, self.spotlight.holdout || self.spotlight.gated),
                );
                if has_transcript_actions() {
                    props.insert("actions".to_string(), serde_json::json!({"click": "spotlight"}));
                }
            }
            None => {
                props.set_str("title", "Spotlight: passthrough");
                props.set_str("aria", "Spotlight: passthrough");
            }
        }
        if self.hidden {
            props.set_hidden(true);
        }
        let mut described = NativeNode::with_props(NodeKind::Text, props);
        described.key = Some(format!("spotlight:{}", self.tool_call_id));
        described
    }
}

/// The notices of one transcript block's calls, in the order the calls first
/// reported a trace.
#[derive(Default)]
pub struct SpotlightNotices {
    by_call: Vec<(String, SpotlightNotice)>,
    hidden: bool,
}

impl SpotlightNotices {
    /// An empty set.
    pub fn new() -> Self {
        SpotlightNotices::default()
    }

    /// Records call `tool_call_id`'s final result; true when a notice appeared
    /// or went.
    pub fn set(&mut self, tool_call_id: &str, details: &serde_json::Value, reduced: &str) -> bool {
        let spotlight = spotlight_details(details);
        let existing = self
            .by_call
            .iter()
            .position(|(id, _notice)| id == tool_call_id);
        match (spotlight, existing) {
            (None, None) => false,
            (None, Some(index)) => {
                self.by_call.remove(index);
                true
            }
            (Some(spotlight), Some(index)) => {
                self.by_call[index].1.update(spotlight, reduced.to_string());
                false
            }
            (Some(spotlight), None) => {
                let mut notice = SpotlightNotice::new(tool_call_id, spotlight, reduced);
                notice.set_hidden(self.hidden);
                self.by_call.push((tool_call_id.to_string(), notice));
                true
            }
        }
    }

    /// Hide or show every notice.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        for (_id, notice) in &mut self.by_call {
            notice.set_hidden(hidden);
        }
    }

    /// The notice blocks; `shown` drops the notices of calls a transcript
    /// presenter hides.
    pub fn list(&self, shown: Option<&dyn Fn(&str) -> bool>) -> Vec<&SpotlightNotice> {
        self.by_call
            .iter()
            .filter(|(id, _notice)| shown.is_none_or(|shown| shown(id)))
            .map(|(_id, notice)| notice)
            .collect()
    }

    /// How many notices are tracked.
    pub fn len(&self) -> usize {
        self.by_call.len()
    }

    /// Whether no notice is tracked.
    pub fn is_empty(&self) -> bool {
        self.by_call.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn original() -> SpotlightOriginal {
        SpotlightOriginal {
            url: "artifact://7".into(),
            lines: 10,
            start: 1,
            heat: "0123456789".into(),
            kept: vec![(1, 2), (5, 6)],
            why: Some("ssccbbbrrf".into()),
            ..SpotlightOriginal::default()
        }
    }

    #[test]
    fn windows_fall_back_to_the_whole_original() {
        let mut one = original();
        one.path = Some("a.rs".into());
        let windows = original_windows(&one);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].lines, 10);
        assert_eq!(windows[0].path.as_deref(), Some("a.rs"));

        let mut joined = original();
        joined.windows = vec![
            SpotlightWindow { at: 0, lines: 4, start: 1, ..SpotlightWindow::default() },
            SpotlightWindow { at: 4, lines: 6, start: 40, ..SpotlightWindow::default() },
        ];
        assert_eq!(original_windows(&joined).len(), 2);
    }

    #[test]
    fn reasons_map_to_characters_and_visibility() {
        let cases = [
            ('a', SpotlightReason::Score, true),
            ('s', SpotlightReason::Structure, true),
            ('n', SpotlightReason::Named, true),
            ('c', SpotlightReason::Context, true),
            ('b', SpotlightReason::BelowCutoff, false),
            ('r', SpotlightReason::Earlier, false),
            ('f', SpotlightReason::Folded, false),
            ('-', SpotlightReason::Unknown, false),
        ];
        for (ch, reason, shows) in cases {
            assert_eq!(SpotlightReason::from_char(ch), Some(reason));
            assert_eq!(reason.as_char(), ch);
            assert_eq!(reason_shows(reason), shows);
            assert_eq!(reason_at(Some("01234567"), "asncbrf".find(ch).unwrap_or(0)), Some(reason));
        }
        assert_eq!(SpotlightReason::from_char('z'), None);
        assert_eq!(reason_at(Some("a"), 5), None);
        assert_eq!(reason_at(None, 0), None);
    }

    #[test]
    fn heat_ranks_lines_in_ten_steps() {
        assert_eq!(encode_heat(&[Some(1.0), Some(2.0), Some(3.0)]), "059");
        assert_eq!(encode_heat(&[Some(1.0)]), "9");
        assert_eq!(encode_heat(&[None, Some(1.0), None]), "-9");
        assert_eq!(heat_at("059", 0), Some(0.0));
        assert_eq!(heat_at("059", 2), Some(1.0));
        assert_eq!(heat_at("059", 3), None);
        assert!(spotlight_hot("9", 0));
        assert!(!spotlight_hot("8", 0));
    }

    #[test]
    fn rank_parts_round_trip() {
        let encoded = encode_rank_part(&[0.0, 0.5, 1.0], 1.0);
        assert_eq!(encoded.len(), 6);
        assert!((rank_part_at(&encoded, 1, 1.0) - 0.5).abs() < 0.02);
        assert!((rank_part_at(&encoded, 2, 1.0) - 1.0).abs() < 0.001);
        assert_eq!(rank_part_at(&encoded, 9, 1.0), 0.0);
    }

    #[test]
    fn kept_ranges_group_runs() {
        assert_eq!(
            kept_ranges(&[true, true, false, true, false, false, true]),
            vec![(1, 2), (4, 4), (7, 7)]
        );
        assert!(kept_ranges(&[false, false]).is_empty());
    }

    #[test]
    fn saving_prefers_the_plain_baseline() {
        let mut details = SpotlightDetails {
            out_tokens: 300,
            saved_tokens: 100,
            ..SpotlightDetails::default()
        };
        assert_eq!(spotlight_saving(&details), 100);
        details.plain_tokens = Some(1_000);
        assert_eq!(spotlight_saving(&details), 700);
    }

    #[test]
    fn details_parse_only_with_a_tool_and_a_saving() {
        let shaped = spotlight_details(&json!({
            "spotlight": {
                "tool": "read",
                "steps": ["window", "rank"],
                "inChars": 1000, "outChars": 400,
                "inTokens": 250, "outTokens": 100,
                "savedTokens": 150,
                "holdout": true,
                "original": {"url": "artifact://1", "lines": 2, "start": 1, "heat": "09", "kept": [[1, 1]], "why": "ab"}
            }
        }))
        .expect("trace parses");
        assert_eq!(shaped.tool, "read");
        assert_eq!(shaped.steps.join(","), "window,rank");
        assert_eq!(shaped.saved_tokens, 150);
        assert!(shaped.holdout);
        let original = shaped.original.expect("original parses");
        assert_eq!(original.heat, "09");
        assert_eq!(original.kept, vec![(1, 1)]);
        assert_eq!(original.why.as_deref(), Some("ab"));

        assert!(spotlight_details(&json!({"tool": "read"})).is_none());
        assert!(spotlight_details(&json!({"spotlight": {"tool": "read"}})).is_none());
    }

    #[test]
    fn tones_follow_the_reason_and_hot_cuts() {
        let tone_of = |why: &str, heat: &str, i: usize| {
            let mut original = original();
            original.why = Some(why.to_string());
            original.heat = heat.to_string();
            spotlight_tone(&original, i)
        };
        assert_eq!(tone_of("a", "0", 0), Some(Tone::Accent));
        assert_eq!(tone_of("s", "0", 0), Some(Tone::Neutral));
        assert_eq!(tone_of("n", "0", 0), Some(Tone::User));
        assert_eq!(tone_of("c", "0", 0), Some(Tone::Info));
        assert_eq!(tone_of("b", "9", 0), Some(Tone::Error));
        assert_eq!(tone_of("b", "0", 0), None);
        assert_eq!(tone_of("-", "9", 0), None);
    }

    #[test]
    fn the_notice_card_charts_bars_and_facts() {
        let card = spotlight_notice_card(&original(), false);
        let chart = &card["c"][0];
        assert_eq!(chart["k"], "chart");
        assert_eq!(chart["p"]["kind"], "bars");
        assert_eq!(chart["c"].as_array().map(Vec::len), None);
        let bars = chart["p"]["series"].as_array().expect("series");
        assert_eq!(bars.len(), 10);
        assert!(bars[0]["value"].as_f64().unwrap() >= 0.0);
        let facts = card["c"][1]["p"]["text"].as_str().unwrap_or_default();
        assert!(facts.starts_with("kept "));
        assert!(facts.contains(" of 10 lines"));
        assert!(facts.contains("by score"));
        assert!(facts.contains("cut though hot"));
        let held = spotlight_notice_card(&original(), true);
        assert!(held["c"][1]["p"]["text"].as_str().unwrap_or_default().starts_with("would keep "));
    }

    #[test]
    fn digits_group_by_thousands() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1_000), "1,000");
        assert_eq!(group_digits(1_234_567), "1,234,567");
    }

    #[test]
    fn the_notice_describes_a_passthrough_without_an_original() {
        let notice = SpotlightNotice::new(
            "call_1",
            SpotlightDetails {
                tool: "bash".into(),
                steps: vec!["none".into()],
                saved_tokens: 0,
                ..SpotlightDetails::default()
            },
            "text",
        );
        let node = notice.describe_native(&DescribeContext::new(80));
        assert_eq!(node.props.role(), Some("omp.tool.spotlight"));
        assert_eq!(node.key.as_deref(), Some("spotlight:call_1"));
        assert_eq!(node.props.tone(), Some(Tone::Muted));
        assert_eq!(node.props.get_str("title"), Some("Spotlight: passthrough"));
        assert!(!notice.handle_action());
        assert!(notice.render(80).is_empty());
    }

    #[test]
    fn a_changed_notice_is_accent_and_carries_a_hover_card() {
        let notice = SpotlightNotice::new(
            "call_2",
            SpotlightDetails {
                tool: "read".into(),
                in_tokens: 20_000,
                out_tokens: 1_000,
                saved_tokens: 19_000,
                original: Some(original()),
                ..SpotlightDetails::default()
            },
            "reduced",
        );
        let node = notice.describe_native(&DescribeContext::new(80));
        assert_eq!(node.props.tone(), Some(Tone::Accent));
        assert!(node.props.contains_key("hovercard"));
        let spans = notice_spans(notice.spotlight());
        assert_eq!(spans[0].t, SHOOTING_STAR);
        assert!(spans[1].t.contains("saved"));
    }

    #[test]
    fn held_out_notices_report_a_would_be_saving() {
        let details = SpotlightDetails {
            holdout: true,
            in_tokens: 5_000,
            out_tokens: 1_000,
            saved_tokens: 0,
            original: Some(original()),
            ..SpotlightDetails::default()
        };
        let spans = notice_spans(&details);
        assert_eq!(spans[0].t, STAR_OFF);
        assert!(spans[1].t.contains("held out"));
        assert!(spans[1].t.contains("would save"));
        let notice = SpotlightNotice::new("call_3", details, "reduced");
        assert_eq!(
            notice.describe_native(&DescribeContext::new(80)).props.tone(),
            Some(Tone::Muted)
        );
    }

    #[test]
    fn the_notices_registry_tracks_calls_in_first_seen_order() {
        let trace = json!({"spotlight": {"tool": "read", "savedTokens": 10}});
        let plain = json!({"other": true});
        let mut notices = SpotlightNotices::new();
        assert!(!notices.set("a", &plain, "x"));
        assert!(notices.set("a", &trace, "x"));
        assert!(notices.set("b", &trace, "y"));
        assert_eq!(notices.len(), 2);
        assert!(!notices.set("a", &trace, "z"));
        let ids: Vec<&str> = notices.list(None).into_iter().map(SpotlightNotice::tool_call_id).collect();
        assert_eq!(ids.join(","), "a,b");
        let shown = notices.list(Some(&|id| id == "b"));
        assert_eq!(shown.len(), 1);
        assert!(notices.set("a", &plain, "x"));
        assert_eq!(notices.len(), 1);
        notices.set_hidden(true);
        assert!(notices.list(None)[0].is_hidden());
    }
}
