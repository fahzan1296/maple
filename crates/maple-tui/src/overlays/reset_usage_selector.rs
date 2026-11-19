//! Reference: `l/src/overlays/reset-usage-selector.ts`.
//!
//! Account picker for `/usage reset`: lists provider accounts with their saved
//! rate-limit reset counts; selecting one redeems a reset. Because a reset is a
//! scarce, irreversible credit, Enter requires a second press to confirm.
//!
//! Deviations:
//! - The reference reads the process-global theme and keybindings; this port
//!   takes [`KeybindingsManager`] (and an epoch-millisecond `now`) from the
//!   host. Text-mode rows carry no ANSI styling: the reference paints them with
//!   the global theme, while here the native description carries the tone
//!   tokens and the host paints the rows.
//! - `UsageResetCreditDetail` here carries the fields this selector reads
//!   (`title`, `program`, `clears`, `requiresLimit`); the crate's
//!   [`crate::overlays::usage_display::UsageResetCreditDetail`] is the
//!   count/expiry view used by the usage report.
//! - `expiresAt` (an ISO string in the reference) is an epoch-millisecond
//!   timestamp, matching every other instant in maple-tui.
//! - The native description is rebuilt on demand instead of memoized.

use crate::chrome::overlay_box::{bottom_border, row, top_border, OverlayTheme};
use crate::component::Component;
use crate::components::menu_selection::{MenuActivation, MenuSource, MenuState};
use crate::keybindings::KeybindingsManager;
use crate::keybinding_matchers::{
	matches_select_cancel, matches_select_down, matches_select_page_down, matches_select_page_up,
	matches_select_up,
};
use crate::keys::{self, Key};
use crate::native::describe::NativeDescribe;
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Span};
use crate::native::overlay::{
	action_hint, format_tooltip_key, hints_row, overlay_card,
};
use crate::native::picker::{
	docked_picker, picker_action, PickerActionOptions, PickerColumn, PickerConfirm, PickerItem,
	PickerKeys, PickerProps, PickerUiEvent, CLOSE_ACTION,
};
use crate::native::node::Text as NodeText;
use crate::overlays::extensions::display_text::sanitize_display_single_line;
use crate::overlays::usage_display::format_usage_reset_window;
use crate::status_line::segments::format_duration;

/// Most account rows the picker lists at once.
pub const RESET_SELECTOR_MAX_VISIBLE: usize = 10;

/// Collapse control characters so one row stays one line.
fn one_line(value: &str) -> String {
	let flattened: String = value
		.chars()
		.map(|character| match character {
			'\r' | '\n' | '\t' => ' ',
			other => other,
		})
		.collect();
	sanitize_display_single_line(&flattened)
}

/// Detail of the reset credit a spend would consume.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetCreditDetail {
	/// Human title of the credit.
	pub title: Option<String>,
	/// Provider program the credit belongs to (`"juniper_tide"`, …).
	pub program: Option<String>,
	/// Window ids the credit clears.
	pub clears: Vec<String>,
	/// `false` when the credit may be spent before the covered limit is hit.
	pub requires_limit: Option<bool>,
}

/// Which credential an account row spends against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetUsageTarget {
	/// Credential id of the account.
	pub credential_id: u64,
	/// Provider the credential belongs to.
	pub provider: String,
	/// Banked credit the row spends.
	pub credit_id: Option<String>,
	/// Provider account id.
	pub account_id: Option<String>,
	/// Account email.
	pub email: Option<String>,
	/// Owning organisation.
	pub org_id: Option<String>,
}

/// One account row with its redeemable rate-limit reset credits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResetUsageAccount {
	/// Display label.
	pub label: String,
	/// Provider id.
	pub provider: String,
	/// Provider display name.
	pub provider_label: String,
	/// Banked resets, including grants that cannot be spent right now.
	pub available_count: u32,
	/// Resets the provider currently permits this account to spend.
	pub redeemable_count: u32,
	/// Credential the resets belong to.
	pub target: ResetUsageTarget,
	/// Whether this is the account in use.
	pub active: bool,
	/// Why the account's resets could not be read.
	pub error: Option<String>,
	/// Why the banked resets cannot be spent.
	pub unavailable_reason: Option<String>,
	/// When the banked resets expire (epoch milliseconds).
	pub expires_at: Option<i64>,
	/// The credit a spend would consume.
	pub credit: Option<ResetCreditDetail>,
}

