//! The resolved [`Theme`]: per-token ANSI/hex caches, symbol access and the
//! styling primitives every renderer uses.
//!
//! Ported from `l/src/theme/theme-class.ts`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::theme::color::{
	bg_ansi, color_luma, color_to_ansi, fg_ansi, is_light_luma, relative_luminance, resolve_to_hex,
	ColorMode, ColorValue, BG_RESET, FG_RESET,
};
use crate::theme::schema::{
	is_valid_theme_bg, is_valid_theme_color, AnsiColor, ColorMap, OPTIONAL_THEME_COLOR_KEYS,
};
use crate::theme::symbols::{
	preset_symbols, spinner_frames, NormalizedSpinnerFrames, SpinnerType, SymbolMap, SymbolPreset,
};

const BACKGROUND_RESET_PATTERN: &str = "\x1b[49m";
const FULL_RESET_PATTERN: &str = "\x1b[0m";
const FOREGROUND_RESET_PATTERN: &str = "\x1b[39m";

/// Theme-derived inputs for session accent generation.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionAccentTheme {
	/// The theme accent hex whose OKLCH weight session accents adopt.
	pub accent_hex: String,
	/// The major theme colors a session accent must not hue-collide with.
	pub color_hexes: Vec<String>,
	/// Light-theme surface luminance to contrast against; `None` on dark themes.
	pub surface_luminance: Option<f64>,
}

// ============================================================================
// Language icons
// ============================================================================

/// Language/file-type name → `lang.*` symbol key. Keys are lowercased before
/// lookup.
pub fn lang_map() -> &'static [(&'static str, &'static str)] {
	&[
		("typescript", "lang.typescript"),
		("ts", "lang.typescript"),
		("tsx", "lang.typescript"),
		("javascript", "lang.javascript"),
		("js", "lang.javascript"),
		("jsx", "lang.javascript"),
		("mjs", "lang.javascript"),
		("cjs", "lang.javascript"),
		("python", "lang.python"),
		("py", "lang.python"),
		("rust", "lang.rust"),
		("rs", "lang.rust"),
		("go", "lang.go"),
		("java", "lang.java"),
		("c", "lang.c"),
		("cpp", "lang.cpp"),
		("c++", "lang.cpp"),
		("cc", "lang.cpp"),
		("cxx", "lang.cpp"),
		("csharp", "lang.csharp"),
		("cs", "lang.csharp"),
		("ruby", "lang.ruby"),
		("rb", "lang.ruby"),
		("julia", "lang.julia"),
		("jl", "lang.julia"),
		("php", "lang.php"),
		("swift", "lang.swift"),
		("kotlin", "lang.kotlin"),
		("kt", "lang.kotlin"),
		("bash", "lang.shell"),
		("sh", "lang.shell"),
		("zsh", "lang.shell"),
		("fish", "lang.shell"),
		("powershell", "lang.shell"),
		("just", "lang.shell"),
		("shell", "lang.shell"),
		("html", "lang.html"),
		("htm", "lang.html"),
		("astro", "lang.html"),
		("vue", "lang.html"),
		("svelte", "lang.html"),
		("css", "lang.css"),
		("scss", "lang.css"),
		("sass", "lang.css"),
		("less", "lang.css"),
		("json", "lang.json"),
		("yaml", "lang.yaml"),
		("yml", "lang.yaml"),
		("markdown", "lang.markdown"),
		("md", "lang.markdown"),
		("sql", "lang.sql"),
		("dockerfile", "lang.docker"),
		("docker", "lang.docker"),
		("lua", "lang.lua"),
		("text", "lang.text"),
		("txt", "lang.text"),
		("plain", "lang.text"),
		("log", "lang.log"),
		("env", "lang.env"),
		("dotenv", "lang.env"),
		("toml", "lang.toml"),
		("xml", "lang.xml"),
		("ini", "lang.ini"),
		("conf", "lang.conf"),
		("cfg", "lang.conf"),
		("config", "lang.conf"),
		("properties", "lang.conf"),
		("csv", "lang.csv"),
		("tsv", "lang.tsv"),
		("image", "lang.image"),
		("img", "lang.image"),
		("png", "lang.image"),
		("jpg", "lang.image"),
		("jpeg", "lang.image"),
		("gif", "lang.image"),
		("webp", "lang.image"),
		("svg", "lang.image"),
		("ico", "lang.image"),
		("bmp", "lang.image"),
		("tiff", "lang.image"),
		("pdf", "lang.pdf"),
		("zip", "lang.archive"),
		("tar", "lang.archive"),
		("gz", "lang.archive"),
		("tgz", "lang.archive"),
		("bz2", "lang.archive"),
		("xz", "lang.archive"),
		("7z", "lang.archive"),
		("exe", "lang.binary"),
		("dll", "lang.binary"),
		("so", "lang.binary"),
		("dylib", "lang.binary"),
		("wasm", "lang.binary"),
		("bin", "lang.binary"),
	]
}

/// Brand colors for language icons, keyed by `lang.*` symbol key.
pub const LANG_BRAND_COLORS: &[(&str, &str)] = &[
	("lang.javascript", "#f7df1e"),
	("lang.python", "#3776ab"),
	("lang.ruby", "#cc342d"),
	("lang.julia", "#9558b2"),
];

/// Map a thinking/effort level name (`off`…`max`) to its color token; unknown
/// levels map to `thinkingOff`.
pub fn thinking_level_token(level: &str) -> &'static str {
	match level {
		"minimal" => "thinkingMinimal",
		"low" => "thinkingLow",
		"medium" => "thinkingMedium",
		"high" => "thinkingHigh",
		"xhigh" => "thinkingXhigh",
		"max" => "thinkingMax",
		_ => "thinkingOff",
	}
}

/// Re-apply `ansi` after every foreground reset (`\x1b[0m`/`\x1b[39m`) in
/// `text`, then close with a foreground reset.
fn wrap_foreground(ansi: &str, text: &str) -> String {
	let body = if text.contains("\x1b[") {
		reapply_after_resets(text, ansi, &[FULL_RESET_PATTERN, FOREGROUND_RESET_PATTERN])
	} else {
		text.to_string()
	};
	format!("{}{}{}", ansi, body, FG_RESET)
}

/// Replace `reset` sequences with themselves followed by `ansi`.
fn reapply_after_resets(text: &str, ansi: &str, resets: &[&str]) -> String {
    let mut out = String::with_capacity(text.len() + ansi.len());
    let mut rest = text;
    while let Some(pos) = rest.find("\x1b[") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        match resets.iter().find(|reset| tail.starts_with(**reset)) {
            Some(reset) => {
                out.push_str(reset);
                out.push_str(ansi);
                rest = &tail[reset.len()..];
            }
            None => {
                out.push_str("\x1b[");
                rest = &tail[2..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ============================================================================
// Symbol groups
// ============================================================================

/// Status indicator glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusSymbols {
	/// Successful completion.
	pub success: String,
	/// Failure.
	pub error: String,
	/// Warning.
	pub warning: String,
	/// Neutral information.
	pub info: String,
	/// Waiting.
	pub pending: String,
	/// Turned off.
	pub disabled: String,
	/// Turned on.
	pub enabled: String,
	/// In progress.
	pub running: String,
	/// De-emphasized.
	pub shadowed: String,
	/// Stopped early.
	pub aborted: String,
	/// Deprecated alias of `success`.
	pub done: String,
}

/// Navigation glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavSymbols {
	/// Current-line cursor.
	pub cursor: String,
	/// Selected row.
	pub selected: String,
	/// Expand control.
	pub expand: String,
	/// Collapse control.
	pub collapse: String,
	/// Back affordance.
	pub back: String,
}

/// Tree connector glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeSymbols {
	/// Mid-tree branch.
	pub branch: String,
	/// Last-child branch.
	pub last: String,
	/// Vertical guide.
	pub vertical: String,
	/// Horizontal guide.
	pub horizontal: String,
	/// Hook.
	pub hook: String,
}

/// Progress bar glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressSymbols {
	/// Filled cell.
	pub filled: String,
	/// Empty cell.
	pub empty: String,
}

