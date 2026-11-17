//! Reference: `l/src/overlays/model-browser.ts`.
//!
//! Frameless, reusable model browser: a fuzzy search row, a windowed model
//! list with role chips and metadata columns, and a selection detail block.
//!
//! Deviations from the reference:
//! - The crate has no `pi-catalog` port. Model facts arrive through
//!   [`ModelSummary`] on a [`CatalogModel`] supplied by the host, and role
//!   resolution goes through [`ModelBrowserSource`] instead of a settings
//!   object graph.
//! - Next dated credit-rate changes arrive pre-formatted
//!   ([`CreditRate::next_note`]); the reference formats a timestamp itself.
//! - The process-global theme, keybindings and terminal size are injected;
//!   input arrives as decoded [`Key`] events.
//! - The reference's native memo caches, picker catalogue patch stream
//!   (`itemsAdd`/`itemsDel`) and the credit-clock rebuild are dropped: the
//!   terminal redraws whole trees, so one-shot rows and a plain `picker`
//!   order are enough. `ScrollView`'s scrollbar is also dropped; the window
//!   itself is unchanged.
//! - Query edits keep the previous selection only when its selector survives
//!   the new ranking (the reference also has a "stable prefix" rule).
//! - Rows carry no `labelColor` styling token; the field is retained for
//!   hosts that style labels themselves.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::component::Component;
use crate::components::input::Input;
use crate::components::scroll_viewport::{
	clamp_scroll_offset, scroll_offset_for_row, ViewportAlignment,
};
use crate::fuzzy;
use crate::keys::{is_arrow_down, is_arrow_up, is_end, is_enter, is_escape, is_home, is_page_down, is_page_up, Key};
use crate::mouse::{MouseButtonKind, MouseKind, MouseEvent};
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{col, kv, md, node, row, span, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::strip_ansi;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use crate::theme::{default_theme, Theme};
use crate::thinking::{ConfiguredThinkingLevel, ThinkingLevel, AUTO_THINKING};

/// Canonical display ordering of built-in model roles.
pub const MODEL_ROLE_IDS: [&str; 16] = [
	"default", "smol", "slow", "vision", "plan", "commit", "tiny", "memory", "task", "advisor", "widget",
	"image", "web", "speech", "dictation", "judge",
];

/// Roles offered by the chat session picker.
pub const CHAT_MODEL_ROLE_IDS: [&str; 11] = [
	"default", "smol", "slow", "vision", "plan", "commit", "tiny", "memory", "task", "advisor", "widget",
];

/// Non-chat runner roles.
pub const KIND_ROLE_IDS: [&str; 5] = ["image", "web", "speech", "dictation", "judge"];

/// Measured model performance shown in browser rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelBrowserPerf {
	pub samples: u64,
	pub tps: f64,
	pub ttft_ms: Option<f64>,
}

/// Which half of a selector's roles a role belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RoleSection {
	#[default]
	Chat,
	Kind,
}

/// Role metadata shown in model selectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleInfo {
	pub tag: Option<String>,
	pub name: String,
	/// Theme colour token of the chip; `None` means `muted`.
	pub color: Option<String>,
	pub hidden: bool,
	pub section: RoleSection,
}

impl RoleInfo {
	/// Metadata for an undescribed role: its id as the name.
	pub fn named(role: &str) -> Self {
		RoleInfo {
			tag: None,
			name: role.to_string(),
			color: None,
			hidden: false,
			section: RoleSection::Chat,
		}
	}

	/// The chip label: tag, then name, then the role id, lowercased.
	fn label(&self, role: &str) -> String {
		self.tag
			.clone()
			.unwrap_or_else(|| {
				if self.name.is_empty() {
					role.to_string()
				} else {
					self.name.clone()
				}
			})
			.to_lowercase()
	}
}

/// A resolved role value, as hosts report it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedRoleValue {
	pub selector: Option<String>,
	pub thinking_level: Option<ConfiguredThinkingLevel>,
	pub explicit_thinking_level: bool,
	pub warning: Option<String>,
}

/// Where auto-compaction triggers for one model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelCompactionPoint {
	/// Context tokens that trigger auto-compaction; `None` when it is off or
	/// the window is unknown.
	pub tokens: Option<u64>,
	/// Why it triggers there: `fixed`, `85% of 400K base`, `80% of window`.
	pub basis: Option<String>,
	/// What sets it: the matching threshold key, `global`, or `default`.
	pub source: String,
	/// The model's own editable entry (`90k`, `80%`), when it has one.
	pub draft: Option<String>,
}

/// The non-rate pricing state a catalog rule declared for a zero-rate row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PricingStatus {
	Free,
	Included,
	Variable,
	Unknown,
}

impl PricingStatus {
	/// Column-width label.
	pub fn short(self) -> &'static str {
		match self {
			PricingStatus::Free => "free",
			PricingStatus::Included => "included",
			PricingStatus::Variable => "varies",
			PricingStatus::Unknown => "unknown",
		}
	}

	/// Detail-pane wording.
	pub fn detail(self) -> &'static str {
		match self {
			PricingStatus::Free => "free",
			PricingStatus::Included => "included",
			PricingStatus::Variable => "price varies",
			PricingStatus::Unknown => "pricing unknown",
		}
	}
}

/// Per-million token pricing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelCost {
	pub input: f64,
	pub output: f64,
	pub cache_read: f64,
	pub cache_write: f64,
}

impl ModelCost {
	fn is_zero(self) -> bool {
		self.input == 0.0 && self.output == 0.0
	}
}

/// A dated credit-rate change.
#[derive(Clone, Debug, PartialEq)]
pub struct CreditRateChange {
	pub rate: f64,
	/// Pre-formatted note for the date it takes effect (`2× from 2026-11-22`).
	pub note: String,
}

/// Standard-credit rate in force now, plus the next dated card.
#[derive(Clone, Debug, PartialEq)]
pub struct CreditRate {
	pub rate: f64,
	pub next: Option<CreditRateChange>,
}

/// Catalog facts of one model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelSummary {
	pub name: String,
	pub description: Option<String>,
	pub context_window: Option<u64>,
	pub max_tokens: Option<u64>,
	pub input: Vec<String>,
	pub reasoning: bool,
	pub int: Option<f64>,
	pub tps: Option<f64>,
	pub is_new: bool,
	pub is_beta: bool,
	pub is_recommended: bool,
	pub cost: Option<ModelCost>,
	/// Declared non-rate pricing state (zero-rate rows only).
	pub pricing_status: Option<PricingStatus>,
	pub priority: Option<i64>,
	pub credits: Option<CreditRate>,
	/// Whether the catalog classifies the model as a chat model.
	pub is_chat: bool,
}

/// A catalog entry as model overlays see it.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogModel {
	pub provider: String,
	pub id: String,
	pub summary: ModelSummary,
}

impl CatalogModel {
	/// The canonical `provider/id` key.
	pub fn selector(&self) -> String {
		format!("{}/{}", self.provider, self.id)
	}

	/// The display name, falling back to the id.
	pub fn display_name(&self) -> &str {
		if self.summary.name.is_empty() {
			&self.id
		} else {
			&self.summary.name
		}
	}
}

/// One selectable browser row.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelBrowserItem {
	pub provider: String,
	pub id: String,
	pub model: CatalogModel,
	pub selector: String,
	/// Optional foreground colour token for the row label.
	pub label_color: Option<String>,
}

impl ModelBrowserItem {
	/// A separator row between recent/role models and the rest.
	fn separator() -> Self {
		let summary = ModelSummary {
			name: "separator".into(),
			..ModelSummary::default()
		};
		ModelBrowserItem {
			provider: String::new(),
			id: "separator".into(),
			model: CatalogModel {
				provider: String::new(),
				id: "separator".into(),
				summary,
			},
			selector: "separator".into(),
			label_color: None,
		}
	}

	/// True for the synthetic separator row.
	pub fn is_separator(&self) -> bool {
		self.id == "separator"
	}
}

/// Resolved role assignment as displayed by the browser and the hub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleAssignment {
	pub selector: String,
	pub thinking_level: ConfiguredThinkingLevel,
	/// True when the role has no configured value and fell back to auto-selection.
	pub auto_selected: bool,
}

/// Map of role id to its resolved assignment (absent roles are unresolved).
pub type RoleAssignments = BTreeMap<String, RoleAssignment>;

/// Host-provided preferences and role resolution for the browser.
pub trait ModelBrowserSource {
	/// Changes whenever a preference the source resolves against changes.
	fn revision(&self) -> u64 {
		0
	}

	fn default_thinking_level(&self) -> Option<ConfiguredThinkingLevel> {
		None
	}

	fn model_provider_order(&self) -> Vec<String> {
		Vec::new()
	}

	fn known_role_ids(&self) -> Vec<String> {
		MODEL_ROLE_IDS.iter().map(|role| (*role).to_string()).collect()
	}

	fn mru_order(&self) -> Vec<String> {
		Vec::new()
	}

	/// Measured TPS/TTFT keyed by `selector` or `selector@tier`.
	fn model_perf(&self) -> HashMap<String, ModelBrowserPerf> {
		HashMap::new()
	}

	/// The configured value of a role, before resolution.
	fn get_model_role(&self, _role: &str) -> Option<String> {
		None
	}

	fn get_role_info(&self, role: &str) -> RoleInfo {
		RoleInfo::named(role)
	}

	/// Whether `model` can fill `role`.
	fn role_accepts(&self, _role: &str, _model: &CatalogModel) -> bool {
		true
	}

	/// Resolve a configured role value against its eligible models.
	fn resolve_role_value(&self, value: &str, _models: &[CatalogModel]) -> ResolvedRoleValue {
		ResolvedRoleValue {
			selector: Some(value.to_string()),
			thinking_level: None,
			explicit_thinking_level: false,
			warning: None,
		}
	}

	/// Service tier the host would send for `model`, when it runs one.
	fn service_tier_for(&self, _model: &CatalogModel) -> Option<String> {
		None
	}

	/// Auto-compaction point for `model`.
	fn compaction_point_for(&self, _model: &CatalogModel) -> Option<ModelCompactionPoint> {
		None
	}
}

/// A source with no preferences, useful for previews and tests.
#[derive(Default)]
pub struct StaticModelBrowserSource;

impl ModelBrowserSource for StaticModelBrowserSource {}

/// True when two catalog models describe the same `provider/id`.
pub fn models_are_equal(a: &CatalogModel, b: &CatalogModel) -> bool {
	a.provider == b.provider && a.id == b.id
}

/// Resolve every known role to its display assignment: configured role values
/// resolve against `all_models`; unconfigured roles fall back to auto-selection
/// over `auto_candidates` (skipped when empty).
pub fn resolve_role_assignments(
	source: &dyn ModelBrowserSource,
	all_models: &[CatalogModel],
	auto_candidates: &[CatalogModel],
) -> RoleAssignments {
	fn thinking_for(
		source: &dyn ModelBrowserSource,
		role: &str,
		resolved: &ResolvedRoleValue,
	) -> ConfiguredThinkingLevel {
		if resolved.explicit_thinking_level {
			if let Some(level) = resolved.thinking_level {
				return level;
			}
		}
		if role == "default" {
			return source.default_thinking_level().unwrap_or(ThinkingLevel::Inherit.into());
		}
		ThinkingLevel::Inherit.into()
	}

	let eligible = |pool: &[CatalogModel], role: &str| -> Vec<CatalogModel> {
		pool.iter()
			.filter(|model| source.role_accepts(role, model))
			.cloned()
			.collect()
	};

	let mut roles = RoleAssignments::new();
	let known_roles = source.known_role_ids();
	let mut configured_roles = HashSet::new();

	for role in &known_roles {
		let Some(value) = source.get_model_role(role) else {
			continue;
		};
		configured_roles.insert(role.clone());
		let pool = eligible(all_models, role);
		let resolved = source.resolve_role_value(&value, &pool);
		if resolved.selector.is_some() {
			let thinking_level = thinking_for(source, role, &resolved);
			let selector = resolved.selector.unwrap();
			roles.insert(
				role.clone(),
				RoleAssignment {
					selector,
					thinking_level,
					auto_selected: false,
				},
			);
		}
	}

	if !auto_candidates.is_empty() {
		for role in &known_roles {
			if configured_roles.contains(role) {
				continue;
			}
			let pool = eligible(auto_candidates, role);
			let resolved = source.resolve_role_value(&format!("pi/{role}"), &pool);
		if resolved.selector.is_none() {
			continue;
		}
		let thinking_level = thinking_for(source, role, &resolved);
		let selector = resolved.selector.unwrap();
		roles.insert(
			role.clone(),
			RoleAssignment {
				selector,
				thinking_level,
				auto_selected: true,
			},
		);
		}
	}

	roles
}

