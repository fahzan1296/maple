//! Mermaid ASCII render cache: memoize per direction variant, then pick the
//! layout that fits a viewport width.
//!
//! Reference: `l/src/theme/mermaid-cache.ts`.
//!
//! # Divergences
//!
//! - `@oh-my-pi/pi-utils/mermaid-ascii::renderMermaidAsciiSafe` and
//!   `@oh-my-pi/pi-utils/lru` are not vendored. Rendering is injected as
//!   `&dyn Fn(&str, Option<MermaidDirection>) -> Option<String>` and the bounded
//!   LRU lives in [`MermaidCache`].
//! - The reference's module-level `cache` becomes a value the caller owns and
//!   passes to [`resolve_mermaid_ascii`].
//! - `MermaidRenderOptions` comes from `@oh-my-pi/pi-natives`, which this crate
//!   does not have. Render options travel as a [`serde_json::Value`] because the
//!   cache's only use of them is the serialized cache key.
//! - Size accounting uses UTF-16 code units (`encode_utf16().count()`), matching
//!   JavaScript `String.length`.

use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::text_shape::visible_width;

/// Entry cap for the mermaid render cache.
pub const MERMAID_CACHE_MAX: usize = 256;
/// Total UTF-16 length cap (key plus rendered ASCII) for the cache.
pub const MERMAID_CACHE_MAX_SIZE: usize = 4 * 1024 * 1024;

static AUTHORED_DIRECTION_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^(?:graph|flowchart)\s+(td|tb|lr|rl)$").expect("valid regex"));

static FORCED_DIRECTIONS: [MermaidDirection; 2] = [MermaidDirection::TD, MermaidDirection::LR];
static TD_ONLY: [MermaidDirection; 1] = [MermaidDirection::TD];
static LR_ONLY: [MermaidDirection; 1] = [MermaidDirection::LR];
static NO_DIRECTIONS: [MermaidDirection; 0] = [];

/// A forced layout direction for flowchart variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MermaidDirection {
	/// Top-down.
	TD,
	/// Left-to-right.
	LR,
}

impl MermaidDirection {
	fn as_str(self) -> &'static str {
		match self {
			MermaidDirection::TD => "TD",
			MermaidDirection::LR => "LR",
		}
	}
}

/// Options for [`resolve_mermaid_ascii`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MermaidResolveOptions {
	/// Raw native render options; the cache serializes them into its key.
	pub render_options: Value,
	/// Maximum display width in terminal columns. Flowcharts are also rendered
	/// top-down and left-to-right; a resize re-decides among cached renders
	/// without drawing again.
	pub max_width: Option<f64>,
}

/// Bounded LRU of rendered ASCII, keyed by render options + direction + source.
///
/// A cached [`None`] is a memoized failure, distinct from a miss.
pub struct MermaidCache {
	order: VecDeque<String>,
	entries: HashMap<String, Option<String>>,
	size: usize,
}

impl MermaidCache {
	/// An empty cache with the shared bounds.
	pub fn new() -> Self {
		Self {
			order: VecDeque::new(),
			entries: HashMap::new(),
			size: 0,
		}
	}

	/// Cached render for `key`; [`None`] means the key is not cached.
	pub fn get(&mut self, key: &str) -> Option<Option<String>> {
		if !self.entries.contains_key(key) {
			return None;
		}
		self.touch(key);
		self.entries.get(key).cloned()
	}

	/// Store a render (or a memoized failure) for `key`, evicting the least
	/// recently used entries once the entry or size bound is exceeded.
	pub fn set(&mut self, key: &str, value: Option<String>) {
		let entry_size = cache_entry_size(key, value.as_deref());
		if let Some(previous) = self.entries.insert(key.to_string(), value) {
			self.size -= cache_entry_size(key, previous.as_deref());
			self.touch(key);
		} else {
			self.order.push_back(key.to_string());
		}
		self.size += entry_size;
		while self.order.len() > MERMAID_CACHE_MAX || self.size > MERMAID_CACHE_MAX_SIZE {
			match self.order.pop_front() {
				Some(evicted) => {
					if let Some(previous) = self.entries.remove(&evicted) {
						self.size -= cache_entry_size(&evicted, previous.as_deref());
					}
				}
				None => break,
			}
		}
	}

	/// Drop every entry.
	pub fn clear(&mut self) {
		self.order.clear();
		self.entries.clear();
		self.size = 0;
	}

	/// Number of cached entries.
	pub fn len(&self) -> usize {
		self.entries.len()
	}

	/// True when nothing is cached.
	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	fn touch(&mut self, key: &str) {
		if let Some(index) = self.order.iter().position(|entry| entry == key) {
			self.order.remove(index);
		}
		self.order.push_back(key.to_string());
	}
}