/// Rounded box glyphs; junctions reuse the sharp set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxRoundSymbols {
	/// Top-left corner.
	pub top_left: String,
	/// Top-right corner.
	pub top_right: String,
	/// Bottom-left corner.
	pub bottom_left: String,
	/// Bottom-right corner.
	pub bottom_right: String,
	/// Horizontal rule.
	pub horizontal: String,
	/// Vertical rule.
	pub vertical: String,
	/// Cross junction (from the sharp set).
	pub cross: String,
	/// Tee pointing down (from the sharp set).
	pub tee_down: String,
	/// Tee pointing up (from the sharp set).
	pub tee_up: String,
	/// Tee pointing right (from the sharp set).
	pub tee_right: String,
	/// Tee pointing left (from the sharp set).
	pub tee_left: String,
}

/// Dotted selection-outline glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxDottedSymbols {
	/// Horizontal dotted rule.
	pub horizontal: String,
	/// Vertical dotted rule.
	pub vertical: String,
}

/// Sharp box glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxSharpSymbols {
	/// Top-left corner.
	pub top_left: String,
	/// Top-right corner.
	pub top_right: String,
	/// Bottom-left corner.
	pub bottom_left: String,
	/// Bottom-right corner.
	pub bottom_right: String,
	/// Horizontal rule.
	pub horizontal: String,
	/// Vertical rule.
	pub vertical: String,
	/// Cross junction.
	pub cross: String,
	/// Tee pointing down.
	pub tee_down: String,
	/// Tee pointing up.
	pub tee_up: String,
	/// Tee pointing right.
	pub tee_right: String,
	/// Tee pointing left.
	pub tee_left: String,
}

/// Separator glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SepSymbols {
	/// Powerline block.
	pub powerline: String,
	/// Thin powerline.
	pub powerline_thin: String,
	/// Left-pointing powerline.
	pub powerline_left: String,
	/// Right-pointing powerline.
	pub powerline_right: String,
	/// Thin left powerline.
	pub powerline_thin_left: String,
	/// Thin right powerline.
	pub powerline_thin_right: String,
	/// Soft band opening cap.
	pub powerline_cap_left: String,
	/// Solid block.
	pub block: String,
	/// Space.
	pub space: String,
	/// ASCII left chevron.
	pub ascii_left: String,
	/// ASCII right chevron.
	pub ascii_right: String,
	/// Middle-dot separator.
	pub dot: String,
	/// Slash separator.
	pub slash: String,
	/// Padded pipe separator.
	pub pipe: String,
}

/// UI icons.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconSymbols {
	/// Model.
	pub model: String,
	/// Plan mode.
	pub plan: String,
	/// Prewalk mode.
	pub prewalk: String,
	/// Goal mode.
	pub goal: String,
	/// Pause.
	pub pause: String,
	/// Loop / retry.
	pub loop_symbol: String,
	/// Folder.
	pub folder: String,
	/// Worktree.
	pub worktree: String,
	/// Scratch folder.
	pub scratch_folder: String,
	/// File.
	pub file: String,
	/// Git.
	pub git: String,
	/// Branch.
	pub branch: String,
	/// Pull request.
	pub pr: String,
	/// Pin.
	pub pin: String,
	/// Tokens.
	pub tokens: String,
	/// Context.
	pub context: String,
	/// Cost.
	pub cost: String,
	/// Subscription.
	pub subscription: String,
	/// Advisor.
	pub advisor: String,
	/// Advisor closed.
	pub advisor_closed: String,
	/// Time.
	pub time: String,
	/// omp.
	pub omp: String,
	/// Ghost.
	pub ghost: String,
	/// Agents.
	pub agents: String,
	/// Background job.
	pub job: String,
	/// Cache.
	pub cache: String,
	/// Cache miss.
	pub cache_miss: String,
	/// Warm cache.
	pub cache_warm: String,
	/// Cold cache.
	pub cache_cold: String,
	/// Input.
	pub input: String,
	/// Output.
	pub output: String,
	/// Throughput.
	pub throughput: String,
	/// Host.
	pub host: String,
	/// Session.
	pub session: String,
	/// Package.
	pub package: String,
	/// Warning.
	pub warning: String,
	/// Rewind.
	pub rewind: String,
	/// Auto mode.
	pub auto: String,
	/// Fast mode.
	pub fast: String,
	/// Extension skill.
	pub extension_skill: String,
	/// Extension tool.
	pub extension_tool: String,
	/// Extension slash command.
	pub extension_slash_command: String,
	/// Extension MCP.
	pub extension_mcp: String,
	/// Extension rule.
	pub extension_rule: String,
	/// Extension hook.
	pub extension_hook: String,
	/// Extension prompt.
	pub extension_prompt: String,
	/// Extension context file.
	pub extension_context_file: String,
	/// Extension instruction.
	pub extension_instruction: String,
	/// Vim normal mode.
	pub vim_normal: String,
	/// Vim insert mode.
	pub vim_insert: String,
	/// Vim visual mode.
	pub vim_visual: String,
	/// Vim visual-line mode.
	pub vim_visual_line: String,
	/// Microphone (dictation).
	pub mic: String,
	/// Camera (compaction divider).
	pub camera: String,
}

