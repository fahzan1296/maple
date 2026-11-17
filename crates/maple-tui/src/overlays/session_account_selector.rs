//! Reference: `l/src/overlays/session-account-selector.ts`.
//!
//! Account picker opened by `/session pin` for the current model provider.
//!
//! Deviations:
//! - The reference extends `OverlayPanel` and adds the select list as a child;
//!   the crate keeps the rows, the list and the theme as fields and renders the
//!   same panel chrome itself, because the native picker adapter needs shared
//!   access to the list during `describe`.
//! - `SelectListSheet` is built from the crate's
//!   [`crate::native::picker::select_list_picker`] options instead of a wrapper
//!   type; the pointer/event routing is
//!   [`crate::native::picker::route_select_list_picker`].

use std::cell::{Ref, RefCell, RefMut};
use std::sync::Arc;

use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::chrome::select_list_mouse_routing::route_select_list_mouse_with_top_border;
use crate::component::Component;
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};
use crate::keys::Key;
use crate::mouse::MouseEvent;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{NativeDescribe, node, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span};
use crate::native::overlay::overlay_card;
use crate::native::picker::{
	PickerColumn, PickerList, PickerView, SelectPickerOptions, docked_picker,
	route_select_list_picker, select_list_picker,
};

/// Most accounts the picker shows at once.
pub const ACCOUNT_SELECTOR_MAX_VISIBLE: usize = 10;

/// Semantic role of the picker panel.
pub const ACCOUNT_SELECTOR_ROLE: &str = "omp.overlay.session-account";

/// Stored account identity rendered and matched by the session account picker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionPinAccount {
	/// Position of the credential in the provider's stored account list.
	pub position: usize,
	/// Credential row id.
	pub credential_id: i64,
	/// Provider account id.
	pub account_id: Option<String>,
	/// Account email.
	pub email: Option<String>,
	/// Cloud project id.
	pub project_id: Option<String>,
	/// Enterprise host.
	pub enterprise_url: Option<String>,
	/// Organization id.
	pub org_id: Option<String>,
	/// Organization name.
	pub org_name: Option<String>,
	/// Whether this account is the one the session currently uses.
	pub active: bool,
	/// Display label.
	pub label: String,
}

/// What tells two accounts of one provider apart besides their label: the
/// organization, project or host.
#[must_use]
pub fn account_detail(account: &SessionPinAccount) -> Option<String> {
	let mut parts: Vec<&str> = Vec::new();
	if let Some(org) = account.org_name.as_deref().or(account.org_id.as_deref()) {
		parts.push(org);
	}
	if let Some(project) = account.project_id.as_deref() {
		parts.push(project);
	}
	if let Some(host) = account.enterprise_url.as_deref() {
		parts.push(host);
	}
	parts.retain(|part| !part.trim().is_empty() && !account.label.contains(part));
	if parts.is_empty() {
		None
	} else {
		Some(parts.join(" · "))
	}
}

/// The account rows of the picker.
#[must_use]
pub fn account_items(accounts: &[SessionPinAccount]) -> Vec<SelectItem> {
	accounts
		.iter()
		.map(|account| {
			let mut item = SelectItem::new(
				account.credential_id.to_string(),
				account.label.clone(),
			);
			if account.active {
				item = item.with_description("active for this session");
			}
			item
		})
		.collect()
}

/// Most rows to show for `count` accounts, never below one.
#[must_use]
pub fn account_max_visible(count: usize) -> usize {
	count.clamp(1, ACCOUNT_SELECTOR_MAX_VISIBLE)
}

/// Shared list handle so `describe`/`describe_native` can borrow it while the
/// component also yields it for mouse routing.
struct SheetList {
	inner: RefCell<SelectList>,
}

impl SheetList {
	fn new(list: SelectList) -> Self {
		SheetList {
			inner: RefCell::new(list),
		}
	}

	fn borrow(&self) -> Ref<'_, SelectList> {
		self.inner.borrow()
	}

	fn borrow_mut(&self) -> RefMut<'_, SelectList> {
		self.inner.borrow_mut()
	}
}

impl PickerList for SheetList {
	fn picker_view(&self) -> PickerView {
		let list = self.inner.borrow();
		PickerView {
			items: list.visible_items().to_vec(),
			selected: list.selected_key(),
			pending: None,
			query: list.filter().to_string(),
			cursor: list.filter().encode_utf16().count(),
		}
	}

	fn select_index(&mut self, index: usize) {
		self.inner.borrow_mut().set_selected_index(index as isize);
	}

	fn activate_index(&mut self, index: usize) {
		let mut list = self.inner.borrow_mut();
		list.set_selected_index(index as isize);
		list.click_item(index);
	}

	fn cancel(&mut self) {
		let mut list = self.inner.borrow_mut();
		list.handle_input(&Key::Esc);
	}
}

/// The session account picker panel.
pub struct SessionAccountSelectorComponent {
	title: String,
	role: String,
	provider_name: String,
	list: SheetList,
	options: SelectPickerOptions,
	theme: OverlayTheme,
}