impl Default for MermaidCache {
	fn default() -> Self {
		Self::new()
	}
}

/// UTF-16 length of the key plus the rendered ASCII, the reference's size metric.
fn cache_entry_size(key: &str, ascii: Option<&str>) -> usize {
	key.encode_utf16().count() + ascii.map_or(0, |ascii| ascii.encode_utf16().count())
}

/// The reference's `{ colorMode: "none", ...rest }` default.
fn base_options(render_options: &Value) -> Value {
	let mut object = match render_options {
		Value::Object(object) => object.clone(),
		_ => Map::new(),
	};
	object
		.entry("colorMode".to_string())
		.or_insert_with(|| Value::String("none".to_string()));
	Value::Object(object)
}

/// Normalize fenced block source: CRLF/CR to LF, then trim.
fn normalize_source(source: &str) -> String {
	source.replace("\r\n", "\n").replace('\r', "\n").trim().to_string()
}

/// Cache key for one direction variant.
fn variant_key(base_key: &str, direction: Option<MermaidDirection>, source: &str) -> String {
	format!(
		"{base_key}\u{0}{}{source}",
		direction.map_or("", MermaidDirection::as_str)
	)
}

/// Display columns of rendered ASCII, ignoring ANSI, matching
/// `Bun.stringWidth(line, { countAnsiEscapeCodes: false })`.
fn ascii_display_width(ascii: &str) -> usize {
	ascii
		.split('\n')
		.map(visible_width)
		.max()
		.unwrap_or(0)
}

struct LayoutCandidate {
	ascii: String,
	width: usize,
	height: usize,
	authored: bool,
}

fn measure_layout(ascii: String, authored: bool) -> LayoutCandidate {
	let width = ascii_display_width(&ascii);
	let height = ascii.split('\n').count();
	LayoutCandidate {
		ascii,
		width,
		height,
		authored,
	}
}

/// Render one direction variant, memoizing the result (including failures).
fn render_variant(
	cache: &mut MermaidCache,
	render: &dyn Fn(&str, Option<MermaidDirection>) -> Option<String>,
	source: &str,
	base_key: &str,
	direction: Option<MermaidDirection>,
) -> Option<String> {
	let key = variant_key(base_key, direction, source);
	if let Some(cached) = cache.get(&key) {
		return cached;
	}
	let ascii = render(source, direction);
	cache.set(&key, ascii.clone());
	ascii
}

/// Forced TD/LR renders that can differ from the authored one.
///
/// Sequence, class, ER, and xychart ignore direction, so they get none. A
/// `graph`/`flowchart` header already authored in one of the forced layouts
/// (`TD`/`TB` lay out as TD, `LR`/`RL` as LR) skips that variant: the native
/// override replaces the header direction, so the render would be identical to
/// the authored one.
fn forced_directions(source: &str) -> &'static [MermaidDirection] {
	let header = source
		.split('\n')
		.next()
		.unwrap_or("")
		.trim()
		.to_ascii_lowercase();
	for prefix in ["xychart-beta", "xychart"] {
		if let Some(rest) = header.strip_prefix(prefix) {
			let word_char = rest
				.chars()
				.next()
				.map(|next| next.is_ascii_alphanumeric() || next == '_')
				.unwrap_or(false);
			if !word_char {
				return &NO_DIRECTIONS;
			}
		}
	}
	if header == "sequencediagram" || header == "classdiagram" || header == "erdiagram" {
		return &NO_DIRECTIONS;
	}
	let authored = AUTHORED_DIRECTION_RE
		.captures(&header)
		.and_then(|captures| captures.get(1))
		.map(|matched| matched.as_str());
	match authored {
		Some("td" | "tb") => &LR_ONLY,
		Some("lr" | "rl") => &TD_ONLY,
		_ => &FORCED_DIRECTIONS,
	}
}

