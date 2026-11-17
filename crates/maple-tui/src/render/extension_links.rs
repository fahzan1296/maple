//! Extension-registered link schemes and the OSC 8 markers that trace them.
//!
//! Reference: `l/src/render/extension-links.ts`.
//!
//! Links whose scheme an extension registered are drawn without the ` (url)`
//! suffix. On a plain terminal the rendered row carries an OSC 8 marker whose
//! params tag maple's own links, so a click's cell or a keyboard hint tag can
//! be traced back to the URL ([`extension_link_labels`]);
//! [`strip_extension_link_markers`] removes the marker from every row the TUI
//! writes so the terminal never receives the link as a hyperlink it could
//! open. In a native (TSP) view the label is drawn as given and the native
//! view routes its own clicks, so no marker is emitted.
//!
//! # Deviations
//!
//! The reference keeps `schemes`, `version` and `pickingProvider` as
//! module-level mutable bindings and registers a native-rendering listener at
//! module load. Rust has no module-level mutable state, so they are explicit
//! process-wide statics behind locks (`SCHEMES`, `PICKING`, `VERSION`);
//! `REGISTER` reproduces the load-time listener that bumps the version when
//! native rendering starts or stops. [`set_extension_link_picking`] takes an
//! `Arc` trait object because the provider is stored past the call. The
//! reference hashes each marker id with `Bun.hash.crc32`; the port implements
//! the same CRC-32 (IEEE 802.3) locally rather than adding a dependency.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

use regex::Regex;

use crate::ansi_scan::strip_ansi;
use crate::native::state::native_state;
use crate::terminal_capabilities::terminal;
use crate::text_shape::visible_width;

/// One OSC 8 sequence (BEL or ST terminated); group 1 is its params, group 2
/// its URI (empty for a close).
static OSC8_REGEX: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"\x1b\]8;([^;\x07\x1b]*);([^\x07\x1b]*)(?:\x07|\x1b\\)").expect("valid OSC 8 regex")
});
/// The OSC 8 params tagging maple's own markers.
static MARKER_PARAMS_REGEX: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^id=omp-extension-link(?:-\d+)?$").expect("valid marker regex"));
/// An SGR sequence; group 1 is its params.
static SGR_REGEX: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"\x1b\[([0-9:;]*)m").expect("valid SGR regex"));

/// The OSC 8 open sequence prefix every drawn marker starts with.
const MARKER_OPEN: &str = "\x1b]8;id=omp-extension-link";

/// Registered schemes, lowercase and without `:`.
static SCHEMES: LazyLock<RwLock<BTreeSet<String>>> = LazyLock::new(|| RwLock::new(BTreeSet::new()));
/// The interactive product's "can pick a link now" probe.
static PICKING: LazyLock<RwLock<Option<Arc<dyn Fn() -> bool + Send + Sync>>>> =
	LazyLock::new(|| RwLock::new(None));
/// Bumped whenever [`SCHEMES`] changes or native rendering starts or stops.
static VERSION: AtomicUsize = AtomicUsize::new(0);
/// Installs the process-lifetime native-rendering listener.
static REGISTER: LazyLock<()> = LazyLock::new(|| {
	native_state().on_native_rendering_change(|_| {
		VERSION.fetch_add(1, Ordering::Relaxed);
	});
});

/// Whether the terminal draws styled underlines. Fixed once capabilities are
/// resolved (nothing mutates it afterwards), so it is read once rather than
/// cloned per rendered link.
static STYLED_UNDERLINES: LazyLock<bool> = LazyLock::new(|| terminal().styled_underlines);

/// Force [`REGISTER`] to run, so the version tracks native rendering however
/// late the module is first used.
fn ensure_registered() {
	let _ = &*REGISTER;
}

/// Run `f` with the registered schemes read-locked, so callers never clone
/// the set.
fn with_schemes<R>(f: impl FnOnce(&BTreeSet<String>) -> R) -> R {
	let schemes = SCHEMES.read().unwrap_or_else(|poisoned| poisoned.into_inner());
	f(&schemes)
}

