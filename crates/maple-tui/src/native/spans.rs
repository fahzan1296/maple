//! ANSI-styled text → escape-free spans and plain text.
//!
//! Callers hand the native path text that is already styled for the ANSI
//! renderer (`theme.fg("toolTitle", x)`, opaque stylers). Native nodes carry
//! no escapes, so [`styled_spans`] parses SGR and OSC 8 back into spans whose
//! `s` tokens are the theme colour names the escapes came from (reverse-mapped
//! through the supplied theme), plus the semantic attribute tokens (`strong`,
//! `em`, `dim`, `del`, `mark`). Every string is sanitized the same way:
//! escapes dropped, control characters removed, tabs expanded.
//!
//! Reference: `l/src/native/spans.ts`.
//!
//! Deviation: the reference reads the process-global `theme` singleton, so its
//! functions take no theme argument; maple-tui has no global theme, so every
//! function that resolves colours takes `&Theme`. Rust strings are UTF-8, so
//! the reference's lone-surrogate removal has no equivalent.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::native::describe;
use crate::native::node::{NativeNode, Props, Span, Text};
use crate::render::render_utils::replace_tabs;
use crate::theme::schema::{is_valid_theme_color, ThemeBg, ThemeColor};
use crate::theme::Theme;

/// Background tokens in reverse-map tie-breaking priority.
const THEME_BG_NAMES: &[ThemeBg] = &[
	ThemeBg::UserMessageBg,
	ThemeBg::ToolErrorBg,
	ThemeBg::ToolSuccessBg,
	ThemeBg::ToolPendingBg,
	ThemeBg::CustomMessageBg,
	ThemeBg::SelectedBg,
	ThemeBg::StatusLineBg,
];

/// CSI, OSC and DCS/APC/PM/SOS strings (BEL or ST terminated: the TUI's
/// cursor marker is a BEL-terminated APC), and two-byte escapes.
static ESCAPE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(
		r"\x1b(?:\[([0-?]*)[ -/]*([@-~])|]([^\x07\x1b]*)(?:\x07|\x1b\\)|[P_^X][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\x5c-\x5f])",
	)
	.expect("valid escape regex")
});

/// A run of `\r`/`\n` in a label.
static LINE_BREAKS: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"[\r\n]+").expect("valid regex"));

/// A run of whitespace in a label.
static WHITESPACE_RUNS: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));

/// Reverse colour maps: canonical SGR key → theme token.
struct ThemeReverse {
	fg: HashMap<String, ThemeColor>,
	bg: HashMap<String, ThemeBg>,
}

impl ThemeReverse {
	/// Build the reverse maps for `theme`; the first token wins a key.
	fn for_theme(theme: &Theme) -> Self {
		let mut reverse = ThemeReverse {
			fg: HashMap::new(),
			bg: HashMap::new(),
		};
		for color in ThemeColor::ALL {
			let Ok(escape) = theme.get_fg_ansi(*color) else {
				continue;
			};
			let Some(key) = escape_color_key(&escape) else {
				continue;
			};
			reverse.fg.entry(key).or_insert(*color);
		}
		for name in THEME_BG_NAMES {
			let Ok(escape) = theme.get_bg_ansi(*name) else {
				continue;
			};
			let Some(key) = escape_color_key(&escape) else {
				continue;
			};
			reverse.bg.entry(key).or_insert(*name);
		}
		reverse
	}
}

/// SGR attribute and colour state carried between escapes.
struct SgrState {
	bold: bool,
	dim: bool,
	italic: bool,
	strike: bool,
	inverse: bool,
	fg: Option<String>,
	bg: Option<String>,
	href: Option<String>,
}

impl SgrState {
	fn initial() -> Self {
		SgrState {
			bold: false,
			dim: false,
			italic: false,
			strike: false,
			inverse: false,
			fg: None,
			bg: None,
			href: None,
		}
	}
}

/// A canonical colour key plus how many parameter slots it consumed.
struct ColorKey {
	key: String,
	consumed: usize,
}

/// Split an SGR parameter string on `;` and `:`; an empty string is `[0]`.
fn parse_params(raw: &str) -> Vec<i64> {
	if raw.is_empty() {
		return vec![0];
	}
	raw.split([';', ':'])
		.map(|part| part.parse::<i64>().unwrap_or(0))
		.collect()
}