impl ResetUsageAccount {
	/// Stable menu identity of the row.
	#[must_use]
	pub fn key(&self) -> String {
		format!(
			"{}:{}:{}",
			self.provider,
			self.target.credential_id,
			self.target.credit_id.as_deref().unwrap_or("")
		)
	}
}

/// Reset count, usability, expiry and unavailability of an account row, or its
/// error.
#[must_use]
pub fn account_count_label(account: &ResetUsageAccount, now_ms: i64) -> String {
	if let Some(error) = &account.error {
		return one_line(error);
	}
	let mut label = format!(
		"{} saved reset{}",
		account.available_count,
		if account.available_count == 1 { "" } else { "s" }
	);
	if account.redeemable_count != account.available_count {
		label.push_str(&format!(" · {} usable now", account.redeemable_count));
	}
	if let Some(expires_at) = account.expires_at {
		if expires_at > now_ms {
			label.push_str(&format!(" · expires in {}", format_duration((expires_at - now_ms) as u64)));
		} else {
			label.push_str(" · expired");
		}
	}
	if account.redeemable_count == 0 {
		if let Some(reason) = &account.unavailable_reason {
			label.push_str(&format!(" · {}", one_line(reason)));
		}
	}
	label
}

/// The picker row of an account: provider mark, identity, its resets as a fact,
/// unusable ones disabled with the reason.
#[must_use]
pub fn account_picker_item(account: &ResetUsageAccount, id: &str, now_ms: i64) -> PickerItem {
	let redeemable = account.redeemable_count > 0;
	let mut detail = vec![Span::styled(
		format!(
			"{} · #{}",
			one_line(&account.provider_label),
			account.target.credential_id
		),
		"muted",
	)];
	if account.active {
		detail.push(Span::styled(" (active)", "muted"));
	}
	let reason = account.error.clone().or_else(|| {
		if redeemable {
			None
		} else {
			Some(
				account
					.unavailable_reason
					.clone()
					.unwrap_or_else(|| "no resets usable now".to_string()),
			)
		}
	});
	if let Some(reason) = &reason {
		detail.push(Span::styled(
			format!(" · {}", one_line(reason)),
			if account.error.is_some() { "error" } else { "warning" },
		));
	}

	let resets = if account.error.is_some() {
		Span::styled("unavailable", "error")
	} else if !redeemable {
		Span::styled(format!("{} saved", account.available_count), "dim")
	} else if account.redeemable_count == account.available_count {
		Span::styled(format!("{} usable", account.redeemable_count), "success")
	} else {
		Span::styled(
			format!("{} of {} usable", account.redeemable_count, account.available_count),
			"success",
		)
	};
	let mut facts = serde_json::Map::new();
	facts.insert(
		"resets".to_string(),
		serde_json::Value::Array(vec![resets.to_value()]),
	);
	if let Some(expires_at) = account.expires_at {
		let span = if expires_at > now_ms {
			Span::styled(format!("expires in {}", format_duration((expires_at - now_ms) as u64)), "dim")
		} else {
			Span::styled("expired", "warning")
		};
		facts.insert(
			"expires".to_string(),
			serde_json::Value::Array(vec![span.to_value()]),
		);
	}

	let mut rest = Props::new();
	rest.insert(
		"detail",
		serde_json::Value::Array(detail.iter().map(Span::to_value).collect()),
	);
	rest.insert(
		"mark",
		serde_json::json!({ "text": account.provider_label, "seed": account.provider }),
	);
	rest.insert("facts", serde_json::Value::Object(facts));
	if let Some(reason) = reason {
		rest.set_str("disabled", one_line(&reason));
	}
	PickerItem {
		id: id.to_string(),
		label: one_line(&account.label),
		rest,
	}
}

/// Menu identity, search text and confirmation rule of an account row.
struct AccountSource;

impl MenuSource<ResetUsageAccount> for AccountSource {
	fn key(&self, item: &ResetUsageAccount) -> String {
		item.key()
	}

	fn search_text(&self, item: &ResetUsageAccount) -> String {
		format!("{} {}", item.label, item.provider_label)
	}

	fn requires_confirmation(&self, item: &ResetUsageAccount) -> bool {
		item.redeemable_count > 0
	}
}

