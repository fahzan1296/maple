//! Fetch/URL-read tool render-model types and selector helpers.
//!
//! Reference: `l/src/tools/fetch.ts`.
//!
//! Deviations:
//! - TS renders TUI components/native views (themed status/cards, hyperlinks,
//!   previews). Rust ports URL read details, selector peeling helpers, and
//!   plain label/description derivation.
//! - Full URL parsing/validation edge cases are approximated; `read` module
//!   selector parsing (`read_selector_range_start`, `is_readable_url_path`) is
//!   not imported because `read.rs` is ported in the same slice — local
//!   selector recognition is provided.

use crate::component::Component;
use crate::components::text::Text;
use crate::native::describe::{span, text};
use crate::native::node::{NodeText, Props, Tone};
use crate::render::render_utils::{
	format_more_items, pluralize, sanitize_display_lines, ToolUiStatus, PREVIEW_LIMITS,
};
use crate::render::tool_card::{
	framed_tool_card, render_status_line, StatusLine, ToolCardOptions, ToolCardPhase,
	ToolCardSection, ToolCardSnapshot,
};
use crate::theme::theme_class::Theme;
use crate::tools::list_limit::{apply_list_limit, ListLimitOptions};
use crate::tools::native_view::{
	error_text, footnote_text, note_text, stats_text, ContentBlock, NativeToolHead, NativeToolView,
};
use crate::tools::output_meta::OutputMeta;

/// Display metadata for fetch/URL read tool results.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadUrlToolDetails {
	pub kind: Option<String>, // "url"
	pub url: String,
	pub final_url: String,
	pub content_type: String,
	pub method: String,
	pub raw: bool,
	pub truncated: bool,
	pub notes: Vec<String>,
	/// Truncation/diagnostic metadata.
	pub meta: Option<OutputMeta>,
}

/// Restore the double slash in a collapsed HTTP URL scheme.
pub fn repair_collapsed_scheme(value: &str) -> String {
	let lower = value.to_ascii_lowercase();
	for scheme in ["https:/", "http:/"] {
		if lower.starts_with(scheme) && !lower.starts_with(&format!("{scheme}/")) {
			let prefix_len = scheme.len(); // includes single slash
			let name_len = scheme.find(':').unwrap_or(4);
			let name = &value[..name_len];
			return format!("{name}://{}", &value[prefix_len..]);
		}
	}
	value.to_string()
}

fn looks_like_url(s: &str) -> bool {
	let s = s.trim();
	s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.")
}

/// Recognize a simple range/tail selector token (permissive).
pub fn is_url_selector_token(token: &str) -> bool {
	let t = token.to_ascii_lowercase();
	if t == "raw" {
		return true;
	}
	// tail -N
	if let Some(rest) = token.strip_prefix('-') {
		if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
			return rest.parse::<u64>().ok().filter(|&n| n > 0).is_some();
		}
	}
	// ranges like 1-2, 1-, - (approximated), 1+2, bare number
	if token.is_empty() {
		return false;
	}
	token.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '+')
		&& token.chars().any(|c| c.is_ascii_digit())
}

/// Peel selector tokens off the right of a URL string.
///
/// Returns `(base_path, selectors_left_to_right)` or `None`.
pub fn try_extract_embedded_url_selector(read_path: &str) -> Option<(String, Vec<String>)> {
	let mut base = read_path.to_string();
	let mut sels: Vec<String> = Vec::new();
	loop {
		let Some(colon) = base.rfind(':') else {
			break;
		};
		if colon == 0 {
			break;
		}
		let candidate = base[colon + 1..].to_string();
		let remainder = base[..colon].to_string();
		if !looks_like_url(&remainder) {
			break;
		}
		if !is_url_selector_token(&candidate) {
			break;
		}
		sels.push(candidate);
		base = remainder;
	}
	if sels.is_empty() {
		return None;
	}
	sels.reverse();
	Some((base, sels))
}

