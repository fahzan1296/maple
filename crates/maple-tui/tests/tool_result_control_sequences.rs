//! Port of `l/test/tool-result-control-sequences.test.ts` (portable subset).
//!
//! TS asserts live TUI rendering of tool result/details carrying raw file bytes
//! (DEC private modes, OSC 52 payloads) through grep/read/write component
//! rendering and hyperlink targets. Rust exposes escape stripping/sanitization
//! and style-to-span parsing (`native::spans::{plain_text, styled_spans,
//! plain_line}`) and terminal row sanitization
//! (`tools::terminal_output::{style_terminal_row, render_output}`) that must
//! keep these control sequences from becoming live display/control bytes. This
//! ports faithful contracts on those helpers.
//!
//! Deviations:
//! - No `ToolExecutionComponent`/`ReadToolGroupComponent` rendering, grouped
//!   read previews, URL/filename hyperlink `href` emission, or native view
//!   describe for read/write cards in these APIs; assertions target sanitized
//!   plain text and styled/row output instead of component frame strings.
//! - OSC 52 payload/link-target preservation in file hyperlinks is a
//!   component/URL concern not representable here; we assert DEC private-mode
//!   toggles and control bytes are stripped from visible/sanitized output.
//! - Theme-dependent styled spans are validated with an empty-escape input and
//!   plain-text parity; full ANSI->theme reverse mapping is covered elsewhere.

use maple_tui::native::spans::{plain_line, plain_text, styled_spans};
use maple_tui::theme::{builtin_theme_json, create_theme, CreateThemeOptions};
use maple_tui::tools::terminal_output::{render_output, style_terminal_row};

const FILE_ESCAPES: &str = "\x1b[?1049h\x1b[?1000h\x1b[?1003h\x1b[?1006h";

fn private_mode_sequences(text: &str) -> Vec<String> {
	let bytes = text.as_bytes();
	let mut out = Vec::new();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == 0x1b && i + 2 < bytes.len() && bytes[i + 1] == b'[' && bytes[i + 2] == b'?' {
			let mut j = i + 3;
			while j < bytes.len() {
				let b = bytes[j];
				if b == b'h' || b == b'l' {
					out.push(text[i..=j].to_string());
					j += 1;
					break;
				}
				if !(b.is_ascii_digit() || b == b';') {
					break;
				}
				j += 1;
			}
			i = j;
			continue;
		}
		i += 1;
	}
	out
}

#[test]
fn plain_text_and_line_strip_dec_private_modes_and_controls_from_file_payload() {
	let line = format!("needle {FILE_ESCAPES}");
	assert_eq!(plain_text(&line), "needle ");
	assert_eq!(plain_line(&line), "needle");
	assert!(private_mode_sequences(&plain_text(&line)).is_empty());

	let with_osc = "needle \x1b]52;c;clipboardpayload\x07 tail";
	let plain = plain_text(with_osc);
	assert!(plain.contains("needle"));
	assert!(plain.contains("tail"));
	assert!(!plain.contains("\x1b"));
	assert!(private_mode_sequences(&plain).is_empty());

	let theme_json = builtin_theme_json("dark").expect("dark theme");
	let theme = create_theme(theme_json, CreateThemeOptions::default()).expect("theme");
	let spans = styled_spans(&line, &theme);
	let joined: String = spans.iter().map(|s| s.t.as_str()).collect();
	assert!(joined.contains("needle"));
	assert!(!joined.contains("\x1b"));
	assert!(private_mode_sequences(&joined).is_empty());
}

#[test]
fn terminal_rows_drop_private_mode_escapes_and_control_bytes_but_keep_text() {
	let row = format!("needle {FILE_ESCAPES}");
	let styled = style_terminal_row(&row, "");
	assert!(styled.contains("needle"));
	assert!(private_mode_sequences(&styled).is_empty());

	let rendered = render_output(&[row, "ok\tline".to_string()], "");
	assert!(rendered.contains("needle"));
	assert!(rendered.contains("ok"));
	assert!(private_mode_sequences(&rendered).is_empty());
	assert!(!rendered.contains('\x1b'));

	// control-byte filename-like label text sanitizes for display
	let label = format!("fixture{FILE_ESCAPES}.txt");
	assert_eq!(plain_text(&label), "fixture.txt");
	assert_eq!(plain_line(&label), "fixture.txt");
}
