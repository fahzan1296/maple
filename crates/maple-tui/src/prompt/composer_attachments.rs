//! Reference: `l/src/prompt/composer-attachments.ts`.
//!
//! Composer attachment vocabulary: chip labels/tokens, stable chip colors, skill
//! and model-mention pills, marker collapsing/compaction, and the placeholder
//! renderer.
//!
//! Deviations:
//! - There is no global theme in this crate ([`Theme`] holds `RefCell`s and is not
//!   `Sync`), so every styling helper takes `Option<&Theme>`; `None` is the
//!   reference's `typeof theme === "undefined"` path (bare label, unicode glyph).
//! - The `regex` crate has no lookbehind/lookahead/backreferences. Word-glyph
//!   boundaries are enforced by [`valid_start`]/[`valid_end`] after matching, and
//!   the `attachment://N` backreference becomes an explicit index comparison.
//! - `register` callbacks are `&mut dyn FnMut`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::prompt::skill_tokens::{allows_model_mentions, allows_skill_tokens};
use crate::prompt::model_mention_syntax::{MODEL_MENTION_RE, model_mention_token};
use crate::theme::symbols::{preset_symbols, SymbolPreset};
use crate::theme::theme_class::Theme;

/// Attachment chip kinds staged in the composer: images, video previews, large
/// text pastes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChipKind {
	Image,
	Video,
	Paste,
}

impl ChipKind {
	pub fn as_str(self) -> &'static str {
		match self {
			ChipKind::Image => "image",
			ChipKind::Video => "video",
			ChipKind::Paste => "paste",
		}
	}

	pub fn from_str(name: &str) -> Option<Self> {
		match name {
			"image" => Some(ChipKind::Image),
			"video" => Some(ChipKind::Video),
			"paste" => Some(ChipKind::Paste),
			_ => None,
		}
	}

	fn icon_key(self) -> &'static str {
		match self {
			ChipKind::Image => "chip.image",
			ChipKind::Video => "chip.video",
			ChipKind::Paste => "chip.paste",
		}
	}
}

fn preset_glyph(preset: SymbolPreset, key: &str) -> String {
	preset_symbols(preset).get(key).unwrap_or("").to_string()
}

/// Compact atomic composer token for an invoked skill in the active symbol
/// preset.
pub fn skill_chip_label(name: &str, theme: Option<&Theme>) -> String {
	let icon = match theme {
		Some(theme) => theme.symbol("icon.extensionSkill").to_string(),
		None => preset_glyph(SymbolPreset::Unicode, "icon.extensionSkill"),
	};
	format!("{icon} {name}")
}

/// Compact atomic composer token for a model mention in the active symbol preset.
pub fn model_mention_chip_label(name: &str, theme: Option<&Theme>) -> String {
	let icon = match theme {
		Some(theme) => theme.symbol("icon.model").to_string(),
		None => preset_glyph(SymbolPreset::Unicode, "icon.model"),
	};
	format!("{icon} {name}")
}

/// Canonical `/skill:<name>` token a skill chip expands to on submit.
pub fn skill_token(name: &str) -> String {
	format!("/skill:{name}")
}

fn chip_pill_style(label: &str, color: &str, restore: &str, theme: Option<&Theme>) -> String {
	let Some(theme) = theme else {
		return label.to_string();
	};
	let background = theme.get_bg_ansi("customMessageBg").unwrap_or_default();
	let foreground = theme.get_fg_ansi(color).unwrap_or_default();
	format!("{background}{foreground}\x1b[1m{label}\x1b[22m{restore}")
}

/// Soft-pill styling for a skill chip: bold skill label color over the
/// custom-message tint. `restore` re-arms the surrounding colors after the chip.
pub fn skill_chip_style(label: &str, restore: &str, theme: Option<&Theme>) -> String {
	chip_pill_style(label, "customMessageLabel", restore, theme)
}

/// Soft-pill styling for a model mention, restoring the surrounding colors
/// afterward.
pub fn model_chip_style(label: &str, restore: &str, theme: Option<&Theme>) -> String {
	chip_pill_style(label, "statusLineModel", restore, theme)
}

