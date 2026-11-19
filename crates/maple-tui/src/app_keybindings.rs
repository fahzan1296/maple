//! Application keybinding definitions, config loading/migration, and manager.
//! Reference: `l/src/app-keybindings.ts`.
//!
//! Deviations: file IO/dirs/logger/yaml utilities are dependency-injected or
//! implemented with std+serde_json (no bun/yaml/jsonc/logger crates). YAML
//! persistence is written as a flat mapping and flat `key: value` / `key: [a, b]`
//! forms are parsed; legacy JSON uses serde_json. `KeyId`/`Keybinding` are
//! strings to match the crate's canonical key-id model.

use crate::key_hint_format;
use crate::keybindings::KeybindingsManager as TuiKeybindingsManager;
use crate::theme::theme_class::Theme;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Application keybinding identifier (app.* namespace).
pub type AppKeybinding = &'static str;
/// Canonical key identifier (e.g. `ctrl+q`, `shift+tab`).
pub type KeyId = String;
/// Any keybinding identifier.
pub type Keybinding = String;
/// User keybindings config: keybinding id -> key(s) or unbound (`None`).
pub type KeybindingsConfig = BTreeMap<String, Option<KeyConfigValue>>;

/// One or many keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyConfigValue {
	One(KeyId),
	Many(Vec<KeyId>),
}

impl KeyConfigValue {
	fn as_vec(&self) -> Vec<KeyId> {
		match self {
			Self::One(k) => vec![k.clone()],
			Self::Many(v) => v.clone(),
		}
	}
	fn includes(&self, target: &str) -> bool {
		self.as_vec().iter().any(|k| k.eq_ignore_ascii_case(target))
	}
}

/// Keybinding definition entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeybindingDefinition {
	pub default_keys: Vec<KeyId>,
	pub description: &'static str,
}

/// Application-level keybinding ids (coding agent specific).
pub const APP_KEYBINDINGS: &[&str] = &[
	"app.interrupt",
	"app.clear",
	"app.exit",
	"app.suspend",
	"app.display.reset",
	"app.thinking.cycle",
	"app.thinking.toggle",
	"app.model.cycleForward",
	"app.model.cycleBackward",
	"app.model.select",
	"app.model.selectTemporary",
	"app.tools.expand",
	"app.tools.toggleVisibility",
	"app.ask.hide",
	"app.ask.note",
	"app.approval.approve",
	"app.editor.external",
	"app.local.open",
	"app.message.followUp",
	"app.retry",
	"app.message.dequeue",
	"app.btw.inject",
	"app.clipboard.pasteImage",
	"app.clipboard.pasteTextRaw",
	"app.clipboard.copyLine",
	"app.clipboard.copyPrompt",
	"app.agents.hub",
	"app.session.new",
	"app.session.tree",
	"app.session.fork",
	"app.session.resume",
	"app.session.observe",
	"app.session.togglePath",
	"app.session.toggleSort",
	"app.session.rename",
	"app.session.delete",
	"app.session.deleteNoninvasive",
	"app.session.pin",
	"app.session.unpin",
	"app.tree.previousBranch",
	"app.tree.nextBranch",
	"app.plan.toggle",
	"app.history.search",
	"app.links.hint",
	"app.stt.pushToTalk",
	"app.stt.toggle",
	"app.live.toggle",
];

/// Resolve default image-paste shortcuts for the current terminal platform.
pub fn get_default_paste_image_keys(platform: &str) -> Vec<KeyId> {
	match platform {
		"win32" => vec!["ctrl+v".into(), "alt+v".into()],
		"darwin" | "macos" => vec!["ctrl+v".into(), "super+v".into()],
		_ => vec!["ctrl+v".into()],
	}
}

fn def(keys: &[&str], description: &'static str) -> KeybindingDefinition {
	KeybindingDefinition {
		default_keys: keys.iter().map(|k| (*k).to_string()).collect(),
		description,
	}
}

fn host_platform() -> &'static str {
	if cfg!(target_os = "windows") {
		"win32"
	} else if cfg!(target_os = "macos") {
		"darwin"
	} else {
		"linux"
	}
}

