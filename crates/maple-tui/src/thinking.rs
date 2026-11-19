//! Thinking-level selectors, parsing, metadata, and model clamping
//! (port of `l/src/thinking.ts`).
//!
//! Reference: `l/src/thinking.ts`.
//!
//! Deviations forced by Rust:
//!
//! - `Effort`, `ThinkingLevel`, and `ResolvedThinkingLevel` come from the
//!   `@oh-my-pi/pi-agent-core` / `pi-catalog` packages, which have no crate
//!   equivalent. They are defined here as enums with the same string values.
//! - `Model`, `getSupportedEfforts`, `clampThinkingLevelForModel`, and
//!   `modelsAreEqual` likewise come from `pi-catalog`. The port carries a
//!   minimal [`ThinkingModel`] exposing only the fields these helpers read
//!   (`provider`, `id`, `reasoning`, `thinking.default_level`,
//!   `thinking.unverified`, `thinking.efforts`) and local implementations of
//!   those helpers against `thinking.efforts`.
//! - `assertModelSupportsEffortCeiling` throws a `RangeError` in TypeScript;
//!   the Rust version returns `Result<(), String>`.
//! - `resolveTaskEffortLevel` can throw; the Rust version returns
//!   `Result<Option<Effort>, String>`.

/// Canonical effort ladder, low to high.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effort {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    /// Every effort in ladder order.
    pub const ALL: [Effort; 6] = [
        Effort::Minimal,
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];

    /// The provider-facing selector string.
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Minimal => "minimal",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }

    /// Position on the canonical ladder.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Agent-local thinking selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThinkingLevel {
    Inherit,
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ThinkingLevel {
    /// Every level in canonical order.
    pub const ALL: [ThinkingLevel; 8] = [
        ThinkingLevel::Inherit,
        ThinkingLevel::Off,
        ThinkingLevel::Minimal,
        ThinkingLevel::Low,
        ThinkingLevel::Medium,
        ThinkingLevel::High,
        ThinkingLevel::XHigh,
        ThinkingLevel::Max,
    ];

    /// The selector string.
    pub fn as_str(self) -> &'static str {
        match self {
            ThinkingLevel::Inherit => "inherit",
            ThinkingLevel::Off => "off",
            ThinkingLevel::Minimal => "minimal",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
            ThinkingLevel::Max => "max",
        }
    }

    /// The concrete effort this level names, if any.
    pub fn to_effort(self) -> Option<Effort> {
        match self {
            ThinkingLevel::Minimal => Some(Effort::Minimal),
            ThinkingLevel::Low => Some(Effort::Low),
            ThinkingLevel::Medium => Some(Effort::Medium),
            ThinkingLevel::High => Some(Effort::High),
            ThinkingLevel::XHigh => Some(Effort::XHigh),
            ThinkingLevel::Max => Some(Effort::Max),
            ThinkingLevel::Inherit | ThinkingLevel::Off => None,
        }
    }

    /// The level naming `effort`.
    pub fn from_effort(effort: Effort) -> Self {
        match effort {
            Effort::Minimal => ThinkingLevel::Minimal,
            Effort::Low => ThinkingLevel::Low,
            Effort::Medium => ThinkingLevel::Medium,
            Effort::High => ThinkingLevel::High,
            Effort::XHigh => ThinkingLevel::XHigh,
            Effort::Max => ThinkingLevel::Max,
        }
    }

    /// Ladder position of the named effort (`Inherit`/`Off` have none).
    fn effort_index(self) -> Option<usize> {
        self.to_effort().map(Effort::index)
    }
}

/// A thinking selector as configured by the user — a concrete level or `auto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfiguredThinkingLevel {
    Inherit,
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
    Auto,
}