/// Default restore sequence (`\x1b[39m\x1b[49m`).
pub const CHIP_STYLE_RESTORE: &str = "\x1b[39m\x1b[49m";

static SKILL_ICONS: LazyLock<Vec<String>> = LazyLock::new(|| {
	let mut seen: Vec<String> = Vec::new();
	for preset in [SymbolPreset::Unicode, SymbolPreset::Nerd, SymbolPreset::Ascii] {
		let icon = preset_glyph(preset, "icon.extensionSkill");
		if !seen.contains(&icon) {
			seen.push(icon);
		}
	}
	seen
});

static CHIP_ICONS: LazyLock<HashMap<ChipKind, Vec<String>>> = LazyLock::new(|| {
	let mut map: HashMap<ChipKind, Vec<String>> = HashMap::new();
	for kind in [ChipKind::Image, ChipKind::Video, ChipKind::Paste] {
		let mut icons: Vec<String> = Vec::new();
		for preset in [SymbolPreset::Unicode, SymbolPreset::Nerd, SymbolPreset::Ascii] {
			let icon = preset_glyph(preset, kind.icon_key());
			if !icons.contains(&icon) {
				icons.push(icon);
			}
		}
		map.insert(kind, icons);
	}
	map
});

/// Skill names as they appear in chips: word characters and dashes, dots only
/// between segments, optionally behind one collision namespace.
const SKILL_NAME_SOURCE: &str = "[\\w-]+(?:\\.[\\w-]+)*(?:/[\\w-]+(?:\\.[\\w-]+)*)?";

fn glyph_source(icon: &str) -> String {
	regex::escape(icon)
}

fn is_word_glyph(icon: &str) -> bool {
	!icon.is_empty() && icon.chars().all(|c| c.is_ascii_alphabetic())
}

