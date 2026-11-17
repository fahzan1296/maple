//! Context-window usage estimation for the status line and the `/context` viewer.
//!
//! Reference: `l/src/status-line/context-usage.ts`.
//!
//! Deviations:
//! - The `@oh-my-pi/*` packages are unavailable, so [`Model`], [`ContextTool`],
//!   [`ContextMessage`] and [`CompactionSettings`] are compact local types, and
//!   tool wire schemas are built with `serde_json` instead of pi-ai's
//!   `toolWireSchema` / `stripSchemaDescriptions` / `renderToolExamples`.
//! - The reference caches through `WeakMap`s keyed by caller-owned arrays and
//!   tokenizer instances. Here they are an explicit [`ContextUsageCache`] that
//!   the caller keeps (and drops with its session); rosters are keyed by slice
//!   address and length, tokenizers by [`Tokenizer::cache_identity`].
//! - Token totals stay `usize` on the tokenizer side and become `u64` where
//!   they meet the context window; the reference uses `number` throughout.
//! - [`CompactionSettings::threshold_percent`] is a fraction of the window
//!   (0.0–1.0). The settings UI stores 0–100, so hosts divide by 100.
//! - [`resolve_threshold_tokens`] / [`effective_reserve_tokens`] fallbacks are
//!   local policy: the reference implementation lives in the unavailable
//!   `@oh-my-pi/pi-agent-core/compaction` package. The defaults are documented
//!   on [`DEFAULT_THRESHOLD_NUMERATOR`] and [`MAX_RESERVE_TOKENS`].
//! - [`SystemPromptSource`] `start`/`end` keep the reference's UTF-16 code-unit
//!   semantics; conversion to byte offsets happens at slice time in
//!   [`compute_non_message_breakdown`] via [`utf16_range_to_bytes`].
//! - `systemPrompt` parts are plain `String`s, so
//!   [`claim_prompt_sources`] takes `&[&str]` rather than `(string | undefined)[]`.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::{Map, Value};

/// Shared empty system-prompt part list, avoiding an allocation per render.
pub static EMPTY_STRING_PARTS: &[&str] = &[];

/// Default auto-compaction threshold numerator (together with
/// [`DEFAULT_THRESHOLD_DENOMINATOR`]): 8/10 of the context window.
pub const DEFAULT_THRESHOLD_NUMERATOR: u64 = 8;
/// Default auto-compaction threshold denominator.
pub const DEFAULT_THRESHOLD_DENOMINATOR: u64 = 10;
/// Default compaction reserve is this fraction of the window…
pub const RESERVE_WINDOW_DIVISOR: u64 = 10;
/// …capped at this many tokens.
pub const MAX_RESERVE_TOKENS: u64 = 20_000;

// ---------------------------------------------------------------------------
// Minimal input types (stand-ins for `@oh-my-pi/*`)
// ---------------------------------------------------------------------------

/// One message, reduced to what token counting needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextMessage {
	/// Message role (`user`, `assistant`, `tool`, …).
	pub role: String,
	/// Message text as the provider sees it.
	pub text: String,
}

/// Counts provider-visible text.
///
/// `cache_identity` stands in for the reference's tokenizer-instance keying:
/// implementations return a stable value per tokenizer instance (typically
/// `self as *const Self as usize`). The agent swaps its tokenizer when the
/// model's encoding changes, so instance identity doubles as the encoding key.
pub trait Tokenizer {
	/// Tokens in the given wire strings, tokenized as separate fragments.
	fn count_tokens(&self, texts: &[&str]) -> usize;
	/// Tokens in the given messages.
	fn count_messages(&self, messages: &[ContextMessage]) -> usize;
	/// Stable per-instance cache key.
	fn cache_identity(&self) -> usize;
}

/// Model facts the breakdown needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
	/// Context window in tokens; `0` when unknown.
	pub context_window: u64,
	/// Whether tool schemas occupy context on every request.
	pub tool_schemas_in_context: bool,
}

impl Default for Model {
	fn default() -> Self {
		Self {
			context_window: 0,
			tool_schemas_in_context: true,
		}
	}
}

/// Compaction strategy categories.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompactionStrategy {
	/// Compaction disabled by strategy.
	Off,
	/// Instant local methods (snapcompact/shake): no speculation.
	Local,
	/// Model-summary compaction that can be speculated.
	#[default]
	Remote,
}

/// Auto-compaction settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompactionSettings {
	/// Compaction master switch.
	pub enabled: bool,
	/// Selected strategy.
	pub strategy: CompactionStrategy,
	/// Explicit threshold in tokens (overrides the percent).
	pub threshold_tokens: Option<u64>,
	/// Threshold as a fraction of the context window (0.0–1.0).
	pub threshold_percent: Option<f64>,
	/// Explicit reserve in tokens (overrides the default floor).
	pub reserve_tokens: Option<u64>,
}

impl Default for CompactionSettings {
	fn default() -> Self {
		Self {
			enabled: true,
			strategy: CompactionStrategy::Remote,
			threshold_tokens: None,
			threshold_percent: None,
			reserve_tokens: None,
		}
	}
}

/// Tokens at which auto-compaction fires for `window`, or `0` when off.
pub fn resolve_threshold_tokens(window: u64, settings: &CompactionSettings) -> u64 {
	if settings.strategy == CompactionStrategy::Off {
		return 0;
	}
	if let Some(tokens) = settings.threshold_tokens {
		return tokens;
	}
	if let Some(percent) = settings.threshold_percent {
		let tokens = percent * window as f64;
		return if tokens > 0.0 { tokens as u64 } else { 0 };
	}
	window.saturating_mul(DEFAULT_THRESHOLD_NUMERATOR) / DEFAULT_THRESHOLD_DENOMINATOR
}

/// Tokens held back for compaction work, for display when nothing else applies.
pub fn effective_reserve_tokens(window: u64, settings: &CompactionSettings) -> u64 {
	settings
		.reserve_tokens
		.unwrap_or_else(|| (window / RESERVE_WINDOW_DIVISOR).min(MAX_RESERVE_TOKENS))
}