/// Count non-empty lines.
pub fn count_non_empty_lines(text: &str) -> usize {
	text.lines().filter(|l| !l.trim().is_empty()).count()
}

/// Get domain-ish label from URL string.
pub fn get_domain(url: &str) -> String {
	let s = repair_collapsed_scheme(url.trim());
	let without = s
		.strip_prefix("https://")
		.or_else(|| s.strip_prefix("http://"))
		.unwrap_or(&s);
	without.split('/').next().unwrap_or("").split(':').next().unwrap_or("").to_string()
}

/// Resolve link target for a URL read input.
pub fn read_url_link_target(input: &str) -> String {
	let repaired = repair_collapsed_scheme(input);
	if let Some((path, sels)) = try_extract_embedded_url_selector(&repaired) {
		let non_raw = sels.iter().filter(|s| s.to_ascii_lowercase() != "raw").count();
		if non_raw > 1 {
			return input.to_string();
		}
		return path;
	}
	repaired
}

/// Plain URL description label `domain /path`.
pub fn format_read_url_description_plain(input: &str) -> String {
	let target = read_url_link_target(input);
	let display = if target.to_ascii_lowercase().starts_with("www.") {
		format!("https://{target}")
	} else {
		target.clone()
	};
	let domain = get_domain(&display);
	let url_path = display
		.find("://")
		.and_then(|i| display[i + 3..].find('/').map(|j| &display[i + 3 + j..]))
		.unwrap_or("");
	let url_path = if url_path.chars().count() > 50 {
		let mut t: String = url_path.chars().take(49).collect();
		t.push('…');
		t
	} else {
		url_path.to_string()
	};
	let label = if url_path.is_empty() {
		domain
	} else {
		format!("{domain} {url_path}")
	};
	label.trim().to_string()
}

/// Text of the first content block (the reference reads `content[0].text`).
fn first_text(content: &[ContentBlock]) -> String {
	content.first().and_then(|block| block.text.clone()).unwrap_or_default()
}

/// Result body: the content after the reference's `---\n\n` header split.
fn content_body(content_text: &str) -> String {
	if content_text.contains("---\n\n") {
		let mut parts: Vec<&str> = content_text.split("---\n\n").collect();
		parts.remove(0);
		parts.join("---\n\n")
	} else {
		content_text.to_string()
	}
}

/// Native head of a URL read: `Read` plus the linked, one-line URL.
fn read_url_head(input: &str, meta: &[String]) -> NativeToolHead {
	let target = read_url_link_target(input);
	let label = if target.trim().is_empty() {
		String::new()
	} else {
		format_read_url_description_plain(&target)
	};
	NativeToolHead {
		title: Some(NodeText::Plain("Read".to_string())),
		target: if label.is_empty() {
			None
		} else {
			Some(NodeText::Plain(label))
		},
		target_kind: Some("text".to_string()),
		href: if target.trim().is_empty() {
			None
		} else {
			Some(target)
		},
		meta: if meta.is_empty() {
			None
		} else {
			Some(meta.iter().cloned().map(NodeText::Plain).collect())
		},
		..NativeToolHead::default()
	}
}

/// Native view of a URL read call: the head only, inline while pending.
pub fn describe_read_url_call(path: Option<&str>, url: Option<&str>, raw: bool) -> NativeToolView {
	let input = path.or(url).unwrap_or("");
	let meta: Vec<String> = if raw { vec!["raw".to_string()] } else { Vec::new() };
	NativeToolView {
		tool: Some(read_url_head(input, &meta)),
		inline: Some(true),
		..NativeToolView::default()
	}
}