/// Canonical colour key of the extended colour selector at `index`.
fn color_key(params: &[i64], index: usize) -> Option<ColorKey> {
	let mode = *params.get(index + 1)?;
	if mode == 5 {
		let index_value = *params.get(index + 2)?;
		return Some(ColorKey {
			key: format!("5;{}", index_value),
			consumed: 2,
		});
	}
	if mode == 2 {
		let red = *params.get(index + 2)?;
		let green = *params.get(index + 3)?;
		let blue = *params.get(index + 4)?;
		return Some(ColorKey {
			key: format!("2;{};{};{}", red, green, blue),
			consumed: 4,
		});
	}
	None
}

/// Colour key of a single-colour escape like `\x1b[38;2;1;2;3m`; `None` for
/// default/reset escapes.
fn escape_color_key(escape: &str) -> Option<String> {
	let rest = escape.strip_prefix("\x1b[")?;
	let raw = rest.strip_suffix('m')?;
	if !raw
		.chars()
		.all(|ch| ch.is_ascii_digit() || ch == ';' || ch == ':')
	{
		return None;
	}
	let params = parse_params(raw);
	let first = params.first().copied()?;
	if first == 38 || first == 48 {
		return color_key(&params, 0).map(|color| color.key);
	}
	if (30..=37).contains(&first) || (90..=97).contains(&first) {
		return Some(format!("b;{}", first));
	}
	if (40..=47).contains(&first) || (100..=107).contains(&first) {
		return Some(format!("b;{}", first - 10));
	}
	None
}

/// Reset the attributes and colours, leaving `href` alone.
fn reset_state(state: &mut SgrState) {
	state.bold = false;
	state.dim = false;
	state.italic = false;
	state.strike = false;
	state.inverse = false;
	state.fg = None;
	state.bg = None;
}

/// Apply one SGR parameter string to `state`.
fn apply_sgr(state: &mut SgrState, raw: &str) {
	let params = parse_params(raw);
	let mut index = 0;
	while index < params.len() {
		let code = params[index];
		match code {
			0 => reset_state(state),
			1 => state.bold = true,
			2 => state.dim = true,
			3 => state.italic = true,
			7 => state.inverse = true,
			9 => state.strike = true,
			22 => {
				state.bold = false;
				state.dim = false;
			}
			23 => state.italic = false,
			27 => state.inverse = false,
			29 => state.strike = false,
			30..=37 | 90..=97 => state.fg = Some(format!("b;{}", code)),
			39 => state.fg = None,
			40..=47 | 100..=107 => state.bg = Some(format!("b;{}", code - 10)),
			49 => state.bg = None,
			38 | 48 => {
				if let Some(color) = color_key(&params, index) {
					if code == 38 {
						state.fg = Some(color.key);
					} else {
						state.bg = Some(color.key);
					}
					index += color.consumed;
				}
			}
			_ => {}
		}
		index += 1;
	}
}

/// Basic 16-colour SGR foreground code → semantic token.
fn basic_fg_token(code: i64) -> Option<&'static str> {
	match code {
		31 => Some("error"),
		32 => Some("success"),
		33 => Some("warning"),
		34 => Some("info"),
		35 => Some("accent"),
		36 => Some("info"),
		90 => Some("dim"),
		91 => Some("error"),
		92 => Some("success"),
		93 => Some("warning"),
		94 => Some("info"),
		95 => Some("accent"),
		96 => Some("info"),
		_ => None,
	}
}

/// The span style token for a colour key, reverse-mapped through `theme`.
fn fg_token(key: Option<&str>, reverse: &ThemeReverse) -> Option<&'static str> {
	let key = key?;
	if let Some(named) = reverse.fg.get(key) {
		let token = named.as_str();
		// `text` is the terminal's own foreground: no information here.
		return if token == "text" { None } else { Some(token) };
	}
	let code = key.strip_prefix("b;")?.parse::<i64>().ok()?;
	basic_fg_token(code)
}

/// The space-joined style tokens for an SGR state, or `None` when unstyled.
fn style_tokens(state: &SgrState, reverse: &ThemeReverse) -> Option<String> {
	let mut tokens: Vec<&str> = Vec::new();
	if let Some(fg) = fg_token(state.fg.as_deref(), reverse) {
		tokens.push(fg);
	}
	if state.bold {
		tokens.push("strong");
	}
	if state.dim {
		tokens.push("dim");
	}
	if state.italic {
		tokens.push("em");
	}
	if state.strike {
		tokens.push("del");
	}
	let selected_bg = state
		.bg
		.as_deref()
		.map_or(false, |bg| reverse.bg.get(bg) == Some(&ThemeBg::SelectedBg));
	if state.inverse || selected_bg {
		tokens.push("mark");
	}
	if tokens.is_empty() {
		None
	} else {
		Some(tokens.join(" "))
	}
}

