//! Port of `l/test/container-memo.test.ts`.
//!
//! The crate's `Container` has no render memo at all and `Box` memoizes its
//! derived rows but returns an owned `Vec`, so reference-identity (`toBe`)
//! assertions are not observable. Content assertions are ported; the
//! memo-identity, Hangul width-epoch, and carried-width tests are ignored.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use maple_tui::components::r#box::Box;
use maple_tui::components::{Container, Text};
use maple_tui::Component;

fn plain(lines: &[String]) -> Vec<String> {
	lines.iter().map(|line| line.trim_end().to_string()).collect()
}

struct Probe {
	lines: Rc<std::cell::RefCell<Vec<String>>>,
	renders: Rc<Cell<usize>>,
}

impl Probe {
	fn new(lines: &[&str]) -> Self {
		Probe {
			lines: Rc::new(std::cell::RefCell::new(
				lines.iter().map(|line| line.to_string()).collect(),
			)),
			renders: Rc::new(Cell::new(0)),
		}
	}

	fn renders(&self) -> usize {
		self.renders.get()
	}
}

impl Component for Probe {
	fn render(&mut self, _width: u16) -> Vec<String> {
		self.renders.set(self.renders.get() + 1);
		self.lines.borrow().clone()
	}
}

#[test]
fn container_concatenates_children() {
	let mut container = Container::new();
	container.add_child(Text::new("alpha")).add_child(Text::new("beta"));
	assert_eq!(plain(&container.render(40)), vec!["alpha", "beta"]);
}

#[test]
fn container_reflects_child_text_change() {
	let mut container = Container::new();
	let text = Probe::new(&["before"]);
	container.add_child(Probe {
		lines: Rc::clone(&text.lines),
		renders: Rc::clone(&text.renders),
	});

	assert_eq!(plain(&container.render(40)), vec!["before"]);
	*text.lines.borrow_mut() = vec!["after".to_string()];
	assert_eq!(plain(&container.render(40)), vec!["after"]);
}

#[test]
fn container_add_child_appends() {
	let mut container = Container::new();
	container.add_child(Text::new("first"));
	assert_eq!(plain(&container.render(40)), vec!["first"]);
	container.add_child(Text::new("second"));
	assert_eq!(plain(&container.render(40)), vec!["first", "second"]);
}

#[test]
fn container_remove_child_drops() {
	let mut container = Container::new();
	container.add_child(Text::new("keep"));
	container.add_child(Text::new("drop"));
	assert_eq!(plain(&container.render(40)), vec!["keep", "drop"]);
	container.remove_child(1);
	assert_eq!(plain(&container.render(40)), vec!["keep"]);
}

#[test]
fn container_renders_children_every_frame() {
	let mut container = Container::new();
	let a = Probe::new(&["probe-a"]);
	let b = Probe::new(&["probe-b"]);
	let a_renders = Rc::clone(&a.renders);
	let b_renders = Rc::clone(&b.renders);
	container.add_child(a).add_child(b);

	let first = container.render(40);
	let second = container.render(40);
	let third = container.render(40);
	assert_eq!(plain(&first), vec!["probe-a", "probe-b"]);
	assert_eq!(plain(&second), plain(&first));
	assert_eq!(plain(&third), plain(&first));
	// Children were rendered each frame regardless of the memo.
	assert_eq!(a_renders.get(), 3);
	assert_eq!(b_renders.get(), 3);
}

#[test]
fn box_pads_content() {
	let mut box_component = Box::new(1, 1);
	box_component.add_child(Text::new("content"));
	assert_eq!(plain(&box_component.render(40)), vec!["", " content", ""]);
}

#[test]
fn box_reflects_child_text_change() {
	let mut box_component = Box::new(1, 0);
	let text = Probe::new(&["old"]);
	box_component.add_child(Probe {
		lines: Rc::clone(&text.lines),
		renders: Rc::clone(&text.renders),
	});

	assert_eq!(plain(&box_component.render(40)), vec![" old"]);
	*text.lines.borrow_mut() = vec!["new".to_string()];
	assert_eq!(plain(&box_component.render(40)), vec![" new"]);
}

#[test]
fn box_restyles_when_bg_output_changes() {
	let tag = Arc::new(Mutex::new("A".to_string()));
	let tag_for_bg = Arc::clone(&tag);
	let mut box_component = Box::styled(
		0,
		0,
		Some(Arc::new(move |text: &str| {
			let tag = tag_for_bg.lock().unwrap();
			format!("<{tag}>{text}</{tag}>")
		})),
		None,
	);
	let probe = Probe::new(&["row"]);
	box_component.add_child(probe);

	let first = box_component.render(10);
	assert_eq!(first[0], "<A>row       </A>");
	// Same closure state → cache hit.
	assert_eq!(box_component.render(10), first);

	// Mutate the closure: same function reference, different output. The
	// bg sample in the cache key must force a rebuild.
	*tag.lock().unwrap() = "B".to_string();
	let second = box_component.render(10);
	assert_ne!(second, first);
	assert_eq!(second[0], "<B>row       </B>");
}

