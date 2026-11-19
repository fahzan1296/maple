//! Port of `l/test/review-overlays-native.test.ts`.
//!
//! Reference deviations:
//! - The constructor is injected (`theme`, `bindings`) and the cancel key is
//!   `Esc`; the reference remaps `tui.select.cancel` to `escape`/`ctrl+g`.
//! - A native Contents selection scrubs the body and moves the ToC cursor but
//!   does not focus the region, so the test presses `Tab` before `a`.
//! - The native body renders section markdown only; annotation callouts are not
//!   described, so a note is observed through the ToC `detail` marker.
//! - Committing an option keeps the options list and disables the confirm
//!   button; there is no `spinner` node.
//! - The three code-review cases target a native event surface the crate does
//!   not have: `AnnotationOverlayComponent` exposes no `handle_native_event`
//!   and `describe_native` returns `rows` (no `lines` list, no `actions/paste`
//!   button). Those remain covered at the component level by
//!   `tests/annotation_overlay.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keybindings::KeybindingsManager;
use maple_tui::keys::Key;
use maple_tui::native::backend::NativeUiEvent;
use maple_tui::native::node::{DescribeContext, NativeNode, NodeKind};
use maple_tui::overlays::plan_review_overlay::{
	PlanReviewFocus, PlanReviewOverlay, PlanReviewOverlayCallbacks, PlanReviewOverlayOptions,
};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{CreateThemeOptions, Theme, create_theme};

const PLAN: &str = "# Plan\n\n## Alpha\n\nfirst\n\n## Beta\n\nsecond\n\n## Gamma\n\nthird\n";

#[derive(Default)]
struct Rec {
	picked: Rc<RefCell<Vec<String>>>,
	feedback: Rc<RefCell<Vec<String>>>,
}

fn record() -> (Rec, PlanReviewOverlayCallbacks) {
	let rec = Rec::default();
	let picked = Rc::clone(&rec.picked);
	let feedback = Rc::clone(&rec.feedback);
	let callbacks = PlanReviewOverlayCallbacks {
		on_pick: Some(Box::new(move |label| picked.borrow_mut().push(label.to_string()))),
		on_feedback_change: Some(Box::new(move |text| feedback.borrow_mut().push(text.to_string()))),
		..Default::default()
	};
	(rec, callbacks)
}

fn theme() -> Theme {
	create_theme(
		builtin_theme_json("dark").expect("embedded dark theme"),
		CreateThemeOptions::default(),
	)
	.expect("dark theme builds")
}

fn options(disabled: &[usize]) -> PlanReviewOverlayOptions {
	PlanReviewOverlayOptions {
		options: vec!["Approve".to_string(), "Refine plan".to_string()],
		disabled_indices: disabled.to_vec(),
		..Default::default()
	}
}

fn overlay(content: &str, options: PlanReviewOverlayOptions) -> (PlanReviewOverlay, Rec) {
	let (rec, callbacks) = record();
	let overlay =
		PlanReviewOverlay::new(theme(), KeybindingsManager::new(), content, options, callbacks);
	(overlay, rec)
}

fn cx() -> DescribeContext {
	DescribeContext::default()
}

/// Depth-first search for the node whose sibling key is `key`.
fn find_keyed<'a>(root: &'a NativeNode, key: &str) -> Option<&'a NativeNode> {
	if root.key.as_deref() == Some(key) {
		return Some(root);
	}
	root.children.iter().find_map(|child| find_keyed(child, key))
}

fn item_label(node: &NativeNode) -> &str {
	node.props.get_str("label").unwrap_or("")
}

#[test]
fn contents_selection_retargets_the_section_that_annotate_uses() {
	let (mut overlay, rec) = overlay(PLAN, options(&[]));
	// Render once so `section_offsets` exists: the Contents select scrubs the
	// body through it before the ToC cursor is derived.
	overlay.render(120);

	let root = overlay.describe(&cx());
	let toc = find_keyed(&root, "toc").expect("toc list");
	assert_eq!(toc.kind, NodeKind::List);
	let labels: Vec<&str> = toc.children.iter().map(item_label).collect();
	assert_eq!(labels, vec!["Alpha", "Beta", "Gamma"]);
	let beta = toc.children[1].key.clone().expect("beta key");

	overlay.handle_native_event(&NativeUiEvent::Select { key: vec!["toc".to_string()], item: beta.clone() });
	let root = overlay.describe(&cx());
	assert_eq!(find_keyed(&root, "toc").and_then(|node| node.props.get_str("selected")), Some(beta.as_str()));

	overlay.handle_input(&Key::Tab);
	assert_eq!(overlay.focus(), PlanReviewFocus::Toc);
	overlay.handle_input(&Key::Char('a'));
	assert!(overlay.is_annotating());
	for character in "tighten this".chars() {
		overlay.handle_input(&Key::Char(character));
	}
	overlay.handle_input(&Key::Enter);
	assert!(!overlay.is_annotating());

	let feedback = rec.feedback.borrow().last().cloned().unwrap_or_default();
	assert!(feedback.contains("## Beta"), "{feedback:?}");
	assert!(feedback.contains("tighten this"), "{feedback:?}");

	let root = overlay.describe(&cx());
	let beta_item = &find_keyed(&root, "toc").expect("toc list").children[1];
	let detail = beta_item.props.get_str("detail").unwrap_or("");
	assert!(detail.contains('✎'), "{detail:?}");
}

#[test]
fn activating_an_option_picks_it_and_disabled_options_ignore_events() {
	let (mut overlay, rec) = overlay(PLAN, options(&[1]));
	let root = overlay.describe(&cx());
	let option_items = find_keyed(&root, "options").expect("options list");
	let disabled = option_items.children[1].key.clone().expect("disabled key");

	overlay.handle_native_event(&NativeUiEvent::Activate {
		key: vec!["options".to_string()],
		item: disabled,
	});
	assert!(rec.picked.borrow().is_empty());

	let enabled = option_items.children[0].key.clone().expect("enabled key");
	overlay.handle_native_event(&NativeUiEvent::Activate {
		key: vec!["options".to_string()],
		item: enabled,
	});
	assert_eq!(rec.picked.borrow().as_slice(), ["Approve"]);
	assert_eq!(overlay.committed_label(), Some("Approve"));

	// No spinner replaces the list; the confirm action dims instead.
	let root = overlay.describe(&cx());
	assert_eq!(find_keyed(&root, "options").map(|node| node.kind), Some(NodeKind::List));
	let confirm = find_keyed(&root, "confirm").expect("confirm button");
	assert_eq!(confirm.props.get_str("disabled"), Some("Choice already submitted"));
}

#[test]
fn describe_is_stable_until_visible_state_changes() {
	let (mut overlay, _rec) = overlay(PLAN, options(&[]));
	let context = cx();
	let first = overlay.describe(&context);
	assert_eq!(first, overlay.describe(&context));

	overlay.handle_input(&Key::Down);
	let moved = overlay.describe(&context);
	assert_ne!(first, moved);
	assert_eq!(
		find_keyed(&moved, "options").and_then(|node| node.props.get_str("selected")),
		Some("option-1")
	);
}