/// Set the registered schemes (lowercase, without `:`). Bumps
/// [`extension_links_version`] on any change so cached markdown renders are
/// redrawn.
pub fn set_extension_links<I, S>(next: I)
where
	I: IntoIterator<Item = S>,
	S: Into<String>,
{
	ensure_registered();
	let next_schemes: BTreeSet<String> = next.into_iter().map(Into::into).collect();
	let mut current = SCHEMES.write().unwrap_or_else(|poisoned| poisoned.into_inner());
	if next_schemes.len() == current.len() && next_schemes.iter().all(|scheme| current.contains(scheme)) {
		return;
	}
	*current = next_schemes;
	VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Changes whenever the scheme set changes; part of the markdown render cache
/// key.
pub fn extension_links_version() -> usize {
	ensure_registered();
	VERSION.load(Ordering::Relaxed)
}

/// Whether activating a registered-scheme link (a click or a hint tag) reaches
/// its extension: a scheme is registered and the terminal is plain.
pub fn extension_links_routed() -> bool {
	ensure_registered();
	!with_schemes(|schemes| schemes.is_empty()) && !native_state().is_native_rendering()
}

/// Set by the interactive product: whether the user has a way to pick a link
/// drawn now (a bound hint key, mouse clicks). `None` when nothing in this
/// process can pick one.
pub fn set_extension_link_picking(provider: Option<Arc<dyn Fn() -> bool + Send + Sync>>) {
	ensure_registered();
	*PICKING.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = provider;
}

/// Whether a registered-scheme link drawn now can be activated: links are
/// routed and the product offers a way to pick them. Process-wide, so a
/// UI-less extension runner answers like the interactive one.
pub fn extension_link_clicks_available() -> bool {
	ensure_registered();
	if !extension_links_routed() {
		return false;
	}
	PICKING
		.read()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.as_ref()
		.is_some_and(|provider| provider())
}

/// The registered scheme `href` uses, or `None` for every other link.
pub fn extension_link_scheme(href: &str) -> Option<String> {
	with_schemes(|schemes| extension_link_scheme_with(schemes, href))
}

fn extension_link_scheme_with(schemes: &BTreeSet<String>, href: &str) -> Option<String> {
	if schemes.is_empty() {
		return None;
	}
	let colon = href.find(':')?;
	if colon == 0 {
		return None;
	}
	let scheme = href[..colon].to_lowercase();
	if schemes.contains(&scheme) {
		Some(scheme)
	} else {
		None
	}
}

/// Dotted underline (`4:4`) where the terminal draws styled underlines, else
/// the plain one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UnderlineStyle {
	on: &'static str,
	off: &'static str,
}

const DOTTED_UNDERLINE: UnderlineStyle = UnderlineStyle { on: "\x1b[4:4m", off: "\x1b[4:0m" };
const PLAIN_UNDERLINE: UnderlineStyle = UnderlineStyle { on: "\x1b[4m", off: "\x1b[24m" };

fn underline_style(styled: bool) -> UnderlineStyle {
	if styled {
		DOTTED_UNDERLINE
	} else {
		PLAIN_UNDERLINE
	}
}

/// Whether an SGR with these params turns underlining off: a full reset, `24`
/// or `4:0`.
fn ends_underline(params: &str) -> bool {
	let codes: Vec<&str> = params.split(';').collect();
	let mut index = 0usize;
	while index < codes.len() {
		let code = codes[index];
		if code.is_empty() || code == "0" || code == "24" || code == "4:0" {
			return true;
		}
		// Skip a color's arguments: `38;5;n` / `38;2;r;g;b` (also 48 and 58),
		// whose `0`s are no reset.
		if code == "38" || code == "48" || code == "58" {
			let mode = codes.get(index + 1);
			index += match mode {
				Some(&"5") => 2,
				Some(&"2") => 4,
				_ => 0,
			};
		}
		index += 1;
	}
	false
}

/// CRC-32 (IEEE 802.3) of `data`, the algorithm `Bun.hash.crc32` implements.
fn crc32(data: &[u8]) -> u32 {
	let mut crc: u32 = 0xffff_ffff;
	for &byte in data {
		crc ^= u32::from(byte);
		for _ in 0..8 {
			let mask = (crc & 1).wrapping_neg();
			crc = (crc >> 1) ^ (0xedb8_8320 & mask);
		}
	}
	!crc
}