impl ConfiguredThinkingLevel {
    /// The selector string.
    pub fn as_str(self) -> &'static str {
        match self {
            ConfiguredThinkingLevel::Inherit => "inherit",
            ConfiguredThinkingLevel::Off => "off",
            ConfiguredThinkingLevel::Minimal => "minimal",
            ConfiguredThinkingLevel::Low => "low",
            ConfiguredThinkingLevel::Medium => "medium",
            ConfiguredThinkingLevel::High => "high",
            ConfiguredThinkingLevel::XHigh => "xhigh",
            ConfiguredThinkingLevel::Max => "max",
            ConfiguredThinkingLevel::Auto => "auto",
        }
    }

    /// The concrete level, when this is not `auto`.
    pub fn concrete(self) -> Option<ThinkingLevel> {
        match self {
            ConfiguredThinkingLevel::Inherit => Some(ThinkingLevel::Inherit),
            ConfiguredThinkingLevel::Off => Some(ThinkingLevel::Off),
            ConfiguredThinkingLevel::Minimal => Some(ThinkingLevel::Minimal),
            ConfiguredThinkingLevel::Low => Some(ThinkingLevel::Low),
            ConfiguredThinkingLevel::Medium => Some(ThinkingLevel::Medium),
            ConfiguredThinkingLevel::High => Some(ThinkingLevel::High),
            ConfiguredThinkingLevel::XHigh => Some(ThinkingLevel::XHigh),
            ConfiguredThinkingLevel::Max => Some(ThinkingLevel::Max),
            ConfiguredThinkingLevel::Auto => None,
        }
    }
}

/// A resolved concrete level: `off`, or a provider effort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResolvedThinkingLevel {
    Off,
    Effort(Effort),
}

/// Thinking selectors accepted by CLI inputs, in display order.
pub const CLI_THINKING_LEVELS: [ConfiguredThinkingLevel; 8] = [
    ConfiguredThinkingLevel::Off,
    ConfiguredThinkingLevel::Minimal,
    ConfiguredThinkingLevel::Low,
    ConfiguredThinkingLevel::Medium,
    ConfiguredThinkingLevel::High,
    ConfiguredThinkingLevel::XHigh,
    ConfiguredThinkingLevel::Max,
    ConfiguredThinkingLevel::Auto,
];

/// Sentinel selector for the coding-agent "auto" thinking mode.
pub const AUTO_THINKING: ConfiguredThinkingLevel = ConfiguredThinkingLevel::Auto;

/// Metadata used to render thinking selector values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThinkingLevelMetadata {
    /// The level this metadata describes.
    pub value: ThinkingLevel,
    /// Short label.
    pub label: String,
    /// One-line description.
    pub description: String,
}

/// Metadata for the `auto` selector value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredThinkingLevelMetadata {
    /// The selector this metadata describes.
    pub value: ConfiguredThinkingLevel,
    /// Short label.
    pub label: String,
    /// One-line description.
    pub description: String,
}

/// The minimal model surface these helpers read (see module docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThinkingModel {
    /// Catalog provider name.
    pub provider: String,
    /// Catalog model id.
    pub id: String,
    /// Whether the model reasons; `None` when unknown.
    pub reasoning: Option<bool>,
    /// Thinking capabilities, when the catalog declares them.
    pub thinking: Option<ModelThinking>,
}

/// A model's declared thinking capabilities.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelThinking {
    /// The model's own default level.
    pub default_level: Option<Effort>,
    /// Thinking support is unverified; keep level unset until the user picks.
    pub unverified: bool,
    /// Efforts the model can be controlled at; empty means no effort surface.
    pub efforts: Vec<Effort>,
}

const EFFORT_BY_SELECTOR: [(&str, Effort); 6] = [
    ("minimal", Effort::Minimal),
    ("low", Effort::Low),
    ("medium", Effort::Medium),
    ("high", Effort::High),
    ("xhigh", Effort::XHigh),
    ("max", Effort::Max),
];

const THINKING_LEVEL_BY_SELECTOR: [(&str, ThinkingLevel); 8] = [
    ("inherit", ThinkingLevel::Inherit),
    ("off", ThinkingLevel::Off),
    ("minimal", ThinkingLevel::Minimal),
    ("low", ThinkingLevel::Low),
    ("medium", ThinkingLevel::Medium),
    ("high", ThinkingLevel::High),
    ("xhigh", ThinkingLevel::XHigh),
    ("max", ThinkingLevel::Max),
];

