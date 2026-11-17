//! Reference: `l/src/overlays/logout-account-selector.ts`.
//!
//! Account picker for `/logout` after the provider has been selected.
//!
//! Deviations:
//! - The reference extends `OverlayPanel` and adds the select list as a child;
//!   the crate keeps the rows, the list and the theme as fields and renders the
//!   panel chrome itself, mirroring [`crate::overlays::session_account_selector`].
//! - The picker's `badges` (`login` / `api key`) have no `SelectItem` slot yet;
//!   the credential type is instead carried by [`LogoutAccount::type_label`]
//!   and the detail line, which is what the terminal list shows anyway.
//! - `new` takes the accounts by value like the reference; `onSelect` receives
//!   the account and `onCancel` fires on Esc or the picker's close action.

use std::cell::{Ref, RefCell, RefMut};
use std::sync::Arc;

use crate::chrome::overlay_box::{OverlayTheme, bottom_border, row, top_border};
use crate::chrome::select_list_mouse_routing::route_select_list_mouse_with_top_border;
use crate::component::Component;
use crate::components::select_list::{SelectItem, SelectList, SelectListTheme};
use crate::keys::Key;
use crate::mouse::MouseEvent;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{NativeDescribe, item, node, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props, Span};
use crate::native::overlay::overlay_card;
use crate::native::picker::{
	PickerColumn, PickerList, PickerView, SelectPickerOptions, docked_picker,
	route_select_list_picker, select_list_picker,
};

/// Most accounts the picker shows at once.
pub const LOGOUT_SELECTOR_MAX_VISIBLE: usize = 10;

/// Semantic role of the picker panel.
pub const LOGOUT_SELECTOR_ROLE: &str = "omp.overlay.logout";

/// How a stored credential was stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogoutAccountType {
	#[default]
	ApiKey,
	Oauth,
}

impl LogoutAccountType {
	/// The picker badge text.
	#[must_use]
	pub fn label(self) -> &'static str {
		match self {
			LogoutAccountType::ApiKey => "api key",
			LogoutAccountType::Oauth => "login",
		}
	}

	/// The wire spelling.
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			LogoutAccountType::ApiKey => "api_key",
			LogoutAccountType::Oauth => "oauth",
		}
	}
}

/// Stored account identity rendered and matched by the logout picker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogoutAccount {
	/// Credential row id.
	pub credential_id: i64,
	/// Provider the credential belongs to.
	pub provider: String,
	/// Display label.
	pub label: String,
	/// What tells it apart from the provider's other accounts.
	pub detail: String,
	/// How the credential was stored.
	pub kind: LogoutAccountType,
	/// Whether this is the account the session currently uses.
	pub active: bool,
}

impl LogoutAccount {
	/// `label detail provider`, what the picker's filter matches against.
	#[must_use]
	pub fn search_text(&self) -> String {
		format!("{} {} {}", self.label, self.detail, self.provider)
	}
}

/// The account rows of the picker: the active one is described, the rest show
/// their detail line.
#[must_use]
pub fn account_items(accounts: &[LogoutAccount]) -> Vec<SelectItem> {
	accounts
		.iter()
		.map(|account| {
			let item = SelectItem::new(account.credential_id.to_string(), account.label.clone());
			let detail = account.detail.trim();
			match (account.active, detail.is_empty()) {
				(true, true) => item.with_description("active"),
				(true, false) => item.with_description(format!("active · {detail}")),
				(false, true) => item,
				(false, false) => item.with_description(detail),
			}
		})
		.collect()
}

/// Most rows to show for `count` accounts, never below one.
#[must_use]
pub fn account_max_visible(count: usize) -> usize {
	count.clamp(1, LOGOUT_SELECTOR_MAX_VISIBLE)
}

/// Shared list handle so `describe_native` can borrow it while the component
/// also yields it for mouse routing.
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
		self.inner.borrow_mut().handle_input(&Key::Esc);
	}
}