/// Draw an extension link's label: dotted-underlined (plain underline where
/// the terminal has no styled underlines, re-applied after any style reset
/// inside it) and wrapped in the OSC 8 marker [`extension_link_labels`] reads
/// back in a plain terminal; as given in a native view. Never followed by a
/// ` (url)` suffix. `key` tells apart links of one URL drawn near each other;
/// it must not change between renders of the same text, or rows rendered twice
/// stop matching.
pub fn format_extension_link(label: &str, href: &str, key: &str) -> String {
	ensure_registered();
	format_extension_link_with(
		label,
		href,
		key,
		native_state().is_native_rendering(),
		*STYLED_UNDERLINES,
	)
}

fn format_extension_link_with(label: &str, href: &str, key: &str, native: bool, styled: bool) -> String {
	if native {
		return label.to_string();
	}
	let safe = href.replace('\x1b', "").replace('\x07', "");
	if safe.is_empty() {
		return label.to_string();
	}
	let underline = underline_style(styled);
	let body = SGR_REGEX
		.replace_all(label, |caps: &regex::Captures| {
			let sequence = caps.get(0).expect("whole match").as_str();
			let params = caps.get(1).expect("params group").as_str();
			if ends_underline(params) {
				format!("{sequence}{}", underline.on)
			} else {
				sequence.to_string()
			}
		})
		.into_owned();
	let id = crc32(format!("{safe}\0{key}").as_bytes());
	format!("{MARKER_OPEN}-{id};{safe}\x07{}{}{}\x1b]8;;\x07", underline.on, body, underline.off)
}

/// `line` without maple's extension-link markers (the OSC 8 open and close
/// around each label), for terminal output. Every other hyperlink is kept as
/// is.
pub fn strip_extension_link_markers(line: &str) -> String {
	if !line.contains(MARKER_OPEN) {
		return line.to_string();
	}
	let mut out = String::with_capacity(line.len());
	let mut inside = false;
	let mut copied = 0usize;
	for caps in OSC8_REGEX.captures_iter(line) {
		let whole = caps.get(0).expect("whole match");
		out.push_str(&line[copied..whole.start()]);
		let params = caps.get(1).expect("params group").as_str();
		let uri = caps.get(2).expect("uri group").as_str();
		if uri.is_empty() {
			let closes_own = inside;
			inside = false;
			if !closes_own {
				out.push_str(whole.as_str());
			}
		} else {
			inside = MARKER_PARAMS_REGEX.is_match(params);
			if !inside {
				out.push_str(whole.as_str());
			}
		}
		copied = whole.end();
	}
	out.push_str(&line[copied..]);
	out
}

/// One extension link label drawn on a rendered row (before
/// [`strip_extension_link_markers`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionLinkLabel {
	/// The link's URL.
	pub url: String,
	/// The marker's params, unique to each drawn link and repeated on every row
	/// a wrapped label spans, so a row's parts of one label can be told from an
	/// adjacent label of the same URL.
	pub id: String,
	/// First 0-based cell the label covers.
	pub start: usize,
	/// Cell just past the label.
	pub end: usize,
	/// Whether only blank cells follow the label on its row, so a wrapped label
	/// may go on below.
	pub ends_row: bool,
}

/// The extension link labels drawn on a rendered row, left to right. Only
/// maple's own markers with a registered scheme match, so ordinary hyperlinks
/// (even of a registered scheme) and rows drawn for a scheme that is no longer
/// registered never do.
pub fn extension_link_labels(line: &str) -> Vec<ExtensionLinkLabel> {
	with_schemes(|schemes| extension_link_labels_with(line, schemes))
}