/// A word glyph must not continue a preceding word.
fn valid_start(text: &str, start: usize, icon: &str) -> bool {
	if !is_word_glyph(icon) || start == 0 {
		return true;
	}
	!text[..start]
		.chars()
		.next_back()
		.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A chip/skill token must not run into a following word character.
fn valid_end(text: &str, end: usize) -> bool {
	!text[end..]
		.chars()
		.next()
		.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

static SKILL_CHIP_SOURCE: LazyLock<String> = LazyLock::new(|| {
	let alternatives: Vec<String> = SKILL_ICONS.iter().map(|icon| glyph_source(icon)).collect();
	format!("(?:{}) ({SKILL_NAME_SOURCE})", alternatives.join("|"))
});

/// Replaces `/skill:<name>` tokens for known skills with compact chip labels and
/// registers each label's token as its atomic editor expansion. Leaves the text
/// untouched when skill tokens are not invocations in this draft.
pub fn collapse_skill_tokens(
	text: &str,
	is_known: &dyn Fn(&str) -> bool,
	register: &mut dyn FnMut(&str, &str),
) -> String {
	if !text.contains("/skill:") || !allows_skill_tokens(text) {
		return text.to_string();
	}
	let tokens = crate::prompt::skill_tokens::skill_tokens(text);
	let mut out = String::with_capacity(text.len());
	let mut cursor = 0usize;
	let mut replaced = false;
	for token in tokens {
		if !is_known(&token.name) {
			continue;
		}
		let label = skill_chip_label(&token.name, None);
		register(&label, &skill_token(&token.name));
		// Keep the leading delimiter, replace from the `/skill:` sigil.
		out.push_str(&text[cursor..token.sigil_start()]);
		out.push_str(&label);
		cursor = token.end;
		replaced = true;
	}
	if !replaced {
		return text.to_string();
	}
	out.push_str(&text[cursor..]);
	out
}

/// Replaces mentionable model selectors with their display chips and registers
/// each chip's canonical `^selector` as its atomic editor expansion.
pub fn collapse_model_mentions(
	text: &str,
	chip_for: &dyn Fn(&str) -> Option<(String, String)>,
	register: &mut dyn FnMut(&str, &str),
) -> String {
	if !text.contains('^') || !allows_model_mentions(text) {
		return text.to_string();
	}
	MODEL_MENTION_RE
		.replace_all(text, |caps: &regex::Captures<'_>| {
			let whole = caps.get(0).map(|m| m.as_str()).unwrap_or("");
			let delimiter = caps.get(1).map(|m| m.as_str()).unwrap_or("");
			let selector = caps.get(2).map(|m| m.as_str()).unwrap_or("");
			match chip_for(selector) {
				Some((label, selector)) => {
					register(&label, &model_mention_token(&selector));
					format!("{delimiter}{label}")
				}
				None => whole.to_string(),
			}
		})
		.into_owned()
}

/// Chip glyph the active theme renders for `kind`, including `symbols.overrides`.
fn active_chip_icon(kind: ChipKind, theme: Option<&Theme>) -> String {
	match theme {
		Some(theme) => theme.symbol(kind.icon_key()).to_string(),
		None => preset_glyph(SymbolPreset::Unicode, kind.icon_key()),
	}
}

/// Compact atomic composer token for attachment `n` in the active symbol preset.
pub fn chip_label(kind: ChipKind, n: i64, theme: Option<&Theme>) -> String {
	format!("{} #{n}", active_chip_icon(kind, theme))
}

static CHIP_TOKEN_SOURCE: LazyLock<String> = LazyLock::new(|| {
	let mut alternatives: Vec<String> = Vec::new();
	for icons in CHIP_ICONS.values() {
		for icon in icons {
			alternatives.push(glyph_source(icon));
		}
	}
	format!("(?:{}) #[1-9]\\d*", alternatives.join("|"))
});

/// Infers an attachment kind from a chip label emitted by any configured symbol
/// preset.
pub fn chip_label_kind(label: &str) -> ChipKind {
	for (kind, icons) in CHIP_ICONS.iter() {
		if icons.iter().any(|icon| label.starts_with(icon.as_str())) {
			return *kind;
		}
	}
	ChipKind::Paste
}

const ATTACHMENT_PALETTE: [(u8, u8, u8); 6] = [
	(255, 179, 102),
	(125, 207, 255),
	(189, 147, 249),
	(105, 220, 158),
	(255, 141, 188),
	(240, 223, 120),
];

/// Stable RGB color assigned to attachment `n`; each attachment type uses a
/// distinct palette offset.
pub fn attachment_rgb(kind: ChipKind, n: i64) -> (u8, u8, u8) {
	let len = ATTACHMENT_PALETTE.len() as i64;
	let index = match kind {
		ChipKind::Image => (n - 1).rem_euclid(len),
		ChipKind::Video => (n + 1).rem_euclid(len),
		ChipKind::Paste => (n + 2).rem_euclid(len),
	};
	ATTACHMENT_PALETTE[index as usize]
}

/// ANSI truecolor foreground sequence for the color assigned by
/// [`attachment_rgb`].
pub fn attachment_sgr(kind: ChipKind, n: i64) -> String {
	let (r, g, b) = attachment_rgb(kind, n);
	format!("\x1b[38;2;{r};{g};{b}m")
}

/// Matches expanded image, video, and paste markers, including optional marker
/// metadata.
pub static PLACEHOLDER_REGEX: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\[(Image|Video|Paste) #([1-9]\d*)(?:,[^\]\n]*)?\]").expect("placeholder regex"));

/// Matches an expanded attachment marker, a compact attachment chip, or a skill
/// chip.
pub static COMPOSER_TOKEN_REGEX: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(&format!(
		"{}|{}|{}",
		PLACEHOLDER_REGEX.as_str(),
		CHIP_TOKEN_SOURCE.as_str(),
		SKILL_CHIP_SOURCE.as_str()
	))
	.expect("composer token regex")
});