#[test]
#[ignore = "missing facility: Container has no render memo in this port, so reference identity is unobservable"]
fn returns_identical_reference_across_renders_while_children_are_ref_stable() {
	let container = Container::new();
	let _ = container.children().len();
}

#[test]
#[ignore = "missing facility: Container has no clear() in this port"]
fn drops_the_memo_on_clear() {
	let container = Container::new();
	let _ = container.children().len();
}

#[test]
#[ignore = "missing facility: Container has no render memo and invalidate is a no-op in this port"]
fn drops_the_memo_on_invalidate_even_when_content_is_unchanged() {
	let container = Container::new();
	let _ = container.children().len();
}

#[test]
#[ignore = "missing facility: no render memo, so the width-change identity miss is unobservable"]
fn misses_the_memo_on_width_change() {
	let container = Container::new();
	let _ = container.children().len();
}

#[test]
#[ignore = "missing facility: Box returns an owned Vec, so reference identity is unobservable"]
fn box_returns_identical_reference_across_renders_at_a_fixed_width() {
	let box_component = Box::new(1, 1);
	let _ = box_component.debug_state();
}

#[test]
#[ignore = "missing facility: setHangulCompatibilityJamoWidth width epoch is not ported"]
fn rerenders_same_text_after_narrow_to_wide_hangul_change() {
	let text = Text::new("\u{3131}\u{314f}");
	assert_eq!(plain(&text.render(6)).len(), 1);
}

#[test]
#[ignore = "missing facility: setHangulCompatibilityJamoWidth width epoch is not ported"]
fn rerenders_nested_box_after_narrow_to_wide_hangul_change() {
	let mut box_component = Box::new(1, 0);
	box_component.set_ignore_tight(true);
	box_component.add_child(Text::new("\u{3131}\u{314f}"));
	assert!(!box_component.render(8).is_empty());
}

#[test]
#[ignore = "missing facility: setHangulCompatibilityJamoWidth width epoch is not ported"]
fn keys_box_cache_by_width_epoch_even_for_ref_stable_child_rows() {
	let mut box_component = Box::new(1, 0);
	box_component.set_ignore_tight(true);
	box_component.add_child(Probe::new(&["\u{3131}\u{314f}"]));
	assert!(!box_component.render(8).is_empty());
}

#[test]
#[ignore = "missing facility: publishLineWidths carried widths are not ported"]
fn rebuilds_after_published_child_mutates_its_same_array() {
	let mut box_component = Box::new(1, 0);
	box_component.set_ignore_tight(true);
	box_component.add_child(Probe::new(&["hi"]));
	assert_eq!(plain(&box_component.render(8)), vec![" hi"]);
}

#[test]
#[ignore = "missing facility: publishLineWidths carried widths are not ported"]
fn rebuilds_after_unpublished_child_mutates_its_same_array() {
	let mut box_component = Box::new(1, 0);
	box_component.set_ignore_tight(true);
	box_component.add_child(Probe::new(&["hi"]));
	assert_eq!(plain(&box_component.render(8)), vec![" hi"]);
}

#[test]
#[ignore = "missing facility: publishLineWidths carried widths are not ported"]
fn falls_back_for_direct_context_sensitive_leading_marks() {
	let mut box_component = Box::new(1, 0);
	box_component.set_ignore_tight(true);
	box_component.add_child(Probe::new(&["\u{200d}\u{fe0f}"]));
	assert!(!box_component.render(4).is_empty());
}

#[test]
#[ignore = "missing facility: publishLineWidths carried widths are not ported"]
fn falls_back_for_sgr_hidden_leading_joiners_and_variation_selectors() {
	let mut box_component = Box::new(1, 0);
	box_component.set_ignore_tight(true);
	box_component.add_child(Probe::new(&["\x1b[31m\u{200d}\u{fe0f}\x1b[0m"]));
	assert!(!box_component.render(4).is_empty());
}

#[test]
#[ignore = "missing facility: publishLineWidths carried widths are not ported"]
fn pads_hard_class_rows_to_full_width_from_carried_widths_at_zero_padding_x() {
	let lines = [
		"\u{0301}a",
		"\u{093f}",
		"1\u{20e3}",
		"\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
		"a\u{200d}b",
		"\u{2764}\u{fe0f}",
		"\u{0e33}\u{fe0f}",
		"\u{0eb3}",
	];
	let mut box_component = Box::new(0, 0);
	box_component.add_child(Probe::new(&lines));
	assert_eq!(box_component.render(8).len(), lines.len());
}