/// The logout account picker panel.
pub struct LogoutAccountSelectorComponent {
	title: String,
	role: String,
	provider_name: String,
	accounts: Vec<LogoutAccount>,
	list: SheetList,
	options: SelectPickerOptions,
	theme: OverlayTheme,
}

impl std::fmt::Debug for LogoutAccountSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LogoutAccountSelectorComponent")
			.field("title", &self.title)
			.field("role", &self.role)
			.finish()
	}
}

impl LogoutAccountSelectorComponent {
	/// Build the picker over `accounts`, resting on the active one.
	pub fn new(
		provider_name: &str,
		accounts: Vec<LogoutAccount>,
		on_select: impl FnMut(&LogoutAccount) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		let mut list = SelectList::new(
			account_items(&accounts),
			account_max_visible(accounts.len()),
			SelectListTheme::default(),
		);
		if let Some(active) = accounts.iter().position(|account| account.active) {
			list.set_selected_index(active as isize);
		}
		let by_value: std::collections::HashMap<String, LogoutAccount> = accounts
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
			title: format!("{provider_name} accounts"),
			subtitle: Some("Pick the account to sign out".to_string()),
			icon: Some("key-round".to_string()),
			noun: Some("accounts".to_string()),
			searchable: false,
			current: Some(
				accounts
					.iter()
					.filter(|account| account.active)
					.map(|account| account.credential_id.to_string())
					.collect(),
			),
			confirm: Some("Sign out".to_string()),
			decorate: Some(Arc::new(move |item: &SelectItem| {
				let account = decorate_by_value.get(&item.key)?;
				let mut props = Props::new();
				props.set_str("detail", account.detail.clone());
				props.set_str("badge", account.kind.label());
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
		};

		LogoutAccountSelectorComponent {
			title: format!("Select {provider_name} account to log out"),
			role: LOGOUT_SELECTOR_ROLE.to_string(),
			provider_name: provider_name.to_string(),
			accounts,
			list: SheetList::new(list),
			options,
			theme: OverlayTheme::default(),
		}
	}

	/// The collapsed title shown in the top border.
	#[must_use]
	pub fn title(&self) -> &str {
		&self.title
	}

	/// The provider the accounts belong to.
	#[must_use]
	pub fn provider_name(&self) -> &str {
		&self.provider_name
	}

	/// The semantic role reported to the native host.
	#[must_use]
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// The accounts offered, as built.
	#[must_use]
	pub fn accounts(&self) -> &[LogoutAccount] {
		&self.accounts
	}

	/// The picker options, for hosts that build the sheet themselves.
	#[must_use]
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
		route_select_list_mouse_with_top_border(&mut self.list.borrow_mut(), event, line, col);
	}

	/// Picker pointer events drive the list exactly as its keys do.
	pub fn handle_native_event(&mut self, event: &crate::native::picker::PickerUiEvent) {
		route_select_list_picker(&mut self.list, event, "");
	}
}

impl Component for LogoutAccountSelectorComponent {
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
		rows.push(row(
			"enter to log out account · esc to cancel",
			width,
			&self.theme,
		));
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

impl NativeDescribe for LogoutAccountSelectorComponent {
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
			.map(|entry| {
				let mut props = Props::new();
				props.set_spans(vec![Span::styled(
					entry.title.clone(),
					entry
						.description
						.as_deref()
						.is_some_and(|detail| !detail.is_empty())
						.then_some("accent")
						.unwrap_or("default"),
				)]);
				if let Some(detail) = &entry.description {
					props.set_str("detail", detail.as_str());
				}
				item(entry.key.clone(), props)
			})
			.collect();
		let mut list_props = Props::new();
		list_props.insert("selected", serde_json::json!(self.list.borrow().selected_key()));
		list_props.set_i64("max", LOGOUT_SELECTOR_MAX_VISIBLE as i64);
		list_props.set_str("empty", "No stored accounts to log out");
		overlay_card(
			&self.role,
			Some(NodeText::plain(self.title.clone())),
			vec![
				node(NodeKind::List, list_props, items, Some("list")),
				text(
					NodeText::plain("enter to log out account · esc to cancel"),
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

	fn account(id: i64, label: &str, active: bool) -> LogoutAccount {
		LogoutAccount {
			credential_id: id,
			provider: "anthropic".to_string(),
			label: label.to_string(),
			detail: format!("{label}@example.com"),
			kind: LogoutAccountType::Oauth,
			active,
		}
	}

	#[test]
	fn rows_carry_credentials_and_mark_the_active_account() {
		let items = account_items(&[account(1, "work", true), account(2, "play", false)]);
		assert_eq!(items[0].key, "1");
		assert_eq!(items[0].title, "work");
		assert_eq!(items[0].description.as_deref(), Some("active · work@example.com"));
		assert_eq!(items[1].key, "2");
		assert_eq!(items[1].description.as_deref(), Some("play@example.com"));
		assert_eq!(account_max_visible(0), 1);
		assert_eq!(account_max_visible(40), LOGOUT_SELECTOR_MAX_VISIBLE);
	}

	#[test]
	fn search_text_joins_label_detail_and_provider() {
		let mut account = account(1, "work", false);
		account.provider = "openai".to_string();
		assert_eq!(account.search_text(), "work work@example.com openai");
		assert_eq!(LogoutAccountType::Oauth.as_str(), "oauth");
		assert_eq!(LogoutAccountType::ApiKey.label(), "api key");
	}

	#[test]
	fn the_picker_rests_on_the_active_account_and_says_sign_out() {
		let component = LogoutAccountSelectorComponent::new(
			"anthropic",
			vec![account(1, "work", false), account(2, "play", true)],
			|_| {},
			|| {},
		);
		assert_eq!(component.title(), "Select anthropic account to log out");
		assert_eq!(component.select_list().selected_key().as_deref(), Some("2"));
		assert_eq!(component.picker_options().title, "anthropic accounts");
		assert_eq!(component.picker_options().confirm.as_deref(), Some("Sign out"));
		assert_eq!(
			component.picker_options().current,
			Some(vec!["2".to_string()])
		);
	}

	#[test]
	fn activating_a_row_reports_its_account() {
		let picked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
		let sink = std::rc::Rc::clone(&picked);
		let mut component = LogoutAccountSelectorComponent::new(
			"openai",
			vec![account(1, "work", true), account(2, "play", false)],
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
	fn cancel_fires_on_escape() {
		let cancels = std::rc::Rc::new(std::cell::RefCell::new(0));
		let sink = std::rc::Rc::clone(&cancels);
		let mut component = LogoutAccountSelectorComponent::new(
			"openai",
			vec![account(1, "work", true)],
			|_| {},
			move || *sink.borrow_mut() += 1,
		);
		component.handle_input(&Key::Esc);
		assert_eq!(*cancels.borrow(), 1);
	}

	#[test]
	fn the_native_picker_and_card_keep_their_chrome() {
		let component =
			LogoutAccountSelectorComponent::new("anthropic", vec![account(1, "work", true)], |_| {}, || {});
		let mut cx = DescribeContext::new(80);
		cx.enable_feature("picker");
		let described = component.describe_native(&cx);
		assert_eq!(described.kind, NodeKind::Picker);
		let items = described
			.props
			.get("items")
			.expect("picker items")
			.as_array()
			.expect("items array")
			.clone();
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].get("id").and_then(|id| id.as_str()), Some("1"));

		let plain = component.describe_native(&DescribeContext::new(80));
		assert_eq!(plain.kind, NodeKind::Overlay);
		assert!(plain.props.get_str("empty").is_some());
	}

	#[test]
	fn render_draws_the_title_and_footer() {
		let mut component =
			LogoutAccountSelectorComponent::new("anthropic", vec![account(1, "work", true)], |_| {}, || {});
		let frame = component.render(60);
		assert!(frame.iter().any(|row| row.contains("Select anthropic account")));
		assert!(frame
			.iter()
			.any(|row| row.contains("enter to log out account")));
	}
}