/// Native view of a URL read result: a bounded content preview over one quiet
/// stats line.
pub fn describe_read_url_result(
	content: &[ContentBlock],
	details: Option<&ReadUrlToolDetails>,
	is_error: bool,
) -> NativeToolView {
	let Some(details) = details.filter(|_| !is_error) else {
		let url_text = match details {
			Some(details) if !details.final_url.is_empty() => details.final_url.clone(),
			Some(details) => details.url.clone(),
			None => String::new(),
		};
		let message = first_text(content);
		let message = message.trim();
		let message = message.strip_prefix("Error:").unwrap_or(message).trim();
		let message = if message.is_empty() { "Read failed" } else { message };
		return NativeToolView {
			tool: Some(read_url_head(&url_text, &[])),
			tone: Some(Tone::Error),
			body: Some(vec![error_text(message)]),
			..NativeToolView::default()
		};
	};

	let truncation = details.meta.as_ref().and_then(|meta| meta.truncation.as_ref());
	let truncated = details.truncated || truncation.is_some();
	let body_text = content_body(&first_text(content));
	let lines: Vec<&str> = body_text.lines().filter(|line| !line.trim().is_empty()).collect();
	let shown: Vec<String> = lines
		.iter()
		.take(PREVIEW_LIMITS.EXPANDED_LINES)
		.map(|line| line.trim_end().to_string())
		.collect();
	let hidden = lines.len().saturating_sub(shown.len());
	let redirected = details.url != details.final_url;

	let line_count = lines.len();
	let mut stats = vec![
		if details.content_type.is_empty() {
			"unknown".to_string()
		} else {
			details.content_type.clone()
		},
		details.method.clone(),
		format!("{line_count} {}", pluralize("line", line_count)),
		format!("{} chars", body_text.trim().chars().count()),
	];
	if hidden > 0 {
		stats.push(format_more_items(hidden, "line"));
	}
	stats.extend(details.notes.iter().cloned());

	let mut preview = Vec::new();
	if shown.is_empty() {
		preview.push(note_text("(no content)", "dim", None));
	} else {
		let mut props = Props::new();
		props.set_str("wrap", "word");
		preview.push(text(NodeText::Spans(vec![span(shown.join("\n"), Some("dim"))]), props));
	}
	preview.extend(stats_text(&stats));
	if truncated {
		preview.extend(footnote_text(&[], details.meta.as_ref()));
	}
	let mut head_meta = Vec::new();
	if redirected {
		head_meta.push(format!("from {}", get_domain(&details.url)));
	}
	NativeToolView {
		tool: Some(read_url_head(&details.final_url, &head_meta)),
		tone: if truncated { Some(Tone::Warning) } else { None },
		body: Some(preview),
		..NativeToolView::default()
	}
}

fn theme_fg(theme: &Theme, token: &str, value: &str) -> String {
	theme.fg(token, value).unwrap_or_else(|_| value.to_string())
}

/// Render the URL read call as a pending status line.
pub fn render_read_url_call(
	path: Option<&str>,
	url: Option<&str>,
	raw: bool,
	theme: &Theme,
) -> Box<dyn Component> {
	let input = path.or(url).unwrap_or("");
	let mut line = StatusLine::new("Read").with_icon(ToolUiStatus::Pending);
	line.description = Some(format_read_url_description_plain(input));
	if raw {
		line.meta.push("raw".to_string());
	}
	Box::new(Text::new(render_status_line(&line, theme)))
}

