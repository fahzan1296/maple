//! Reference: `l/test/selector-picker-native.test.ts`.
//!
//! Deviations:
//! - The reference builds providers from `getOAuthProviders()`; the crate has no
//!   catalogue port, so [`OAuthProviderInfo`] rows are injected.
//! - Hook selector callbacks travel in `HookSelectorOptions::callbacks` rather
//!   than positional arguments, and row keys are original indices.
//! - The OAuth picker reports credential state as `value` ("logged in") plus an
//!   origin `detail`; the reference's combined "Signed in · login" text is not
//!   reproduced.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::keys::Key;
use maple_tui::native::backend::NativeUiEvent;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NativeNode, NodeKind, Props};
use maple_tui::native::picker::{PickerUiEvent, PICKER_KEY};
use maple_tui::overlays::hook_selector::{
	HookSelectorCallbacks, HookSelectorComponent, HookSelectorOptionInput, HookSelectorOptions,
};
use maple_tui::overlays::oauth_selector::{
	CredentialOrigin, CredentialOriginKind, OAuthProviderInfo, OAuthSelectorAuthSource,
	OAuthSelectorComponent, OAuthSelectorMode, OAuthSelectorOptions,
};
use maple_tui::overlays::reset_usage_selector::{
	ResetCreditDetail, ResetUsageAccount, ResetUsageSelectorComponent, ResetUsageTarget,
};
use maple_tui::overlays::session_account_selector::{
	SessionAccountSelectorComponent, SessionPinAccount,
};
use maple_tui::overlays::theme_selector::ThemeSelectorComponent;
use maple_tui::overlays::thinking_selector::{ThinkingLevelOption, ThinkingSelectorComponent, AUTO_THINKING};
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::loader::{create_theme, CreateThemeOptions};
use maple_tui::theme::theme_class::Theme;

fn picker_cx() -> DescribeContext {
	DescribeContext::with_capabilities(120, &[NodeKind::Picker], &["picker"])
}

fn generic_cx() -> DescribeContext {
	DescribeContext::new(120)
}

/// The picker node: the root itself, or the hoisted child of a docked sheet.
fn picker_node(root: &NativeNode) -> &NativeNode {
	if root.kind == NodeKind::Picker {
		return root;
	}
	root.children
		.iter()
		.find(|child| child.kind == NodeKind::Picker)
		.expect("picker child")
}

fn item_prop<'a>(node: &'a NativeNode, id: &str, key: &str) -> Option<&'a serde_json::Value> {
	item(node, id)?.prop(key)
}
fn item<'a>(node: &'a NativeNode, id: &str) -> Option<&'a NativeNode> {
	let matches = |item: &&NativeNode| item.get_str("id").as_deref() == Some(id) || item.key.as_deref() == Some(id);
	if let Some(items) = props(node).get("items").and_then(|value| value.as_array()) {
		if let Some(found) = items.iter().find(|item| matches(item)) {
			return Some(found);
		}
	}
	let picker = picker_node(node);
	picker
		.children
	.iter()
		.find(|child| child.kind == NodeKind::List)
		.and_then(|list| list.children.iter().find(|item| matches(item)))
}


fn labels(node: &NativeNode, key: &str) -> Vec<String> {
	props(node)
		.get(key)
		.as_array()
		.map(|actions| actions.iter().filter_map(|action| action.get_str("label")).collect())
		.unwrap_or_default()
}

fn strings(node: &NativeNode, key: &str) -> Vec<String> {
	props(node)
		.get(key)
		.as_array()
		.map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_string)).collect())
		.unwrap_or_default()
}

fn select(item: &str) -> PickerUiEvent {
	PickerUiEvent::Select { key: PICKER_KEY.to_string(), item: item.to_string() }
}

fn activate(item: &str) -> PickerUiEvent {
	PickerUiEvent::Activate { key: PICKER_KEY.to_string(), item: item.to_string() }
}

fn action(act: &str) -> PickerUiEvent {
	PickerUiEvent::Action { key: PICKER_KEY.to_string(), act: act.to_string(), value: None, mods: Vec::new() }
}

fn native_action(act: &str) -> NativeUiEvent {
	NativeUiEvent::Action { key: Vec::new(), act: act.to_string(), value: None, mods: Vec::new() }
}

fn native_activate(item: &str) -> NativeUiEvent {
	NativeUiEvent::Activate { key: Vec::new(), item: item.to_string() }
}

