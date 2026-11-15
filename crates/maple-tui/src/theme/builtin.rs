//! Embedded built-in themes.
//!
//! Ported from `l/src/theme/{dark,light}.json` plus the `BUILTIN_THEMES` table
//! in `l/src/theme/loader.ts`. The upstream table also spreads `defaults.json`
//! (100 extra themes); this port ships only `dark` and `light` — see
//! [`builtin_theme_json`] for the gap.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::theme::schema::ThemeJson;

/// Exact contents of `l/src/theme/dark.json`.
const DARK_JSON: &str = r##"{
	"$schema": "https://raw.githubusercontent.com/can1357/oh-my-pi/main/packages/coding-agent/theme-schema.json",
	"name": "dark",
	"vars": {
		"cyan": "#0088fa",
		"blue": "#178fb9",
		"green": "#89d281",
		"red": "#fc3a4b",
		"yellow": "#e4c00f",
		"gray": "#777d88",
		"dimGray": "#5f6673",
		"darkGray": "#3d424a",
		"accent": "#febc38",
		"selectedBg": "#31363f",
		"userMsgBg": "#221d1a",
		"toolPendingBg": "#1d2129",
		"toolSuccessBg": "#161a1f",
		"toolErrorBg": "#291d1d",
		"customMsgBg": "#2a2530"
	},
	"colors": {
		"accent": "accent",
		"border": "blue",
		"borderAccent": "cyan",
		"borderMuted": "darkGray",
		"success": "green",
		"error": "red",
		"warning": "yellow",
		"muted": "gray",
		"dim": "dimGray",
		"text": "",
		"thinkingText": "gray",
		"selectedBg": "selectedBg",
		"userMessageBg": "userMsgBg",
		"userMessageText": "",
		"customMessageBg": "customMsgBg",
		"customMessageText": "",
		"customMessageLabel": "#b281d6",
		"toolPendingBg": "toolPendingBg",
		"toolSuccessBg": "toolSuccessBg",
		"toolErrorBg": "toolErrorBg",
		"toolTitle": "",
		"toolOutput": "gray",
		"mdHeading": "#febc38",
		"mdLink": "#0088fa",
		"mdLinkUrl": "dimGray",
		"mdCode": "#e5c1ff",
		"mdCodeBlock": "#9CDCFE",
		"mdCodeBlockBorder": "gray",
		"mdQuote": "gray",
		"mdQuoteBorder": "darkGray",
		"mdHr": "darkGray",
		"mdListBullet": "accent",
		"toolDiffAdded": "green",
		"toolDiffRemoved": "red",
		"toolDiffContext": "gray",
		"link": "#0088fa",
		"syntaxComment": "#6A9955",
		"syntaxKeyword": "#569CD6",
		"syntaxFunction": "#DCDCAA",
		"syntaxVariable": "#9CDCFE",
		"syntaxString": "#CE9178",
		"syntaxNumber": "#B5CEA8",
		"syntaxType": "#4EC9B0",
		"syntaxOperator": "#D4D4D4",
		"syntaxPunctuation": "#D4D4D4",
		"thinkingOff": "darkGray",
		"thinkingMinimal": "dimGray",
		"thinkingLow": "#178fb9",
		"thinkingMedium": "#0088fa",
		"thinkingHigh": "#b281d6",
		"thinkingXhigh": "#e5c1ff",
		"bashMode": "cyan",
		"statusLineBg": "#121212",
		"statusLineSep": 244,
		"statusLineModel": "#d787af",
		"statusLinePath": "#00afaf",
		"statusLineGitClean": "#5faf5f",
		"statusLineGitDirty": "#d7af5f",
		"statusLineContext": "#8787af",
		"statusLineSpend": "#5fafaf",
		"statusLineStaged": 70,
		"statusLineDirty": 178,
		"statusLineUntracked": 39,
		"statusLineOutput": 205,
		"statusLineCost": 205,
		"statusLineSubagents": "accent",
		"pythonMode": "yellow"
	},
	"export": {
		"pageBg": "#18181e",
		"cardBg": "#1e1e24",
		"infoBg": "#26262e"
	},
	"terminal": {
		"background": "#18181e",
		"foreground": "#cccccc",
		"chrome": "#121212",
		"widget": "#1e1e24",
		"ansi": [
			"#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
			"#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#e5e5e5"
		]
	}
}"##;