/// Render the URL read result as a framed card: metadata over a bounded content
/// preview.
pub fn render_read_url_result(
	content: &[ContentBlock],
	details: Option<&ReadUrlToolDetails>,
	is_error: bool,
	expanded: bool,
	theme: &Theme,
) -> Box<dyn Component> {
	let Some(details) = details.filter(|_| !is_error) else {
		let url_text = match details {
			Some(details) if !details.final_url.is_empty() => details.final_url.clone(),
			Some(details) => details.url.clone(),
			None => String::new(),
		};
		let mut line = StatusLine::new("Read").with_icon(ToolUiStatus::Error);
		if !url_text.is_empty() {
			line.description = Some(format_read_url_description_plain(&url_text));
		}
		let message = first_text(content);
		let message = message.trim();
		let message = message.strip_prefix("Error:").unwrap_or(message).trim();
		let message = if message.is_empty() { "Read failed" } else { message };
		let error_lines: Vec<String> = sanitize_display_lines(message)
			.into_iter()
			.map(|line| theme_fg(theme, "error", &line))
			.collect();
		let header = render_status_line(&line, theme);
		let card_theme = theme.clone();
		return framed_tool_card(
			card_theme,
			move |_ctx| ToolCardSnapshot {
				header: Some(header.clone()),
				phase: Some(ToolCardPhase::Error),
				sections: vec![ToolCardSection::new(error_lines.clone().into())],
				..ToolCardSnapshot::default()
			},
			ToolCardOptions::default(),
		);
	};

	let truncation = details.meta.as_ref().and_then(|meta| meta.truncation.as_ref());
	let truncated = details.truncated || truncation.is_some();
	let redirected = details.url != details.final_url;
	let mut line = StatusLine::new("Read").with_icon(if truncated {
		ToolUiStatus::Warning
	} else {
		ToolUiStatus::Success
	});
	line.description = Some(format_read_url_description_plain(&details.final_url));
	let header = render_status_line(&line, theme);

	let body_text = content_body(&first_text(content));
	let line_count = count_non_empty_lines(&body_text);
	let char_count = body_text.trim().chars().count();
	let content_lines: Vec<String> = body_text.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();

	let mut metadata_lines = vec![
		format!("{} {}", theme_fg(theme, "muted", "Content-Type:"), if details.content_type.is_empty() { "unknown" } else { &details.content_type }),
		format!("{} {}", theme_fg(theme, "muted", "Method:"), details.method),
	];
	if redirected {
		metadata_lines.push(format!(
			"{} {}",
			theme_fg(theme, "muted", "Final URL:"),
			theme_fg(theme, "mdLinkUrl", &details.final_url)
		));
	}
	metadata_lines.push(format!(
		"{} {line_count} {}",
		theme_fg(theme, "muted", "Lines:"),
		pluralize("line", line_count)
	));
	metadata_lines.push(format!("{} {char_count}", theme_fg(theme, "muted", "Chars:")));
	if truncated {
		metadata_lines.push(theme_fg(theme, "warning", "Output truncated"));
	}
	if !details.notes.is_empty() {
		metadata_lines.push(format!(
			"{} {}",
			theme_fg(theme, "muted", "Notes:"),
			details.notes.join("; ")
		));
	}

	let preview_limit = if expanded {
		PREVIEW_LIMITS.EXPANDED_LINES
	} else {
		PREVIEW_LIMITS.COLLAPSED_LINES
	};
	let total_lines = content_lines.len();
	let limited = apply_list_limit(
		content_lines,
		ListLimitOptions {
			head_limit: Some(preview_limit),
			..ListLimitOptions::default()
		},
	);
	let mut preview_lines: Vec<String> = limited
		.items
		.iter()
		.flat_map(|line| sanitize_display_lines(line))
		.map(|line| theme_fg(theme, "dim", line.trim_end()))
		.collect();
	if preview_lines.is_empty() {
		preview_lines.push(theme_fg(theme, "dim", "(no content)"));
	}
	let remaining = total_lines.saturating_sub(limited.items.len());
	if remaining > 0 {
		let hint = crate::render::render_utils::format_expand_hint(theme, Some(expanded), Some(true));
		let more = crate::render::render_utils::format_more_items(remaining, "line");
		preview_lines.push(theme_fg(theme, "muted", &if hint.is_empty() { more } else { format!("{more} {hint}") }));
	}

	let phase = if truncated {
		ToolCardPhase::Warning
	} else {
		ToolCardPhase::Success
	};
	let card_theme = theme.clone();
	let title = theme_fg(theme, "toolTitle", "Metadata");
	let preview_title = theme_fg(theme, "toolTitle", "Content Preview");
	framed_tool_card(
		card_theme,
		move |_ctx| ToolCardSnapshot {
			header: Some(header.clone()),
			phase: Some(phase),
			sections: vec![
				ToolCardSection::labeled(title.clone(), metadata_lines.clone().into()),
				ToolCardSection::labeled(preview_title.clone(), preview_lines.clone().into()),
			],
			apply_bg: Some(false),
			..ToolCardSnapshot::default()
		},
		ToolCardOptions::default(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn repair_scheme() {
		assert_eq!(repair_collapsed_scheme("https:/example.com"), "https://example.com");
		assert_eq!(repair_collapsed_scheme("https://x"), "https://x");
	}

	#[test]
	fn extract_selector() {
		let (base, sels) = try_extract_embedded_url_selector("https://a.com/x:1-2:raw").unwrap();
		assert_eq!(base, "https://a.com/x");
		assert_eq!(sels, vec!["1-2", "raw"]);
		assert!(try_extract_embedded_url_selector("https://a.com/x").is_none());
	}

	#[test]
	fn domain_and_desc() {
		assert_eq!(get_domain("https://www.example.com/a"), "www.example.com");
		let d = format_read_url_description_plain("https://example.com/path");
		assert!(d.contains("example.com"));
		assert!(d.contains("/path"));
	}

	#[test]
	fn selector_token() {
		assert!(is_url_selector_token("raw"));
		assert!(is_url_selector_token("1-5"));
		assert!(is_url_selector_token("-10"));
		assert!(!is_url_selector_token("nope"));
	}

	fn details() -> ReadUrlToolDetails {
		ReadUrlToolDetails {
			kind: Some("url".to_string()),
			url: "https://example.com/a".to_string(),
			final_url: "https://example.com/b".to_string(),
			content_type: "text/html".to_string(),
			method: "GET".to_string(),
			truncated: false,
			notes: vec!["redirected".to_string()],
			..ReadUrlToolDetails::default()
		}
	}

	#[test]
	fn describe_call_links_the_target() {
		let view = describe_read_url_call(Some("https://example.com/a:1-5"), None, true);
		let head = view.tool.expect("head");
		assert_eq!(head.title, Some(NodeText::Plain("Read".to_string())));
		assert_eq!(head.href.as_deref(), Some("https://example.com/a"));
		assert_eq!(head.meta.expect("meta").len(), 1);
	}

	#[test]
	fn describe_result_has_preview_and_stats() {
		let content = vec![ContentBlock::text("---\n\n# Title\nbody line\n")];
		let details = details();
		let view = describe_read_url_result(&content, Some(&details), false);
		let body = view.body.expect("body");
		assert_eq!(body.len(), 2);
		let head = view.tool.expect("head");
		assert!(head.target.is_some());
	}

	#[test]
	fn describe_error_uses_error_tone() {
		let content = vec![ContentBlock::text("Error: boom")];
		let view = describe_read_url_result(&content, None, true);
		assert_eq!(view.tone, Some(Tone::Error));
	}

	#[test]
	fn render_call_shows_url_and_raw_flag() {
		let theme = crate::render::render_utils::tests::sample_theme();
		let mut component = render_read_url_call(None, Some("https://example.com/a"), true, &theme);
		let rows = component.render(120);
		let line = rows.join("\n");
		assert!(line.contains("Read"));
		assert!(line.contains("example.com"));
		assert!(line.contains("raw"));
	}

	#[test]
	fn render_result_card_lists_metadata() {
		let theme = crate::render::render_utils::tests::sample_theme();
		let content = vec![ContentBlock::text("# Title\nbody line\n")];
		let details = details();
		let mut component = render_read_url_result(&content, Some(&details), false, false, &theme);
		let rows = component.render(120).join("\n");
		assert!(rows.contains("Metadata"));
		assert!(rows.contains("text/html"));
		assert!(rows.contains("example.com"));
	}
}