/// Exact selector match, else a unique prefix of at least two characters.
fn get_own_selector<T: Copy>(selectors: &[(&str, T)], value: Option<&str>) -> Option<T> {
    let value = value?;
    if let Some((_, selected)) = selectors.iter().find(|(key, _)| *key == value) {
        return Some(*selected);
    }
    if value.len() < 2 {
        return None;
    }
    let matches: Vec<(&str, T)> = selectors
        .iter()
        .filter(|(key, _)| key.starts_with(value))
        .copied()
        .collect();
    if matches.len() == 1 {
        Some(matches[0].1)
    } else {
        None
    }
}

/// Parse a provider-facing effort value; accepts unambiguous abbreviations.
pub fn parse_effort(value: Option<&str>) -> Option<Effort> {
    get_own_selector(&EFFORT_BY_SELECTOR, value)
}

/// Parse an agent-local thinking selector; accepts unambiguous abbreviations.
pub fn parse_thinking_level(value: Option<&str>) -> Option<ThinkingLevel> {
    get_own_selector(&THINKING_LEVEL_BY_SELECTOR, value)
}

/// Display metadata for a thinking selector.
pub fn get_thinking_level_metadata(level: ThinkingLevel) -> ThinkingLevelMetadata {
    let (label, description) = match level {
        ThinkingLevel::Inherit => ("inherit", "Inherit session default"),
        ThinkingLevel::Off => ("off", "No reasoning"),
        ThinkingLevel::Minimal => ("min", "Very brief reasoning (~1k tokens)"),
        ThinkingLevel::Low => ("low", "Light reasoning (~2k tokens)"),
        ThinkingLevel::Medium => ("medium", "Moderate reasoning (~8k tokens)"),
        ThinkingLevel::High => ("high", "Deep reasoning (~16k tokens)"),
        ThinkingLevel::XHigh => ("xhigh", "Extended reasoning (~32k tokens)"),
        ThinkingLevel::Max => ("max", "Maximum reasoning the model supports"),
    };
    ThinkingLevelMetadata {
        value: level,
        label: label.to_string(),
        description: description.to_string(),
    }
}

/// Convert an agent-local selector into the effort sent to providers.
pub fn to_reasoning_effort(level: Option<ThinkingLevel>) -> Option<Effort> {
    match level {
        None | Some(ThinkingLevel::Off) | Some(ThinkingLevel::Inherit) => None,
        Some(level) => level.to_effort(),
    }
}

/// True when a selector explicitly requests provider-side reasoning disablement.
pub fn should_disable_reasoning(level: Option<ThinkingLevel>) -> bool {
    level == Some(ThinkingLevel::Off)
}

/// Efforts a model can be controlled at; empty when it has no effort surface.
pub fn get_supported_efforts(model: &ThinkingModel) -> &[Effort] {
    model
        .thinking
        .as_ref()
        .map(|thinking| thinking.efforts.as_slice())
        .unwrap_or(&[])
}

/// Clamp a level to the model's supported ladder; unchanged when the model
/// declares no efforts.
fn clamp_thinking_level_for_model(
    model: Option<&ThinkingModel>,
    level: ThinkingLevel,
) -> ThinkingLevel {
    let Some(effort) = level.to_effort() else {
        return level;
    };
    let Some(model) = model else {
        return level;
    };
    let supported = get_supported_efforts(model);
    if supported.is_empty() {
        return level;
    }
    let requested = effort.index();
    supported
        .iter()
        .rev()
        .find(|candidate| candidate.index() <= requested)
        .or(supported.first())
        .map(|candidate| ThinkingLevel::from_effort(*candidate))
        .unwrap_or(level)
}

/// Resolve a selector against the current model while preserving explicit "off".
pub fn resolve_thinking_level_for_model(
    model: Option<&ThinkingModel>,
    level: Option<ThinkingLevel>,
) -> Option<ResolvedThinkingLevel> {
    match level {
        None | Some(ThinkingLevel::Inherit) => None,
        Some(ThinkingLevel::Off) => Some(ResolvedThinkingLevel::Off),
        Some(level) => {
            let clamped = clamp_thinking_level_for_model(model, level);
            Some(ResolvedThinkingLevel::Effort(
                clamped.to_effort().unwrap_or(Effort::High),
            ))
        }
    }
}