/// Exact contents of `l/src/theme/light.json`.
const LIGHT_JSON: &str = r##"{
	"$schema": "https://raw.githubusercontent.com/can1357/oh-my-pi/main/packages/coding-agent/theme-schema.json",
	"name": "light",
	"vars": {
		"teal": "#5a8080",
		"blue": "#547da7",
		"green": "#588458",
		"red": "#aa5555",
		"yellow": "#9a7326",
		"mediumGray": "#6c6c6c",
		"dimGray": "#767676",
		"lightGray": "#b0b0b0",
		"selectedBg": "#d0d0e0",
		"userMsgBg": "#e8e8e8",
		"toolPendingBg": "#e8e8f0",
		"toolSuccessBg": "#e8f0e8",
		"toolErrorBg": "#f0e8e8",
		"customMsgBg": "#ede7f6"
	},
	"colors": {
		"accent": "teal",
		"border": "blue",
		"borderAccent": "teal",
		"borderMuted": "lightGray",
		"success": "green",
		"error": "red",
		"warning": "yellow",
		"muted": "mediumGray",
		"dim": "dimGray",
		"text": "",
		"thinkingText": "mediumGray",
		"selectedBg": "selectedBg",
		"userMessageBg": "userMsgBg",
		"userMessageText": "",
		"customMessageBg": "customMsgBg",
		"customMessageText": "",
		"customMessageLabel": "#7e57c2",
		"toolPendingBg": "toolPendingBg",
		"toolSuccessBg": "toolSuccessBg",
		"toolErrorBg": "toolErrorBg",
		"toolTitle": "",
		"toolOutput": "mediumGray",
		"mdHeading": "yellow",
		"mdLink": "blue",
		"mdLinkUrl": "dimGray",
		"mdCode": "teal",
		"mdCodeBlock": "green",
		"mdCodeBlockBorder": "mediumGray",
		"mdQuote": "mediumGray",
		"mdQuoteBorder": "mediumGray",
		"mdHr": "mediumGray",
		"mdListBullet": "green",
		"toolDiffAdded": "green",
		"toolDiffRemoved": "red",
		"toolDiffContext": "mediumGray",
		"syntaxComment": "#008000",
		"syntaxKeyword": "#0000FF",
		"syntaxFunction": "#795E26",
		"syntaxVariable": "#001080",
		"syntaxString": "#A31515",
		"syntaxNumber": "#098658",
		"syntaxType": "#267F99",
		"syntaxOperator": "#000000",
		"syntaxPunctuation": "#000000",
		"thinkingOff": "lightGray",
		"thinkingMinimal": "#767676",
		"thinkingLow": "blue",
		"thinkingMedium": "teal",
		"thinkingHigh": "#875f87",
		"thinkingXhigh": "#8b008b",
		"bashMode": "green",
		"statusLineBg": "#e0e0e0",
		"statusLineSep": "#808080",
		"statusLineModel": "#875f87",
		"statusLinePath": "#005f87",
		"statusLineGitClean": "#005f00",
		"statusLineGitDirty": "#af5f00",
		"statusLineContext": "#5f5f87",
		"statusLineSpend": "#005f5f",
		"statusLineStaged": 28,
		"statusLineDirty": 136,
		"statusLineUntracked": 31,
		"statusLineOutput": 133,
		"statusLineCost": 133,
		"statusLineSubagents": "teal",
		"pythonMode": "yellow"
	},
	"export": {
		"pageBg": "#f8f8f8",
		"cardBg": "#ffffff",
		"infoBg": "#fffae6"
	},
	"terminal": {
		"background": "#f8f8f8",
		"foreground": "#3b3b3b",
		"chrome": "#f8f8f8",
		"widget": "#ffffff",
		"ansi": [
			"#000000", "#cd3131", "#107c10", "#919514", "#0451a5", "#bc05bc", "#0598bc", "#555555",
			"#666666", "#f14c4c", "#09a60a", "#919516", "#3b8eea", "#ce68ce", "#1a9cba", "#a5a5a5"
		]
	}
}"##;