/// App-specific keybinding definitions (TUI defaults live in `crate::keybindings`).
pub fn keybindings() -> BTreeMap<String, KeybindingDefinition> {
	let mut m = BTreeMap::new();
	m.insert("app.interrupt".into(), def(&["escape"], "Interrupt current operation"));
	m.insert("app.clear".into(), def(&["ctrl+c"], "Clear screen or cancel"));
	m.insert("app.exit".into(), def(&["ctrl+d"], "Exit application"));
	m.insert("app.suspend".into(), def(&["ctrl+z"], "Suspend application"));
	m.insert("app.display.reset".into(), def(&["alt+l"], "Reset terminal display"));
	m.insert("app.thinking.cycle".into(), def(&["shift+tab"], "Cycle thinking level"));
	m.insert("app.thinking.toggle".into(), def(&["ctrl+t"], "Toggle thinking mode"));
	m.insert("app.model.cycleForward".into(), def(&["ctrl+p"], "Cycle to next model"));
	m.insert("app.model.cycleBackward".into(), def(&["shift+ctrl+p"], "Cycle to previous model"));
	m.insert("app.model.select".into(), def(&["alt+m"], "Select model"));
	m.insert(
		"app.model.selectTemporary".into(),
		def(&["alt+p"], "Select temporary model for current session"),
	);
	m.insert("app.tools.expand".into(), def(&["ctrl+o"], "Expand tools"));
	m.insert(
		"app.tools.toggleVisibility".into(),
		def(&["ctrl+shift+o"], "Show or hide tool activity"),
	);
	m.insert("app.ask.hide".into(), def(&["alt+h"], "Hide or show a pending ask dialog"));
	m.insert(
		"app.ask.note".into(),
		def(&["n", "alt+n"], "Add or edit a note on the highlighted ask option"),
	);
	m.insert(
		"app.approval.approve".into(),
		def(&["ctrl+enter", "ctrl+q"], "Approve a pending tool call"),
	);
	m.insert("app.editor.external".into(), def(&["ctrl+g"], "Open external editor"));
	m.insert("app.local.open".into(), def(&["alt+g"], "Open a local:// file in external editor"));
	m.insert(
		"app.message.followUp".into(),
		def(&["ctrl+q", "ctrl+enter"], "Send follow-up message"),
	);
	m.insert("app.retry".into(), def(&["f5", "alt+r"], "Retry last failed assistant turn"));
	m.insert("app.message.dequeue".into(), def(&["alt+up", "shift+up"], "Dequeue message"));
	m.insert(
		"app.btw.inject".into(),
		def(&["alt+i"], "Send a /btw answer's steering prompt to the main session"),
	);
	m.insert(
		"app.clipboard.pasteImage".into(),
		KeybindingDefinition {
			default_keys: get_default_paste_image_keys(host_platform()),
			description: "Paste image or text from clipboard",
		},
	);
	m.insert(
		"app.clipboard.pasteTextRaw".into(),
		def(&["ctrl+shift+v", "alt+shift+v"], "Paste text from clipboard as raw text (no collapse)"),
	);
	m.insert("app.clipboard.copyLine".into(), def(&["alt+shift+l"], "Copy current line"));
	m.insert("app.clipboard.copyPrompt".into(), def(&["alt+shift+c"], "Copy prompt"));
	m.insert("app.agents.hub".into(), def(&["alt+a"], "Open the agent hub"));
	m.insert("app.session.new".into(), def(&[], "Create new session"));
	m.insert("app.session.tree".into(), def(&[], "Show session tree"));
	m.insert("app.session.fork".into(), def(&[], "Fork session"));
	m.insert("app.session.resume".into(), def(&[], "Resume session"));
	m.insert("app.session.observe".into(), def(&["ctrl+s"], "Open the agent hub"));
	m.insert("app.session.togglePath".into(), def(&["ctrl+p"], "Toggle session path display"));
	m.insert("app.session.toggleSort".into(), def(&["ctrl+s"], "Toggle session sort order"));
	m.insert("app.session.rename".into(), def(&["ctrl+r"], "Rename session"));
	m.insert("app.session.delete".into(), def(&["backspace", "delete"], "Delete session"));
	m.insert(
		"app.session.deleteNoninvasive".into(),
		def(&["ctrl+backspace"], "Delete session (non-invasive)"),
	);
	m.insert("app.session.pin".into(), def(&["alt+k"], "Pin the selected session in the resume picker"));
	m.insert(
		"app.session.unpin".into(),
		def(&["alt+shift+k"], "Unpin the selected session in the resume picker"),
	);
	m.insert("app.tree.previousBranch".into(), def(&[], "Jump to the previous branch in /tree"));
	m.insert("app.tree.nextBranch".into(), def(&[], "Jump to the next branch in /tree"));
	m.insert("app.plan.toggle".into(), def(&["alt+shift+p"], "Toggle plan mode"));
	m.insert("app.history.search".into(), def(&["ctrl+r"], "Search history"));
	m.insert(
		"app.links.hint".into(),
		def(&["alt+o"], "Pick an extension link on screen by its hint tag"),
	);
	m.insert("app.stt.pushToTalk".into(), def(&["space"], "Hold to record speech-to-text"));
	m.insert("app.stt.toggle".into(), def(&[], "Toggle speech-to-text recording"));
	m.insert("app.live.toggle".into(), def(&["ctrl+l"], "Start or stop live voice mode (/live)"));
	m
}

