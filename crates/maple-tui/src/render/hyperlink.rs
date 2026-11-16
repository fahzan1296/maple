//! OSC 8 terminal hyperlinks for paths and URLs.
//!
//! [`hyperlink`] wraps display text in `ESC ] 8 ; id=HASH ; URI ESC \ TEXT
//! ESC ] 8 ; ; ESC \`. Emission is gated by the `tui.hyperlinks` policy
//! ([`HyperlinkMode::Off`] never emits, [`HyperlinkMode::Always`] always does,
//! [`HyperlinkMode::Auto`] follows terminal capability detection and
//! `NO_COLOR`). [`strip_hyperlinks`] removes the wrapper again for surfaces
//! that cannot open links, and [`find_hyperlink_at`] maps a clicked cell back
//! to its target.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::ansi_scan::{scan, strip_ansi, Token};
use crate::text_shape::visible_width;

const OSC: &str = "\x1b]";
const ST: &str = "\x1b\\";
const BEL: &str = "\x07";

/// The `tui.hyperlinks` policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HyperlinkMode {
    /// Never emit OSC 8.
    Off,
    /// Follow terminal capability detection.
    #[default]
    Auto,
    /// Emit unconditionally.
    Always,
}

/// Remote file-link configuration applied to every emitted file link.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileLinkConfig {
    pub scheme: Option<String>,
    pub host: Option<String>,
}

/// A hyperlink drawn on a rendered row, with the display cells it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HyperlinkSpan {
    pub url: String,
    pub start: usize,
    pub end: usize,
}

/// Optional cursor position carried by a file link.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilePosition {
    pub line: Option<usize>,
    pub col: Option<usize>,
}

static MODE: AtomicU8 = AtomicU8::new(HyperlinkMode::Auto as u8);
static DETECTED: AtomicBool = AtomicBool::new(false);
static FILE_TARGET: std::sync::RwLock<Option<(String, String)>> = std::sync::RwLock::new(None);

/// Record whether the terminal advertised OSC 8 support.
pub fn set_detected_hyperlink_support(supported: bool) {
    DETECTED.store(supported, Ordering::Relaxed);
}

/// Push a resolved [`HyperlinkMode`] policy; `None` keeps the last one.
pub fn apply_hyperlink_setting(mode: Option<HyperlinkMode>) {
    if let Some(mode) = mode {
        MODE.store(mode as u8, Ordering::Relaxed);
    }
}

/// The active hyperlink policy.
pub fn hyperlink_mode() -> HyperlinkMode {
    match MODE.load(Ordering::Relaxed) {
        0 => HyperlinkMode::Off,
        2 => HyperlinkMode::Always,
        _ => HyperlinkMode::Auto,
    }
}

/// Whether OSC 8 hyperlinks should be emitted under the active policy.
pub fn is_hyperlink_enabled() -> bool {
    match hyperlink_mode() {
        HyperlinkMode::Off => false,
        HyperlinkMode::Always => true,
        HyperlinkMode::Auto => {
            if std::env::var_os("NO_COLOR").is_some() {
                return false;
            }
            if !std::io::stdout().is_terminal() {
                return false;
            }
            DETECTED.load(Ordering::Relaxed)
        }
    }
}

/// Resolve the remote file-link target once at host startup. An invalid scheme
/// or host keeps ordinary `file:` links.
pub fn apply_file_link_setting(config: FileLinkConfig) {
    let next = match config.scheme {
        Some(scheme) if is_valid_scheme(&scheme) => {
            let host = config.host.unwrap_or_default();
            if is_valid_host(&host) {
                Some((scheme.to_ascii_lowercase(), host))
            } else {
                None
            }
        }
        _ => None,
    };
    if let Ok(mut guard) = FILE_TARGET.write() {
        *guard = next;
    }
}

fn is_valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '+' | '.' | '-'))
}

fn is_valid_host(host: &str) -> bool {
    match host.split_once(':') {
        Some((name, port)) => is_valid_hostname(name) && port.parse::<u16>().is_ok(),
        None => is_valid_hostname(host),
    }
}

fn is_valid_hostname(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// Stable 8-char hex id derived from a URI so terminals coalesce identical
/// adjacent links.
fn build_link_id(uri: &str) -> String {
    let mut hash: u32 = 0;
    for byte in uri.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u32);
    }
    format!("{hash:08x}")
}