fn theme_selector(log: &Rc<RefCell<Vec<String>>>) -> ThemeSelectorComponent {
	let sink = Rc::clone(log);
	let cancel_sink = Rc::clone(log);
	let preview_sink = Rc::clone(log);
	ThemeSelectorComponent::new(
		"dark",
		&["dark".to_string(), "light".to_string(), "nord".to_string()],
		move |name| sink.borrow_mut().push(format!("select:{name}")),
		move || cancel_sink.borrow_mut().push("cancel".to_string()),
		move |name| preview_sink.borrow_mut().push(format!("preview:{name}")),
	)
}

#[test]
fn theme_picker_describes_and_routes_events() {
	let log = Rc::new(RefCell::new(Vec::new()));
	let mut selector = theme_selector(&log);
	let described = selector.describe_native(&picker_cx());
	let p = props(&described);
	let ids: Vec<&str> = p
		.get("items")
		.as_array()
		.unwrap()
		.iter()
		.map(|item| item.get_str("id").unwrap())
		.collect();
	assert_eq!(ids, ["dark", "light", "nord"]);
	assert_eq!(item_prop(&described, "nord", "mark").and_then(|mark| mark.get("seed")).and_then(|seed| seed.as_str()), Some("nord"));
	assert_eq!(strings(&described, "current"), ["dark"]);
	assert_eq!(p.get_str("selected").as_deref(), Some("dark"));
	assert_eq!(labels(&described, "actions"), ["Apply", "Close"]);

	let generic = selector.describe_native(&generic_cx());
	assert!(generic.children.iter().all(|child| child.kind != NodeKind::Picker));

	assert!(selector.handle_picker_event(&select("nord")));
	assert_eq!(props(&selector.describe_native(&picker_cx())).get_str("selected").as_deref(), Some("nord"));
	assert!(selector.handle_picker_event(&activate("nord")));
	assert!(selector.handle_picker_event(&action("close")));
	assert_eq!(&*log.borrow(), &["preview:nord".to_string(), "select:nord".to_string(), "cancel".to_string()]);
}

fn level(value: &str, depth: Option<f32>) -> ThinkingLevelOption {
	ThinkingLevelOption { value: value.to_string(), label: value.to_string(), description: None, depth }
}

#[test]
fn thinking_picker_meters_depth_and_marks_auto() {
	let mut selector = ThinkingSelectorComponent::new(
		Some("high"),
		vec![level("low", Some(0.25)), level("high", Some(1.0))],
		|_| {},
		|| {},
	);
	let described = selector.describe_native(&picker_cx());
	let p = props(&described);
	let formats: Vec<Option<String>> = p
		.get("columns")
		.as_array()
		.unwrap()
		.iter()
		.map(|column| column.get_str("format"))
		.collect();
	assert_eq!(formats, [Some("bar".to_string())]);
	let low = item_prop(&described, "low", "facts").and_then(|facts| facts.get("depth")).and_then(|depth| depth.as_f64()).unwrap();
	let high = item_prop(&described, "high", "facts").and_then(|facts| facts.get("depth")).and_then(|depth| depth.as_f64()).unwrap();
	assert!(high > low);
	assert!(high <= 1.0);
	assert_eq!(strings(&described, "current"), ["high"]);
	assert!(selector.handle_picker_event(&select("low")));
	assert_eq!(props(&selector.describe_native(&picker_cx())).get_str("selected").as_deref(), Some("low"));

	let with_auto = ThinkingSelectorComponent::new(
		Some(AUTO_THINKING),
		vec![level("off", Some(0.0)), level(AUTO_THINKING, None), level("high", Some(0.75))],
		|_| {},
		|| {},
	);
	let described = with_auto.describe_native(&picker_cx());
	assert_eq!(strings(&described, "current"), [AUTO_THINKING]);
	assert_eq!(props(&described).get_str("selected").as_deref(), Some(AUTO_THINKING));
	assert!(item(&described, AUTO_THINKING).unwrap().prop("facts").is_none());
	assert_eq!(
		item_prop(&described, "off", "facts").and_then(|facts| facts.get("depth")).and_then(|depth| depth.as_f64()),
		Some(0.0)
	);
}