/// Slash-command type indicators.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CmdSymbols {
	/// `/action`.
	pub action: String,
	/// `/prompt`.
	pub prompt: String,
	/// `/extension`.
	pub extension: String,
	/// `/settings`.
	pub settings: String,
	/// `/gear`.
	pub gear: String,
	/// `/shield`.
	pub shield: String,
	/// `/wave`.
	pub wave: String,
	/// `/compass`.
	pub compass: String,
	/// `/inbox`.
	pub inbox: String,
	/// `/swap`.
	pub swap: String,
	/// `/expand`.
	pub expand: String,
	/// `/computer`.
	pub computer: String,
	/// `/eye`.
	pub eye: String,
	/// `/todo`.
	pub todo: String,
	/// `/stats`.
	pub stats: String,
	/// `/news`.
	pub news: String,
	/// `/keyboard`.
	pub keyboard: String,
	/// `/export`.
	pub export: String,
	/// `/clipboard`.
	pub clipboard: String,
	/// `/share`.
	pub share: String,
	/// `/broadcast`.
	pub broadcast: String,
	/// `/globe`.
	pub globe: String,
	/// `/copy`.
	pub copy: String,
	/// `/plus`.
	pub plus: String,
	/// `/restart`.
	pub restart: String,
	/// `/eraser`.
	pub eraser: String,
	/// `/trash`.
	pub trash: String,
	/// `/compress`.
	pub compress: String,
	/// `/vibrate`.
	pub vibrate: String,
	/// `/handoff`.
	pub handoff: String,
	/// `/history`.
	pub history: String,
	/// `/question`.
	pub question: String,
	/// `/rocket`.
	pub rocket: String,
	/// `/stethoscope`.
	pub stethoscope: String,
	/// `/redo`.
	pub redo: String,
	/// `/bug`.
	pub bug: String,
	/// `/memory`.
	pub memory: String,
	/// `/pencil`.
	pub pencil: String,
	/// `/folder-move`.
	pub folder_move: String,
	/// `/folder-plus`.
	pub folder_plus: String,
	/// `/folder-minus`.
	pub folder_minus: String,
	/// `/hammer`.
	pub hammer: String,
	/// `/power`.
	pub power: String,
	/// `/cart`.
	pub cart: String,
	/// Model (shared `icon.model`).
	pub model: String,
	/// Plan (shared `icon.plan`).
	pub plan: String,
	/// Prewalk (shared `icon.prewalk`).
	pub prewalk: String,
	/// Goal (shared `icon.goal`).
	pub goal: String,
	/// Pause (shared `icon.pause`).
	pub pause: String,
	/// Loop (shared `icon.loop`).
	pub loop_symbol: String,
	/// Session (shared `icon.session`).
	pub session: String,
	/// Jobs (shared `icon.job`).
	pub jobs: String,
	/// Gauge (shared `icon.throughput`).
	pub gauge: String,
	/// Context (shared `icon.context`).
	pub context: String,
	/// Ghost (shared `icon.ghost`).
	pub ghost: String,
	/// Agents (shared `icon.agents`).
	pub agents: String,
	/// Branch (shared `icon.branch`).
	pub branch: String,
	/// Tree (shared `icon.worktree`).
	pub tree: String,
	/// Sign in (shared `icon.input`).
	pub sign_in: String,
	/// Sign out (shared `icon.output`).
	pub sign_out: String,
	/// Advisor (shared `icon.advisor`).
	pub advisor: String,
	/// Host (shared `icon.host`).
	pub host: String,
	/// Package (shared `icon.package`).
	pub package: String,
	/// Fast (shared `icon.fast`).
	pub fast: String,
	/// Voice (shared `icon.mic`).
	pub voice: String,
	/// Tools (shared `icon.extensionTool`).
	pub tools: String,
	/// Rule (shared `icon.extensionRule`).
	pub rule: String,
	/// Skill (shared `icon.extensionSkill`).
	pub skill: String,
	/// MCP (shared `icon.extensionMcp`).
	pub mcp: String,
	/// Pin (shared `icon.pin`).
	pub pin: String,
}

/// Thinking-level glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThinkingSymbols {
	/// Minimal effort.
	pub minimal: String,
	/// Low effort.
	pub low: String,
	/// Medium effort.
	pub medium: String,
	/// High effort.
	pub high: String,
	/// Extra-high effort.
	pub xhigh: String,
	/// Max effort.
	pub max: String,
	/// Auto mode pending a level.
	pub auto_pending: String,
}

/// Checkbox glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckboxSymbols {
	/// Checked box.
	pub checked: String,
	/// Unchecked box.
	pub unchecked: String,
}

/// Radio-button glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioSymbols {
	/// Selected option.
	pub selected: String,
	/// Unselected option.
	pub unselected: String,
}

/// Text-formatting glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatSymbols {
	/// Bullet.
	pub bullet: String,
	/// Dash.
	pub dash: String,
	/// Left bracket.
	pub bracket_left: String,
	/// Right bracket.
	pub bracket_right: String,
}

/// Markdown glyphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MdSymbols {
	/// Quote rail.
	pub quote_border: String,
	/// Horizontal rule.
	pub hr_char: String,
	/// Bullet.
	pub bullet: String,
	/// Color swatch.
	pub color_swatch: String,
}

// ============================================================================
// Theme
// ============================================================================

/// A fully resolved theme: every color token mapped to foreground/background
/// SGR and hex, plus the active symbol preset with overrides applied.
#[derive(Clone)]
pub struct Theme {
	fg_colors: HashMap<String, String>,
	bg_colors: HashMap<String, String>,
	hex_fg: HashMap<String, String>,
	hex_bg: HashMap<String, String>,
	fg_order: Vec<String>,
	bg_order: Vec<String>,
	ansi_fg_colors: Vec<String>,
	resolved_fg: RefCell<HashMap<String, String>>,
	session_accent_inputs: RefCell<Option<SessionAccentTheme>>,
	symbols: HashMap<String, String>,
	status_spinner_frames: Vec<String>,
	activity_spinner_frames: Vec<String>,
	mode: ColorMode,
	symbol_preset: SymbolPreset,
	status_line_luminance: Option<f64>,
	status_line_contrast_luminance: Option<f64>,
}

impl Theme {
	/// Resolve raw color maps into a theme.
	///
	/// `fg_colors`/`bg_colors` come from [`crate::theme::loader::create_theme`],
	/// which splits the theme JSON by the exact background-key set.
	pub fn new(
		fg_colors: &ColorMap,
		bg_colors: &ColorMap,
		mode: ColorMode,
		symbol_preset: SymbolPreset,
		symbol_overrides: &HashMap<String, String>,
		spinner_frames_overrides: &NormalizedSpinnerFrames,
		ansi_palette: &[ColorValue],
	) -> Result<Theme, String> {
		let status_line_luminance = bg_colors.get("statusLineBg").and_then(color_luma);
		let status_line_contrast_luminance = bg_colors.get("statusLineBg").and_then(relative_luminance);
		let sl_is_light = is_light_luma(status_line_luminance);

		let mut fg = HashMap::new();
		let mut hex_fg = HashMap::new();
		let mut fg_order = Vec::new();
		for (key, value) in fg_colors.iter() {
			if !is_valid_theme_color(key) {
				continue;
			}
			let hex = resolve_to_hex(value, sl_is_light)?;
			let ansi = fg_ansi(value, mode)?;
			fg.insert(key.to_string(), ansi);
			hex_fg.insert(key.to_string(), hex);
			fg_order.push(key.to_string());
		}
		for key in OPTIONAL_THEME_COLOR_KEYS {
			fg.entry((*key).to_string()).or_insert_with(|| FG_RESET.to_string());
			hex_fg.entry((*key).to_string()).or_insert_with(String::new);
		}

		let mut ansi_fg_colors = Vec::with_capacity(16);
		for index in 0..16 {
			let value = ansi_palette
				.get(index)
				.cloned()
				.unwrap_or(ColorValue::Index(index as i64));
			ansi_fg_colors.push(fg_ansi(&value, mode)?);
		}

		let mut bg = HashMap::new();
		let mut hex_bg = HashMap::new();
		let mut bg_order = Vec::new();
		for (key, value) in bg_colors.iter() {
			if !is_valid_theme_bg(key) {
				continue;
			}
			let ansi = bg_ansi(value, mode)?;
			let hex = resolve_to_hex(value, sl_is_light)?;
			bg.insert(key.to_string(), ansi);
			hex_bg.insert(key.to_string(), hex);
			bg_order.push(key.to_string());
		}

		let mut symbols = HashMap::new();
		let base: SymbolMap = preset_symbols(symbol_preset);
		for (key, value) in base.iter() {
			symbols.insert(key.to_string(), value.to_string());
		}
		for (key, value) in symbol_overrides {
			if symbols.contains_key(key) {
				symbols.insert(key.clone(), value.clone());
			}
		}
		if !symbol_overrides.contains_key("status.done") {
			if let Some(success) = symbols.get("status.success").cloned() {
				symbols.insert("status.done".to_string(), success);
			}
		}

		let status_spinner_frames = spinner_frames_overrides
			.status
			.clone()
			.unwrap_or_else(|| preset_frames(symbol_preset, SpinnerType::Status));
		let activity_spinner_frames = spinner_frames_overrides
			.activity
			.clone()
			.unwrap_or_else(|| preset_frames(symbol_preset, SpinnerType::Activity));

		Ok(Theme {
			fg_colors: fg,
			bg_colors: bg,
			hex_fg,
			hex_bg,
			fg_order,
			bg_order,
			ansi_fg_colors,
			resolved_fg: RefCell::new(HashMap::new()),
			session_accent_inputs: RefCell::new(None),
			symbols,
			status_spinner_frames,
			activity_spinner_frames,
			mode,
			symbol_preset,
			status_line_luminance,
			status_line_contrast_luminance,
		})
	}