/// Account picker for `/usage reset`.
pub struct ResetUsageSelectorComponent {
	title: String,
	role: String,
	menu: MenuState<ResetUsageAccount>,
	status_message: Option<String>,
	on_select: Box<dyn FnMut(&ResetUsageAccount)>,
	on_cancel: Box<dyn FnMut()>,
	now_ms: i64,
	bindings: KeybindingsManager,
	theme: OverlayTheme,
}

impl std::fmt::Debug for ResetUsageSelectorComponent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ResetUsageSelectorComponent")
			.field("title", &self.title)
			.field("status_message", &self.status_message)
			.finish()
	}
}

impl ResetUsageSelectorComponent {
	/// Build the picker over `accounts`, resolving expiries against `now_ms`.
	pub fn new(
		accounts: Vec<ResetUsageAccount>,
		now_ms: i64,
		on_select: impl FnMut(&ResetUsageAccount) + 'static,
		on_cancel: impl FnMut() + 'static,
	) -> Self {
		let first_key = accounts
			.iter()
			.find(|account| account.redeemable_count > 0)
			.map(|account| account.key());
		let mut menu = MenuState::new(accounts, Box::new(AccountSource));
		if let Some(key) = first_key {
			menu.set_selected_key(&key);
		}
		ResetUsageSelectorComponent {
			title: "Spend a saved rate-limit reset".to_string(),
			role: "omp.overlay.reset-usage".to_string(),
			menu,
			status_message: None,
			on_select: Box::new(on_select),
			on_cancel: Box::new(on_cancel),
			now_ms,
			bindings: KeybindingsManager::new(),
			theme: OverlayTheme::default(),
		}
	}

	/// Use a host keybinding set for navigation and hints.
	pub fn set_keybindings(&mut self, bindings: KeybindingsManager) {
		self.bindings = bindings;
	}

	/// The collapsed title shown in the top border.
	pub fn title(&self) -> &str {
		&self.title
	}

	/// The semantic role reported to the native host.
	pub fn native_role(&self) -> &str {
		&self.role
	}

	/// The account menu state.
	pub fn menu(&self) -> &MenuState<ResetUsageAccount> {
		&self.menu
	}

	/// The account menu state, mutably.
	pub fn menu_mut(&mut self) -> &mut MenuState<ResetUsageAccount> {
		&mut self.menu
	}

	/// The warning line shown under the list, if any.
	pub fn status_message(&self) -> Option<&str> {
		self.status_message.as_deref()
	}

	/// What a reset of `account` spends and covers, without key instructions.
	#[must_use]
	pub fn confirmation_notes(&self, account: &ResetUsageAccount) -> (String, Vec<String>) {
		let subject = account
			.credit
			.as_ref()
			.and_then(|credit| credit.title.as_ref())
			.map(|title| format!("“{title}”"))
			.unwrap_or_else(|| "1 saved reset".to_string());
		let mut notes = Vec::new();
		if let Some(credit) = &account.credit {
			if credit.program.as_deref() == Some("juniper_tide") {
				notes.push("This resets Claude's 5h session limit only; weekly limits stay unchanged.".to_string());
			} else if !credit.clears.is_empty() {
				let windows: Vec<String> = credit
					.clears
					.iter()
					.map(|window| format_usage_reset_window(window))
					.collect();
				notes.push(format!("Covers {}.", windows.join(", ")));
			}
			if credit.requires_limit == Some(false) {
				notes.push("Optional early use: this can be spent before the covered limit is reached.".to_string());
			}
		}
		(subject, notes)
	}

	/// The confirmation line for `account`: what a second Enter spends.
	#[must_use]
	pub fn confirmation_message(&self, account: &ResetUsageAccount) -> String {
		let (subject, notes) = self.confirmation_notes(account);
		let cancel = self.hint_key("tui.select.cancel");
		let mut message = format!(
			"Press {} again to spend {} for {} ({}).",
			format_tooltip_key("enter"),
			subject,
			account.label,
			account.provider_label
		);
		for note in notes {
			message.push(' ');
			message.push_str(&note);
		}
		message.push_str(&format!(" {cancel} cancels."));
		message
	}

	fn hint_key(&self, action: &str) -> String {
		self.bindings
			.get_keys(action)
			.first()
			.map(|key| format_tooltip_key(key))
			.unwrap_or_default()
	}

	fn hint_line(&self) -> String {
		let up = self.hint_key("tui.select.up");
		let down = self.hint_key("tui.select.down");
		format!(
			"{up}/{down} select · {} spend a reset · {} cancel",
			format_tooltip_key("enter"),
			self.hint_key("tui.select.cancel")
		)
	}