/// Level applied to `model` when the user has not picked one for it.
pub fn automatic_thinking_level<T>(
    model: Option<&ThinkingModel>,
    fallback: Option<T>,
) -> Option<T> {
    if model
        .and_then(|model| model.thinking.as_ref())
        .is_some_and(|thinking| thinking.unverified)
    {
        return None;
    }
    fallback
}

/// Maps the session-level `auto` sentinel to `None`; concrete levels pass through.
pub fn concrete_thinking_level(
    level: Option<ConfiguredThinkingLevel>,
) -> Option<ThinkingLevel> {
    level.and_then(ConfiguredThinkingLevel::concrete)
}

/// Selector a new session on `model` starts with.
pub fn new_session_thinking_level(
    model: Option<&ThinkingModel>,
    role_level: Option<ConfiguredThinkingLevel>,
    configured_default: Option<ConfiguredThinkingLevel>,
) -> Option<ConfiguredThinkingLevel> {
    if role_level.is_some() {
        return role_level;
    }
    if configured_default != Some(AUTO_THINKING) {
        if let Some(default) = model
            .and_then(|model| model.thinking.as_ref())
            .and_then(|thinking| thinking.default_level)
        {
            return Some(ConfiguredThinkingLevel::from(ThinkingLevel::from_effort(
                default,
            )));
        }
    }
    automatic_thinking_level(model, configured_default)
}

impl From<ThinkingLevel> for ConfiguredThinkingLevel {
    fn from(level: ThinkingLevel) -> Self {
        match level {
            ThinkingLevel::Inherit => ConfiguredThinkingLevel::Inherit,
            ThinkingLevel::Off => ConfiguredThinkingLevel::Off,
            ThinkingLevel::Minimal => ConfiguredThinkingLevel::Minimal,
            ThinkingLevel::Low => ConfiguredThinkingLevel::Low,
            ThinkingLevel::Medium => ConfiguredThinkingLevel::Medium,
            ThinkingLevel::High => ConfiguredThinkingLevel::High,
            ThinkingLevel::XHigh => ConfiguredThinkingLevel::XHigh,
            ThinkingLevel::Max => ConfiguredThinkingLevel::Max,
        }
    }
}

/// Concrete level `selector` runs at on `model`.
pub fn resolve_configured_thinking_level(
    model: Option<&ThinkingModel>,
    selector: Option<ConfiguredThinkingLevel>,
) -> Option<ResolvedThinkingLevel> {
    match selector {
        Some(ConfiguredThinkingLevel::Auto) => {
            resolve_provisional_auto_level(model).map(ResolvedThinkingLevel::Effort)
        }
        Some(selector) => resolve_thinking_level_for_model(model, selector.concrete()),
        None => resolve_thinking_level_for_model(model, None),
    }
}

/// True when two models are the same catalog entry.
fn models_are_equal(current: Option<&ThinkingModel>, target: &ThinkingModel) -> bool {
    current.is_some_and(|current| current.provider == target.provider && current.id == target.id)
}

/// True when a prewalk hand-off would change nothing observable.
pub fn prewalk_would_be_noop(
    current: Option<&ThinkingModel>,
    current_level: Option<ConfiguredThinkingLevel>,
    target: &ThinkingModel,
    target_level: Option<ConfiguredThinkingLevel>,
) -> bool {
    if !models_are_equal(current, target) {
        return false;
    }
    if target_level.is_none() {
        return true;
    }
    if (target_level == Some(AUTO_THINKING)) != (current_level == Some(AUTO_THINKING)) {
        return false;
    }
    resolve_thinking_level_for_model(
        Some(target),
        concrete_thinking_level(target_level),
    ) == resolve_thinking_level_for_model(Some(target), concrete_thinking_level(current_level))
}
/// Display metadata for a configured selector, including `auto`.
pub fn get_configured_thinking_level_metadata(
    level: ConfiguredThinkingLevel,
) -> ConfiguredThinkingLevelMetadata {
    if level == ConfiguredThinkingLevel::Auto {
        return ConfiguredThinkingLevelMetadata {
            value: level,
            label: "auto".to_string(),
            description: "Auto-detect per prompt".to_string(),
        };
    }
    let metadata = get_thinking_level_metadata(level.concrete().unwrap_or(ThinkingLevel::Inherit));
    ConfiguredThinkingLevelMetadata {
        value: level,
        label: metadata.label,
        description: metadata.description,
    }
}