	/// True when the theme has a light status-line background.
	pub fn is_light(&self) -> bool {
		is_light_luma(self.status_line_luminance)
	}

	/// Surface luminance to size session accents against on light themes.
	pub fn accent_surface_luminance(&self) -> Option<f64> {
		if self.is_light() {
			self.status_line_contrast_luminance
		} else {
			None
		}
	}

	/// The resolved CSS hex for a foreground color token.
	pub fn get_color_hex(&self, color: &str) -> Result<String, String> {
		match self.hex_fg.get(color) {
			Some(hex) => Ok(if hex.is_empty() {
				if self.is_light() { "#000000" } else { "#e5e5e7" }.to_string()
			} else {
				hex.clone()
			}),
			None => Err(format!("Unknown theme color: {}", color)),
		}
	}

	/// The resolved CSS hex for a background color token.
	pub fn get_bg_hex(&self, color: &str) -> Result<String, String> {
		match self.hex_bg.get(color) {
			Some(hex) => Ok(if hex.is_empty() {
				if self.is_light() { "#ffffff" } else { "#000000" }.to_string()
			} else {
				hex.clone()
			}),
			None => Err(format!("Unknown theme background color: {}", color)),
		}
	}

	/// All non-empty foreground then background hexes, in declaration order.
	pub fn get_all_theme_color_hexes(&self) -> Vec<String> {
		let mut hexes = Vec::new();
		for key in &self.fg_order {
			if let Some(hex) = self.hex_fg.get(key) {
				if !hex.is_empty() {
					hexes.push(hex.clone());
				}
			}
		}
		for key in &self.bg_order {
			if let Some(hex) = self.hex_bg.get(key) {
				if !hex.is_empty() {
					hexes.push(hex.clone());
				}
			}
		}
		hexes
	}

	/// The visually dominant theme colors, in the exact order session accents
	/// expect; empty (terminal-default) colors are skipped.
	pub fn get_major_theme_color_hexes(&self) -> Vec<String> {
		const MAJORS: &[&str] = &[
			"accent",
			"border",
			"borderAccent",
			"borderMuted",
			"success",
			"error",
			"warning",
			"mdHeading",
			"mdLink",
			"mdCode",
			"mdCodeBlock",
			"mdQuoteBorder",
			"mdListBullet",
			"toolDiffAdded",
			"toolDiffRemoved",
			"customMessageLabel",
			"thinkingText",
		];
		let mut hexes = Vec::new();
		for key in MAJORS {
			if let Some(hex) = self.hex_fg.get(*key) {
				if !hex.is_empty() {
					hexes.push(hex.clone());
				}
			}
		}
		hexes
	}

	/// The resolved accent hex.
	pub fn get_accent_color_hex(&self) -> Result<String, String> {
		self.get_color_hex("accent")
	}

	/// Theme-derived inputs for session accent generation, built once.
	pub fn get_session_accent_inputs(&self) -> SessionAccentTheme {
		let mut cached = self.session_accent_inputs.borrow_mut();
		if cached.is_none() {
			*cached = Some(SessionAccentTheme {
				accent_hex: self.get_accent_color_hex().unwrap_or_default(),
				color_hexes: self.get_major_theme_color_hexes(),
				surface_luminance: self.accent_surface_luminance(),
			});
		}
		cached.as_ref().expect("just populated").clone()
	}

	/// True when the theme paints `color` with a real foreground.
	pub fn has_color(&self, color: &str) -> bool {
		matches!(self.fg_colors.get(color), Some(ansi) if ansi != FG_RESET)
	}

	/// Color `text` with a foreground token.
	pub fn fg<C: AsRef<str>>(&self, color: C, text: &str) -> Result<String, String> {
		let color = color.as_ref();
		let ansi = self
			.fg_colors
			.get(color)
			.ok_or_else(|| format!("Unknown theme color: {}", color))?;
		Ok(wrap_foreground(ansi, text))
	}

	/// Color `text` with a slot of the theme's 16-color ANSI palette.
	pub fn ansi_fg(&self, color: AnsiColor, text: &str) -> String {
		wrap_foreground(&self.ansi_fg_colors[color.index()], text)
	}

	/// Apply a foreground, replacing terminal-default tokens with the theme's
	/// contrast-safe fallback.
	pub fn fg_resolved<C: AsRef<str>>(&self, color: C, text: &str) -> Result<String, String> {
		let color = color.as_ref();
		let mut cache = self.resolved_fg.borrow_mut();
		if let Some(resolved) = cache.get(color) {
			return Ok(wrap_foreground(resolved, text));
		}
		let ansi = self
			.fg_colors
			.get(color)
			.ok_or_else(|| format!("Unknown theme color: {}", color))?
			.clone();
		let resolved = if ansi == FG_RESET {
			color_to_ansi(&self.get_color_hex(color)?, self.mode)?
		} else {
			ansi
		};
		cache.insert(color.to_string(), resolved.clone());
		drop(cache);
		Ok(wrap_foreground(&resolved, text))
	}

	/// Color `text` with a background token.
	pub fn bg<B: AsRef<str>>(&self, background: B, text: &str) -> Result<String, String> {
		let background = background.as_ref();
		let ansi = self
			.bg_colors
			.get(background)
			.ok_or_else(|| format!("Unknown theme background color: {}", background))?;
		Ok(format!("{}{}{}", ansi, text, BG_RESET))
	}

	/// Apply a background fill that resumes after nested full/background resets.
	pub fn bg_fill<B: AsRef<str>>(&self, background: B, text: &str) -> Result<String, String> {
		let background = background.as_ref();
		let ansi = self
			.bg_colors
			.get(background)
			.ok_or_else(|| format!("Unknown theme background color: {}", background))?;
		let body = reapply_after_resets(text, ansi, &[FULL_RESET_PATTERN, BACKGROUND_RESET_PATTERN]);
		Ok(format!("{}{}{}", ansi, body, BG_RESET))
	}

	/// Apply a foreground over a controlled background, choosing a
	/// contrast-safe fallback when the token requests the terminal default.
	pub fn fg_on_bg<C: AsRef<str>, B: AsRef<str>>(
		&self,
		color: C,
		background: B,
		text: &str,
	) -> Result<String, String> {
		let ansi = self.get_fg_on_bg_ansi(color, background)?;
		Ok(wrap_foreground(&ansi, text))
	}

	/// Bold text (`\x1b[1m` … `\x1b[22m`).
	pub fn bold(&self, text: &str) -> String {
		format!("\x1b[1m{}\x1b[22m", text)
	}

	/// Dim text without replacing the foreground color.
	pub fn faint(&self, text: &str) -> String {
		format!("\x1b[2m{}\x1b[22m", text)
	}