/// Role-resolution pool for a `--models`/`enabledModels` scope: the scoped
/// models plus every available non-chat runner.
pub fn scoped_role_pool(scoped: &[CatalogModel], available: &[CatalogModel]) -> Vec<CatalogModel> {
	let mut pool = scoped.to_vec();
	for model in available {
		if model.summary.is_chat || scoped.iter().any(|entry| models_are_equal(entry, model)) {
			continue;
		}
		pool.push(model.clone());
	}
	pool
}

/// Wrap raw models into browser items.
pub fn build_browser_items(models: &[CatalogModel]) -> Vec<ModelBrowserItem> {
	models
		.iter()
		.map(|model| ModelBrowserItem {
			provider: model.provider.clone(),
			id: model.id.clone(),
			model: model.clone(),
			selector: model.selector(),
			label_color: None,
		})
		.collect()
}

static DOT_VERSION_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?:^|[-_])(\d+\.\d+)").expect("valid regex"));
static DASH_VERSION_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?:^|[-_])(\d{1,2})-(\d{1,2})(?=-|$)").expect("valid regex"));
static SINGLE_VERSION_RE: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?:^|[-_])(\d+)").expect("valid regex"));
static DATED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-(\d{8})$").expect("valid regex"));
static LATEST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-latest$").expect("valid regex"));

/// Extract the first version number from a model ID (`gemini-2.5-pro` → 2.5).
fn extract_version_number(id: &str) -> f64 {
	if let Some(matched) = DOT_VERSION_RE.captures(id) {
		if let Some(value) = matched.get(1).and_then(|m| m.as_str().parse::<f64>().ok()) {
			return value;
		}
	}
	if let Some(matched) = DASH_VERSION_RE.captures(id) {
		let major = matched.get(1).map(|m| m.as_str());
		let minor = matched.get(2).map(|m| m.as_str());
		if let (Some(major), Some(minor)) = (major, minor) {
			if let Ok(value) = format!("{major}.{minor}").parse::<f64>() {
				return value;
			}
		}
	}
	if let Some(matched) = SINGLE_VERSION_RE.captures(id) {
		if let Some(value) = matched.get(1).and_then(|m| m.as_str().parse::<f64>().ok()) {
			return value;
		}
	}
	0.0
}

/// Rank a model by the first built-in role it is assigned to (lower = earlier).
fn compute_model_rank(model: &CatalogModel, roles: &RoleAssignments) -> usize {
	let selector = model.selector();
	for (index, role) in MODEL_ROLE_IDS.iter().enumerate() {
		if let Some(assignment) = roles.get(*role) {
			if assignment.selector == selector {
				return index;
			}
		}
	}
	MODEL_ROLE_IDS.len()
}

/// Options for [`sort_model_items`].
#[derive(Clone, Debug, Default)]
pub struct SortModelItemsOptions {
	pub roles: Option<RoleAssignments>,
	pub mru_order: Option<Vec<String>>,
	/// When a search query is narrowing the list, role assignments should not
	/// promote a weakly-matching default model above a perfect text match.
	pub skip_role_rank: bool,
}

fn mru_index(mru_order: &[String]) -> HashMap<String, usize> {
	mru_order
		.iter()
		.enumerate()
		.map(|(index, selector)| (selector.clone(), index))
		.collect()
}

fn model_item_compare(
	a: &ModelBrowserItem,
	b: &ModelBrowserItem,
	roles: &RoleAssignments,
	mru: &HashMap<String, usize>,
	skip_role_rank: bool,
) -> Ordering {
	if !skip_role_rank {
		let a_rank = compute_model_rank(&a.model, roles);
		let b_rank = compute_model_rank(&b.model, roles);
		if a_rank != b_rank {
			return a_rank.cmp(&b_rank);
		}
	}

	let missing = usize::MAX;
	let a_mru = mru.get(&a.selector).copied().unwrap_or(missing);
	let b_mru = mru.get(&b.selector).copied().unwrap_or(missing);
	if a_mru != b_mru {
		return a_mru.cmp(&b_mru);
	}

	let provider = a.provider.cmp(&b.provider);
	if provider != Ordering::Equal {
		return provider;
	}

	let a_priority = a.model.summary.priority.unwrap_or(i64::MAX);
	let b_priority = b.model.summary.priority.unwrap_or(i64::MAX);
	if a_priority != b_priority {
		return a_priority.cmp(&b_priority);
	}

	let a_version = extract_version_number(&a.id);
	let b_version = extract_version_number(&b.id);
	if a_version != b_version {
		return b_version
			.partial_cmp(&a_version)
			.unwrap_or(Ordering::Equal);
	}

	let a_latest = LATEST_RE.is_match(&a.id);
	let b_latest = LATEST_RE.is_match(&b.id);
	let a_date = DATED_RE
		.captures(&a.id)
		.and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
		.unwrap_or_default();
	let b_date = DATED_RE
		.captures(&b.id)
		.and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
		.unwrap_or_default();

	let a_has_recency = a_latest || !a_date.is_empty();
	let b_has_recency = b_latest || !b_date.is_empty();
	if a_has_recency != b_has_recency {
		return if a_has_recency {
			Ordering::Less
		} else {
			Ordering::Greater
		};
	}
	if !a_has_recency {
		return a.id.cmp(&b.id);
	}
	if a_latest != b_latest {
		return if a_latest {
			Ordering::Less
		} else {
			Ordering::Greater
		};
	}
	if !a_date.is_empty() && !b_date.is_empty() {
		return b_date.cmp(&a_date);
	}
	a.id.cmp(&b.id)
}

/// Order models for display: role-assigned first, then most-recently-used,
/// then per provider by priority, version, and recency.
pub fn sort_model_items(items: &mut [ModelBrowserItem], options: &SortModelItemsOptions) {
	let roles = options.roles.clone().unwrap_or_default();
	let mru = mru_index(options.mru_order.as_deref().unwrap_or(&[]));
	items.sort_by(|a, b| model_item_compare(a, b, &roles, &mru, options.skip_role_rank));
}

/// Picker candidates and ordering inputs shared with composer model mentions.
#[derive(Clone, Debug, Default)]
pub struct SessionModelScope {
	pub items: Vec<ModelBrowserItem>,
	pub roles: RoleAssignments,
	pub mru_order: Vec<String>,
	pub error: Option<String>,
}

/// Build the session picker's current scope without creating a browser.
pub fn build_session_model_scope(
	source: &dyn ModelBrowserSource,
	scoped: &[CatalogModel],
	available: &[CatalogModel],
	catalog: &[CatalogModel],
	error: Option<String>,
) -> SessionModelScope {
	if !scoped.is_empty() {
		let role_models = scoped_role_pool(scoped, available);
		let mut error = error;
		if role_models.is_empty() {
			error = Some("no role models in scope".into());
		}
		return scope_from_inputs(source, scoped, &role_models, &role_models, error);
	}
	let mut error = error;
	let role_models = available.to_vec();
	if role_models.is_empty() {
		error = error.or_else(|| Some("no available models".into()));
	}
	let models: Vec<CatalogModel> = role_models
		.iter()
		.filter(|model| model.summary.is_chat)
		.cloned()
		.collect();
	scope_from_inputs(source, &models, catalog, &role_models, error)
}

fn scope_from_inputs(
	source: &dyn ModelBrowserSource,
	models: &[CatalogModel],
	all_models: &[CatalogModel],
	role_models: &[CatalogModel],
	error: Option<String>,
) -> SessionModelScope {
	let roles = resolve_role_assignments(source, all_models, role_models);
	let mru_order = source.mru_order();
	let mut items = build_browser_items(models);
	sort_model_items(
		&mut items,
		&SortModelItemsOptions {
			roles: Some(roles.clone()),
			mru_order: Some(mru_order.clone()),
			skip_role_rank: false,
		},
	);
	SessionModelScope {
		items,
		roles,
		mru_order,
		error,
	}
}

/// User affinity used to order search matches within one relevance tier.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchAffinity {
	/// `provider/id` (lowercased) → rank.
	pub models: HashMap<String, usize>,
	/// provider (lowercased) → rank.
	pub providers: HashMap<String, usize>,
}

#[derive(Default)]
struct RoleProviderStats {
	count: usize,
	first_role: usize,
}

/// Build model and provider affinity from explicit configuration, configured
/// role assignments, and recent model use. Auto-selected roles are catalog
/// policy, not evidence of user preference.
pub fn build_search_affinity(
	provider_order: &[String],
	roles: &RoleAssignments,
	mru_order: &[String],
) -> SearchAffinity {
	let mut models = Vec::new();
	let mut seen_models = HashSet::new();
	let mut add_model = |selector: &str| {
		let key = selector.to_lowercase();
		if seen_models.insert(key.clone()) {
			models.push(key);
		}
	};

	let mut providers: Vec<String> = Vec::new();
	let mut seen_providers = HashSet::new();
	let mut add_provider = |provider: &str| {
		let key = provider.trim().to_lowercase();
		if key.is_empty() || !seen_providers.insert(key.clone()) {
			return;
		}
		providers.push(key);
	};

	for provider in provider_order {
		add_provider(provider);
	}

	let mut role_stats: HashMap<String, RoleProviderStats> = HashMap::new();
	let mut seen_roles = HashSet::new();
	let mut role_index = 0usize;
	let mut record_role = |role: &str| {
		if !seen_roles.insert(role.to_string()) {
			return;
		}
		if let Some(assignment) = roles.get(role) {
			if !assignment.auto_selected {
				add_model(&assignment.selector);
				if let Some((provider, _)) = assignment.selector.split_once('/') {
					let provider = provider.to_lowercase();
					let entry = role_stats.entry(provider).or_insert(RoleProviderStats {
						count: 0,
						first_role: role_index,
					});
					entry.count += 1;
				}
			}
		}
		role_index += 1;
	};
	for role in MODEL_ROLE_IDS {
		record_role(role);
	}
	for role in roles.keys() {
		record_role(role);
	}
	for selector in mru_order {
		add_model(selector);
	}

	let mut preferred_by_role: Vec<(String, RoleProviderStats)> = role_stats.into_iter().collect();
	preferred_by_role.sort_by(|(_, a), (_, b)| {
		b.count
			.cmp(&a.count)
			.then_with(|| a.first_role.cmp(&b.first_role))
	});
	for (provider, _) in &preferred_by_role {
		add_provider(provider);
	}

	for selector in mru_order {
		if let Some((provider, _)) = selector.split_once('/') {
			if !provider.is_empty() {
				add_provider(provider);
			}
		}
	}

	SearchAffinity {
		models: models
			.into_iter()
			.enumerate()
			.map(|(index, selector)| (selector, index))
			.collect(),
		providers: providers
			.into_iter()
			.enumerate()
			.map(|(index, provider)| (provider, index))
			.collect(),
	}
}

/// Collapse punctuation so exact and contiguous model-name matches form
/// stable relevance tiers.
fn compact_model_search_text(value: &str) -> String {
	value
		.chars()
		.filter(|c| c.is_alphanumeric())
		.flat_map(char::to_lowercase)
		.collect()
}