/// Resolve mermaid ASCII from fenced block source text.
///
/// Returns [`None`] when rendering fails, memoizing the failure so a repeated
/// render is not attempted. Without `max_width` the authored layout is returned
/// as-is; otherwise the shortest fitting variant wins, falling back to the
/// narrowest when nothing fits.
pub fn resolve_mermaid_ascii(
	cache: &mut MermaidCache,
	render: &dyn Fn(&str, Option<MermaidDirection>) -> Option<String>,
	source: &str,
	options: Option<&MermaidResolveOptions>,
) -> Option<String> {
	let normalized_source = normalize_source(source);
	if normalized_source.is_empty() {
		return None;
	}
	let options = options.cloned().unwrap_or_default();
	let base_key = serde_json::to_string(&base_options(&options.render_options))
		.unwrap_or_else(|_| "{}".to_string());

	let base = render_variant(cache, render, &normalized_source, &base_key, None)?;
	let max_width = match options.max_width {
		Some(max_width) => max_width,
		None => return Some(base.clone()),
	};

	// Width selection is against cached renders, so a later resize re-decides
	// without drawing again. Only forced layouts that can differ from the
	// authored one are rendered.
	let mut candidates = vec![measure_layout(base, true)];
	for direction in forced_directions(&normalized_source) {
		let variant = render_variant(cache, render, &normalized_source, &base_key, Some(*direction));
		if let Some(variant) = variant {
			candidates.push(measure_layout(variant, false));
		}
	}

	let fitting: Vec<&LayoutCandidate> = candidates
		.iter()
		.filter(|candidate| (candidate.width as f64) <= max_width)
		.collect();
	let all: Vec<&LayoutCandidate> = candidates.iter().collect();
	let pool: &[&LayoutCandidate] = if fitting.is_empty() { &all } else { &fitting };
	let mut best = pool[0];
	for next in &pool[1..] {
		if !fitting.is_empty() {
			if next.height != best.height {
				if next.height < best.height {
					best = next;
				}
				continue;
			}
			if next.authored != best.authored {
				if next.authored {
					best = next;
				}
				continue;
			}
			if next.width < best.width {
				best = next;
			}
			continue;
		}
		if next.width != best.width {
			if next.width < best.width {
				best = next;
			}
			continue;
		}
		if next.height != best.height {
			if next.height < best.height {
				best = next;
			}
			continue;
		}
		if next.authored {
			best = next;
		}
	}
	Some(best.ascii.clone())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;

	struct Recorded {
		directions: Rc<RefCell<Vec<Option<MermaidDirection>>>>,
		outputs: Vec<(&'static str, Option<MermaidDirection>, &'static str)>,
		calls: Rc<RefCell<usize>>,
		failing: bool,
	}

	impl Recorded {
		fn new(outputs: Vec<(&'static str, Option<MermaidDirection>, &'static str)>) -> Self {
			Self {
				directions: Rc::new(RefCell::new(Vec::new())),
				outputs,
				calls: Rc::new(RefCell::new(0)),
				failing: false,
			}
		}

		fn render(&self) -> impl Fn(&str, Option<MermaidDirection>) -> Option<String> {
			let directions = self.directions.clone();
			let outputs = self.outputs.clone();
			let calls = self.calls.clone();
			let failing = self.failing;
			move |source, direction| {
				directions.borrow_mut().push(direction);
				*calls.borrow_mut() += 1;
				if failing || source.trim().is_empty() {
					return None;
				}
				outputs
					.iter()
					.find(|(prefix, variant, _)| source.starts_with(*prefix) && *variant == direction)
					.map(|(_, _, ascii)| ascii.to_string())
			}
		}
	}

	fn options(max_width: Option<f64>) -> MermaidResolveOptions {
		MermaidResolveOptions {
			render_options: Value::Null,
			max_width,
		}
	}

	#[test]
	fn resolves_the_authored_layout_and_normalizes_source() {
		let recorded = Recorded::new(vec![("graph TD", None, "AAAA\nBB")]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let resolved = resolve_mermaid_ascii(
			&mut cache,
			&render,
			"  graph TD\r\nA-->B  \r",
			Some(&options(None)),
		);
		assert_eq!(resolved, Some("AAAA\nBB".to_string()));
		let requested: Vec<_> = recorded.directions.borrow().clone();
		assert_eq!(requested, vec![None]);
		assert_eq!(*recorded.calls.borrow(), 1);
	}

	#[test]
	fn memoized_failures_are_not_retried() {
		let recorded = Recorded::new(vec![]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let source = "graph TD\nA-->B";
		assert_eq!(
			resolve_mermaid_ascii(&mut cache, &render, source, Some(&options(None))),
			None
		);
		assert_eq!(
			resolve_mermaid_ascii(&mut cache, &render, source, Some(&options(None))),
			None
		);
		assert_eq!(*recorded.calls.borrow(), 1);
		assert_eq!(cache.len(), 1);
	}

	#[test]
	fn empty_source_resolves_to_none_without_rendering() {
		let recorded = Recorded::new(vec![]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		assert_eq!(
			resolve_mermaid_ascii(&mut cache, &render, " \r\n\t ", Some(&options(None))),
			None
		);
		assert_eq!(*recorded.calls.borrow(), 0);
		assert!(cache.is_empty());
	}

	#[test]
	fn picks_the_shortest_fitting_variant() {
		let recorded = Recorded::new(vec![
			("graph TD", None, "AAAAAAAA\nBB\nCC"),
			("graph TD", Some(MermaidDirection::LR), "FFFFF"),
		]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let resolved = resolve_mermaid_ascii(
			&mut cache,
			&render,
			"graph TD\nA-->B",
			Some(&options(Some(8.0))),
		);
		assert_eq!(resolved, Some("FFFFF".to_string()));
	}

	#[test]
	fn falls_back_to_the_narrowest_when_nothing_fits() {
		let recorded = Recorded::new(vec![
			("graph TD", None, "AAAAAAAA\nBB\nCC"),
			("graph TD", Some(MermaidDirection::LR), "FFFFF"),
		]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let resolved = resolve_mermaid_ascii(
			&mut cache,
			&render,
			"graph TD\nA-->B",
			Some(&options(Some(2.0))),
		);
		assert_eq!(resolved, Some("FFFFF".to_string()));
	}

	#[test]
	fn prefers_the_authored_render_on_equal_height() {
		let recorded = Recorded::new(vec![
			("graph TD", None, "AAAAAA\nBB"),
			("graph TD", Some(MermaidDirection::LR), "CCCC\nDD"),
		]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let resolved = resolve_mermaid_ascii(
			&mut cache,
			&render,
			"graph TD\nA-->B",
			Some(&options(Some(9.0))),
		);
		assert_eq!(resolved, Some("AAAAAA\nBB".to_string()));
	}

	#[test]
	fn skips_direction_variants_that_cannot_differ() {
		let recorded = Recorded::new(vec![
			("sequenceDiagram", None, "A->>B: hi"),
			("classDiagram", None, "class A"),
			("erDiagram", None, "A ||--o{ B"),
			("xychart-beta", None, "bar"),
			("xychart", None, "line"),
		]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		for source in [
			"sequenceDiagram\nA->>B: hi",
			"classDiagram\nclass A",
			"erDiagram\nA ||--o{ B",
			"xychart-beta\nbar",
			"xychart\nline",
		] {
			let resolved = resolve_mermaid_ascii(&mut cache, &render, source, Some(&options(Some(20.0))));
			assert!(resolved.is_some(), "missing render for {source}");
		}
		let requested: Vec<_> = recorded.directions.borrow().clone();
		assert!(requested.iter().all(|direction| *direction == None));
	}

	#[test]
	fn authored_lr_only_forces_td() {
		let recorded = Recorded::new(vec![
			("graph LR", None, "AAAA"),
			("graph LR", Some(MermaidDirection::TD), "B\nC"),
		]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let _ = resolve_mermaid_ascii(
			&mut cache,
			&render,
			"graph LR\nA-->B",
			Some(&options(Some(20.0))),
		);
		let requested: Vec<_> = recorded.directions.borrow().clone();
		assert_eq!(
			requested,
			vec![None, Some(MermaidDirection::TD)]
		);
	}

	#[test]
	fn authored_td_only_forces_lr() {
		let recorded = Recorded::new(vec![
			("graph TD", None, "AAAA"),
			("graph TD", Some(MermaidDirection::LR), "B\nC"),
		]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let _ = resolve_mermaid_ascii(
			&mut cache,
			&render,
			"graph TD\nA-->B",
			Some(&options(Some(20.0))),
		);
		let requested: Vec<_> = recorded.directions.borrow().clone();
		assert_eq!(
			requested,
			vec![None, Some(MermaidDirection::LR)]
		);
	}

	#[test]
	fn clear_drops_every_entry() {
		let recorded = Recorded::new(vec![("graph TD", None, "AAAA")]);
		let render = recorded.render();
		let mut cache = MermaidCache::new();
		let _ = resolve_mermaid_ascii(&mut cache, &render, "graph TD\nA-->B", Some(&options(None)));
		assert_eq!(cache.len(), 1);
		cache.clear();
		assert!(cache.is_empty());
		let _ = resolve_mermaid_ascii(&mut cache, &render, "graph TD\nA-->B", Some(&options(None)));
		assert_eq!(*recorded.calls.borrow(), 2);
	}

	#[test]
	fn cache_is_bounded_by_entry_count() {
		let mut cache = MermaidCache::new();
		for index in 0..(MERMAID_CACHE_MAX + 40) {
			cache.set(&format!("key-{index}"), Some("x".to_string()));
		}
		assert_eq!(cache.len(), MERMAID_CACHE_MAX);
	}

	#[test]
	fn cache_is_bounded_by_size() {
		let mut cache = MermaidCache::new();
		let chunk = "x".repeat(MERMAID_CACHE_MAX_SIZE + 1);
		cache.set("big", Some(chunk));
		assert!(cache.is_empty());
	}
}