/// Migration map from old keybinding names to new namespaced IDs.
pub fn keybinding_name_migrations() -> BTreeMap<&'static str, &'static str> {
	BTreeMap::from([
		("interrupt", "app.interrupt"),
		("clear", "app.clear"),
		("exit", "app.exit"),
		("suspend", "app.suspend"),
		("displayReset", "app.display.reset"),
		("cycleThinkingLevel", "app.thinking.cycle"),
		("cycleModelForward", "app.model.cycleForward"),
		("cycleModelBackward", "app.model.cycleBackward"),
		("selectModel", "app.model.select"),
		("selectModelTemporary", "app.model.selectTemporary"),
		("togglePlanMode", "app.plan.toggle"),
		("historySearch", "app.history.search"),
		("expandTools", "app.tools.expand"),
		("toggleThinking", "app.thinking.toggle"),
		("externalEditor", "app.editor.external"),
		("followUp", "app.message.followUp"),
		("retry", "app.retry"),
		("dequeue", "app.message.dequeue"),
		("pasteImage", "app.clipboard.pasteImage"),
		("pasteTextRaw", "app.clipboard.pasteTextRaw"),
		("copyLine", "app.clipboard.copyLine"),
		("copyPrompt", "app.clipboard.copyPrompt"),
		("newSession", "app.session.new"),
		("tree", "app.session.tree"),
		("fork", "app.session.fork"),
		("resume", "app.session.resume"),
		("observeSessions", "app.session.observe"),
		("toggleSTT", "app.stt.toggle"),
		("cursorUp", "tui.editor.cursorUp"),
		("cursorDown", "tui.editor.cursorDown"),
		("cursorLeft", "tui.editor.cursorLeft"),
		("cursorRight", "tui.editor.cursorRight"),
		("cursorWordLeft", "tui.editor.cursorWordLeft"),
		("cursorWordRight", "tui.editor.cursorWordRight"),
		("cursorLineStart", "tui.editor.cursorLineStart"),
		("cursorLineEnd", "tui.editor.cursorLineEnd"),
		("jumpForward", "tui.editor.jumpForward"),
		("jumpBackward", "tui.editor.jumpBackward"),
		("pageUp", "tui.editor.pageUp"),
		("pageDown", "tui.editor.pageDown"),
		("deleteCharBackward", "tui.editor.deleteCharBackward"),
		("deleteCharForward", "tui.editor.deleteCharForward"),
		("deleteWordBackward", "tui.editor.deleteWordBackward"),
		("deleteWordForward", "tui.editor.deleteWordForward"),
		("deleteToLineStart", "tui.editor.deleteToLineStart"),
		("deleteToLineEnd", "tui.editor.deleteToLineEnd"),
		("yank", "tui.editor.yank"),
		("yankPop", "tui.editor.yankPop"),
		("undo", "tui.editor.undo"),
		("newLine", "tui.input.newLine"),
		("submit", "tui.input.submit"),
		("tab", "tui.input.tab"),
		("copy", "tui.input.copy"),
		("selectUp", "tui.select.up"),
		("selectDown", "tui.select.down"),
		("selectPageUp", "tui.select.pageUp"),
		("selectPageDown", "tui.select.pageDown"),
		("selectConfirm", "tui.select.confirm"),
		("selectCancel", "tui.select.cancel"),
		("toggleSessionNamedFilter", "app.session.togglePath"),
	])
}