/// Registers the model-chip labels into the composer placeholder matcher.
pub fn composer_token_regex(mention_labels: &[String]) -> Regex {
	let mut labels: Vec<&String> = mention_labels.iter().collect();
	labels.sort_by(|a, b| b.len().cmp(&a.len()));
	labels.dedup();
	if labels.is_empty() {
		return COMPOSER_TOKEN_REGEX.clone();
	}
	let escaped: Vec<String> = labels.iter().map(|label| regex::escape(label)).collect();
	Regex::new(&format!(
		"{}|(?P<mention>{})",
		COMPOSER_TOKEN_REGEX.as_str(),
		escaped.join("|")
	))
	.expect("composer token regex with mentions")
}

/// Attachment indices referenced by a composer buffer, per kind. Image/video
/// count compact chips and expanded markers; paste counts compact chips only,
/// since `[Paste #N]` markers number the base editor's separate paste buffer.
pub fn referenced_attachments(text: &str, recorded: &HashMap<String, ChipKind>) -> HashMap<ChipKind, BTreeSet<i64>> {
	let mut kind_by_icon: HashMap<String, ChipKind> = HashMap::new();
	for (kind, icons) in CHIP_ICONS.iter() {
		for icon in icons {
			kind_by_icon.insert(icon.clone(), *kind);
		}
	}
	for kind in [ChipKind::Image, ChipKind::Video, ChipKind::Paste] {
		kind_by_icon.insert(active_chip_icon(kind, None), kind);
	}
	let mut icons: Vec<String> = kind_by_icon.keys().cloned().collect();
	icons.sort_by(|a, b| b.len().cmp(&a.len()));
	let icon_source = icons
		.iter()
		.map(|icon| glyph_source(icon))
		.collect::<Vec<_>>()
		.join("|");
	let mut labels: Vec<&String> = recorded.keys().collect();
	labels.sort_by(|a, b| b.len().cmp(&a.len()));
	let label_source = labels
		.iter()
		.map(|label| regex::escape(label))
		.collect::<Vec<_>>()
		.join("|");
	let source = if label_source.is_empty() {
		format!(
			"{}|({icon_source}) #([1-9]\\d*)",
			PLACEHOLDER_REGEX.as_str()
		)
	} else {
		format!(
			"{}|({label_source})(\\d*)|({icon_source}) #([1-9]\\d*)",
			PLACEHOLDER_REGEX.as_str()
		)
	};
	let scanner = Regex::new(&source).expect("attachment reference scanner");
	let mut refs: HashMap<ChipKind, BTreeSet<i64>> = HashMap::new();
	for kind in [ChipKind::Image, ChipKind::Video, ChipKind::Paste] {
		refs.entry(kind).or_default();
	}
	for caps in scanner.captures_iter(text) {
		let whole = caps.get(0).expect("whole match");
		match caps.get(1).map(|m| m.as_str()) {
			Some("Image") => {
				let n: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or("").parse().unwrap_or(0);
				refs.entry(ChipKind::Image).or_default().insert(n);
			}
			Some("Video") => {
				let n: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or("").parse().unwrap_or(0);
				refs.entry(ChipKind::Video).or_default().insert(n);
			}
			_ => {}
		}
	// Recorded label: reject when it is a prefix of a longer number. Without any
	// recorded labels the icon/number groups shift down by two.
	let icon_group = if label_source.is_empty() { 3 } else { 5 };
	let number_group = icon_group + 1;
	if !label_source.is_empty() {
		if let Some(label) = caps.get(3).map(|m| m.as_str()) {
			let digits = caps.get(4).map(|m| m.as_str()).unwrap_or("");
			if digits.is_empty() {
				if let Some(kind) = recorded.get(label) {
					if let Some(number) = label.rsplit('#').next().and_then(|n| n.parse::<i64>().ok()) {
						refs.entry(*kind).or_default().insert(number);
					}
				}
			}
			continue;
		}
	}
	if let Some(icon) = caps.get(icon_group).map(|m| m.as_str()) {
		if !valid_start(text, whole.start(), icon) || !valid_end(text, whole.end()) {
			continue;
		}
		if let Some(kind) = kind_by_icon.get(icon) {
			let n: i64 = caps
				.get(number_group)
				.map(|m| m.as_str())
				.unwrap_or("")
				.parse()
				.unwrap_or(0);
			refs.entry(*kind).or_default().insert(n);
		}
	}
	}
	refs
}

