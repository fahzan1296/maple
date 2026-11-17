//! Fullscreen `/models` hub: state model and pure logic.
//!
//! Reference: `l/src/overlays/model-hub.ts`.
//!
//! This chunk ports the hub's state: Roles-view rows, strip state, saved-model
//! presets, compaction-point editing, provider grouping, sidebar selection and
//! scrolling, and the Roles-view key map. Rendering (`impl Component`) lands in
//! the next chunk; the struct and every field are declared here so the module
//! compiles standalone.
//!
//! Deviations:
//! - The reference's async host calls and `Bun` timers are represented as data
//!   (`refresh_schedule`, `refreshing_providers`, …); the component layer owns
//!   the actual I/O.
//! - `Input::getValue()` is `Input::value()` in this crate.
//! - Provider label ordering uses `str::cmp`; the reference uses
//!   `localeCompare`, which agrees for the ASCII provider ids in practice.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use crate::components::input::Input;
use crate::keys::Key;
use crate::overlays::agent_hub_renderer::sanitize_display_single_line;
use crate::overlays::hub_frame::{HubFrame, SidebarEntry};
use crate::overlays::model_browser::{
	ModelBrowser, ModelBrowserItem, ModelBrowserPerf, ModelBrowserSource, ResolvedRoleValue, RoleAssignments,
	RoleInfo, RoleSection, model_search_text, CatalogModel,
};
use crate::overlays::model_selector::{
	ModelStringParseOptions, ThinkingSuffixOptions, format_model_selector_value, parse_model_string,
	split_upstream_routing,
};
use crate::thinking::{AUTO_THINKING, ConfiguredThinkingLevel, get_configured_thinking_level_metadata};
use crate::chrome::segment_track::{TrackSegment, render_segment_track};
use crate::component::Component;
use crate::keybindings::KeybindingsManager;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{NativeDescribe, col, item, kbd, list, node, row, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{NativeHint, action_hint, hints_row};
use crate::overlays::hub_frame::{
	HubFrameOptions, HubFrameStyler, HubGeometry, HubHit, PaneSide, SidebarStyle,
	StripChip as HubStripChip, StripState as HubStripState, describe_hub_chips, describe_hub_frame,
	describe_hub_sidebar, move_strip_selection,
};
use crate::overlays::model_browser::{
	ModelBrowserOptions, ModelCompactionPoint, SortModelItemsOptions, build_browser_items,
	resolve_role_assignments, scoped_role_pool, sort_model_items,
};
use crate::render::render_utils::{bold_text, paint};
use crate::text_shape::{Ellipsis, truncate_to_width, visible_width};
use crate::theme::Theme;

/// Floor for the hub body pane's width.
pub const MODEL_HUB_BODY_MIN_WIDTH: usize = 28;
/// Debounce before a sidebar hover triggers a provider refresh.
pub const PROVIDER_REFRESH_DEBOUNCE_MS: u64 = 120;
/// Maximum entries in the Recent scope.
pub const RECENT_LIMIT: usize = 15;
/// Accepted compaction limit input, shown beside the field.
pub const COMPACTION_INPUT_HINT: &str = "400k base · f400k fixed · 80% · empty resets";

/// A row of the Roles view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RolesRow {
	/// A role and its configured chain.
	Role { role: String },
	/// A `provider/model-id` or `provider/*` chain header.
	ChainKey { role: String },
	/// One entry of a chain.
	Fallback { role: String, chain_index: usize, selector: String },
	/// Horizontal rule between role halves or before the chain groups.
	Separator,
	/// Trailing "new fallback chain" affordance.
	NewFallback,
	/// Trailing "new role" affordance.
	NewRole,
}

impl RolesRow {
	/// The `kind` discriminant as the reference spells it.
	#[must_use]
	pub fn kind(&self) -> &'static str {
		match self {
			RolesRow::Role { .. } => "role",
			RolesRow::ChainKey { .. } => "chainKey",
			RolesRow::Fallback { .. } => "fallback",
			RolesRow::Separator => "separator",
			RolesRow::NewFallback => "newFallback",
			RolesRow::NewRole => "newRole",
		}
	}
}

/// Stable native list-item key of a Roles-view row.
#[must_use]
pub fn roles_row_key(row: &RolesRow, index: usize) -> String {
	match row {
		RolesRow::Role { role } => format!("role:{role}"),
		RolesRow::ChainKey { role } => format!("chain:{role}"),
		RolesRow::Fallback { role, chain_index, .. } => format!("fallback:{role}:{chain_index}"),
		RolesRow::Separator => format!("sep:{index}"),
		RolesRow::NewRole | RolesRow::NewFallback => row.kind().to_string(),
	}
}

/// What the model browser is currently picking for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssignTarget {
	/// A role's model.
	Role { role: String },
	/// A slot in a fallback chain; `None` appends.
	Fallback { role: String, index: Option<usize> },
	/// The primary model a brand-new fallback chain protects.
	FallbackKey,
}

/// A `--models` scope entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ScopedModelItem {
	pub model: CatalogModel,
	pub thinking_level: Option<String>,
}

/// Where a role assignment is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelRoleSelectionScope {
	Global,
	Project,
}

/// Why an `on_compaction_point_change` call kept the field open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompactionPointChangeResult {
	Error { message: String },
	Confirm { message: String },
}

/// Hub construction options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelHubOptions {
	/// Preselect this provider's sidebar entry.
	pub initial_provider_id: Option<String>,
	/// `provider/id` of the session's model.
	pub current_selector: Option<String>,
}

/// The focused sidebar entry plus its row, captured before a rebuild.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarAnchor {
	pub id: String,
	pub index: isize,
	pub offset: isize,
}

/// The Roles-view half a role belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleTab {
	All,
	Chat,
	Kind,
}

impl RoleTab {
	/// Every tab in display order.
	pub const ALL: [RoleTab; 3] = [RoleTab::All, RoleTab::Chat, RoleTab::Kind];

	/// Cycle to the previous/next tab.
	#[must_use]
	pub fn shift(self, delta: isize) -> Self {
		let len = Self::ALL.len() as isize;
		let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as isize;
		Self::ALL[(current + delta).rem_euclid(len) as usize]
	}
}

/// Model-kind tab of the browser view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKindTab {
	All,
	Chat,
	Tiny,
	Image,
	Tts,
	Stt,
	Search,
	Judge,
	Embedding,
	Rerank,
	Video,
}

impl ModelKindTab {
	/// Every tab in display order.
	pub const ALL: [ModelKindTab; 11] = [
		ModelKindTab::All,
		ModelKindTab::Chat,
		ModelKindTab::Tiny,
		ModelKindTab::Image,
		ModelKindTab::Tts,
		ModelKindTab::Stt,
		ModelKindTab::Search,
		ModelKindTab::Judge,
		ModelKindTab::Embedding,
		ModelKindTab::Rerank,
		ModelKindTab::Video,
	];

	/// Sentence-case label (acronyms stay upper).
	#[must_use]
	pub fn label(self) -> &'static str {
		match self {
			ModelKindTab::All => "All",
			ModelKindTab::Chat => "Chat",
			ModelKindTab::Tiny => "Tiny",
			ModelKindTab::Image => "Image",
			ModelKindTab::Tts => "TTS",
			ModelKindTab::Stt => "STT",
			ModelKindTab::Search => "Search",
			ModelKindTab::Judge => "Judge",
			ModelKindTab::Embedding => "Embedding",
			ModelKindTab::Rerank => "Rerank",
			ModelKindTab::Video => "Video",
		}
	}

	/// Cycle to the previous/next kind tab.
	#[must_use]
	pub fn shift(self, delta: isize) -> Self {
		let len = Self::ALL.len() as isize;
		let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as isize;
		Self::ALL[(current + delta).rem_euclid(len) as usize]
	}
}

/// A Roles-view command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolesAction {
	Pick,
	Clear,
	Fallback,
	Cycle,
	Earlier,
	Later,
	New,
	Thinking,
	Compaction,
	Save,
	NextPreset,
	PrevPreset,
}

/// Printable key of the Roles view and the command it runs.
#[must_use]
pub fn roles_action_for_char(ch: char) -> Option<RolesAction> {
	Some(match ch {
		'x' => RolesAction::Clear,
		'f' => RolesAction::Fallback,
		'c' => RolesAction::Cycle,
		'[' => RolesAction::Earlier,
		']' => RolesAction::Later,
		'n' => RolesAction::New,
		't' => RolesAction::Thinking,
		'k' => RolesAction::Compaction,
		's' => RolesAction::Save,
		'p' => RolesAction::NextPreset,
		'P' => RolesAction::PrevPreset,
		_ => return None,
	})
}

/// Roles-view command for a decoded key, if its printable character maps.
#[must_use]
pub fn roles_action_for_key(key: &Key) -> Option<RolesAction> {
	key.as_char().and_then(roles_action_for_char)
}

/// Whether `name` is a valid new-role or preset name.
#[must_use]
pub fn is_valid_role_name(name: &str) -> bool {
	let mut chars = name.chars();
	match chars.next() {
		Some(first) if first.is_ascii_alphabetic() => {}
		_ => return false,
	}
	chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// Thinking levels a role assignment can apply.
///
/// `supported` is the model's effort ladder; a model that does not reason gets
/// none, and unverified-thinking models omit the `auto` sentinel.
#[must_use]
pub fn thinking_options(
	reasoning: bool,
	unverified: bool,
	supported: &[ConfiguredThinkingLevel],
) -> Vec<ConfiguredThinkingLevel> {
	if !reasoning {
		return Vec::new();
	}
	let mut options = vec![ConfiguredThinkingLevel::Inherit, ConfiguredThinkingLevel::Off];
	if !unverified {
		options.push(AUTO_THINKING);
	}
	options.extend_from_slice(supported);
	options
}

/// Label of a thinking level as the strip renders it.
#[must_use]
pub fn thinking_chip_label(level: ConfiguredThinkingLevel) -> String {
	get_configured_thinking_level_metadata(level).label
}

// ═══════════════════════════════════════════════════════════════════════
// Fallback chains
// ═══════════════════════════════════════════════════════════════════════

/// The configured `retry.fallbackChains` record (already well-typed here).
#[must_use]
pub fn sanitized_fallback_chains(raw: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<String>> {
	raw.clone()
}

/// A fallback-chain entry split into its model base, effort, and routing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFallbackEntry {
	pub provider: String,
	pub id: String,
	pub thinking_level: Option<ConfiguredThinkingLevel>,
	pub upstream: Option<String>,
}

/// A fallback entry resolved against the registry.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFallbackEntry {
	pub item: ModelBrowserItem,
	pub thinking_level: Option<ConfiguredThinkingLevel>,
	pub upstream: Option<String>,
}

fn fallback_parse_options(is_literal: &dyn Fn(&str, &str) -> bool) -> ModelStringParseOptions<'_> {
	ModelStringParseOptions {
		suffix: ThinkingSuffixOptions { allow_max_suffix: true, allow_auto_alias: true },
		is_literal_model_id: Some(is_literal),
	}
}

/// Split a fallback entry into model base, explicit effort, and `@upstream`
/// routing. Exact literal ids win over routing, mirroring runtime precedence.
#[must_use]
pub fn parse_fallback_entry(
	raw: &str,
	is_literal: &dyn Fn(&str, &str) -> bool,
) -> Option<ParsedFallbackEntry> {
	let trimmed = raw.trim();
	let options = fallback_parse_options(is_literal);
	let literal = parse_model_string(trimmed, Some(&options));
	if let Some(parsed) = &literal {
		if is_literal(&parsed.provider, &parsed.id) {
			return Some(ParsedFallbackEntry {
				provider: parsed.provider.clone(),
				id: parsed.id.clone(),
				thinking_level: parsed.thinking_level,
				upstream: None,
			});
		}
	}
	if let Some(routing) = split_upstream_routing(trimmed) {
		let parsed = parse_model_string(routing.base.trim(), Some(&options))?;
		return Some(ParsedFallbackEntry {
			provider: parsed.provider,
			id: parsed.id,
			thinking_level: parsed.thinking_level,
			upstream: Some(routing.upstream),
		});
	}
	let parsed = literal?;
	Some(ParsedFallbackEntry {
		provider: parsed.provider,
		id: parsed.id,
		thinking_level: parsed.thinking_level,
		upstream: None,
	})
}

/// Resolve a chain entry to its browser item, effort, and routing.
///
/// `None` for wildcard entries (`provider/*`) and models the registry does not
/// know, which makes the row inert.
#[must_use]
pub fn resolve_fallback_entry(
	chains: &BTreeMap<String, Vec<String>>,
	role: &str,
	index: usize,
	find: &dyn Fn(&str, &str) -> Option<CatalogModel>,
) -> Option<ResolvedFallbackEntry> {
	let raw = chains.get(role)?.get(index)?;
	if raw.ends_with("/*") {
		return None;
	}
	let parsed = parse_fallback_entry(raw, &|provider, id| find(provider, id).is_some())?;
	let model = find(&parsed.provider, &parsed.id)?;
	Some(ResolvedFallbackEntry {
		item: ModelBrowserItem {
			provider: model.provider.clone(),
			id: model.id.clone(),
			selector: format!("{}/{}", model.provider, model.id),
			model,
			label_color: None,
		},
		thinking_level: parsed.thinking_level,
		upstream: parsed.upstream,
	})
}

/// Replace/append `selector` in a chain, deduping other occurrences.
#[must_use]
pub fn commit_fallback_chain(chain: &[String], target: Option<usize>, selector: &str) -> Vec<String> {
	let mut chain = chain.to_vec();
	if let Some(index) = target {
		if index < chain.len() {
			chain[index] = selector.to_string();
			return dedupe_excluding(&chain, selector, index);
		}
	}
	if !chain.iter().any(|entry| entry == selector) {
		chain.push(selector.to_string());
	}
	chain
}

/// Drop later duplicates of `selector`, keeping the entry at `keep`.
#[must_use]
pub fn dedupe_excluding(chain: &[String], selector: &str, keep: usize) -> Vec<String> {
	let mut result = Vec::with_capacity(chain.len());
	for (index, entry) in chain.iter().enumerate() {
		if entry == selector && index != keep {
			continue;
		}
		result.push(entry.clone());
	}
	result
}

/// Append `selector` unless the chain already holds it.
#[must_use]
pub fn append_fallback(chain: &[String], selector: &str) -> Vec<String> {
	let mut chain = chain.to_vec();
	if !chain.iter().any(|entry| entry == selector) {
		chain.push(selector.to_string());
	}
	chain
}

/// Remove one chain entry.
#[must_use]
pub fn remove_fallback(chain: &[String], index: usize) -> Vec<String> {
	let mut chain = chain.to_vec();
	if index < chain.len() {
		chain.remove(index);
	}
	chain
}

/// Move a chain entry one slot earlier/later.
#[must_use]
pub fn move_fallback(chain: &[String], index: usize, delta: isize) -> Vec<String> {
	let mut chain = chain.to_vec();
	let target = index as isize + delta;
	if index >= chain.len() || target < 0 || target as usize >= chain.len() {
		return chain;
	}
	chain.swap(index, target as usize);
	chain
}

/// Persist a fallback entry's effort: explicit effort suffixed, inherit bare,
/// routing re-attached ahead of the suffix.
#[must_use]
pub fn fallback_thinking_selector(
	provider: &str,
	id: &str,
	upstream: Option<&str>,
	level: ConfiguredThinkingLevel,
) -> String {
	let base = format!("{provider}/{id}");
	let routed = match upstream {
		Some(upstream) => format!("{base}@{upstream}"),
		None => base,
	};
	format_model_selector_value(&routed, Some(level))
}

// ═══════════════════════════════════════════════════════════════════════
// Quick-switch cycle
// ═══════════════════════════════════════════════════════════════════════

/// Toggle `role`'s membership, appending at the end.
#[must_use]
pub fn toggle_cycle_membership(order: &[String], role: &str) -> Vec<String> {
	let mut order = order.to_vec();
	if let Some(index) = order.iter().position(|entry| entry == role) {
		order.remove(index);
	} else {
		order.push(role.to_string());
	}
	order
}

/// Move `role` one slot earlier/later in the cycle order.
#[must_use]
pub fn move_cycle_membership(order: &[String], role: &str, delta: isize) -> Vec<String> {
	let mut order = order.to_vec();
	let Some(index) = order.iter().position(|entry| entry == role) else {
		return order;
	};
	let target = index as isize + delta;
	if target < 0 || target as usize >= order.len() {
		return order;
	}
	order.swap(index, target as usize);
	order
}

// ═══════════════════════════════════════════════════════════════════════
// Saved model presets
// ═══════════════════════════════════════════════════════════════════════

/// Last preset ctrl+←/→ tried, with the settings revision after the attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetCursor {
	pub name: String,
	pub revision: u64,
}

/// The preset stepping starts from: the cursor while nothing changed, else the
/// setup's matching preset.
#[must_use]
pub fn preset_base(cursor: Option<&PresetCursor>, active: Option<&str>, revision: u64) -> Option<String> {
	if let Some(cursor) = cursor {
		if active.is_none() || cursor.revision == revision {
			return Some(cursor.name.clone());
		}
	}
	active.map(str::to_string)
}

/// Name of the previous/next preset to apply, wrapping.
#[must_use]
pub fn switch_preset_name(
	names: &[String],
	active: Option<&str>,
	cursor: Option<&PresetCursor>,
	revision: u64,
	delta: isize,
) -> Option<String> {
	if names.is_empty() {
		return None;
	}
	let base = preset_base(cursor, active, revision);
	let current = base
		.as_deref()
		.and_then(|name| names.iter().position(|entry| entry == name))
		.map(|index| index as isize)
		.unwrap_or(-1);
	let len = names.len() as isize;
	let next = if current < 0 {
		if delta > 0 { 0 } else { len - 1 }
	} else {
		(current + delta).rem_euclid(len)
	};
	names.get(next as usize).cloned()
}

