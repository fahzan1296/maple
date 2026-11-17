//! Port of `l/test/hyperlink.test.ts`.
//!
//! The crate ports the OSC 8 helpers in `render::hyperlink` plus the native
//! `file_href`/`openable_image_props` seams. Cases that need a TTY probe, Node's
//! Windows path parser, a logger warning hook, or the chat Markdown renderer
//! stay ignored.

use maple_tui::native::blobs::openable_image_props;
use maple_tui::render::hyperlink::{
	apply_file_link_setting, apply_hyperlink_setting, file_hyperlink, file_link_span,
	file_uri_for_terminal, hyperlink_mode, is_hyperlink_enabled, uri_hyperlink, url_hyperlink,
	url_hyperlink_always, FileLinkConfig, FilePosition, HyperlinkMode,
};
use maple_tui::tools::native_view::file_href;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Hyperlink policy and file-link target are process globals; serialize tests.
fn lock() -> MutexGuard<'static, ()> {
	static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
	LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn extract_uri(text: &str) -> Option<String> {
	let start = text.find("\x1b]8;")? + "\x1b]8;".len();
	let rest = &text[start..];
	let end = rest.find(|ch| ch == '\x1b' || ch == '\x07')?;
	let body = &rest[..end];
	body.split_once(';').map(|(_, uri)| uri.to_string())
}

fn wrapped(text: &str) -> bool {
	text.contains("\x1b]8;") && text.contains("\x1b]8;;\x1b\\")
}

fn position(line: Option<usize>, col: Option<usize>) -> FilePosition {
	FilePosition { line, col }
}

fn remote_target() {
	apply_file_link_setting(FileLinkConfig {
		scheme: Some("ompfile".to_string()),
		host: Some("beelink".to_string()),
	});
}

fn clear_target() {
	apply_file_link_setting(FileLinkConfig::default());
}

#[test]
fn is_hyperlink_enabled_follows_the_policy() {
	let _guard = lock();
	apply_hyperlink_setting(Some(HyperlinkMode::Off));
	assert!(!is_hyperlink_enabled());

	apply_hyperlink_setting(Some(HyperlinkMode::Always));
	assert!(is_hyperlink_enabled());

	apply_hyperlink_setting(Some(HyperlinkMode::Auto));
	std::env::set_var("NO_COLOR", "1");
	assert!(!is_hyperlink_enabled());
	std::env::remove_var("NO_COLOR");
}

#[test]
fn auto_mode_is_immune_to_runtime_capability_mutation() {
	let _guard = lock();
	apply_hyperlink_setting(Some(HyperlinkMode::Auto));
	std::env::remove_var("NO_COLOR");
	maple_tui::render::hyperlink::set_detected_hyperlink_support(true);
	let detected = is_hyperlink_enabled();
	maple_tui::render::hyperlink::set_detected_hyperlink_support(false);
	assert_eq!(is_hyperlink_enabled(), detected);
}

#[test]
fn file_hyperlink_respects_the_policy_and_encodes_paths() {
	let _guard = lock();
	clear_target();

	apply_hyperlink_setting(Some(HyperlinkMode::Off));
	assert_eq!(file_hyperlink("/Users/foo/bar.ts", "bar.ts", position(None, None)), "bar.ts");

	apply_hyperlink_setting(Some(HyperlinkMode::Always));
	let wrapped = file_hyperlink("/Users/foo/bar.ts", "bar.ts", position(None, None));
	assert!(self::wrapped(&wrapped), "{wrapped:?}");
	assert!(wrapped.contains("bar.ts"));

	let encoded = file_hyperlink("/tmp/a#b?c% d.ts", "a#b?c% d.ts", position(Some(12), None));
	let uri = extract_uri(&encoded).expect("uri");
	assert_eq!(uri, "file:///tmp/a%23b%3Fc%25%20d.ts");
	assert!(!uri.contains('?'));
	assert!(!uri.contains('#'));
}

#[test]
fn file_hyperlink_resolves_relative_paths_and_keeps_positions_out_of_the_uri() {
	let _guard = lock();
	clear_target();
	apply_hyperlink_setting(Some(HyperlinkMode::Always));

	let relative = file_hyperlink("relative file#1.ts", "relative file#1.ts", position(None, None));
	let uri = extract_uri(&relative).expect("uri");
	assert!(uri.ends_with("relative%20file%231.ts"), "{uri}");

	let located = file_hyperlink("/Users/foo/bar.ts", "bar.ts", position(Some(42), Some(7)));
	let uri = extract_uri(&located).expect("uri");
	assert_eq!(uri, "file:///Users/foo/bar.ts");
	assert!(!uri.contains("line"));
	assert!(!uri.contains("col"));
}