static VISION_MARKER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"\[(Image|Video) #([1-9]\d*)((?:,[^\]\n]*)?)\](?: attachment://([1-9]\d*))?")
		.expect("vision marker regex")
});

/// Marker for the Nth attached image or video preview: `[Image #N, WxH]`, or
/// `[Image #N]` without dims.
pub fn format_vision_marker(kind: ChipKind, n: i64, dims: Option<(u32, u32)>) -> String {
	let label = format!("{} #{n}", if kind == ChipKind::Video { "Video" } else { "Image" });
	match dims {
		Some((width, height)) => format!("[{label}, {width}x{height}]"),
		None => format!("[{label}]"),
	}
}

/// Offsets image marker indices, including matching `attachment://` references.
/// With `image_count`, markers above it are left alone.
pub fn shift_image_markers(text: &str, offset: i64, image_count: Option<i64>) -> String {
	if offset == 0 || image_count == Some(0) {
		return text.to_string();
	}
	VISION_MARKER_REGEX
		.replace_all(text, |caps: &regex::Captures<'_>| {
			let whole = caps.get(0).map(|m| m.as_str()).unwrap_or("");
			let kind = caps.get(1).map(|m| m.as_str()).unwrap_or("Image");
			let index: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or("0").parse().unwrap_or(0);
			let tail = caps.get(3).map(|m| m.as_str()).unwrap_or("");
			if image_count.is_some_and(|count| index > count) {
				return whole.to_string();
			}
			let marker = format!("[{kind} #{}", index + offset);
			let marker = format!("{marker}{tail}]");
			match caps.get(4).map(|m| m.as_str()).and_then(|n| n.parse::<i64>().ok()) {
				// The reference ties the attachment index to the marker via a
				// backreference; only rewrite when they agree.
				Some(attachment) if attachment == index => format!("{marker} attachment://{}", attachment + offset),
				_ => marker,
			}
		})
		.into_owned()
}

/// Replaces valid expanded image markers with compact tokens and registers each
/// token's original marker as its atomic editor expansion.
pub fn collapse_image_markers(text: &str, image_count: i64, register: &mut dyn FnMut(&str, &str)) -> String {
	if image_count == 0 {
		return text.to_string();
	}
	VISION_MARKER_REGEX
		.replace_all(text, |caps: &regex::Captures<'_>| {
			let whole = caps.get(0).map(|m| m.as_str()).unwrap_or("");
			let kind = caps.get(1).map(|m| m.as_str()).unwrap_or("Image");
			let index: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or("0").parse().unwrap_or(0);
			let tail = caps.get(3).map(|m| m.as_str()).unwrap_or("");
			if index > image_count {
				return whole.to_string();
			}
			let chip_kind = if kind == "Video" { ChipKind::Video } else { ChipKind::Image };
			let label = chip_label(chip_kind, index, None);
			register(&label, &format!("[{kind} #{index}{tail}]"));
			label
		})
		.into_owned()
}