/// One tool definition, reduced to its provider-visible fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextTool {
	/// Tool name.
	pub name: String,
	/// Wire description; `None` when the tool has none.
	pub description: Option<String>,
	/// JSON schema of the parameters.
	pub parameters: Value,
	/// Optional examples carried next to the schema.
	pub examples: Option<Value>,
}

/// Provider-visible JSON of one tool definition, in wire order.
pub fn tool_wire_schema(tool: &ContextTool) -> Value {
	let mut wire = Map::new();
	wire.insert("name".to_string(), Value::String(tool.name.clone()));
	if let Some(description) = &tool.description {
		wire.insert("description".to_string(), Value::String(description.clone()));
	}
	wire.insert("parameters".to_string(), tool.parameters.clone());
	if let Some(examples) = &tool.examples {
		wire.insert("examples".to_string(), examples.clone());
	}
	Value::Object(wire)
}

/// Recursively delete every `description` key from a JSON schema.
pub fn strip_schema_descriptions(schema: &mut Value) {
	match schema {
		Value::Object(map) => {
			map.remove("description");
			for value in map.values_mut() {
				strip_schema_descriptions(value);
			}
		}
		Value::Array(items) => {
			for item in items {
				strip_schema_descriptions(item);
			}
		}
		_ => {}
	}
}

/// Rendered examples block, or `None` when there is nothing to render.
///
/// The reference delegates to pi-ai's dialect renderer; this port emits the
/// examples array as compact JSON.
pub fn render_tool_examples(examples: Option<&Value>) -> Option<String> {
	let examples = examples?;
	if matches!(examples, Value::Array(items) if items.is_empty()) {
		return None;
	}
	serde_json::to_string(examples).ok()
}

// ---------------------------------------------------------------------------
// Compaction boundaries and breakdown shapes
// ---------------------------------------------------------------------------

/// Percent positions (0–100 of the context window) for the auto-compaction
/// boundaries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompactionBoundaries {
	/// Where auto-compaction fires.
	pub threshold_percent: f64,
	/// Where the background speculative summarizer starts, or `None` when no
	/// speculation will run.
	pub speculation_percent: Option<f64>,
}

/// Boundary positions for the status line's annotated context gauge. `None`
/// when compaction is disabled/off or the window is unknown.
pub fn compute_compaction_boundaries(
	settings: &CompactionSettings,
	context_window: u64,
	speculation_lead_tokens: Option<u64>,
) -> Option<CompactionBoundaries> {
	if context_window == 0 {
		return None;
	}
	if !settings.enabled || settings.strategy == CompactionStrategy::Off {
		return None;
	}
	let threshold_tokens = resolve_threshold_tokens(context_window, settings);
	if threshold_tokens == 0 || threshold_tokens > context_window {
		return None;
	}
	Some(CompactionBoundaries {
		threshold_percent: (threshold_tokens as f64 / context_window as f64) * 100.0,
		speculation_percent: speculation_lead_tokens.map(|lead| {
			(threshold_tokens.saturating_sub(lead) as f64 / context_window as f64) * 100.0
		}),
	})
}

/// Savings computed by the host's inline-image planner, not by the renderer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextSavingsEstimate {
	/// Model accepts images.
	pub vision_capable: bool,
	/// System-prompt image savings.
	pub system_prompt: Option<SystemPromptSavings>,
	/// Tool-result image savings.
	pub tool_results: Option<ToolResultsSavings>,
	/// Total estimated savings.
	pub saved_tokens: u64,
}

/// Why system-prompt image substitution did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptSavingsReason {
	/// Prompt was empty.
	Empty,
	/// No margin left.
	Margin,
	/// Budget exhausted.
	Budget,
}

/// Which prompt blocks may carry inline images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemPromptSavingsScope {
	/// Only the AGENTS.md block.
	AgentsMd,
	/// All system-prompt blocks.
	All,
}

/// System-prompt image savings.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemPromptSavings {
	/// Substitution applied.
	pub applied: bool,
	/// Why it did not apply.
	pub reason: Option<PromptSavingsReason>,
	/// Tokens of the original text.
	pub text_tokens: u64,
	/// Frames attached.
	pub frames: u64,
	/// Tokens of the frames.
	pub image_tokens: u64,
	/// Net tokens saved.
	pub saved_tokens: u64,
	/// Blocks eligible for substitution.
	pub scope: SystemPromptSavingsScope,
}

/// Tool-result image savings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolResultsSavings {
	/// Tool results seen.
	pub total: u64,
	/// Results swapped for images.
	pub swapped: u64,
	/// Tokens of the original text.
	pub text_tokens: u64,
	/// Frames attached.
	pub frames: u64,
	/// Tokens of the frames.
	pub image_tokens: u64,
	/// Net tokens saved.
	pub saved_tokens: u64,
}

/// Stable inputs used to cache non-message token estimates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NonMessageTokenSource {
	/// Active model.
	pub model: Option<Model>,
	/// System-prompt blocks.
	pub system_prompt: Vec<String>,
	/// Tool roster.
	pub tools: Vec<ContextTool>,
	/// Whether requests ship description-stripped specs because the catalog
	/// rides in the system prompt (`Agent.prunesToolDescriptions`).
	pub prunes_tool_descriptions: bool,
	/// Context files and skills located in the built prompt.
	pub system_prompt_sources: Option<SystemPromptSources>,
}

/// A context file or skill located in a built system-prompt block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemPromptSource {
	/// Stable source id.
	pub id: String,
	/// Source category.
	pub kind: SourceKind,
	/// Display label.
	pub label: String,
	/// Source path, when the source came from a file.
	pub path: Option<String>,
	/// Index of the built block holding this source.
	pub block: usize,
	/// UTF-16 start offset of the source in that block.
	pub start: usize,
	/// UTF-16 end offset of the source in that block.
	pub end: usize,
}

/// Source category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
	/// Context/instruction file.
	InstructionFiles,
	/// Skill description block.
	Skills,
}