/// Exact id/selector → contiguous literal → fuzzy-only.
fn model_search_tier(query: &str, id: &str, selector: &str) -> u8 {
	if query.is_empty() {
		return 2;
	}
	if query == id || query == selector {
		return 0;
	}
	if id.contains(query) || selector.contains(query) {
		return 1;
	}
	2
}

/// Inputs to [`rank_model_items`] / [`ModelItemRanker`].
#[derive(Clone, Debug, Default)]
pub struct ModelRankOptions {
	pub roles: RoleAssignments,
	pub mru_order: Vec<String>,
	pub affinity: SearchAffinity,
}

#[derive(Clone, Debug)]
struct ModelRankKeys {
	compact_id: String,
	compact_selector: String,
	model_affinity: usize,
	provider_affinity: usize,
	order_group: usize,
}

/// [`rank_model_items`] prepared for one candidate list: the fuzzy haystacks,
/// MRU/version order, and affinity ranks depend only on the items and options,
/// so per-keystroke callers hold one ranker and pay only the fuzzy scan.
pub struct ModelItemRanker {
	items: Vec<ModelBrowserItem>,
	options: ModelRankOptions,
	haystacks: Vec<String>,
	keys: Vec<ModelRankKeys>,
}

impl ModelItemRanker {
	/// Prepare the ranker over `items`.
	pub fn new(items: Vec<ModelBrowserItem>, options: ModelRankOptions) -> Self {
		let mut order: Vec<usize> = (0..items.len()).collect();
		let mru = mru_index(&options.mru_order);
		order.sort_by(|&a, &b| {
			model_item_compare(
				&items[a],
				&items[b],
				&options.roles,
				&mru,
				true,
			)
		});
		let mut groups = vec![0usize; items.len()];
		let mut group = 0usize;
		for position in 0..order.len() {
			if position > 0
				&& model_item_compare(
					&items[order[position - 1]],
					&items[order[position]],
					&options.roles,
					&mru,
					true,
				) != Ordering::Equal
			{
				group += 1;
			}
			groups[order[position]] = group;
		}

		let missing = usize::MAX;
		let haystacks: Vec<String> = items.iter().map(model_search_text).collect();
		let keys = items
			.iter()
			.enumerate()
			.map(|(index, item)| ModelRankKeys {
				compact_id: compact_model_search_text(&item.id),
				compact_selector: compact_model_search_text(&item.selector),
				model_affinity: options
					.affinity
					.models
					.get(&item.selector.to_lowercase())
					.copied()
					.unwrap_or(missing),
				provider_affinity: options
					.affinity
					.providers
					.get(&item.provider.to_lowercase())
					.copied()
					.unwrap_or(missing),
				order_group: groups[index],
			})
			.collect();

		ModelItemRanker {
			items,
			options,
			haystacks,
			keys,
		}
	}

	/// The candidates the ranker was built over.
	pub fn items(&self) -> &[ModelBrowserItem] {
		&self.items
	}

	/// Rank by text relevance, user affinity, and MRU/version order.
	pub fn rank(&self, query: &str) -> Vec<ModelBrowserItem> {
		if query.trim().is_empty() {
			return self.items.clone();
		}
		let query_key = compact_model_search_text(query);
		// Exact and contiguous matches stay ahead of fuzzy-only candidates;
		// affinity breaks ties before fuzzy quality and the normal MRU/version
		// ordering, and fuzzy order settles items that ordering cannot tell
		// apart.
		let mut rows: Vec<(ModelBrowserItem, usize, usize, usize, i64, usize, usize)> =
			fuzzy::fuzzy_rank(&self.haystacks, query, |text| text)
				.into_iter()
				.enumerate()
				.map(|(fuzzy_index, ranked)| {
					let keys = &self.keys[ranked.index];
					(
						self.items[ranked.index].clone(),
						model_search_tier(&query_key, &keys.compact_id, &keys.compact_selector) as usize,
						keys.model_affinity,
						keys.provider_affinity,
						(ranked.score / 10.0).round() as i64,
						keys.order_group,
						fuzzy_index,
					)
				})
				.collect();
		rows.sort_by(|a, b| {
			a.1.cmp(&b.1)
				.then_with(|| a.2.cmp(&b.2))
				.then_with(|| a.3.cmp(&b.3))
				.then_with(|| a.4.cmp(&b.4))
				.then_with(|| a.5.cmp(&b.5))
				.then_with(|| a.6.cmp(&b.6))
		});
		rows.into_iter().map(|row| row.0).collect()
	}
}

/// Rank picker and mention candidates by text relevance, user affinity, and
/// MRU/version order.
pub fn rank_model_items(
	query: &str,
	items: Vec<ModelBrowserItem>,
	options: ModelRankOptions,
) -> Vec<ModelBrowserItem> {
	ModelItemRanker::new(items, options).rank(query)
}

/// `● default ◉` — solid dot for configured assignments, hollow for
/// auto-selected fallbacks, thinking glyph attached when set.
pub fn format_role_chip(role: &str, assignment: &RoleAssignment, source: &dyn ModelBrowserSource, theme: &Theme) -> String {
	let info = source.get_role_info(role);
	let label = info.label(role);
	let glyph = thinking_level_glyph(assignment.thinking_level, theme);
	let suffix = if glyph.is_empty() {
		String::new()
	} else {
		format!(" {}", theme.fg("dim", &glyph).unwrap_or(glyph))
	};
	if assignment.auto_selected {
		let dot = theme.status().shadowed;
		return format!("{}{suffix}", theme.fg("dim", &format!("{dot} {label}")).unwrap_or_default());
	}
	let dot = theme.status().enabled;
	let color = info.color.unwrap_or_else(|| "muted".into());
	format!(
		"{}{suffix}",
		theme.fg(&color, &format!("{dot} {label}")).unwrap_or_default()
	)
}

/// Theme glyph of a configured thinking level; empty for inherit and auto.
fn thinking_level_glyph(level: ConfiguredThinkingLevel, theme: &Theme) -> String {
	match level {
		ConfiguredThinkingLevel::Inherit | ConfiguredThinkingLevel::Auto => String::new(),
		other => theme.symbol(&format!("thinking.{}", other.as_str())).to_string(),
	}
}

/// The omp theme token of a thinking level's dot; none for inherit and auto.
pub fn thinking_dot_token(level: ConfiguredThinkingLevel) -> Option<String> {
	if matches!(level, ConfiguredThinkingLevel::Inherit | ConfiguredThinkingLevel::Auto) {
		return None;
	}
	let mut chars = level.as_str().chars();
	let first = chars.next()?.to_uppercase().to_string();
	Some(format!("thinking{first}{}", chars.as_str()))
}

fn declared_pricing_status(model: &CatalogModel) -> Option<PricingStatus> {
	model.summary.pricing_status
}

/// No token price and no subscription-credit charge (unless a pricing state is
/// declared).
fn is_free_model(model: &CatalogModel) -> bool {
	if let Some(declared) = declared_pricing_status(model) {
		return declared == PricingStatus::Free;
	}
	let cost = model.summary.cost.unwrap_or_default();
	let credits = model.summary.credits.as_ref().map(|credits| credits.rate);
	(cost.input == 0.0 && cost.output == 0.0) && credits.map_or(true, |rate| rate == 0.0)
}

/// One per-million price leg: `3`, `0.25`, `12.5`; `?` when unknown.
fn format_cost_leg(n: f64) -> String {
	if !n.is_finite() || n < 0.0 {
		return "?".into();
	}
	if n > 0.0 && n < 0.01 {
		return format!("{n}");
	}
	let s = if n >= 100.0 {
		format!("{}", n.round() as i64)
	} else if n >= 10.0 {
		format!("{n:.1}")
	} else {
		format!("{n:.2}")
	};
	trim_trailing_zeros(&s)
}

fn trim_trailing_zeros(s: &str) -> String {
	if !s.contains('.') {
		return s.to_string();
	}
	let trimmed = s.trim_end_matches('0').trim_end_matches('.');
	trimmed.to_string()
}

/// `2× from 2026-11-22` when a dated credit card changes the rate in force.
fn next_credit_rate_note(model: &CatalogModel) -> Option<String> {
	model
		.summary
		.credits
		.as_ref()
		.and_then(|credits| credits.next.as_ref())
		.map(|next| next.note.clone())
}

/// Adds the credit rate in force now to a dollar price, replacing the price
/// when the model has no dollar reference.
fn with_credit_badge(model: &CatalogModel, price: &str) -> String {
	let Some(credits) = model.summary.credits.as_ref() else {
		return price.to_string();
	};
	let badge = format!("{}×", format_cost_leg(credits.rate));
	let cost = model.summary.cost.unwrap_or_default();
	if !cost.is_zero() {
		format!("{price} {badge}")
	} else {
		badge
	}
}

/// `$in/out` per-million cost pair with any credit badge.
pub fn format_cost_pair(model: &CatalogModel) -> String {
	if let Some(declared) = declared_pricing_status(model) {
		return declared.short().into();
	}
	if is_free_model(model) {
		return "free".into();
	}
	let cost = model.summary.cost.unwrap_or_default();
	with_credit_badge(
		model,
		&format!(
			"${}/{}",
			format_cost_leg(cost.input),
			format_cost_leg(cost.output)
		),
	)
}

/// Detail-pane price fact: `$3/15 per M`, or the declared pricing state.
pub fn format_cost_detail(model: &CatalogModel) -> String {
	if let Some(declared) = declared_pricing_status(model) {
		return declared.detail().into();
	}
	format!("{} per M", format_cost_pair(model))
}

/// `$3·15` price fact of a picker row.
pub fn picker_price(model: &CatalogModel) -> String {
	if let Some(declared) = declared_pricing_status(model) {
		return declared.short().into();
	}
	if is_free_model(model) {
		return "free".into();
	}
	let cost = model.summary.cost.unwrap_or_default();
	with_credit_badge(
		model,
		&format!(
			"${}·{}",
			format_cost_leg(cost.input),
			format_cost_leg(cost.output)
		),
	)
}

/// `$2 in · $10 out · $0.2 cache` for a model preview.
pub fn preview_price(model: &CatalogModel) -> String {
	if let Some(declared) = declared_pricing_status(model) {
		return declared.detail().into();
	}
	let cost = model.summary.cost.unwrap_or_default();
	let mut parts = vec![
		format!("${} in", format_cost_leg(cost.input)),
		format!("${} out", format_cost_leg(cost.output)),
	];
	if cost.cache_read > 0.0 {
		parts.push(format!("${} cache", format_cost_leg(cost.cache_read)));
	}
	let priced = with_credit_badge(model, &parts.join(" · "));
	match next_credit_rate_note(model) {
		Some(note) => format!("{priced} ({note})"),
		None => priced,
	}
}

/// A provider's picker mark: initials on the provider's own gradient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderMark {
	pub text: String,
	pub seed: String,
	pub icon: String,
}

/// `amazon-bedrock` → `AB`, `anthropic` → `An`.
pub fn provider_mark(provider_id: &str) -> ProviderMark {
	let words: Vec<&str> = provider_id
		.split(['-', '_', ' ', '.'])
		.filter(|word| !word.is_empty())
		.collect();
	let text = if words.len() >= 2 {
		let first = words[0].chars().next().unwrap_or_default();
		let second = words[1].chars().next().unwrap_or_default();
		format!("{first}{second}").to_uppercase()
	} else {
		let word = words.first().copied().unwrap_or(provider_id);
		let mut chars = word.chars();
		let first = chars.next().unwrap_or_default().to_uppercase();
		let second = chars.next().unwrap_or_default();
		format!("{first}{second}")
	};
	ProviderMark {
		text,
		seed: provider_id.to_string(),
		icon: format!("provider:{provider_id}"),
	}
}