/// Drops unreferenced vision attachments from a submission and densely remaps
/// retained image/video markers. `None` when no compaction is needed. With
/// `by_appearance`, retained markers are also renumbered in the order they first
/// appear.
pub fn compact_image_markers(text: &str, image_count: i64, by_appearance: bool) -> Option<(String, Vec<i64>)> {
	if image_count == 0 {
		return None;
	}
	let mut referenced: Vec<i64> = Vec::new();
	let mut seen: HashSet<i64> = HashSet::new();
	for caps in VISION_MARKER_REGEX.captures_iter(text) {
		let n: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or("0").parse().unwrap_or(0);
		if n <= image_count && seen.insert(n) {
			referenced.push(n);
		}
	}
	let mut keep = referenced;
	if !by_appearance {
		keep.sort_unstable();
	}
	if keep.len() as i64 == image_count && keep.iter().enumerate().all(|(i, n)| *n == i as i64 + 1) {
		return None;
	}
	let remap: HashMap<i64, i64> = keep
		.iter()
		.enumerate()
		.map(|(i, n)| (*n, i as i64 + 1))
		.collect();
	let rewritten = VISION_MARKER_REGEX
		.replace_all(text, |caps: &regex::Captures<'_>| {
			let whole = caps.get(0).map(|m| m.as_str()).unwrap_or("");
			let kind = caps.get(1).map(|m| m.as_str()).unwrap_or("Image");
			let index: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or("0").parse().unwrap_or(0);
			let tail = caps.get(3).map(|m| m.as_str()).unwrap_or("");
			let Some(mapped) = remap.get(&index) else {
				return whole.to_string();
			};
			let marker = format!("[{kind} #{mapped}{tail}]");
			match caps.get(4).map(|m| m.as_str()).and_then(|n| n.parse::<i64>().ok()) {
				Some(attachment) if attachment == index => format!("{marker} attachment://{mapped}"),
				_ => marker,
			}
		})
		.into_owned();
	let keep: Vec<i64> = keep.iter().map(|n| n - 1).collect();
	Some((rewritten, keep))
}

/// Attachment kinds understood by placeholder renderers.
pub type PlaceholderKind = ChipKind;

/// Which form a rendered reference took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceholderForm {
	/// Expanded `[Image #N, WxH]` marker.
	Marker,
	/// Compact `<icon> #N` chip.
	Chip,
}

/// Rendering callbacks for plain text and parsed attachment references.
pub trait PlaceholderRenderers {
	/// Renders text outside attachment references.
	fn render_text(&self, text: &str) -> String;
	/// Renders one parsed marker or compact chip token.
	fn render_reference(&self, label: &str, kind: PlaceholderKind, index: i64, form: PlaceholderForm) -> String;
	/// Renders one skill chip (`<icon> <name>`).
	fn render_skill(&self, label: &str, name: &str) -> String;
	/// Renders one registered model mention chip.
	fn render_mention(&self, label: &str) -> String;
}

/// Renders text while treating expanded markers, attachment chips, skill chips,
/// and model mentions as distinct references.
pub fn render_placeholders(
	text: &str,
	renderers: &dyn PlaceholderRenderers,
	token_regex: Option<&Regex>,
) -> String {
	let regex = token_regex.unwrap_or(&COMPOSER_TOKEN_REGEX);
	let mut result = String::new();
	let mut last = 0usize;
	let mut matched = false;

	for caps in regex.captures_iter(text) {
		let Some(whole) = caps.get(0) else { continue };
		let label = whole.as_str();
		if caps.name("mention").is_some() {
			if whole.start() > last {
				result.push_str(&renderers.render_text(&text[last..whole.start()]));
			}
			result.push_str(&renderers.render_mention(label));
			matched = true;
			last = whole.end();
			continue;
		}
		if label.starts_with('[') {
			if whole.start() > last {
				result.push_str(&renderers.render_text(&text[last..whole.start()]));
			}
			let kind = match caps.get(1).map(|m| m.as_str()) {
				Some("Paste") => PlaceholderKind::Paste,
				Some("Video") => PlaceholderKind::Video,
				_ => PlaceholderKind::Image,
			};
			let index: i64 = caps.get(2).map(|m| m.as_str()).unwrap_or("0").parse().unwrap_or(0);
			result.push_str(&renderers.render_reference(label, kind, index, PlaceholderForm::Marker));
			matched = true;
			last = whole.end();
			continue;
		}
		if let Some(name) = caps.get(3).map(|m| m.as_str()) {
			if !valid_end(text, whole.end()) {
				continue;
			}
			if whole.start() > last {
				result.push_str(&renderers.render_text(&text[last..whole.start()]));
			}
			result.push_str(&renderers.render_skill(label, name));
			matched = true;
			last = whole.end();
			continue;
		}
		let index: i64 = label
			.rsplit('#')
			.next()
			.and_then(|n| n.parse().ok())
			.unwrap_or(0);
		let kind = chip_label_kind(label);
		let icon = CHIP_ICONS
			.get(&kind)
			.and_then(|icons| icons.iter().find(|icon| label.starts_with(icon.as_str())));
		if let Some(icon) = icon {
			if !valid_start(text, whole.start(), icon) || !valid_end(text, whole.end()) {
				continue;
			}
		}
		if whole.start() > last {
			result.push_str(&renderers.render_text(&text[last..whole.start()]));
		}
		result.push_str(&renderers.render_reference(label, kind, index, PlaceholderForm::Chip));
		matched = true;
		last = whole.end();
	}

	if !matched {
		return renderers.render_text(text);
	}
	if last < text.len() {
		result.push_str(&renderers.render_text(&text[last..]));
	}
	result
}