/// Strip C0/C1 control characters, keeping tab and line breaks.
fn strip_control(content: &str) -> String {
	content
		.chars()
		.filter(|ch| !ch.is_control() || *ch == '\t' || *ch == '\n' || *ch == '\r')
		.collect()
}

/// Escape-free display text: control characters removed, tabs expanded.
fn clean(content: &str) -> String {
	replace_tabs(&strip_control(content))
}

/// Append a cleaned run, coalescing it into the previous span when the style
/// and link target match.
fn push_span(spans: &mut Vec<Span>, content: &str, s: Option<&str>, href: Option<&str>) {
	let cleaned = clean(content);
	if cleaned.is_empty() {
		return;
	}
	if let Some(last) = spans.last_mut() {
		if last.s.as_deref() == s && last.href.as_deref() == href {
			last.t.push_str(&cleaned);
			return;
		}
	}
	let mut span = Span::new(cleaned);
	span.s = s.map(str::to_string);
	span.href = href.map(str::to_string);
	spans.push(span);
}

/// The URI of an OSC 8 payload, or `None` for a non-link OSC string. The inner
/// `Option` is the cleared link (`None`) or the target (`Some`).
fn osc8_uri(payload: &str) -> Option<Option<String>> {
	let rest = payload.strip_prefix("8;")?;
	let uri = match rest.find(';') {
		Some(index) => &rest[index + 1..],
		None => payload,
	};
	Some(if uri.is_empty() {
		None
	} else {
		Some(uri.to_string())
	})
}

/// Convert ANSI-styled text into escape-free spans. SGR colours map back to
/// the theme tokens that produced them; OSC 8 becomes `href`; every other
/// escape (cursor moves, APC markers) is dropped.
pub fn styled_spans(styled: &str, theme: &Theme) -> Vec<Span> {
	let mut spans = Vec::new();
	if !styled.contains('\x1b') {
		push_span(&mut spans, styled, None, None);
		return spans;
	}
	let reverse = ThemeReverse::for_theme(theme);
	let mut state = SgrState::initial();
	let mut cursor = 0;
	for caps in ESCAPE_PATTERN.captures_iter(styled) {
		let Some(whole) = caps.get(0) else {
			continue;
		};
		if whole.start() > cursor {
			push_span(
				&mut spans,
				&styled[cursor..whole.start()],
				style_tokens(&state, &reverse).as_deref(),
				state.href.as_deref(),
			);
		}
		cursor = whole.end();
		if let Some(final_byte) = caps.get(2) {
			if final_byte.as_str() == "m" {
				apply_sgr(&mut state, caps.get(1).map_or("", |group| group.as_str()));
			}
		} else if let Some(payload) = caps.get(3) {
			if let Some(uri) = osc8_uri(payload.as_str()) {
				state.href = uri;
			}
		}
	}
	if cursor < styled.len() {
		push_span(
			&mut spans,
			&styled[cursor..],
			style_tokens(&state, &reverse).as_deref(),
			state.href.as_deref(),
		);
	}
	spans
}

/// The visible characters of ANSI-styled text: every escape dropped,
/// sanitized like [`styled_spans`].
pub fn plain_text(styled: &str) -> String {
	let stripped = if styled.contains('\x1b') {
		ESCAPE_PATTERN.replace_all(styled, "").into_owned()
	} else {
		styled.to_string()
	};
	clean(&stripped)
}

/// [`plain_text`] as a single-line label: line breaks and whitespace runs
/// collapse to one space.
pub fn plain_line(styled: &str) -> String {
	let text = plain_text(styled);
	let flat = LINE_BREAKS.replace_all(&text, " ");
	WHITESPACE_RUNS.replace_all(&flat, " ").trim().to_string()
}

/// Spans for plain text passed through an optional ANSI styler.
pub fn style_spans<F: Fn(&str) -> String>(content: &str, style: Option<F>, theme: &Theme) -> Vec<Span> {
	match style {
		Some(style) => styled_spans(&style(content), theme),
		None => styled_spans(content, theme),
	}
}

/// Collapse unstyled single-span text to a plain string (smaller frames).
pub fn compact_text(spans: &[Span]) -> Text {
	if spans.is_empty() {
		return Text::Plain(String::new());
	}
	if spans.len() == 1 {
		let only = &spans[0];
		if only.s.is_none() && only.fx.is_none() && only.href.is_none() {
			return Text::Plain(only.t.clone());
		}
	}
	Text::Spans(spans.to_vec())
}

