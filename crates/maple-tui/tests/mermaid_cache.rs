//! Port of `l/test/mermaid-cache.test.ts`.
//!
//! The reference mocks `renderMermaidAsciiSafe`; the crate injects the render
//! function, so the same mock table drives `resolve_mermaid_ascii`.

use std::cell::RefCell;

use maple_tui::theme::mermaid_cache::{
	MermaidCache, MermaidDirection, MermaidResolveOptions, resolve_mermaid_ascii,
};

struct RenderLog {
	entries: RefCell<Vec<String>>,
}

impl RenderLog {
	fn new() -> Self {
		Self {
			entries: RefCell::new(Vec::new()),
		}
	}

	fn log(&self, source: &str, direction: Option<MermaidDirection>) -> Option<String> {
		let label = match direction {
			None => "authored",
			Some(MermaidDirection::TD) => "TD",
			Some(MermaidDirection::LR) => "LR",
		};
		self.entries.borrow_mut().push(format!("{source}:{label}"));
		match source {
			"bad" => None,
			"wide-authored" => match direction {
				None => Some("AUTHORED-ALSO-WIDE\nsecond\nthird".to_string()),
				Some(MermaidDirection::TD) => Some("td\ntd\ntd".to_string()),
				Some(MermaidDirection::LR) => Some("left-to-right".to_string()),
			},
			"colored" => match direction {
				None => Some("\x1b[31mabcd\x1b[0m".to_string()),
				Some(MermaidDirection::TD) => Some("too-wide-for-four".to_string()),
				Some(MermaidDirection::LR) => Some("z".to_string()),
			},
			_ => Some("direction-ignored-and-wide".to_string()),
		}
	}

	fn renders(&self) -> Vec<String> {
		self.entries.borrow().clone()
	}
}

fn options(max_width: f64) -> MermaidResolveOptions {
	MermaidResolveOptions {
		render_options: serde_json::json!({}),
		max_width: Some(max_width),
	}
}
fn resolve(cache: &mut MermaidCache, log: &RenderLog, source: &str, opts: Option<&MermaidResolveOptions>) -> Option<String> {
	resolve_mermaid_ascii(cache, &|text: &str, direction: Option<MermaidDirection>| log.log(text, direction), source, opts)
}

#[test]
fn keeps_the_authored_layout_when_no_width_is_given() {
	let mut cache = MermaidCache::new();
	let log = RenderLog::new();
	assert_eq!(
		resolve(&mut cache, &log, "wide-authored", None),
		Some("AUTHORED-ALSO-WIDE\nsecond\nthird".to_string())
	);
	assert_eq!(log.renders(), vec!["wide-authored:authored"]);
}

#[test]
fn picks_the_shortest_fitting_orientation_then_switches_on_resize() {
	let mut cache = MermaidCache::new();
	let log = RenderLog::new();
	assert_eq!(
		resolve(&mut cache, &log, "wide-authored", Some(&options(30.0))),
		Some("left-to-right".to_string())
	);
	assert_eq!(
		log.renders(),
		vec!["wide-authored:authored", "wide-authored:TD", "wide-authored:LR"]
	);

	assert_eq!(
		resolve(&mut cache, &log, "wide-authored", Some(&options(10.0))),
		Some("td\ntd\ntd".to_string())
	);
	assert_eq!(log.renders().len(), 3);
}

#[test]
fn uses_the_narrowest_layout_only_when_nothing_fits() {
	let mut cache = MermaidCache::new();
	let log = RenderLog::new();
	assert_eq!(
		resolve(&mut cache, &log, "wide-authored", Some(&options(1.0))),
		Some("td\ntd\ntd".to_string())
	);
}

#[test]
fn measures_themed_ascii_without_counting_ansi() {
	let mut cache = MermaidCache::new();
	let log = RenderLog::new();
	assert_eq!(
		resolve(&mut cache, &log, "colored", Some(&options(4.0))),
		Some("\x1b[31mabcd\x1b[0m".to_string())
	);
}

#[test]
fn returns_null_without_trying_orientations_when_the_source_fails() {
	let mut cache = MermaidCache::new();
	let log = RenderLog::new();
	assert_eq!(resolve(&mut cache, &log, "bad", Some(&options(80.0))), None);
	assert_eq!(log.renders(), vec!["bad:authored"]);
}

#[test]
fn renders_direction_ignoring_diagrams_only_as_authored() {
	let sources = [
		"sequenceDiagram\nAlice->>Bob: hi",
		"classDiagram\nclass A",
		"erDiagram\nA ||--o{ B : rel",
		"xychart-beta\nbar [1, 2]",
		"xychart\nbar [1, 2]",
	];
	for source in sources {
		let mut cache = MermaidCache::new();
		let log = RenderLog::new();
		assert_eq!(
			resolve(&mut cache, &log, source, Some(&options(4.0))),
			Some("direction-ignored-and-wide".to_string())
		);
		assert_eq!(log.renders(), vec![format!("{source}:authored")]);
	}
}

#[test]
fn skips_the_forced_render_that_repeats_the_authored_direction() {
	let cases: [(&str, &[&str]); 6] = [
		("flowchart TD\nA --> B", &["LR"]),
		("graph tb\nA --> B", &["LR"]),
		("flowchart LR\nA --> B", &["TD"]),
		("graph RL\nA --> B", &["TD"]),
		("flowchart BT\nA --> B", &["TD", "LR"]),
		("stateDiagram-v2\n[*] --> A", &["TD", "LR"]),
	];
	for (source, forced) in cases {
		let mut cache = MermaidCache::new();
		let log = RenderLog::new();
		resolve(&mut cache, &log, source, Some(&options(4.0)));
		let mut expected = vec![format!("{source}:authored")];
		expected.extend(forced.iter().map(|direction| format!("{source}:{direction}")));
		assert_eq!(log.renders(), expected);
	}
}