/// The fuzzy haystack for a model row: the displayed `provider/id`, plus `free`
/// for zero-cost models so the cost column's own word is searchable.
pub fn model_search_text(item: &ModelBrowserItem) -> String {
	let base = format!("{}/{}", item.provider, item.id);
	if is_free_model(&item.model) {
		format!("{base} free")
	} else {
		base
	}
}

fn format_description(description: &str) -> String {
	description
		.replace(['\r', '\n'], " ")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

/// Compact count: `400k`, `1.2m`.
pub fn format_number(n: u64) -> String {
	if n >= 1_000_000 {
		let value = n as f64 / 1_000_000.0;
		if (value.fract() * 10.0).round() == 0.0 {
			format!("{}m", value.round() as u64)
		} else {
			format!("{value:.1}m")
		}
	} else if n >= 1_000 {
		let value = n as f64 / 1_000.0;
		if (value.fract() * 10.0).round() == 0.0 {
			format!("{}k", value.round() as u64)
		} else {
			format!("{value:.1}k")
		}
	} else {
		n.to_string()
	}
}

/// `400k ◫` context-window column; empty when unreported.
fn format_context(model: &CatalogModel, theme: &Theme) -> String {
	let ctx = model.summary.context_window.unwrap_or(0);
	if ctx == 0 {
		return String::new();
	}
	let icon = theme
		.symbol("icon.context")
		.trim_end_matches(':');
	format!("{} {icon}", format_number(ctx))
}

/// `118t/s` average output speed; one decimal below 10 t/s.
fn format_tps(tps: f64) -> String {
	let value = if tps >= 10.0 {
		format!("{}", tps.round() as i64)
	} else {
		format!("{tps:.1}")
	};
	format!("{value}t/s")
}

/// Brain-icon intelligence score delivered with the model catalog.
fn format_intelligence(model: &CatalogModel, theme: &Theme) -> String {
	let Some(int) = model.summary.int else {
		return String::new();
	};
	if !int.is_finite() {
		return String::new();
	}
	format!(
		"{} {}",
		theme.symbol("icon.intelligence"),
		int.round() as i64
	)
}

/// `0.9s` average time-to-first-token; whole seconds from 10s up.
fn format_ttft(ms: f64) -> String {
	let seconds = ms / 1000.0;
	if seconds >= 10.0 {
		format!("{}s", seconds.round() as i64)
	} else {
		format!("{seconds:.1}s")
	}
}

/// Pad `text` on the left to `width` terminal columns.
fn pad_left_visible(text: &str, width: usize) -> String {
	let missing = width.saturating_sub(visible_width(text));
	format!("{}{text}", " ".repeat(missing))
}

/// Total width of present metadata columns, joined by two-space gaps.
fn meta_columns_width(widths: &[usize]) -> usize {
	let present: Vec<usize> = widths.iter().copied().filter(|&w| w > 0).collect();
	if present.is_empty() {
		return 0;
	}
	present.iter().sum::<usize>() + 2 * (present.len() - 1)
}

/// One picker fact column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerColumnSpec {
	pub id: String,
	pub head: String,
	pub format: String,
	pub priority: u8,
}

/// Fact columns of a model picker; the lowest priority hides first.
pub fn model_picker_columns() -> Vec<PickerColumnSpec> {
	vec![
		PickerColumnSpec {
			id: "int".into(),
			head: "Int".into(),
			format: "num".into(),
			priority: 1,
		},
		PickerColumnSpec {
			id: "speed".into(),
			head: "t/s".into(),
			format: "num".into(),
			priority: 2,
		},
		PickerColumnSpec {
			id: "ctx".into(),
			head: "Ctx".into(),
			format: "num".into(),
			priority: 4,
		},
		PickerColumnSpec {
			id: "price".into(),
			head: "$/M".into(),
			format: "price".into(),
			priority: 3,
		},
	]
}

/// How [`ModelBrowser::picker_order`] heads its rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerGrouping {
	/// One group per provider after the Recent block.
	pub providers: bool,
	/// Label of the rows after the Recent block when not split by provider;
	/// `None` drops every head.
	pub rest: Option<String>,
}

impl Default for PickerGrouping {
	fn default() -> Self {
		PickerGrouping {
			providers: false,
			rest: Some("All models".into()),
		}
	}
}

/// One entry of a picker order: a row selector or a group head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerOrderEntry {
	Item(String),
	Group {
		group: String,
		label: String,
		count: usize,
	},
}

/// A browser's visible rows as picker order plus the query's hit ranges.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelPickerOrder {
	pub order: Vec<PickerOrderEntry>,
	pub hits: Option<HashMap<String, Vec<(usize, usize)>>>,
	/// Selectable rows in `order`.
	pub count: usize,
}

/// Row width from which the measured-perf column appears (TPS only).
const PERF_TPS_MIN_WIDTH: usize = 76;
/// Row width from which the perf column also includes TTFT.
const PERF_FULL_MIN_WIDTH: usize = 96;
/// Narrowest model-name cell retained before cost and context are dropped.
const MIN_NAME_WIDTH: usize = 16;
/// Rendered rows before the list window: search row + blank.
const LIST_ROW_START: usize = 2;
/// Rendered rows after the list window: blank + two detail rows.
const DETAIL_ROWS: usize = 3;

/// What the per-row perf column shows at the current width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PerfMode {
	Off,
	Tps,
	Full,
}

#[derive(Clone, Debug)]
struct MeasuredPerf {
	perf: ModelBrowserPerf,
	tier: Option<String>,
}

/// Behavior switches for [`ModelBrowser`].
#[derive(Default)]
pub struct ModelBrowserOptions {
	/// Render the dim `provider/` prefix before model ids. Default true.
	pub show_provider: Option<bool>,
	/// Session token count used to flag models whose context window is exceeded.
	pub current_context_tokens: u64,
	/// When true, over-context rows render grayed; picking one compacts first.
	pub mark_over_context: bool,
	/// Host-provided empty-state text.
	pub empty_text: Option<Box<dyn Fn() -> Option<String>>>,
}

/// The reusable browser component.
pub struct ModelBrowser {
	source: Box<dyn ModelBrowserSource>,
	theme: Theme,
	search: Input,
	items: Vec<ModelBrowserItem>,
	visible: Vec<ModelBrowserItem>,
	selected: isize,
	roles: RoleAssignments,
	mru_order: Vec<String>,
	affinity: SearchAffinity,
	ranker: Option<ModelItemRanker>,
	perf: HashMap<String, ModelBrowserPerf>,
	hovered: Option<usize>,
	max_visible: usize,
	show_provider: bool,
	current_context_tokens: u64,
	mark_over_context: bool,
	empty_text: Option<Box<dyn Fn() -> Option<String>>>,
	preserve_query_order: bool,
	window_start: usize,
	window_count: usize,
	focused: bool,
	current_selector: Option<String>,
	/// Enter or click-on-selected.
	pub on_activate: Option<Box<dyn FnMut(&ModelBrowserItem)>>,
	pub on_selection_change: Option<Box<dyn FnMut(Option<&ModelBrowserItem>)>>,
	pub on_query_change: Option<Box<dyn FnMut(&str)>>,
	/// Cancel key with an empty query.
	pub on_cancel: Option<Box<dyn FnMut()>>,
}

impl ModelBrowser {
	/// A browser over `source` with no items; call [`Self::set_items`] first.
	pub fn new(source: Box<dyn ModelBrowserSource>, options: ModelBrowserOptions) -> Self {
		let theme = default_theme();
		let mut browser = ModelBrowser {
			source,
			theme,
			search: Input::new(),
			items: Vec::new(),
			visible: Vec::new(),
			selected: -1,
			roles: RoleAssignments::new(),
			mru_order: Vec::new(),
			affinity: SearchAffinity::default(),
			ranker: None,
			perf: HashMap::new(),
			hovered: None,
			max_visible: 10,
			show_provider: options.show_provider.unwrap_or(true),
			current_context_tokens: options.current_context_tokens,
			mark_over_context: options.mark_over_context,
			empty_text: options.empty_text,
			preserve_query_order: false,
			window_start: 0,
			window_count: 0,
			focused: true,
			current_selector: None,
			on_activate: None,
			on_selection_change: None,
			on_query_change: None,
			on_cancel: None,
		};
		browser.sync_affinity();
		browser
	}

	/// Replace the component's theme.
	pub fn set_theme(&mut self, theme: Theme) {
		self.theme = theme;
	}

	/// Mark `selector` as the session's active model (`None` clears it).
	pub fn set_current_selector(&mut self, selector: Option<String>) {
		self.current_selector = selector;
	}

	/// Replace the scope's base items; the live query re-applies and selection
	/// is pinned by selector.
	pub fn set_items(&mut self, items: Vec<ModelBrowserItem>) {
		let selected_key = self.selected_item().map(|item| item.selector.clone());
		self.items = items;
		self.ranker = None;
		self.refilter(selected_key.as_deref());
		if let Some(key) = selected_key {
			self.select_selector(&key);
		}
	}

	pub fn set_roles(&mut self, roles: RoleAssignments) {
		self.roles = roles;
		self.sync_affinity();
		self.invalidate_ranker();
	}

	pub fn set_mru_order(&mut self, order: Vec<String>) {
		self.mru_order = order;
		self.sync_affinity();
		self.invalidate_ranker();
	}

	fn sync_affinity(&mut self) {
		self.affinity = build_search_affinity(
			&self.source.model_provider_order(),
			&self.roles,
			&self.mru_order,
		);
	}

	/// Measured TPS/TTFT averages keyed by `provider/id` (or `selector@tier`).
	pub fn set_perf_stats(&mut self, perf: HashMap<String, ModelBrowserPerf>) {
		self.perf = perf;
	}

	pub fn set_max_visible(&mut self, rows: usize) {
		self.max_visible = rows.max(1);
	}

	pub fn set_show_provider(&mut self, show: bool) {
		self.show_provider = show;
	}

	/// Keep the source order after fuzzy filtering instead of model ranking.
	pub fn set_preserve_query_order(&mut self, preserve: bool) {
		self.preserve_query_order = preserve;
	}

	pub fn set_mark_over_context(&mut self, mark: bool) {
		self.mark_over_context = mark;
	}

	/// Focused: accent cursor + selected-row band. Unfocused: dim cursor, no band.
	pub fn set_focused(&mut self, focused: bool) {
		self.focused = focused;
	}

	pub fn set_current_context_tokens(&mut self, tokens: u64) {
		self.current_context_tokens = tokens;
	}

	/// Total rendered height for the current `max_visible`.
	pub fn rendered_rows(&self) -> usize {
		LIST_ROW_START + self.max_visible + DETAIL_ROWS
	}

	pub fn query(&self) -> &str {
		self.search.value()
	}

	/// Caret into the query in characters.
	pub fn cursor(&self) -> usize {
		self.search.cursor_viewport_column(u16::MAX)
	}

	pub fn set_query(&mut self, query: &str) {
		self.search.set_value(query);
		self.refilter(None);
	}

	pub fn selected_item(&self) -> Option<&ModelBrowserItem> {
		if self.selected < 0 {
			return None;
		}
		self.visible.get(self.selected as usize)
	}

	pub fn selected_index(&self) -> isize {
		self.selected
	}

	pub fn visible_items(&self) -> &[ModelBrowserItem] {
		&self.visible
	}

	pub fn visible_count(&self) -> usize {
		self.visible.len()
	}

	/// Base items matching the live query, best first, without the separator;
	/// `None` while the query is blank.
	pub fn query_matches(&self) -> Option<Vec<&ModelBrowserItem>> {
		if self.query().trim().is_empty() {
			return None;
		}
		Some(self.visible.iter().filter(|item| !item.is_separator()).collect())
	}

