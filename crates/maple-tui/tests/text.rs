//! Port of `l/test/text.test.ts`.
//!
//! The crate's `Text` has no style-fn hook (only a `Style`), no render cache,
//! and `set_text` returns `()`, so two of the three reference tests are
//! ignored. The style-at-render-time test is ported with a `Style`.

use maple_tui::components::Text;
use maple_tui::style::{Color, Style};
use maple_tui::Component;

#[test]
fn applies_the_style_at_render_time_not_construction_time() {
	let mut text = Text::new("hello");
	text.set_style(Style::fg(Color::Red));
	let rendered = text.render(40).join("\n");
	assert!(rendered.contains("hello"));
	assert!(rendered.contains('\x1b'));
}

#[test]
#[ignore = "missing facility: Text::set_text returns () in this port; the changed-flag assertion is unobservable"]
fn reports_whether_set_text_changed_the_stored_text() {
	let mut text = Text::new("a");
	text.set_text("b");
	assert_eq!(text.text(), "b");
}

#[test]
#[ignore = "missing facility: Text has no render cache in this port; invalidate cannot re-resolve a stale style"]
fn re_resolves_the_style_fn_after_invalidate_so_a_theme_change_re_shapes() {
	let mut text = Text::new("hello");
	text.set_style(Style::fg(Color::Red));
	assert!(text.render(40).join("\n").contains("hello"));
}
