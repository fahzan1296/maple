//! Reference: `l/test/session-picker-native.test.ts`.
//!
//! Deviations:
//! - The picker placeholder is `Search sessions…` / `Search all sessions…`; the
//!   reference interpolates the folder name.
//! - Row detail is a plain shortened path, not `path dim`/`path` spans.
//! - Rename runs synchronously, so the reference's render-await helper is a
//!   direct assertion after the key.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::native::backend::NativeUiEvent;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use maple_tui::overlays::session_selector::{
	SessionSelectorComponent, SessionSelectorEntry, SessionSelectorOptions, SessionSelectorStatus,
};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::loader::{create_theme, CreateThemeOptions};

const DAY_MS: i64 = 86_400_000;
const NOW: i64 = 1_800_000_000_000;

fn theme() -> maple_tui::theme::theme_class::Theme {
	create_theme(builtin_theme_json("dark").expect("dark theme"), CreateThemeOptions::default()).expect("theme")
}

fn session(id: &str, title: &str, age_ms: i64, cwd: &str) -> SessionSelectorEntry {
	SessionSelectorEntry {
		path: format!("{cwd}/.omp/{id}.jsonl"),
		id: id.to_string(),
		cwd: cwd.to_string(),
		title: Some(title.to_string()),
		modified: NOW - age_ms,
		size: 5_600,
		first_message: format!("{title} prompt"),
		all_messages_text: format!("{title} prompt {title} answer"),
		status: Some(SessionSelectorStatus::Complete),
		..SessionSelectorEntry::default()
	}
}

fn picker_cx() -> DescribeContext {
	DescribeContext::with_capabilities(120, &[NodeKind::Picker], &["picker"])
}

fn generic_cx() -> DescribeContext {
	DescribeContext::new(120)
}

fn item<'a>(root: &'a NativeNode, id: &str) -> Option<&'a NativeNode> {
	root.props
		.get("items")
		.and_then(|value| value.as_array())
		.and_then(|items| items.iter().find(|item| item.get_str("id").as_deref() == Some(id)))
}

fn order(root: &NativeNode) -> Vec<serde_json::Value> {
	root.props.get("order").and_then(|value| value.as_array()).cloned().unwrap_or_default()
}

fn strings(root: &NativeNode, key: &str) -> Vec<String> {
	root.props
		.get(key)
		.and_then(|value| value.as_array())
		.map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_string)).collect())
		.unwrap_or_default()
}

fn labels(root: &NativeNode) -> Vec<String> {
	root.props
		.get("actions")
		.and_then(|value| value.as_array())
		.map(|actions| actions.iter().filter_map(|action| action.get_str("label")).collect())
		.unwrap_or_default()
}

fn action(id: &str) -> NativeUiEvent {
	NativeUiEvent::Action { key: Vec::new(), act: id.to_string(), value: None, mods: Vec::new() }
}

fn select(item: &str) -> NativeUiEvent {
	NativeUiEvent::Select { key: Vec::new(), item: item.to_string() }
}

struct Harness {
	selector: SessionSelectorComponent<SessionSelectorEntry>,
	log: Rc<RefCell<Vec<String>>>,
}

fn make(
	entries: Vec<SessionSelectorEntry>,
	all: Option<Vec<SessionSelectorEntry>>,
	log: Rc<RefCell<Vec<String>>>,
	with_delete: bool,
) -> Harness {
	let sink = Rc::clone(&log);
	let delete_sink = Rc::clone(&log);
	let cancel_sink = Rc::clone(&log);
	let mut options = SessionSelectorOptions {
		current_session_path: Some(entries[1].path.clone()),
		all_sessions: all,
		..SessionSelectorOptions::default()
	};
	if with_delete {
		options.on_delete = Some(Box::new(move |session: &SessionSelectorEntry| {
			delete_sink.borrow_mut().push(session.id.clone());
			Ok(true)
		}));
	}
	let selector = SessionSelectorComponent::new(
		entries,
		move |session: &SessionSelectorEntry| sink.borrow_mut().push(format!("select:{}", session.id)),
		move || cancel_sink.borrow_mut().push("cancel".to_string()),
		|| {},
		options,
		theme(),
	);
	Harness { selector, log }
}