// ═══════════════════════════════════════════════════════════════════════
// Compaction point
// ═══════════════════════════════════════════════════════════════════════

/// A pending warning a second Enter accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmState {
	pub value: String,
	pub message: String,
}

/// Host preview of the typed limit, cached per value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewCache {
	pub value: String,
	pub text: Option<String>,
}

/// The note beside the compaction field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeStyle {
	Error,
	Warning,
	Dim,
}

/// Text and severity of the compaction notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionNotice {
	pub text: String,
	pub style: NoticeStyle,
}

/// Whether the compaction field still holds the text its warning was raised for.
#[must_use]
pub fn compaction_confirm_pending(confirm: Option<&ConfirmState>, value: &str) -> bool {
	confirm.is_some_and(|confirm| confirm.value == value)
}

/// Strip host text to one safe display line.
#[must_use]
pub fn sanitize_warning(text: &str) -> String {
	sanitize_display_single_line(&crate::strip_ansi(text))
}

/// The note beside the compaction field: last error, pending warning, host
/// preview, or the input syntax hint.
#[must_use]
pub fn compaction_notice(
	error: Option<&String>,
	confirm: Option<&ConfirmState>,
	value: &str,
	preview_text: Option<&str>,
) -> CompactionNotice {
	if let Some(error) = error {
		return CompactionNotice { text: sanitize_warning(error), style: NoticeStyle::Error };
	}
	if let Some(confirm) = confirm {
		if compaction_confirm_pending(Some(confirm), value) {
			return CompactionNotice {
				text: sanitize_warning(&confirm.message),
				style: NoticeStyle::Warning,
			};
		}
	}
	if !value.trim().is_empty() {
		if let Some(text) = preview_text {
			return CompactionNotice { text: sanitize_warning(text), style: NoticeStyle::Dim };
		}
	}
	CompactionNotice { text: COMPACTION_INPUT_HINT.to_string(), style: NoticeStyle::Dim }
}

// ═══════════════════════════════════════════════════════════════════════
// Sidebar rows
// ═══════════════════════════════════════════════════════════════════════

/// A sidebar row plus the hub metadata the browser needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarEntryX {
	pub entry: SidebarEntry,
	pub provider_id: Option<String>,
	pub locked: bool,
	pub oauth: bool,
	pub catalog_count: usize,
}

impl SidebarEntryX {
	/// Build a provider row.
	#[must_use]
	pub fn provider(
		provider_id: &str,
		locked: bool,
		annotation: Option<String>,
		oauth: bool,
		catalog_count: usize,
	) -> Self {
		SidebarEntryX {
			entry: SidebarEntry {
				id: format!("provider:{provider_id}"),
				kind: "provider".to_string(),
				label: provider_id.to_string(),
				annotation,
			},
			provider_id: Some(provider_id.to_string()),
			locked,
			oauth,
			catalog_count,
		}
	}
}

/// Discovery state of one provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryStatus {
	Idle,
	Ok,
	Empty,
	Cached,
	Unavailable,
	Unauthenticated,
}

/// Provider discovery facts the grouping consults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDiscoveryState {
	pub optional: bool,
	pub status: DiscoveryStatus,
	pub fetched_at: Option<u64>,
	pub error: Option<String>,
}

/// Inputs to provider grouping.
pub struct ProviderGroupInput<'a> {
	/// Scoped hubs (`--models` active) surface no locked providers.
	pub scoped: bool,
	pub available_counts: &'a HashMap<String, usize>,
	pub catalog_counts: &'a HashMap<String, usize>,
	pub disabled_providers: &'a HashSet<String>,
	pub discoverable: &'a [String],
	pub authenticated: &'a dyn Fn(&str) -> bool,
	pub discovery_state: &'a dyn Fn(&str) -> Option<ProviderDiscoveryState>,
	pub oauth_ids: &'a HashSet<String>,
}

/// Split providers into selectable (unlocked) and locked rows, alphabetized.
#[must_use]
pub fn group_providers(input: &ProviderGroupInput<'_>) -> (Vec<SidebarEntryX>, Vec<SidebarEntryX>) {
	let mut unlocked: HashSet<String> = input.available_counts.keys().cloned().collect();
	let mut locked: HashSet<String> = HashSet::new();
	if !input.scoped {
		for provider in input.catalog_counts.keys() {
			if !unlocked.contains(provider) && !input.disabled_providers.contains(provider) {
				locked.insert(provider.clone());
			}
		}
		for provider in input.discoverable {
			if unlocked.contains(provider) || input.disabled_providers.contains(provider) {
				continue;
			}
			let authenticated = (input.authenticated)(provider);
			if authenticated || !locked.contains(provider) {
				if !authenticated {
					if let Some(state) = (input.discovery_state)(provider) {
						if state.optional
							&& matches!(state.status, DiscoveryStatus::Idle | DiscoveryStatus::Unavailable)
						{
							continue;
						}
					}
				}
				locked.remove(provider);
				unlocked.insert(provider.clone());
			}
		}
	}

	let build = |provider: &String, locked: bool| {
		let count = input.available_counts.get(provider).copied().unwrap_or(0);
		SidebarEntryX::provider(
			provider,
			locked,
			if locked { None } else { Some(count.to_string()) },
			input.oauth_ids.contains(provider),
			input.catalog_counts.get(provider).copied().unwrap_or(0),
		)
	};
	let mut unlocked_entries: Vec<SidebarEntryX> = unlocked.iter().map(|p| build(p, false)).collect();
	let mut locked_entries: Vec<SidebarEntryX> = locked.iter().map(|p| build(p, true)).collect();
	unlocked_entries.sort_by(|a, b| a.entry.label.cmp(&b.entry.label));
	locked_entries.sort_by(|a, b| a.entry.label.cmp(&b.entry.label));
	(unlocked_entries, locked_entries)
}

/// Assemble the sidebar from its sections. While searching, providers with
/// matches float to the top of their section. Returns the entries and whether
/// the active entry vanished (so focus falls back to All models).
#[must_use]
pub fn compose_entries(
	fixed: Vec<SidebarEntryX>,
	unlocked: &[SidebarEntryX],
	locked: &[SidebarEntryX],
	search_counts: Option<&HashMap<String, usize>>,
	active_id: &str,
) -> (Vec<SidebarEntryX>, bool) {
	let mut providers = unlocked.to_vec();
	if let Some(counts) = search_counts {
		providers.sort_by(|a, b| {
			let a_matched = counts.get(a.provider_id.as_deref().unwrap_or("")).copied().unwrap_or(0) > 0;
			let b_matched = counts.get(b.provider_id.as_deref().unwrap_or("")).copied().unwrap_or(0) > 0;
			if a_matched != b_matched {
				return b_matched.cmp(&a_matched);
			}
			a.entry.label.cmp(&b.entry.label)
		});
	}

	let mut entries = fixed;
	if !providers.is_empty() {
		entries.push(SidebarEntryX {
			entry: SidebarEntry {
				id: "sep:providers".to_string(),
				kind: "separator".to_string(),
				label: String::new(),
				annotation: None,
			},
			provider_id: None,
			locked: false,
			oauth: false,
			catalog_count: 0,
		});
		entries.extend(providers);
	}
	if !locked.is_empty() {
		entries.push(SidebarEntryX {
			entry: SidebarEntry {
				id: "sep:locked".to_string(),
				kind: "separator".to_string(),
				label: String::new(),
				annotation: None,
			},
			provider_id: None,
			locked: false,
			oauth: false,
			catalog_count: 0,
		});
		entries.extend(locked.iter().cloned());
	}

	let active_missing = !entries.iter().any(|entry| entry.entry.id == active_id);
	(entries, active_missing)
}

/// Entries the scope hop skips: separators always; while searching also Roles,
/// an empty Recent, locked providers, and providers without matches.
#[must_use]
pub fn is_hop_skipped(
	entry: &SidebarEntryX,
	search_counts: Option<&HashMap<String, usize>>,
	recent_search_count: usize,
) -> bool {
	if entry.entry.is_separator() {
		return true;
	}
	let Some(counts) = search_counts else {
		return false;
	};
	match entry.entry.kind.as_str() {
		"roles" => true,
		"recent" => recent_search_count == 0,
		"provider" => {
			if entry.locked {
				return true;
			}
			counts.get(entry.provider_id.as_deref().unwrap_or("")).copied().unwrap_or(0) == 0
		}
		_ => false,
	}
}

/// The selectable entry nearest `preferred` in the current list.
#[must_use]
pub fn nearest_navigable_index(
	entries: &[SidebarEntryX],
	preferred: isize,
	is_skipped: &dyn Fn(&SidebarEntryX) -> bool,
) -> Option<usize> {
	if entries.is_empty() {
		return None;
	}
	let start = preferred.clamp(0, entries.len() as isize - 1) as usize;
	for radius in 0..entries.len() {
		let mut candidates = vec![start];
		if radius > 0 {
			candidates.push(start + radius);
			candidates.push(start.saturating_sub(radius));
		}
		for index in candidates {
			if let Some(entry) = entries.get(index) {
				if !is_skipped(entry) {
					return Some(index);
				}
			}
		}
	}
	None
}

/// Snapshot the focused entry and its row within the sidebar viewport.
#[must_use]
pub fn capture_anchor(entries: &[SidebarEntryX], active_id: &str, scroll: usize) -> SidebarAnchor {
	let index = entries
		.iter()
		.position(|entry| entry.entry.id == active_id)
		.map(|index| index as isize)
		.unwrap_or(-1);
	SidebarAnchor { id: active_id.to_string(), index, offset: index - scroll as isize }
}

/// Reposition the sidebar after a rebuild. Returns a replacement active id when
/// the focused entry vanished, and the restored scroll offset.
#[must_use]
pub fn restore_sidebar_anchor(
	entries: &[SidebarEntryX],
	anchor: &SidebarAnchor,
	is_skipped: &dyn Fn(&SidebarEntryX) -> bool,
) -> (Option<String>, usize) {
	if anchor.index < 0 {
		return (None, 0);
	}
	let offset = anchor.offset;
	if let Some(survivor) = entries.iter().position(|entry| entry.entry.id == anchor.id) {
		return (None, (survivor as isize - offset).max(0) as usize);
	}
	let Some(replacement) = nearest_navigable_index(entries, anchor.index, is_skipped) else {
		return (None, 0);
	};
	let id = entries[replacement].entry.id.clone();
	(Some(id), (replacement as isize - offset).max(0) as usize)
}

// ═══════════════════════════════════════════════════════════════════════
// Selection and scrolling
// ═══════════════════════════════════════════════════════════════════════

/// Clamp a roles cursor into range.
#[must_use]
pub fn clamp_role_index(index: usize, row_count: usize) -> usize {
	if row_count == 0 {
		0
	} else {
		index.min(row_count - 1)
	}
}

/// First roles row to draw so `cursor` stays inside the visible window.
#[must_use]
pub fn follow_cursor(
	scroll_start: usize,
	cursor: usize,
	visible_count: usize,
	row_count: usize,
) -> usize {
	if row_count == 0 || visible_count == 0 {
		return 0;
	}
	let max_start = row_count.saturating_sub(visible_count);
	let next = if cursor < scroll_start {
		cursor
	} else if cursor >= scroll_start.saturating_add(visible_count) {
		cursor + 1 - visible_count
	} else {
		scroll_start
	};
	next.min(max_start)
}

// ═══════════════════════════════════════════════════════════════════════
// Provider refresh scheduling
// ═══════════════════════════════════════════════════════════════════════

/// Providers already auto-refreshed this process.
static AUTO_REFRESHED_PROVIDERS: LazyLock<Mutex<HashSet<String>>> =
	LazyLock::new(|| Mutex::new(HashSet::new()));

/// Test hook: forget which providers were auto-refreshed.
pub fn reset_provider_auto_refresh_guard() {
	if let Ok(mut set) = AUTO_REFRESHED_PROVIDERS.lock() {
		set.clear();
	}
}

/// Whether `provider` already auto-refreshed this process.
#[must_use]
pub fn provider_auto_refreshed(provider: &str) -> bool {
	AUTO_REFRESHED_PROVIDERS
		.lock()
		.map(|set| set.contains(provider))
		.unwrap_or(false)
}

/// Record that `provider` auto-refreshed (consumed when the fetch starts).
pub fn mark_provider_auto_refreshed(provider: &str) {
	if let Ok(mut set) = AUTO_REFRESHED_PROVIDERS.lock() {
		set.insert(provider.to_string());
	}
}

/// What a sidebar selection should do for a provider's live model list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshSchedule {
	/// Nothing to do: already scheduled/refreshing, once-guard burned, or scoped.
	Skip,
	/// Queue the debounced catalog-only fetch.
	Debounce,
	/// F5: start a credential-refreshing fetch now.
	StartForced,
	/// F5 while a catalog-only fetch is in flight: re-run after it settles.
	QueueCredentialRefresh,
}

/// Decide the refresh action for selecting `provider`.
#[must_use]
pub fn refresh_schedule(
	scoped: bool,
	provider: &str,
	force: bool,
	scheduled: &HashSet<String>,
	refreshing: &HashSet<String>,
) -> RefreshSchedule {
	if scoped || provider.is_empty() {
		return RefreshSchedule::Skip;
	}
	if force {
		if scheduled.contains(provider) {
			return RefreshSchedule::StartForced;
		}
		if refreshing.contains(provider) {
			return RefreshSchedule::QueueCredentialRefresh;
		}
	} else if scheduled.contains(provider) || refreshing.contains(provider) {
		return RefreshSchedule::Skip;
	}
	if !force && provider_auto_refreshed(provider) {
		return RefreshSchedule::Skip;
	}
	RefreshSchedule::Debounce
}

// ═══════════════════════════════════════════════════════════════════════
// Host contracts and component state
// ═══════════════════════════════════════════════════════════════════════

/// Where role values are persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelRoleStorage {
	Global,
	Project,
}

/// Which store a role's value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelRoleSource {
	Global,
	Project,
	Default,
}

/// Live preferences and selector operations the hub needs beyond the browser.
pub trait ModelHubSource: ModelBrowserSource {
	fn disabled_providers(&self) -> Vec<String> {
		Vec::new()
	}

	fn fallback_chains(&self) -> BTreeMap<String, Vec<String>> {
		BTreeMap::new()
	}

	fn model_role_storage(&self) -> ModelRoleStorage {
		ModelRoleStorage::Global
	}

	fn cycle_order(&self) -> Vec<String> {
		Vec::new()
	}

	fn get_project_model_role(&self, _role: &str) -> Option<String> {
		None
	}

	fn get_global_model_role(&self, role: &str) -> Option<String> {
		self.get_model_role(role)
	}

	fn get_model_role_source(&self, _role: &str) -> ModelRoleSource {
		ModelRoleSource::Default
	}

	/// Saved presets and the one the current setup matches, if the host has them.
	fn get_model_presets(&self) -> Option<(Vec<String>, Option<String>)> {
		None
	}

	/// Effort ladder `model` supports, for thinking strips. Catalog rows carry
	/// no thinking data, so hosts that have it override this.
	fn thinking_efforts(&self, _model: &CatalogModel) -> Vec<ConfiguredThinkingLevel> {
		Vec::new()
	}
}

/// Catalog capabilities the hub needs beyond the browser source.
pub trait ModelHubRegistry {
	fn auth_key_source(&self, _provider: &str) -> Option<String> {
		None
	}

	fn get_discoverable_providers(&self) -> Vec<String> {
		Vec::new()
	}

	fn get_provider_discovery_state(&self, _provider: &str) -> Option<ProviderDiscoveryState> {
		None
	}

	fn find(&self, _provider: &str, _id: &str) -> Option<CatalogModel> {
		None
	}

	/// Every catalog model, for locked-provider previews.
	fn get_all(&self) -> Vec<CatalogModel> {
		Vec::new()
	}

	/// Models the session may use, for scope previews.
	fn get_available(&self) -> Vec<CatalogModel> {
		Vec::new()
	}

	/// Catalog load error, shown outside provider scopes.
	fn get_error(&self) -> Option<String> {
		None
	}

	/// Environment variables that configure `provider`.
	fn provider_env_vars(&self, _provider: &str) -> Vec<String> {
		Vec::new()
	}

	/// Providers that authenticate through OAuth.
	fn oauth_providers(&self) -> Vec<String> {
		Vec::new()
	}
}

/// Host callbacks the hub invokes.
pub trait ModelHubCallbacks {
	/// Persist a role assignment; `false` refuses.
	fn on_assign(
		&self,
		_model: &CatalogModel,
		_role: &str,
		_thinking_level: ConfiguredThinkingLevel,
		_selector: &str,
		_scope: Option<ModelRoleSelectionScope>,
	) -> bool {
		let _ = (_model, _role, _thinking_level, _selector, _scope);
		true
	}

	/// Clear a configured role back to auto-selection.
	fn on_unassign(&self, _role: &str, _scope: Option<ModelRoleSelectionScope>) {}

	/// Persist a `retry.fallbackChains` entry.
	fn on_fallback_chain_change(&self, _role: &str, _chain: &[String]) {}

	/// Locked provider activation: forward to the login flow.
	fn on_login_request(&self, _provider_id: &str) {}

	/// Rebuilt `--models` scope after a catalog refresh.
	fn on_catalog_refreshed(&self, _provider_id: Option<&str>) -> Option<Vec<ScopedModelItem>> {
		None
	}

	/// Save the current setup as a named preset.
	fn on_save_preset(&self, _name: &str) {}

	/// Apply a saved preset.
	fn on_switch_preset(&self, _name: &str) -> bool {
		true
	}

	/// Persist a new quick-switch cycle order.
	fn on_cycle_order_change(&self, _order: &[String]) {}

	/// Persist a model's own compaction limit.
	fn on_compaction_point_change(
		&self,
		_model: &CatalogModel,
		_input: &str,
		_confirmed: bool,
	) -> Option<CompactionPointChangeResult> {
		let _ = _model;
		None
	}