	/// Italic text.
	pub fn italic(&self, text: &str) -> String {
		format!("\x1b[3m{}\x1b[23m", text)
	}

	/// Underlined text.
	pub fn underline(&self, text: &str) -> String {
		format!("\x1b[4m{}\x1b[24m", text)
	}

	/// Strikethrough text.
	pub fn strikethrough(&self, text: &str) -> String {
		format!("\x1b[9m{}\x1b[29m", text)
	}

	/// Inverted text.
	pub fn inverse(&self, text: &str) -> String {
		format!("\x1b[7m{}\x1b[27m", text)
	}

	/// Raw foreground SGR for a color token.
	pub fn get_fg_ansi<C: AsRef<str>>(&self, color: C) -> Result<String, String> {
		let color = color.as_ref();
		self.fg_colors
			.get(color)
			.cloned()
			.ok_or_else(|| format!("Unknown theme color: {}", color))
	}

	/// Raw background SGR for a color token.
	pub fn get_bg_ansi<B: AsRef<str>>(&self, background: B) -> Result<String, String> {
		let background = background.as_ref();
		self.bg_colors
			.get(background)
			.cloned()
			.ok_or_else(|| format!("Unknown theme background color: {}", background))
	}

	/// Foreground SGR for text drawn over a controlled theme background.
	pub fn get_fg_on_bg_ansi<C: AsRef<str>, B: AsRef<str>>(
		&self,
		color: C,
		background: B,
	) -> Result<String, String> {
		let color = color.as_ref();
		let background = background.as_ref();
		let ansi = self
			.fg_colors
			.get(color)
			.ok_or_else(|| format!("Unknown theme color: {}", color))?;
		if ansi != FG_RESET {
			return Ok(ansi.clone());
		}
		let background_ansi = self
			.bg_colors
			.get(background)
			.ok_or_else(|| format!("Unknown theme background color: {}", background))?;
		if background_ansi == BG_RESET {
			return Ok(ansi.clone());
		}
		let luma = match self.get_bg_hex(background) {
			Ok(hex) => color_luma(&ColorValue::Hex(hex)),
			Err(_) => None,
		};
		let fallback = match luma {
			Some(luma) if luma > 0.5 => "#000000",
			_ => "#e5e5e7",
		};
		color_to_ansi(fallback, self.mode)
	}

	/// Foreground SGR for a label drawn on a solid fill color.
	pub fn get_contrast_fg_ansi(&self, fill_color: &str) -> String {
		let Some(ansi) = self.fg_colors.get(fill_color) else {
			return self.fg_colors.get("text").cloned().unwrap_or_default();
		};
		let Some(rgb) = parse_truecolor_sgr(ansi) else {
			return self.fg_colors.get("text").cloned().unwrap_or_default();
		};
		let luma = 0.299 * rgb.0 as f64 + 0.587 * rgb.1 as f64 + 0.114 * rgb.2 as f64;
		if luma > 140.0 {
			"\x1b[38;2;0;0;0m".to_string()
		} else {
			"\x1b[38;2;255;255;255m".to_string()
		}
	}

	/// The active color mode.
	pub fn get_color_mode(&self) -> ColorMode {
		self.mode
	}