/// Sources paired with the blocks they were built into; valid only while those
/// blocks are unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemPromptSources {
	/// Built blocks.
	pub blocks: Vec<String>,
	/// Sources located in those blocks.
	pub sources: Vec<SystemPromptSource>,
}

/// Attribution of built sources in an effective system prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptSourceClaims {
	/// Sources of each block the effective prompt holds unchanged at its built
	/// index, in block order and sorted by start offset.
	pub by_block: BTreeMap<usize, Vec<SystemPromptSource>>,
	/// Sources left in their block because it changed or moved after the build.
	pub left: Vec<SystemPromptSource>,
}

/// Attributes each source whose built block the effective prompt still holds
/// unchanged at the same index.
pub fn claim_prompt_sources(effective: &[&str], metadata: Option<&SystemPromptSources>) -> PromptSourceClaims {
	let mut by_block: BTreeMap<usize, Vec<SystemPromptSource>> = BTreeMap::new();
	let mut left = Vec::new();
	let Some(metadata) = metadata else {
		return PromptSourceClaims { by_block, left };
	};
	for source in &metadata.sources {
		let unchanged = match (effective.get(source.block), metadata.blocks.get(source.block)) {
			(Some(actual), Some(built)) => *actual == built.as_str(),
			_ => false,
		};
		if !unchanged {
			left.push(source.clone());
			continue;
		}
		by_block.entry(source.block).or_default().push(source.clone());
	}
	for sources in by_block.values_mut() {
		sources.sort_by_key(|source| source.start);
	}
	PromptSourceClaims { by_block, left }
}

/// Convert a UTF-16 code-unit range to byte offsets in `text`.
///
/// Offsets inside a surrogate pair clamp to the start of the character that
/// holds them; offsets past the end clamp to `text.len()`.
pub fn utf16_range_to_bytes(text: &str, start: usize, end: usize) -> (usize, usize) {
	let mut start_byte = None;
	let mut end_byte = None;
	let mut units = 0usize;
	for (index, ch) in text.char_indices() {
		if start_byte.is_none() && units >= start {
			start_byte = Some(index);
		}
		if end_byte.is_none() && units >= end {
			end_byte = Some(index);
			break;
		}
		units += ch.len_utf16();
	}
	let start_byte = start_byte.unwrap_or(text.len());
	let end_byte = end_byte.unwrap_or(text.len()).max(start_byte);
	(start_byte, end_byte)
}

// ---------------------------------------------------------------------------
// Tool schema fragments
// ---------------------------------------------------------------------------

/// Provider-visible text of one tool definition, in wire order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolSchemaFragments {
	/// Name, description, wire schema JSON, then rendered examples.
	pub fragments: Vec<String>,
	/// `fragments` with the schema JSON indented; present only when requested.
	pub display: Option<Vec<String>>,
	/// Why serialization stopped.
	pub schema_error: Option<String>,
}

/// How [`tool_schema_fragments`] renders one tool definition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolSchemaFragmentOptions {
	/// Also return `display`: the same fragments with the schema JSON indented
	/// by this many spaces.
	pub display_indent: Option<usize>,
	/// Mirror `normalizeTools({ pruneDescriptions: true })`: name plus the
	/// description-stripped schema, with no description or examples.
	pub prune_descriptions: bool,
}

/// Provider-visible text of one tool definition.
pub fn tool_schema_fragments(
	tool: &ContextTool,
	options: &ToolSchemaFragmentOptions,
) -> ToolSchemaFragments {
	let mut fragments: Vec<String> = Vec::new();
	let mut display: Option<Vec<String>> = options.display_indent.map(|_| Vec::new());

	fn push(
		fragments: &mut Vec<String>,
		display: &mut Option<Vec<String>>,
		wire: String,
		shown: Option<String>,
	) {
		match display.as_mut() {
			Some(display) => {
				let shown = shown.unwrap_or_else(|| wire.clone());
				display.push(shown);
				fragments.push(wire);
			}
			None => fragments.push(wire),
		}
	}

	push(
		&mut fragments,
		&mut display,
		tool.name.clone(),
		None,
	);
	if !options.prune_descriptions {
		if let Some(description) = &tool.description {
			push(
				&mut fragments,
				&mut display,
				description.clone(),
				None,
			);
		}
	}
	let mut wire_schema = tool_wire_schema(tool);
	if options.prune_descriptions {
		strip_schema_descriptions(&mut wire_schema);
	}
	match to_json(&wire_schema, options.display_indent) {
		Ok(wire) => {
			let shown = match (display.as_ref(), options.display_indent) {
				(Some(_), Some(indent)) => to_json(&wire_schema, Some(indent)).ok(),
				_ => None,
			};
			push(&mut fragments, &mut display, wire, shown);
			if !options.prune_descriptions {
				if let Some(examples) = render_tool_examples(tool.examples.as_ref()) {
					push(&mut fragments, &mut display, examples, None);
				}
			}
		}
		Err(error) => {
			return ToolSchemaFragments {
				fragments,
				display,
				schema_error: Some(error.to_string()),
			};
		}
	}
	ToolSchemaFragments {
		fragments,
		display,
		schema_error: None,
	}
}

