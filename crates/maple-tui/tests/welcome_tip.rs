//! Port of `l/test/welcome-tip.test.ts`.

use maple_tui::key_hint_format::KeyHintFormatter;
use maple_tui::keybindings::KeybindingsManager;
use maple_tui::prompt::welcome::{render_welcome_tip, WelcomeEnv};
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

fn tip_lines(tip: &str, width: usize, phase: f64) -> Vec<String> {
	let theme = dark();
	let formatter = KeyHintFormatter::new(&theme);
	let keybindings = KeybindingsManager::new();
	let env = WelcomeEnv {
		theme: &theme,
		formatter: &formatter,
		keybindings: &keybindings,
		true_color: true,
		native_rendering: false,
	};
	render_welcome_tip(tip, width, phase, &env)
}

fn plain(lines: &[String]) -> Vec<String> {
	lines.iter().map(|line| strip_ansi(line)).collect()
}

#[test]
fn wraps_long_tips_into_unindented_lines_instead_of_truncating() {
	let tip = "Next time you see spaghetti try creating a TTSR rule that prevents this pattern before it spreads";
	let width = 44;
	let lines = plain(&tip_lines(tip, width, 0.0));
	assert!(lines.len() > 1, "{lines:?}");
	assert!(!lines.join(" ").contains('…'), "{lines:?}");
	assert!(lines[0].starts_with("Tip: Next time"), "{:?}", lines[0]);
	for line in &lines {
		assert!(!line.starts_with(' '), "{line:?}");
		assert!(visible_width(line) <= width, "{line:?}");
	}
}

#[test]
fn replaces_a_trailing_new_marker_with_the_rainbow_tag() {
	let lines = tip_lines("Try the shiny advisor [NEW]", 60, 0.0);
	let plain = plain(&lines).join("\n");
	let styled = lines.join("\n");
	assert!(plain.contains("Try the shiny advisor"), "{plain:?}");
	assert!(!plain.contains("[NEW]"), "{plain:?}");
	assert!(plain.contains("NEW!"), "{plain:?}");
	assert!(styled.contains("\x1b[1m"), "{styled:?}");
	assert_ne!(styled, plain);
}

#[test]
fn keeps_the_new_tag_within_the_box_width() {
	for phase in [0.0, 0.25, 0.5, 0.75] {
		let lines = tip_lines("Try the shiny advisor [NEW]", 40, phase);
		for line in &lines {
			assert!(visible_width(line) <= 40, "phase {phase}: {line:?}");
		}
	}
}

#[test]
fn leaves_tips_without_the_marker_untouched() {
	let tip = "Run /doctor if something looks off.";
	let lines = tip_lines(tip, 80, 0.0);
	let plain = plain(&lines).join("\n");
	assert!(plain.contains(tip), "{plain:?}");
	assert!(!plain.contains("NEW!"), "{plain:?}");
	assert!(!lines.join("\n").contains("\x1b[1m"), "{lines:?}");
}