	/// Border styler for a thinking/effort level.
	pub fn get_thinking_border_color<'a>(
		&'a self,
		level: &str,
	) -> Box<dyn Fn(&str) -> Result<String, String> + 'a> {
		let token = thinking_level_token(level);
		let color = if token == "thinkingMax" && !self.fg_colors.contains_key("thinkingMax") {
			"thinkingXhigh"
		} else {
			token
		};
		Box::new(move |text: &str| self.fg(color, text))
	}

	/// Border styler for bash mode.
	pub fn get_bash_mode_border_color<'a>(
		&'a self,
	) -> Box<dyn Fn(&str) -> Result<String, String> + 'a> {
		Box::new(move |text: &str| self.fg("bashMode", text))
	}

	/// Border styler for python mode.
	pub fn get_python_mode_border_color<'a>(
		&'a self,
	) -> Box<dyn Fn(&str) -> Result<String, String> + 'a> {
		Box::new(move |text: &str| self.fg("pythonMode", text))
	}

	/// A symbol by key (empty string for an unknown key).
	pub fn symbol(&self, key: &str) -> &str {
		self.symbols.get(key).map(String::as_str).unwrap_or("")
	}

	/// A symbol painted with a color token.
	pub fn styled_symbol<C: AsRef<str>>(&self, key: &str, color: C) -> Result<String, String> {
		let icon = self.symbol(key);
		self.fg(color, icon)
	}

	/// The active symbol preset.
	pub fn get_symbol_preset(&self) -> SymbolPreset {
		self.symbol_preset
	}

	/// Status indicator glyphs.
	pub fn status(&self) -> StatusSymbols {
		StatusSymbols {
			success: self.sym("status.success"),
			error: self.sym("status.error"),
			warning: self.sym("status.warning"),
			info: self.sym("status.info"),
			pending: self.sym("status.pending"),
			disabled: self.sym("status.disabled"),
			enabled: self.sym("status.enabled"),
			running: self.sym("status.running"),
			shadowed: self.sym("status.shadowed"),
			aborted: self.sym("status.aborted"),
			done: self.sym("status.done"),
		}
	}

	/// Navigation glyphs.
	pub fn nav(&self) -> NavSymbols {
		NavSymbols {
			cursor: self.sym("nav.cursor"),
			selected: self.sym("nav.selected"),
			expand: self.sym("nav.expand"),
			collapse: self.sym("nav.collapse"),
			back: self.sym("nav.back"),
		}
	}

	/// Tree connector glyphs.
	pub fn tree(&self) -> TreeSymbols {
		TreeSymbols {
			branch: self.sym("tree.branch"),
			last: self.sym("tree.last"),
			vertical: self.sym("tree.vertical"),
			horizontal: self.sym("tree.horizontal"),
			hook: self.sym("tree.hook"),
		}
	}

	/// Progress bar glyphs.
	pub fn progress(&self) -> ProgressSymbols {
		ProgressSymbols {
			filled: self.sym("progress.filled"),
			empty: self.sym("progress.empty"),
		}
	}

	/// Rounded box glyphs.
	pub fn box_round(&self) -> BoxRoundSymbols {
		BoxRoundSymbols {
			top_left: self.sym("boxRound.topLeft"),
			top_right: self.sym("boxRound.topRight"),
			bottom_left: self.sym("boxRound.bottomLeft"),
			bottom_right: self.sym("boxRound.bottomRight"),
			horizontal: self.sym("boxRound.horizontal"),
			vertical: self.sym("boxRound.vertical"),
			cross: self.sym("boxSharp.cross"),
			tee_down: self.sym("boxSharp.teeDown"),
			tee_up: self.sym("boxSharp.teeUp"),
			tee_right: self.sym("boxSharp.teeRight"),
			tee_left: self.sym("boxSharp.teeLeft"),
		}
	}

	/// Dotted selection-outline glyphs.
	pub fn box_dotted(&self) -> BoxDottedSymbols {
		BoxDottedSymbols {
			horizontal: self.sym("boxDotted.horizontal"),
			vertical: self.sym("boxDotted.vertical"),
		}
	}

	/// Sharp box glyphs.
	pub fn box_sharp(&self) -> BoxSharpSymbols {
		BoxSharpSymbols {
			top_left: self.sym("boxSharp.topLeft"),
			top_right: self.sym("boxSharp.topRight"),
			bottom_left: self.sym("boxSharp.bottomLeft"),
			bottom_right: self.sym("boxSharp.bottomRight"),
			horizontal: self.sym("boxSharp.horizontal"),
			vertical: self.sym("boxSharp.vertical"),
			cross: self.sym("boxSharp.cross"),
			tee_down: self.sym("boxSharp.teeDown"),
			tee_up: self.sym("boxSharp.teeUp"),
			tee_right: self.sym("boxSharp.teeRight"),
			tee_left: self.sym("boxSharp.teeLeft"),
		}
	}

	/// Separator glyphs.
	pub fn sep(&self) -> SepSymbols {
		SepSymbols {
			powerline: self.sym("sep.powerline"),
			powerline_thin: self.sym("sep.powerlineThin"),
			powerline_left: self.sym("sep.powerlineLeft"),
			powerline_right: self.sym("sep.powerlineRight"),
			powerline_thin_left: self.sym("sep.powerlineThinLeft"),
			powerline_thin_right: self.sym("sep.powerlineThinRight"),
			powerline_cap_left: self.sym("sep.powerlineCapLeft"),
			block: self.sym("sep.block"),
			space: self.sym("sep.space"),
			ascii_left: self.sym("sep.asciiLeft"),
			ascii_right: self.sym("sep.asciiRight"),
			dot: self.sym("sep.dot"),
			slash: self.sym("sep.slash"),
			pipe: self.sym("sep.pipe"),
		}
	}

	/// UI icons.
	pub fn icon(&self) -> IconSymbols {
		IconSymbols {
			model: self.sym("icon.model"),
			plan: self.sym("icon.plan"),
			prewalk: self.sym("icon.prewalk"),
			goal: self.sym("icon.goal"),
			pause: self.sym("icon.pause"),
			loop_symbol: self.sym("icon.loop"),
			folder: self.sym("icon.folder"),
			worktree: self.sym("icon.worktree"),
			scratch_folder: self.sym("icon.scratchFolder"),
			file: self.sym("icon.file"),
			git: self.sym("icon.git"),
			branch: self.sym("icon.branch"),
			pr: self.sym("icon.pr"),
			pin: self.sym("icon.pin"),
			tokens: self.sym("icon.tokens"),
			context: self.sym("icon.context"),
			cost: self.sym("icon.cost"),
			subscription: self.sym("icon.subscription"),
			advisor: self.sym("icon.advisor"),
			advisor_closed: self.sym("icon.advisorClosed"),
			time: self.sym("icon.time"),
			omp: self.sym("icon.omp"),
			ghost: self.sym("icon.ghost"),
			agents: self.sym("icon.agents"),
			job: self.sym("icon.job"),
			cache: self.sym("icon.cache"),
			cache_miss: self.sym("icon.cacheMiss"),
			cache_warm: self.sym("icon.cacheWarm"),
			cache_cold: self.sym("icon.cacheCold"),
			input: self.sym("icon.input"),
			output: self.sym("icon.output"),
			throughput: self.sym("icon.throughput"),
			host: self.sym("icon.host"),
			session: self.sym("icon.session"),
			package: self.sym("icon.package"),
			warning: self.sym("icon.warning"),
			rewind: self.sym("icon.rewind"),
			auto: self.sym("icon.auto"),
			fast: self.sym("icon.fast"),
			extension_skill: self.sym("icon.extensionSkill"),
			extension_tool: self.sym("icon.extensionTool"),
			extension_slash_command: self.sym("icon.extensionSlashCommand"),
			extension_mcp: self.sym("icon.extensionMcp"),
			extension_rule: self.sym("icon.extensionRule"),
			extension_hook: self.sym("icon.extensionHook"),
			extension_prompt: self.sym("icon.extensionPrompt"),
			extension_context_file: self.sym("icon.extensionContextFile"),
			extension_instruction: self.sym("icon.extensionInstruction"),
			vim_normal: self.sym("icon.vimNormal"),
			vim_insert: self.sym("icon.vimInsert"),
			vim_visual: self.sym("icon.vimVisual"),
			vim_visual_line: self.sym("icon.vimVisualLine"),
			mic: self.sym("icon.mic"),
			camera: self.sym("icon.camera"),
		}
	}

	/// Slash-command type indicators.
	pub fn cmd(&self) -> CmdSymbols {
		CmdSymbols {
			action: self.sym("cmd.action"),
			prompt: self.sym("cmd.prompt"),
			extension: self.sym("cmd.extension"),
			settings: self.sym("cmd.settings"),
			gear: self.sym("cmd.gear"),
			shield: self.sym("cmd.shield"),
			wave: self.sym("cmd.wave"),
			compass: self.sym("cmd.compass"),
			inbox: self.sym("cmd.inbox"),
			swap: self.sym("cmd.swap"),
			expand: self.sym("cmd.expand"),
			computer: self.sym("cmd.computer"),
			eye: self.sym("cmd.eye"),
			todo: self.sym("cmd.todo"),
			stats: self.sym("cmd.stats"),
			news: self.sym("cmd.news"),
			keyboard: self.sym("cmd.keyboard"),
			export: self.sym("cmd.export"),
			clipboard: self.sym("cmd.clipboard"),
			share: self.sym("cmd.share"),
			broadcast: self.sym("cmd.broadcast"),
			globe: self.sym("cmd.globe"),
			copy: self.sym("cmd.copy"),
			plus: self.sym("cmd.plus"),
			restart: self.sym("cmd.restart"),
			eraser: self.sym("cmd.eraser"),
			trash: self.sym("cmd.trash"),
			compress: self.sym("cmd.compress"),
			vibrate: self.sym("cmd.vibrate"),
			handoff: self.sym("cmd.handoff"),
			history: self.sym("cmd.history"),
			question: self.sym("cmd.question"),
			rocket: self.sym("cmd.rocket"),
			stethoscope: self.sym("cmd.stethoscope"),
			redo: self.sym("cmd.redo"),
			bug: self.sym("cmd.bug"),
			memory: self.sym("cmd.memory"),
			pencil: self.sym("cmd.pencil"),
			folder_move: self.sym("cmd.folderMove"),
			folder_plus: self.sym("cmd.folderPlus"),
			folder_minus: self.sym("cmd.folderMinus"),
			hammer: self.sym("cmd.hammer"),
			power: self.sym("cmd.power"),
			cart: self.sym("cmd.cart"),
			model: self.sym("icon.model"),
			plan: self.sym("icon.plan"),
			prewalk: self.sym("icon.prewalk"),
			goal: self.sym("icon.goal"),
			pause: self.sym("icon.pause"),
			loop_symbol: self.sym("icon.loop"),
			session: self.sym("icon.session"),
			jobs: self.sym("icon.job"),
			gauge: self.sym("icon.throughput"),
			context: self.sym("icon.context"),
			ghost: self.sym("icon.ghost"),
			agents: self.sym("icon.agents"),
			branch: self.sym("icon.branch"),
			tree: self.sym("icon.worktree"),
			sign_in: self.sym("icon.input"),
			sign_out: self.sym("icon.output"),
			advisor: self.sym("icon.advisor"),
			host: self.sym("icon.host"),
			package: self.sym("icon.package"),
			fast: self.sym("icon.fast"),
			voice: self.sym("icon.mic"),
			tools: self.sym("icon.extensionTool"),
			rule: self.sym("icon.extensionRule"),
			skill: self.sym("icon.extensionSkill"),
			mcp: self.sym("icon.extensionMcp"),
			pin: self.sym("icon.pin"),
		}
	}

	/// Thinking-level glyphs.
	pub fn thinking(&self) -> ThinkingSymbols {
		ThinkingSymbols {
			minimal: self.sym("thinking.minimal"),
			low: self.sym("thinking.low"),
			medium: self.sym("thinking.medium"),
			high: self.sym("thinking.high"),
			xhigh: self.sym("thinking.xhigh"),
			max: self.sym("thinking.max"),
			auto_pending: self.sym("thinking.autoPending"),
		}
	}

	/// Checkbox glyphs.
	pub fn checkbox(&self) -> CheckboxSymbols {
		CheckboxSymbols {
			checked: self.sym("checkbox.checked"),
			unchecked: self.sym("checkbox.unchecked"),
		}
	}

	/// Radio-button glyphs.
	pub fn radio(&self) -> RadioSymbols {
		RadioSymbols {
			selected: self.sym("radio.selected"),
			unselected: self.sym("radio.unselected"),
		}
	}

	/// Text-formatting glyphs.
	pub fn format(&self) -> FormatSymbols {
		FormatSymbols {
			bullet: self.sym("format.bullet"),
			dash: self.sym("format.dash"),
			bracket_left: self.sym("format.bracketLeft"),
			bracket_right: self.sym("format.bracketRight"),
		}
	}

	/// Markdown glyphs.
	pub fn md(&self) -> MdSymbols {
		MdSymbols {
			quote_border: self.sym("md.quoteBorder"),
			hr_char: self.sym("md.hrChar"),
			bullet: self.sym("md.bullet"),
			color_swatch: self.sym("md.colorSwatch"),
		}
	}

	/// Default (status) spinner frames.
	pub fn spinner_frames(&self) -> &[String] {
		&self.status_spinner_frames
	}

	/// Spinner frames for a spinner type, honoring theme overrides.
	pub fn get_spinner_frames(&self, typ: SpinnerType) -> &[String] {
		match typ {
			SpinnerType::Status => &self.status_spinner_frames,
			SpinnerType::Activity => &self.activity_spinner_frames,
		}
	}

	/// The language/file-type icon for a language name.
	pub fn get_lang_icon(&self, lang: Option<&str>) -> String {
		let Some(lang) = lang.filter(|l| !l.is_empty()) else {
			return self.sym("lang.default").to_string();
		};
		let key = lang_map()
			.iter()
			.find(|(name, _)| *name == lang.to_lowercase())
			.map(|(_, key)| *key);
		match key {
			Some(key) => self.sym(key).to_string(),
			None => self.sym("lang.default").to_string(),
		}
	}

	/// Language icon tinted with its brand color, or the muted theme color when
	/// the language has no brand entry.
	pub fn get_lang_icon_styled(&self, lang: Option<&str>) -> Result<String, String> {
		let icon = self.get_lang_icon(lang);
		if icon.is_empty() {
			return Ok(icon);
		}
		let key = lang
			.filter(|l| !l.is_empty())
			.and_then(|l| {
				lang_map()
					.iter()
					.find(|(name, _)| *name == l.to_lowercase())
					.map(|(_, key)| *key)
			});
		let brand = key.and_then(|key| {
			LANG_BRAND_COLORS
				.iter()
				.find(|(k, _)| *k == key)
				.map(|(_, hex)| *hex)
		});
		match brand {
			Some(hex) => Ok(format!("{}{}{}", color_to_ansi(hex, self.mode)?, icon, FG_RESET)),
			None => self.fg("muted", &icon),
		}
	}

	/// Fetch a symbol as an owned `String`.
	fn sym(&self, key: &str) -> String {
		self.symbols.get(key).cloned().unwrap_or_default()
	}

}