/// Serialize a schema, indenting with spaces when `indent` is set.
fn to_json(value: &Value, indent: Option<usize>) -> Result<String, serde_json::Error> {
	match indent {
		None | Some(0) => serde_json::to_string(value),
		Some(width) => {
			let pad = vec![b' '; width];
			let formatter = serde_json::ser::PrettyFormatter::with_indent(&pad);
			let mut buffer = Vec::new();
			let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
			value.serialize(&mut serializer)?;
			String::from_utf8(buffer).map_err(|error| {
				serde_json::Error::io(std::io::Error::new(
					std::io::ErrorKind::InvalidData,
					error,
				))
			})
		}
	}
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Identity of a caller-owned slice: address plus length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SliceKey {
	ptr: usize,
	len: usize,
}

impl SliceKey {
	fn of<T>(slice: &[T]) -> Self {
		Self {
			ptr: slice.as_ptr() as usize,
			len: slice.len(),
		}
	}
}

#[derive(Clone, Copy, Debug)]
struct CachedToolEstimate {
	tokenizer: usize,
	revision: u64,
	source_revision: u64,
	prune_descriptions: bool,
	tokens: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct RosterCacheEntry {
	revision: u64,
	estimate: Option<CachedToolEstimate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NonMessageKey {
	system_prompt: SliceKey,
	prompt_sources: Option<usize>,
	tools: SliceKey,
	tool_schemas_in_context: bool,
	prune_tool_descriptions: bool,
	tools_revision: u64,
	source_revision: u64,
	tokenizer: usize,
}

#[derive(Clone, Copy, Debug)]
struct NonMessageCacheEntry {
	key: NonMessageKey,
	tokens: Option<usize>,
	breakdown: Option<NonMessageBreakdown>,
}

/// Per-roster metadata revisions, token estimates, and the non-message cache.
///
/// The reference keeps these in module-level `WeakMap`s and a symbol-keyed
/// session slot. Callers here own one cache per session view and may drop it
/// with that view; a dropped tokenizer or roster address is harmless because a
/// fresh cache starts empty.
#[derive(Debug, Default)]
pub struct ContextUsageCache {
	rosters: HashMap<SliceKey, RosterCacheEntry>,
	non_message: Option<NonMessageCacheEntry>,
}

/// Current dynamic metadata revision for one tool roster.
pub fn get_tool_schema_metadata_revision(cache: &ContextUsageCache, tools: &[ContextTool]) -> u64 {
	cache
		.rosters
		.get(&SliceKey::of(tools))
		.map_or(0, |entry| entry.revision)
}

/// Invalidate token estimates after a live tool description or parameter schema
/// can change without replacing the tools array.
pub fn invalidate_tool_schema_metadata(cache: &mut ContextUsageCache, tools: &[ContextTool]) {
	let entry = cache.rosters.entry(SliceKey::of(tools)).or_default();
	entry.revision = entry.revision.wrapping_add(1);
	entry.estimate = None;
}

/// Inputs that select which tool-schema estimate applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolSchemaEstimateOptions {
	/// Caller settings/source revision; a change invalidates the cached estimate.
	pub source_revision: u64,
	/// Count the description-stripped specs sent when the catalog rides in the
	/// system prompt.
	pub prune_descriptions: bool,
}

/// Estimate provider-visible tool-schema tokens.
pub fn estimate_tool_schema_tokens(
	cache: &mut ContextUsageCache,
	tools: &[ContextTool],
	tokenizer: &dyn Tokenizer,
	options: &ToolSchemaEstimateOptions,
) -> usize {
	let key = SliceKey::of(tools);
	let roster = cache.rosters.get(&key).copied().unwrap_or_default();
	let tokenizer_id = tokenizer.cache_identity();
	if let Some(cached) = roster.estimate {
		if cached.tokenizer == tokenizer_id
			&& cached.revision == roster.revision
			&& cached.source_revision == options.source_revision
			&& cached.prune_descriptions == options.prune_descriptions
		{
			return cached.tokens;
		}
	}
	let fragment_options = ToolSchemaFragmentOptions {
		display_indent: None,
		prune_descriptions: options.prune_descriptions,
	};
	let mut fragments: Vec<String> = Vec::new();
	for tool in tools {
		fragments.extend(tool_schema_fragments(tool, &fragment_options).fragments);
	}
	let texts: Vec<&str> = fragments.iter().map(String::as_str).collect();
	let tokens = tokenizer.count_tokens(&texts);
	cache.rosters.entry(key).or_default().estimate = Some(CachedToolEstimate {
		tokenizer: tokenizer_id,
		revision: roster.revision,
		source_revision: options.source_revision,
		prune_descriptions: options.prune_descriptions,
		tokens,
	});
	tokens
}

/// Count schemas carried on every request, excluding server-side deferred
/// catalogs.
pub fn estimate_context_tool_schema_tokens(
	cache: &mut ContextUsageCache,
	tools: &[ContextTool],
	tokenizer: &dyn Tokenizer,
	model: Option<&Model>,
	options: &ToolSchemaEstimateOptions,
) -> usize {
	if let Some(model) = model {
		if !model.tool_schemas_in_context {
			return 0;
		}
	}
	estimate_tool_schema_tokens(cache, tools, tokenizer, options)
}

fn non_message_key(
	cache: &ContextUsageCache,
	session: &NonMessageTokenSource,
	tokenizer: &dyn Tokenizer,
	source_revision: u64,
) -> NonMessageKey {
	NonMessageKey {
		system_prompt: SliceKey::of(&session.system_prompt),
		prompt_sources: session
			.system_prompt_sources
			.as_ref()
			.map(|sources| sources as *const SystemPromptSources as usize),
		tools: SliceKey::of(&session.tools),
		tool_schemas_in_context: session
			.model
			.map_or(true, |model| model.tool_schemas_in_context),
		prune_tool_descriptions: session.prunes_tool_descriptions,
		tools_revision: get_tool_schema_metadata_revision(cache, &session.tools),
		source_revision,
		tokenizer: tokenizer.cache_identity(),
	}
}

fn cached_non_message(
	cache: &ContextUsageCache,
	key: NonMessageKey,
) -> Option<NonMessageCacheEntry> {
	cache.non_message.filter(|entry| entry.key == key)
}

fn store_non_message(
	cache: &mut ContextUsageCache,
	key: NonMessageKey,
	previous: Option<NonMessageCacheEntry>,
	tokens: Option<usize>,
	breakdown: Option<NonMessageBreakdown>,
) {
	let previous = previous.unwrap_or(NonMessageCacheEntry {
		key,
		tokens: None,
		breakdown: None,
	});
	cache.non_message = Some(NonMessageCacheEntry {
		key,
		tokens: tokens.or(previous.tokens),
		breakdown: breakdown.or(previous.breakdown),
	});
}

/// Non-message token totals, with instruction files and skills counted through
/// [`claim_prompt_sources`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NonMessageBreakdown {
	/// Tokens inside skill sources.
	pub skills_tokens: usize,
	/// Tokens inside instruction-file sources.
	pub instruction_files_tokens: usize,
	/// Tokens in provider-visible tool schemas.
	pub tools_tokens: usize,
	/// Blocks after the first, without attributed sources.
	pub system_context_tokens: usize,
	/// The first block, without attributed sources.
	pub system_prompt_tokens: usize,
}

/// Compute just the NON-MESSAGE token total: the system prompt as a whole plus
/// tool schemas.
pub fn compute_non_message_tokens(
	cache: &mut ContextUsageCache,
	session: &NonMessageTokenSource,
	tokenizer: &dyn Tokenizer,
	source_revision: u64,
) -> usize {
	let key = non_message_key(cache, session, tokenizer, source_revision);
	let previous = cached_non_message(cache, key);
	if let Some(tokens) = previous.and_then(|entry| entry.tokens) {
		return tokens;
	}
	let parts: Vec<&str> = session.system_prompt.iter().map(String::as_str).collect();
	let tokens = tokenizer.count_tokens(&parts)
		+ estimate_context_tool_schema_tokens(
			cache,
			&session.tools,
			tokenizer,
			session.model.as_ref(),
			&ToolSchemaEstimateOptions {
				source_revision,
				prune_descriptions: key.prune_tool_descriptions,
			},
		);
	store_non_message(cache, key, previous, Some(tokens), None);
	tokens
}

/// Shared helper for the four non-message token totals used by
/// [`compute_context_breakdown`]. Keep this category split stable: the
/// status-line fast path intentionally uses the equivalent collapsed total in
/// [`compute_non_message_tokens`].
pub fn compute_non_message_breakdown(
	cache: &mut ContextUsageCache,
	session: &NonMessageTokenSource,
	tokenizer: &dyn Tokenizer,
	source_revision: u64,
) -> NonMessageBreakdown {
	let key = non_message_key(cache, session, tokenizer, source_revision);
	let previous = cached_non_message(cache, key);
	if let Some(breakdown) = previous.and_then(|entry| entry.breakdown) {
		return breakdown;
	}
	let tools_tokens = estimate_context_tool_schema_tokens(
		cache,
		&session.tools,
		tokenizer,
		session.model.as_ref(),
		&ToolSchemaEstimateOptions {
			source_revision,
			prune_descriptions: key.prune_tool_descriptions,
		},
	);
	let parts: Vec<&str> = session.system_prompt.iter().map(String::as_str).collect();
	let claims = claim_prompt_sources(&parts, session.system_prompt_sources.as_ref());
	let mut skills_tokens = 0;
	let mut instruction_files_tokens = 0;
	let mut system_context_tokens = 0;
	let mut system_prompt_tokens = 0;
	for (index, block) in parts.iter().enumerate() {
		let mut remaining = String::new();
		let mut cursor = 0usize;
		if let Some(sources) = claims.by_block.get(&index) {
			for source in sources {
				let (start, end) = utf16_range_to_bytes(block, source.start, source.end);
				let start = start.max(cursor).min(block.len());
				let end = end.max(start).min(block.len());
				remaining.push_str(&block[cursor..start]);
				let tokens = tokenizer.count_tokens(&[&block[start..end]]);
				match source.kind {
					SourceKind::Skills => skills_tokens += tokens,
					SourceKind::InstructionFiles => instruction_files_tokens += tokens,
				}
				cursor = end;
			}
		}
		let tokens = if cursor == 0 {
			tokenizer.count_tokens(&[*block])
		} else {
			let rest = format!("{remaining}{}", &block[cursor..]);
			tokenizer.count_tokens(&[rest.as_str()])
		};
		if index == 0 {
			system_prompt_tokens = tokens;
		} else {
			system_context_tokens += tokens;
		}
	}
	let breakdown = NonMessageBreakdown {
		skills_tokens,
		instruction_files_tokens,
		tools_tokens,
		system_context_tokens,
		system_prompt_tokens,
	};
	store_non_message(cache, key, previous, None, Some(breakdown));
	breakdown
}

// ---------------------------------------------------------------------------
// Full breakdown
// ---------------------------------------------------------------------------

/// Context categories in the breakdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CategoryId {
	/// First system-prompt block without attributed sources.
	SystemPrompt,
	/// Later system-prompt blocks without attributed sources.
	SystemContext,
	/// Tool schemas.
	SystemTools,
	/// Instruction-file sources.
	InstructionFiles,
	/// Skill sources.
	Skills,
	/// Conversation messages.
	Messages,
}