#[cfg(test)]
mod tests {
	use super::*;

	fn kind_map(pairs: &[(&str, ChipKind)]) -> HashMap<String, ChipKind> {
		pairs
			.iter()
			.map(|(label, kind)| (label.to_string(), *kind))
			.collect()
	}

	#[test]
	fn chip_labels_use_unicode_glyphs_without_a_theme() {
		assert_eq!(chip_label(ChipKind::Image, 2, None), "📷 #2");
		assert_eq!(skill_chip_label("search", None), "✦ search");
		assert_eq!(model_mention_chip_label("gpt", None), "⬢ gpt");
		assert_eq!(skill_token("search"), "/skill:search");
	}

	#[test]
	fn styles_are_plain_without_a_theme() {
		assert_eq!(skill_chip_style("✦ s", CHIP_STYLE_RESTORE, None), "✦ s");
		assert_eq!(model_chip_style("⬢ g", CHIP_STYLE_RESTORE, None), "⬢ g");
	}

	#[test]
	fn chip_kind_inference_covers_every_preset() {
		assert_eq!(chip_label_kind("📷 #1"), ChipKind::Image);
		assert_eq!(chip_label_kind("img #3"), ChipKind::Image);
		assert_eq!(chip_label_kind("vid #3"), ChipKind::Video);
		assert_eq!(chip_label_kind("txt #3"), ChipKind::Paste);
	}

	#[test]
	fn palette_offsets_per_kind() {
		assert_eq!(attachment_rgb(ChipKind::Image, 1), (255, 179, 102));
		assert_eq!(attachment_rgb(ChipKind::Video, 1), (125, 207, 255));
		assert_eq!(attachment_rgb(ChipKind::Paste, 1), (189, 147, 249));
		assert_eq!(attachment_rgb(ChipKind::Image, 7), (255, 179, 102));
		assert_eq!(attachment_sgr(ChipKind::Image, 2), "\x1b[38;2;125;207;255m");
	}

	#[test]
	fn vision_markers_and_shifts() {
		assert_eq!(format_vision_marker(ChipKind::Image, 1, None), "[Image #1]");
		assert_eq!(
			format_vision_marker(ChipKind::Video, 2, Some((10, 20))),
			"[Video #2, 10x20]"
		);
		assert_eq!(shift_image_markers("[Image #1]", 2, None), "[Image #3]");
		assert_eq!(
			shift_image_markers("[Image #1 attachment://1]", 1, None),
			"[Image #2 attachment://2]"
		);
		assert_eq!(shift_image_markers("[Image #3]", 1, Some(2)), "[Image #3]");
		assert_eq!(shift_image_markers("[Image #1]", 0, None), "[Image #1]");
	}

	#[test]
	fn collapses_and_compacts_markers() {
		let mut registered: Vec<(String, String)> = Vec::new();
		let text = collapse_image_markers("[Image #1, 4x4] and [Image #9]", 2, &mut |label, expansion| {
			registered.push((label.to_string(), expansion.to_string()));
		});
		assert_eq!(text, "📷 #1 and [Image #9]");
		assert_eq!(registered, vec![("📷 #1".to_string(), "[Image #1, 4x4]".to_string())]);

		let (rewritten, keep) = compact_image_markers("[Image #2] and [Image #5]", 5, false).expect("compaction");
		assert_eq!(rewritten, "[Image #1] and [Image #2]");
		assert_eq!(keep, vec![1, 4]);

		assert_eq!(compact_image_markers("[Image #1]", 1, false), None);
		assert_eq!(compact_image_markers("nothing", 2, false), Some(("nothing".to_string(), vec![])));
	}