	/// Move selection to `selector`; false when it is not in the current view.
	pub fn select_selector(&mut self, selector: &str) -> bool {
		let Some(index) = self.visible.iter().position(|item| item.selector == selector) else {
			return false;
		};
		if self.visible[index].is_separator() {
			return false;
		}
		self.selected = index as isize;
		self.ensure_selected_visible();
		true
	}

	fn is_disabled(item: &ModelBrowserItem) -> bool {
		item.is_separator()
	}

	/// True when `item`'s context window is smaller than the live session token
	/// count (grayed row; hosts compact before switching).
	pub fn is_over_context(&self, item: &ModelBrowserItem) -> bool {
		if item.is_separator() || !self.mark_over_context || self.current_context_tokens == 0 {
			return false;
		}
		let context_window = item.model.summary.context_window.unwrap_or(0);
		context_window > 0 && self.current_context_tokens > context_window
	}

	fn clamp_window_start(&self, start: usize) -> usize {
		clamp_scroll_offset(start, self.visible.len(), self.max_visible)
	}

	fn ensure_selected_visible(&mut self) {
		if self.selected < 0 {
			return;
		}
		self.window_start = scroll_offset_for_row(
			self.window_start,
			self.selected as usize,
			self.visible.len(),
			self.max_visible,
			ViewportAlignment::Nearest,
		);
	}

	/// Move the selection by `delta` rows, skipping disabled rows. Single steps
	/// wrap at the ends; `wrap: false` clamps instead.
	pub fn move_selection(&mut self, delta: isize, wrap: bool) {
		if self.visible.is_empty() || delta == 0 {
			return;
		}
		let total = self.visible.len() as isize;
		let direction = if delta < 0 { -1 } else { 1 };
		let mut remaining = delta.abs();
		let mut index = self.selected.max(0);
		let mut attempts = 0;
		while attempts < total && remaining > 0 {
			let mut next = index + direction;
			if next < 0 || next >= total {
				if !wrap {
					break;
				}
				next = if direction > 0 { 0 } else { total - 1 };
			}
			if next == index {
				break;
			}
			index = next;
			if !Self::is_disabled(&self.visible[index as usize]) {
				remaining -= 1;
			}
			attempts += 1;
		}
		if index != self.selected {
			self.selected = index;
			self.ensure_selected_visible();
			self.notify_selection();
		}
	}

	fn is_recent_or_role(&self, item: &ModelBrowserItem) -> bool {
		if self.mru_order.iter().any(|key| key == &item.selector) {
			return true;
		}
		self.roles
			.values()
			.any(|assignment| assignment.selector == item.selector)
	}

	fn insert_separator(&self, items: Vec<ModelBrowserItem>) -> Vec<ModelBrowserItem> {
		let mut filtered: Vec<ModelBrowserItem> =
			items.into_iter().filter(|item| !item.is_separator()).collect();
		let Some(first) = filtered.iter().position(|item| !self.is_recent_or_role(item)) else {
			return filtered;
		};
		if first == 0 || first >= filtered.len() {
			return filtered;
		}
		filtered.insert(first, ModelBrowserItem::separator());
		filtered
	}

	/// Rank base items for the live query and re-seat the separator.
	fn filter_items(&self, items: &[ModelBrowserItem], query: &str) -> Vec<ModelBrowserItem> {
		let base: Vec<ModelBrowserItem> = items
			.iter()
			.filter(|item| !item.is_separator())
			.cloned()
			.collect();
		if self.preserve_query_order {
			if query.trim().is_empty() {
				return self.insert_separator(base);
			}
			let ranked = fuzzy::fuzzy_filter(query, &base.iter().map(model_search_text).collect::<Vec<_>>());
			return self.insert_separator(ranked.into_iter().map(|index| base[index].clone()).collect());
		}
		if query.trim().is_empty() {
			return self.insert_separator(base);
		}
		match &self.ranker {
			Some(ranker) => self.insert_separator(ranker.rank(query)),
			None => self.insert_separator(base),
		}
	}

	fn rebuild_ranker(&mut self) {
		if self.query().trim().is_empty() {
			self.ranker = None;
			return;
		}
		let base: Vec<ModelBrowserItem> = self
			.items
			.iter()
			.filter(|item| !item.is_separator())
			.cloned()
			.collect();
		self.ranker = Some(ModelItemRanker::new(
			base,
			ModelRankOptions {
				roles: self.roles.clone(),
				mru_order: self.mru_order.clone(),
				affinity: self.affinity.clone(),
			},
		));
	}

	fn invalidate_ranker(&mut self) {
		self.ranker = None;
		if !self.query().trim().is_empty() {
			self.rebuild_ranker();
			self.refilter(None);
		}
	}

	/// Re-run the filter and keep the selection on `preferred` when it survives.
	fn refilter(&mut self, preferred: Option<&str>) {
		let previous = preferred
			.map(str::to_string)
			.or_else(|| self.selected_item().map(|item| item.selector.clone()));
		let query = self.query().to_string();
		let base = self.items.clone();
		self.visible = self.filter_items(&base, &query);
		if self.visible.is_empty() {
			self.selected = -1;
		} else if let Some(selector) = previous.as_deref() {
			self.selected = self
				.visible
				.iter()
				.position(|item| item.selector == selector && !Self::is_disabled(item))
				.map(|index| index as isize)
				.unwrap_or_else(|| self.first_selectable());
		} else {
			self.selected = self.first_selectable();
		}
		self.ensure_selected_visible();
		self.notify_selection();
	}

	fn first_selectable(&self) -> isize {
		self.visible
			.iter()
			.position(|item| !Self::is_disabled(item))
			.map(|index| index as isize)
			.unwrap_or(-1)
	}

	fn notify_selection(&mut self) {
		let selected = self.selected_item().cloned();
		if let Some(callback) = self.on_selection_change.as_mut() {
			callback(selected.as_ref());
		}
	}

	/// Cancel-key ladder: clear a non-empty query first, then bubble to the host.
	pub fn handle_cancel(&mut self) {
		if !self.query().is_empty() {
			self.set_query("");
			if let Some(callback) = self.on_query_change.as_mut() {
				callback("");
			}
			return;
		}
		if let Some(callback) = self.on_cancel.as_mut() {
			callback();
		}
	}

	/// Route a mouse event. `line` is relative to the browser's first row.
	pub fn route_mouse(&mut self, event: &MouseEvent, line: usize) {
		match event.kind {
			MouseKind::ScrollUp => {
				self.window_start = self.clamp_window_start(self.window_start.saturating_sub(1));
				self.hovered = self.hover_index_at(line);
			}
			MouseKind::ScrollDown => {
				self.window_start = self.clamp_window_start(self.window_start + 1);
				self.hovered = self.hover_index_at(line);
			}
			MouseKind::Moved | MouseKind::Drag(_) => {
				self.hovered = self.hover_index_at(line);
			}
			MouseKind::Down(MouseButtonKind::Left) | MouseKind::Up(MouseButtonKind::Left) => {
				if event.kind != MouseKind::Down(MouseButtonKind::Left) {
					return;
				}
				let Some(index) = self.hover_index_at(line) else {
					return;
				};
				if index == self.selected as usize {
					let activated = self.visible[index].clone();
					if let Some(callback) = self.on_activate.as_mut() {
						callback(&activated);
					}
				} else {
					self.selected = index as isize;
					self.ensure_selected_visible();
					self.notify_selection();
				}
			}
			_ => {}
		}
	}

	/// Drop the hover band.
	pub fn clear_hover(&mut self) {
		self.hovered = None;
	}

	/// List index under a frame-local row, `None` when off-list or disabled.
	fn hover_index_at(&self, line: usize) -> Option<usize> {
		let list_line = line.checked_sub(LIST_ROW_START)?;
		if list_line >= self.window_count {
			return None;
		}
		let index = self.window_start + list_line;
		let item = self.visible.get(index)?;
		if Self::is_disabled(item) {
			None
		} else {
			Some(index)
		}
	}

	/// Measured perf for a row: the host's tier aggregate, the standard
	/// aggregate, else any measured tier (fastest first).
	fn perf_for(&self, item: &ModelBrowserItem) -> Option<MeasuredPerf> {
		if self.perf.is_empty() {
			return None;
		}
		if let Some(tier) = self.source.service_tier_for(&item.model) {
			if let Some(perf) = self.perf.get(&format!("{}@{tier}", item.selector)) {
				return Some(MeasuredPerf {
					perf: perf.clone(),
					tier: Some(tier),
				});
			}
		}
		if let Some(perf) = self.perf.get(&item.selector) {
			return Some(MeasuredPerf {
				perf: perf.clone(),
				tier: None,
			});
		}
		let prefix = format!("{}@", item.selector);
		let mut best: Option<MeasuredPerf> = None;
		for (key, perf) in &self.perf {
			if let Some(tier) = key.strip_prefix(&prefix) {
				if best
					.as_ref()
					.map_or(true, |current| perf.tps > current.perf.tps)
				{
					best = Some(MeasuredPerf {
						perf: perf.clone(),
						tier: Some(tier.to_string()),
					});
				}
			}
		}
		best
	}

	/// Measured TPS/TTFT, falling back to the catalog TPS as `~118t/s`.
	fn perf_cell(&self, item: &ModelBrowserItem, mode: PerfMode) -> String {
		if mode == PerfMode::Off {
			return String::new();
		}
		if let Some(measured) = self.perf_for(item) {
			let tps = format_tps(measured.perf.tps);
			let tier = measured
				.tier
				.map(|tier| format!(" {tier}"))
				.unwrap_or_default();
			if mode == PerfMode::Full {
				if let Some(ttft) = measured.perf.ttft_ms {
					return format!("{} {tps}{tier}", format_ttft(ttft));
				}
			}
			return format!("{tps}{tier}");
		}
		match item.model.summary.tps {
			Some(tps) if tps.is_finite() && tps > 0.0 => format!("~{}", format_tps(tps)),
			_ => String::new(),
		}
	}

	#[allow(clippy::too_many_arguments)]
	fn render_row(
		&self,
		item: &ModelBrowserItem,
		width: usize,
		selected: bool,
		hovered: bool,
		ctx_width: usize,
		cost_width: usize,
		intelligence_width: usize,
		perf_width: usize,
		intelligence: &str,
		ctx: &str,
		cost: &str,
		perf_cell: &str,
	) -> String {
		if item.is_separator() {
			let dash_count = width.saturating_sub(4);
			let line = self
				.theme
				.fg("muted", &"─".repeat(dash_count))
				.unwrap_or_default();
			return format!("  {line}  ");
		}
		let over_context = self.is_over_context(item);
		let prefix = if selected && self.focused {
			format!(
				"{} ",
				self.theme
					.fg("accent", &self.theme.nav().cursor)
					.unwrap_or_default()
			)
		} else {
			"  ".into()
		};
		let provider_prefix = if self.show_provider {
			self.theme
				.fg("dim", &format!("{}/", item.provider))
				.unwrap_or_default()
		} else {
			String::new()
		};
		let name = if let Some(color) = item.label_color.as_deref() {
			self.theme.fg(color, &item.id).unwrap_or_default()
		} else if selected {
			self.theme.fg("accent", &item.id).unwrap_or_default()
		} else {
			item.id.clone()
		};
		let current_mark = if Some(&item.selector) == self.current_selector.as_ref() {
			format!(
				" {}",
				self.theme
					.fg("success", &self.theme.status().enabled)
					.unwrap_or_default()
			)
		} else {
			String::new()
		};
		let over_limit = if over_context {
			format!(
				" {} context>{}",
				self.theme.status().disabled,
				format_number(item.model.summary.context_window.unwrap_or(0))
			)
		} else {
			String::new()
		};
		let mut left = format!("{prefix}{provider_prefix}{name}{current_mark}{over_limit}");

		let mut cols: Vec<String> = Vec::new();
		if intelligence_width > 0 {
			cols.push(
				self.theme
					.fg("dim", &pad_left_visible(intelligence, intelligence_width))
					.unwrap_or_default(),
			);
		}
		if perf_width > 0 {
			cols.push(
				self.theme
					.fg("dim", &pad_left_visible(perf_cell, perf_width))
					.unwrap_or_default(),
			);
		}
		if ctx_width > 0 {
			cols.push(
				self.theme
					.fg("dim", &pad_left_visible(ctx, ctx_width))
					.unwrap_or_default(),
			);
		}
		if cost_width > 0 {
			cols.push(
				self.theme
					.fg("dim", &pad_left_visible(cost, cost_width))
					.unwrap_or_default(),
			);
		}
		let meta_width = meta_columns_width(&[intelligence_width, perf_width, ctx_width, cost_width]);
		let available = width
			.saturating_sub(meta_width)
			.saturating_sub(usize::from(!cols.is_empty()))
			.max(1);
		left = truncate_to_width(&left, available, Ellipsis::Omit);
		let gap = available.saturating_sub(visible_width(&left));

		let mut line = if cols.is_empty() {
			format!("{left}{}", " ".repeat(gap))
		} else {
			format!("{left}{} {}", " ".repeat(gap), cols.join("  "))
		};
		if over_context {
			let plain_prefix = strip_ansi(&prefix).chars().count();
			let body: String = strip_ansi(&line).chars().skip(plain_prefix).collect();
			line = format!(
				"{prefix}{}",
				self.theme.fg("dim", &body).unwrap_or_default()
			);
		}
		if hovered {
			line = self.theme.bg("selectedBg", &line).unwrap_or(line);
		}
		line
	}