impl CategoryId {
	/// Display label.
	pub const fn label(self) -> &'static str {
		match self {
			Self::SystemPrompt => "System prompt",
			Self::SystemContext => "System context",
			Self::SystemTools => "System tools",
			Self::InstructionFiles => "Instruction files",
			Self::Skills => "Skills",
			Self::Messages => "Messages",
		}
	}
}

/// One category row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryInfo {
	/// Category.
	pub id: CategoryId,
	/// Display label.
	pub label: &'static str,
	/// Estimated tokens.
	pub tokens: u64,
}

/// Estimated context usage by category for the active session and model.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextBreakdown {
	/// Active model.
	pub model: Option<Model>,
	/// Context window in tokens; `0` when unknown.
	pub context_window: u64,
	/// Category rows in display order.
	pub categories: Vec<CategoryInfo>,
	/// Sum of every category.
	pub used_tokens: u64,
	/// Room reserved for compaction.
	pub auto_compact_buffer_tokens: u64,
	/// Tokens still free before the window is full.
	pub free_tokens: u64,
	/// Where auto-compaction fires; `None` when compaction is off.
	pub threshold_tokens: Option<u64>,
	/// Estimated snapcompact wire savings, when requested.
	pub snapcompact: Option<ContextSavingsEstimate>,
}