	/// The account row texts in view order.
	#[must_use]
	pub fn visible_rows(&self) -> Vec<String> {
		let mut rows = Vec::with_capacity(self.menu.visible_items().len());
		for (index, account) in self.menu.visible_items().iter().enumerate() {
			let selected = index as isize == self.menu.selected_index();
			let cursor = if selected { "› " } else { "  " };
			let active = if account.active { " (active)" } else { "" };
			rows.push(format!(
				"{cursor}[{} · #{}] {}{active}  {}",
				one_line(&account.provider_label),
				account.target.credential_id,
				one_line(&account.label),
				account_count_label(account, self.now_ms)
			));
		}
		rows
	}

	/// The rows the list may show, centred on the selection.
	fn list_rows(&self) -> Vec<String> {
		let rows = self.visible_rows();
		let total = rows.len();
		if total == 0 {
			return vec!["No provider accounts with saved resets".to_string()];
		}
		let selected = self.menu.selected_index().max(0) as usize;
		let max_visible = RESET_SELECTOR_MAX_VISIBLE;
		let window = crate::components::scroll_viewport::centered_viewport_range(selected, total, max_visible);
		rows[window.start..window.end].to_vec()
	}

	fn body_rows(&self) -> Vec<String> {
		let mut rows = self.list_rows();
		if let Some(pending) = self.pending_account() {
			rows.push(self.confirmation_message(pending));
		} else {
			rows.push(self.hint_line());
		}
		if let Some(status) = &self.status_message {
			rows.push(String::new());
			rows.push(one_line(status));
		}
		rows
	}

	fn pending_account(&self) -> Option<&ResetUsageAccount> {
		self.menu
			.visible_items()
			.iter()
			.find(|item| self.menu.is_pending(item))
	}

	/// Text-mode rows for `width`, including the rounded border chrome.
	pub fn rows(&self, width: u16) -> Vec<String> {
		let mut rows = vec![top_border(width, &self.title, &self.theme)];
		for line in self.body_rows() {
			rows.push(row(&line, width, &self.theme));
		}
		rows.push(bottom_border(width, &self.theme));
		rows
	}

	/// Spend the selected account's reset, or explain why it cannot be spent.
	pub fn activate_selection(&mut self) {
		let Some(account) = self.menu.selected_item().cloned() else {
			return;
		};
		if account.redeemable_count == 0 {
			self.status_message = Some(match &account.unavailable_reason {
				Some(reason) => format!("That account's saved resets are unavailable: {reason}"),
				None => "That account has no saved resets usable right now.".to_string(),
			});
			return;
		}
		let index = self.menu.selected_index();
		match self.menu.request_activation(index) {
			MenuActivation::Confirmed(account) => {
				self.status_message = None;
				(self.on_select)(&account);
			}
			MenuActivation::Pending(_) => {
				self.status_message = None;
			}
			MenuActivation::Disabled(account) => {
				self.status_message = Some(format!(
					"That account has no saved resets usable right now: {}",
					account.label
				));
			}
			MenuActivation::Empty => {
				self.status_message = None;
			}
		}
	}

	/// The docked picker: accounts as provider-marked cards with their resets as
	/// facts; a first `Spend reset` turns the selected row into a confirm strip.
	#[must_use]
	pub fn picker_props(&self) -> PickerProps {
		let items: Vec<PickerItem> = self
			.menu
			.visible_items()
			.iter()
			.map(|account| account_picker_item(account, &account.key(), self.now_ms))
			.collect();
		let pending = self.pending_account();
		let mut confirm = None;
		let mut subtitle = Some("Spend a saved reset to refresh an account's rate limit now".to_string());
		if let Some(pending) = pending {
			let (subject, notes) = self.confirmation_notes(pending);
			confirm = Some(PickerConfirm {
				text: format!("Spend {subject} on {}?", one_line(&pending.label)),
				act: "confirm".to_string(),
				label: "Spend".to_string(),
			});
			if !notes.is_empty() {
				subtitle = Some(one_line(&notes.join(" ")));
			}
		}
		if let Some(status) = &self.status_message {
			subtitle = Some(one_line(status));
		}
		PickerProps {
			title: "Rate-limit resets".to_string(),
			subtitle,
			icon: Some("refresh".to_string()),
			noun: "accounts".to_string(),
			size: "md".to_string(),
			layout: "cards".to_string(),
			preview: "none".to_string(),
			query: None,
			cursor: None,
			items,
			selected: self.menu.selected_key(),
			current: Some(
				self.menu
					.visible_items()
					.iter()
					.filter(|account| account.active)
					.map(ResetUsageAccount::key)
					.collect(),
			),
			columns: Some(vec![
				PickerColumn {
					id: "expires".to_string(),
					head: None,
					format: None,
					align: None,
					grow: None,
					priority: Some(1),
				},
				PickerColumn {
					id: "resets".to_string(),
					head: None,
					format: None,
					align: None,
					grow: None,
					priority: Some(2),
				},
			]),
			actions: vec![
				picker_action(
					"confirm",
					"Spend reset",
					Some(PickerKeys::Id("enter")),
					PickerActionOptions {
						primary: true,
						..PickerActionOptions::default()
					},
				),
				(*CLOSE_ACTION).clone(),
			],
			confirm,
		}
	}