#[test]
fn file_hyperlink_ids_are_stable_and_wrapping_is_idempotent() {
	let _guard = lock();
	clear_target();
	apply_hyperlink_setting(Some(HyperlinkMode::Always));

	let first = file_hyperlink("/Users/foo/bar.ts", "bar.ts", position(None, None));
	let second = file_hyperlink("/Users/foo/bar.ts", "different display text", position(None, None));
	let id = |text: &str| {
		let body = &text[text.find("\x1b]8;").unwrap() + 5..];
		let end = body.find('\x1b').unwrap();
		body[..end].to_string()
	};
	assert_eq!(id(&first), id(&second));

	let already = "\x1b]8;id=abc123;file:///foo/bar.ts\x1b\\bar.ts\x1b]8;;\x1b\\";
	assert_eq!(
		file_hyperlink("/Users/foo/other.ts", already, position(None, None)),
		already
	);

	let colored = "\x1b[32mbar.ts\x1b[0m";
	let result = file_hyperlink("/Users/foo/bar.ts", colored, position(None, None));
	assert!(result.contains(colored), "{result:?}");
	assert!(wrapped(&result));
}

#[test]
fn file_uri_for_terminal_uses_the_vscode_form_only_for_vscode() {
	let _guard = lock();
	clear_target();

	assert_eq!(
		file_uri_for_terminal("/Users/foo/bar.ts", position(Some(42), Some(7)), "vscode"),
		Some("vscode://file/Users/foo/bar.ts:42:7".to_string())
	);
	assert_eq!(
		file_uri_for_terminal("/Users/foo/bar.ts", position(Some(42), None), "vscode"),
		Some("vscode://file/Users/foo/bar.ts:42".to_string())
	);
	assert_eq!(
		file_uri_for_terminal("/Users/foo/bar.ts", position(None, None), "vscode"),
		Some("vscode://file/Users/foo/bar.ts".to_string())
	);
	assert_eq!(
		file_uri_for_terminal("/Users/foo/a#b?c% d.ts", position(Some(42), Some(7)), "vscode"),
		Some("vscode://file/Users/foo/a%23b%3Fc%25%20d.ts:42:7".to_string())
	);
	for terminal in ["iterm2", "kitty", "base"] {
		assert_eq!(
			file_uri_for_terminal("/Users/foo/bar.ts", position(Some(42), Some(7)), terminal),
			Some("file:///Users/foo/bar.ts".to_string())
		);
	}
}

#[test]
fn file_uri_for_terminal_honors_the_remote_target_over_vscode() {
	let _guard = lock();
	remote_target();
	assert_eq!(
		file_uri_for_terminal("/Users/foo/bar.ts", position(Some(10), None), "vscode"),
		Some("ompfile://beelink/Users/foo/bar.ts?line=10".to_string())
	);
	clear_target();

	apply_file_link_setting(FileLinkConfig {
		host: Some("remote-server".to_string()),
		..FileLinkConfig::default()
	});
	assert_eq!(
		file_uri_for_terminal("/Users/foo/bar.ts", position(None, None), "base"),
		Some("file:///Users/foo/bar.ts".to_string())
	);
	clear_target();
	assert_eq!(
		file_uri_for_terminal("/Users/foo/bar.ts", position(None, None), "kitty"),
		Some("file:///Users/foo/bar.ts".to_string())
	);
	assert_eq!(
		file_uri_for_terminal("/Users/foo/bar.ts", position(Some(10), None), "vscode"),
		Some("vscode://file/Users/foo/bar.ts:10".to_string())
	);
}

#[test]
fn custom_handler_links_carry_positions_and_native_hrefs() {
	let _guard = lock();
	remote_target();
	apply_hyperlink_setting(Some(HyperlinkMode::Always));

	let linked = file_hyperlink("/tmp/a#b?c% 中文.ts", "a.ts", position(Some(42), Some(7)));
	let uri = extract_uri(&linked).expect("uri");
	let (base, query) = uri.split_once('?').expect("query");
	assert_eq!(base, "ompfile://beelink/tmp/a%23b%3Fc%25%20%E4%B8%AD%E6%96%87.ts");
	assert_eq!(query, "line=42&col=7");

	let probe = maple_tui::render::hyperlink::try_file_uri(
		"/tmp/a#b?c% 中文.ts",
		position(Some(42), Some(7)),
		false,
	)
	.expect("uri");
	assert!(probe.ends_with("?col=7"), "{probe}");

	let span = file_link_span("/tmp/remote project/a.ts", "a.ts", "path");
	assert_eq!(span.t, "a.ts");
	assert_eq!(span.s.as_deref(), Some("path"));
	assert_eq!(span.href.as_deref(), Some("ompfile://beelink/tmp/remote%20project/a.ts"));

	assert_eq!(
		file_href(Some("/tmp/remote project/a.ts")).as_deref(),
		Some("ompfile://beelink/tmp/remote%20project/a.ts")
	);
	let props = openable_image_props(Some("/tmp/remote project/a.ts"));
	assert_eq!(props.get_str("href"), Some("file:///tmp/remote%20project/a.ts"));
	assert_eq!(
		props.get("actions").and_then(|value| value.get("menu")).and_then(|value| value.as_array()).map(|items| items.len()),
		Some(1)
	);
}