/// Pre-rendered ANSI rows as one unwrapped text block (rows already carry
/// their line breaks).
pub fn rows_text(lines: &[String], theme: &Theme) -> NativeNode {
	let joined = lines.join("\n");
	let text = compact_text(&styled_spans(&joined, theme));
	let mut props = Props::default();
	props.set_str("wrap", "none");
	describe::text(text, props)
}

/// Semantic span tokens drawn with the theme colour that stands for them.
fn semantic_fg(token: &str) -> Option<ThemeColor> {
	match token {
		"info" => Some(ThemeColor::Accent),
		"link" => Some(ThemeColor::MdLink),
		"code" | "mono" => Some(ThemeColor::MdCode),
		"path" | "num" => Some(ThemeColor::Accent),
		"ins" => Some(ThemeColor::ToolDiffAdded),
		"del" => Some(ThemeColor::ToolDiffRemoved),
		_ => None,
	}
}

/// Styled text as one ANSI terminal line: the reverse of [`styled_spans`].
/// Each span takes the last theme colour (or semantic colour token) in `s`,
/// bold for `strong` and italic for `em`; text is sanitized like
/// [`plain_text`] and line breaks become spaces.
pub fn spans_line(content: &Text, theme: &Theme) -> String {
	let spans = match content {
		Text::Plain(text) => return plain_line(text),
		Text::Spans(spans) => spans,
	};
	let mut out = String::new();
	for part in spans {
		let mut value = clean(&part.t);
		value = LINE_BREAKS.replace_all(&value, " ").into_owned();
		if value.is_empty() {
			continue;
		}
		let mut color: Option<ThemeColor> = None;
		let mut bold = false;
		let mut italic = false;
		if let Some(tokens) = &part.s {
			for token in tokens.split(' ') {
				if token == "strong" {
					bold = true;
				} else if token == "em" {
					italic = true;
				} else if is_valid_theme_color(token) {
					color = ThemeColor::from_str(token);
				} else if let Some(semantic) = semantic_fg(token) {
					color = Some(semantic);
				}
			}
		}
		if let Some(named) = color {
			if let Ok(painted) = theme.fg(named, &value) {
				value = painted;
			}
		}
		if bold {
			value = theme.bold(&value);
		}
		if italic {
			value = theme.italic(&value);
		}
		out.push_str(&value);
	}
	out
}