fn base_entries() -> Vec<SessionSelectorEntry> {
	vec![session("a", "Alpha", 0, "/work/app"), session("b", "Beta", 1, "/work/app"), session("c", "Gamma", 40 * DAY_MS, "/work/app")]
}

#[test]
fn describes_grouped_cards_picker_with_preview() {
	let log = Rc::new(RefCell::new(Vec::new()));
	let harness = make(base_entries(), None, log, false);
	let root = harness.selector.describe_native(&picker_cx());
	assert_eq!(root.kind, NodeKind::Picker);
	assert_eq!(root.props.get_str("size").as_deref(), Some("lg"));
	assert_eq!(root.props.get_str("layout").as_deref(), Some("cards"));
	assert_eq!(root.props.get_str("placeholder").as_deref(), Some("Search sessions…"));
	assert_eq!(root.props.get_str("state").as_deref(), Some("ready"));

	let entries = base_entries();
	let expected_order = vec![
		serde_json::json!({ "group": "today-0", "label": "Today", "count": 2 }),
		serde_json::Value::String(entries[0].path.clone()),
		serde_json::Value::String(entries[1].path.clone()),
		serde_json::json!({ "group": "earlier-2", "label": "Earlier", "count": 1 }),
		serde_json::Value::String(entries[2].path.clone()),
	];
	assert_eq!(order(&root), expected_order);
	assert_eq!(root.props.get_str("selected").as_deref(), Some(entries[1].path.as_str()));
	assert_eq!(strings(&root, "current"), [entries[1].path.clone()]);
	assert!(labels(&root).contains(&"All projects".to_string()));

	let beta = item(&root, &entries[1].path).expect("beta row");
	let badges = beta.props.get("badges").and_then(|value| value.as_array()).expect("badges");
	assert_eq!(badges[0].get("text").and_then(|value| value.as_str()), Some("current"));
	assert_eq!(beta.props.get_str("dot").as_deref(), Some("success"));

	assert_eq!(root.children[0].kind, NodeKind::Text);
	assert_eq!(root.children[0].props.get_str("text").as_deref(), Some("Beta"));
	assert_eq!(root.children[0].props.get_str("role").as_deref(), Some("omp.picker.title"));
	assert!(root.children.iter().any(|child| child.kind == NodeKind::Section && child.props.get_str("head").as_deref() == Some("Conversation")));

	let generic = harness.selector.describe_native(&generic_cx());
	assert_eq!(generic.kind, NodeKind::Card);
}

#[test]
fn typing_filters_order_hits_and_selection_but_keeps_the_catalogue() {
	let log = Rc::new(RefCell::new(Vec::new()));
	let mut harness = make(base_entries(), None, log, false);
	let before = harness.selector.describe_native(&picker_cx());
	let before_items = before.props.get("items").and_then(|value| value.as_array()).expect("items").len();

	for key in [Key::Char('g'), Key::Char('a'), Key::Char('m')] {
		harness.selector.handle_input(&key);
	}
	let after = harness.selector.describe_native(&picker_cx());
	assert_eq!(after.props.get_str("query").as_deref(), Some("gam"));
	assert_eq!(after.props.get("items").and_then(|value| value.as_array()).expect("items").len(), before_items);
	let entries = base_entries();
	assert_eq!(order(&after), vec![serde_json::Value::String(entries[2].path.clone())]);
	assert_eq!(after.props.get_str("selected").as_deref(), Some(entries[2].path.as_str()));
	let hits = after.props.get("hits").and_then(|value| value.as_object()).expect("hits");
	let ranges = hits.get(&entries[2].path).and_then(|value| value.as_array()).expect("ranges");
	assert_eq!(ranges, &vec![serde_json::json!([0, 3])]);
}