fn preset_frames(preset: SymbolPreset, typ: SpinnerType) -> Vec<String> {
	spinner_frames(preset, typ).iter().map(|s| s.to_string()).collect()
}

/// Extract `R;G;B` from a `38;2;R;G;B` SGR sequence.
fn parse_truecolor_sgr(ansi: &str) -> Option<(u8, u8, u8)> {
	let rest = ansi.strip_prefix("\x1b[38;2;")?;
	let rest = rest.strip_suffix('m')?;
	let mut parts = rest.split(';');
	let r = parts.next()?.parse().ok()?;
	let g = parts.next()?.parse().ok()?;
	let b = parts.next()?.parse().ok()?;
	Some((r, g, b))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::symbols::{normalize_spinner_frames_override, SpinnerFramesOverride, SpinnerType};

	fn sample_theme() -> Theme {
		let mut fg = ColorMap::new();
		for (key, hex) in [
			("accent", "#febc38"),
			("text", ""),
			("muted", "#777d88"),
			("success", "#89d281"),
			("mdHeading", "#febc38"),
			("border", "#178fb9"),
			("borderAccent", "#0088fa"),
			("borderMuted", "#3d424a"),
			("error", "#fc3a4b"),
			("warning", "#e4c00f"),
			("mdLink", "#0088fa"),
			("mdCode", "#e5c1ff"),
			("mdCodeBlock", "#9cdcfe"),
			("mdQuoteBorder", "#3d424a"),
			("mdListBullet", "#febc38"),
			("toolDiffAdded", "#89d281"),
			("toolDiffRemoved", "#fc3a4b"),
			("customMessageLabel", "#b281d6"),
			("thinkingText", "#777d88"),
			("thinkingMax", "#e5c1ff"),
			("thinkingHigh", "#b281d6"),
			("bashMode", "#0088fa"),
			("pythonMode", "#e4c00f"),
		] {
			fg.insert(key.to_string(), ColorValue::Hex(hex.to_string()));
		}
		let mut bg = ColorMap::new();
		bg.insert("statusLineBg".into(), ColorValue::Hex("#121212".into()));
		bg.insert("selectedBg".into(), ColorValue::Hex("#31363f".into()));
		let mut overrides = HashMap::new();
		overrides.insert("sep.dot".to_string(), ".".to_string());
		Theme::new(
			&fg,
			&bg,
			ColorMode::TrueColor,
			SymbolPreset::Unicode,
			&overrides,
			&NormalizedSpinnerFrames::default(),
			&[],
		)
		.unwrap()
	}

	#[test]
	fn fg_wraps_and_preserves_inner_resets() {
		let theme = sample_theme();
		assert_eq!(theme.fg("accent", "hello").unwrap(), "\x1b[38;2;254;188;56mhello\x1b[39m");
		assert_eq!(
			theme.fg("accent", "a\x1b[0mb").unwrap(),
			"\x1b[38;2;254;188;56ma\x1b[0m\x1b[38;2;254;188;56mb\x1b[39m"
		);
		assert_eq!(
			theme.fg("muted", "x\x1b[39my").unwrap(),
			"\x1b[38;2;119;125;136mx\x1b[39m\x1b[38;2;119;125;136my\x1b[39m"
		);
	}

	#[test]
	fn unknown_color_is_an_error() {
		let theme = sample_theme();
		assert_eq!(theme.fg("nope", "x").unwrap_err(), "Unknown theme color: nope");
		assert_eq!(theme.get_color_hex("nope").unwrap_err(), "Unknown theme color: nope");
		assert_eq!(
			theme.get_bg_hex("nope").unwrap_err(),
			"Unknown theme background color: nope"
		);
	}

	#[test]
	fn bg_and_bg_fill_reset_background_only() {
		let theme = sample_theme();
		assert_eq!(
			theme.bg("selectedBg", "x").unwrap(),
			"\x1b[48;2;49;54;63mx\x1b[49m"
		);
		assert_eq!(
			theme.bg_fill("selectedBg", "x\x1b[0my").unwrap(),
			"\x1b[48;2;49;54;63mx\x1b[0m\x1b[48;2;49;54;63my\x1b[49m"
		);
	}

	#[test]
	fn hex_resolution_and_major_order() {
		let theme = sample_theme();
		assert_eq!(theme.get_color_hex("accent").unwrap(), "#febc38");
		assert_eq!(theme.get_color_hex("text").unwrap(), "#e5e5e7");
		assert_eq!(theme.get_bg_hex("selectedBg").unwrap(), "#31363f");
		let majors = theme.get_major_theme_color_hexes();
		assert_eq!(majors[0], "#febc38");
		assert!(majors.contains(&"#178fb9".to_string()));
		assert!(!majors.contains(&"#e5e5e7".to_string()));
		assert_eq!(theme.get_accent_color_hex().unwrap(), "#febc38");
		assert!(!theme.has_color("text"));
		assert!(theme.has_color("accent"));
	}

	#[test]
	fn has_color_reports_optional_tokens_unset() {
		let theme = sample_theme();
		assert!(!theme.has_color("assistantMessageText"));
		assert_eq!(theme.get_fg_ansi("assistantMessageText").unwrap(), FG_RESET);
		assert_eq!(theme.get_fg_ansi("accent").unwrap(), "\x1b[38;2;254;188;56m");
	}

	#[test]
	fn fg_resolved_replaces_terminal_default() {
		let theme = sample_theme();
		assert_eq!(
			theme.fg_resolved("text", "x").unwrap(),
			"\x1b[38;2;229;229;231mx\x1b[39m"
		);
	}

	#[test]
	fn fg_on_bg_picks_contrast_fallback() {
		let theme = sample_theme();
		assert_eq!(
			theme.fg_on_bg("text", "selectedBg", "x").unwrap(),
			"\x1b[38;2;229;229;231mx\x1b[39m"
		);
		assert_eq!(
			theme.fg_on_bg("accent", "selectedBg", "x").unwrap(),
			"\x1b[38;2;254;188;56mx\x1b[39m"
		);
	}

	#[test]
	fn contrast_fg_uses_rec601_luma() {
		let theme = sample_theme();
		assert_eq!(theme.get_contrast_fg_ansi("accent"), "\x1b[38;2;0;0;0m");
		assert_eq!(theme.get_contrast_fg_ansi("borderMuted"), "\x1b[38;2;255;255;255m");
	}

	#[test]
	fn symbol_groups_and_overrides() {
		let theme = sample_theme();
		assert_eq!(theme.status().success, "\u{2714}");
		assert_eq!(theme.status().done, theme.status().success);
		assert_eq!(theme.sep().dot, ".");
		assert_eq!(theme.box_round().top_left, "\u{256d}");
		assert_eq!(theme.box_round().cross, theme.box_sharp().cross);
		assert_eq!(theme.format().bracket_left, "\u{27e6}");
		assert_eq!(theme.icon().folder, "\u{1f4c1}");
		assert_eq!(theme.cmd().model, theme.icon().model);
		assert_eq!(theme.thinking().minimal, "\u{25cb} min");
	}

	#[test]
	fn spinner_frames_honor_overrides() {
		let theme = sample_theme();
		assert_eq!(theme.spinner_frames().len(), 8);
		assert_eq!(theme.get_spinner_frames(SpinnerType::Activity).len(), 10);
		let mut fg = ColorMap::new();
		fg.insert("accent".into(), ColorValue::Hex("#febc38".into()));
		let mut bg = ColorMap::new();
		bg.insert("statusLineBg".into(), ColorValue::Hex("#121212".into()));
		let raw = SpinnerFramesOverride::ByType {
			status: Some(vec!["s".into()]),
			activity: None,
		};
		let theme = Theme::new(
			&fg,
			&bg,
			ColorMode::TrueColor,
			SymbolPreset::Nerd,
			&HashMap::new(),
			&normalize_spinner_frames_override(Some(&raw)),
			&[],
		)
		.unwrap();
		assert_eq!(theme.spinner_frames(), &["s".to_string()]);
		assert_eq!(theme.get_spinner_frames(SpinnerType::Activity).len(), 10);
	}

	#[test]
	fn lang_icons_map_names_and_fall_back() {
		let theme = sample_theme();
		assert_eq!(theme.get_lang_icon(None), "\u{2318}");
		assert_eq!(theme.get_lang_icon(Some("")), "\u{2318}");
		assert_eq!(theme.get_lang_icon(Some("TypeScript")), "\u{1f7e6}");
		assert_eq!(theme.get_lang_icon(Some("nonsense")), "\u{2318}");
		assert_eq!(
			theme.get_lang_icon_styled(Some("python")).unwrap(),
			format!("\x1b[38;2;55;118;171m\u{1f40d}{}", FG_RESET)
		);
		assert_eq!(
			theme.get_lang_icon_styled(Some("rust")).unwrap(),
			theme.fg("muted", &theme.get_lang_icon(Some("rust"))).unwrap()
		);
	}

	#[test]
	fn thinking_levels_map_to_tokens() {
		assert_eq!(thinking_level_token("minimal"), "thinkingMinimal");
		assert_eq!(thinking_level_token("xhigh"), "thinkingXhigh");
		assert_eq!(thinking_level_token("max"), "thinkingMax");
		assert_eq!(thinking_level_token("auto"), "thinkingOff");
		let theme = sample_theme();
		assert_eq!(
			theme.get_thinking_border_color("high")("x").unwrap(),
			theme.fg("thinkingHigh", "x").unwrap()
		);
		assert_eq!(
			theme.get_bash_mode_border_color()("x").unwrap(),
			theme.fg("bashMode", "x").unwrap()
		);
		assert_eq!(
			theme.get_python_mode_border_color()("x").unwrap(),
			theme.fg("pythonMode", "x").unwrap()
		);
	}

	#[test]
	fn ansi_palette_slots_paint() {
		let theme = sample_theme();
		assert_eq!(theme.ansi_fg(AnsiColor::Red, "x"), "\x1b[38;5;1mx\x1b[39m");
		assert_eq!(theme.ansi_fg(AnsiColor::BrightWhite, "x"), "\x1b[38;5;15mx\x1b[39m");
	}

	#[test]
	fn session_accent_inputs_are_cached() {
		let theme = sample_theme();
		let first = theme.get_session_accent_inputs();
		assert_eq!(first.accent_hex, "#febc38");
		assert!(first.surface_luminance.is_none());
		assert_eq!(first.color_hexes, theme.get_major_theme_color_hexes());
		assert_eq!(theme.get_session_accent_inputs(), first);
	}

	#[test]
	fn text_styles_wrap_with_additive_sgr() {
		let theme = sample_theme();
		assert_eq!(theme.bold("x"), "\x1b[1mx\x1b[22m");
		assert_eq!(theme.faint("x"), "\x1b[2mx\x1b[22m");
		assert_eq!(theme.italic("x"), "\x1b[3mx\x1b[23m");
		assert_eq!(theme.underline("x"), "\x1b[4mx\x1b[24m");
		assert_eq!(theme.strikethrough("x"), "\x1b[9mx\x1b[29m");
		assert_eq!(theme.inverse("x"), "\x1b[7mx\x1b[27m");
	}
}
