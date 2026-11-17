//! Port of `l/test/container-dispose.test.ts`.
//!
//! The crate's `Container` does not override `Component::dispose` (the trait
//! default is a no-op) and has no `dispose_children`, so the reference's
//! dispose-ordering and detachment assertions have no facility to exercise.

use maple_tui::components::Container;

#[test]
#[ignore = "missing facility: Container::dispose is the no-op trait default and dispose_children is not ported"]
fn calls_dispose_on_each_child_and_tolerates_children_without_dispose() {
	let container = Container::new();
	let _ = container.children().len();
}

#[test]
#[ignore = "missing facility: Container::dispose does not recurse in this port"]
fn recurses_through_nested_containers() {
	let outer = Container::new();
	let _ = outer.children().len();
}

#[test]
#[ignore = "missing facility: dispose_children is not ported to Container"]
fn dispose_children_disposes_children_and_detaches_them() {
	let container = Container::new();
	assert!(container.children().is_empty());
}