/// Theme background token of the first SGR background in ANSI-styled text.
pub fn styled_background(styled: &str, theme: &Theme) -> Option<ThemeBg> {
	let reverse = ThemeReverse::for_theme(theme);
	for caps in ESCAPE_PATTERN.captures_iter(styled) {
		let Some(final_byte) = caps.get(2) else {
			continue;
		};
		if final_byte.as_str() != "m" {
			continue;
		}
		let mut state = SgrState::initial();
		apply_sgr(&mut state, caps.get(1).map_or("", |group| group.as_str()));
		if let Some(bg) = state.bg.as_deref() {
			return reverse.bg.get(bg).copied();
		}
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::native::node::NodeKind;
	use crate::theme::color::{ColorMode, ColorValue};
	use crate::theme::schema::ColorMap;
	use crate::theme::symbols::{NormalizedSpinnerFrames, SymbolPreset};

	fn test_theme() -> Theme {
		let mut fg = ColorMap::new();
		for (key, hex) in [
			("accent", "#febc38"),
			("text", "#e5e5e7"),
			("error", "#fc3a4b"),
			("success", "#89d281"),
			("warning", "#e4c00f"),
			("mdLink", "#0088fa"),
			("mdCode", "#e5c1ff"),
			("toolDiffAdded", "#89d281"),
			("toolDiffRemoved", "#fc3a4b"),
		] {
			fg.insert(key.to_string(), ColorValue::Hex(hex.to_string()));
		}
		fg.insert("muted".to_string(), ColorValue::Index(42));
		let mut bg = ColorMap::new();
		bg.insert("statusLineBg".to_string(), ColorValue::Hex("#121212".to_string()));
		bg.insert("selectedBg".to_string(), ColorValue::Hex("#31363f".to_string()));
		bg.insert(
			"userMessageBg".to_string(),
			ColorValue::Hex("#1b1e28".to_string()),
		);
		Theme::new(
			&fg,
			&bg,
			ColorMode::TrueColor,
			SymbolPreset::Unicode,
			&HashMap::<String, String>::new(),
			&NormalizedSpinnerFrames::default(),
			&[],
		)
		.expect("test theme resolves")
	}

	fn styled(t: &str, s: &str) -> Span {
		Span::styled(t, s)
	}

	#[test]
	fn plain_text_passes_through_without_escapes() {
		let theme = test_theme();
		assert_eq!(plain_text("hello world"), "hello world");
		assert_eq!(styled_spans("hello", &theme), vec![Span::new("hello")]);
		assert_eq!(plain_line("  hello   world  "), "hello world");
	}

	#[test]
	fn plain_text_drops_every_escape_kind() {
		assert_eq!(plain_text("\x1b[31mred\x1b[0m"), "red");
		assert_eq!(plain_text("\x1b]8;;https://x\x07link\x1b]8;;\x07"), "link");
		assert_eq!(plain_text("a\x1b_pi:c\x07b"), "ab");
		assert_eq!(plain_text("a\x1b_tsp;x\x1b\\b"), "ab");
		assert_eq!(plain_text("a\x1bMb"), "ab");
		assert_eq!(plain_text("a\x1bPq\x07b"), "ab");
	}

	#[test]
	fn sgr_foreground_maps_to_semantic_token() {
		let theme = test_theme();
		let spans = styled_spans("\x1b[31mred\x1b[0m plain", &theme);
		assert_eq!(spans, vec![styled("red", "error"), Span::new(" plain")]);
		assert_eq!(
			styled_spans("\x1b[92mgreen\x1b[0m", &theme),
			vec![styled("green", "success")]
		);
	}

	#[test]
	fn extended_foreground_reverse_maps_to_theme_token() {
		let theme = test_theme();
		assert_eq!(
			styled_spans("\x1b[38;2;254;188;56maccent\x1b[39m", &theme),
			vec![styled("accent", "accent")]
		);
		assert_eq!(
			styled_spans("\x1b[38;5;42mmuted\x1b[39m", &theme),
			vec![styled("muted", "muted")]
		);
	}

	#[test]
	fn terminal_default_text_maps_to_no_token() {
		let theme = test_theme();
		assert_eq!(
			styled_spans("\x1b[38;2;229;229;231mtext\x1b[39m", &theme),
			vec![Span::new("text")]
		);
	}

	#[test]
	fn background_reverse_maps_and_marks_selection() {
		let theme = test_theme();
		assert_eq!(
			styled_spans("\x1b[48;2;49;54;63mpick\x1b[49m", &theme),
			vec![styled("pick", "mark")]
		);
		assert_eq!(
			styled_spans("\x1b[48;2;27;30;40mhi\x1b[49m", &theme),
			vec![Span::new("hi")]
		);
	}

	#[test]
	fn osc8_sets_and_clears_href() {
		let theme = test_theme();
		let spans = styled_spans(
			"\x1b]8;;https://example.com\x07link\x1b]8;;\x07tail",
			&theme,
		);
		assert_eq!(
			spans,
			vec![
				Span {
					t: "link".to_string(),
					s: None,
					fx: None,
					href: Some("https://example.com".to_string()),
				},
				Span::new("tail"),
			]
		);
		let st_terminated =
			styled_spans("\x1b]8;id=1;https://x\x1b\\go\x1b]8;;\x1b\\", &theme);
		assert_eq!(
			st_terminated,
			vec![Span {
				t: "go".to_string(),
				s: None,
				fx: None,
				href: Some("https://x".to_string()),
			}]
		);
	}

	#[test]
	fn attribute_tokens_are_emitted_in_order() {
		let theme = test_theme();
		assert_eq!(
			styled_spans("\x1b[1;3;9;7mX\x1b[0m", &theme),
			vec![styled("X", "strong em del mark")]
		);
		assert_eq!(
			styled_spans("\x1b[2mdim\x1b[22m\x1b[3mem\x1b[23m", &theme),
			vec![styled("dim", "dim"), styled("em", "em")]
		);
	}

	#[test]
	fn adjacent_runs_coalesce() {
		let theme = test_theme();
		assert_eq!(
			styled_spans("\x1b[31ma\x1b[31mb\x1b[0mc", &theme),
			vec![styled("ab", "error"), Span::new("c")]
		);
		assert_eq!(
			styled_spans("a\x1b]8;;u\x07b\x1b]8;;\x07c", &theme),
			vec![Span::new("a"), styled_span_with_href("b", "u"), Span::new("c")]
		);
	}

	#[test]
	fn control_characters_are_stripped_and_tabs_expanded() {
		assert_eq!(plain_text("a\u{1}b\tc"), "ab   c");
		assert_eq!(plain_text("a\u{7f}b"), "ab");
		assert_eq!(plain_text("a\nb"), "a\nb");
		assert_eq!(
			styled_spans("x\ty", &test_theme())
				.first()
				.map(|span| span.t.as_str()),
			Some("x   y")
		);
	}

	#[test]
	fn plain_line_collapses_breaks_and_whitespace() {
		assert_eq!(plain_line("a \r\n b\t c"), "a b c");
		assert_eq!(plain_line("line1\nline2"), "line1 line2");
	}

	#[test]
	fn compact_text_collapses_only_plain_single_spans() {
		assert_eq!(compact_text(&[]), Text::Plain(String::new()));
		assert_eq!(compact_text(&[Span::new("x")]), Text::Plain("x".to_string()));
		assert_eq!(
			compact_text(&[styled("x", "accent")]),
			Text::Spans(vec![styled("x", "accent")])
		);
		let linked = styled_span_with_href("x", "u");
		assert_eq!(compact_text(&[linked.clone()]), Text::Spans(vec![linked]));
	}

	#[test]
	fn rows_text_is_one_unwrapped_block() {
		let theme = test_theme();
		let node = rows_text(&["a".to_string(), "b".to_string()], &theme);
		assert_eq!(node.kind, NodeKind::Text);
		assert_eq!(node.props.get_str("wrap"), Some("none"));
		assert_eq!(node.text(), Some("a\nb"));
		let node = rows_text(&["\x1b[31ma".to_string(), "b".to_string()], &theme);
		assert_eq!(
			node.props.spans(),
			Some(vec![styled("a\nb", "error")])
		);
	}

	#[test]
	fn spans_line_round_trips_strings_and_spans() {
		let theme = test_theme();
		assert_eq!(
			spans_line(&Text::Plain("a\x1b[31mb\x1b[0m".to_string()), &theme),
			"ab"
		);
		assert_eq!(
			spans_line(
				&Text::Spans(vec![styled("x", "strong"), styled("y", "em")]),
				&theme
			),
			"\x1b[1mx\x1b[22m\x1b[3my\x1b[23m"
		);
		assert_eq!(
			spans_line(
				&Text::Spans(vec![Span::new("a\nb"), Span::new("\u{1}"), Span::new("c")]),
				&theme
			),
			"a bc"
		);
	}

	#[test]
	fn spans_line_uses_last_colour_and_semantic_tokens() {
		let theme = test_theme();
		let spans = vec![
			styled("i", "info"),
			styled("c", "code"),
			styled("b", "accent error"),
		];
		assert_eq!(
			spans_line(&Text::Spans(spans), &theme),
			"\x1b[38;2;254;188;56mi\x1b[39m\x1b[38;2;229;193;255mc\x1b[39m\x1b[38;2;252;58;75mb\x1b[39m"
		);
	}

	#[test]
	fn styled_background_reports_the_first_background() {
		let theme = test_theme();
		assert_eq!(
			styled_background("\x1b[48;2;49;54;63mpick", &theme),
			Some(ThemeBg::SelectedBg)
		);
		assert_eq!(
			styled_background("\x1b[48;2;27;30;40mhi", &theme),
			Some(ThemeBg::UserMessageBg)
		);
		assert_eq!(
			styled_background("\x1b[31m\x1b[48;2;49;54;63m", &theme),
			Some(ThemeBg::SelectedBg)
		);
		assert_eq!(styled_background("\x1b[31mred\x1b[0m", &theme), None);
		assert_eq!(styled_background("plain", &theme), None);
		assert_eq!(styled_background("\x1b[41m", &theme), None);
	}

	#[test]
	fn style_spans_applies_the_optional_styler() {
		let theme = test_theme();
		let spans = style_spans("x", Some(|text: &str| format!("\x1b[31m{}\x1b[0m", text)), &theme);
		assert_eq!(spans, vec![styled("x", "error")]);
		let plain = style_spans("x", None::<fn(&str) -> String>, &theme);
		assert_eq!(plain, vec![Span::new("x")]);
	}

	fn styled_span_with_href(t: &str, href: &str) -> Span {
		Span {
			t: t.to_string(),
			s: None,
			fx: None,
			href: Some(href.to_string()),
		}
	}
}