/// Check if a key is a legacy keybinding name.
pub fn is_legacy_keybinding_name(key: &str) -> bool {
	keybinding_name_migrations().contains_key(key)
}

fn to_keybindings_config(value: &Value) -> KeybindingsConfig {
	let mut config = KeybindingsConfig::new();
	let Some(obj) = value.as_object() else { return config; };
	for (k, v) in obj {
		if v.is_null() {
			config.insert(k.clone(), None);
		} else if let Some(s) = v.as_str() {
			config.insert(k.clone(), Some(KeyConfigValue::One(s.to_string())));
		} else if let Some(arr) = v.as_array() {
			if arr.iter().all(|x| x.is_string()) {
				let keys = arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect();
				config.insert(k.clone(), Some(KeyConfigValue::Many(keys)));
			}
		}
	}
	config
}

/// Migrate old keybinding names to new namespaced IDs.
pub fn migrate_keybinding_names(raw: Option<&Value>) -> (KeybindingsConfig, bool) {
	let Some(raw) = raw else { return (KeybindingsConfig::new(), false); };
	let config = to_keybindings_config(raw);
	let migrations = keybinding_name_migrations();
	let mut migrated = KeybindingsConfig::new();
	let mut did = false;
	for (key, value) in config {
		if let Some(new_key) = migrations.get(key.as_str()) {
			migrated.insert((*new_key).to_string(), value);
			did = true;
		} else {
			migrated.insert(key, value);
		}
	}
	(migrated, did)
}

/// Order keybindings config to match known keybinding order, then append rest.
pub fn order_keybindings_config(config: &KeybindingsConfig) -> KeybindingsConfig {
	let mut ordered = KeybindingsConfig::new();
	let mut seen = BTreeSet::new();
	for key in keybindings().keys() {
		if let Some(v) = config.get(key) {
			ordered.insert(key.clone(), v.clone());
			seen.insert(key.clone());
		}
	}
	for (k, v) in config {
		if !seen.contains(k) {
			ordered.insert(k.clone(), v.clone());
		}
	}
	ordered
}

const KEYBINDINGS_YML: &str = "keybindings.yml";
const KEYBINDINGS_YAML: &str = "keybindings.yaml";
const LEGACY_KEYBINDINGS_JSON: &str = "keybindings.json";

#[derive(Clone, Debug)]
struct KeybindingsConfigPaths {
	read_path: PathBuf,
	write_back_path: PathBuf,
}

/// Controls inherited keybinding lookup when creating a manager for a named profile.
#[derive(Clone, Debug, Default)]
pub struct KeybindingsCreateOptions {
	/// Default-profile agent directory whose keybindings are merged before profile-specific bindings.
	pub inherited_agent_dir: Option<PathBuf>,
}

fn parse_flat_yaml(content: &str) -> Option<Value> {
	let mut map = serde_json::Map::new();
	for line in content.lines() {
		let line = line.trim();
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		let Some((k, v)) = line.split_once(':') else { continue; };
		let key = k.trim().trim_matches('"').trim_matches('\'').to_string();
		let v = v.trim();
		if v.is_empty() || v == "null" || v == "~" {
			map.insert(key, Value::Null);
			continue;
		}
		if v.starts_with('[') && v.ends_with(']') {
			let inner = &v[1..v.len() - 1];
			let arr: Vec<Value> = inner
				.split(',')
				.map(|s| s.trim().trim_matches('"').trim_matches('\''))
				.filter(|s| !s.is_empty())
				.map(|s| Value::String(s.to_string()))
				.collect();
			map.insert(key, Value::Array(arr));
		} else {
			let s = v.trim_matches('"').trim_matches('\'').to_string();
			map.insert(key, Value::String(s));
		}
	}
	if map.is_empty() { None } else { Some(Value::Object(map)) }
}