/// Token counts a session can supply directly, skipping the non-message walk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextUsageSnapshot {
	/// Message tokens.
	pub messages_tokens: usize,
	/// Skill tokens.
	pub skills_tokens: usize,
	/// Instruction-file tokens.
	pub instruction_files_tokens: usize,
	/// Tool-schema tokens.
	pub system_tools_tokens: usize,
	/// Later system-prompt block tokens.
	pub system_context_tokens: usize,
	/// First system-prompt block tokens.
	pub system_prompt_tokens: usize,
	/// Total context tokens.
	pub used_tokens: usize,
}

/// The session view the breakdown reads.
pub trait ContextUsageSession {
	/// Tokenizer for this session's model encoding.
	fn tokenizer(&self) -> &dyn Tokenizer;
	/// Non-message inputs (model, prompt, roster, sources).
	fn non_message_source(&self) -> &NonMessageTokenSource;
	/// Transcript.
	fn messages(&self) -> &[ContextMessage] {
		&[]
	}
	/// Pre-computed counts, when the host already has them.
	fn context_breakdown(&self) -> Option<ContextUsageSnapshot> {
		None
	}
}

/// Inputs for [`compute_context_breakdown`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextUsageOptions {
	/// Compaction settings.
	pub compaction: CompactionSettings,
	/// Caller settings/source revision.
	pub source_revision: u64,
	/// Snapcompact savings to surface with the breakdown.
	pub snapcompact: Option<ContextSavingsEstimate>,
}