#[test]
fn runs_native_events_down_resume_delete_and_scope_paths() {
	let log = Rc::new(RefCell::new(Vec::new()));
	let entries = base_entries();
	let all = vec![
		entries[0].clone(),
		entries[1].clone(),
		session("d", "Delta", 3 * DAY_MS, "/work/other"),
		entries[2].clone(),
	];
	let mut harness = make(entries.clone(), Some(all), log, true);

	harness.selector.handle_native_event(&select(&entries[2].path));
	assert_eq!(
		harness.selector.describe_native(&picker_cx()).props.get_str("selected").as_deref(),
		Some(entries[2].path.as_str())
	);

	harness.selector.handle_native_event(&action("resume"));
	assert_eq!(&*harness.log.borrow(), &vec!["select:c".to_string()]);

	harness.selector.handle_native_event(&action("delete"));
	let confirm = harness.selector.describe_native(&picker_cx()).props.get("confirm").expect("confirm");
	assert_eq!(confirm.get("act").and_then(|value| value.as_str()), Some("delete-confirm"));
	harness.selector.handle_native_event(&action("cancel"));
	assert!(harness.selector.describe_native(&picker_cx()).props.get("confirm").is_none());

	harness.selector.handle_input(&Key::Backspace);
	let confirm = harness.selector.describe_native(&picker_cx()).props.get("confirm").expect("confirm");
	assert_eq!(
		confirm.get("text").and_then(|value| value.as_str()),
		Some("Delete “Gamma”? This removes the session file.")
	);

	harness.selector.handle_native_event(&action("delete-confirm"));
	assert_eq!(&*harness.log.borrow(), &vec!["select:c".to_string(), "c".to_string()]);
	let after_delete = harness.selector.describe_native(&picker_cx());
	assert!(order(&after_delete).iter().all(|entry| entry.as_str() != Some(entries[2].path.as_str())));

	harness.selector.handle_native_event(&action("scope"));
	let all_view = harness.selector.describe_native(&picker_cx());
	assert_eq!(all_view.props.get_str("placeholder").as_deref(), Some("Search all sessions…"));
	assert!(labels(&all_view).contains(&"This folder".to_string()));
	let delta = item(&all_view, &entries[2].path);
	assert!(delta.is_some());
	let delta = item(&all_view, "/work/other/.omp/d.jsonl").expect("delta row");
	assert!(delta.props.get_str("detail").as_deref().unwrap_or_default().contains("other"));
}

#[test]
fn rename_action_opens_editor_and_updates_the_row() {
	let log = Rc::new(RefCell::new(Vec::new()));
	let entries = base_entries();
	let calls = Rc::new(RefCell::new(Vec::new()));
	let sink = Rc::clone(&calls);
	let mut options = SessionSelectorOptions {
		current_session_path: Some(entries[1].path.clone()),
		on_rename: Some(Box::new(move |session: &SessionSelectorEntry, title: &str| {
			sink.borrow_mut().push((session.id.clone(), title.to_string()));
			Ok(Some(title.to_string()))
		})),
		..SessionSelectorOptions::default()
	};
	options.on_delete = None;
	let mut selector = SessionSelectorComponent::new(
		entries.clone(),
		|_| {},
		|| {},
		|| {},
		options,
		theme(),
	);

	let with_hook = selector.describe_native(&picker_cx());
	assert!(labels(&with_hook).contains(&"Rename".to_string()));

	selector.handle_input(&Key::F(2));
	let card = selector.describe_native(&picker_cx());
	assert_eq!(card.kind, NodeKind::Card);
	assert!(card.children.iter().any(|child| child.kind == NodeKind::Editor && child.key.as_deref() == Some("rename")));

	for key in " Prime".chars().map(Key::Char) {
		selector.handle_input(&key);
	}
	selector.handle_input(&Key::Enter);
	assert_eq!(&*calls.borrow(), &[("b".to_string(), "Beta Prime".to_string())]);
	let after = selector.describe_native(&picker_cx());
	assert_eq!(item(&after, &entries[1].path).and_then(|row| row.props.get_str("label")).as_deref(), Some("Beta Prime"));
}