/// Parse a configured thinking selector, accepting `auto` in addition to
/// every value [`parse_thinking_level`] accepts.
pub fn parse_configured_thinking_level(
    value: Option<&str>,
) -> Option<ConfiguredThinkingLevel> {
    if matches!(value, Some("auto")) {
        return Some(AUTO_THINKING);
    }
    parse_thinking_level(value).map(ConfiguredThinkingLevel::from)
}

/// Parse a `--thinking` CLI value; rejects `inherit`.
pub fn parse_cli_thinking_level(
    value: Option<&str>,
) -> Option<ConfiguredThinkingLevel> {
    let level = parse_configured_thinking_level(value)?;
    if level == ConfiguredThinkingLevel::Inherit {
        None
    } else {
        Some(level)
    }
}

/// Resolve an auto-classified effort against the model's supported range.
pub fn clamp_auto_thinking_effort(
    model: Option<&ThinkingModel>,
    effort: Effort,
    ceiling: Option<Effort>,
) -> Option<Effort> {
    let ceiling = ceiling.unwrap_or(Effort::Max);
    let supported = model.map_or(Effort::ALL.to_vec(), |model| {
        get_supported_efforts(model).to_vec()
    });
    if supported.is_empty() {
        return None;
    }
    let low_index = Effort::Low.index();
    let ceiling_index = ceiling.index();
    let at_or_above_low: Vec<Effort> = supported
        .iter()
        .filter(|level| level.index() >= low_index)
        .copied()
        .collect();
    let floored = if at_or_above_low.is_empty() {
        supported
    } else {
        at_or_above_low
    };
    let pool: Vec<Effort> = floored
        .iter()
        .filter(|level| level.index() <= ceiling_index)
        .copied()
        .collect();
    if pool.is_empty() {
        return None;
    }
    let requested_index = effort.index();
    let mut chosen = pool[0];
    for candidate in pool {
        if candidate.index() > requested_index {
            break;
        }
        chosen = candidate;
    }
    Some(chosen)
}

/// Coarse per-spawn effort selectors accepted by the task tool.
pub const TASK_EFFORTS: [&str; 3] = ["lo", "med", "hi"];

/// Coarse task-spawn effort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskEffort {
    Lo,
    Med,
    Hi,
}

impl TaskEffort {
    /// The selector string.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskEffort::Lo => "lo",
            TaskEffort::Med => "med",
            TaskEffort::Hi => "hi",
        }
    }
}

/// Map a coarse task effort onto the model's supported thinking range.
pub fn resolve_task_effort_level(
    model: Option<&ThinkingModel>,
    effort: TaskEffort,
    max_effort: Option<Effort>,
) -> Result<Option<Effort>, String> {
    let supported = model.map_or(Effort::ALL.to_vec(), |model| {
        get_supported_efforts(model).to_vec()
    });
    if supported.is_empty() {
        return Ok(None);
    }
    let resolved = match effort {
        TaskEffort::Lo => supported[0],
        TaskEffort::Med => supported[(supported.len() - 1) >> 1],
        TaskEffort::Hi => supported[supported.len() - 1],
    };
    let Some(max_effort) = max_effort else {
        return Ok(Some(resolved));
    };
    if let Some(model) = model {
        assert_model_supports_effort_ceiling(model, max_effort)?;
    }
    let max_index = max_effort.index();
    let ceiling = supported
        .iter()
        .rev()
        .find(|candidate| candidate.index() <= max_index)
        .copied()
        .unwrap_or(max_effort);
    Ok(Some(if resolved.index() > ceiling.index() {
        ceiling
    } else {
        resolved
    }))
}