/// Whether a URI is safe to embed: no terminal control bytes.
fn safe_uri(uri: &str) -> bool {
    !uri
        .chars()
        .any(|ch| ('\x00'..='\x1f').contains(&ch) || ch == '\x7f')
}

fn wrap_core(uri: &str, display_text: &str, terminator: &str) -> String {
    // Do not double-wrap text that already embeds an OSC 8 sequence.
    if display_text.contains("\x1b]8;") {
        return display_text.to_string();
    }
    if !safe_uri(uri) {
        return display_text.to_string();
    }
    let id = build_link_id(uri);
    format!("{OSC}8;id={id};{uri}{terminator}{display_text}{OSC}8;;{terminator}")
}

/// Wrap `text` in an OSC 8 hyperlink pointing at `url`.
///
/// Returns `text` unchanged when hyperlinks are disabled, `url` carries
/// control bytes, or `text` already contains an OSC 8 sequence.
pub fn hyperlink(text: &str, url: &str) -> String {
    if !is_hyperlink_enabled() {
        return text.to_string();
    }
    wrap_core(url, text, ST)
}

/// Alias of [`hyperlink`] with the TS argument order.
pub fn uri_hyperlink(uri: &str, display_text: &str) -> String {
    hyperlink(display_text, uri)
}

/// Wrap `text` in an OSC 8 hyperlink to an HTTP(S) URL. `www.` inputs are
/// linked as `https://`.
pub fn url_hyperlink(url: &str, display_text: &str) -> String {
    match normalize_http_url(url) {
        Some(href) => {
            if !is_hyperlink_enabled() {
                display_text.to_string()
            } else {
                wrap_core(&href, display_text, ST)
            }
        }
        None => display_text.to_string(),
    }
}

/// Wrap `text` in a BEL-terminated OSC 8 hyperlink, bypassing capability
/// auto-detection. An explicit [`HyperlinkMode::Off`] still suppresses it.
pub fn url_hyperlink_always(url: &str, display_text: &str) -> String {
    if hyperlink_mode() == HyperlinkMode::Off {
        return display_text.to_string();
    }
    match normalize_http_url(url) {
        Some(href) => wrap_core(&href, display_text, BEL),
        None => display_text.to_string(),
    }
}

fn normalize_http_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("www.") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    };
    let lower = candidate.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(candidate)
    } else {
        None
    }
}

/// Percent-encode a path for a URI, keeping `/` and a Windows drive colon
/// structural.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '/' => out.push('/'),
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' | '_' | '~' | ':' => out.push(ch),
            _ => {
                let mut buffer = [0u8; 4];
                for byte in ch.encode_utf8(&mut buffer).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}


/// The body and terminator of an OSC 8 sequence, when `sequence` opens one.
fn osc8_body(sequence: &str) -> Option<(&str, &str)> {
    let body = sequence.strip_prefix("\x1b]8;")?;
    let bell = body.find('\x07');
    let terminator = body.find("\x1b\\");
    let end = match (bell, terminator) {
        (Some(b), Some(s)) => b.min(s),
        (Some(b), None) => b,
        (None, Some(s)) => s,
        (None, None) => return None,
    };
    let terminator = if bell == Some(end) { BEL } else { ST };
    Some((&body[..end], terminator))
}

/// Split a filesystem path into URI authority and path. A UNC target keeps its
/// server as the authority; everything else is a plain absolute path.
fn file_url_parts(file_path: &str) -> Option<(String, String)> {
    if file_path.is_empty() {
        return None;
    }
    if file_path.starts_with("\\\\") || file_path.starts_with("//") {
        let trimmed = file_path.trim_start_matches(['\\', '/']);
        let (host, rest) = trimmed.split_once(['\\', '/'])?;
        if host.is_empty() {
            return None;
        }
        return Some((host.to_string(), format!("/{}", encode_path(rest))));
    }
    let absolute = if file_path.starts_with('/') {
        file_path.to_string()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(file_path).to_string_lossy().into_owned(),
            Err(_) => format!("/{file_path}"),
        }
    };
    Some((String::new(), encode_path(&absolute)))
}

