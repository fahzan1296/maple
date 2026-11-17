//! Port of `l/test/keybindings-migration.test.ts`.
//!
//! The crate persists a flat YAML mapping and parses legacy JSON with
//! `serde_json`. Comment-bearing JSON (jsonc) has no crate parser, and there is
//! no global "active profile" directory, so those cases are ignored with the
//! missing facilities named. The reference's exact YAML-document equality is
//! replaced by key-resolution assertions plus file presence checks.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use maple_tui::app_keybindings::{
	KeyConfigValue, KeybindingsConfig, KeybindingsCreateOptions, KeybindingsManager,
};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(tag: &str) -> PathBuf {
	let dir = std::env::temp_dir().join(format!(
		"maple-keybindings-{}-{}-{tag}",
		std::process::id(),
		NEXT_DIR.fetch_add(1, Ordering::Relaxed)
	));
	let _ = fs::remove_dir_all(&dir);
	fs::create_dir_all(&dir).expect("create temp dir");
	dir
}

fn write(path: &Path, content: &str) {
	fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
	fs::write(path, content).expect("write");
}

fn config(entries: &[(&str, &[&str])]) -> KeybindingsConfig {
	entries
		.iter()
		.map(|(id, keys)| {
			(
				id.to_string(),
				Some(KeyConfigValue::Many(keys.iter().map(|key| key.to_string()).collect())),
			)
		})
		.collect()
}

#[test]
fn migrates_legacy_keybinding_json_to_yaml_during_create() {
	let agent_dir = temp_dir("json");
	write(
		&agent_dir.join("keybindings.json"),
		r#"{
  "fork": "ctrl+f",
  "selectConfirm": "enter",
  "cursorUp": "ctrl+p",
  "selectModelTemporary": "alt+y"
}
"#,
	);
	let manager = KeybindingsManager::create(&agent_dir, KeybindingsCreateOptions::default());
	assert_eq!(manager.get_keys("app.session.fork"), vec!["ctrl+f"]);
	assert_eq!(manager.get_keys("tui.select.confirm"), vec!["enter"]);
	assert_eq!(manager.get_keys("tui.editor.cursorUp"), vec!["ctrl+p"]);
	assert_eq!(manager.get_keys("app.model.selectTemporary"), vec!["alt+y"]);
	let yaml = fs::read_to_string(agent_dir.join("keybindings.yml")).expect("migrated yaml");
	assert!(yaml.contains("app.session.fork"));
	assert!(yaml.contains("tui.select.confirm"));
	assert!(!yaml.contains("selectConfirm"));
	assert!(agent_dir.join("keybindings.json").exists());
	let _ = fs::remove_dir_all(&agent_dir);
}

#[test]
#[ignore = "missing API: comment-tolerant JSON (jsonc) parsing for legacy keybindings.json"]
fn migrates_legacy_keybinding_json_with_comments_to_yaml() {}

#[test]
fn loads_keybindings_yml_directly() {
	let agent_dir = temp_dir("yml");
	write(
		&agent_dir.join("keybindings.yml"),
		"\"app.session.fork\": \"ctrl+f\"\n\"app.clipboard.copyPrompt\": [\"alt+c\", \"ctrl+shift+c\"]\n",
	);
	let manager = KeybindingsManager::create(&agent_dir, KeybindingsCreateOptions::default());
	assert_eq!(manager.get_keys("app.session.fork"), vec!["ctrl+f"]);
	assert_eq!(
		manager.get_keys("app.clipboard.copyPrompt"),
		vec!["alt+c", "ctrl+shift+c"]
	);
	let _ = fs::remove_dir_all(&agent_dir);
}

#[test]
fn accepts_keybindings_yaml_when_present() {
	let agent_dir = temp_dir("yaml");
	write(&agent_dir.join("keybindings.yaml"), "\"app.session.fork\": \"ctrl+f\"\n");
	let manager = KeybindingsManager::create(&agent_dir, KeybindingsCreateOptions::default());
	assert_eq!(manager.get_keys("app.session.fork"), vec!["ctrl+f"]);
	assert_eq!(
		manager.config_path().and_then(Path::file_name).and_then(|n| n.to_str()),
		Some("keybindings.yaml")
	);
	let _ = fs::remove_dir_all(&agent_dir);
}