	/// One line describing where `model` would compact with `input`.
	fn preview_compaction_point(&self, _model: &CatalogModel, _input: &str) -> Option<String> {
		None
	}

	/// Close the hub.
	fn on_cancel(&self) {}
}

/// Footer strip state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripKind {
	Role,
	Scope,
	Thinking,
}

/// Action carried by a footer chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripAction {
	Assign,
	Unassign,
	Fallback,
	FallbackModel,
	FallbackProvider,
	Scope,
	Thinking,
}

/// A pre-styled footer choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripChip {
	pub label: String,
	pub styled: String,
	pub action: StripAction,
	pub role: Option<String>,
	pub thinking_level: Option<ConfiguredThinkingLevel>,
	pub scope: Option<ModelRoleSelectionScope>,
}

/// Purpose of a footer text input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamePurpose {
	Role,
	Preset,
	Compaction,
}

/// Footer strip state: chip strip or text input.
pub enum StripState {
	Role {
		kind: StripKind,
		item: ModelBrowserItem,
		chips: Vec<StripChip>,
		index: usize,
		role: Option<String>,
		fallback_index: Option<usize>,
		scope: Option<ModelRoleSelectionScope>,
		return_to_roles: bool,
		initial_thinking_level: Option<ConfiguredThinkingLevel>,
	},
	Name {
		purpose: NamePurpose,
		input: Input,
		model: Option<CatalogModel>,
		error: Option<String>,
		confirm: Option<ConfirmState>,
		preview: Option<PreviewCache>,
	},
}

impl StripState {
	/// The selected chip, if this is a chip strip with a selection.
	#[must_use]
	pub fn selected_chip(&self) -> Option<&StripChip> {
		match self {
			StripState::Role { chips, index, .. } => chips.get(*index),
			StripState::Name { .. } => None,
		}
	}

	/// Mutable chip vector, if this is a chip strip.
	pub fn chips_mut(&mut self) -> Option<&mut Vec<StripChip>> {
		match self {
			StripState::Role { chips, .. } => Some(chips),
			StripState::Name { .. } => None,
		}
	}

	/// Selected chip index, if this is a chip strip.
	#[must_use]
	pub fn chip_index(&self) -> Option<usize> {
		match self {
			StripState::Role { index, .. } => Some(*index),
			StripState::Name { .. } => None,
		}
	}

	/// The text-input value, if this is a name strip.
	#[must_use]
	pub fn input_value(&self) -> &str {
		match self {
			StripState::Name { input, .. } => input.value(),
			StripState::Role { .. } => "",
		}
	}

	/// Whether the strip is a text input.
	#[must_use]
	pub fn is_name(&self) -> bool {
		matches!(self, StripState::Name { .. })
	}
}

/// The fullscreen model hub's state.
///
/// Rendering lands with `impl Component` in the next chunk; the fields are the
/// reference's, minus the `TUI` handle (the component layer owns repaint).
/// Which pane owns the arrow keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubFocus {
	Scope,
	List,
}

impl HubFocus {
	#[must_use]
	pub fn as_str(self) -> &'static str {
		match self {
			HubFocus::Scope => "scope",
			HubFocus::List => "list",
		}
	}

	#[must_use]
	pub fn toggled(self) -> Self {
		match self {
			HubFocus::Scope => HubFocus::List,
			HubFocus::List => HubFocus::Scope,
		}
	}
}

/// Cached native description; invalidated by `native_version`.
#[derive(Clone)]
pub struct NativeCache {
	pub version: u64,
	pub node: NativeNode,
}

/// Adapt an `Arc<dyn ModelHubSource>` to the browser's source trait. Rust 1.85
/// has no trait upcasting, so the browser cannot take the hub source directly.
struct HubBrowserSource(Arc<dyn ModelHubSource>);

impl ModelBrowserSource for HubBrowserSource {
	fn revision(&self) -> u64 {
		self.0.revision()
	}

	fn default_thinking_level(&self) -> Option<ConfiguredThinkingLevel> {
		self.0.default_thinking_level()
	}

	fn model_provider_order(&self) -> Vec<String> {
		self.0.model_provider_order()
	}

	fn known_role_ids(&self) -> Vec<String> {
		self.0.known_role_ids()
	}

	fn mru_order(&self) -> Vec<String> {
		self.0.mru_order()
	}

	fn model_perf(&self) -> HashMap<String, ModelBrowserPerf> {
		self.0.model_perf()
	}

	fn get_model_role(&self, role: &str) -> Option<String> {
		self.0.get_model_role(role)
	}

	fn get_role_info(&self, role: &str) -> RoleInfo {
		self.0.get_role_info(role)
	}

	fn role_accepts(&self, role: &str, model: &CatalogModel) -> bool {
		self.0.role_accepts(role, model)
	}

	fn resolve_role_value(&self, value: &str, models: &[CatalogModel]) -> ResolvedRoleValue {
		self.0.resolve_role_value(value, models)
	}

	fn service_tier_for(&self, model: &CatalogModel) -> Option<String> {
		self.0.service_tier_for(model)
	}

	fn compaction_point_for(&self, model: &CatalogModel) -> Option<ModelCompactionPoint> {
		self.0.compaction_point_for(model)
	}
}

pub struct ModelHub {
	pub theme: Theme,
	pub source: Arc<dyn ModelHubSource>,
	pub registry: Box<dyn ModelHubRegistry>,
	pub scoped_models: Vec<ScopedModelItem>,
	pub callbacks: Box<dyn ModelHubCallbacks>,
	pub browser: ModelBrowser,
	pub frame: HubFrame,
	pub roles: RoleAssignments,
	pub available_items: Vec<ModelBrowserItem>,
	pub recent_items: Vec<ModelBrowserItem>,
	pub recent_selectors: HashSet<String>,
	pub candidate_items: Vec<ModelBrowserItem>,
	pub candidate_catalog: Option<Vec<ModelBrowserItem>>,
	pub browser_catalog: Option<Vec<ModelBrowserItem>>,
	pub model_kind_tab: ModelKindTab,
	pub role_tab: RoleTab,
	pub config_error: Option<String>,
	pub entries: Vec<SidebarEntryX>,
	pub fixed_entries: Vec<SidebarEntryX>,
	pub unlocked_provider_entries: Vec<SidebarEntryX>,
	pub locked_provider_entries: Vec<SidebarEntryX>,
	pub recent_search_count: usize,
	pub search_total: usize,
	pub active_entry_id: String,
	pub sidebar_follow_active: bool,
	pub sidebar_hover: Option<usize>,
	pub focus: HubFocus,
	/// First Roles row in body-line coordinates, for mouse routing.
	pub roles_row_start: usize,
	/// Body line of the locked view's OAuth login row, if shown.
	pub locked_login_line: Option<usize>,
	/// Cached native node; `None` after `request_render`.
	pub native_cache: Option<NativeCache>,
	/// Frame index for the sidebar refresh spinner.
	pub refresh_spinner_frame: usize,
	/// Keybinding lookups for hints and matches.
	pub bindings: KeybindingsManager,
	pub roles_rows: Vec<RolesRow>,
	pub role_index: usize,
	pub role_hover: Option<usize>,
	pub role_scroll_start: usize,
	pub roles_visible_count: usize,
	pub assigning: Option<AssignTarget>,
	pub strip: Option<StripState>,
	pub assignment_pending: bool,
	pub preset_cursor: Option<PresetCursor>,
	pub presets_memo: Option<(u64, Vec<String>, Option<String>)>,
	pub disposed: bool,
	pub search_counts: Option<HashMap<String, usize>>,
	pub refreshing_providers: HashSet<String>,
	pub scheduled_provider_refreshes: HashSet<String>,
	pub pending_credential_refresh_providers: HashSet<String>,
	pub native_version: u64,
	pub current_selector: Option<String>,
	pub catalog_refreshing: bool,
	/// Host callback presence flags the footer hints consult.
	pub can_save_preset: bool,
}

impl ModelHub {

	/// Build the hub over `source`, with `registry` for catalog data and
	/// `callbacks` for host mutations.
	#[must_use]
	pub fn new(
		source: Arc<dyn ModelHubSource>,
		registry: Box<dyn ModelHubRegistry>,
		scoped_models: Vec<ScopedModelItem>,
		callbacks: Box<dyn ModelHubCallbacks>,
		options: ModelHubOptions,
	) -> Self {
		let browser = ModelBrowser::new(
			Box::new(HubBrowserSource(source.clone())),
			ModelBrowserOptions {
				show_provider: Some(true),
				current_context_tokens: 0,
				mark_over_context: false,
				empty_text: None,
			},
		);
		let frame = HubFrame::new(
			"Models",
			(24, 34),
			HubFrameOptions {
				body_min_width: MODEL_HUB_BODY_MIN_WIDTH,
				preserve_sidebar: true,
			},
		);
		let current_selector = options.current_selector.clone();
		let initial_provider_id = options.initial_provider_id.clone();
		let mut hub = ModelHub {
			theme: crate::theme::default_theme(),
			source,
			registry,
			scoped_models,
			callbacks,
			browser,
			frame,
			roles: RoleAssignments::new(),
			available_items: Vec::new(),
			recent_items: Vec::new(),
			recent_selectors: HashSet::new(),
			candidate_items: Vec::new(),
			candidate_catalog: None,
			browser_catalog: None,
			model_kind_tab: ModelKindTab::All,
			role_tab: RoleTab::All,
			config_error: None,
			entries: Vec::new(),
			fixed_entries: Vec::new(),
			unlocked_provider_entries: Vec::new(),
			locked_provider_entries: Vec::new(),
			recent_search_count: 0,
			search_total: 0,
			active_entry_id: "all".to_string(),
			sidebar_follow_active: true,
			sidebar_hover: None,
			focus: HubFocus::Scope,
			roles_row_start: 0,
			locked_login_line: None,
			native_cache: None,
			refresh_spinner_frame: 0,
			bindings: KeybindingsManager::new(),
			roles_rows: Vec::new(),
			role_index: 0,
			role_hover: None,
			role_scroll_start: 0,
			roles_visible_count: 0,
			assigning: None,
			strip: None,
			assignment_pending: false,
			preset_cursor: None,
			presets_memo: None,
			disposed: false,
			search_counts: None,
			refreshing_providers: HashSet::new(),
			scheduled_provider_refreshes: HashSet::new(),
			pending_credential_refresh_providers: HashSet::new(),
			native_version: 0,
			current_selector,
			catalog_refreshing: false,
			can_save_preset: true,
		};
		hub.sync_from_registry_state();
		if let Some(provider) = initial_provider_id {
			let id = format!("provider:{provider}");
			if hub.entries.iter().any(|entry| entry.entry.id == id) {
				hub.set_active_entry(&id);
			}
		}
		hub
	}

	/// The active sidebar row, if any.
	#[must_use]
	pub fn active_entry(&self) -> Option<&SidebarEntryX> {
		self.entries
			.iter()
			.find(|entry| entry.entry.id == self.active_entry_id)
			.or_else(|| self.entries.first())
	}

	/// Whether the body is the model browser for `entry`.
	#[must_use]
	pub fn is_browser_view(&self, entry: &SidebarEntryX) -> bool {
		if self.assigning.is_some() {
			return true;
		}
		entry.entry.kind == "recent"
			|| entry.entry.kind == "all"
			|| (entry.entry.kind == "provider" && !entry.locked)
	}

	/// Whether the body is the Roles manager for `entry`.
	#[must_use]
	pub fn roles_view(&self, entry: &SidebarEntryX) -> bool {
		entry.entry.kind == "roles" && self.assigning.is_none()
	}

	/// Whether the body is a locked provider pane for `entry`.
	#[must_use]
	pub fn locked_view(&self, entry: &SidebarEntryX) -> bool {
		entry.entry.kind == "provider" && entry.locked && self.assigning.is_none()
	}

	/// Select `id` and rebuild the body for it.
	pub fn set_active_entry(&mut self, id: &str) {
		if self.entries.iter().all(|entry| entry.entry.id != id) {
			return;
		}
		self.active_entry_id = id.to_string();
		self.sidebar_follow_active = true;
		if id == "roles" {
			self.focus = HubFocus::List;
		}
		self.apply_scope();
		self.request_render();
	}

	/// Rebuild the browser items for the active scope.
	pub fn apply_scope(&mut self) {
		let Some(entry) = self.active_entry().cloned() else {
			return;
		};
		self.browser.set_show_provider(true);
		let items = match entry.entry.kind.as_str() {
			"roles" => Vec::new(),
			"recent" => self.recent_items.clone(),
			"all" => self.available_items.clone(),
			"provider" => {
				if entry.locked {
					Vec::new()
				} else {
					let provider = entry.provider_id.clone().unwrap_or_default();
					self.available_items
						.iter()
						.filter(|item| item.model.provider == provider)
						.cloned()
						.collect()
				}
			}
			_ => Vec::new(),
		};
		self.candidate_items = items.clone();
		self.candidate_catalog = Some(items);
		self.apply_model_kind();
	}

	/// Replace the candidate catalog, filtering by an active role assignment.
	pub fn set_candidate_items(&mut self, items: Vec<ModelBrowserItem>) {
		let items = match &self.assigning {
			Some(AssignTarget::Role { role }) => items
				.into_iter()
				.filter(|item| self.source.role_accepts(role, &item.model))
				.collect(),
			_ => items,
		};
		self.candidate_items = items.clone();
		self.candidate_catalog = Some(items);
		self.apply_model_kind();
	}

	/// Filter the candidate catalog by the active model-kind tab.
	pub fn apply_model_kind(&mut self) {
		let items = self
			.candidate_catalog
			.clone()
			.unwrap_or_else(|| self.available_items.clone());
		let filtered: Vec<ModelBrowserItem> = items
			.into_iter()
			.filter(|item| {
				self.model_kind_tab == ModelKindTab::All
					|| catalog_model_kind(&item.model) == self.model_kind_tab
			})
			.collect();
		self.candidate_items = filtered.clone();
		self.browser.set_items(filtered);
	}

	/// Recompute search counts and the sidebar after a query change.
	pub fn on_query_changed(&mut self, _query: &str) {
		let query = self.browser.query().to_string();
		if query.trim().is_empty() {
			self.search_counts = None;
			self.recent_search_count = 0;
			self.search_total = 0;
		} else {
			let matches = catalog_matches(&self.available_items, &query);
			self.search_total = matches.len();
			let mut counts: HashMap<String, usize> = HashMap::new();
			for index in matches {
				let provider = self.available_items[index].model.provider.clone();
				*counts.entry(provider).or_insert(0) += 1;
			}
			self.search_counts = Some(counts);
			self.recent_search_count = catalog_matches(&self.recent_items, &query).len();
		}
		self.compose_entries();
		self.request_render();
	}

	/// Rebuild the sidebar from its sections, keeping the active entry when it survives.
	pub fn compose_entries(&mut self) {
		let (entries, vanished) = compose_entries(
			self.fixed_entries.clone(),
			&self.unlocked_provider_entries,
			&self.locked_provider_entries,
			self.search_counts.as_ref(),
			&self.active_entry_id,
		);
		if vanished && !self.entries.is_empty() {
			self.active_entry_id = "all".to_string();
		}
		self.entries = entries;
	}

	/// Reload catalog data from the registry and rebuild every derived list.
	pub fn sync_from_registry_state(&mut self) {
		let all = self.registry.get_all();
		let available = self.registry.get_available();
		self.available_items = build_browser_items(&available);
		let mru = self.source.mru_order();
		self.recent_selectors = mru.iter().cloned().collect();
		self.recent_items = self
			.available_items
			.iter()
			.filter(|item| mru.contains(&item.selector))
			.cloned()
			.collect();
		self.config_error = self.registry.get_error();
		self.roles = resolve_role_assignments(&*self.source, &all, &available);
		self.build_sidebar();
		self.build_roles_rows();
		self.browser.set_roles(self.roles.clone());
		self.browser.set_mru_order(mru);
		self.browser.set_perf_stats(self.source.model_perf());
		self.browser.set_current_selector(self.current_selector.clone());
	}

	/// Build sidebar sections from catalog counts and provider discovery.
	pub fn build_sidebar(&mut self) {
		let all = self.registry.get_all();
		let available = self.registry.get_available();
		let mut available_counts: HashMap<String, usize> = HashMap::new();
		for model in &available {
			*available_counts.entry(model.provider.clone()).or_insert(0) += 1;
		}
		let mut catalog_counts: HashMap<String, usize> = HashMap::new();
		for model in &all {
			*catalog_counts.entry(model.provider.clone()).or_insert(0) += 1;
		}
		let disabled: HashSet<String> = self.source.disabled_providers().into_iter().collect();
		let discoverable = self.registry.get_discoverable_providers();
		let oauth_ids: HashSet<String> = self.registry.oauth_providers().into_iter().collect();
		let authenticated = |provider: &str| available_counts.contains_key(provider);
		let discovery_state = |provider: &str| self.registry.get_provider_discovery_state(provider);
		let (unlocked, locked) = group_providers(&ProviderGroupInput {
			scoped: !self.scoped_models.is_empty(),
			available_counts: &available_counts,
			catalog_counts: &catalog_counts,
			disabled_providers: &disabled,
			discoverable: &discoverable,
			authenticated: &authenticated,
			discovery_state: &discovery_state,
			oauth_ids: &oauth_ids,
		});
		let role_count = self.visible_role_ids().len();
		let fixed = vec![
			SidebarEntryX {
				entry: SidebarEntry::new("roles", "roles", "Roles"),
				provider_id: None,
				locked: false,
				oauth: false,
				catalog_count: role_count,
			},
			SidebarEntryX {
				entry: SidebarEntry::new("all", "all", "All models"),
				provider_id: None,
				locked: false,
				oauth: false,
				catalog_count: available.len(),
			},
			SidebarEntryX {
				entry: SidebarEntry::new("recent", "recent", "Recently used"),
				provider_id: None,
				locked: false,
				oauth: false,
				catalog_count: self.recent_items.len(),
			},
		];
		self.fixed_entries = fixed;
		self.unlocked_provider_entries = unlocked;
		self.locked_provider_entries = locked;
		self.compose_entries();
	}