fn stringify_flat_yaml(config: &KeybindingsConfig) -> String {
	let mut out = String::new();
	for (k, v) in config {
		match v {
			None => out.push_str(&format!("{k}: null\n")),
			Some(KeyConfigValue::One(s)) => out.push_str(&format!("{k}: \"{s}\"\n")),
			Some(KeyConfigValue::Many(arr)) => {
				let items = arr.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", ");
				out.push_str(&format!("{k}: [{items}]\n"));
			}
		}
	}
	out
}

fn load_raw_config(file_path: &Path) -> Option<Value> {
	let content = fs::read_to_string(file_path).ok()?;
	match file_path.extension().and_then(|e| e.to_str()) {
		Some("json") => serde_json::from_str(&content).ok(),
		Some("yml") | Some("yaml") => serde_json::from_str(&content).ok().or_else(|| parse_flat_yaml(&content)),
		_ => None,
	}
}

fn write_keybindings_config(file_path: &Path, config: &KeybindingsConfig) -> bool {
	if let Some(parent) = file_path.parent() {
		let _ = fs::create_dir_all(parent);
	}
	fs::write(file_path, stringify_flat_yaml(config)).is_ok()
}

fn resolve_keybindings_config_paths(agent_dir: &Path) -> KeybindingsConfigPaths {
	let yml = agent_dir.join(KEYBINDINGS_YML);
	if yml.exists() {
		return KeybindingsConfigPaths { read_path: yml.clone(), write_back_path: yml };
	}
	let yaml = agent_dir.join(KEYBINDINGS_YAML);
	if yaml.exists() {
		return KeybindingsConfigPaths { read_path: yaml.clone(), write_back_path: yaml };
	}
	let json = agent_dir.join(LEGACY_KEYBINDINGS_JSON);
	if json.exists() {
		return KeybindingsConfigPaths { read_path: json, write_back_path: yml };
	}
	KeybindingsConfigPaths { read_path: yml.clone(), write_back_path: yml }
}

pub fn merge_keybindings_config(inherited: &KeybindingsConfig, profile: &KeybindingsConfig) -> KeybindingsConfig {
	let mut out = inherited.clone();
	for (k, v) in profile {
		out.insert(k.clone(), v.clone());
	}
	out
}

fn resolve_inherited_agent_dir(agent_dir: &Path, options: &KeybindingsCreateOptions) -> Option<PathBuf> {
	let inherited = options.inherited_agent_dir.clone()?;
	if inherited.canonicalize().ok() == agent_dir.canonicalize().ok() {
		return None;
	}
	Some(inherited)
}

fn load_keybindings_config(file_path: &Path, write_back_path: Option<&Path>) -> (KeybindingsConfig, PathBuf) {
	let raw = load_raw_config(file_path);
	let Some(raw) = raw else { return (KeybindingsConfig::new(), file_path.to_path_buf()); };
	let (migrated, did_migrate) = migrate_keybinding_names(Some(&raw));
	let ext_diff = write_back_path.map(|w| w != file_path).unwrap_or(false);
	if let Some(write_back) = write_back_path {
		if did_migrate || ext_diff {
			let ordered = order_keybindings_config(&migrated);
			if write_keybindings_config(write_back, &ordered) {
				return (migrated, write_back.to_path_buf());
			}
		}
	}
	(migrated, file_path.to_path_buf())
}

fn load_merged_keybindings_config(
	agent_dir: &Path,
	options: &KeybindingsCreateOptions,
) -> (KeybindingsConfig, PathBuf, Option<PathBuf>) {
	let profile_paths = resolve_keybindings_config_paths(agent_dir);
	let (profile, profile_path) = load_keybindings_config(&profile_paths.read_path, Some(&profile_paths.write_back_path));
	let Some(inherited_agent_dir) = resolve_inherited_agent_dir(agent_dir, options) else {
		return (profile, profile_path, None);
	};
	let inherited_paths = resolve_keybindings_config_paths(&inherited_agent_dir);
	// Read-only for inherited: do not write migration output into default profile dir.
	let (inherited, inherited_path) = load_keybindings_config(&inherited_paths.read_path, None);
	(
		merge_keybindings_config(&inherited, &profile),
		profile_path,
		Some(inherited_path),
	)
}