#[test]
fn custom_handler_keeps_a_unc_server_in_the_target() {
	let _guard = lock();
	remote_target();
	let uri = maple_tui::render::hyperlink::try_file_uri(
		"\\\\nas\\share\\a#b% 中文.ts",
		position(None, None),
		false,
	)
	.expect("uri");
	assert_eq!(uri, "ompfile://beelink//nas/share/a%23b%25%20%E4%B8%AD%E6%96%87.ts");
}

#[test]
fn uri_hyperlink_wraps_safe_uris_only() {
	let _guard = lock();
	apply_hyperlink_setting(Some(HyperlinkMode::Always));
	let linked = uri_hyperlink("local://handoff.md", "handoff");
	assert!(wrapped(&linked), "{linked:?}");
	assert_eq!(extract_uri(&linked).as_deref(), Some("local://handoff.md"));

	assert_eq!(uri_hyperlink("https://example.com/\x07bad", "bad"), "bad");
}

#[test]
fn url_hyperlink_normalizes_www_and_rejects_other_schemes() {
	let _guard = lock();
	apply_hyperlink_setting(Some(HyperlinkMode::Always));
	let linked = url_hyperlink("www.example.com/path", "example");
	assert!(wrapped(&linked), "{linked:?}");
	assert_eq!(extract_uri(&linked).as_deref(), Some("https://www.example.com/path"));
	assert_eq!(url_hyperlink("ftp://example.com/file", "file"), "file");
}

#[test]
fn url_hyperlink_always_bypasses_capability_detection() {
	let _guard = lock();
	apply_hyperlink_setting(Some(HyperlinkMode::Auto));
	std::env::set_var("NO_COLOR", "1");
	assert!(!is_hyperlink_enabled());
	let linked = url_hyperlink_always("www.example.com/path", "example");
	assert!(linked.contains("\x1b]8;"), "{linked:?}");
	assert!(linked.contains("\x1b]8;;\x07"), "{linked:?}");
	assert_eq!(extract_uri(&linked).as_deref(), Some("https://www.example.com/path"));
	std::env::remove_var("NO_COLOR");

	apply_hyperlink_setting(Some(HyperlinkMode::Off));
	assert_eq!(url_hyperlink_always("https://example.com/path", "example"), "example");

	apply_hyperlink_setting(Some(HyperlinkMode::Always));
	assert_eq!(url_hyperlink_always("ftp://example.com/file", "file"), "file");
	assert_eq!(hyperlink_mode(), HyperlinkMode::Always);
}

#[test]
#[ignore = "missing facility: no stdout TTY probe in the Rust test harness"]
fn auto_mode_is_false_when_stdout_is_not_a_tty() {}

#[test]
#[ignore = "missing facility: Rust chars cannot hold a lone surrogate"]
fn keeps_a_lone_surrogate_path_link_free() {}

#[test]
#[ignore = "platform-specific: reference asserts Node's Windows path parsing for a malformed UNC path"]
fn renders_a_path_node_cannot_encode_as_plain_text() {}

#[test]
#[ignore = "divergence: crate leaves the host empty, reference defaults to os.hostname()"]
fn defaults_host_to_the_machine_name_when_only_the_scheme_is_set() {}

#[test]
#[ignore = "missing facility: no logger warning hook on apply_file_link_setting"]
fn warns_and_restores_ordinary_links_for_a_protocol_separator_scheme() {}

#[test]
#[ignore = "missing facility: no logger warning hook on apply_file_link_setting"]
fn warns_and_restores_ordinary_links_for_a_host_that_would_reshape_the_uri() {}

#[test]
#[ignore = "divergence: crate scheme validation rejects uppercase, reference accepts case-insensitively"]
fn accepts_extended_scheme_characters_case_insensitively() {}

#[test]
#[ignore = "missing facility: no SkillMessageComponent or chat markdown renderer"]
fn routes_a_native_skill_card_to_the_remote_skill_md() {}

#[test]
#[ignore = "missing facility: no chat Markdown component wired to terminal capability flags"]
fn chat_markdown_links_honor_the_hyperlink_policy() {}
