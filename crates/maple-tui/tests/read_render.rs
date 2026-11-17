//! Port of `l/test/read-render.test.ts`.
//!
//! The crate has no local-file read renderer (no resolved-path notices, no
//! markdown/code selection, no selector hyperlinks); only the URL read card in
//! `tools::fetch` is ported. The URL cases are real, including the shared
//! `sanitize_display_lines` behavior behind the ssh CRLF case. Local-file and
//! OSC-8 link cases stay ignored.

use maple_tui::component::Component;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};
use maple_tui::tools::fetch::{render_read_url_result, ReadUrlToolDetails};
use maple_tui::tools::native_view::ContentBlock;

fn theme() -> Theme {
	create_theme(builtin_theme_json("dark").expect("embedded dark theme"), CreateThemeOptions::default())
		.expect("dark theme resolves")
}

fn url_details(url: &str, final_url: &str) -> ReadUrlToolDetails {
	ReadUrlToolDetails {
		kind: Some("url".to_string()),
		url: url.to_string(),
		final_url: final_url.to_string(),
		content_type: "text/plain".to_string(),
		method: "fetch".to_string(),
		truncated: false,
		notes: Vec::new(),
		..ReadUrlToolDetails::default()
	}
}

fn render(component: &mut Box<dyn Component>, width: u16) -> String {
	component.render(width).join("\n")
}

#[test]
fn links_http_read_result_headers_to_the_final_url() {
	let theme = theme();
	let content = vec![ContentBlock::text("---\n\nhello")];
	let details = url_details("http://example.com/start", "http://example.com/final");
	let mut component = render_read_url_result(&content, Some(&details), false, false, &theme);
	let rendered = render(&mut component, 200);
	assert!(rendered.contains("example.com /final"), "{rendered:?}");
}

#[test]
fn sanitizes_url_read_errors() {
	let theme = theme();
	let content = vec![ContentBlock::text("fetch failed:\tconn reset\r\nby peer")];
	let mut component = render_read_url_result(&content, None, true, false, &theme);
	let raw = render(&mut component, 100);
	assert!(!raw.contains('\t'), "{raw:?}");
	assert!(!raw.contains('\r'), "{raw:?}");
	assert!(raw.contains("fetch failed:"), "{raw:?}");
}

#[test]
fn strips_crlf_and_expands_tabs_in_read_failure_output() {
	let theme = theme();
	let content = vec![ContentBlock::text(
		"Failed to start SSH master for can.internal: The fingerprint for the ED25519 key sent by the remote host is\r\nSHA256:abc\tdef\r\nAdd correct host key in /root/.ssh/known_hosts to get rid of this message.\r\nHost key verification failed.",
	)];
	let mut component = render_read_url_result(&content, None, true, false, &theme);
	let raw = render(&mut component, 100);
	assert!(!raw.contains('\t'), "{raw:?}");
	assert!(!raw.contains('\r'), "{raw:?}");
	assert!(raw.contains("SHA256:abc"), "{raw:?}");
	assert!(raw.contains("Host key verification failed."), "{raw:?}");
}

#[test]
fn expands_tabs_in_url_content_previews() {
	let theme = theme();
	let content = vec![ContentBlock::text("---\n\ncol1\tcol2\nrow2")];
	let details = url_details("http://example.com/start", "http://example.com/final");
	let mut component = render_read_url_result(&content, Some(&details), false, false, &theme);
	let raw = render(&mut component, 200);
	assert!(!raw.contains('\t'), "{raw:?}");
	assert!(!raw.contains('\r'), "{raw:?}");
	assert!(raw.contains("col1"), "{raw:?}");
}

#[test]
#[ignore = "missing facility: no local read renderer with resolved-path hyperlinks"]
fn links_local_style_read_titles_to_the_resolved_path() {}

#[test]
#[ignore = "missing facility: no local read renderer for call-path file URIs"]
fn links_absolute_read_call_paths_to_file_uris() {}

#[test]
#[ignore = "missing facility: no local read renderer selecting the markdown renderer"]
fn renders_text_markdown_details_through_the_markdown_renderer() {}

#[test]
#[ignore = "missing facility: no local read renderer choosing code vs markdown"]
fn keeps_untagged_markdown_source_in_the_code_renderer() {}

#[test]
#[ignore = "missing facility: no local read renderer honoring the raw selector"]
fn keeps_raw_markdown_selector_reads_in_the_code_renderer() {}

#[test]
#[ignore = "missing facility: no local read renderer with a resolved-path notice"]
fn shortens_the_home_directory_in_resolved_path_notices() {}

#[test]
#[ignore = "missing facility: no local read renderer for image detail lines"]
fn expands_tabs_in_image_detail_lines() {}