fn account(position: usize, credential_id: i64, email: &str, active: bool) -> SessionPinAccount {
	SessionPinAccount {
		position,
		credential_id,
		email: Some(email.to_string()),
		org_name: if credential_id == 11 { Some("Acme".to_string()) } else { None },
		active,
		label: email.to_string(),
		..SessionPinAccount::default()
	}
}

#[test]
fn account_picker_pins_active_and_routes() {
	let picked = Rc::new(RefCell::new(Vec::new()));
	let sink = Rc::clone(&picked);
	let cancel_sink = Rc::clone(&picked);
	let mut selector = SessionAccountSelectorComponent::new(
		"Anthropic",
		&[account(0, 11, "a@x.so", false), account(1, 12, "b@x.so", true)],
		move |chosen| sink.borrow_mut().push(chosen.credential_id),
		move || cancel_sink.borrow_mut().push(-1),
	);
	let described = selector.describe_native(&picker_cx());
	assert_eq!(strings(&described, "current"), ["12"]);
	assert_eq!(props(&described).get_str("selected").as_deref(), Some("12"));
	assert_eq!(item_prop(&described, "11", "detail").and_then(|detail| detail.as_str()), Some("Acme"));
	assert_eq!(labels(&described, "actions"), ["Pin", "Close"]);

	selector.handle_native_event(&select("11"));
	selector.handle_native_event(&action("confirm"));
	selector.handle_native_event(&action("close"));
	assert_eq!(&*picked.borrow(), &[11, -1]);

	let generic = selector.describe_native(&generic_cx());
	assert_ne!(generic.kind, NodeKind::Picker);
	assert!(generic.children.iter().all(|child| child.kind != NodeKind::Picker));
}

fn reset_account() -> (ResetUsageAccount, ResetUsageAccount) {
	let usable = ResetUsageAccount {
		label: "a@x.so".to_string(),
		provider: "anthropic".to_string(),
		provider_label: "Claude Max".to_string(),
		available_count: 2,
		redeemable_count: 2,
		target: ResetUsageTarget { credential_id: 4, provider: "anthropic".to_string(), credit_id: Some("c1".to_string()), ..ResetUsageTarget::default() },
		active: true,
		credit: Some(ResetCreditDetail { clears: vec!["anthropic:5h".to_string()], ..ResetCreditDetail::default() }),
		..ResetUsageAccount::default()
	};
	let blocked = ResetUsageAccount {
		label: "b@x.so".to_string(),
		provider: "openai-codex".to_string(),
		provider_label: "ChatGPT Pro".to_string(),
		available_count: 1,
		redeemable_count: 0,
		target: ResetUsageTarget { credential_id: 7, provider: "openai-codex".to_string(), ..ResetUsageTarget::default() },
		unavailable_reason: Some("limit not reached".to_string()),
		..ResetUsageAccount::default()
	};
	(usable, blocked)
}

#[test]
fn reset_usage_picker_gates_spend_on_confirm() {
	let (usable, blocked) = reset_account();
	let spent = Rc::new(RefCell::new(Vec::new()));
	let sink = Rc::clone(&spent);
	let cancel_sink = Rc::clone(&spent);
	let mut selector = ResetUsageSelectorComponent::new(
		vec![usable, blocked],
		0,
		move |account| sink.borrow_mut().push(account.label.clone()),
		move || cancel_sink.borrow_mut().push("cancel".to_string()),
	);
	let first = selector.picker_props();
	assert_eq!(first.current, Some(vec!["anthropic:4:c1".to_string()]));
	assert_eq!(
		item_prop(&selector.describe_native(&picker_cx()), "openai-codex:7:", "disabled").and_then(|value| value.as_str()),
		Some("limit not reached")
	);
	assert!(first.confirm.is_none());

	assert!(selector.handle_native_event(&action("confirm")));
	let pending = selector.picker_props();
	assert_eq!(pending.confirm.as_ref().map(|confirm| confirm.label.as_str()), Some("Spend"));
	assert!(pending.subtitle.as_deref().unwrap_or_default().starts_with("Covers "));
	assert!(spent.borrow().is_empty());

	assert!(selector.handle_native_event(&action("cancel")));
	assert!(selector.picker_props().confirm.is_none());
	assert!(spent.borrow().is_empty());

	assert!(selector.handle_native_event(&action("confirm")));
	assert!(selector.handle_native_event(&action("confirm")));
	assert_eq!(&*spent.borrow(), &["a@x.so".to_string()]);
	assert!(selector.handle_native_event(&action("close")));
	assert_eq!(&*spent.borrow(), &["a@x.so".to_string(), "cancel".to_string()]);
}

