//! One-shot transcript block that prints a collab browser-join URL as a
//! scannable QR code, plus the scheme-less link helper.
//!
//! Reference: `l/src/chrome/collab-qrcode.ts`.
//!
//! Deviations:
//! - The reference reads the process-global `theme` through `fgOrPlain`; this
//!   port holds an explicit `&Theme` and passes it to [`collab_browser_link`].
//! - `setTranscriptAllocation` receives a `number` that may be `Infinity`; the
//!   Rust trait hands over `usize`, so the unbounded state is `None`.
//! - `describe` memoizes per `cx.dark`; the Rust port rebuilds the node (the
//!   output is identical).

use crate::chrome::qrcode::{render_qr_half_blocks, render_qr_text, QrCode};
use crate::chrome::transcript_container::{AnimationFrame, FinalizableBlock, TranscriptPresentationTarget};
use crate::component::Component;
use crate::native::describe::{code, col, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeText, Props, Span};
use crate::render::hyperlink::url_hyperlink_always;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::Theme;

/// The `fgOrPlain` equivalent: paint `text` with `color`, or leave it unstyled
/// when the theme has no real foreground for the token.
fn fg_or_plain(theme: &Theme, color: &str, text: &str) -> String {
	if theme.has_color(color) {
		theme.fg(color, text).unwrap_or_else(|_| text.to_string())
	} else {
		text.to_string()
	}
}

/// Drop a leading `http://`/`https://` (the reference's `/^https?:\/\//`).
fn strip_scheme(url: &str) -> &str {
	url.strip_prefix("https://")
		.or_else(|| url.strip_prefix("http://"))
		.unwrap_or(url)
}

/// Scheme-less display form of a collab browser deep link, OSC-8 linked.
pub fn collab_browser_link(theme: &Theme, web_link: &str, label: Option<&str>) -> String {
	let scheme_less = strip_scheme(web_link);
	let display = fg_or_plain(theme, "accent", &format!("\x1b[4m{}\x1b[24m", label.unwrap_or(scheme_less)));
	let linked = url_hyperlink_always(web_link, &display);
	// Without OSC-8 support, keep the literal URL at the visible prefix.
	if label.is_some() && linked == display {
		fg_or_plain(theme, "accent", &format!("\x1b[4m{scheme_less}\x1b[24m"))
	} else {
		linked
	}
}

/// One-shot transcript block that prints a collab browser-join URL as a
/// scannable QR code. The symbol is encoded once at construction (byte mode,
/// EC level M) and rendered as ANSI half-blocks; on terminals too narrow for
/// the symbol it degrades to a one-line hint that includes the URL.
///
/// The transcript viewport can also clip a live block to a single row under
/// pressure. The QR's first/last rows are a white quiet zone, so a 1-row clip
/// looks like an empty white line; when the allocated height cannot fit the
/// full symbol the block degrades to the same hint, which carries the URL
/// itself because the sibling heading may have collapsed too.
pub struct CollabQrCodeComponent<'a> {
	/// The collab browser-join URL.
	pub url: String,
	theme: &'a Theme,
	qr: QrCode,
	lines: Vec<String>,
	min_width: usize,
	allocated_rows: Option<usize>,
}

impl<'a> CollabQrCodeComponent<'a> {
	/// Encode `url` and render its half-block rows.
	pub fn new(url: &str, theme: &'a Theme) -> Result<Self, String> {
		let qr = QrCode::encode_text(url, crate::chrome::qrcode::QrEcLevel::M)?;
		let rows = render_qr_half_blocks(&qr, None);
		let min_width = rows.iter().map(|row| visible_width(row)).max().unwrap_or(0) + 1;
		let lines = rows.iter().map(|row| format!(" {row}")).collect();
		Ok(CollabQrCodeComponent {
			url: url.to_string(),
			theme,
			qr,
			lines,
			min_width,
			allocated_rows: None,
		})
	}

	/// The one-line fallback shown when the symbol cannot be drawn.
	fn hidden_hint(&self, reason: &str, width: u16) -> String {
		let link = collab_browser_link(self.theme, &self.url, Some("Join"));
		let warning = fg_or_plain(self.theme, "warning", &format!("QR code hidden: {reason}."));
		truncate_to_width(&format!("{link} {warning}"), width as usize, Ellipsis::Omit)
	}
}

impl Component for CollabQrCodeComponent<'_> {
	fn render(&mut self, width: u16) -> Vec<String> {
		if (width as usize) < self.min_width {
			return vec![self.hidden_hint(
				&format!("terminal width {width}; need {}", self.min_width),
				width,
			)];
		}
		if let Some(rows) = self.allocated_rows {
			if rows < self.lines.len() {
				return vec![self.hidden_hint(
					&format!("viewport height {rows}; need {}", self.lines.len()),
					width,
				)];
			}
		}
		self.lines.clone()
	}
}