	fn held_roles(&self, model: &CatalogModel) -> Vec<(String, RoleAssignment)> {
		let mut held = Vec::new();
		let mut seen = HashSet::new();
		let mut push = |role: &str| {
			if !seen.insert(role.to_string()) {
				return;
			}
			let Some(assignment) = self.roles.get(role) else {
				return;
			};
			if assignment.selector != model.selector() {
				return;
			}
			if self.source.get_role_info(role).hidden {
				return;
			}
			held.push((role.to_string(), assignment.clone()));
		};
		for role in MODEL_ROLE_IDS {
			push(role);
		}
		for role in self.roles.keys() {
			push(role);
		}
		held
	}

	fn detail_lines(&self, width: usize) -> (String, String) {
		let Some(selected) = self.selected_item() else {
			return (String::new(), String::new());
		};
		let model = &selected.model;
		let summary = &model.summary;

		let mut facts: Vec<String> = vec![model.display_name().into()];
		if summary.is_new {
			facts.push("new".into());
		}
		if summary.is_beta {
			facts.push("beta".into());
		}
		if summary.is_recommended {
			facts.push("recommended".into());
		}
		if let Some(ctx) = summary.context_window.filter(|&ctx| ctx > 0) {
			facts.push(format!("{} ctx", format_number(ctx)));
		}
		if let Some(out) = summary.max_tokens.filter(|&out| out > 0) {
			facts.push(format!("{} out", format_number(out)));
		}
		facts.push(format_cost_detail(model));
		if summary.reasoning {
			facts.push("reasoning".into());
		}
		if summary.input.iter().any(|input| input == "image") {
			facts.push("vision".into());
		}
		let intelligence = format_intelligence(model, &self.theme);
		if !intelligence.is_empty() {
			facts.push(intelligence);
		}
		if let Some(measured) = self.perf_for(selected) {
			let tier = measured
				.tier
				.map(|tier| format!(" {tier}"))
				.unwrap_or_default();
			facts.push(format!("~{} {}", format_tps(measured.perf.tps), tier.trim_end()));
			if let Some(ttft) = measured.perf.ttft_ms {
				facts.push(format!("{} ttft", format_ttft(ttft)));
			}
		} else if let Some(tps) = summary.tps {
			if tps.is_finite() && tps > 0.0 {
				facts.push(format!("~{}", format_tps(tps)));
			}
		}
		if let Some(note) = next_credit_rate_note(model) {
			facts.push(note);
		}
		if let Some(description) = summary.description.as_deref() {
			let description = format_description(description);
			if !description.is_empty() {
				facts.push(description);
			}
		}
		let line1 = truncate_to_width(
			&self
				.theme
				.fg("muted", &format!("  {}", facts.join(" · ")))
				.unwrap_or_default(),
			width,
			Ellipsis::Omit,
		);

		if self.is_over_context(selected) {
			let warning = format!(
				"  {} context {} exceeds {} limit · compacts with current model, then switches",
				self.theme.status().disabled,
				format_number(self.current_context_tokens),
				format_number(summary.context_window.unwrap_or(0))
			);
			return (
				line1,
				truncate_to_width(
					&self.theme.fg("warning", &warning).unwrap_or_default(),
					width,
					Ellipsis::Omit,
				),
			);
		}

		let mut chips: Vec<String> = Vec::new();
		if Some(&selected.selector) == self.current_selector.as_ref() {
			chips.push(
				self.theme
					.fg("success", &format!("{} current", self.theme.status().enabled))
					.unwrap_or_default(),
			);
		}
		for (role, assignment) in self.held_roles(model) {
			chips.push(format_role_chip(
				&role,
				&assignment,
				self.source.as_ref(),
				&self.theme,
			));
		}
		let line2 = if chips.is_empty() {
			String::new()
		} else {
			truncate_to_width(
				&format!(
					"  {}",
					chips.join(&self.theme.fg("dim", " · ").unwrap_or_default())
				),
				width,
				Ellipsis::Omit,
			)
		};
		(line1, line2)
	}
}

impl Component for ModelBrowser {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width = width as usize;
		let mut lines: Vec<String> = Vec::new();

		let search_icon = self.theme.symbol("icon.search");
		let input_width = width.saturating_sub(visible_width(search_icon) + 2).max(4);
		let mut search_lines = self.search.render(input_width as u16);
		let input_line = search_lines.pop().unwrap_or_default();
		lines.push(format!(
			" {} {input_line}",
			self.theme.fg("accent", search_icon).unwrap_or_default()
		));
		lines.push(String::new());

		let total = self.visible.len();
		self.window_start = self.clamp_window_start(self.window_start);
		let start_index = self.window_start;
		let end_index = (start_index + self.max_visible).min(total);
		self.window_count = end_index.saturating_sub(start_index);

		if total == 0 {
			let empty = self.empty_text.as_ref().and_then(|f| f());
			let message = empty.unwrap_or_else(|| {
				if self.query().trim().is_empty() {
					"  No models available in this scope".into()
				} else {
					"  No matching models".into()
				}
			});
			lines.push(truncate_to_width(
				&self.theme.fg("muted", &message).unwrap_or_default(),
				width,
				Ellipsis::Omit,
			));
			for _ in 1..self.max_visible {
				lines.push(String::new());
			}
		} else {
			let perf_mode = if width >= PERF_FULL_MIN_WIDTH {
				PerfMode::Full
			} else if width >= PERF_TPS_MIN_WIDTH {
				PerfMode::Tps
			} else {
				PerfMode::Off
			};
			let mut ctx_width = 0;
			let mut cost_width = 0;
			let mut intelligence_width = 0;
			let mut perf_width = 0;
			let mut cells: Vec<(String, String, String, String, usize, usize, usize, usize)> = Vec::new();
			for index in start_index..end_index {
				let item = &self.visible[index];
				let intelligence = format_intelligence(&item.model, &self.theme);
				let ctx = format_context(&item.model, &self.theme);
				let cost = format_cost_pair(&item.model);
				let perf_cell = self.perf_cell(item, perf_mode);
				let intelligence_width_cell = visible_width(&intelligence);
				let ctx_cell_width = visible_width(&ctx);
				let cost_cell_width = visible_width(&cost);
				let perf_cell_width = visible_width(&perf_cell);
				ctx_width = ctx_width.max(ctx_cell_width);
				cost_width = cost_width.max(cost_cell_width);
				if perf_mode != PerfMode::Off {
					intelligence_width = intelligence_width.max(intelligence_width_cell);
				}
				perf_width = perf_width.max(perf_cell_width);
				cells.push((
					intelligence,
					ctx,
					cost,
					perf_cell,
					intelligence_width_cell,
					ctx_cell_width,
					cost_cell_width,
					perf_cell_width,
				));
			}
			let mut name_room = width
				.saturating_sub(2)
				.saturating_sub(meta_columns_width(&[
					intelligence_width,
					perf_width,
					ctx_width,
					cost_width,
				]));
			if name_room < MIN_NAME_WIDTH {
				cost_width = 0;
			}
			name_room = width
				.saturating_sub(2)
				.saturating_sub(meta_columns_width(&[
					intelligence_width,
					perf_width,
					ctx_width,
					cost_width,
				]));
			if name_room < MIN_NAME_WIDTH {
				ctx_width = 0;
			}
			// Per-cell widths are clamped to the surviving columns.
			for (offset, index) in (start_index..end_index).enumerate() {
				let item = &self.visible[index];
				let (intelligence, ctx, cost, perf_cell, _, _, _, _) = &cells[offset];
				let mut row = self.render_row(
					item,
					width.saturating_sub(1),
					index as isize == self.selected,
					Some(index) == self.hovered,
					ctx_width,
					cost_width,
					intelligence_width,
					perf_width,
					intelligence,
					ctx,
					cost,
					perf_cell,
				);
				row = truncate_to_width(&row, width, Ellipsis::Omit);
				lines.push(row);
			}
			for _ in (end_index - start_index)..self.max_visible {
				lines.push(String::new());
			}
		}

		lines.push(String::new());
		let (detail1, detail2) = self.detail_lines(width);
		lines.push(detail1);
		lines.push(detail2);
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		if is_escape(key) {
			self.handle_cancel();
			return;
		}
		if is_arrow_up(key) {
			self.move_selection(-1, true);
			return;
		}
		if is_arrow_down(key) {
			self.move_selection(1, true);
			return;
		}
		if is_page_up(key) {
			self.move_selection(-(self.max_visible as isize), false);
			return;
		}
		if is_page_down(key) {
			self.move_selection(self.max_visible as isize, false);
			return;
		}
		if is_home(key) {
			self.move_selection(-(self.visible.len() as isize), false);
			return;
		}
		if is_end(key) {
			self.move_selection(self.visible.len() as isize, false);
			return;
		}
		if is_enter(key) {
			if let Some(selected) = self.selected_item() {
				if !Self::is_disabled(selected) {
					let item = selected.clone();
					if let Some(callback) = self.on_activate.as_mut() {
						callback(&item);
					}
				}
			}
			return;
		}
		let before = self.query().to_string();
		self.search.handle_input(key);
		let after = self.query().to_string();
		if after != before {
			self.rebuild_ranker();
			self.refilter(None);
			if let Some(callback) = self.on_query_change.as_mut() {
				callback(&after);
			}
		}
	}

	fn handle_paste(&mut self, text: &str) {
		let before = self.query().to_string();
		self.search.handle_paste(text);
		let after = self.query().to_string();
		if after != before {
			self.rebuild_ranker();
			self.refilter(None);
			if let Some(callback) = self.on_query_change.as_mut() {
				callback(&after);
			}
		}
	}

	fn invalidate(&mut self) {}
}

impl ModelBrowser {
	/// Base rows of the current scope, before the query (the head's "of N").
	pub fn base_count(&self) -> usize {
		self.items.iter().filter(|item| !item.is_separator()).count()
	}

	/// The selected row's selector, `None` when nothing selectable is selected.
	pub fn picker_selected(&self) -> Option<&str> {
		self.selected_item()
			.filter(|item| !Self::is_disabled(item))
			.map(|item| item.selector.as_str())
	}