impl std::fmt::Debug for SessionAccountSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SessionAccountSelectorComponent")
			.field("title", &self.title)
			.field("role", &self.role)
			.finish()
	}
}

impl SessionAccountSelectorComponent {
	/// Build the picker over `accounts`, resting on the active one.
	pub fn new(
		provider_name: &str,
		accounts: &[SessionPinAccount],
		on_select: impl FnMut(&SessionPinAccount) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		let items = account_items(accounts);
		let mut list = SelectList::new(
			items,
			account_max_visible(accounts.len()),
			SelectListTheme::default(),
		);
		if let Some(active) = accounts.iter().position(|account| account.active) {
			list.set_selected_index(active as isize);
		}
		let by_value: std::collections::HashMap<String, SessionPinAccount> = accounts
			.iter()
			.map(|account| (account.credential_id.to_string(), account.clone()))
			.collect();
		let mut on_select = on_select;
		let lookup = by_value.clone();
		list.on_select(move |item| {
			if let Some(account) = lookup.get(&item.key) {
				on_select(account);
			}
		});
		let mut on_cancel = on_cancel;
		list.on_cancel(move || on_cancel());

		let decorate_by_value = by_value;
		let options = SelectPickerOptions {
			title: format!("{provider_name} account"),
			subtitle: Some(
				"Pin the account this session sends its requests with".to_string(),
			),
			icon: Some("key-round".to_string()),
			noun: Some("accounts".to_string()),
			current: Some(
				accounts
					.iter()
					.filter(|account| account.active)
					.map(|account| account.credential_id.to_string())
					.collect(),
			),
			confirm: Some("Pin".to_string()),
			decorate: Some(Arc::new(move |item: &SelectItem| {
				let account = decorate_by_value.get(&item.key)?;
				let mut props = Props::new();
				let mark_text = account.email.clone().unwrap_or_else(|| account.label.clone());
				props.insert(
					"mark",
					serde_json::json!({ "text": mark_text, "seed": account.label }),
				);
				props.set_str("detail", account_detail(account).unwrap_or_default());
				Some(props)
			})),
			columns: Some(vec![PickerColumn {
				id: "detail".to_string(),
				head: None,
				format: Some("dim".to_string()),
				align: None,
				grow: None,
				priority: None,
			}]),
			..SelectPickerOptions::default()
		};

		SessionAccountSelectorComponent {
			title: format!("Select a {provider_name} account for this session"),
			role: ACCOUNT_SELECTOR_ROLE.to_string(),
			provider_name: provider_name.to_string(),
			list: SheetList::new(list),
			options,
			theme: OverlayTheme::default(),
		}
	}

	/// The collapsed title shown in the top border.
	pub fn title(&self) -> &str {
		&self.title
	}

	/// The provider the accounts belong to.
	pub fn provider_name(&self) -> &str {
		&self.provider_name
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// The picker options, for hosts that build the sheet themselves.
	pub fn picker_options(&self) -> &SelectPickerOptions {
		&self.options
	}

	/// The terminal list, for tests and hosts that drive it directly.
	pub fn select_list(&self) -> Ref<'_, SelectList> {
		self.list.borrow()
	}

	/// The terminal list, mutably.
	pub fn select_list_mut(&self) -> RefMut<'_, SelectList> {
		self.list.borrow_mut()
	}

	/// Route mouse selection through the title rows into the account list.
	pub fn route_mouse(&mut self, event: &MouseEvent, line: u16, col: u16) {
		route_select_list_mouse_with_top_border(
			&mut self.list.borrow_mut(),
			event,
			line,
			col,
		);
	}

	/// Picker pointer events drive the list exactly as its keys do.
	pub fn handle_native_event(&mut self, event: &crate::native::picker::PickerUiEvent) {
		route_select_list_picker(&mut self.list, event, "");
	}
}

impl Component for SessionAccountSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		let inner = (width as usize).saturating_sub(4).max(1) as u16;
		let mut rows = vec![
			top_border(width, &self.title, &self.theme),
			row("", width, &self.theme),
		];
		for line in self.list.borrow_mut().render(inner) {
			rows.push(row(&line, width, &self.theme));
		}
		rows.push(row("", width, &self.theme));
		rows.push(row("enter to pin · esc to cancel", width, &self.theme));
		rows.push(bottom_border(width, &self.theme));
		rows
	}

	fn handle_input(&mut self, key: &Key) {
		self.list.borrow_mut().handle_input(key);
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		self.route_mouse(event, event.row, event.column);
	}
}