	/// Point the menu at `key`; moving to another account drops a pending
	/// confirmation.
	pub fn select_key(&mut self, key: &str) -> bool {
		if self.menu.selected_key().as_deref() == Some(key) {
			return true;
		}
		self.menu.cancel_confirmation();
		self.menu.set_selected_key(key);
		if self.menu.selected_key().as_deref() != Some(key) {
			return false;
		}
		self.status_message = None;
		true
	}

	/// Route a native picker event: a row click selects, `confirm` presses Enter,
	/// `cancel` drops the confirmation, `close` cancels the picker.
	pub fn handle_native_event(&mut self, event: &PickerUiEvent) -> bool {
		match event {
			PickerUiEvent::Action { act, .. } => match act.as_str() {
				"confirm" => self.activate_selection(),
				"cancel" => {
					if self.menu.cancel_confirmation() {
						self.status_message = None;
					} else {
						(self.on_cancel)();
					}
				}
				"close" => (self.on_cancel)(),
				_ => return false,
			},
			PickerUiEvent::Select { item, .. } | PickerUiEvent::Activate { item, .. } => {
				let activate = matches!(event, PickerUiEvent::Activate { .. });
				if self.select_key(item) && activate {
					self.activate_selection();
				}
			}
			_ => return false,
		}
		true
	}
}

impl Component for ResetUsageSelectorComponent {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.rows(width)
	}

	fn handle_input(&mut self, key: &Key) {
		if matches_select_cancel(key, &self.bindings) {
			if self.menu.cancel_confirmation() {
				self.status_message = None;
				return;
			}
			(self.on_cancel)();
			return;
		}
		if matches_select_up(key, &self.bindings) {
			self.menu.cancel_confirmation();
			self.menu.move_selection(-1, true);
			self.status_message = None;
		} else if matches_select_down(key, &self.bindings) {
			self.menu.cancel_confirmation();
			self.menu.move_selection(1, true);
			self.status_message = None;
		} else if matches_select_page_up(key, &self.bindings) {
			self.menu.cancel_confirmation();
			self.menu.move_selection(-(RESET_SELECTOR_MAX_VISIBLE as isize), false);
			self.status_message = None;
		} else if matches_select_page_down(key, &self.bindings) {
			self.menu.cancel_confirmation();
			self.menu.move_selection(RESET_SELECTOR_MAX_VISIBLE as isize, false);
			self.status_message = None;
		} else if keys::is_enter(key) {
			self.activate_selection();
		}
	}
}

impl NativeDescribe for ResetUsageSelectorComponent {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		if cx.feature("picker") {
			return docked_picker(self.picker_props().to_props(), None);
		}
		let items: Vec<NativeNode> = self
			.menu
			.visible_items()
			.iter()
			.map(|account| {
				let redeemable = account.redeemable_count > 0;
				let mut props = Props::new();
				props.set_spans(vec![Span::styled(
					one_line(&account.label),
					if redeemable { "accent" } else { "dim" },
				)]);
				props.set_i64("value", account.redeemable_count.into());
				crate::native::describe::node(
					NodeKind::Item,
					props,
					Vec::new(),
					Some(account.key().as_str()),
				)
			})
			.collect();
		let mut list_props = Props::new();
		list_props.insert("selected", serde_json::json!(self.menu.selected_key()));
		list_props.set_i64("max", RESET_SELECTOR_MAX_VISIBLE as i64);
		let list = crate::native::describe::node(NodeKind::List, list_props, items, Some("list"));

