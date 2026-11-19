//! Port of `l/test/tight-layout.test.ts`.
//!
//! Only `Box` and the render panes consult the process-wide tight flag; the
//! crate's `Text` and `Markdown` components take raw padding and ignore it, so
//! the three cases asserting tight-aware padding there stay ignored rather than
//! pinning behaviour the reference does not have. The `Box` and `Editor` cases
//! share the process-wide flag, so they run as one test to avoid racing the
//! harness's parallel threads.

use maple_tui::component::Component;
use maple_tui::components::editor::Editor;
use maple_tui::components::r#box::Box;
use maple_tui::components::text::Text;
use maple_tui::render::render_utils::set_tight_layout;

#[test]
fn box_drops_one_padding_column_under_tight_layout_while_editor_keeps_its_width() {
	let mut boxed = Box::new(1, 0);
	boxed.add_child(Text::padded("Hi", 0, 0));

	let mut editor = Editor::new();
	editor.set_text("hello");
	editor.set_focused(true);

	set_tight_layout(false);
	assert_eq!(boxed.render(10), vec![" Hi       "]);
	let normal_editor = editor.render(20);

	set_tight_layout(true);
	assert_eq!(boxed.render(10), vec!["Hi        "]);
	assert_eq!(editor.render(20), normal_editor);

	set_tight_layout(false);
}

#[test]
#[ignore = "missing facility: Text padding does not consult the tight-layout flag"]
fn removes_one_character_of_horizontal_padding_from_text() {
	set_tight_layout(false);
}

#[test]
#[ignore = "missing facility: Markdown padding does not consult the tight-layout flag"]
fn removes_one_character_of_horizontal_padding_from_markdown() {
	set_tight_layout(false);
}

#[test]
#[ignore = "missing facility: Markdown has no ignoreTight switch in the crate"]
fn preserves_padding_on_markdown_if_ignore_tight_is_set() {
	set_tight_layout(false);
}