/// Compute a breakdown of estimated context usage by category for the active
/// session and model.
pub fn compute_context_breakdown(
	cache: &mut ContextUsageCache,
	session: &dyn ContextUsageSession,
	options: &ContextUsageOptions,
) -> ContextBreakdown {
	let source = session.non_message_source();
	let model = source.model;
	let tokenizer = session.tokenizer();
	let context_window = model.map_or(0, |model| model.context_window);

	let counts = match session.context_breakdown() {
		Some(snapshot) => snapshot,
		None => {
			let messages_tokens = tokenizer.count_messages(session.messages());
			let non_message =
				compute_non_message_breakdown(cache, source, tokenizer, options.source_revision);
			let used_tokens = non_message.skills_tokens
				+ non_message.instruction_files_tokens
				+ non_message.tools_tokens
				+ non_message.system_context_tokens
				+ non_message.system_prompt_tokens
				+ messages_tokens;
			ContextUsageSnapshot {
				messages_tokens,
				skills_tokens: non_message.skills_tokens,
				instruction_files_tokens: non_message.instruction_files_tokens,
				system_tools_tokens: non_message.tools_tokens,
				system_context_tokens: non_message.system_context_tokens,
				system_prompt_tokens: non_message.system_prompt_tokens,
				used_tokens,
			}
		}
	};

	let used_tokens = counts.used_tokens as u64;
	let categories = vec![
		CategoryInfo {
			id: CategoryId::SystemPrompt,
			label: CategoryId::SystemPrompt.label(),
			tokens: counts.system_prompt_tokens as u64,
		},
		CategoryInfo {
			id: CategoryId::SystemTools,
			label: CategoryId::SystemTools.label(),
			tokens: counts.system_tools_tokens as u64,
		},
		CategoryInfo {
			id: CategoryId::SystemContext,
			label: CategoryId::SystemContext.label(),
			tokens: counts.system_context_tokens as u64,
		},
		CategoryInfo {
			id: CategoryId::InstructionFiles,
			label: CategoryId::InstructionFiles.label(),
			tokens: counts.instruction_files_tokens as u64,
		},
		CategoryInfo {
			id: CategoryId::Skills,
			label: CategoryId::Skills.label(),
			tokens: counts.skills_tokens as u64,
		},
		CategoryInfo {
			id: CategoryId::Messages,
			label: CategoryId::Messages.label(),
			tokens: counts.messages_tokens as u64,
		},
	];

	let mut auto_compact_buffer_tokens = 0u64;
	let mut threshold_tokens = None;
	if context_window > 0 {
		let settings = &options.compaction;
		if settings.enabled && settings.strategy != CompactionStrategy::Off {
			let threshold = resolve_threshold_tokens(context_window, settings);
			if threshold > 0 && threshold <= context_window {
				threshold_tokens = Some(threshold);
			}
			auto_compact_buffer_tokens = context_window.saturating_sub(threshold);
		} else {
			auto_compact_buffer_tokens = 0;
		}
		// Even when fully disabled, fall back to a sensible reserve floor for display.
		if auto_compact_buffer_tokens == 0 && settings.enabled {
			auto_compact_buffer_tokens = effective_reserve_tokens(context_window, settings);
		}
	}
	auto_compact_buffer_tokens =
		auto_compact_buffer_tokens.min(context_window.saturating_sub(used_tokens));

	let free_tokens = context_window
		.saturating_sub(used_tokens)
		.saturating_sub(auto_compact_buffer_tokens);

	ContextBreakdown {
		model,
		context_window,
		categories,
		used_tokens,
		auto_compact_buffer_tokens,
		free_tokens,
		threshold_tokens,
		snapcompact: options.snapcompact.clone(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	use std::cell::Cell;

	#[derive(Default)]
	struct WordTokenizer {
		calls: Cell<usize>,
	}

	impl Tokenizer for WordTokenizer {
		fn count_tokens(&self, texts: &[&str]) -> usize {
			self.calls.set(self.calls.get() + 1);
			texts
				.iter()
				.map(|text| text.split_whitespace().count())
				.sum()
		}

		fn count_messages(&self, messages: &[ContextMessage]) -> usize {
			messages
				.iter()
				.map(|message| message.text.split_whitespace().count())
				.sum()
		}

		fn cache_identity(&self) -> usize {
			self as *const Self as usize
		}
	}

	fn source(id: &str, kind: SourceKind, block: usize, start: usize, end: usize) -> SystemPromptSource {
		SystemPromptSource {
			id: id.to_string(),
			kind,
			label: id.to_string(),
			path: None,
			block,
			start,
			end,
		}
	}

	fn block_with_sources() -> (Vec<String>, SystemPromptSources) {
		let blocks = vec!["intro alpha beta".to_string(), "discussion gamma".to_string()];
		let sources = SystemPromptSources {
			blocks: blocks.clone(),
			sources: vec![
				source("skill", SourceKind::Skills, 1, 11, 16),
				source("files", SourceKind::InstructionFiles, 0, 6, 16),
			],
		};
		(blocks, sources)
	}

	fn tool(name: &str, description: Option<&str>) -> ContextTool {
		ContextTool {
			name: name.to_string(),
			description: description.map(str::to_string),
			parameters: json!({"type": "object"}),
			examples: None,
		}
	}

	#[test]
	fn claims_unchanged_blocks_and_leaves_changed_or_moved_sources() {
		let metadata = SystemPromptSources {
			blocks: vec!["first".to_string(), "second".to_string()],
			sources: vec![
				source("late", SourceKind::Skills, 0, 4, 6),
				source("early", SourceKind::InstructionFiles, 0, 0, 2),
				source("moved", SourceKind::Skills, 1, 0, 3),
				source("changed", SourceKind::Skills, 2, 0, 1),
			],
		};
		let effective = ["first", "SECOND"];
		let claims = claim_prompt_sources(&effective, Some(&metadata));
		let claimed = claims.by_block.get(&0).expect("block 0 claimed");
		assert_eq!(
			claimed.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(),
			vec!["early", "late"]
		);
		assert!(!claims.by_block.contains_key(&1));
		let left: Vec<&str> = claims.left.iter().map(|entry| entry.id.as_str()).collect();
		assert_eq!(left, vec!["moved", "changed"]);
	}

	#[test]
	fn claims_without_metadata_are_empty() {
		let claims = claim_prompt_sources(&["block"], None);
		assert!(claims.by_block.is_empty());
		assert!(claims.left.is_empty());
	}

	#[test]
	fn pruned_fragments_drop_description_and_examples() {
		let mut tool = tool("grep", Some("Search files"));
		tool.parameters = json!({
			"type": "object",
			"properties": {"q": {"type": "string", "description": "query"}}
		});
		tool.examples = Some(json!([{"q": "needle"}]));
		let options = ToolSchemaFragmentOptions {
			display_indent: None,
			prune_descriptions: false,
		};
		let full = tool_schema_fragments(&tool, &options);
		assert_eq!(full.schema_error, None);
		assert_eq!(full.fragments[0], "grep");
		assert_eq!(full.fragments[1], "Search files");
		assert_eq!(full.fragments.len(), 4);
		assert!(full.fragments[2].contains("\"description\":\"Search files\""));
		assert_eq!(full.fragments[3], "[{\"q\":\"needle\"}]");

		let pruned = tool_schema_fragments(
			&tool,
			&ToolSchemaFragmentOptions {
				display_indent: None,
				prune_descriptions: true,
			},
		);
		assert_eq!(pruned.fragments.len(), 2);
		assert_eq!(pruned.fragments[0], "grep");
		assert!(!pruned.fragments[1].contains("description"));
		assert!(!pruned.fragments[1].contains("needle"));
	}

	#[test]
	fn display_indent_indents_schema_json() {
		let tool = tool("read", Some("Read a file"));
		let fragments = tool_schema_fragments(
			&tool,
			&ToolSchemaFragmentOptions {
				display_indent: Some(2),
				prune_descriptions: false,
			},
		);
		let display = fragments.display.expect("display requested");
		assert_eq!(display.len(), fragments.fragments.len());
		assert!(!fragments.fragments[2].contains('\n'));
		assert!(display[2].contains("\n  \"name\""));
	}

	#[test]
	fn token_estimate_is_cached_until_revision_advances() {
		let mut tools = vec![tool("grep", Some("one"))];
		let tokenizer = WordTokenizer::default();
		let mut cache = ContextUsageCache::default();
		let options = ToolSchemaEstimateOptions::default();

		let first = estimate_tool_schema_tokens(&mut cache, &tools, &tokenizer, &options);
		tools[0].description = Some("one two three".to_string());
		let cached = estimate_tool_schema_tokens(&mut cache, &tools, &tokenizer, &options);
		assert_eq!(cached, first);
		let calls = tokenizer.calls.get();
		estimate_tool_schema_tokens(&mut cache, &tools, &tokenizer, &options);
		assert_eq!(tokenizer.calls.get(), calls, "cache hit must not recount");

		invalidate_tool_schema_metadata(&mut cache, &tools);
		assert_eq!(get_tool_schema_metadata_revision(&cache, &tools), 1);
		let refreshed = estimate_tool_schema_tokens(&mut cache, &tools, &tokenizer, &options);
		assert!(refreshed > first);

		let bumped = ToolSchemaEstimateOptions {
			source_revision: 2,
			prune_descriptions: false,
		};
		let recounted = estimate_tool_schema_tokens(&mut cache, &tools, &tokenizer, &bumped);
		assert_eq!(recounted, refreshed);
		assert!(tokenizer.calls.get() > calls);
	}

	#[test]
	fn breakdown_splits_skill_and_instruction_slices() {
		let (blocks, sources) = block_with_sources();
		let session = NonMessageTokenSource {
			model: Some(Model::default()),
			system_prompt: blocks,
			prunes_tool_descriptions: false,
			tools: Vec::new(),
			system_prompt_sources: Some(sources),
		};
		let tokenizer = WordTokenizer::default();
		let mut cache = ContextUsageCache::default();
		let breakdown = compute_non_message_breakdown(&mut cache, &session, &tokenizer, 0);
		assert_eq!(breakdown.instruction_files_tokens, 2);
		assert_eq!(breakdown.skills_tokens, 1);
		assert_eq!(breakdown.system_prompt_tokens, 1);
		assert_eq!(breakdown.system_context_tokens, 1);
		assert_eq!(breakdown.tools_tokens, 0);
		assert_eq!(
			compute_non_message_tokens(&mut cache, &session, &tokenizer, 0),
			5
		);
	}

	#[test]
	fn source_offsets_are_utf16_code_units() {
		let blocks = vec!["🦀 alpha".to_string()];
		let sources = SystemPromptSources {
			blocks: blocks.clone(),
			sources: vec![source("files", SourceKind::InstructionFiles, 0, 3, 8)],
		};
		assert_eq!(utf16_range_to_bytes(&blocks[0], 3, 8), (5, 10));
		let session = NonMessageTokenSource {
			model: None,
			system_prompt: blocks,
			prunes_tool_descriptions: false,
			tools: Vec::new(),
			system_prompt_sources: Some(sources),
		};
		let tokenizer = WordTokenizer::default();
		let mut cache = ContextUsageCache::default();
		let breakdown = compute_non_message_breakdown(&mut cache, &session, &tokenizer, 0);
		assert_eq!(breakdown.instruction_files_tokens, 1);
		assert_eq!(breakdown.system_prompt_tokens, 1);
	}

	struct View {
		tokenizer: WordTokenizer,
		source: NonMessageTokenSource,
		messages: Vec<ContextMessage>,
	}

	impl ContextUsageSession for View {
		fn tokenizer(&self) -> &dyn Tokenizer {
			&self.tokenizer
		}

		fn non_message_source(&self) -> &NonMessageTokenSource {
			&self.source
		}

		fn messages(&self) -> &[ContextMessage] {
			&self.messages
		}
	}

	fn view(context_window: u64) -> View {
		let (blocks, sources) = block_with_sources();
		View {
			tokenizer: WordTokenizer::default(),
			source: NonMessageTokenSource {
				model: Some(Model {
					context_window,
					tool_schemas_in_context: true,
				}),
				system_prompt: blocks,
				tools: Vec::new(),
				prunes_tool_descriptions: false,
				system_prompt_sources: Some(sources),
			},
			messages: vec![ContextMessage {
				role: "user".to_string(),
				text: "hello world".to_string(),
			}],
		}
	}

	#[test]
	fn breakdown_totals_and_buffer_math() {
		let session = view(1000);
		let mut cache = ContextUsageCache::default();
		let options = ContextUsageOptions {
			compaction: CompactionSettings {
				enabled: true,
				strategy: CompactionStrategy::Remote,
				threshold_percent: Some(0.8),
				..CompactionSettings::default()
			},
			source_revision: 0,
			snapcompact: None,
		};
		let breakdown = compute_context_breakdown(&mut cache, &session, &options);
		assert_eq!(breakdown.context_window, 1000);
		assert_eq!(breakdown.used_tokens, 7);
		assert_eq!(breakdown.threshold_tokens, Some(800));
		assert_eq!(breakdown.auto_compact_buffer_tokens, 200);
		assert_eq!(breakdown.free_tokens, 793);
		let total: u64 = breakdown.categories.iter().map(|c| c.tokens).sum();
		assert_eq!(total, breakdown.used_tokens);
		assert_eq!(
			breakdown
				.categories
				.iter()
				.map(|c| c.id)
				.collect::<Vec<_>>(),
			vec![
				CategoryId::SystemPrompt,
				CategoryId::SystemTools,
				CategoryId::SystemContext,
				CategoryId::InstructionFiles,
				CategoryId::Skills,
				CategoryId::Messages,
			]
		);
		assert_eq!(breakdown.categories[3].label, "Instruction files");

		let disabled = ContextUsageOptions {
			compaction: CompactionSettings {
				enabled: false,
				..CompactionSettings::default()
			},
			source_revision: 0,
			snapcompact: None,
		};
		let off = compute_context_breakdown(&mut cache, &session, &disabled);
		assert_eq!(off.threshold_tokens, None);
		assert_eq!(off.auto_compact_buffer_tokens, 0);
		assert_eq!(off.free_tokens, 993);
	}

	#[test]
	fn compaction_boundaries_report_percent_and_speculation() {
		let settings = CompactionSettings {
			enabled: true,
			strategy: CompactionStrategy::Remote,
			threshold_percent: Some(0.8),
			..CompactionSettings::default()
		};
		let boundaries = compute_compaction_boundaries(&settings, 1000, None).expect("boundaries");
		assert_eq!(boundaries.threshold_percent, 80.0);
		assert_eq!(boundaries.speculation_percent, None);
		let speculative = compute_compaction_boundaries(&settings, 1000, Some(100)).expect("boundaries");
		assert_eq!(speculative.speculation_percent, Some(70.0));

		assert_eq!(compute_compaction_boundaries(&settings, 0, None), None);
		assert_eq!(
			compute_compaction_boundaries(
				&CompactionSettings {
					enabled: false,
					..settings
				},
				1000,
				None
			),
			None
		);
		assert_eq!(
			compute_compaction_boundaries(
				&CompactionSettings {
					threshold_tokens: Some(2000),
					..settings
				},
				1000,
				None
			),
			None
		);
	}

	#[test]
	fn threshold_and_reserve_fall_back_to_local_policy() {
		let settings = CompactionSettings {
			enabled: true,
			strategy: CompactionStrategy::Remote,
			threshold_tokens: None,
			threshold_percent: None,
			reserve_tokens: None,
		};
		assert_eq!(resolve_threshold_tokens(1000, &settings), 800);
		assert_eq!(effective_reserve_tokens(1000, &settings), 100);
		assert_eq!(effective_reserve_tokens(1_000_000, &settings), MAX_RESERVE_TOKENS);

		let explicit = CompactionSettings {
			threshold_tokens: Some(640),
			reserve_tokens: Some(64),
			..settings
		};
		assert_eq!(resolve_threshold_tokens(1000, &explicit), 640);
		assert_eq!(effective_reserve_tokens(1000, &explicit), 64);
		assert_eq!(
			resolve_threshold_tokens(
				1000,
				&CompactionSettings {
					strategy: CompactionStrategy::Off,
					threshold_tokens: Some(640),
					..settings
				}
			),
			0
		);
	}
}