impl NativeDescribe for SessionAccountSelectorComponent {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		if cx.feature("picker") {
			let props = select_list_picker(&self.list, &self.options).to_props();
			return docked_picker(props, None);
		}
		let items: Vec<NativeNode> = self
			.list
			.borrow()
			.visible_items()
			.iter()
			.map(|item| {
				let mut props = Props::new();
				props.set_spans(vec![Span::styled(
					item.title.clone(),
					if item.description.as_deref().is_some_and(|detail| !detail.is_empty()) {
						"accent"
					} else {
						"default"
					},
				)]);
				if let Some(description) = &item.description {
					props.set_str("detail", description.as_str());
				}
				crate::native::describe::item(item.key.clone(), props)
			})
			.collect();
		let mut list_props = Props::new();
		list_props.insert("selected", serde_json::json!(self.list.borrow().selected_key()));
		overlay_card(
			&self.role,
			Some(NodeText::plain(self.title.clone())),
			vec![
				node(NodeKind::List, list_props, items, Some("accounts")),
				text(
					NodeText::plain("enter to pin · esc to cancel"),
					Props::new(),
				),
			],
			None,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn account(id: i64, label: &str, active: bool) -> SessionPinAccount {
		SessionPinAccount {
			position: id as usize,
			credential_id: id,
			account_id: Some(format!("acct-{id}")),
			email: Some(format!("{label}@example.com")),
			label: label.to_string(),
			active,
			..SessionPinAccount::default()
		}
	}

	#[test]
	fn account_detail_names_the_org_project_and_host() {
		let mut account = account(1, "work", true);
		account.org_name = Some("Acme".to_string());
		account.org_id = Some("org-1".to_string());
		account.project_id = Some("proj-7".to_string());
		account.enterprise_url = Some("https://acme.example".to_string());
		assert_eq!(
			account_detail(&account),
			Some("Acme · proj-7 · https://acme.example".to_string())
		);
		// A part already present in the label is dropped: it tells nothing apart.
		account.label = "work proj-7".to_string();
		assert_eq!(
			account_detail(&account),
			Some("Acme · https://acme.example".to_string())
		);
		let mut bare = account(2, "personal", false);
		bare.org_name = None;
		bare.org_id = None;
		assert_eq!(account_detail(&bare), None);
	}

	#[test]
	fn rows_carry_credentials_and_mark_the_active_account() {
		let items = account_items(&[account(1, "work", true), account(2, "personal", false)]);
		assert_eq!(items[0].key, "1");
		assert_eq!(items[0].title, "work");
		assert_eq!(items[0].description.as_deref(), Some("active for this session"));
		assert_eq!(items[1].key, "2");
		assert_eq!(items[1].description, None);
		assert_eq!(account_max_visible(0), 1);
		assert_eq!(account_max_visible(4), 4);
		assert_eq!(account_max_visible(40), ACCOUNT_SELECTOR_MAX_VISIBLE);
	}

	#[test]
	fn the_picker_rests_on_the_active_account() {
		let component = SessionAccountSelectorComponent::new(
			"anthropic",
			&[account(1, "work", false), account(2, "personal", true)],
			|_| {},
			|| {},
		);
		assert_eq!(component.title(), "Select a anthropic account for this session");
		assert_eq!(
			component.select_list().selected_key().as_deref(),
			Some("2")
		);
		assert_eq!(component.picker_options().title, "anthropic account");
		assert_eq!(component.picker_options().confirm.as_deref(), Some("Pin"));
		assert_eq!(
			component.picker_options().current,
			Some(vec!["2".to_string()])
		);
	}

	#[test]
	fn activating_a_row_reports_its_account() {
		let picked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = std::rc::Rc::clone(&picked);
		let mut component = SessionAccountSelectorComponent::new(
			"openai",
			&[account(1, "work", true), account(2, "play", false)],
			move |account| sink.borrow_mut().push(account.label.clone()),
			|| {},
		);
		component.handle_native_event(&NativeUiEvent::Activate {
			key: Vec::new(),
			item: "2".to_string(),
		});
		assert_eq!(&*picked.borrow(), &["play".to_string()]);
		assert_eq!(component.select_list().selected_key().as_deref(), Some("2"));
	}

	#[test]
	fn the_native_picker_carries_the_confirm_and_detail() {
		let mut account = account(1, "work", true);
		account.org_name = Some("Acme".to_string());
		let component = SessionAccountSelectorComponent::new(
			"anthropic",
			std::slice::from_ref(&account),
			|_| {},
			|| {},
		);
		let mut cx = DescribeContext::new(80);
		cx.enable_feature("picker");
		let described = component.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Picker);
		let props = &described.props;
		assert!(props.get_str("confirm").is_some(), "confirm missing");
		let items = props.get("items").expect("picker items");
		let items = items.as_array().expect("items array");
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].get("id").and_then(|id| id.as_str()), Some("1"));
	}

	#[test]
	fn keyboard_cancel_flows_through_the_list() {
		let cancels = std::rc::Rc::new(std::cell::RefCell::new(0));
		let sink = std::rc::Rc::clone(&cancels);
		let mut component =
			SessionAccountSelectorComponent::new("openai", &[account(1, "work", true)], |_| {}, move || {
				*sink.borrow_mut() += 1;
			});
		component.handle_input(&Key::Esc);
		assert_eq!(*cancels.borrow(), 1);
	}
}