impl TranscriptPresentationTarget for CollabQrCodeComponent<'_> {
	fn set_transcript_allocation(&mut self, rows: usize, _frame: AnimationFrame) {
		self.allocated_rows = Some(rows);
	}
}

impl FinalizableBlock for CollabQrCodeComponent<'_> {
	/// Survives emergency 1-row transcript pressure when this block is
	/// otherwise hidden.
	fn render_transcript_block_emergency_row(&self, width: u16) -> Option<String> {
		let reason = match self.allocated_rows {
			Some(rows) => format!("viewport height {rows}; need {}", self.lines.len()),
			None => "transcript pressure".to_string(),
		};
		Some(self.hidden_hint(&reason, width))
	}
}

impl NativeDescribe for CollabQrCodeComponent<'_> {
	/// The symbol as an unwrapped monospace block plus the join link. Glyphs
	/// are drawn in the terminal's foreground, so on a dark appearance the
	/// light modules are inked to keep the dark-on-light symbol scanners
	/// expect.
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		let qr = render_qr_text(&self.qr, cx.dark, None).join("\n");
		let mut link_props = Props::new();
		link_props.set_str("wrap", "none");
		let link = text(
			NodeText::Spans(vec![Span {
				t: strip_scheme(&self.url).to_string(),
				s: Some("accent link".to_string()),
				fx: None,
				href: Some(self.url.clone()),
			}]),
			link_props,
		);
		let mut code_props = Props::new();
		code_props.set_str("lang", "text");
		code_props.set_bool("wrap", false);
		let symbol = code(qr, code_props);
		let mut props = Props::new();
		props.set_str("role", "omp.collab.qrcode");
		col(vec![link, symbol], props)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::color::ColorMode;
	use crate::theme::{create_theme, CreateThemeOptions};

	const URL: &str = "https://example.com/collab/abc";

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin theme"),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.expect("theme resolves")
	}

	#[test]
	fn wide_terminal_shows_the_symbol_rows() {
		let theme = theme();
		let mut component = CollabQrCodeComponent::new(URL, &theme).expect("encodes");
		let lines = component.render(80);
		assert_eq!(lines.len(), (component.qr.size + 9) / 2);
		assert!(lines[0].starts_with(' '));
	}

	#[test]
	fn narrow_terminal_degrades_to_a_hint_carrying_the_url() {
		let theme = theme();
		let mut component = CollabQrCodeComponent::new(URL, &theme).expect("encodes");
		let lines = component.render(1);
		assert_eq!(lines.len(), 1);
		assert!(lines[0].contains("example.com/collab/abc"));
		assert!(lines[0].contains("QR code hidden"));
	}

	#[test]
	fn clipped_allocation_degrades_with_its_row_budget() {
		let theme = theme();
		let mut component = CollabQrCodeComponent::new(URL, &theme).expect("encodes");
		component.set_transcript_allocation(1, AnimationFrame::default());
		let lines = component.render(80);
		assert_eq!(lines.len(), 1);
		assert!(lines[0].contains("viewport height 1"));
	}

	#[test]
	fn emergency_row_reports_transcript_pressure_without_an_allocation() {
		let theme = theme();
		let component = CollabQrCodeComponent::new(URL, &theme).expect("encodes");
		let row = component.render_transcript_block_emergency_row(80).expect("emergency row");
		assert!(row.contains("transcript pressure"));
		assert!(row.contains("example.com/collab/abc"));
	}

	#[test]
	fn describe_builds_the_collab_qrcode_node() {
		let theme = theme();
		let component = CollabQrCodeComponent::new(URL, &theme).expect("encodes");
		let node = component.describe_native(&DescribeContext::new(80));
		assert_eq!(node.kind, crate::native::node::NodeKind::Col);
		assert_eq!(node.props.get_str("role"), Some("omp.collab.qrcode"));
		assert_eq!(node.children.len(), 2);
		assert_eq!(node.children[0].kind, crate::native::node::NodeKind::Text);
		assert_eq!(node.children[1].kind, crate::native::node::NodeKind::Code);
	}

	#[test]
	fn browser_link_strips_the_scheme_and_underlines_the_label() {
		let theme = theme();
		let link = collab_browser_link(&theme, URL, Some("Join"));
		assert!(!link.contains("https://"));
		assert!(link.contains("Join"));
	}

	#[test]
	fn construction_rejects_a_url_that_cannot_fit() {
		let theme = theme();
		let huge = format!("https://example.com/{}", "a".repeat(4000));
		assert!(CollabQrCodeComponent::new(&huge, &theme).is_err());
	}
}