fn extension_link_labels_with(line: &str, schemes: &BTreeSet<String>) -> Vec<ExtensionLinkLabel> {
	if schemes.is_empty() || !line.contains(MARKER_OPEN) {
		return Vec::new();
	}
	let mut labels: Vec<ExtensionLinkLabel> = Vec::new();
	let mut x = 0usize;
	let mut cursor = 0usize;
	// Code-unit index where the last pushed label's text ends.
	let mut last_close = 0usize;
	let mut open: Option<(String, String, usize)> = None;
	for caps in OSC8_REGEX.captures_iter(line) {
		let whole = caps.get(0).expect("whole match");
		x += visible_width(&line[cursor..whole.start()]);
		if let Some((url, id, start)) = open.take() {
			if x > start {
				labels.push(ExtensionLinkLabel {
					url,
					id,
					start,
					end: x,
					ends_row: false,
				});
				last_close = whole.start();
			}
		}
		let params = caps.get(1).expect("params group").as_str();
		let uri = caps.get(2).expect("uri group").as_str();
		cursor = whole.end();
		if !uri.is_empty() && MARKER_PARAMS_REGEX.is_match(params) && extension_link_scheme_with(schemes, uri).is_some() {
			open = Some((uri.to_string(), params.to_string(), x));
		}
	}
	x += visible_width(&line[cursor..]);
	let mut pushed = false;
	if let Some((url, id, start)) = open {
		if x > start {
			labels.push(ExtensionLinkLabel {
				url,
				id,
				start,
				end: x,
				ends_row: true,
			});
			pushed = true;
		}
	}
	if !pushed {
		if let Some(last) = labels.last_mut() {
			if strip_ansi(&line[last_close..]).trim().is_empty() {
				last.ends_row = true;
			}
		}
	}
	labels
}

/// The process-wide scheme state is shared with the link-hints tests, so the
/// tests that mutate it serialize on one lock.
#[cfg(test)]
pub(crate) fn test_lock() -> &'static std::sync::Mutex<()> {
	static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
	&TEST_LOCK
}

#[cfg(test)]
mod tests {
	use super::*;


	fn marked(id: u32, url: &str, text: &str) -> String {
		format!("\x1b]8;id=omp-extension-link-{id};{url}\x07{text}\x1b]8;;\x07")
	}

	fn omp_schemes() -> BTreeSet<String> {
		["omp".to_string()].into_iter().collect()
	}

	#[test]
	fn scheme_matches_only_registered_lowercase_prefix() {
		let schemes = omp_schemes();
		assert_eq!(extension_link_scheme_with(&schemes, "OMP://x"), Some("omp".to_string()));
		assert_eq!(extension_link_scheme_with(&schemes, "omp://x"), Some("omp".to_string()));
		assert_eq!(extension_link_scheme_with(&schemes, "file:///x"), None);
		assert_eq!(extension_link_scheme_with(&schemes, "nocolon"), None);
		assert_eq!(extension_link_scheme_with(&schemes, "://x"), None);
		assert_eq!(extension_link_scheme_with(&BTreeSet::new(), "omp://x"), None);
	}

	#[test]
	fn ends_underline_table() {
		for params in ["", "0", "24", "4:0", "1;24;39", "0;38;5;1"] {
			assert!(ends_underline(params), "{params} should end underline");
		}
		for params in ["1", "4:3", "38;5;0", "38;2;0;0;0", "48;2;1;2;3", "58;5;9"] {
			assert!(!ends_underline(params), "{params} should keep underline");
		}
	}

	#[test]
	fn format_marks_label_and_strips_back_out() {
		let schemes = omp_schemes();
		let formatted = format_extension_link_with("Alpha", "omp://a", "k", false, false);
		assert!(formatted.starts_with("\x1b]8;id=omp-extension-link-"), "{formatted:?}");
		assert!(formatted.contains("\x1b[4mAlpha\x1b[24m"), "{formatted:?}");
		assert!(formatted.ends_with("\x1b]8;;\x07"), "{formatted:?}");
		assert_eq!(strip_extension_link_markers(&formatted), "\x1b[4mAlpha\x1b[24m");

		let labels = extension_link_labels_with(&formatted, &schemes);
		assert_eq!(labels.len(), 1);
		assert_eq!(labels[0].url, "omp://a");
		assert_eq!(labels[0].start, 0);
		assert_eq!(labels[0].end, 5);
		assert!(labels[0].ends_row);
	}

	#[test]
	fn format_uses_dotted_underline_where_supported() {
		let formatted = format_extension_link_with("Alpha", "omp://a", "k", false, true);
		assert!(formatted.contains("\x1b[4:4mAlpha\x1b[4:0m"), "{formatted:?}");
		assert!(!formatted.contains("\x1b[4m"), "{formatted:?}");
	}

	#[test]
	fn format_reapplies_underline_after_a_reset_inside_the_label() {
		let formatted = format_extension_link_with("a\x1b[0mb", "omp://a", "k", false, false);
		assert!(formatted.contains("\x1b[0m\x1b[4mb"), "{formatted:?}");
	}