/// Clamp a concrete thinking selector to a per-session effort ceiling.
pub fn clamp_thinking_level_to_ceiling(
    model: Option<&ThinkingModel>,
    level: Option<ThinkingLevel>,
    ceiling: Option<Effort>,
) -> Option<ThinkingLevel> {
    let level = level?;
    let ceiling = ceiling?;
    if level == ThinkingLevel::Off || level == ThinkingLevel::Inherit {
        return Some(level);
    }
    let max_index = ceiling.index();
    if level.effort_index().is_some_and(|index| index <= max_index) {
        return Some(level);
    }
    let supported = model.map_or(Effort::ALL.to_vec(), |model| {
        get_supported_efforts(model).to_vec()
    });
    supported
        .iter()
        .rev()
        .find(|candidate| candidate.index() <= max_index)
        .map(|candidate| ThinkingLevel::from_effort(*candidate))
        .or(Some(level))
}

/// Clamp a concrete effort to a per-session effort ceiling.
pub fn clamp_effort_to_ceiling(
    model: Option<&ThinkingModel>,
    level: Option<Effort>,
    ceiling: Option<Effort>,
) -> Option<Effort> {
    let level = level?;
    let ceiling = ceiling?;
    if level.index() <= ceiling.index() {
        return Some(level);
    }
    let supported = model.map_or(Effort::ALL.to_vec(), |model| {
        get_supported_efforts(model).to_vec()
    });
    supported
        .iter()
        .rev()
        .find(|candidate| candidate.index() <= ceiling.index())
        .copied()
        .or(Some(level))
}

/// True when `model` can honor a thinking-effort ceiling.
pub fn model_supports_effort_ceiling(model: &ThinkingModel, ceiling: Effort) -> bool {
    let supported = get_supported_efforts(model);
    if supported.is_empty() {
        return true;
    }
    let max_index = ceiling.index();
    supported
        .iter()
        .any(|candidate| candidate.index() <= max_index)
}

/// Err when `model` has no supported effort at or below `max_effort`.
pub fn assert_model_supports_effort_ceiling(
    model: &ThinkingModel,
    max_effort: Effort,
) -> Result<(), String> {
    if model_supports_effort_ceiling(model, max_effort) {
        return Ok(());
    }
    Err(format!(
        "{}/{} has no supported thinking effort at or below task.maxEffort={}",
        model.provider,
        model.id,
        max_effort.as_str()
    ))
}