	/// `col[search row, list, detail]`: the query field, every visible model as
	/// a keyed `item`, and the selection's facts and role chips.
	pub fn describe(&mut self) -> NativeNode {
		let search = row(
			vec![
				text(
					NodeText::Spans(vec![span(self.theme.symbol("icon.search"), Some("accent"))]),
					Props::new(),
				),
				text(
					NodeText::Plain(format!("{} ", self.query())),
					Props::new(),
				),
			],
			Props::new(),
		);
		let list = self.describe_list();
		let detail = self.describe_detail();
		let mut props = Props::new();
		props.set_str("gap", "sm");
		col(vec![search, list, detail], props)
	}

	/// List `select`/`activate` on a model = highlight it, then Enter.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		let (key, item) = match event {
			NativeUiEvent::Select { key, item } | NativeUiEvent::Activate { key, item } => (key, item),
			_ => return,
		};
		if !key.iter().any(|segment| segment == "list") {
			return;
		}
		let Some(index) = self.visible.iter().position(|row| &row.selector == item) else {
			return;
		};
		if Self::is_disabled(&self.visible[index]) {
			return;
		}
		let changed = index as isize != self.selected;
		self.selected = index as isize;
		self.ensure_selected_visible();
		if changed {
			self.notify_selection();
		}
		let activated = self.visible[index].clone();
		if let Some(callback) = self.on_activate.as_mut() {
			callback(&activated);
		}
	}

	fn describe_list(&self) -> NativeNode {
		let children: Vec<NativeNode> = self
			.visible
			.iter()
			.map(|item| self.describe_item(item))
			.collect();
		let mut props = Props::new();
		props.set_str("role", "omp.model-browser.list");
		if let Some(selected) = self.selected_item() {
			props.set_str("selected", &selected.selector);
		}
		let filter = self.query().trim();
		if !filter.is_empty() {
			props.set_str("filter", filter);
		}
		if self.visible.is_empty() {
			let empty = self
				.empty_text
				.as_ref()
				.and_then(|f| f())
				.unwrap_or_else(|| {
					if filter.is_empty() {
						"No models available in this scope".into()
					} else {
						"No matching models".into()
					}
				});
			props.insert("empty", serde_json::json!([{"t": empty, "s": "muted"}]));
		}
		props.set_bool("virtual", true);
		props.set_i64("grow", 1);
		node(NodeKind::List, props, children, Some("list"))
	}

	fn describe_item(&self, item: &ModelBrowserItem) -> NativeNode {
		if item.is_separator() {
			return node(NodeKind::Rule, Props::new(), Vec::new(), Some("separator"));
		}
		let mut label = Vec::new();
		if self.show_provider {
			label.push(span(format!("{}/", item.provider), Some("dim")));
		}
		label.push(span(
			item.id.clone(),
			item.label_color.as_deref(),
		));
		if Some(&item.selector) == self.current_selector.as_ref() {
			label.push(span(
				format!(" {}", self.theme.status().enabled),
				Some("success"),
			));
		}
		let metrics: Vec<String> = vec![
			format_intelligence(&item.model, &self.theme),
			self.perf_cell(item, PerfMode::Full),
			format_context(&item.model, &self.theme),
			format_cost_pair(&item.model),
		]
		.into_iter()
		.filter(|metric| !metric.is_empty())
		.collect();
		let over_context = self.is_over_context(item);
		let mut props = Props::new();
		props.set_spans(label);
		props.insert(
			"value",
			serde_json::Value::Array(vec![span(metrics.join("  "), Some("dim")).to_value()]),
		);
		if over_context {
			props.insert(
				"detail",
				serde_json::Value::Array(vec![span(
					format!(
						"{} context>{}",
						self.theme.status().disabled,
						format_number(item.model.summary.context_window.unwrap_or(0))
					),
					Some("warning"),
				)
				.to_value()]),
			);
			props.set_str("tone", "muted");
		}
		node(NodeKind::Item, props, Vec::new(), Some(item.selector.as_str()))
	}

	fn describe_detail(&self) -> NativeNode {
		let mut children: Vec<NativeNode> = Vec::new();
		if let Some(selected) = self.selected_item() {
			let model = &selected.model;
			let summary = &model.summary;
			let mut head: Vec<NativeNode> = vec![text(
				NodeText::Spans(vec![span(model.display_name(), Some("strong"))]),
				Props::new(),
			)];
			for (flag, text_value, tone) in [
				(summary.is_new, "new", "accent"),
				(summary.is_beta, "beta", "warning"),
				(summary.is_recommended, "recommended", "success"),
			] {
				if flag {
					let mut props = Props::new();
					props.set_str("text", text_value);
					props.set_str("tone", tone);
					head.push(node(NodeKind::Badge, props, Vec::new(), None));
				}
			}
			let mut row_props = Props::new();
			row_props.set_str("gap", "sm");
			row_props.set_str("align", "center");
			row_props.set_bool("wrap", true);
			children.push(row(head, row_props));

			let mut facts: Vec<String> = Vec::new();
			if let Some(ctx) = summary.context_window.filter(|&ctx| ctx > 0) {
				facts.push(format!("{} ctx", format_number(ctx)));
			}
			if let Some(out) = summary.max_tokens.filter(|&out| out > 0) {
				facts.push(format!("{} out", format_number(out)));
			}
			facts.push(format_cost_detail(model));
			if summary.reasoning {
				facts.push("reasoning".into());
			}
			if summary.input.iter().any(|input| input == "image") {
				facts.push("vision".into());
			}
			let intelligence = format_intelligence(model, &self.theme);
			if !intelligence.is_empty() {
				facts.push(intelligence);
			}
			if let Some(measured) = self.perf_for(selected) {
				let tier = measured
					.tier
					.map(|tier| format!(" {tier}"))
					.unwrap_or_default();
				facts.push(format!("~{} {}", format_tps(measured.perf.tps), tier.trim_end()));
				if let Some(ttft) = measured.perf.ttft_ms {
					facts.push(format!("{} ttft", format_ttft(ttft)));
				}
			}
			if let Some(note) = next_credit_rate_note(model) {
				facts.push(note);
			}
			let mut fact_props = Props::new();
			fact_props.set_str("wrap", "word");
			children.push(text(
				NodeText::Spans(vec![span(facts.join(" · "), Some("muted"))]),
				fact_props,
			));
			if let Some(description) = summary.description.as_deref() {
				let description = format_description(description);
				if !description.is_empty() {
					let mut props = Props::new();
					props.set_str("wrap", "word");
					props.set_i64("lines", 2);
					children.push(text(
						NodeText::Spans(vec![span(description, Some("dim"))]),
						props,
					));
				}
			}

			if self.is_over_context(selected) {
				children.push(text(
					NodeText::Spans(vec![span(
						format!(
							"{} context {} exceeds {} limit · compacts with current model, then switches",
							self.theme.status().disabled,
							format_number(self.current_context_tokens),
							format_number(summary.context_window.unwrap_or(0))
						),
						Some("warning"),
					)]),
					Props::new(),
				));
			} else {
				let mut chips = Vec::new();
				if Some(&selected.selector) == self.current_selector.as_ref() {
					chips.push(span(
						format!("{} current", self.theme.status().enabled),
						Some("success"),
					));
				}
				for (role, assignment) in self.held_roles(model) {
					let info = self.source.get_role_info(&role);
					let label = info.label(&role);
					if assignment.auto_selected {
						chips.push(span(format!("{} {label}", self.theme.status().shadowed), Some("dim")));
					} else {
						chips.push(span(
							format!("{} {label}", self.theme.status().enabled),
							Some(info.color.as_deref().unwrap_or("muted")),
						));
					}
					let glyph = thinking_level_glyph(assignment.thinking_level, &self.theme);
					if !glyph.is_empty() {
						chips.push(span(format!(" {glyph}"), Some("dim")));
					}
				}
				if !chips.is_empty() {
					children.push(text(NodeText::Spans(chips), Props::new()));
				}
			}
		}
		let mut props = Props::new();
		props.set_str("role", "omp.model-browser.detail");
		props.set_str("gap", "none");
		node(NodeKind::Col, props, children, Some("detail"))
	}

	/// The visible rows as picker order: the Recent block, then the rest per
	/// provider or under `grouping.rest`.
	pub fn picker_order(&self, grouping: &PickerGrouping) -> ModelPickerOrder {
		let visible = &self.visible;
		let query = self.query().trim();
		let split = visible.iter().position(|item| item.is_separator());
		let mut order = Vec::new();
		let mut count = 0usize;
		let mut push_rows = |rows: &[ModelBrowserItem], order: &mut Vec<PickerOrderEntry>, count: &mut usize| {
			for item in rows {
				if item.is_separator() {
					continue;
				}
				order.push(PickerOrderEntry::Item(item.selector.clone()));
				*count += 1;
			}
		};
		let (recent, rest): (&[ModelBrowserItem], &[ModelBrowserItem]) = match split {
			Some(index) => (&visible[..index], &visible[index + 1..]),
			None => (&[], visible),
		};
		if grouping.rest.is_none() {
			push_rows(visible, &mut order, &mut count);
		} else {
			if !recent.is_empty() {
				order.push(PickerOrderEntry::Group {
					group: "recent".into(),
					label: "Recent".into(),
					count: recent.len(),
				});
				push_rows(recent, &mut order, &mut count);
			}
			if grouping.providers {
				let mut start = 0;
				while start < rest.len() {
					let provider = rest[start].provider.clone();
					let mut end = start + 1;
					while end < rest.len() && rest[end].provider == provider {
						end += 1;
					}
					order.push(PickerOrderEntry::Group {
						group: format!("provider:{provider}:{start}"),
						label: provider,
						count: end - start,
					});
					push_rows(&rest[start..end], &mut order, &mut count);
					start = end;
				}
			} else {
				if !recent.is_empty() && !rest.is_empty() {
					order.push(PickerOrderEntry::Group {
						group: "rest".into(),
						label: grouping.rest.clone().unwrap_or_default(),
						count: rest.len(),
					});
				}
				push_rows(rest, &mut order, &mut count);
			}
		}

		let mut hits = None;
		if !query.is_empty() {
			let mut map = HashMap::new();
			for item in visible {
				if item.is_separator() {
					continue;
				}
				let label = if item.provider.is_empty() {
					item.id.clone()
				} else {
					item.selector.clone()
				};
				if let Some(ranges) = crate::native::picker::picker_fuzzy_hits(&label, query) {
					map.insert(item.selector.clone(), ranges);
				}
			}
			hits = Some(map);
		}
		ModelPickerOrder { order, hits, count }
	}

	/// Preview children for any model row: `full` (the model hub's side pane)
	/// or `compact` (the quick picker's strip below the list).
	pub fn model_preview(
		&self,
		item: &ModelBrowserItem,
		compact: bool,
		current: Option<&str>,
	) -> Vec<NativeNode> {
		let model = &item.model;
		let summary = &model.summary;
		let selector = model.selector();
		let measured = self.perf_for(item);
		let ctx = summary.context_window.unwrap_or(0);
		let out = summary.max_tokens.unwrap_or(0);
		let over_context = self.is_over_context(item);
		let warning = if over_context {
			let mut props = Props::new();
			props.set_str("wrap", "word");
			Some(text(
				NodeText::Spans(vec![span(
					format!(
						"Context {} exceeds the {} limit · compacts with the current model, then switches",
						format_number(self.current_context_tokens),
						format_number(ctx)
					),
					Some("warning"),
				)]),
				props,
			))
		} else {
			None
		};
		let held = self.held_roles(model);
		let role_badge = |role: &str, assignment: Option<&RoleAssignment>| -> NativeNode {
			let info = self.source.get_role_info(role);
			let level = assignment
				.filter(|assignment| {
					assignment.thinking_level != ConfiguredThinkingLevel::Inherit
				})
				.map(|assignment| {
					crate::thinking::get_configured_thinking_level_metadata(assignment.thinking_level).label
				});
			let mut props = Props::new();
			props.set_str("text", info.label(role));
			let title = match assignment {
				Some(assignment) => format!(
					"{}{}{}",
					info.name,
					if assignment.auto_selected {
						" (auto-selected)"
					} else {
						""
					},
					level
						.as_ref()
						.map(|level| format!(" · thinking {level}"))
						.unwrap_or_default()
				),
				None => format!("{}: not assigned", info.name),
			};
			props.set_str("title", title);
			if let Some(assignment) = assignment {
				props.set_str(
					"tone",
					if assignment.auto_selected {
						"muted"
					} else {
						"accent"
					},
				);
			}
			node(NodeKind::Badge, props, Vec::new(), None)
		};

		if compact {
			let mut facts: Vec<(String, NodeText)> = Vec::new();
			let mut fact = |key: &str, value: String| {
				facts.push((key.to_string(), NodeText::Plain(value)));
			};
			if ctx > 0 {
				fact("ctx", format_number(ctx));
			}
			if out > 0 {
				fact("out", format_number(out));
			}
			fact(
				"price",
				if is_free_model(model) {
					"free".into()
				} else {
					format_cost_detail(model)
				},
			);
			if let Some(note) = next_credit_rate_note(model) {
				fact("then", note);
			}
			fact("reasoning", if summary.reasoning { "yes".into() } else { "no".into() });
			let mut kv_props = Props::new();
			kv_props.set_str("layout", "inline");
			let mut children: Vec<NativeNode> = Vec::new();
			if let Some(grid) = kv(facts.into_iter().map(|(k, v)| (k, v)).collect()) {
				children.push(grid);
			}
			let mut chips: Vec<NativeNode> = Vec::new();
			if Some(item.selector.as_str()) == current {
				let mut props = Props::new();
				props.set_str("text", "current");
				props.set_str("tone", "success");
				chips.push(node(NodeKind::Badge, props, Vec::new(), None));
			}
			for (role, assignment) in &held {
				chips.push(role_badge(role, Some(assignment)));
			}
			if !chips.is_empty() {
				let mut props = Props::new();
				props.set_str("gap", "xs");
				props.set_bool("wrap", true);
				children.push(row(chips, props));
			}
			if let Some(warning) = warning {
				children.push(warning);
			}
			return children;
		}

		let mut children: Vec<NativeNode> = vec![
			text(
				NodeText::Plain(model.display_name().into()),
				{
					let mut props = Props::new();
					props.set_str("role", "omp.picker.title");
					props
				},
			),
			text(
				NodeText::Spans(vec![span(selector.clone(), Some("mono"))]),
				{
					let mut props = Props::new();
					props.set_str("title", "Copy model id");
					props.set_str("truncate", "middle");
					props.insert("actions", serde_json::json!({ "click": "copy" }));
					props
				},
			),
		];
		let mut badges: Vec<NativeNode> = Vec::new();
		let mut badge = |text_value: &str, tone: Option<&str>| {
			let mut props = Props::new();
			props.set_str("text", text_value);
			if let Some(tone) = tone {
				props.set_str("tone", tone);
			}
			badges.push(node(NodeKind::Badge, props, Vec::new(), None));
		};
		if Some(item.selector.as_str()) == current {
			badge("current", Some("success"));
		}
		if summary.is_new {
			badge("new", Some("accent"));
		}
		if summary.is_beta {
			badge("beta", Some("warning"));
		}
		if summary.is_recommended {
			badge("recommended", Some("success"));
		}
		if summary.reasoning {
			badge("reasoning", None);
		}
		if summary.input.iter().any(|input| input == "image") {
			badge("vision", None);
		}
		if !badges.is_empty() {
			let mut props = Props::new();
			props.set_str("gap", "xs");
			props.set_bool("wrap", true);
			children.push(row(badges, props));
		}

		let mut speed: Vec<String> = Vec::new();
		if let Some(measured) = &measured {
			let tier = measured
				.tier
				.as_ref()
				.map(|tier| format!(" {tier}"))
				.unwrap_or_default();
			speed.push(format!(
				"{}{}",
				format_tps(measured.perf.tps).replace("t/s", " t/s"),
				tier
			));
			if let Some(ttft) = measured.perf.ttft_ms {
				speed.push(format!("{} TTFT", format_ttft(ttft).replace('s', " s")));
			}
			speed.push(format!(
				"{} {}",
				measured.perf.samples,
				if measured.perf.samples == 1 {
					"sample"
				} else {
					"samples"
				}
			));
		} else if let Some(tps) = summary.tps {
			if tps.is_finite() && tps > 0.0 {
				speed.push(format!(
					"~{} (catalog)",
					format_tps(tps).replace("t/s", " t/s")
				));
			}
		}
		let mut facts: Vec<(String, NodeText)> = Vec::new();
		{
			let mut fact = |key: &str, value: Option<String>| {
				if let Some(value) = value.filter(|value| !value.is_empty()) {
					facts.push((
						key.to_string(),
						NodeText::Spans(vec![span(value, Some("mono"))]),
					));
				}
			};
			fact(
				"Context",
				if ctx > 0 {
					Some(group_thousands(ctx))
				} else {
					None
				},
			);
		}
		if let Some(compaction) = self.source.compaction_point_for(model) {
			let value = match compaction.tokens {
				None => "off".to_string(),
				Some(tokens) => format!(
					"{}{}",
					group_thousands(tokens),
					compaction
						.basis
						.as_ref()
						.map(|basis| format!(" · {basis}"))
						.unwrap_or_default()
				),
			};
			facts.push((
				"Compacts at".into(),
				NodeText::Spans(vec![
					span(value, Some("mono")),
					span(format!(" · {}", compaction.source), Some("dim")),
				]),
			));
		}
		{
			let mut fact = |key: &str, value: Option<String>| {
				if let Some(value) = value.filter(|value| !value.is_empty()) {
					facts.push((
						key.to_string(),
						NodeText::Spans(vec![span(value, Some("mono"))]),
					));
				}
			};
			fact(
				"Max output",
				if out > 0 {
					Some(group_thousands(out))
				} else {
					None
				},
			);
			fact(
				"Price",
				Some(if is_free_model(model) {
					"free".into()
				} else {
					format!("{} per M", preview_price(model))
				}),
			);
			fact("Speed", Some(speed.join(" · ")));
			fact(
				"Intelligence",
				summary.int.filter(|int| int.is_finite()).map(|int| format!("{}", int.round() as i64)),
			);
			fact(
				"Input",
				if summary.input.is_empty() {
					None
				} else {
					Some(summary.input.join(" · "))
				},
			);
			fact("Reasoning", Some(if summary.reasoning { "yes".into() } else { "no".into() }));
		}
		if let Some(grid) = kv(facts) {
			children.push(grid);
		}
		if let Some(warning) = warning {
			children.push(warning);
		}

		let held_by: HashMap<String, RoleAssignment> = held.into_iter().collect();
		let mut role_badges: Vec<NativeNode> = Vec::new();
		for role in self.source.known_role_ids() {
			let info = self.source.get_role_info(&role);
			if info.hidden || !self.source.role_accepts(&role, model) {
				continue;
			}
			role_badges.push(role_badge(&role, held_by.get(&role)));
		}
		if !role_badges.is_empty() {
			let mut row_props = Props::new();
			row_props.set_str("gap", "xs");
			row_props.set_bool("wrap", true);
			let mut section_props = Props::new();
			section_props.set_str("head", "Roles");
			children.push(node(
				NodeKind::Section,
				section_props,
				vec![row(role_badges, row_props)],
				None,
			));
		}
		if let Some(description) = summary
			.description
			.as_deref()
			.map(str::trim)
			.filter(|description| !description.is_empty())
		{
			children.push(md(description, Props::new()));
		}
		children
	}

	/// A picker row click: `select` highlights it, `activate` also runs Enter's
	/// path. False when not a visible row.
	pub fn route_picker_item(&mut self, selector: &str, activate: bool) -> bool {
		let Some(index) = self.visible.iter().position(|item| item.selector == selector) else {
			return false;
		};
		if Self::is_disabled(&self.visible[index]) {
			return false;
		}
		let changed = index as isize != self.selected;
		self.selected = index as isize;
		self.ensure_selected_visible();
		if changed {
			self.notify_selection();
		}
		if activate {
			let item = self.visible[index].clone();
			if let Some(callback) = self.on_activate.as_mut() {
				callback(&item);
			}
		}
		true
	}
}