	#[test]
	fn format_keeps_label_in_native_view_or_when_href_is_unsafe() {
		assert_eq!(format_extension_link_with("Alpha", "omp://a", "k", true, false), "Alpha");
		assert_eq!(format_extension_link_with("Alpha", "\x1b\x07", "k", false, false), "Alpha");
	}

	#[test]
	fn marker_id_is_stable_per_key_and_unique_per_link() {
		let a = format_extension_link_with("x", "omp://a", "k", false, false);
		let b = format_extension_link_with("x", "omp://a", "k", false, false);
		let other_key = format_extension_link_with("x", "omp://a", "other", false, false);
		let other_url = format_extension_link_with("x", "omp://b", "k", false, false);
		assert_eq!(a, b);
		assert_ne!(a, other_key);
		assert_ne!(a, other_url);
	}

	#[test]
	fn strip_keeps_foreign_osc8_sequences() {
		let foreign = "\x1b]8;;https://example.com\x07link\x1b]8;;\x07";
		assert_eq!(strip_extension_link_markers(foreign), foreign);
		let mixed = format!("a{}b{foreign}c", marked(1, "omp://a", "Alpha"));
        let stripped = strip_extension_link_markers(&mixed);
        // Only omp's own OSC 8 wrappers are removed; the label text stays.
        assert!(stripped.starts_with("aAlpha"), "{stripped:?}");
        assert!(stripped.ends_with(&format!("b{foreign}c")), "{stripped:?}");
        assert!(!stripped.contains("id=omp-extension-link-"), "{stripped:?}");
	}

	#[test]
	fn labels_report_columns_and_ends_row() {
		let schemes = omp_schemes();
		let line = format!("see {} tail", marked(1, "omp://a", "Alpha"));
		let labels = extension_link_labels_with(&line, &schemes);
		assert_eq!(labels.len(), 1);
		assert_eq!(labels[0].url, "omp://a");
		assert_eq!(labels[0].id, "id=omp-extension-link-1");
		assert_eq!(labels[0].start, 4);
		assert_eq!(labels[0].end, 9);
		assert!(!labels[0].ends_row);

		let line = format!("see {}{}", marked(1, "omp://a", "Alpha"), "   ");
		let labels = extension_link_labels_with(&line, &schemes);
		assert!(labels[0].ends_row);

		let line = marked(1, "omp://a", "Alpha");
		let labels = extension_link_labels_with(&line, &schemes);
		assert!(labels[0].ends_row);
	}

	#[test]
	fn labels_ignore_unregistered_schemes_and_foreign_markers() {
		let line = format!("{}{}", marked(1, "omp://a", "Alpha"), marked(2, "other://b", "Beta"));
		assert!(extension_link_labels_with(&line, &BTreeSet::new()).is_empty());
		let schemes = omp_schemes();
		let labels = extension_link_labels_with(&line, &schemes);
		assert_eq!(labels.len(), 1);
		assert_eq!(labels[0].url, "omp://a");
	}

	#[test]
	fn process_wide_scheme_state() {
		let _guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		let before = extension_links_version();
		set_extension_links(["omp", "omp"]);
		assert_eq!(extension_links_version(), before, "an unchanged scheme set must not bump");
		set_extension_links(["omp", "other"]);
		assert!(extension_links_version() > before);

		assert_eq!(extension_link_scheme("OMP://x"), Some("omp".to_string()));
		assert_eq!(extension_link_scheme("nope://x"), None);
		assert_eq!(
			extension_links_routed(),
			!native_state().is_native_rendering(),
			"one scheme is registered, so routing follows native rendering"
		);

		set_extension_link_picking(None);
		assert!(!extension_link_clicks_available());
		set_extension_link_picking(Some(Arc::new(|| false)));
		assert!(!extension_link_clicks_available());
		set_extension_link_picking(Some(Arc::new(|| true)));
		assert_eq!(
			extension_link_clicks_available(),
			!native_state().is_native_rendering()
		);
		set_extension_link_picking(None);
		set_extension_links(Vec::<String>::new());
		assert!(!extension_links_routed());
		assert_eq!(extension_link_scheme("omp://x"), None);
	}
}