	/// Refresh derived state after a host mutation.
	pub fn refresh_after_mutation(&mut self) {
		self.sync_from_registry_state();
		self.apply_scope();
		self.request_render();
	}

	/// Record a provider refresh request; the host performs the actual fetch.
	pub fn schedule_provider_refresh(&mut self, provider: &str, force: bool) {
		if force {
			self.scheduled_provider_refreshes.remove(provider);
			self.refreshing_providers.insert(provider.to_string());
		} else {
			self.scheduled_provider_refreshes.insert(provider.to_string());
		}
		self.request_render();
	}

	/// The model a role assignment points at, resolved against the catalog.
	#[must_use]
	pub fn role_assignment_model(&self, role: &str) -> Option<CatalogModel> {
		let selector = self.roles.get(role)?.selector.clone();
		let (provider, id) = selector.split_once('/')?;
		self.registry.find(provider, id)
	}

	/// The model a Roles row edits, if it resolves.
	#[must_use]
	pub fn role_row_model(&self) -> Option<CatalogModel> {
		let row = self.roles_rows.get(self.role_index)?;
		match row {
			RolesRow::Role { role } => self.role_assignment_model(role),
			RolesRow::Fallback { role, chain_index, .. } => {
				let chains = self.fallback_chains();
				let selector = chains.get(role)?.get(*chain_index)?;
				let (provider, id) = selector.split_once('/')?;
				self.registry.find(provider, id)
			}
			_ => None,
		}
	}

	/// Assign `item` to `role` at `scope`.
	pub fn assign_role(
		&mut self,
		item: &ModelBrowserItem,
		role: &str,
		_return_to_roles: bool,
		scope: Option<ModelRoleSelectionScope>,
	) {
		let level = self
			.source
			.default_thinking_level()
			.unwrap_or(ConfiguredThinkingLevel::Inherit);
		let accepted = self.callbacks.on_assign(&item.model, role, level, &item.selector, scope);
		self.strip = None;
		self.assignment_pending = false;
		if accepted {
			self.refresh_after_mutation();
		} else {
			self.request_render();
		}
	}

	/// Clear `role` back to auto-selection.
	pub fn unassign_role(&mut self, role: &str) {
		let scope = match self.source.model_role_storage() {
			ModelRoleStorage::Project => Some(ModelRoleSelectionScope::Project),
			ModelRoleStorage::Global => None,
		};
		self.callbacks.on_unassign(role, scope);
		self.refresh_after_mutation();
	}

	/// Close the footer strip.
	pub fn close_strip(&mut self) {
		self.strip = None;
		self.request_render();
	}

	/// Thinking ladder offered for `model`.
	#[must_use]
	pub fn thinking_options_for(&self, model: &CatalogModel) -> Vec<ConfiguredThinkingLevel> {
		thinking_options(
			model.summary.reasoning,
			false,
			&self.source.thinking_efforts(model),
		)
	}

	/// The item a role's assignment resolves to, for thinking strips.
	#[must_use]
	pub fn role_thinking_target(&self, role: &str) -> Option<ModelBrowserItem> {
		let selector = self.roles.get(role)?.selector.clone();
		self.available_items
			.iter()
			.find(|item| item.selector == selector)
			.cloned()
	}

	/// Open the thinking strip for a role or fallback row.
	pub fn open_thinking_strip(
		&mut self,
		item: ModelBrowserItem,
		role: String,
		return_to_roles: bool,
		scope: Option<ModelRoleSelectionScope>,
	) {
		let options = self.thinking_options_for(&item.model);
		let current = self.roles.get(&role).map(|assignment| assignment.thinking_level);
		let chips = options
			.into_iter()
			.map(|level| StripChip {
				label: thinking_chip_label(level),
				styled: thinking_chip_label(level),
				action: StripAction::Thinking,
				role: Some(role.clone()),
				thinking_level: Some(level),
				scope,
			})
			.collect();
		self.strip = Some(StripState::Role {
			kind: StripKind::Thinking,
			item,
			chips,
			index: 0,
			role: Some(role),
			fallback_index: None,
			scope,
			return_to_roles,
			initial_thinking_level: current,
		});
		self.request_render();
	}

	/// Open the fallback-entry thinking strip.
	pub fn open_fallback_thinking_strip(&mut self, row: &RolesRow) {
		let RolesRow::Fallback { role, chain_index, selector } = row else {
			return;
		};
		let Some((provider, id)) = selector.split_once('/') else {
			return;
		};
		let Some(model) = self.registry.find(provider, id) else {
			return;
		};
		let Some(item) = self
			.available_items
			.iter()
			.find(|item| item.model.provider == provider && item.model.id == id)
			.cloned()
		else {
			return;
		};
		let options = self.thinking_options_for(&model);
		let chips = options
			.into_iter()
			.map(|level| StripChip {
				label: thinking_chip_label(level),
				styled: thinking_chip_label(level),
				action: StripAction::Thinking,
				role: Some(role.clone()),
				thinking_level: Some(level),
				scope: None,
			})
			.collect();
		self.strip = Some(StripState::Role {
			kind: StripKind::Thinking,
			item,
			chips,
			index: 0,
			role: Some(role.clone()),
			fallback_index: Some(*chain_index),
			scope: None,
			return_to_roles: true,
			initial_thinking_level: None,
		});
		self.request_render();
	}

	/// Persist a fallback entry's thinking suffix.
	pub fn set_fallback_thinking(&mut self, role: &str, index: usize, level: ConfiguredThinkingLevel) {
		let Some(selector) = self.fallback_chains().get(role).and_then(|chain| chain.get(index)).cloned() else {
			return;
		};
		let Some(parsed) = parse_fallback_entry(&selector, &|provider, id| {
			self.registry.find(provider, id).is_some()
		}) else {
			return;
		};
		let updated = fallback_thinking_selector(&parsed.provider, &parsed.id, parsed.upstream.as_deref(), level);
		let mut updated_chain = self.fallback_chains().get(role).cloned().unwrap_or_default();
		if index < updated_chain.len() {
			updated_chain[index] = updated;
		}
		self.callbacks.on_fallback_chain_change(role, &updated_chain);
		self.refresh_after_mutation();
	}

	/// Enter assign mode for `role`.
	pub fn start_assign(&mut self, role: &str) {
		self.assigning = Some(AssignTarget::Role { role: role.to_string() });
		self.focus = HubFocus::List;
		self.browser.set_show_provider(true);
		self.set_candidate_items(self.available_items.clone());
		self.browser.set_query("");
		if let Some(current) = self.roles.get(role) {
			let selector = current.selector.clone();
			let _ = self.browser.select_selector(&selector);
		}
		self.request_render();
	}

	/// Enter assign mode for a fallback chain slot.
	pub fn start_assign_fallback(&mut self, role: &str, index: Option<usize>) {
		self.assigning = Some(AssignTarget::Fallback {
			role: role.to_string(),
			index,
		});
		self.focus = HubFocus::List;
		self.browser.set_show_provider(true);
		self.set_candidate_items(self.available_items.clone());
		self.browser.set_query("");
		if let Some(index) = index {
			if let Some(selector) = self.fallback_chains().get(role).and_then(|chain| chain.get(index)) {
				let parsed = parse_fallback_entry(selector, &|provider, id| {
					self.registry.find(provider, id).is_some()
				});
				let base = parsed
					.as_ref()
					.map(|entry| format!("{}/{}", entry.provider, entry.id))
					.unwrap_or_else(|| selector.clone());
				let _ = self.browser.select_selector(&base);
			}
		}
		self.request_render();
	}

	/// Enter assign mode for a brand-new fallback chain's primary model.
	pub fn start_assign_fallback_key(&mut self) {
		self.assigning = Some(AssignTarget::FallbackKey);
		self.focus = HubFocus::List;
		self.browser.set_show_provider(true);
		self.set_candidate_items(self.available_items.clone());
		self.browser.set_query("");
		self.request_render();
	}

	/// Open the fallback-key strip for `item`.
	pub fn open_fallback_key_strip(&mut self, item: &ModelBrowserItem) {
		let chips = vec![
			StripChip {
				label: format!("for {}", item.selector),
				styled: paint(&self.theme, "muted", &format!("for {}", item.selector)),
				action: StripAction::FallbackModel,
				role: None,
				thinking_level: None,
				scope: None,
			},
			StripChip {
				label: format!("for {}/*", item.model.provider),
				styled: paint(&self.theme, "muted", &format!("for {}/*", item.model.provider)),
				action: StripAction::FallbackProvider,
				role: None,
				thinking_level: None,
				scope: None,
			},
		];
		self.strip = Some(StripState::Role {
			kind: StripKind::Role,
			item: item.clone(),
			chips,
			index: 0,
			role: None,
			fallback_index: None,
			scope: None,
			return_to_roles: false,
			initial_thinking_level: None,
		});
		self.request_render();
	}

	/// Write `item` into a fallback chain slot and return to Roles.
	pub fn commit_fallback(&mut self, item: &ModelBrowserItem, role: &str, index: Option<usize>) {
		let mut chain = self.fallback_chains().get(role).cloned().unwrap_or_default();
		let selector = item.selector.clone();
		if let Some(index) = index {
			if index < chain.len() {
				chain[index] = selector.clone();
				chain = dedupe_excluding(&chain, &selector, index);
			} else if !chain.contains(&selector) {
				chain.push(selector.clone());
			}
		} else if !chain.contains(&selector) {
			chain.push(selector.clone());
		}
		self.callbacks.on_fallback_chain_change(role, &chain);
		self.browser.set_query("");
		self.set_active_entry("roles");
		self.focus = HubFocus::List;
		if let Some(row) = self.roles_rows.iter().position(|row_def| {
			matches!(row_def, RolesRow::Fallback { role: row_role, selector: row_selector, .. } if row_role == role && row_selector == &selector)
		}) {
			self.role_index = row;
		}
		self.refresh_after_mutation();
	}

	/// Append `item` to `role`'s chain.
	pub fn append_fallback(&mut self, item: &ModelBrowserItem, role: &str) {
		let mut chain = self.fallback_chains().get(role).cloned().unwrap_or_default();
		if chain.contains(&item.selector) {
			return;
		}
		chain.push(item.selector.clone());
		self.callbacks.on_fallback_chain_change(role, &chain);
		self.refresh_after_mutation();
	}

	/// Remove one chain entry.
	pub fn remove_fallback(&mut self, row: &RolesRow) {
		let RolesRow::Fallback { role, chain_index, .. } = row else {
			return;
		};
		let mut chain = self.fallback_chains().get(role).cloned().unwrap_or_default();
		if *chain_index >= chain.len() {
			return;
		}
		chain.remove(*chain_index);
		self.callbacks.on_fallback_chain_change(role, &chain);
		self.role_index = self.role_index.min(self.roles_rows.len().saturating_sub(1));
		self.refresh_after_mutation();
	}

	/// Move a chain entry one slot.
	pub fn move_fallback(&mut self, row: &RolesRow, delta: isize) {
		let RolesRow::Fallback { role, chain_index, .. } = row else {
			return;
		};
		let mut chain = self.fallback_chains().get(role).cloned().unwrap_or_default();
		let target = *chain_index as isize + delta;
		if *chain_index >= chain.len() || target < 0 || target as usize >= chain.len() {
			return;
		}
		chain.swap(*chain_index, target as usize);
		self.callbacks.on_fallback_chain_change(role, &chain);
		self.role_index = (self.role_index as isize + delta).max(0) as usize;
		self.refresh_after_mutation();
	}

	/// Leave assign mode and return to Roles.
	pub fn cancel_assign(&mut self) {
		self.assigning = None;
		self.browser.set_query("");
		self.set_active_entry("roles");
		self.focus = HubFocus::List;
	}

	/// Toggle `role` in the quick-switch cycle.
	pub fn toggle_cycle_membership(&mut self, role: &str) {
		let mut order = self.source.cycle_order();
		if let Some(index) = order.iter().position(|candidate| candidate == role) {
			order.remove(index);
		} else {
			order.push(role.to_string());
		}
		self.callbacks.on_cycle_order_change(&order);
		self.refresh_after_mutation();
	}

	/// Move `role` one slot in the quick-switch cycle.
	pub fn move_cycle_membership(&mut self, role: &str, delta: isize) {
		let mut order = self.source.cycle_order();
		let Some(index) = order.iter().position(|candidate| candidate == role) else {
			return;
		};
		let target = index as isize + delta;
		if target < 0 || target as usize >= order.len() {
			return;
		}
		order.swap(index, target as usize);
		self.callbacks.on_cycle_order_change(&order);
		self.refresh_after_mutation();
	}

	/// Open the name strip for a new role or preset.
	pub fn open_name_strip(&mut self, purpose: NamePurpose) {
		self.strip = Some(StripState::Name {
			purpose,
			input: Input::new(),
			model: None,
			error: None,
			confirm: None,
			preview: None,
		});
		self.request_render();
	}

	/// Open the compaction-point strip for the selected Roles row.
	pub fn open_compaction_strip(&mut self) {
		let Some(model) = self.role_row_model() else {
			return;
		};
		let value = self
			.source
			.compaction_point_for(&model)
			.and_then(|point| point.draft)
			.unwrap_or_default();
		let mut input = Input::new();
		input.set_value(&value);
		self.strip = Some(StripState::Name {
			purpose: NamePurpose::Compaction,
			input,
			model: Some(model),
			error: None,
			confirm: None,
			preview: None,
		});
		self.request_render();
	}

	/// Submit the name strip.
	pub fn submit_name_strip(&mut self) {
		let Some(strip) = self.strip.take() else {
			return;
		};
		let StripState::Name { purpose, input, model, confirm, .. } = strip else {
			self.strip = Some(strip);
			return;
		};
		match purpose {
			NamePurpose::Compaction => {
				let Some(model) = model else { return };
				let value = input.value().to_string();
				let confirmed = compaction_confirm_pending(confirm.as_ref(), &value);
				let result = self.callbacks.on_compaction_point_change(&model, &value, confirmed);
				match result {
					Some(CompactionPointChangeResult::Error { message }) => {
						self.strip = Some(StripState::Name { purpose, input, model: Some(model), error: Some(message), confirm: None, preview: None });
					}
					Some(CompactionPointChangeResult::Confirm { message }) => {
						self.strip = Some(StripState::Name { purpose, input, model: Some(model), error: None, confirm: Some(ConfirmState { value, message }), preview: None });
					}
					None => {
						self.refresh_after_mutation();
					}
				}
			}
			NamePurpose::Preset => {
				let name = input.value().trim().to_string();
				if is_valid_role_name(&name) {
					self.callbacks.on_save_preset(&name);
					self.refresh_after_mutation();
				} else {
					self.strip = Some(StripState::Name { purpose, input, model: None, error: None, confirm: None, preview: None });
				}
			}
			NamePurpose::Role => {
				let name = input.value().trim().to_string();
				if is_valid_role_name(&name) && !self.visible_role_ids().contains(&name) {
					self.strip = None;
					self.start_assign(&name);
				} else {
					self.strip = Some(StripState::Name { purpose, input, model: None, error: None, confirm: None, preview: None });
				}
			}
		}
		self.request_render();
	}

	/// Apply a saved preset.
	pub fn switch_preset(&mut self, delta: isize) {
		let Some((names, active)) = self.source.get_model_presets() else {
			return;
		};
		if names.is_empty() {
			return;
		}
		let base = preset_base(self.preset_cursor.as_ref(), active.as_deref(), self.source.revision());
		let current = base.and_then(|name| names.iter().position(|candidate| candidate == &name));
		let next = match current {
			Some(index) => (index as isize + delta).rem_euclid(names.len() as isize) as usize,
			None => {
				if delta > 0 { 0 } else { names.len() - 1 }
			}
		};
		let Some(name) = names.get(next) else { return };
		let name = name.clone();
		if Some(&name) == active.as_ref() {
			self.preset_cursor = Some(PresetCursor { name, revision: self.source.revision() });
			self.refresh_after_mutation();
			return;
		}
		let accepted = self.callbacks.on_switch_preset(&name);
		self.preset_cursor = Some(PresetCursor { name, revision: self.source.revision() });
		if accepted {
			self.refresh_after_mutation();
		} else {
			self.request_render();
		}
	}

	/// Cycle the model-kind tab.
	pub fn move_model_kind(&mut self, delta: isize) {
		self.model_kind_tab = self.model_kind_tab.shift(delta);
		self.apply_model_kind();
		self.request_render();
	}

	/// Cycle the role tab.
	pub fn move_role_tab(&mut self, delta: isize) {
		self.role_tab = self.role_tab.shift(delta);
		self.role_index = 0;
		self.role_scroll_start = 0;
		self.build_roles_rows();
		self.request_render();
	}

	/// Keybinding match through the manager.
	#[must_use]
	fn matches_id(&self, key: &Key, id: &str) -> bool {
		self.bindings.matches(key, id)
	}