	#[test]
	fn referenced_attachments_counts_markers_and_chips() {
		let recorded = kind_map(&[("📷 #4", ChipKind::Image)]);
		let refs = referenced_attachments("[Video #1] 📷 #4 📷 #2 [Paste #3] img #5", &recorded);
		assert_eq!(
			refs.get(&ChipKind::Image).unwrap().iter().copied().collect::<Vec<i64>>(),
			vec![2, 4, 5]
		);
		assert_eq!(
			refs.get(&ChipKind::Video).unwrap().iter().copied().collect::<Vec<i64>>(),
			vec![1]
		);
		assert!(refs.get(&ChipKind::Paste).unwrap().is_empty());
	}

	#[test]
	fn records_registered_labels_without_prefix_matches() {
		let recorded = kind_map(&[("📷 #1", ChipKind::Image)]);
		let refs = referenced_attachments("📷 #10", &recorded);
		assert!(refs.get(&ChipKind::Image).unwrap().is_empty());
	}

	#[test]
	fn collapses_skill_tokens() {
		let mut registrations: Vec<(String, String)> = Vec::new();
		let text = collapse_skill_tokens("/skill:search now", &|name| name == "search", &mut |label, expansion| {
			registrations.push((label.to_string(), expansion.to_string()));
		});
		assert_eq!(text, "✦ search now");
		assert_eq!(
			registrations,
			vec![("✦ search".to_string(), "/skill:search".to_string())]
		);
	}

	#[test]
	fn collapses_model_mentions() {
		let mut registrations: Vec<(String, String)> = Vec::new();
		let text = collapse_model_mentions(
			"ask ^openai/gpt-5 please",
			&|selector| {
				(selector == "openai/gpt-5").then(|| ("⬢ gpt-5".to_string(), selector.to_string()))
			},
			&mut |label, expansion| registrations.push((label.to_string(), expansion.to_string())),
		);
		assert_eq!(text, "ask ⬢ gpt-5 please");
		assert_eq!(
			registrations,
			vec![("⬢ gpt-5".to_string(), "^openai/gpt-5".to_string())]
		);
	}

	struct Upper;

	impl PlaceholderRenderers for Upper {
		fn render_text(&self, text: &str) -> String {
			text.to_uppercase()
		}

		fn render_reference(&self, _label: &str, kind: PlaceholderKind, index: i64, form: PlaceholderForm) -> String {
			format!("<{}:{index}:{}>", kind.as_str(), matches!(form, PlaceholderForm::Marker))
		}

		fn render_skill(&self, _label: &str, name: &str) -> String {
			format!("<skill:{name}>")
		}

		fn render_mention(&self, label: &str) -> String {
			format!("<mention:{label}>")
		}
	}

	#[test]
	fn renders_markers_chips_skills_and_mentions() {
		let labels = vec!["⬢ gpt-5".to_string()];
		let regex = composer_token_regex(&labels);
		let text = "hi [Image #2] 📷 #1 ✦ search ⬢ gpt-5 end";
		assert_eq!(
			render_placeholders(text, &Upper, Some(&regex)),
			"HI <image:2:true> <image:1:false> <skill:search> <mention:⬢ gpt-5> END"
		);
		// Plain text goes through unchanged when no token matches.
		assert_eq!(render_placeholders("plain", &Upper, None), "PLAIN");
	}

	#[test]
	fn placeholder_marker_kinds() {
		let text = "[Paste #1] [Video #2]";
		assert_eq!(
			render_placeholders(text, &Upper, None),
			"<paste:1:true> <video:2:true>"
		);
	}
}