/// Parse an embedded theme; the literals are compile-time constants.
fn parse_embedded(content: &str) -> ThemeJson {
	serde_json::from_str(content).expect("embedded builtin theme JSON is valid")
}

/// The lazily parsed built-in table.
static BUILTIN_THEMES: LazyLock<HashMap<&'static str, ThemeJson>> = LazyLock::new(|| {
	let mut map = HashMap::new();
	map.insert("dark", parse_embedded(DARK_JSON));
	map.insert("light", parse_embedded(LIGHT_JSON));
	map
});

/// Look up an embedded theme by name (`"dark"` or `"light"`).
///
/// # Gap
///
/// Upstream `BUILTIN_THEMES` also spreads `defaults.json`, contributing ~100
/// additional named themes (`amethyst`, `dark-dracula`, `light-gruvbox`, …).
/// Those values are not ported; only the two first-frame themes are available.
pub fn builtin_theme_json(name: &str) -> Option<&'static ThemeJson> {
	BUILTIN_THEMES.get(name)
}

/// Names of the embedded themes, in table order.
pub fn builtin_theme_names() -> &'static [&'static str] {
	&["dark", "light"]
}

/// True when `name` is an embedded theme.
pub fn is_builtin_theme(name: &str) -> bool {
	builtin_theme_json(name).is_some()
}

/// The embedded theme table.
pub fn get_builtin_themes() -> &'static HashMap<&'static str, ThemeJson> {
	&BUILTIN_THEMES
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn dark_and_light_parse_with_exact_tokens() {
		let dark = builtin_theme_json("dark").expect("dark");
		assert_eq!(dark.name, "dark");
		assert_eq!(dark.vars.get("accent"), Some(&crate::theme::color::ColorValue::Hex("#febc38".into())));
		assert_eq!(dark.colors.get("mdHeading"), Some(&crate::theme::color::ColorValue::Hex("#febc38".into())));
		assert_eq!(dark.colors.get("accent"), Some(&crate::theme::color::ColorValue::Hex("accent".into())));
		assert_eq!(dark.colors.get("link"), Some(&crate::theme::color::ColorValue::Hex("#0088fa".into())));
		assert_eq!(dark.colors.get("statusLineSep"), Some(&crate::theme::color::ColorValue::Index(244)));
		let terminal = dark.terminal.as_ref().unwrap();
		assert_eq!(terminal.foreground, Some(crate::theme::color::ColorValue::Hex("#cccccc".into())));
		let ansi = terminal.ansi.as_ref().unwrap();
		assert_eq!(ansi.len(), 16);
		assert_eq!(ansi[0], crate::theme::color::ColorValue::Hex("#000000".into()));
		assert_eq!(ansi[15], crate::theme::color::ColorValue::Hex("#e5e5e5".into()));
		let export = dark.export.as_ref().unwrap();
		assert_eq!(export.page_bg, Some(crate::theme::color::ColorValue::Hex("#18181e".into())));
		assert_eq!(export.card_bg, Some(crate::theme::color::ColorValue::Hex("#1e1e24".into())));
		assert_eq!(export.info_bg, Some(crate::theme::color::ColorValue::Hex("#26262e".into())));

		let light = builtin_theme_json("light").expect("light");
		assert_eq!(light.name, "light");
		assert_eq!(light.colors.get("mdHeading"), Some(&crate::theme::color::ColorValue::Hex("yellow".into())));
		assert_eq!(light.colors.get("statusLineSep"), Some(&crate::theme::color::ColorValue::Hex("#808080".into())));
		assert_eq!(light.terminal.as_ref().unwrap().ansi.as_ref().unwrap().len(), 16);
	}

	#[test]
	fn only_dark_and_light_are_builtin() {
		assert_eq!(builtin_theme_names(), &["dark", "light"]);
		assert!(is_builtin_theme("dark"));
		assert!(is_builtin_theme("light"));
		assert!(builtin_theme_json("amethyst").is_none());
	}
}