	/// Handle one key.
	pub fn handle_input(&mut self, key: &Key) {
		self.native_version = self.native_version.wrapping_add(1);
		if self.assignment_pending {
			if self.matches_id(key, "tui.select.cancel") {
				self.callbacks.on_cancel();
			}
			return;
		}
		if let Some(strip) = self.strip.take() {
			self.handle_strip_input(key, strip);
			return;
		}
		if self.matches_id(key, "tui.select.cancel") {
			self.cancel();
			return;
		}
		let Some(entry) = self.active_entry().cloned() else {
			return;
		};
		let roles_view = self.roles_view(&entry);
		let locked_view = self.locked_view(&entry);
		if self.matches_id(key, "tab") || self.matches_id(key, "shift+tab") {
			self.focus = self.focus.toggled();
			self.request_render();
			return;
		}
		if self.matches_id(key, "f5") {
			if entry.entry.kind == "provider" && !entry.locked {
				if let Some(provider) = entry.provider_id.clone() {
					self.schedule_provider_refresh(&provider, true);
				}
			}
			return;
		}
		if roles_view && (self.matches_id(key, "ctrl+left") || self.matches_id(key, "ctrl+right")) {
			self.switch_preset(if self.matches_id(key, "ctrl+left") { -1 } else { 1 });
			return;
		}
		if self.matches_id(key, "alt+left") || self.matches_id(key, "alt+b") {
			if roles_view { self.move_role_tab(-1); } else { self.move_model_kind(-1); }
			return;
		}
		if self.matches_id(key, "alt+right") || self.matches_id(key, "alt+f") {
			if roles_view { self.move_role_tab(1); } else { self.move_model_kind(1); }
			return;
		}
		if self.matches_id(key, "left") {
			self.focus = HubFocus::Scope;
			self.request_render();
			return;
		}
		if self.matches_id(key, "right") {
			if roles_view || self.is_browser_view(&entry) {
				self.focus = HubFocus::List;
			}
			self.request_render();
			return;
		}
		if self.focus == HubFocus::Scope {
			if self.matches_id(key, "tui.select.up") {
				self.move_sidebar(-1);
				return;
			}
			if self.matches_id(key, "tui.select.down") {
				self.move_sidebar(1);
				return;
			}
		}
		if roles_view {
			let printable = key.as_char().filter(|ch| !ch.is_whitespace());
			let edits_query = !self.browser.query().is_empty()
				&& (printable.is_some() || self.matches_id(key, "backspace"));
			if (self.focus == HubFocus::Scope && printable.is_some()) || edits_query {
				self.set_active_entry("all");
				self.focus = HubFocus::List;
				self.browser.handle_input(key);
				let query = self.browser.query().to_string();
				self.on_query_changed(&query);
				return;
			}
			self.handle_roles_view_input(key);
			return;
		}
		if locked_view {
			let printable = key.as_char().filter(|ch| !ch.is_whitespace());
			if printable.is_some() {
				self.set_active_entry("all");
				self.focus = HubFocus::List;
				self.browser.handle_input(key);
				let query = self.browser.query().to_string();
				self.on_query_changed(&query);
				return;
			}
			if self.matches_id(key, "enter") {
				self.request_login(&entry);
			}
			return;
		}
		if self.focus == HubFocus::Scope && self.matches_id(key, "enter") {
			self.focus = HubFocus::List;
			self.request_render();
			return;
		}
		let before = self.browser.query().to_string();
		let printable = key.as_char().is_some();
		self.browser.handle_input(key);
		if printable || self.browser.query() != before {
			self.focus = HubFocus::List;
		}
		let query = self.browser.query().to_string();
		self.on_query_changed(&query);
	}

	/// Cancel ladder: strip, assign mode, query, hub.
	pub fn cancel(&mut self) {
		if self.assignment_pending {
			self.callbacks.on_cancel();
			return;
		}
		if self.strip.is_some() {
			self.close_strip();
			return;
		}
		if self.assigning.is_some() {
			self.cancel_assign();
			return;
		}
		let fallback_entry = SidebarEntryX {
			entry: SidebarEntry::new("all", "all", "All models"),
			provider_id: None,
			locked: false,
			oauth: false,
			catalog_count: 0,
		};
		let entry = self.active_entry().cloned().unwrap_or(fallback_entry);
		if self.is_browser_view(&entry) && !self.browser.query().is_empty() {
			self.browser.handle_cancel();
			self.on_query_changed("");
			return;
		}
		self.callbacks.on_cancel();
	}

	/// Handle a key while the strip is open.
	pub fn handle_strip_input(&mut self, key: &Key, mut strip: StripState) {
		if self.matches_id(key, "tui.select.cancel") {
			self.close_strip();
			return;
		}
		match strip {
			StripState::Name { purpose, ref mut input, .. } => {
				if self.matches_id(key, "enter") {
					self.strip = Some(strip);
					self.submit_name_strip();
					return;
				}
				input.handle_input(key);
				if purpose == NamePurpose::Compaction {
					if let StripState::Name { error, .. } = &mut strip {
						*error = None;
					}
				}
				self.strip = Some(strip);
			}
			StripState::Role { ref mut chips, ref mut index, .. } => {
				let data = crate::keybindings::key_id(key).unwrap_or_default();
				if matches!(data.as_str(), "left" | "up" | "shift+tab") && !chips.is_empty() {
					*index = (*index + chips.len() - 1) % chips.len();
				} else if matches!(data.as_str(), "right" | "down" | "tab") && !chips.is_empty() {
					*index = (*index + 1) % chips.len();
				} else if self.matches_id(key, "enter") {
					self.strip = Some(strip);
					self.activate_strip_chip();
					return;
				}
				self.strip = Some(strip);
			}
		}
		self.request_render();
	}

	/// Activate the selected footer chip.
	pub fn activate_strip_chip(&mut self) {
		let Some(strip) = self.strip.take() else {
			return;
		};
		let StripState::Role { kind: _, item, chips, index, role, fallback_index, scope, return_to_roles, initial_thinking_level } = strip else {
			self.strip = None;
			return;
		};
		let Some(chip) = chips.get(index).cloned() else {
			self.strip = None;
			return;
		};
		match chip.action {
			StripAction::Assign => {
				if let Some(role) = chip.role {
					self.strip = None;
					self.assign_role(&item, &role, return_to_roles, chip.scope);
				}
			}
			StripAction::Unassign => {
				if let Some(role) = chip.role.clone() {
					self.unassign_role(&role);
				}
				self.strip = None;
			}
			StripAction::Fallback => {
				self.append_fallback(&item, "default");
				self.strip = None;
			}
			StripAction::FallbackModel => {
				self.strip = None;
				self.start_assign_fallback(&item.selector, None);
			}
			StripAction::FallbackProvider => {
				self.strip = None;
				self.start_assign_fallback(&format!("{}/*", item.model.provider), None);
			}
			StripAction::Scope => {
				if let (Some(role), Some(scope)) = (role.clone(), chip.scope) {
					self.strip = None;
					self.assign_role(&item, &role, return_to_roles, Some(scope));
				}
			}
			StripAction::Thinking => {
				if let (Some(role), Some(level), Some(fallback_index)) = (role.clone(), chip.thinking_level, fallback_index) {
					self.set_fallback_thinking(&role, fallback_index, level);
					self.strip = None;
					return;
				}
				let changed = chip.thinking_level != initial_thinking_level;
				if let (Some(role), Some(level)) = (role.clone(), chip.thinking_level) {
					if changed {
						self.assign_role(&item, &role, return_to_roles, chip.scope);
					}
				}
				self.strip = None;
			}
		}
		self.request_render();
	}

	/// Move the sidebar selection by `delta`, skipping non-hoppable rows.
	pub fn move_sidebar(&mut self, delta: isize) {
		let count = self.entries.len();
		if count == 0 {
			return;
		}
		let mut index = self
			.entries
			.iter()
			.position(|entry| entry.entry.id == self.active_entry_id)
			.unwrap_or(0) as isize;
		for _ in 0..count {
			index = (index + delta).rem_euclid(count as isize);
			let entry = &self.entries[index as usize];
			if !is_hop_skipped(entry, self.search_counts.as_ref(), self.recent_search_count) {
				if entry.entry.kind == "roles" {
					self.assigning = None;
				}
				let id = entry.entry.id.clone();
				self.set_active_entry(&id);
				return;
			}
		}
	}

	/// Step the Roles cursor by one non-separator row.
	#[must_use]
	pub fn step_role_index(&self, from: usize, delta: isize, wrap: bool) -> usize {
		let count = self.roles_rows.len();
		if count == 0 {
			return 0;
		}
		let mut index = from as isize;
		for _ in 0..count {
			index += delta;
			if index < 0 || index >= count as isize {
				if !wrap {
					return from;
				}
				index = index.rem_euclid(count as isize);
			}
			if !matches!(self.roles_rows[index as usize], RolesRow::Separator) {
				return index as usize;
			}
		}
		from
	}

	/// Keep `role_index` inside a window of `view_height`.
	#[must_use]
	pub fn ensure_role_visible(&self, view_height: usize, total: usize) -> usize {
		if view_height == 0 {
			return 0;
		}
		let start = if self.role_index < self.role_scroll_start {
			self.role_index
		} else if self.role_index >= self.role_scroll_start + view_height {
			self.role_index - view_height + 1
		} else {
			self.role_scroll_start
		};
		start.min(total.saturating_sub(view_height))
	}

	/// Handle a key in the Roles view.
	pub fn handle_roles_view_input(&mut self, key: &Key) {
		if self.focus == HubFocus::Scope {
			if self.matches_id(key, "enter") || self.matches_id(key, "space") {
				self.focus = HubFocus::List;
				self.request_render();
			}
			return;
		}
		if self.matches_id(key, "tui.select.up") {
			self.role_index = self.step_role_index(self.role_index, -1, true);
			self.request_render();
			return;
		}
		if self.matches_id(key, "tui.select.down") {
			self.role_index = self.step_role_index(self.role_index, 1, true);
			self.request_render();
			return;
		}
		if self.matches_id(key, "enter") {
			self.run_roles_action(RolesAction::Pick);
			return;
		}
		if self.matches_id(key, "backspace") || self.matches_id(key, "delete") {
			self.run_roles_action(RolesAction::Clear);
			return;
		}
		if self.matches_id(key, "shift+up") {
			self.run_roles_action(RolesAction::Earlier);
			return;
		}
		if self.matches_id(key, "shift+down") {
			self.run_roles_action(RolesAction::Later);
			return;
		}
		if let Some(action) = roles_action_for_key(key) {
			self.run_roles_action(action);
		}
	}

	/// Run one Roles-view command on the selected row.
	pub fn run_roles_action(&mut self, action: RolesAction) {
		let Some(row) = self.roles_rows.get(self.role_index).cloned() else {
			return;
		};
		let role = match &row {
			RolesRow::Role { role } => Some(role.clone()),
			_ => None,
		};
		match action {
			RolesAction::Pick => self.activate_roles_row(&row),
			RolesAction::Clear => {
				if let Some(role) = role {
					self.unassign_role(&role);
				} else {
					match row {
						RolesRow::Fallback { .. } => self.remove_fallback(&row),
						RolesRow::ChainKey { role } => {
							self.callbacks.on_fallback_chain_change(&role, &[]);
							self.refresh_after_mutation();
						}
						_ => {}
					}
				}
			}
			RolesAction::Fallback => match &row {
				RolesRow::NewFallback => self.start_assign_fallback_key(),
				RolesRow::NewRole | RolesRow::Separator => {}
				_ => {
					let role = match &row {
						RolesRow::Role { role } => role.clone(),
						RolesRow::ChainKey { role } => role.clone(),
						RolesRow::Fallback { role, .. } => role.clone(),
						_ => return,
					};
					self.start_assign_fallback(&role, None);
				}
			},
			RolesAction::Cycle => {
				if let Some(role) = role {
					self.toggle_cycle_membership(&role);
				}
			}
			RolesAction::Earlier | RolesAction::Later => {
				let delta = if action == RolesAction::Earlier { -1 } else { 1 };
				if let Some(role) = role {
					self.move_cycle_membership(&role, delta);
				} else if let RolesRow::Fallback { .. } = row {
					self.move_fallback(&row, delta);
				}
			}
			RolesAction::New => self.open_name_strip(NamePurpose::Role),
			RolesAction::Save => {
				if self.can_save_preset {
					self.open_name_strip(NamePurpose::Preset);
				}
			}
			RolesAction::NextPreset => self.switch_preset(1),
			RolesAction::PrevPreset => self.switch_preset(-1),
			RolesAction::Compaction => self.open_compaction_strip(),
			RolesAction::Thinking => {
				if let Some(role) = role {
					if let Some(item) = self.role_thinking_target(&role) {
						self.open_thinking_strip(item, role, true, None);
					}
				} else if let RolesRow::Fallback { .. } = row {
					self.open_fallback_thinking_strip(&row);
				}
			}
		}
		self.request_render();
	}

	/// Enter/click activation for a Roles row.
	pub fn activate_roles_row(&mut self, row: &RolesRow) {
		match row {
			RolesRow::Role { role } => self.start_assign(role),
			RolesRow::ChainKey { role } => self.start_assign_fallback(role, None),
			RolesRow::Fallback { role, chain_index, .. } => {
				self.start_assign_fallback(role, Some(*chain_index));
			}
			RolesRow::NewFallback => self.start_assign_fallback_key(),
			RolesRow::NewRole => self.open_name_strip(NamePurpose::Role),
			RolesRow::Separator => {}
		}
		self.request_render();
	}

	/// Forward a locked provider click to the login flow.
	pub fn request_login(&mut self, entry: &SidebarEntryX) {
		if let Some(provider) = entry.provider_id.clone() {
			if entry.oauth {
				self.callbacks.on_login_request(&provider);
			}
		}
	}

	/// Pointer activation of a sidebar entry.
	pub fn click_sidebar_entry(&mut self, clicked: Option<&SidebarEntryX>) {
		let Some(clicked) = clicked else { return };
		if clicked.entry.kind == "separator" {
			return;
		}
		let already = clicked.entry.id == self.active_entry_id;
		if clicked.entry.kind == "roles" {
			self.assigning = None;
		}
		let id = clicked.entry.id.clone();
		self.set_active_entry(&id);
		if clicked.entry.kind == "roles" {
			self.focus = HubFocus::List;
		}
		if already && clicked.entry.kind == "provider" && clicked.locked {
			self.request_login(clicked);
		}
		self.request_render();
	}

	/// Sidebar entry index at a content line.
	#[must_use]
	pub fn sidebar_entry_index_at(&self, content_line: isize) -> Option<usize> {
		let index = self.frame.sidebar_scroll as isize + content_line;
		if index < 0 || index >= self.entries.len() as isize {
			return None;
		}
		Some(index as usize)
	}

	/// Route a mouse event through the frame geometry.
	pub fn route_mouse(&mut self, event: &MouseEvent) {
		if self.assignment_pending {
			return;
		}
		let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 40));
		let _ = term_width;
		let height = (term_height as usize).max(16);
		let rows = HubFrame::content_rows(height);
		let sidebar_entries: Vec<SidebarEntry> = self.entries.iter().map(|entry| entry.entry.clone()).collect();
		let left_width = self.frame.sidebar_width(&sidebar_entries);
		let row = event.row as usize;
		let col = event.column as usize;
		let pane = if col >= 2 && col < 2 + left_width {
			Some((PaneSide::Left, row))
		} else {
			Some((PaneSide::Right, row))
		};
		let hit = self.frame.locate(&HubGeometry {
			stack_child: Some((1, row, col)),
			body_height: rows,
			pane,
		});
		let Some(entry) = self.active_entry().cloned() else {
			return;
		};
		if let Some(footer_column) = hit.footer_column {
			if self.strip.is_some() && matches!(event.kind, MouseKind::Down(MouseButtonKind::Left)) {
				if let Some(StripState::Role { chips: strip_chips, index, .. }) = &self.strip {
					let chips: Vec<HubStripChip<StripAction>> = strip_chips
						.iter()
						.map(|chip| HubStripChip::new(chip.label.clone(), chip.styled.clone(), chip.action))
						.collect();
					let mut strip_state = HubStripState::new(chips);
					strip_state.index = *index;
					if self.frame.select_chip_at(&mut strip_state, footer_column) {
						self.request_render();
						self.activate_strip_chip();
					}
				}
			}
			return;
		}
		match event.kind {
			MouseKind::ScrollUp | MouseKind::ScrollDown => {
				let delta = if event.kind == MouseKind::ScrollUp { -1 } else { 1 };
				if hit.over_sidebar {
					self.frame.scroll_sidebar(delta, rows, self.entries.len());
					self.sidebar_hover = self.sidebar_entry_index_at(hit.content_line);
				} else if hit.over_body {
					if self.roles_view(&entry) {
						self.role_index = self.step_role_index(self.role_index, delta, false);
					} else if self.is_browser_view(&entry) && hit.body_line > 0 {
						self.browser.route_mouse(event, hit.body_line as usize - 1);
					}
				}
			}
			MouseKind::Moved | MouseKind::Drag(_) => {
				self.sidebar_hover = if hit.over_sidebar { self.sidebar_entry_index_at(hit.content_line) } else { None };
				if hit.over_body && self.roles_view(&entry) {
					let role_line = hit.body_line - self.roles_row_start as isize;
					self.role_hover = if role_line >= 0 && role_line < self.roles_visible_count as isize {
						Some(role_line as usize + self.role_scroll_start)
					} else {
						None
					};
				} else {
					self.role_hover = None;
					if hit.over_body && self.is_browser_view(&entry) && hit.body_line > 0 {
						self.browser.route_mouse(event, hit.body_line as usize - 1);
					} else {
						self.browser.clear_hover();
					}
				}
			}
			MouseKind::Down(MouseButtonKind::Left) => {
				if hit.over_sidebar {
					let index = self.sidebar_entry_index_at(hit.content_line);
					let clicked = index.and_then(|index| self.entries.get(index)).cloned();
					self.click_sidebar_entry(clicked.as_ref());
				} else if hit.over_body {
					if self.roles_view(&entry) {
						self.focus = HubFocus::List;
						let list_line = hit.body_line - self.roles_row_start as isize;
						if list_line >= 0 && list_line < self.roles_visible_count as isize {
							let role_line = list_line as usize + self.role_scroll_start;
							if let Some(row_def) = self.roles_rows.get(role_line).cloned() {
								if !matches!(row_def, RolesRow::Separator) {
									if role_line == self.role_index {
										self.activate_roles_row(&row_def);
									} else {
										self.role_index = role_line;
									}
								}
							}
						}
					} else if self.locked_view(&entry) {
						if self.locked_login_line == Some(hit.body_line as usize) {
							self.request_login(&entry);
						}
					} else if self.is_browser_view(&entry) && hit.body_line > 0 {
						self.browser.route_mouse(event, hit.body_line as usize - 1);
					}
				}
			}
			_ => {}
		}
		self.request_render();
	}
	/// Visible (non-hidden) role ids.
	#[must_use]
	pub fn visible_role_ids(&self) -> Vec<String> {
		self.source
			.known_role_ids()
			.into_iter()
			.filter(|role| !self.source.get_role_info(role).hidden)
			.collect()
	}

	/// Rebuild the Roles-view rows from the visible roles and chain record.
	pub fn build_roles_rows(&mut self) {
		let chains = self.source.fallback_chains();
		let sections = self
			.source
			.known_role_ids()
			.into_iter()
			.map(|role| {
				let section = self.source.get_role_info(&role).section;
				(role, section)
			})
			.collect();
		self.roles_rows =
			build_roles_rows_with(&chains, &self.visible_role_ids(), &sections, self.role_tab);
	}

	/// Rows the Roles view lists on the All tab, separators aside.
	#[must_use]
	pub fn all_roles_row_count(&self) -> usize {
		let chains = self.source.fallback_chains();
		all_roles_row_count(&chains, &self.visible_role_ids())
	}

	/// Sanitized chain record.
	#[must_use]
	pub fn fallback_chains(&self) -> BTreeMap<String, Vec<String>> {
		sanitized_fallback_chains(&self.source.fallback_chains())
	}

	/// Mark the native described node dirty and ask for a repaint.
	pub fn request_render(&mut self) {
		self.native_version = self.native_version.wrapping_add(1);
	}

	/// The command a Roles-view printable key runs.
	#[must_use]
	pub fn roles_action(&self, key: &Key) -> Option<RolesAction> {
		roles_action_for_key(key)
	}
}