/// Build a `file:` URI for `file_path`, honoring the configured remote target
/// and optional cursor position. Returns `None` when the path cannot be
/// encoded.
pub fn try_file_uri(file_path: &str, position: FilePosition, local: bool) -> Option<String> {
    let (authority, path) = file_url_parts(file_path)?;
    let file_url = format!("file://{authority}{path}");
    let _ = local;
    let target = FILE_TARGET.read().ok();
    let Some((scheme, host)) = target.as_ref().and_then(|guard| guard.as_ref()) else {
        return Some(file_url);
    };
    // Preserve UNC servers so the handler can reconstruct the network path.
    let target_path = if authority.is_empty() {
        path.clone()
    } else {
        format!("//{authority}{path}")
    };
    let mut query = String::new();
    if let Some(line) = position.line {
        query.push_str(&format!("?line={line}"));
    }
    if let Some(col) = position.col {
        if query.is_empty() {
            query.push('?');
        } else {
            query.push('&');
        }
        query.push_str(&format!("col={col}"));
    }
    Some(format!("{scheme}://{host}{target_path}{query}"))
}

/// Build the OSC 8 target for a file path on a terminal. VS Code navigates the
/// documented `vscode://file/<path>:<line>:<col>` form; every other terminal
/// uses a plain `file:` URI, with a custom handler taking precedence.
pub fn file_uri_for_terminal(
    file_path: &str,
    position: FilePosition,
    terminal_id: &str,
) -> Option<String> {
    let target = FILE_TARGET.read().ok();
    if target.as_ref().and_then(|guard| guard.as_ref()).is_some() || terminal_id != "vscode" {
        return try_file_uri(file_path, position, false);
    }
    let (authority, path) = file_url_parts(file_path)?;
    if !authority.is_empty() {
        return Some(format!("vscode://file/{authority}{path}"));
    }
    let position_suffix = match (position.line, position.col) {
        (None, _) => String::new(),
        (Some(line), None) => format!(":{line}"),
        (Some(line), Some(col)) => format!(":{line}:{col}"),
    };
    Some(format!("vscode://file{path}{position_suffix}"))
}

/// Wrap `display_text` in an OSC 8 hyperlink to a filesystem path. Relative
/// paths resolve against the current directory; an unencodable path keeps its
/// text without a link.
pub fn file_hyperlink(file_path: &str, display_text: &str, position: FilePosition) -> String {
    match file_uri_for_terminal(file_path, position, "auto") {
        Some(uri) => hyperlink(display_text, &uri),
        None => display_text.to_string(),
    }
}

/// Remove every OSC 8 open/close sequence, leaving the display text in place.
pub fn strip_hyperlinks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in scan(text) {
        match token {
            Token::Escape(seq) if seq.starts_with("\x1b]8;") => {}
            Token::Escape(seq) => out.push_str(seq),
            Token::Text(chunk) => out.push_str(chunk),
        }
    }
    out
}

/// The hyperlinks drawn on a rendered row, with the display cells they cover.
pub fn hyperlink_spans(text: &str) -> Vec<HyperlinkSpan> {
    let mut spans: Vec<HyperlinkSpan> = Vec::new();
    let mut column = 0usize;
    let mut open: Option<String> = None;
    let mut span_start = 0usize;
    for token in scan(text) {
        match token {
            Token::Text(chunk) => column += visible_width(chunk),
            Token::Escape(seq) => {
                if let Some((body, _)) = osc8_body(seq) {
                    if let Some((params, uri)) = body.split_once(';') {
                        if uri.is_empty() {
                            if let Some(url) = open.take() {
                                if column > span_start {
                                    spans.push(HyperlinkSpan {
                                        url,
                                        start: span_start,
                                        end: column,
                                    });
                                }
                            }
                        } else if params.starts_with("id=") {
                            open = Some(uri.to_string());
                            span_start = column;
                        }
                    }
                }
            }
        }
    }
    spans
}

/// The link covering `column`, if any; used to route a mouse click.
pub fn find_hyperlink_at(text: &str, column: usize) -> Option<String> {
    hyperlink_spans(text)
        .into_iter()
        .find(|span| column >= span.start && column < span.end)
        .map(|span| span.url)
}