#[test]
fn hook_selector_moves_then_confirms_and_stays_inline() {
	let picked = Rc::new(RefCell::new(Vec::new()));
	let sink = Rc::clone(&picked);
	let mut selector = HookSelectorComponent::new(
		"Pick",
		vec![HookSelectorOptionInput::Label("one".to_string()), HookSelectorOptionInput::Label("two".to_string())],
		HookSelectorOptions {
			callbacks: HookSelectorCallbacks { on_select: Some(Box::new(move |label| sink.borrow_mut().push(label.to_string()))), ..HookSelectorCallbacks::default() },
			..HookSelectorOptions::default()
		},
	);
	selector.handle_key(&Key::Down);
	selector.handle_key(&Key::Enter);
	assert_eq!(&*picked.borrow(), &["two".to_string()]);

	let inline_picked = Rc::new(RefCell::new(Vec::new()));
	let inline_sink = Rc::clone(&inline_picked);
	let mut inline = HookSelectorComponent::new(
		"Save where?",
		vec![HookSelectorOptionInput::Label("project".to_string()), HookSelectorOptionInput::Label("global".to_string())],
		HookSelectorOptions {
			inline: true,
			callbacks: HookSelectorCallbacks {
				on_select: Some(Box::new(move |label| inline_sink.borrow_mut().push(label.to_string()))),
				..HookSelectorCallbacks::default()
			},
			..HookSelectorOptions::default()
		},
	);
	let root = inline.describe();
	assert!(root.children.iter().all(|child| child.kind != NodeKind::Picker));
	assert!(inline.activate_row(1));
	assert_eq!(&*inline_picked.borrow(), &["global".to_string()]);
}

#[derive(Default)]
struct AuthSource {
	signed_in: String,
}

impl OAuthSelectorAuthSource for AuthSource {
	fn has_credential(&self, provider_id: &str) -> bool {
		provider_id == self.signed_in
	}

	fn source(&self, provider_id: &str) -> Option<CredentialOrigin> {
		(self.provider_id == self.signed_in).then(|| CredentialOrigin::new(CredentialOriginKind::OAuth))
	}
}

#[test]
fn oauth_picker_describes_providers_and_activates() {
	let providers = vec![
		OAuthProviderInfo::new("a", "Provider A"),
		OAuthProviderInfo::new("b", "Provider B").unavailable(),
	];
	let auth = AuthSource { signed_in: "a".to_string() };
	let log = Rc::new(RefCell::new(Vec::new()));
	let sink = Rc::clone(&log);
	let cancel_sink = Rc::clone(&log);
	let mut selector = OAuthSelectorComponent::new(
		create_theme(builtin_theme_json("dark").expect("dark theme"), CreateThemeOptions::default()).expect("theme"),
		maple_tui::keybindings::KeybindingsManager::default(),
		OAuthSelectorMode::Login,
		providers,
		Box::new(auth),
		move |id| sink.borrow_mut().push(id),
		move || cancel_sink.borrow_mut().push("cancel".to_string()),
		OAuthSelectorOptions::default(),
	);
	let described = selector.describe(&picker_cx());
	assert_eq!(described.kind, NodeKind::Picker);
	let p = props(&described);
	assert_eq!(p.get_str("layout").as_deref(), Some("cards"));
	assert_eq!(p.get_str("title").as_deref(), Some("Sign in"));
	assert_eq!(item_prop(&described, "a", "value").and_then(|value| value.as_str()), Some("logged in"));
	let detail = item_prop(&described, "a", "detail").and_then(|detail| detail.as_array());
	assert_eq!(detail.map(|spans| spans.len()), Some(1));
	assert_eq!(detail.and_then(|spans| spans.first()).and_then(|span| span.as_str()), Some("login"));
	let unavailable = item(&described, "b").unwrap();
	assert_eq!(unavailable.get_str("tone").as_deref(), Some("muted"));
	assert!(p.get_str("query").is_none() || p.get_str("query").as_deref() == Some(""));

	let target = "a".to_string();
	selector.handle_native_event(&native_activate(&target));
	selector.handle_native_event(&native_action("close"));
	assert_eq!(&*log.borrow(), &["a".to_string(), "cancel".to_string()]);
}