/// Migrate legacy keybinding names and JSON configuration in an agent directory.
pub fn migrate_keybindings_config_file(agent_dir: &Path) {
	let paths = resolve_keybindings_config_paths(agent_dir);
	let _ = load_keybindings_config(&paths.read_path, Some(&paths.write_back_path));
}

const FOLLOW_UP_KEYBINDING: &str = "app.message.followUp";
const WINDOWS_FOLLOW_UP_FALLBACK_KEY: &str = "ctrl+q";
const DEQUEUE_KEYBINDING: &str = "app.message.dequeue";
const MACOS_DEQUEUE_FALLBACK_KEY: &str = "shift+up";

fn fallback_key(keybinding: &str) -> Option<&'static str> {
	match keybinding {
		FOLLOW_UP_KEYBINDING => Some(WINDOWS_FOLLOW_UP_FALLBACK_KEY),
		DEQUEUE_KEYBINDING => Some(MACOS_DEQUEUE_FALLBACK_KEY),
		_ => None,
	}
}

fn user_binding_claims_key(config: &KeybindingsConfig, target: &str, except: &str) -> bool {
	for (keybinding, keys) in config {
		if keybinding == except {
			continue;
		}
		if let Some(keys) = keys {
			if keys.includes(target) {
				return true;
			}
		}
	}
	false
}

fn remove_key(keys: Vec<KeyId>, target: &str) -> Vec<KeyId> {
	keys.into_iter().filter(|k| !k.eq_ignore_ascii_case(target)).collect()
}

fn key_config_value(keys: Vec<KeyId>) -> Option<KeyConfigValue> {
	match keys.len() {
		0 => Some(KeyConfigValue::Many(vec![])),
		1 => Some(KeyConfigValue::One(keys.into_iter().next().unwrap())),
		_ => Some(KeyConfigValue::Many(keys)),
	}
}

/// App-level keybindings manager (wraps TUI manager, adds file config/fallbacks).
pub struct KeybindingsManager {
	inner: TuiKeybindingsManager,
	config_path: Option<PathBuf>,
	inherited_config_path: Option<PathBuf>,
	user_bindings: KeybindingsConfig,
}

impl KeybindingsManager {
	pub fn new(user_bindings: KeybindingsConfig, config_path: Option<PathBuf>, inherited_config_path: Option<PathBuf>) -> Self {
		let mut mgr = Self {
			inner: TuiKeybindingsManager::new(),
			config_path,
			inherited_config_path,
			user_bindings: user_bindings.clone(),
		};
		mgr.apply_user_bindings(&user_bindings);
		mgr
	}

	/// Create from config files at agent_dir/keybindings.yml and the default profile.
	pub fn create(agent_dir: &Path, options: KeybindingsCreateOptions) -> Self {
		let (user, profile_path, inherited_path) = load_merged_keybindings_config(agent_dir, &options);
		Self::new(user, Some(profile_path), inherited_path)
	}

	/// Create an in-memory keybindings manager without file persistence.
	pub fn in_memory(user_bindings: KeybindingsConfig) -> Self {
		Self::new(user_bindings, None, None)
	}

	fn apply_user_bindings(&mut self, user: &KeybindingsConfig) {
		for (id, val) in user {
			let keys = val.as_ref().map(|v| v.as_vec());
			self.inner.set_user_binding(id.clone(), keys);
		}
	}

	/// Reload keybindings from the config files.
	pub fn reload(&mut self, agent_dir: &Path, options: KeybindingsCreateOptions) {
		if self.config_path.is_none() {
			return;
		}
		let (user, profile_path, inherited_path) = load_merged_keybindings_config(agent_dir, &options);
		self.config_path = Some(profile_path);
		self.inherited_config_path = inherited_path;
		self.set_user_bindings(user);
	}

	pub fn set_user_bindings(&mut self, user: KeybindingsConfig) {
		self.user_bindings = user.clone();
		self.inner = TuiKeybindingsManager::new();
		self.apply_user_bindings(&user);
	}