/// The provisional concrete level shown while `auto` awaits classification.
pub fn resolve_provisional_auto_level(model: Option<&ThinkingModel>) -> Option<Effort> {
    let model = model?;
    if model.reasoning != Some(true) {
        return None;
    }
    if model
        .thinking
        .as_ref()
        .is_some_and(|thinking| thinking.unverified)
    {
        return None;
    }
    let preferred = model
        .thinking
        .as_ref()
        .and_then(|thinking| thinking.default_level)
        .unwrap_or(Effort::High);
    clamp_auto_thinking_effort(
        Some(model),
        if preferred == Effort::Max {
            Effort::XHigh
        } else {
            preferred
        },
        Some(Effort::XHigh),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(efforts: &[Effort], default_level: Option<Effort>) -> ThinkingModel {
        ThinkingModel {
            provider: "test".to_string(),
            id: "m1".to_string(),
            reasoning: Some(true),
            thinking: Some(ModelThinking {
                default_level,
                unverified: false,
                efforts: efforts.to_vec(),
            }),
        }
    }

    #[test]
    fn parses_exact_and_abbreviated_selectors() {
        assert_eq!(parse_effort(Some("high")), Some(Effort::High));
        assert_eq!(parse_effort(Some("h")), None);
        assert_eq!(parse_effort(Some("xhi")), Some(Effort::XHigh));
        assert_eq!(parse_effort(Some("med")), Some(Effort::Medium));
        assert_eq!(parse_effort(Some("m")), None);
        assert_eq!(parse_effort(None), None);
        assert_eq!(parse_thinking_level(Some("inherit")), Some(ThinkingLevel::Inherit));
        assert_eq!(parse_thinking_level(Some("auto")), None);
        assert_eq!(
            parse_configured_thinking_level(Some("auto")),
            Some(ConfiguredThinkingLevel::Auto)
        );
        assert_eq!(parse_cli_thinking_level(Some("inherit")), None);
        assert_eq!(
            parse_cli_thinking_level(Some("off")),
            Some(ConfiguredThinkingLevel::Off)
        );
    }

    #[test]
    fn metadata_labels_match_reference() {
        let metadata = get_thinking_level_metadata(ThinkingLevel::Minimal);
        assert_eq!(metadata.label, "min");
        assert_eq!(metadata.description, "Very brief reasoning (~1k tokens)");
        let auto = get_configured_thinking_level_metadata(ConfiguredThinkingLevel::Auto);
        assert_eq!(auto.label, "auto");
        assert_eq!(auto.description, "Auto-detect per prompt");
    }

    #[test]
    fn effort_conversion_preserves_off_and_inherit() {
        assert_eq!(to_reasoning_effort(Some(ThinkingLevel::High)), Some(Effort::High));
        assert_eq!(to_reasoning_effort(Some(ThinkingLevel::Off)), None);
        assert_eq!(to_reasoning_effort(Some(ThinkingLevel::Inherit)), None);
        assert_eq!(to_reasoning_effort(None), None);
        assert!(should_disable_reasoning(Some(ThinkingLevel::Off)));
        assert!(!should_disable_reasoning(Some(ThinkingLevel::Minimal)));
    }

    #[test]
    fn resolves_against_model_ladder() {
        let model = model(&[Effort::Low, Effort::Medium, Effort::High], Some(Effort::Medium));
        assert_eq!(
            resolve_thinking_level_for_model(Some(&model), Some(ThinkingLevel::XHigh)),
            Some(ResolvedThinkingLevel::Effort(Effort::High))
        );
        assert_eq!(
            resolve_thinking_level_for_model(Some(&model), Some(ThinkingLevel::Off)),
            Some(ResolvedThinkingLevel::Off)
        );
        assert_eq!(resolve_thinking_level_for_model(Some(&model), None), None);
        let unrestricted = model(&Effort::ALL, Some(Effort::High));
        assert_eq!(
            resolve_thinking_level_for_model(Some(&unrestricted), Some(ThinkingLevel::XHigh)),
            Some(ResolvedThinkingLevel::Effort(Effort::XHigh))
        );
    }

    #[test]
    fn provisional_auto_prefers_default_below_max() {
        let model = model(&Effort::ALL, Some(Effort::Medium));
        assert_eq!(resolve_provisional_auto_level(Some(&model)), Some(Effort::Medium));
        let sparse = model(&[Effort::Max], None);
        assert_eq!(resolve_provisional_auto_level(Some(&sparse)), None);
        let unverified = ThinkingModel {
            reasoning: Some(true),
            thinking: Some(ModelThinking {
                unverified: true,
                ..Default::default()
            }),
            ..model.clone()
        };
        assert_eq!(resolve_provisional_auto_level(Some(&unverified)), None);
    }

    #[test]
    fn new_session_level_order() {
        let model = model(&Effort::ALL, Some(Effort::Low));
        assert_eq!(
            new_session_thinking_level(
                Some(&model),
                Some(ConfiguredThinkingLevel::High),
                Some(ConfiguredThinkingLevel::Auto)
            ),
            Some(ConfiguredThinkingLevel::High)
        );
        assert_eq!(
            new_session_thinking_level(
                Some(&model),
                None,
                Some(ConfiguredThinkingLevel::Minimal)
            ),
            Some(ThinkingLevel::from_effort(Effort::Low).into())
        );
        assert_eq!(
            new_session_thinking_level(Some(&model), None, Some(ConfiguredThinkingLevel::Auto)),
            Some(ConfiguredThinkingLevel::Auto)
        );
        assert_eq!(
            new_session_thinking_level(Some(&model), None, None),
            None
        );
    }

    #[test]
    fn auto_effort_floors_at_low_and_ceils() {
        let sparse = model(&[Effort::Minimal, Effort::High, Effort::Max], None);
        assert_eq!(
            clamp_auto_thinking_effort(Some(&sparse), Effort::Medium, None),
            Some(Effort::High)
        );
        assert_eq!(
            clamp_auto_thinking_effort(Some(&sparse), Effort::XHigh, Some(Effort::High)),
            Some(Effort::High)
        );
        assert_eq!(
            clamp_auto_thinking_effort(Some(&sparse), Effort::XHigh, Some(Effort::Low)),
            None
        );
        let empty = model(&[], None);
        assert_eq!(clamp_auto_thinking_effort(Some(&empty), Effort::High, None), None);
    }

    #[test]
    fn task_effort_maps_range_ends_and_middle() {
        assert_eq!(
            resolve_task_effort_level(None, TaskEffort::Lo, None),
            Ok(Some(Effort::Minimal))
        );
        assert_eq!(
            resolve_task_effort_level(None, TaskEffort::Med, None),
            Ok(Some(Effort::Medium))
        );
        assert_eq!(
            resolve_task_effort_level(None, TaskEffort::Hi, None),
            Ok(Some(Effort::Max))
        );
        let sparse = model(&[Effort::Low, Effort::High], None);
        assert_eq!(
            resolve_task_effort_level(Some(&sparse), TaskEffort::Med, None),
            Ok(Some(Effort::Low))
        );
        let floor = model(&[Effort::High], None);
        assert_eq!(
            resolve_task_effort_level(Some(&floor), TaskEffort::Hi, Some(Effort::Low)),
            Err("test/m1 has no supported thinking effort at or below task.maxEffort=low".to_string())
        );
        assert_eq!(
            resolve_task_effort_level(Some(&sparse), TaskEffort::Hi, Some(Effort::Low)),
            Ok(Some(Effort::Low))
        );
    }

    #[test]
    fn ceiling_clamps_above_and_keeps_special_levels() {
        let sparse = model(&[Effort::Low, Effort::Medium], None);
        assert_eq!(
            clamp_thinking_level_to_ceiling(
                Some(&sparse),
                Some(ThinkingLevel::Max),
                Some(Effort::Medium)
            ),
            Some(ThinkingLevel::Medium)
        );
        assert_eq!(
            clamp_thinking_level_to_ceiling(
                Some(&sparse),
                Some(ThinkingLevel::Off),
                Some(Effort::Medium)
            ),
            Some(ThinkingLevel::Off)
        );
        assert_eq!(
            clamp_thinking_level_to_ceiling(Some(&sparse), None, Some(Effort::Medium)),
            None
        );
        assert_eq!(
            clamp_effort_to_ceiling(Some(&sparse), Some(Effort::Max), Some(Effort::Low)),
            Some(Effort::Low)
        );
    }

    #[test]
    fn prewalk_compares_models_mode_and_clamped_effort() {
        let model = model(&[Effort::Low, Effort::Medium, Effort::High], None);
        assert!(prewalk_would_be_noop(
            Some(&model),
            Some(ConfiguredThinkingLevel::Medium),
            &model,
            Some(ConfiguredThinkingLevel::Medium)
        ));
        assert!(!prewalk_would_be_noop(
            Some(&model),
            Some(ConfiguredThinkingLevel::Auto),
            &model,
            Some(ConfiguredThinkingLevel::Medium)
        ));
        assert!(prewalk_would_be_noop(
            Some(&model),
            Some(ConfiguredThinkingLevel::XHigh),
            &model,
            Some(ConfiguredThinkingLevel::High)
        ));
        let other = ThinkingModel {
            id: "m2".to_string(),
            ..model.clone()
        };
        assert!(!prewalk_would_be_noop(
            Some(&model),
            Some(ConfiguredThinkingLevel::High),
            &other,
            Some(ConfiguredThinkingLevel::High)
        ));
    }

    #[test]
    fn configured_auto_resolves_to_provisional() {
        let model = model(&Effort::ALL, Some(Effort::Medium));
        assert_eq!(
            resolve_configured_thinking_level(Some(&model), Some(ConfiguredThinkingLevel::Auto)),
            Some(ResolvedThinkingLevel::Effort(Effort::Medium))
        );
        assert_eq!(
            resolve_configured_thinking_level(Some(&model), Some(ConfiguredThinkingLevel::High)),
            Some(ResolvedThinkingLevel::Effort(Effort::High))
        );
        assert_eq!(resolve_configured_thinking_level(Some(&model), None), None);
    }
}
