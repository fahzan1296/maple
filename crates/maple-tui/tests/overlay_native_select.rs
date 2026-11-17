//! Reference: `l/test/overlay-native-select.test.ts`.
//!
//! The reference drives the logout account selector with arrow keys and native
//! picker/list events. The crate ports the same component, list and picker
//! event routing, so the cases are asserted directly.
//! A `select` event only moves the selection (the reference also confirmed the
//! pick when the plain list node received `select`); the crate confirms via
//! `activate`/Enter.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NativeNode, NodeKind};
use maple_tui::native::picker::{PickerUiEvent};
use maple_tui::overlays::logout_account_selector::{
	LogoutAccount, LogoutAccountSelectorComponent, LogoutAccountType,
};

fn accounts() -> Vec<LogoutAccount> {
	vec![
		LogoutAccount {
			credential_id: 1,
			provider: "anthropic".into(),
			label: "work".into(),
			detail: "oauth".into(),
			kind: LogoutAccountType::Oauth,
			active: true,
		},
		LogoutAccount {
			credential_id: 2,
			provider: "anthropic".into(),
			label: "personal".into(),
			detail: "api key".into(),
			kind: LogoutAccountType::ApiKey,
			active: false,
		},
		LogoutAccount {
			credential_id: 3,
			provider: "anthropic".			label: "ci".into(),
			detail: "api key".into(),
			kind: LogoutAccountType::ApiKey,
			active: false,
		},
	]
}

type Picked = Rc<RefCell<Vec<LogoutAccount>>>;

fn selector(picked: Picked) -> LogoutAccountSelectorComponent {
	LogoutAccountSelectorComponent::new(
		"Anthropic",
		accounts(),
		move |account| picked.borrow_mut().push(account.clone()),
		|| {},
	)
}

fn generic_cx() -> DescribeContext {
	DescribeContext::new(100)
}

fn picker_cx() -> DescribeContext {
	let mut cx = DescribeContext::new(100);
	cx.enable_feature("picker");
	cx
}

fn find_list(root: &NativeNode) -> Option<NativeNode> {
	let mut stack = vec![root.clone()];
	while let Some(current) = stack.pop() {
		if current.kind == NodeKind::List {
			return Some(current);
		}
		stack.extend(current.children);
	}
	None
}

#[test]
fn keys_and_native_activate_pick_the_same_account() {
	let via_keys: Picked = Rc::new(RefCell::new(Vec::new()));
	let mut keyed = selector(via_keys.clone());
	keyed.handle_input(&Key::Down);
	keyed.handle_input(&Key::Down);
	keyed.handle_input(&Key::Enter);
	let keyed_ids: Vec<i64> = via_keys.borrow().iter().map(|account| account.credential_id).collect();
	assert_eq!(keyed_ids, vec![3]);

	let via_pointer: Picked = Rc::new(RefCell::new(Vec::new()));
	let mut native = selector(via_pointer.clone());
	let list = find_list(&native.describe_native(&generic_cx())).expect("list node");
	let target = list.children.iter().find(|child| child.key.as_deref() == Some("3"));
	assert!(target.is_some(), "account 3 row missing from the list");
	// A row click only moves the selection; activation picks the account.
	native.handle_native_event(&PickerUiEvent::Select { key: String::new(), item: "3".into() });
	assert!(via_pointer.borrow().is_empty(), "select must not sign out");
	native.handle_native_event(&PickerUiEvent::Activate { key: String::new(), item: "3".into() });
	let pointer_ids: Vec<i64> =
		via_pointer.borrow().iter().map(|account| account.credential_id).collect();
	assert_eq!(pointer_ids, keyed_ids);
}

#[test]
fn keeps_its_description_while_unchanged_and_moves_the_selection_with_arrows() {
	let mut component = selector(Rc::new(RefCell::new(Vec::new())));
	let first = component.describe_native(&generic_cx());
	assert_eq!(component.describe_native(&generic_cx()), first);
	assert_eq!(find_list(&first).unwrap().props.get_str("selected"), Some("1"));

	component.handle_input(&Key::Down);
	let moved = component.describe_native(&generic_cx());
	assert_ne!(moved, first);
	assert_eq!(find_list(&moved).unwrap().props.get_str("selected"), Some("2"));
}

#[test]
fn as_a_picker_a_row_click_moves_the_selection_and_a_second_click_signs_out() {
	let picked: Picked = Rc::new(RefCell::new(Vec::new()));
	let mut component = selector(picked.clone());
	let sheet = {
		let described = component.describe_native(&picker_cx());
		assert_eq!(described.children.len(), 1);
		described.children[0].clone()
	};
	assert_eq!(sheet.kind, NodeKind::Picker);
	assert_eq!(sheet.props.get_str("selected"), Some("1"));
	assert_eq!(
		sheet.props.get("current"),
		Some(&serde_json::json!(["1"])),
		"picker current values"
	);
	assert_eq!(sheet.props.get_str("size"), Some("md"));

	component.handle_native_event(&PickerUiEvent::Select { key: String::new(), item: "3".into() });
	assert!(picked.borrow().is_empty(), "a row click only selects");
	let selected = component.describe_native(&picker_cx()).children[0]
		.props
		.get_str("selected")
		.map(str::to_string);
	assert_eq!(selected.as_deref(), Some("3"));

	component.handle_native_event(&PickerUiEvent::Activate { key: String::new(), item: "3".into() });
	let picked_ids: Vec<i64> = picked.borrow().iter().map(|account| account.credential_id).collect();
	assert_eq!(picked_ids, vec![3]);
}