	/// Resolved keys for a keybinding, applying platform fallbacks.
	pub fn get_keys(&self, keybinding: &str) -> Vec<KeyId> {
		let defaults = keybindings();
		let mut keys = if let Some(v) = self.user_bindings.get(keybinding) {
			match v {
				Some(k) => k.as_vec(),
				None => vec![],
			}
		} else if let Some(def) = defaults.get(keybinding) {
			def.default_keys.clone()
		} else {
			self.inner.get_keys(keybinding)
		};
		if let Some(fb) = fallback_key(keybinding) {
			if self.user_bindings.get(keybinding).is_none()
				&& user_binding_claims_key(&self.user_bindings, fb, keybinding)
			{
				keys = remove_key(keys, fb);
			}
		}
		keys
	}

	/// Effective resolved bindings (defaults + user overrides), with fallbacks applied.
	pub fn get_resolved_bindings(&self) -> KeybindingsConfig {
		let mut resolved = self.user_bindings.clone();
		for (id, def) in keybindings() {
			resolved.entry(id.clone()).or_insert_with(|| {
				Some(if def.default_keys.len() == 1 {
					KeyConfigValue::One(def.default_keys[0].clone())
				} else {
					KeyConfigValue::Many(def.default_keys.clone())
				})
			});
		}
		if let Some(v) = key_config_value(self.get_keys(FOLLOW_UP_KEYBINDING)) {
			resolved.insert(FOLLOW_UP_KEYBINDING.into(), Some(v));
		}
		resolved
	}

	pub fn get_effective_config(&self) -> KeybindingsConfig {
		self.get_resolved_bindings()
	}

	/// Display string for a keybinding's keys; empty when unbound.
	pub fn get_display_string(&self, keybinding: &str, theme: &Theme) -> String {
		let keys = self.get_keys(keybinding);
		if keys.is_empty() {
			return String::new();
		}
		let fmt = key_hint_format::KeyHintFormatter::new(theme);
		keys.iter().map(|k| fmt.format_key_hint(k)).collect::<Vec<_>>().join("/")
	}

	pub fn user_bindings(&self) -> &KeybindingsConfig {
		&self.user_bindings
	}

	pub fn config_path(&self) -> Option<&Path> {
		self.config_path.as_deref()
	}

	pub fn inherited_config_path(&self) -> Option<&Path> {
		self.inherited_config_path.as_deref()
	}
}

// Re-export formatting helpers (TS: export * from "./key-hint-format")
pub use crate::key_hint_format::*;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn paste_image_defaults() {
		assert_eq!(get_default_paste_image_keys("win32"), vec!["ctrl+v", "alt+v"]);
		assert_eq!(get_default_paste_image_keys("darwin"), vec!["ctrl+v", "super+v"]);
		assert_eq!(get_default_paste_image_keys("linux"), vec!["ctrl+v"]);
	}

	#[test]
	fn migrates_legacy_names() {
		let raw = serde_json::json!({"followUp": "alt+enter", "newLine": "shift+enter"});
		let (cfg, did) = migrate_keybinding_names(Some(&raw));
		assert!(did);
		assert!(cfg.contains_key("app.message.followUp"));
		assert!(cfg.contains_key("tui.input.newLine"));
		assert!(is_legacy_keybinding_name("followUp"));
	}

	#[test]
	fn merges_and_orders() {
		let mut inherited = KeybindingsConfig::new();
		inherited.insert("app.interrupt".into(), Some(KeyConfigValue::One("escape".into())));
		let mut profile = KeybindingsConfig::new();
		profile.insert("app.clear".into(), Some(KeyConfigValue::One("ctrl+c".into())));
		let merged = merge_keybindings_config(&inherited, &profile);
		assert!(merged.contains_key("app.interrupt") && merged.contains_key("app.clear"));
		let ordered = order_keybindings_config(&merged);
		let keys: Vec<_> = ordered.keys().cloned().collect();
		let i = keys.iter().position(|k| k == "app.interrupt").unwrap();
		let c = keys.iter().position(|k| k == "app.clear").unwrap();
		assert!(i < c);
	}

	#[test]
	fn keybindings_include_app_ids() {
		let defs = keybindings();
		assert!(defs.contains_key("app.live.toggle"));
		assert_eq!(defs.get("app.tools.expand").unwrap().default_keys, vec!["ctrl+o"]);
		assert!(APP_KEYBINDINGS.contains(&"app.stt.pushToTalk"));
	}
}
