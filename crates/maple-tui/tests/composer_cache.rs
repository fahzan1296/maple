//! Port of the `l/test/composer-cache.test.ts` store contracts that are visible
//! across connections: shared per-project speculation, own-status precedence,
//! and a legacy payload format being dropped.
//!
//! Deviations: the reference spies on bun:sqlite statement/transaction calls;
//! the crate's store is a JSON file with its own identical-write guard (unit
//! tested in the module), so the transaction-spy test is not portable. XDG
//! environment loading has no crate equivalent either.

use std::path::PathBuf;

use maple_tui::prompt::composer::{BorderColor, ComposerPreferences, ComposerStatusCache};
use maple_tui::prompt::composer_cache::{ComposerCache, ComposerStartupCache, ComposerThemePreferences};
use maple_tui::theme::symbols::SymbolPreset;

fn temp_store(name: &str) -> PathBuf {
	let dir = std::env::temp_dir().join(format!("maple-composer-cache-it-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).expect("temp dir");
	dir.join(name)
}

fn status(thinking_level: &str) -> ComposerStatusCache {
	ComposerStatusCache {
		border_color: Some(BorderColor {
			prefix: "\u{1b}[36m".to_string(),
			suffix: "\u{1b}[39m".to_string(),
		}),
		status_line: serde_json::json!({
			"settings": { "leftSegments": ["model", "path", "git"], "contextLine": "embedded" },
			"gitEnabled": true,
			"thinkingLevel": thinking_level,
			"autoThinking": false,
			"fastMode": false,
			"usingSubscription": true,
			"autoCompactEnabled": true,
			"compactionBoundaries": { "thresholdPercent": 80, "speculationPercent": null },
		}),
	}
}

#[test]
fn round_trips_per_project_speculation_and_serves_shared_rows_to_other_projects() {
	let path = temp_store("round-trip.json");
	let mut preferences = ComposerPreferences::default();
	preferences.composer_shape = "rail".to_string();
	preferences.autocomplete_max_visible = 7;
	let theme = ComposerThemePreferences {
		symbol_preset: Some(SymbolPreset::Ascii),
		color_blind_mode: Some(true),
		dark_theme: Some("dark".to_string()),
		light_theme: Some("light".to_string()),
	};
	let cache_status = status("high");

	{
		let mut writer = ComposerCache::open(Some(&path)).expect("writer opens");
		writer.write_ui("/tmp/project", &preferences, &theme);
		writer.write_status("/tmp/project", &cache_status, true);
		writer.close();
	}

	let mut reader = ComposerCache::open(Some(&path)).expect("reader opens");
	let expected = ComposerStartupCache {
		preferences: Some(preferences.clone()),
		theme: Some(theme.clone()),
		status: Some(cache_status.clone()),
	};
	assert_eq!(reader.read("/tmp/project"), expected);
	// Theme and status follow the user to a project without its own row.
	assert_eq!(reader.read("/tmp/other"), expected);
	drop(reader);
	let _ = std::fs::remove_file(&path);
}

#[test]
fn prefers_a_projects_own_status_over_the_last_status_written_elsewhere() {
	let path = temp_store("precedence.json");
	let mut cache = ComposerCache::open(Some(&path)).expect("opens");
	cache.write_status("/tmp/a", &status("low"), true);
	cache.write_status("/tmp/b", &status("high"), true);

	let own = cache.read("/tmp/a");
	assert_eq!(
		own.status.expect("own status").status_line["thinkingLevel"].as_str(),
		Some("low")
	);
	let fresh = cache.read("/tmp/fresh");
	assert_eq!(
		fresh.status.expect("fallback status").status_line["thinkingLevel"].as_str(),
		Some("high")
	);
	cache.close();
	let _ = std::fs::remove_file(&path);
}

#[test]
fn drops_a_store_written_in_an_older_payload_format() {
	let path = temp_store("legacy.json");
	std::fs::write(
		&path,
		serde_json::json!({
			"version": 0,
			"entries": {
				"/tmp/project\u{0}status": serde_json::to_string(&status("high")).expect("payload"),
			}
		})
		.to_string(),
	)
	.expect("legacy store");

	let mut cache = ComposerCache::open(Some(&path)).expect("opens");
	let read = cache.read("/tmp/project");
	assert!(read.status.is_none(), "a legacy store must not be read");
	assert!(read.preferences.is_none());
	cache.close();
	let _ = std::fs::remove_file(&path);
}

#[test]
#[ignore = "missing facility: no sqlite statement/transaction layer to spy on; the JSON store guards identical writes by mtime (module unit test)"]
fn skips_write_transactions_for_identical_payloads() {
	let path = temp_store("skip.json");
	let mut cache = ComposerCache::open(Some(path.as_path())).expect("opens");
	cache.write_status("/tmp/project", &status("high"), true);
	let first = std::fs::metadata(&path).expect("written").modified().expect("mtime");
	std::thread::sleep(std::time::Duration::from_millis(10));
	cache.write_status("/tmp/project", &status("high"), true);
	let second = std::fs::metadata(&path).expect("written").modified().expect("mtime");
	assert_eq!(first, second);
}