		let footer = match self.pending_account() {
			Some(pending) => crate::native::describe::text(
				NodeText::plain(self.confirmation_message(pending)),
				Props::new(),
			),
			None => hints_row(
				&[
					action_hint(&self.bindings, &["tui.select.up", "tui.select.down"], "select"),
					Some(crate::native::overlay::NativeHint {
						keys: vec!["enter".to_string()],
						label: "spend a reset".to_string(),
					}),
					action_hint(&self.bindings, "tui.select.cancel", "cancel"),
				],
				"reset-usage-hints",
			),
		};
		let mut children = vec![list, footer];
		if let Some(status) = &self.status_message {
			children.push(crate::native::describe::text(
				NodeText::plain(one_line(status)),
				Props::new(),
			));
		}
		overlay_card(
			&self.role,
			Some(NodeText::plain(self.title.clone())),
			children,
			None,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;
	use std::rc::Rc;

	const NOW: i64 = 1_700_000_000_000;

	fn account(name: &str, credential: u64, available: u32, redeemable: u32) -> ResetUsageAccount {
		ResetUsageAccount {
			label: name.to_string(),
			provider: "claude".to_string(),
			provider_label: "Claude".to_string(),
			available_count: available,
			redeemable_count: redeemable,
			target: ResetUsageTarget {
				credential_id: credential,
				provider: "claude".to_string(),
				..ResetUsageTarget::default()
			},
			..ResetUsageAccount::default()
		}
	}

	fn fixture() -> Vec<ResetUsageAccount> {
		let mut unusable = account("Spent", 3, 2, 0);
		unusable.unavailable_reason = Some("grants are locked".to_string());
		vec![account("Primary", 1, 2, 1), account("Backup", 2, 1, 1), unusable]
	}

	#[test]
	fn count_label_reports_counts_usability_and_expiry() {
		let mut row = account("A", 1, 2, 1);
		assert_eq!(account_count_label(&row, NOW), "2 saved resets · 1 usable now");
		row.expires_at = Some(NOW + 3_600_000);
		assert_eq!(
			account_count_label(&row, NOW),
			"2 saved resets · 1 usable now · expires in 1h"
		);
		row.expires_at = Some(NOW - 1);
		assert_eq!(account_count_label(&row, NOW), "2 saved resets · 1 usable now · expired");
	}

	#[test]
	fn count_label_reports_errors_and_unavailability() {
		let mut row = account("A", 1, 0, 0);
		row.unavailable_reason = Some("locked".to_string());
		assert_eq!(account_count_label(&row, NOW), "0 saved resets · locked");
		row.error = Some("credential\nrevoked".to_string());
		assert_eq!(account_count_label(&row, NOW), "credential revoked");
	}

	#[test]
	fn picker_item_carries_mark_facts_and_disabled_reason() {
		let mut usable = account("Primary", 1, 2, 2);
		usable.active = true;
		let item = account_picker_item(&usable, &usable.key(), NOW);
		assert_eq!(item.id, "claude:1:");
		assert_eq!(item.label, "Primary");
		let facts = item.rest.get("facts").expect("facts");
		assert!(facts.get("resets").is_some());
		assert_eq!(item.rest.get("mark").and_then(|mark| mark.get("seed")).and_then(|v| v.as_str()), Some("claude"));

		let mut unusable = account("Backup", 2, 1, 0);
		unusable.unavailable_reason = Some("locked".to_string());
		let item = account_picker_item(&unusable, &unusable.key(), NOW);
		assert_eq!(item.rest.get_str("disabled"), Some("locked"));
	}

	#[test]
	fn starts_on_the_first_redeemable_account() {
		let mut selector = ResetUsageSelectorComponent::new(fixture(), NOW, |_| {}, || {});
		assert_eq!(selector.menu().selected_key().as_deref(), Some("claude:1:"));
		selector.handle_input(&Key::Down);
		assert_eq!(selector.menu().selected_key().as_deref(), Some("claude:2:"));
	}

	#[test]
	fn enter_confirms_before_spending() {
		let spent = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&spent);
		let mut selector = ResetUsageSelectorComponent::new(
			fixture(),
			NOW,
			move |account| sink.borrow_mut().push(account.label.clone()),
			|| {},
		);
		selector.handle_input(&Key::Enter);
		assert!(spent.borrow().is_empty());
		assert!(selector.menu().is_pending(&fixture()[0]));
		selector.handle_input(&Key::Enter);
		assert_eq!(&*spent.borrow(), &["Primary".to_string()]);
	}