/// The display text of a row with hyperlinks removed.
pub fn plain_hyperlink_text(text: &str) -> String {
    strip_ansi(&strip_hyperlinks(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that read or write the process-wide file-link target.
    /// `try_file_uri` consults a global, so unsynchronized parallel tests race
    /// (the setting from one test leaks into another's expectation).
    static FILE_LINK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn file_link_guard() -> std::sync::MutexGuard<'static, ()> {
        FILE_LINK_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn wraps_with_osc8_and_id() {
        apply_hyperlink_setting(Some(HyperlinkMode::Always));
        let linked = hyperlink("file.rs", "file:///tmp/file.rs");
        assert!(linked.starts_with("\x1b]8;id="));
        assert!(linked.contains(";file:///tmp/file.rs\x1b\\file.rs\x1b]8;;\x1b\\"));
        assert_eq!(build_link_id("x"), "00000078");
    }

    #[test]
    fn off_policy_returns_plain_text() {
        apply_hyperlink_setting(Some(HyperlinkMode::Off));
        assert_eq!(hyperlink("a", "https://example.com"), "a");
        apply_hyperlink_setting(Some(HyperlinkMode::Auto));
    }

    #[test]
    fn rejects_control_bytes_and_double_wrap() {
        apply_hyperlink_setting(Some(HyperlinkMode::Always));
        assert_eq!(hyperlink("a", "http://x/\x1b[0m"), "a");
        let once = hyperlink("a", "http://x/");
        assert_eq!(hyperlink(&once, "http://y/"), once);
    }

    #[test]
    fn url_normalization_accepts_http_only() {
        assert_eq!(
            normalize_http_url("www.example.com"),
            Some("https://www.example.com".to_string())
        );
        assert_eq!(
            normalize_http_url("http://example.com"),
            Some("http://example.com".to_string())
        );
        assert_eq!(normalize_http_url("ftp://example.com"), None);
        assert_eq!(normalize_http_url("not a url"), None);
    }

    #[test]
    fn strip_hyperlinks_keeps_display_text() {
        let linked = "\x1b]8;id=abc;https://x\x1b\\label\x1b]8;;\x1b\\";
        assert_eq!(strip_hyperlinks(linked), "label");
        assert_eq!(strip_hyperlinks("plain"), "plain");
        assert_eq!(strip_hyperlinks(&format!("a{linked}b")), "alabelb");
    }

    #[test]
    fn spans_report_display_cell_range() {
        let linked = "\x1b]8;id=abc;https://x\x1b\\file.rs\x1b]8;;\x1b\\";
        assert_eq!(
            hyperlink_spans(&format!("see {linked}")),
            vec![HyperlinkSpan {
                url: "https://x".to_string(),
                start: 4,
                end: 11,
            }]
        );
        assert_eq!(
            find_hyperlink_at(&format!("see {linked}"), 5),
            Some("https://x".to_string())
        );
        assert_eq!(find_hyperlink_at(&format!("see {linked}"), 3), None);
    }

    #[test]
    fn file_uri_encodes_paths() {
        let _file_link_guard = file_link_guard();
        assert_eq!(
            try_file_uri("/tmp/a b.rs", FilePosition::default(), true),
            Some("file:///tmp/a%20b.rs".to_string())
        );
        assert_eq!(try_file_uri("", FilePosition::default(), true), None);
    }

    #[test]
    fn vscode_uri_carries_position_suffix() {
        let _file_link_guard = file_link_guard();
        assert_eq!(
            file_uri_for_terminal(
                "/tmp/a.rs",
                FilePosition {
                    line: Some(4),
                    col: Some(2)
                },
                "vscode"
            ),
            Some("vscode://file/tmp/a.rs:4:2".to_string())
        );
        assert_eq!(
            file_uri_for_terminal("/tmp/a.rs", FilePosition::default(), "vscode"),
            Some("vscode://file/tmp/a.rs".to_string())
        );
    }

    #[test]
    fn file_link_setting_validates_scheme_and_host() {
        let _file_link_guard = file_link_guard();
        apply_file_link_setting(FileLinkConfig {
            scheme: Some("myapp".into()),
            host: Some("example.com".into()),
        });
        assert_eq!(
            try_file_uri("/tmp/a.rs", FilePosition::default(), false),
            Some("myapp://example.com/tmp/a.rs".to_string())
        );
        apply_file_link_setting(FileLinkConfig {
            scheme: Some("BAD SCHEME".into()),
            host: None,
        });
        assert_eq!(
            try_file_uri("/tmp/a.rs", FilePosition::default(), false),
            Some("file:///tmp/a.rs".to_string())
        );
        apply_file_link_setting(FileLinkConfig::default());
    }

    #[test]
    fn plain_text_drops_wrapper_and_styling() {
        assert_eq!(
            plain_hyperlink_text("\x1b]8;id=a;https://x\x1b\\\x1b[31mlink\x1b[0m\x1b]8;;\x1b\\"),
            "link"
        );
    }
}