fn group_thousands(n: u64) -> String {
	let digits = n.to_string();
	let mut out = String::with_capacity(digits.len() + digits.len() / 3);
	for (index, ch) in digits.chars().rev().enumerate() {
		if index > 0 && index % 3 == 0 {
			out.push(',');
		}
		out.push(ch);
	}
	out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn model(provider: &str, id: &str) -> CatalogModel {
		CatalogModel {
			provider: provider.into(),
			id: id.into(),
			summary: ModelSummary {
				name: id.into(),
				is_chat: true,
				..ModelSummary::default()
			},
		}
	}

	#[test]
	fn version_numbers_parse() {
		assert_eq!(extract_version_number("gemini-2.5-pro"), 2.5);
		assert_eq!(extract_version_number("claude-sonnet-4-6"), 4.6);
		assert_eq!(extract_version_number("gpt-4o"), 4.0);
		assert_eq!(extract_version_number("llama-3-1-8b"), 3.1);
		assert_eq!(extract_version_number("o3"), 0.0);
	}

	#[test]
	fn cost_pair_prefers_declared_status_then_free() {
		let free = model("nvidia", "free-one");
		assert_eq!(format_cost_pair(&free), "free");
		let mut varying = model("openrouter", "x");
		varying.summary.pricing_status = Some(PricingStatus::Variable);
		assert_eq!(format_cost_pair(&varying), "varies");
		let mut priced = model("anthropic", "claude");
		priced.summary.cost = Some(ModelCost {
			input: 3.0,
			output: 15.0,
			..ModelCost::default()
		});
		assert_eq!(format_cost_pair(&priced), "$3/15");
	}

	#[test]
	fn role_rank_orders_default_first() {
		let mut roles = RoleAssignments::new();
		roles.insert(
			"default".into(),
			RoleAssignment {
				selector: "b/b-model".into(),
				thinking_level: ConfiguredThinkingLevel::Inherit,
				auto_selected: false,
			},
		);
		let mut items = build_browser_items(&[model("a", "a-model"), model("b", "b-model")]);
		sort_model_items(
			&mut items,
			&SortModelItemsOptions {
				roles: Some(roles),
				..SortModelItemsOptions::default()
			},
		);
		assert_eq!(items[0].selector, "b/b-model");
	}

	#[test]
	fn ranker_prefers_exact_match_over_affinity() {
		let items = build_browser_items(&[model("openai", "gpt-4o"), model("anthropic", "claude-sonnet-4")]);
		let mut affinity = SearchAffinity::default();
		affinity.models.insert("anthropic/claude-sonnet-4".into(), 0);
		let ranker = ModelItemRanker::new(
			items,
			ModelRankOptions {
				affinity,
				..ModelRankOptions::default()
			},
		);
		let ranked = ranker.rank("gpt-4o");
		assert_eq!(ranked[0].selector, "openai/gpt-4o");
	}

	#[test]
	fn free_models_match_the_free_word() {
		let items = build_browser_items(&[model("nvidia", "model-x")]);
		assert_eq!(model_search_text(&items[0]), "nvidia/model-x free");
	}

	#[test]
	fn formatting_helpers() {
		assert_eq!(format_cost_leg(0.25), "0.25");
		assert_eq!(format_cost_leg(3.0), "3");
		assert_eq!(format_number(400_000), "400k");
		assert_eq!(format_tps(118.4), "118t/s");
		assert_eq!(format_ttft(900.0), "0.9s");
		assert_eq!(pad_left_visible("x", 3), "  x");
		assert_eq!(provider_mark("amazon-bedrock").text, "AB");
		assert_eq!(provider_mark("anthropic").text, "An");
	}
}