/// Rebuild Roles rows from explicit inputs.
#[must_use]
/// Best-effort model-kind classification for a catalog row.
#[must_use]
pub fn catalog_model_kind(model: &CatalogModel) -> ModelKindTab {
	let id = model.id.to_ascii_lowercase();
	let modalities = model
		.summary
		.input
		.iter()
		.map(|value| value.to_ascii_lowercase())
		.collect::<Vec<_>>();
	let has = |needle: &str| {
		id.contains(needle)
			|| modalities.iter().any(|value| value.contains(needle))
	};
	if model.summary.is_chat {
		return ModelKindTab::Chat;
	}
	if has("video") {
		ModelKindTab::Video
	} else if has("image") {
		ModelKindTab::Image
	} else if has("audio") || has("speech") || has("tts") {
		ModelKindTab::Tts
	} else if has("transcri") || has("stt") {
		ModelKindTab::Stt
	} else if has("embed") {
		ModelKindTab::Embedding
	} else if has("rerank") {
		ModelKindTab::Rerank
	} else if has("search") {
		ModelKindTab::Search
	} else if has("judge") {
		ModelKindTab::Judge
	} else if has("tiny") || has("nano") || has("mini") {
		ModelKindTab::Tiny
	} else {
		ModelKindTab::Chat
	}
}

impl ModelHub {
	/// Terminal height for fullscreen painting.
	#[must_use]
	fn terminal_height() -> usize {
		crossterm::terminal::size()
			.map(|(_, height)| height as usize)
			.unwrap_or(40)
			.max(16)
	}

	/// Sidebar rows in the frame's flat form.
#[must_use]
	fn sidebar_entries(&self) -> Vec<SidebarEntry> {
		self.entries.iter().map(|entry| entry.entry.clone()).collect()
	}

	/// Sidebar decoration shared by the painted and native renderers.
#[must_use]
	fn sidebar_style_static(
		theme: &Theme,
		entry: &SidebarEntry,
		index: usize,
		searching: bool,
		counts: &HashMap<String, usize>,
		recent_count: usize,
		total: usize,
		refreshing: &HashSet<String>,
		hover: Option<usize>,
	) -> SidebarStyle {
		let mut match_count: Option<usize> = None;
		if searching {
			match entry.kind.as_str() {
				"provider" if entry.annotation.is_some() => {}
				_ => {}
			}
		}
		let provider_id = entry
			.id
			.strip_prefix("provider:")
			.map(str::to_string);
		if searching {
			if entry.kind == "provider" {
				match_count = provider_id.as_ref().and_then(|id| counts.get(id)).copied();
			} else if entry.kind == "recent" {
				match_count = Some(recent_count);
			} else if entry.kind == "all" {
				match_count = Some(total);
			}
		}
		let muted = entry.kind == "provider" && entry.annotation.is_none() && false;
		let _ = muted;
		let locked = false;
		let _ = locked;
		let icon = match entry.kind.as_str() {
			"recent" => theme.icon().time.clone(),
			"roles" => theme.icon().extension_skill.clone(),
			"all" => theme.icon().model.clone(),
			_ => theme.status().enabled.clone(),
		};
		let refreshing_now = provider_id
		.as_ref()
		.is_some_and(|id| refreshing.contains(id));
		let annotation = match match_count {
			Some(count) => count.to_string(),
			None => entry.annotation.clone().unwrap_or_default(),
		};
		SidebarStyle {
			icon: paint(theme, "accent", &icon),
			annotation: if refreshing_now {
				paint(theme, "warning", "…")
			} else {
				paint(theme, "dim", &annotation)
			},
			muted: match_count == Some(0),
			hovered: hover == Some(index),
			pad_truncated: true,
		}
	}

	/// Paint the sidebar viewport.
	pub fn render_sidebar(&mut self, width: usize, rows: usize) -> Vec<String> {
		let entries = self.entries.clone();
		let theme = self.theme.clone();
		let searching = self.search_counts.is_some();
		let counts = self.search_counts.clone().unwrap_or_default();
		let recent = self.recent_search_count;
		let total = self.search_total;
		let refreshing = self.refreshing_providers.clone();
		let hover = self.sidebar_hover;
		let active = self.active_entry_id.clone();
		let focused = self.focus == HubFocus::Scope;
		let follow = self.sidebar_follow_active;
		let frame = &mut self.frame;
		let lines = frame.render_sidebar(
			&entries
				.iter()
				.map(|entry| entry.entry.clone())
				.collect::<Vec<_>>(),
			width,
			rows,
			(active.as_str(), focused, follow, true),
			&|entry: &SidebarEntry, index: usize| {
				Self::sidebar_style_static(
					&theme, entry, index, searching, &counts, recent, total, &refreshing, hover,
				)
			},
			&theme,
		);
		self.sidebar_follow_active = false;
		lines
	}

	/// The model-kind tab track.
#[must_use]
	pub fn render_model_kind_tabs(&self, width: usize) -> String {
		let active = ModelKindTab::ALL
			.iter()
			.position(|tab| *tab == self.model_kind_tab)
			.unwrap_or(0);
		let segments: Vec<TrackSegment> = ModelKindTab::ALL
			.iter()
			.map(|tab| TrackSegment::new(tab.label().to_lowercase()))
			.collect();
		let track = render_segment_track(&segments, active, &self.theme);
		truncate_to_width(
			&format!(" {} {}  {}", paint(&self.theme, "dim", "Kind:"), track, ""),
			width,
			Ellipsis::Omit,
		)
	}

	/// The role tab track plus preset hint.
#[must_use]
	pub fn render_role_tabs(&self, width: usize) -> String {
		let labels: Vec<&str> = RoleTab::ALL.iter().map(|tab| match tab {
			RoleTab::All => "All",
			RoleTab::Chat => "Chat",
			RoleTab::Kind => "Kinds",
		}).collect();
		let active = RoleTab::ALL
			.iter()
			.position(|tab| *tab == self.role_tab)
			.unwrap_or(0);
		let segments: Vec<TrackSegment> = labels
			.iter()
			.map(|label| TrackSegment::new((*label).to_string()))
			.collect();
		let track = render_segment_track(&segments, active, &self.theme);
		truncate_to_width(
			&format!(" {} {}", paint(&self.theme, "dim", "Roles:"), track),
			width,
			Ellipsis::Omit,
		)
	}

	/// Saved presets and the active one.
#[must_use]
	pub fn presets(&self) -> (Vec<String>, Option<String>) {
		self.source
			.get_model_presets()
			.map(|(names, active)| (names.clone(), active.clone()))
			.unwrap_or_default()
	}

	/// The body status row.
	#[must_use]
	pub fn status_row(&self, width: usize) -> String {
		if self.assignment_pending {
			return truncate_to_width(
				&paint(&self.theme, "accent", " Applying model…"),
				width,
				Ellipsis::Omit,
			);
		}
		if self.assigning.is_some() {
			return truncate_to_width(
				&paint(&self.theme, "accent", " Assigning model…"),
				width,
				Ellipsis::Omit,
			);
		}
		let Some(entry) = self.active_entry() else {
			return String::new();
		};
		let scoped = if self.scoped_models.is_empty() { "" } else { " · --models scope" };
		let text = match entry.entry.kind.as_str() {
			"recent" => format!("Recently used models{scoped}"),
			"roles" => "Model roles".to_string(),
			"provider" => {
				if entry.locked {
					format!("{} · not configured", entry.entry.label)
				} else {
					format!(
						"{} · {} models{scoped}",
						entry.entry.label,
						entry.entry.annotation.clone().unwrap_or_else(|| "0".to_string())
					)
				}
			}
			_ => format!("All available models{scoped}"),
		};
		if let Some(error) = &self.config_error {
			if entry.entry.kind != "provider" {
				return truncate_to_width(
					&paint(&self.theme, "error", &format!(" {error}")),
					width,
					Ellipsis::Omit,
				);
			}
		}
		truncate_to_width(
			&paint(&self.theme, "muted", &format!(" {text}")),
			width,
			Ellipsis::Omit,
		)
	}

	/// Pad a Roles row and paint its hover band.
#[must_use]
	fn finish_roles_row(&self, line: &str, width: usize, hovered: bool) -> String {
		let out = truncate_to_width(line, width, Ellipsis::Omit);
		if hovered {
			let padded = format!("{out}{}", " ".repeat(width.saturating_sub(visible_width(&out))));
			return paint(&self.theme, "selectedBg", &padded);
		}
		out
	}

	/// Paint the Roles view.
	pub fn render_roles_view(&mut self, width: usize, rows: usize) -> Vec<String> {
		let mut lines = vec![self.render_role_tabs(width)];
		self.roles_row_start = lines.len();
		let mut tag_width = 0;
		for row in &self.roles_rows {
			if let RolesRow::Role { role } = row {
				let info = self.source.get_role_info(role);
				tag_width = tag_width.max(visible_width(info.tag.as_deref().unwrap_or(&info.name)));
			}
		}
		let total = self.roles_rows.len();
		let capacity = rows.saturating_sub(2 + self.roles_row_start);
		let overflow = total > capacity;
		let view_height = if overflow { capacity.saturating_sub(1) } else { capacity };
		self.role_scroll_start = self.ensure_role_visible(view_height, total);
		let end = (self.role_scroll_start + view_height).min(total);
		self.roles_visible_count = end.saturating_sub(self.role_scroll_start);
		for index in self.role_scroll_start..end {
			let Some(row) = self.roles_rows.get(index) else { continue };
			let selected = index == self.role_index;
			let hovered = Some(index) == self.role_hover;
			let cursor = if selected && self.focus == HubFocus::List {
				paint(&self.theme, "accent", &self.theme.nav().cursor)
			} else {
				" ".to_string()
			};
			let line = match row {
				RolesRow::Separator => format!("   {}", paint(&self.theme, "border", &"─".repeat(width.saturating_sub(6).max(1)))),
				RolesRow::NewRole | RolesRow::NewFallback => {
					let label = if matches!(row, RolesRow::NewRole) { "+ New role…" } else { "+ New fallback…" };
					format!(" {cursor} {}", paint(&self.theme, if selected { "accent" } else { "dim" }, label))
				}
				RolesRow::ChainKey { role } => {
					let slash = role.rfind('/').unwrap_or(0);
					format!(" {cursor} {} {}", paint(&self.theme, "dim", &self.theme.status().shadowed), role)
				}
				RolesRow::Fallback { selector, .. } => {
					format!(" {cursor}   ↳ {}", paint(&self.theme, "muted", selector))
				}
				RolesRow::Role { role } => {
					let info = self.source.get_role_info(role);
					let tag = info.tag.clone().unwrap_or_else(|| info.name.clone());
					let tag = format!("{tag:<tag_width$}");
					let assignment = self.roles.get(role);
					let value = match assignment {
						Some(assignment) if !assignment.auto_selected => assignment.selector.clone(),
						Some(assignment) => format!("auto → {}", assignment.selector),
						None => "—".to_string(),
					};
					format!(" {cursor} {} {}  {}", paint(&self.theme, "dim", &self.theme.status().shadowed), paint(&self.theme, "dim", &tag), value)
				}
			};
			lines.push(self.finish_roles_row(&line, width, hovered));
		}
		if overflow {
			lines.push(truncate_to_width(
				&paint(&self.theme, "dim", &format!("   ↑ {} more  ↓ {} more", self.role_scroll_start, total - end)),
				width,
				Ellipsis::Omit,
			));
		}
		while lines.len() < rows.saturating_sub(1) {
			lines.push(String::new());
		}
		if rows >= 2 {
			lines.push(truncate_to_width(
				&paint(&self.theme, "dim", "  cycle is empty — press c on a role to add it"),
				width,
				Ellipsis::Omit,
			));
		}
		lines
	}

	/// Paint a locked provider pane.
	pub fn render_locked_view(&mut self, entry: &SidebarEntryX, width: usize, rows: usize) -> Vec<String> {
		let mut lines = vec![String::new()];
		self.locked_login_line = None;
		lines.push(truncate_to_width(
			&paint(&self.theme, "warning", &format!("  {} has no credentials configured", entry.entry.label)),
			width,
			Ellipsis::Omit,
		));
		lines.push(String::new());
		let env_vars = entry
			.provider_id
			.as_ref()
			.map(|provider| self.registry.provider_env_vars(provider))
			.unwrap_or_default();
		if env_vars.is_empty() {
			lines.push(truncate_to_width(
				&paint(&self.theme, "muted", "  Add an API key for this provider in config."),
				width,
				Ellipsis::Omit,
			));
		} else {
			lines.push(truncate_to_width(
				&paint(&self.theme, "muted", &format!("  Set {} in your environment, or add a key in config.", env_vars.join(" or "))),
				width,
				Ellipsis::Omit,
			));
		}
		if entry.oauth {
			self.locked_login_line = Some(lines.len() + 1);
			lines.push(truncate_to_width(
				&paint(&self.theme, "accent", &format!("  {} Log in with OAuth", self.theme.nav().cursor)),
				width,
				Ellipsis::Omit,
			));
		}
		lines.push(String::new());
		if entry.catalog_count > 0 {
			lines.push(truncate_to_width(
				&paint(&self.theme, "dim", &format!("  {} models in catalog:", entry.catalog_count)),
				width,
				Ellipsis::Omit,
			));
			if self.scoped_models.is_empty() {
				for model in self.registry.get_all() {
					if Some(&model.provider) != entry.provider_id.as_ref() {
						continue;
					}
					if lines.len() >= rows {
						break;
					}
					lines.push(truncate_to_width(
						&paint(&self.theme, "dim", &format!("    {}", model.id)),
						width,
						Ellipsis::Omit,
					));
				}
			}
		}
		while lines.len() < rows {
			lines.push(String::new());
		}
		lines.truncate(rows);
		lines
	}

	/// Footer hint text.
	#[must_use]
	pub fn footer_hint(&self) -> String {
		let Some(entry) = self.active_entry() else {
			return "esc close".to_string();
		};
		if self.strip.is_some() {
			return "←/→ choose · enter apply · esc cancel".to_string();
		}
		if self.assigning.is_some() {
			return "enter pick · esc cancel".to_string();
		}
		if entry.entry.kind == "roles" {
			return "↑/↓ rows · enter pick · f fallback · x clear · c cycle · n new · esc close".to_string();
		}
		if entry.locked {
			return "enter log in · esc close".to_string();
		}
		"↑/↓ models · type to search · esc close".to_string()
	}

	/// Paint the footer, including an open strip.
	pub fn render_footer(&mut self, width: usize) -> String {
		let hint = self.footer_hint();
		let Some(strip) = self.strip.take() else {
			return self.frame.render_footer(width, &hint, None, &self.theme);
		};
		let rendered = self.render_strip(width, &strip);
		self.strip = Some(strip);
		self.frame.render_footer(width, &hint, Some(&|| rendered.clone()), &self.theme)
	}

	/// Paint one footer strip.
#[must_use]
	pub fn render_strip(&mut self, width: usize, strip: &StripState) -> String {
		match strip {
			StripState::Name { purpose, input, model, .. } => {
				let label = match purpose {
					NamePurpose::Compaction => format!("{} limit:", model.as_ref().map(|model| model.id.clone()).unwrap_or_default()),
					NamePurpose::Preset => "Preset name:".to_string(),
					NamePurpose::Role => "New role name:".to_string(),
				};
				let input_width = width.saturating_sub(visible_width(&label) + 24).max(8).min(32);
				let mut scratch = Input::new();
				scratch.set_value(input.value());
				let mut rendered = scratch.render(input_width as u16);
				let line = rendered.pop().unwrap_or_default();
				truncate_to_width(
					&format!("{} {} {}", paint(&self.theme, "accent", &label), line, paint(&self.theme, "dim", "(letters, digits, - and _)")),
					width,
					Ellipsis::Omit,
				)
			}
			StripState::Role { item, chips, index, .. } => {
				let prefix = format!("{} → ", paint(&self.theme, "accent", &item.id));
				let hub_chips: Vec<HubStripChip<StripAction>> = chips
					.iter()
					.map(|chip| HubStripChip::new(chip.label.clone(), chip.styled.clone(), chip.action))
					.collect();
				let mut state = HubStripState::new(hub_chips);
				state.index = *index;
				self.frame.render_chips(width, &prefix, &state, 0, &self.theme)
			}
		}
	}