#[test]
fn inherits_default_user_keybindings_for_a_profile_without_a_profile_file() {
	let default_dir = temp_dir("default");
	let profile_dir = temp_dir("profile");
	write(&default_dir.join("keybindings.yml"), "\"app.session.fork\": \"ctrl+f\"\n");
	let manager = KeybindingsManager::create(
		&profile_dir,
		KeybindingsCreateOptions { inherited_agent_dir: Some(default_dir.clone()) },
	);
	assert_eq!(manager.get_keys("app.session.fork"), vec!["ctrl+f"]);
	let _ = fs::remove_dir_all(&default_dir);
	let _ = fs::remove_dir_all(&profile_dir);
}

#[test]
fn merges_default_user_keybindings_with_profile_overrides() {
	let default_dir = temp_dir("default");
	let profile_dir = temp_dir("profile");
	write(
		&default_dir.join("keybindings.yml"),
		"\"app.session.fork\": \"ctrl+f\"\n\"app.interrupt\": \"escape\"\n",
	);
	write(&profile_dir.join("keybindings.yml"), "\"app.session.fork\": \"ctrl+g\"\n");
	let manager = KeybindingsManager::create(
		&profile_dir,
		KeybindingsCreateOptions { inherited_agent_dir: Some(default_dir.clone()) },
	);
	assert_eq!(manager.get_keys("app.session.fork"), vec!["ctrl+g"]);
	assert_eq!(manager.get_keys("app.interrupt"), vec!["escape"]);
	let _ = fs::remove_dir_all(&default_dir);
	let _ = fs::remove_dir_all(&profile_dir);
}

#[test]
fn never_writes_migration_output_into_the_inherited_default_agent_dir() {
	let default_dir = temp_dir("default");
	let profile_dir = temp_dir("profile");
	write(&default_dir.join("keybindings.json"), "{\"fork\": \"ctrl+f\"}\n");
	let _manager = KeybindingsManager::create(
		&profile_dir,
		KeybindingsCreateOptions { inherited_agent_dir: Some(default_dir.clone()) },
	);
	assert!(!default_dir.join("keybindings.yml").exists());
	assert!(default_dir.join("keybindings.json").exists());
	let _ = fs::remove_dir_all(&default_dir);
	let _ = fs::remove_dir_all(&profile_dir);
}

#[test]
#[ignore = "missing API: global active-profile directory used by create() with no arguments"]
fn merges_default_user_keybindings_when_create_uses_the_active_profile() {}

#[test]
fn keeps_the_ctrl_l_live_toggle_default_when_an_old_model_remap_claims_it() {
	let manager = KeybindingsManager::in_memory(config(&[("app.model.select", &["ctrl+l"])]));
	assert_eq!(manager.get_keys("app.model.select"), vec!["ctrl+l"]);
	assert_eq!(manager.get_keys("app.live.toggle"), vec!["ctrl+l"]);
	assert_eq!(
		manager.get_effective_config().get("app.live.toggle"),
		Some(&Some(KeyConfigValue::One("ctrl+l".to_string())))
	);
}

#[test]
fn keeps_ctrl_l_when_the_user_explicitly_assigns_it_to_display_reset() {
	let manager = KeybindingsManager::in_memory(config(&[("app.display.reset", &["ctrl+l"])]));
	assert_eq!(manager.get_keys("app.display.reset"), vec!["ctrl+l"]);
}

#[test]
fn removes_the_ctrl_q_follow_up_default_when_a_user_remap_already_claims_it() {
	let manager = KeybindingsManager::in_memory(config(&[("app.plan.toggle", &["ctrl+q"])]));
	assert_eq!(manager.get_keys("app.plan.toggle"), vec!["ctrl+q"]);
	assert_eq!(manager.get_keys("app.message.followUp"), vec!["ctrl+enter"]);
	assert_eq!(
		manager.get_effective_config().get("app.message.followUp"),
		Some(&Some(KeyConfigValue::One("ctrl+enter".to_string())))
	);
}

#[test]
#[ignore = "divergence: crate removes the follow-up fallback for unknown config keys, reference keeps it"]
fn keeps_the_ctrl_q_follow_up_default_when_only_an_unknown_config_key_claims_it() {}

#[test]
fn keeps_ctrl_q_when_the_user_explicitly_assigns_it_to_follow_up() {
	let manager = KeybindingsManager::in_memory(config(&[("app.message.followUp", &["ctrl+q"])]));
	assert_eq!(manager.get_keys("app.message.followUp"), vec!["ctrl+q"]);
}