	#[test]
	fn moving_away_drops_the_pending_confirmation() {
		let mut selector = ResetUsageSelectorComponent::new(fixture(), NOW, |_| {}, || {});
		selector.handle_input(&Key::Enter);
		assert!(selector.menu().pending_key().is_some());
		selector.handle_input(&Key::Down);
		assert!(selector.menu().pending_key().is_none());
		assert!(selector.menu().selected_key().as_deref() == Some("claude:2:"));
	}

	#[test]
	fn unusable_accounts_explain_instead_of_spending() {
		let spent = Rc::new(RefCell::new(0usize));
		let sink = Rc::clone(&spent);
		let mut selector = ResetUsageSelectorComponent::new(
			fixture(),
			NOW,
			move |_| *sink.borrow_mut() += 1,
			|| {},
		);
		selector.select_key("claude:3:");
		selector.handle_input(&Key::Enter);
		assert_eq!(*spent.borrow(), 0);
		assert_eq!(
			selector.status_message(),
			Some("That account's saved resets are unavailable: grants are locked")
		);
	}

	#[test]
	fn escape_clears_the_confirmation_then_cancels() {
		let cancelled = Rc::new(RefCell::new(0usize));
		let sink = Rc::clone(&cancelled);
		let mut selector = ResetUsageSelectorComponent::new(fixture(), NOW, |_| {}, move || {
			*sink.borrow_mut() += 1;
		});
		selector.handle_input(&Key::Enter);
		selector.handle_input(&Key::Esc);
		assert_eq!(*cancelled.borrow(), 0);
		assert!(selector.menu().pending_key().is_none());
		selector.handle_input(&Key::Esc);
		assert_eq!(*cancelled.borrow(), 1);
	}

	#[test]
	fn confirmation_message_names_subject_notes_and_cancel_key() {
		let mut selector = ResetUsageSelectorComponent::new(fixture(), NOW, |_| {}, || {});
		let mut row = account("Primary", 1, 1, 1);
		row.credit = Some(ResetCreditDetail {
			title: Some("Weekly Boost".to_string()),
			program: Some("default".to_string()),
			clears: vec!["five_hour".to_string(), "seven_day_opus".to_string()],
			requires_limit: Some(false),
		});
		let message = selector.confirmation_message(&row);
		assert!(message.contains("“Weekly Boost”"), "{message}");
		assert!(message.contains("Covers 5-hour quota, Opus quota."), "{message}");
		assert!(message.contains("Optional early use"), "{message}");
		assert!(message.contains("cancels."), "{message}");
		let (subject, notes) = selector.confirmation_notes(&fixture()[0]);
		assert_eq!(subject, "1 saved reset");
		assert!(notes.is_empty());
		selector.handle_input(&Key::Down);
		assert_eq!(selector.menu().selected_index(), 1);
	}

	#[test]
	fn picker_props_describe_cards_with_an_expiry_column() {
		let selector = ResetUsageSelectorComponent::new(fixture(), NOW, |_| {}, || {});
		let props = selector.picker_props();
		assert_eq!(props.title, "Rate-limit resets");
		assert_eq!(props.layout, "cards");
		assert_eq!(props.items.len(), 3);
		let columns = props.columns.expect("columns");
		assert_eq!(columns[0].id, "expires");
		assert_eq!(columns[1].id, "resets");
		assert_eq!(props.actions[0].id, "confirm");
		assert_eq!(props.actions[0].label, "Spend reset");
		assert_eq!(props.selected.as_deref(), Some("claude:1:"));
		assert!(props.confirm.is_none());
	}

	#[test]
	fn text_rows_include_the_hint_line_and_borders() {
		let selector = ResetUsageSelectorComponent::new(fixture(), NOW, |_| {}, || {});
		let rows = selector.rows(70);
		assert!(rows[0].contains("Spend a saved rate-limit reset"), "{rows:?}");
		assert!(rows[1].contains("Claude · #1"), "{rows:?}");
		assert!(rows.iter().any(|row| row.contains("spend a reset")), "{rows:?}");
		assert!(rows.last().unwrap().contains('╚') || rows.last().unwrap().contains('└'));
		assert_eq!(rows.len(), 3 + 1 + 2);
	}
}
