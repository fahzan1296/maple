//! Port of `l/test/welcome.test.ts`.
//!
//! Deviations: the reference inspects a native node tree and mocks
//! `Math.random`/the theme symbol preset; the crate renders plain lines and
//! picks tips from wall-clock entropy, so tip choice is asserted statistically
//! through [`pick_weighted_tip`] and stability across renders rather than by
//! forcing a roll.

use std::time::Instant;

use maple_tui::key_hint_format::KeyHintFormatter;
use maple_tui::keybindings::KeybindingsManager;
use maple_tui::prompt::welcome::{
	pick_weighted_tip, render_welcome_tip, VersionUpdate, WelcomeComponent, WelcomeEnv,
	NEW_TIP_WEIGHT, TIP_MIN_COLUMNS,
};
use maple_tui::strip_ansi;
use maple_tui::text_shape::visible_width;
use maple_tui::theme::builtin::builtin_theme_json;
use maple_tui::theme::{create_theme, CreateThemeOptions, Theme};

fn dark() -> Theme {
	create_theme(
		builtin_theme_json("dark").expect("embedded dark theme"),
		CreateThemeOptions::default(),
	)
	.expect("dark theme resolves")
}

macro_rules! env {
	() => {{
		let theme = dark();
		let formatter = KeyHintFormatter::new(&theme);
		let keybindings = KeybindingsManager::new();
		WelcomeEnv {
			theme: &theme,
			formatter: &formatter,
			keybindings: &keybindings,
			true_color: true,
			native_rendering: false,
		}
	}};
}

fn plain(lines: &[String]) -> Vec<String> {
	lines.iter().map(|line| strip_ansi(line)).collect()
}

#[test]
fn sets_the_version_under_the_wordmark_beside_the_logo() {
	let env = env!();
	let mut welcome = WelcomeComponent::new("18.4.12");
	let rows = plain(&welcome.render(140, &env, Instant::now()));
	assert!(rows.iter().any(|row| row.contains('█')));
	let word = rows.iter().find(|row| row.contains("▄▀▀▄")).expect("wordmark");
	let version = rows.iter().find(|row| row.contains("v18.4.12")).expect("version");
	// The version sits on the row below the wordmark, under its left edge.
	assert_eq!(version.find("v18.4.12"), word.find("▄▀▀▄"));
}

#[test]
fn trails_the_version_with_update_progress_without_moving_the_lockup() {
	let env = env!();
	let now = Instant::now();
	let baseline = plain(&WelcomeComponent::new("1.0.0").render(80, &env, now));
	let mut welcome = WelcomeComponent::new("1.0.0");
	welcome.set_version_update(Some(VersionUpdate::Updating {
		version: "1.1.0".into(),
		fraction: Some(0.5),
	}));
	let updated = plain(&welcome.render(80, &env, now));
	assert_eq!(&baseline[..4], &updated[..4]);
	let version_row = updated.iter().find(|row| row.contains("v1.0.0")).expect("version row");
	assert!(version_row.contains("→ v1.1.0"), "{version_row}");
	assert!(version_row.contains("50%"), "{version_row}");

	// Narrower terminals drop the arrow rather than overflow.
	let mut narrow = WelcomeComponent::new("1.0.0");
	narrow.set_version_update(Some(VersionUpdate::Updating {
		version: "1.1.0".into(),
		fraction: None,
	}));
	for columns in [80usize, 56, 38] {
		let rows = plain(&narrow.render(columns, &env, now));
		let row = rows.iter().find(|row| row.contains("v1.0.0")).expect("version row");
		assert!(visible_width(row) < columns, "{columns}: {row:?}");
	}
	let very_narrow = plain(&narrow.render(38, &env, now));
	let row = very_narrow.iter().find(|row| row.contains("v1.0.0")).unwrap();
	assert!(!row.contains('→'), "{row}");
}

#[test]
fn weights_new_tips_above_ordinary_tips() {
	let tips: Vec<String> = ["plain one", "shiny thing [NEW]", "plain two"]
		.iter()
		.map(|tip| tip.to_string())
		.collect();
	let samples = 10_000;
	let mut counts = std::collections::HashMap::new();
	for index in 0..samples {
		let r = (index as f64 + 0.5) / samples as f64;
		let tip = pick_weighted_tip(&tips, r);
		*counts.entry(tip).or_insert(0usize) += 1;
	}
	let new_max = counts
		.iter()
		.filter(|(tip, _)| tip.ends_with("[NEW]"))
		.map(|(_, count)| *count)
		.max()
		.unwrap_or(0);
	let ordinary_max = counts
		.iter()
		.filter(|(tip, _)| !tip.ends_with("[NEW]"))
		.map(|(_, count)| *count)
		.max()
		.unwrap_or(0);
	assert!(new_max > 0);
	assert!(new_max > ordinary_max);
	assert_eq!(pick_weighted_tip(&[], 0.5), "");
	assert!(NEW_TIP_WEIGHT > 1);
}

#[test]
fn centers_the_lockup_and_keeps_a_column_clear_on_each_side() {
	let env = env!();
	let columns = 140;
	let rows = plain(&WelcomeComponent::new("1.0.0").render(columns, &env, Instant::now()));
	let word = rows.iter().find(|row| row.contains("▄▀▀▄")).expect("wordmark");
	let left = visible_width(word) - visible_width(word.trim_start());
	let right = columns - visible_width(word);
	assert!(left.abs_diff(right) <= 1, "left {left} right {right}");
}

#[test]
fn drops_the_tip_below_fifty_columns() {
	let env = env!();
	let now = Instant::now();
	let at_limit = plain(&WelcomeComponent::new("1.0.0").render(TIP_MIN_COLUMNS, &env, now));
	assert!(at_limit.iter().any(|row| row.contains("Tip: ")));
	let below = plain(&WelcomeComponent::new("1.0.0").render(TIP_MIN_COLUMNS - 1, &env, now));
	assert!(!below.iter().any(|row| row.contains("Tip: ")));
	assert!(below.iter().any(|row| row.contains("v1.0.0")));
}

#[test]
fn keeps_the_logo_alone_when_the_lockup_does_not_fit() {
	let env = env!();
	let now = Instant::now();
	let fits = plain(&WelcomeComponent::new("1.0.0").render(33, &env, now));
	assert!(fits.iter().any(|row| row.contains("▄▀▀▄")));
	assert!(fits.iter().any(|row| row.contains("v1.0.0")));

	let narrow = plain(&WelcomeComponent::new("1.0.0").render(32, &env, now));
	assert!(narrow.iter().any(|row| row.contains("█")));
	assert!(!narrow.iter().any(|row| row.contains("▄▀▀▄")));
	assert!(!narrow.iter().any(|row| row.contains("v1.0.0")));
}

#[test]
fn latches_the_session_tip_across_renders() {
	let env = env!();
	let mut welcome = WelcomeComponent::new("1.0.0");
	let first = welcome.tip(&env).expect("a tip");
	assert_eq!(welcome.tip(&env), Some(first.clone()));
	assert_eq!(welcome.tip(&env), Some(first.clone()));

	// The rendered tip wraps within the measure.
	let lines = render_welcome_tip(&first, 72, 0.0, &env);
	assert!(!lines.is_empty());
	for line in &lines {
		assert!(visible_width(line) <= 72, "{line:?}");
	}
}