	/// Paint the body pane.
pub fn render_body(&mut self, width: usize, rows: usize) -> Vec<String> {
		let mut lines = vec![self.status_row(width)];
		let Some(entry) = self.active_entry().cloned() else {
			lines.resize(rows, String::new());
			return lines;
		};
		if self.roles_view(&entry) {
			lines.extend(self.render_roles_view(width, rows.saturating_sub(1)));
		} else if self.locked_view(&entry) {
			lines.extend(self.render_locked_view(&entry, width, rows.saturating_sub(1)));
		} else {
			lines.push(self.render_model_kind_tabs(width));
			self.browser
				.set_max_visible(rows.saturating_sub(2).max(1));
			lines.extend(self.browser.render(width as u16));
		}
		lines.truncate(rows);
		while lines.len() < rows {
			lines.push(String::new());
		}
		lines
	}

	/// Native status node.
#[must_use]
	fn describe_status(&self) -> NativeNode {
		let spans = if self.assignment_pending {
			vec![span("Applying model…", Some("accent"))]
		} else if self.assigning.is_some() {
			vec![span("Assigning model…", Some("accent"))]
		} else if let Some(error) = &self.config_error {
			vec![span(error.clone(), Some("error"))]
		} else {
			vec![span(self.status_row(usize::MAX).trim().to_string(), Some("muted"))]
		};
		text(NodeText::Spans(spans), Props::new())
	}

	/// Native Roles view.
#[must_use]
	fn describe_roles_view(&self) -> NativeNode {
		let items: Vec<NativeNode> = self
			.roles_rows
			.iter()
			.enumerate()
			.map(|(index, row)| {
				let key = roles_row_key(row, index);
				let label = match row {
					RolesRow::Separator => {
						return node(NodeKind::Rule, Props::new(), Vec::new(), Some(key.as_str()));
					}
					RolesRow::NewRole => "+ New role…".to_string(),
					RolesRow::NewFallback => "+ New fallback…".to_string(),
					RolesRow::ChainKey { role } => role.clone(),
					RolesRow::Fallback { selector, .. } => selector.clone(),
					RolesRow::Role { role } => role.clone(),
				};
				let mut props = Props::new();
				props.set_spans(vec![span(label, None)]);
				item(key, props)
			})
			.collect();
		let selected = self
			.roles_rows
			.get(self.role_index)
			.map(|row| roles_row_key(row, self.role_index));
		let mut props = Props::new();
		props.set_str("selected", selected.as_deref().unwrap_or(""));
		props.set_str("role", "omp.model-hub.roles");
		list(items, props, Some("roles"))
	}

	/// Native locked-provider pane.
#[must_use]
	fn describe_locked_view(&self, entry: &SidebarEntryX) -> NativeNode {
		let children = vec![text(
			NodeText::Spans(vec![span(
				format!("{} has no credentials configured", entry.entry.label),
				Some("warning"),
			)]),
			Props::new(),
		)];
		let mut props = Props::new();
		props.set_str("gap", "sm");
		col(children, props)
	}

	/// Native body node.
#[must_use]
	fn describe_body(&self) -> NativeNode {
		let mut children = vec![self.describe_status()];
		let Some(entry) = self.active_entry() else {
			let mut props = Props::new();
			props.set_str("gap", "sm");
			return col(children, props);
		};
		if self.roles_view(&entry) {
			children.push(self.describe_roles_view());
		} else if self.locked_view(&entry) {
			children.push(self.describe_locked_view(&entry));
		} else {
			let mut tabs = Props::new();
			tabs.insert(
				"items",
				serde_json::Value::Array(
					ModelKindTab::ALL
						.iter()
						.map(|tab| serde_json::json!({ "id": tab.label().to_lowercase(), "label": tab.label() }))
						.collect(),
				),
			);
			tabs.set_str("active", self.model_kind_tab.label().to_lowercase());
			children.push(node(NodeKind::Tabs, tabs, Vec::new(), Some("kinds")));
			children.push(text(
				NodeText::Spans(vec![span(
					format!("{} models", self.browser.base_count()),
					Some("muted"),
				)]),
				Props::new(),
			));
		}
		let mut props = Props::new();
		props.set_str("gap", "sm");
		col(children, props)
	}

	/// Footer hints for the native description.
#[must_use]
	fn hints(&self) -> Vec<Option<NativeHint>> {
		vec![
			action_hint(&self.bindings, "tui.select.up", "up"),
			action_hint(&self.bindings, "tui.select.down", "down"),
			action_hint(&self.bindings, "tui.select.cancel", "close"),
		]
	}
}

impl Component for ModelHub {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width as usize;
		let height = Self::terminal_height();
		let rows = HubFrame::content_rows(height);
		let footer = self.render_footer(width.saturating_sub(4));
		let sidebar = self.render_sidebar(width, rows);
		let body = self.render_body(width, rows);
		let entries = self.sidebar_entries();
		self.frame.render(width, height, &entries, &footer, &sidebar, &body)
	}

	fn handle_input(&mut self, key: &Key) {
		ModelHub::handle_input(self, key);
	}

	fn handle_mouse(&mut self, event: &MouseEvent) {
		self.route_mouse(event);
	}

	fn handle_paste(&mut self, text: &str) {
		self.native_version = self.native_version.wrapping_add(1);
		if let Some(StripState::Name { input, .. }) = self.strip.as_mut() {
			input.handle_paste(text);
		} else {
			self.browser.handle_paste(text);
			let query = self.browser.query().to_string();
			self.on_query_changed(&query);
		}
		self.request_render();
	}

	fn invalidate(&mut self) {
		self.native_version = self.native_version.wrapping_add(1);
	}

	fn dispose(&mut self) {
		self.disposed = true;
		self.strip = None;
		self.browser.clear_hover();
	}
}

impl NativeDescribe for ModelHub {
	fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
		let entries = self.sidebar_entries();
		let active = self.active_entry_id.clone();
		let searching = self.search_counts.is_some();
		let counts = self.search_counts.clone().unwrap_or_default();
		let recent = self.recent_search_count;
		let total = self.search_total;
		let refreshing = self.refreshing_providers.clone();
		let hover = self.sidebar_hover;
		let theme = self.theme.clone();
		let sidebar = describe_hub_sidebar(
			&entries,
			&active,
			&|entry: &SidebarEntry, index: usize| {
				let style = Self::sidebar_style_static(
					&theme, entry, index, searching, &counts, recent, total, &refreshing, hover,
				);
				(style.icon.clone(), style.annotation.clone(), style.muted)
			},
			"scopes",
		);
		let body = self.describe_body();
		let mut footer_props = Props::new();
		footer_props.set_str("gap", "xs");
		let footer = node(
			NodeKind::Col,
			footer_props,
			vec![hints_row(&self.hints(), "hints")],
			Some("footer"),
		);
		describe_hub_frame(
			"omp.overlay.model-hub",
			NodeText::Plain("Models".to_string()),
			sidebar,
			body,
			footer,
		)
	}
}

pub fn build_roles_rows_with(
	chains: &BTreeMap<String, Vec<String>>,
	visible_roles: &[String],
	sections: &BTreeMap<String, RoleSection>,
	role_tab: RoleTab,
) -> Vec<RolesRow> {
	let mut rows = Vec::new();
	let append_roles = |rows: &mut Vec<RolesRow>, roles: &[String]| {
		for role in roles {
			rows.push(RolesRow::Role { role: role.clone() });
			let chain = chains.get(role).cloned().unwrap_or_default();
			for (index, selector) in chain.iter().enumerate() {
				rows.push(RolesRow::Fallback {
					role: role.clone(),
					chain_index: index,
					selector: selector.clone(),
				});
			}
		}
	};

	let section = |role: &str| sections.get(role).copied().unwrap_or(RoleSection::Chat);
	let visible: Vec<String> = visible_roles.to_vec();
	match role_tab {
		RoleTab::All => {
			let chat_roles: Vec<String> = visible
				.iter()
				.filter(|role| section(role) == RoleSection::Chat)
				.cloned()
				.collect();
			let kind_roles: Vec<String> = visible
				.iter()
				.filter(|role| section(role) == RoleSection::Kind)
				.cloned()
				.collect();
			append_roles(&mut rows, &chat_roles);
			if !chat_roles.is_empty() && !kind_roles.is_empty() {
				rows.push(RolesRow::Separator);
			}
			append_roles(&mut rows, &kind_roles);
		}
		RoleTab::Chat | RoleTab::Kind => {
			let wanted = if role_tab == RoleTab::Chat { RoleSection::Chat } else { RoleSection::Kind };
			let filtered: Vec<String> = visible
				.iter()
				.filter(|role| section(role) == wanted)
				.cloned()
				.collect();
			append_roles(&mut rows, &filtered);
		}
	}
	rows.push(RolesRow::NewRole);
	rows.push(RolesRow::Separator);
	let mut model_keys: Vec<&String> = chains.keys().filter(|key| key.contains('/')).collect();
	model_keys.sort();
	for key in model_keys {
		let chain = chains.get(key).cloned().unwrap_or_default();
		rows.push(RolesRow::ChainKey { role: key.clone() });
		for (index, selector) in chain.iter().enumerate() {
			rows.push(RolesRow::Fallback {
				role: key.clone(),
				chain_index: index,
				selector: selector.clone(),
			});
		}
	}
	rows.push(RolesRow::NewFallback);
	rows
}

/// Rows the All tab lists, separators aside.
#[must_use]
pub fn all_roles_row_count(chains: &BTreeMap<String, Vec<String>>, visible_roles: &[String]) -> usize {
	let mut count = 2;
	for role in visible_roles {
		count += 1 + chains.get(role).map(Vec::len).unwrap_or(0);
	}
	for (key, chain) in chains {
		if key.contains('/') {
			count += 1 + chain.len();
		}
	}
	count
}

/// Catalog items matching `query`, best first, via the crate fuzzy ranker.
#[must_use]
pub fn catalog_matches(items: &[ModelBrowserItem], query: &str) -> Vec<usize> {
	let texts: Vec<String> = items.iter().map(|item| model_search_text(item)).collect();
	crate::fuzzy::fuzzy_rank(&texts, query, |text| text.as_str())
		.into_iter()
		.map(|ranked| ranked.index)
		.collect()
}

/// A model-hub source that also answers registry questions, for hosts that keep
/// both in one object.
pub trait ModelHubHost: ModelHubSource + ModelHubRegistry {}

