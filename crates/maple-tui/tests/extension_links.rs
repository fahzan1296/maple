//! Port of `l/test/extension-links.test.ts`.
//!
//! The crate ports the scheme registry, the marker format/strip helpers and the
//! label scanner. Markdown rendering, composer link hints and transcript
//! settling are host components that are not ported, so those stay ignored.

use maple_tui::render::extension_links::{
	extension_link_clicks_available, extension_link_labels, extension_link_scheme,
	extension_links_routed, extension_links_version, format_extension_link,
	set_extension_link_picking, set_extension_links, strip_extension_link_markers,
};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// The scheme registry is process-wide; serialize the tests that mutate it.
fn lock() -> MutexGuard<'static, ()> {
	static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
	LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn plain(text: &str) -> String {
	maple_tui::strip_ansi(text)
}

fn reset() {
	set_extension_links(Vec::<String>::new());
	set_extension_link_picking(None);
}

#[test]
fn labels_require_maples_own_marker_and_a_registered_scheme() {
	let _guard = lock();
	reset();
	let unclaimed = format_extension_link("the details", "demo:open/7", "k");
	assert!(extension_link_labels(&unclaimed).is_empty());
	assert!(extension_link_labels("See \x1b]8;;demo:open/7\x07the details\x1b]8;;\x07.").is_empty());
	assert!(extension_link_labels("See \x1b]8;id=other;demo:open/7\x07the details\x1b]8;;\x07.").is_empty());

	set_extension_links(["demo"]);
	let marked = format_extension_link("the details", "demo:open/7", "k");
	let labels = extension_link_labels(&marked);
	assert_eq!(labels.len(), 1);
	assert_eq!(labels[0].url, "demo:open/7");
	assert_eq!((labels[0].start, labels[0].end), (0, 12));
	assert_eq!(plain(&marked), "the details");

	set_extension_links(["other"]);
	assert!(extension_link_labels(&marked).is_empty());
	reset();
}

#[test]
fn format_keeps_the_underline_across_resets_inside_the_label() {
	let _guard = lock();
	reset();
	let drawn = format_extension_link("\x1b[38;2;0;136;250mb\x1b[0mc\x1b[39m", "demo:x", "k");
	let plain = "\x1b[4m\x1b[38;2;0;136;250mb\x1b[0m\x1b[4mc\x1b[39m\x1b[24m";
	let styled = "\x1b[4:4m\x1b[38;2;0;136;250mb\x1b[0m\x1b[4:4mc\x1b[39m\x1b[4:0m";
	assert!(strip_extension_link_markers(&drawn) == plain || strip_extension_link_markers(&drawn) == styled);
	assert!(!drawn.contains("\x1b[44m") && !drawn.contains("\x1b[40m"));
}

#[test]
fn identical_links_get_distinct_markers_by_key() {
	let _guard = lock();
	reset();
	set_extension_links(["demo"]);
	let first = format_extension_link("x", "demo:open", "k1");
	let second = format_extension_link("x", "demo:open", "k2");
	let ids = |line: &str| {
		extension_link_labels(line)
			.into_iter()
			.map(|label| label.id)
			.collect::<Vec<_>>()
	};
	assert_eq!(ids(&first).len(), 1);
	assert_eq!(ids(&second).len(), 1);
	assert_ne!(ids(&first), ids(&second));
	reset();
}

#[test]
fn strip_removes_only_maples_markers() {
	let _guard = lock();
	reset();
	let foreign = "See \x1b]8;;demo:open/7\x07the details\x1b]8;;\x07.";
	assert_eq!(strip_extension_link_markers(foreign), foreign);

	for params in ["id=omp-extension-link", "id=omp-extension-link-12"] {
		for end in ["\x07", "\x1b\\"] {
			let line = format!("a\x1b]8;{params};demo:x{end}b\x1b]8;;{end}c");
			let stripped = strip_extension_link_markers(&line);
			assert_eq!(plain(&stripped), "abc");
			assert!(!stripped.contains("\x1b]8"));
		}
	}
}

#[test]
fn registry_routes_links_and_reports_the_version() {
	let _guard = lock();
	reset();
	let before = extension_links_version();
	assert!(!extension_links_routed());
	assert!(extension_link_scheme("demo:open/7").is_none());
	assert!(!extension_link_clicks_available());

	set_extension_links(["demo"]);
	assert!(extension_links_version() > before);
	assert!(extension_links_routed());
	assert_eq!(extension_link_scheme("demo:open/7").as_deref(), Some("demo"));
	assert_eq!(extension_link_scheme("DEMO:open/7").as_deref(), Some("demo"));
	assert_eq!(extension_link_scheme("https://example.com").as_deref(), None);
	assert!(!extension_link_clicks_available());

	set_extension_link_picking(Some(Arc::new(|| true)));
	assert!(extension_link_clicks_available());
	set_extension_link_picking(Some(Arc::new(|| false)));
	assert!(!extension_link_clicks_available());
	reset();
}

#[test]
#[ignore = "missing API: no Markdown component; link cells are produced by the chat renderer"]
fn draws_a_claimed_link_without_the_url_suffix() {}

#[test]
#[ignore = "missing API: no Markdown component or wrapped-row renderer"]
fn maps_a_wrapped_labels_cells_to_its_url() {}

#[test]
#[ignore = "missing API: no Composer link-hint picker"]
fn tags_labels_with_hint_letters() {}

#[test]
#[ignore = "missing API: no TranscriptContainer or AssistantMessageComponent"]
fn retires_settled_rows_to_scrollback_with_markers() {}