impl<T> ModelHubHost for T where T: ModelHubSource + ModelHubRegistry {}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::component::Component;
	use crate::native::describe::NativeDescribe;

	impl ModelHubSource for StaticModelBrowserSource {}

	fn model(provider: &str, id: &str) -> CatalogModel {
		CatalogModel {
			provider: provider.to_string(),
			id: id.to_string(),
			summary: ModelSummary { name: id.to_string(), ..ModelSummary::default() },
		}
	}

	fn browser_item(provider: &str, id: &str) -> ModelBrowserItem {
		ModelBrowserItem {
			provider: provider.to_string(),
			id: id.to_string(),
			selector: format!("{provider}/{id}"),
			model: model(provider, id),
			label_color: None,
		}
	}

	#[test]
	fn roles_row_keys_are_stable() {
		assert_eq!(roles_row_key(&RolesRow::Role { role: "plan".into() }, 0), "role:plan");
		assert_eq!(roles_row_key(&RolesRow::ChainKey { role: "p/m".into() }, 3), "chain:p/m");
		assert_eq!(
			roles_row_key(
				&RolesRow::Fallback { role: "p/m".into(), chain_index: 2, selector: "a/b".into() },
				0
			),
			"fallback:p/m:2"
		);
		assert_eq!(roles_row_key(&RolesRow::Separator, 4), "sep:4");
		assert_eq!(roles_row_key(&RolesRow::NewRole, 0), "newRole");
		assert_eq!(roles_row_key(&RolesRow::NewFallback, 7), "newFallback");
	}

	#[test]
	fn roles_rows_group_chat_then_kind_and_chain_keys() {
		let mut chains = BTreeMap::new();
		chains.insert(
			"plan".to_string(),
			vec!["anthropic/claude".to_string(), "openai/gpt".to_string()],
		);
		chains.insert("p/m".to_string(), vec!["a/b".to_string()]);
		let mut sections = BTreeMap::new();
		sections.insert("default".to_string(), RoleSection::Chat);
		sections.insert("plan".to_string(), RoleSection::Chat);
		sections.insert("image".to_string(), RoleSection::Kind);
		let visible =
			vec!["default".to_string(), "plan".to_string(), "image".to_string()];
		let rows = build_roles_rows_with(&chains, &visible, &sections, RoleTab::All);
		assert_eq!(rows[0], RolesRow::Role { role: "default".into() });
		assert_eq!(rows[1], RolesRow::Role { role: "plan".into() });
		assert_eq!(
			rows[2],
			RolesRow::Fallback {
				role: "plan".into(),
				chain_index: 0,
				selector: "anthropic/claude".into()
			}
		);
		assert_eq!(rows[4], RolesRow::Separator);
		assert_eq!(rows[5], RolesRow::Role { role: "image".into() });
		assert_eq!(rows[6], RolesRow::NewRole);
		assert_eq!(rows[7], RolesRow::Separator);
		assert_eq!(rows[8], RolesRow::ChainKey { role: "p/m".into() });
		assert_eq!(rows[10], RolesRow::NewFallback);

		let chat_only = build_roles_rows_with(&chains, &visible, &sections, RoleTab::Chat);
		assert!(!chat_only
			.iter()
			.any(|row| matches!(row, RolesRow::Role { role } if role == "image")));

		let kind_only = build_roles_rows_with(&chains, &visible, &sections, RoleTab::Kind);
		assert!(kind_only
			.iter()
			.any(|row| matches!(row, RolesRow::Role { role } if role == "image")));
		assert!(!kind_only
			.iter()
			.any(|row| matches!(row, RolesRow::Role { role } if role == "default")));
	}

	#[test]
	fn all_roles_row_count_matches_the_all_tab() {
		let mut chains = BTreeMap::new();
		chains.insert("plan".to_string(), vec!["a/b".to_string(), "c/d".to_string()]);
		chains.insert("p/m".to_string(), vec!["a/b".to_string()]);
		let visible = vec!["default".to_string(), "plan".to_string()];
		// 2 (new role + new fallback) + default(1) + plan(1+2) + chain key p/m(1+1)
		assert_eq!(all_roles_row_count(&chains, &visible), 2 + 1 + 3 + 2);
	}

	#[test]
	fn role_tabs_cycle() {
		assert_eq!(RoleTab::All.shift(1), RoleTab::Chat);
		assert_eq!(RoleTab::Chat.shift(1), RoleTab::Kind);
		assert_eq!(RoleTab::Kind.shift(1), RoleTab::All);
		assert_eq!(RoleTab::All.shift(-1), RoleTab::Kind);
	}

	#[test]
	fn kind_tabs_cover_every_label_and_cycle() {
		assert_eq!(ModelKindTab::ALL.len(), 11);
		assert_eq!(ModelKindTab::All.label(), "All");
		assert_eq!(ModelKindTab::Tts.label(), "TTS");
		assert_eq!(ModelKindTab::Stt.label(), "STT");
		assert_eq!(ModelKindTab::Video.shift(-1), ModelKindTab::Rerank);
	}

	#[test]
	fn roles_action_keys_match_the_reference() {
		assert_eq!(roles_action_for_char('x'), Some(RolesAction::Clear));
		assert_eq!(roles_action_for_char('f'), Some(RolesAction::Fallback));
		assert_eq!(roles_action_for_char('c'), Some(RolesAction::Cycle));
		assert_eq!(roles_action_for_char('['), Some(RolesAction::Earlier));
		assert_eq!(roles_action_for_char(']'), Some(RolesAction::Later));
		assert_eq!(roles_action_for_char('n'), Some(RolesAction::New));
		assert_eq!(roles_action_for_char('t'), Some(RolesAction::Thinking));
		assert_eq!(roles_action_for_char('k'), Some(RolesAction::Compaction));
		assert_eq!(roles_action_for_char('s'), Some(RolesAction::Save));
		assert_eq!(roles_action_for_char('p'), Some(RolesAction::NextPreset));
		assert_eq!(roles_action_for_char('P'), Some(RolesAction::PrevPreset));
		assert_eq!(roles_action_for_char('z'), None);
	}

	#[test]
	fn role_names_require_leading_letter() {
		assert!(is_valid_role_name("plan"));
		assert!(is_valid_role_name("my-role_2"));
		assert!(!is_valid_role_name("2plan"));
		assert!(!is_valid_role_name(""));
		assert!(!is_valid_role_name("has space"));
	}

	#[test]
	fn thinking_options_follow_reasoning_support() {
		assert!(thinking_options(false, false, &[ConfiguredThinkingLevel::High]).is_empty());
		let unverified = thinking_options(true, true, &[ConfiguredThinkingLevel::High]);
		assert_eq!(
			unverified,
			vec![
				ConfiguredThinkingLevel::Inherit,
				ConfiguredThinkingLevel::Off,
				ConfiguredThinkingLevel::High
			]
		);
		let verified = thinking_options(true, false, &[ConfiguredThinkingLevel::High]);
		assert_eq!(
			verified,
			vec![
				ConfiguredThinkingLevel::Inherit,
				ConfiguredThinkingLevel::Off,
				AUTO_THINKING,
				ConfiguredThinkingLevel::High
			]
		);
		assert_eq!(thinking_chip_label(ConfiguredThinkingLevel::High), "high");
	}

	#[test]
	fn fallback_parsing_prefers_literal_over_routing() {
		let is_literal =
			|provider: &str, id: &str| provider == "google-vertex" && id == "claude-opus-4-8@default";
		let parsed =
			parse_fallback_entry("google-vertex/claude-opus-4-8@default", &is_literal).unwrap();
		assert_eq!(parsed.provider, "google-vertex");
		assert_eq!(parsed.id, "claude-opus-4-8@default");
		assert_eq!(parsed.upstream, None);

		let routed =
			parse_fallback_entry("openrouter/z-ai/glm-4.7@fireworks", &|_p, _id| false).unwrap();
		assert_eq!(routed.provider, "openrouter");
		assert_eq!(routed.id, "z-ai/glm-4.7");
		assert_eq!(routed.upstream.as_deref(), Some("fireworks"));

		let effort = parse_fallback_entry("anthropic/claude:low", &|_p, _id| true).unwrap();
		assert_eq!(effort.thinking_level, Some(ConfiguredThinkingLevel::Low));
		assert_eq!(parse_fallback_entry("   ", &|_p, _id| true), None);
	}

	#[test]
	fn fallback_chains_commit_append_remove_and_move() {
		let chain = vec!["a/b".to_string(), "c/d".to_string()];
		assert_eq!(commit_fallback_chain(&chain, Some(0), "c/d"), vec!["c/d".to_string()]);
		assert_eq!(
			commit_fallback_chain(&chain, None, "e/f"),
			vec!["a/b".to_string(), "c/d".to_string(), "e/f".to_string()]
		);
		assert_eq!(
			commit_fallback_chain(&chain, None, "a/b"),
			vec!["a/b".to_string(), "c/d".to_string()]
		);
		assert_eq!(append_fallback(&chain, "c/d"), chain);
		assert_eq!(remove_fallback(&chain, 0), vec!["c/d".to_string()]);
		assert_eq!(move_fallback(&chain, 0, 1), vec!["c/d".to_string(), "a/b".to_string()]);
		assert_eq!(move_fallback(&chain, 0, -1), chain);
		assert_eq!(
			fallback_thinking_selector(
				"openai",
				"gpt-5.5",
				Some("fireworks"),
				ConfiguredThinkingLevel::Off
			),
			"openai/gpt-5.5@fireworks:off"
		);
		assert_eq!(
			fallback_thinking_selector("openai", "gpt-5.5", None, ConfiguredThinkingLevel::Inherit),
			"openai/gpt-5.5"
		);
	}

	#[test]
	fn fallback_resolution_skips_wildcards_and_unknown_models() {
		let mut chains = BTreeMap::new();
		chains.insert(
			"default".to_string(),
			vec!["provider/*".to_string(), "acme/good".to_string()],
		);
		let lookup = |provider: &str, id: &str| {
			(provider == "acme" && id == "good").then(|| model("acme", "good"))
		};
		assert!(resolve_fallback_entry(&chains, "default", 0, &lookup).is_none());
		let resolved = resolve_fallback_entry(&chains, "default", 1, &lookup).unwrap();
		assert_eq!(resolved.item.selector, "acme/good");
		assert!(resolve_fallback_entry(&chains, "missing", 0, &lookup).is_none());
	}

	#[test]
	fn cycle_membership_toggles_and_moves() {
		let order = vec!["plan".to_string(), "commit".to_string()];
		assert_eq!(toggle_cycle_membership(&order, "plan"), vec!["commit".to_string()]);
		assert_eq!(
			toggle_cycle_membership(&order, "image"),
			vec!["plan".to_string(), "commit".to_string(), "image".to_string()]
		);
		assert_eq!(
			move_cycle_membership(&order, "plan", 1),
			vec!["commit".to_string(), "plan".to_string()]
		);
		assert_eq!(move_cycle_membership(&order, "plan", -1), order);
	}

	#[test]
	fn preset_stepping_starts_from_cursor_or_active() {
		let names = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
		assert_eq!(preset_base(None, Some("beta"), 1), Some("beta".to_string()));
		let cursor = PresetCursor { name: "alpha".to_string(), revision: 1 };
		assert_eq!(preset_base(Some(&cursor), Some("beta"), 2), Some("beta".to_string()));
		assert_eq!(preset_base(Some(&cursor), None, 2), Some("alpha".to_string()));
		assert_eq!(switch_preset_name(&names, Some("alpha"), None, 1, 1), Some("beta".to_string()));
		assert_eq!(
			switch_preset_name(&names, Some("gamma"), None, 1, 1),
			Some("alpha".to_string())
		);
		assert_eq!(switch_preset_name(&names, None, None, 1, -1), Some("gamma".to_string()));
	}

	#[test]
	fn compaction_notice_prefers_error_then_warning_then_preview() {
		let confirm = ConfirmState { value: "80%".into(), message: "extends window".into() };
		assert!(compaction_confirm_pending(Some(&confirm), "80%"));
		assert!(!compaction_confirm_pending(Some(&confirm), "400k"));
		let notice = compaction_notice(Some(&"bad".into()), None, "", None);
		assert_eq!(notice.style, NoticeStyle::Error);
		let notice = compaction_notice(None, Some(&confirm), "80%", None);
		assert_eq!(notice.style, NoticeStyle::Warning);
		let notice = compaction_notice(None, Some(&confirm), "400k", Some("compacts at 340K"));
		assert_eq!(notice.text, "compacts at 340K");
		assert_eq!(notice.style, NoticeStyle::Dim);
		let notice = compaction_notice(None, None, "", None);
		assert_eq!(notice.text, COMPACTION_INPUT_HINT);
		assert_eq!(sanitize_warning("a\nb\tc"), "a b c");
	}

	#[test]
	fn provider_grouping_splits_locked_from_selectable() {
		let mut available = HashMap::new();
		available.insert("acme".to_string(), 2);
		let mut catalog = HashMap::new();
		catalog.insert("acme".to_string(), 2);
		catalog.insert("locked".to_string(), 3);
		let disabled = HashSet::new();
		let discoverable = vec!["local".to_string()];
		let authenticated = |_provider: &str| false;
		let discovery_state = |provider: &str| {
			(provider == "local").then(|| ProviderDiscoveryState {
				optional: true,
				status: DiscoveryStatus::Idle,
				fetched_at: None,
				error: None,
			})
		};
		let oauth_ids = HashSet::new();
		let input = ProviderGroupInput {
			scoped: false,
			available_counts: &available,
			catalog_counts: &catalog,
			disabled_providers: &disabled,
			discoverable: &discoverable,
			authenticated: &authenticated,
			discovery_state: &discovery_state,
			oauth_ids: &oauth_ids,
		};
		let (unlocked, locked) = group_providers(&input);
		assert_eq!(unlocked.len(), 1);
		assert_eq!(unlocked[0].entry.label, "acme");
		assert_eq!(unlocked[0].entry.annotation.as_deref(), Some("2"));
		assert_eq!(locked.len(), 1);
		assert_eq!(locked[0].entry.label, "locked");
		assert!(locked[0].entry.annotation.is_none());
	}

	#[test]
	fn compose_orders_matched_providers_first() {
		let fixed = vec![SidebarEntryX {
			entry: SidebarEntry::new("all", "all", "All models"),
			provider_id: None,
			locked: false,
			oauth: false,
			catalog_count: 4,
		}];
		let a = SidebarEntryX::provider("acme", false, Some("1".into()), false, 1);
		let z = SidebarEntryX::provider("zeta", false, Some("0".into()), false, 1);
		let mut counts = HashMap::new();
		counts.insert("zeta".to_string(), 2);
		let (entries, missing) = compose_entries(fixed, &[a, z], &[], Some(&counts), "all");
		assert!(!missing);
		assert_eq!(entries[1].entry.kind, "separator");
		assert_eq!(entries[2].entry.label, "zeta");
		assert_eq!(entries[3].entry.label, "acme");
	}

	#[test]
	fn compose_reports_a_missing_active_entry() {
		let fixed = vec![SidebarEntryX {
			entry: SidebarEntry::new("all", "all", "All models"),
			provider_id: None,
			locked: false,
			oauth: false,
			catalog_count: 4,
		}];
		let (entries, missing) = compose_entries(fixed, &[], &[], None, "gone");
		assert!(missing);
		assert_eq!(entries.len(), 1);
	}

	#[test]
	fn hop_skip_rules_match_the_reference() {
		let mut counts = HashMap::new();
		counts.insert("acme".to_string(), 1);
		let roles = SidebarEntryX {
			entry: SidebarEntry::new("roles", "roles", "Roles"),
			provider_id: None,
			locked: false,
			oauth: false,
			catalog_count: 0,
		};
		let recent = SidebarEntryX {
			entry: SidebarEntry::new("recent", "recent", "Recent"),
			provider_id: None,
			locked: false,
			oauth: false,
			catalog_count: 0,
		};
		let acme = SidebarEntryX::provider("acme", false, None, false, 1);
		let locked = SidebarEntryX::provider("locked", true, None, false, 1);
		assert!(is_hop_skipped(&roles, Some(&counts), 1));
		assert!(is_hop_skipped(&recent, Some(&counts), 0));
		assert!(!is_hop_skipped(&recent, Some(&counts), 1));
		assert!(!is_hop_skipped(&acme, Some(&counts), 0));
		assert!(is_hop_skipped(&locked, Some(&counts), 0));
		assert!(!is_hop_skipped(&acme, None, 0));
	}

	#[test]
	fn anchors_restore_and_fall_back_to_nearest() {
		let entries = vec![
			SidebarEntryX {
				entry: SidebarEntry::new("all", "all", "All"),
				provider_id: None,
				locked: false,
				oauth: false,
				catalog_count: 0,
			},
			SidebarEntryX {
				entry: SidebarEntry::new("sep", "separator", ""),
				provider_id: None,
				locked: false,
				oauth: false,
				catalog_count: 0,
			},
			SidebarEntryX {
				entry: SidebarEntry::new("acme", "provider", "acme"),
				provider_id: Some("acme".into()),
				locked: false,
				oauth: false,
				catalog_count: 1,
			},
		];
		let anchor = capture_anchor(&entries, "acme", 0);
		assert_eq!(anchor.index, 2);
		let (replacement, scroll) = restore_sidebar_anchor(&entries, &anchor, &|_| false);
		assert_eq!(replacement, None);
		assert_eq!(scroll, 2);
		let gone = SidebarAnchor { id: "missing".into(), index: 1, offset: 0 };
		let (replacement, _) =
			restore_sidebar_anchor(&entries, &gone, &|entry| entry.entry.is_separator());
		assert_eq!(replacement.as_deref(), Some("acme"));
		let nearest = nearest_navigable_index(&entries, 1, &|entry| entry.entry.is_separator());
		assert_eq!(nearest, Some(2));
		assert_eq!(nearest_navigable_index(&entries, 1, &|_| true), None);
	}

	#[test]
	fn cursor_following_and_clamping() {
		assert_eq!(clamp_role_index(9, 3), 2);
		assert_eq!(clamp_role_index(1, 0), 0);
		assert_eq!(follow_cursor(0, 5, 3, 10), 3);
		assert_eq!(follow_cursor(3, 1, 3, 10), 1);
		assert_eq!(follow_cursor(2, 2, 3, 10), 2);
		assert_eq!(follow_cursor(8, 9, 3, 10), 8);
	}

	#[test]
	fn refresh_schedule_follows_force_and_once_guard() {
		reset_provider_auto_refresh_guard();
		let empty = HashSet::new();
		assert_eq!(
			refresh_schedule(false, "acme", false, &empty, &empty),
			RefreshSchedule::Debounce
		);
		mark_provider_auto_refreshed("acme");
		assert_eq!(
			refresh_schedule(false, "acme", false, &empty, &empty),
			RefreshSchedule::Skip
		);
		assert_eq!(
			refresh_schedule(false, "acme", true, &empty, &empty),
			RefreshSchedule::Debounce
		);
		let mut scheduled = HashSet::new();
		scheduled.insert("acme".to_string());
		assert_eq!(
			refresh_schedule(false, "acme", true, &scheduled, &empty),
			RefreshSchedule::StartForced
		);
		let mut refreshing = HashSet::new();
		refreshing.insert("acme".to_string());
		assert_eq!(
			refresh_schedule(false, "acme", true, &empty, &refreshing),
			RefreshSchedule::QueueCredentialRefresh
		);
		assert_eq!(refresh_schedule(true, "acme", false, &empty, &empty), RefreshSchedule::Skip);
		assert_eq!(refresh_schedule(false, "", false, &empty, &empty), RefreshSchedule::Skip);
		assert!(provider_auto_refreshed("acme"));
		reset_provider_auto_refresh_guard();
		assert!(!provider_auto_refreshed("acme"));
	}

	#[test]
	fn strip_state_accessors_cover_both_variants() {
		let mut role_strip = StripState::Role {
			kind: StripKind::Role,
			item: browser_item("acme", "good"),
			chips: vec![
				StripChip {
					label: "plan".into(),
					styled: "plan".into(),
					action: StripAction::Assign,
					role: Some("plan".into()),
					thinking_level: None,
					scope: None,
				},
				StripChip {
					label: "global plan".into(),
					styled: "global plan".into(),
					action: StripAction::Assign,
					role: Some("plan".into()),
					thinking_level: None,
					scope: Some(ModelRoleSelectionScope::Global),
				},
			],
			index: 1,
			role: None,
			fallback_index: None,
			scope: None,
			return_to_roles: false,
			initial_thinking_level: None,
		};
		assert_eq!(
			role_strip.selected_chip().unwrap().scope,
			Some(ModelRoleSelectionScope::Global)
		);
		assert_eq!(role_strip.chip_index(), Some(1));
		assert_eq!(role_strip.input_value(), "");
		assert!(!role_strip.is_name());
		role_strip.chips_mut().unwrap().clear();
		assert!(role_strip.selected_chip().is_none());

		let mut name_strip = StripState::Name {
			purpose: NamePurpose::Compaction,
			input: Input::new(),
			model: Some(model("acme", "good")),
			error: None,
			confirm: None,
			preview: None,
		};
		match &mut name_strip {
			StripState::Name { input, .. } => input.set_value("80%"),
			StripState::Role { .. } => panic!("expected name strip"),
		}
		assert!(name_strip.is_name());
		assert_eq!(name_strip.input_value(), "80%");
		assert!(name_strip.selected_chip().is_none());
	}

	#[test]
	fn catalog_matches_ranks_by_model_search_text() {
		let items = vec![browser_item("acme", "fast-one"), browser_item("acme", "slow-two")];
		let matches = catalog_matches(&items, "fast");
		assert_eq!(matches, vec![0]);
		assert_eq!(catalog_matches(&items, ""), vec![0, 1]);
	}

	#[test]
	fn default_source_and_registry_are_inert() {
		let source = StaticModelBrowserSource;
		assert!(source.disabled_providers().is_empty());
		assert!(source.fallback_chains().is_empty());
		assert_eq!(source.model_role_storage(), ModelRoleStorage::Global);
		assert_eq!(source.get_model_role_source("plan"), ModelRoleSource::Default);
		assert_eq!(source.get_model_presets(), None);
	}

	struct TestRegistry;

	impl ModelHubRegistry for TestRegistry {}

	struct TestCallbacks {
		cancelled: Arc<std::sync::atomic::AtomicBool>,
	}

	impl ModelHubCallbacks for TestCallbacks {
		fn on_cancel(&self) {
			self.cancelled
				.store(true, std::sync::atomic::Ordering::SeqCst);
		}
	}

	fn test_hub(callbacks: Box<dyn ModelHubCallbacks>) -> ModelHub {
		ModelHub::new(
			Arc::new(StaticModelBrowserSource),
			Box::new(TestRegistry),
			Vec::new(),
			callbacks,
			ModelHubOptions::default(),
		)
	}

	#[test]
	fn render_shows_title_and_roles() {
		let mut hub = test_hub(Box::new(TestCallbacks {
			cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
		}));
		let rows = Component::render(&mut hub, 80);
		let text = rows.join("\n");
		assert!(text.contains("Models"), "title missing: {text}");
		assert!(text.contains("Roles"), "roles missing: {text}");
	}

	#[test]
	fn model_kind_tab_focus_cycles() {
		let mut hub = test_hub(Box::new(TestCallbacks {
			cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
		}));
		hub.move_model_kind(1);
		assert_eq!(hub.model_kind_tab, ModelKindTab::Chat);
	}

	#[test]
	fn cancel_reports_to_host() {
		let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
		let mut hub = test_hub(Box::new(TestCallbacks {
			cancelled: cancelled.clone(),
		}));
		hub.cancel();
		assert!(cancelled.load(std::sync::atomic::Ordering::SeqCst));
	}

	#[test]
	fn strip_activation_consumes_chip() {
		let mut hub = test_hub(Box::new(TestCallbacks {
			cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
		}));
		let chip = StripChip {
			label: "Unassign".into(),
			styled: "Unassign".into(),
			action: StripAction::Unassign,
			role: Some("plan".into()),
			thinking_level: None,
			scope: None,
		};
		hub.strip = Some(StripState::Role {
			kind: StripKind::Role,
			item: browser_item("acme", "good"),
			chips: vec![chip],
			index: 0,
			role: Some("plan".into()),
			fallback_index: None,
			scope: None,
			return_to_roles: false,
			initial_thinking_level: None,
		});
		hub.activate_strip_chip();
		assert!(hub.strip.is_none());
	}

	#[test]
	fn native_describe_exposes_scopes() {
		let hub = test_hub(Box::new(TestCallbacks {
			cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
		}));
		let node = hub.describe_native(&DescribeContext::default());
		let debug = format!("{node:?}");
		assert!(debug.contains("scopes"), "describe missing scopes: {debug}");
	}

	#[test]
	fn mouse_route_is_safe_without_entries() {
		let mut hub = test_hub(Box::new(TestCallbacks {
			cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
		}));
		hub.route_mouse(&MouseEvent {
			kind: MouseKind::Down(MouseButtonKind::Left),
			column: 0,
			row: 0,
			shift: false,
			ctrl: false,
			alt: false,
		});
	}
}
