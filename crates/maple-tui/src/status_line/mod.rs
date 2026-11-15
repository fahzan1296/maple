//! Status line: segment catalog, renderers, separators, and the bar component.
//!
//! The module mirrors `l/src/status-line/`:
//!
//! - [`schema`] — segment identifiers, presets, settings, and separator styles.
//! - [`component`] — the composed bar, overflow handling, and input routing.
//! - [`segments`] — one renderer per catalog segment.
//! - [`separators`] — separator glyph definitions per style.
//!
//! Rendering is deterministic and performs no I/O: host, home-directory and
//! project-directory facts travel through [`segments::SegmentContext`] rather
//! than being probed on the render path.

pub mod schema;
pub mod segments;
pub mod separators;
pub mod component;

use std::cell::RefCell;
use std::rc::Rc;

/// The theme surface the status line needs.
///
/// The clean-room theme slice implements this trait for its `Theme` type; the
/// status line never depends on theme internals. `fg`/`bg` implement the
/// `wrapForeground` semantics of the TypeScript `Theme` class: nested
/// foreground resets inside `text` are re-opened so inner colors survive.
pub trait StatusTheme {
    /// Raw foreground SGR for `token` (no trailing reset), e.g. `\x1b[38;2;…m`.
    fn fg_ansi(&self, token: &str) -> String;

    /// Raw background SGR for `token` (no trailing reset), e.g. `\x1b[48;2;…m`.
    fn bg_ansi(&self, token: &str) -> String;

    /// Full symbol key, e.g. `icon.model`, `status.success`, `context.speculation`.
    fn symbol(&self, key: &str) -> String;

    /// Separator glyph by short name (`powerline`, `powerline-thin`, `slash`, …).
    fn sep(&self, name: &str) -> String;

    /// Horizontal rule glyph (`boxRound.horizontal`).
    fn horizontal(&self) -> String;

    /// Spinner frames for `status` or `activity`.
    fn spinner_frames(&self, kind: &str) -> Vec<String>;

    /// Active symbol preset name: `unicode`, `nerd`, or `ascii`.
    fn symbol_preset(&self) -> &'static str {
        "unicode"
    }

    /// Hash-derived session accent ANSI for a session title, when enabled.
    fn session_accent_ansi(&self, _name: &str) -> Option<String> {
        None
    }

    /// Wrap `text` in `token`'s foreground color, re-opening nested resets.
    fn fg(&self, token: &str, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let ansi = self.fg_ansi(token);
        let body = if text.contains('\x1b') {
            text.replace("\x1b[0m", &format!("\x1b[0m{ansi}"))
                .replace("\x1b[39m", &format!("\x1b[39m{ansi}"))
        } else {
            text.to_string()
        };
        format!("{ansi}{body}\x1b[39m")
    }

    /// Wrap `text` in `token`'s background color, closing with a background reset.
    fn bg(&self, token: &str, text: &str) -> String {
        format!("{}{}\x1b[49m", self.bg_ansi(token), text)
    }
}

thread_local! {
    static ACTIVE_THEME: RefCell<Option<Rc<dyn StatusTheme>>> = const { RefCell::new(None) };
}

/// Handle to the process-wide active theme used by status-line rendering.
///
/// The integrator installs the real theme once at startup:
///
/// ```ignore
/// status_line::ActiveTheme::set_active(Rc::new(theme::Theme::default()));
/// ```
///
/// Until then [`ActiveTheme::active`] returns [`FallbackTheme`], so the status
/// line stays deterministic and testable without a theme.
pub struct ActiveTheme;

impl ActiveTheme {
    /// Install `theme` as the active status-line theme.
    pub fn set_active(theme: Rc<dyn StatusTheme>) {
        ACTIVE_THEME.with(|slot| *slot.borrow_mut() = Some(theme));
    }

    /// The active theme, or [`FallbackTheme`] when none was installed.
    pub fn active() -> Rc<dyn StatusTheme> {
        ACTIVE_THEME
            .with(|slot| slot.borrow().clone())
            .unwrap_or_else(|| Rc::new(FallbackTheme::new()))
    }
}

/// Deterministic built-in theme used when no theme has been installed.
///
/// Glyphs are the Unicode symbol preset; colors follow the dark built-in
/// theme's key tokens. It exists so rendering and tests never depend on I/O or
/// on the theme slice being wired first.
#[derive(Clone, Debug, Default)]
pub struct FallbackTheme;

impl FallbackTheme {
    /// Create the fallback theme.
    pub fn new() -> Self {
        FallbackTheme
    }

    /// Unicode glyph for a full symbol key; empty when the key is unknown.
    pub fn unicode_symbol(key: &str) -> &'static str {
        match key {
            "status.success" | "status.done" => "✔",
            "status.error" => "✘",
            "status.warning" => "⚠",
            "status.info" => "ⓘ",
            "status.pending" => "⏳",
            "status.disabled" => "⦸",
            "status.enabled" => "●",
            "status.running" => "⟳",
            "status.shadowed" => "○",
            "status.aborted" => "⏹",
            "context.speculation" => "╎",
            "context.compaction" => "┃",
            "boxRound.horizontal" => "─",
            "boxRound.vertical" => "│",
            "sep.powerline" => "▕",
            "sep.powerlineThin" => "┆",
            "sep.powerlineLeft" => "▶",
            "sep.powerlineRight" => "◀",
            "sep.powerlineThinLeft" => ">",
            "sep.powerlineThinRight" => "<",
            "sep.powerlineCapLeft" => "",
            "sep.block" => "▌",
            "sep.space" => " ",
            "sep.asciiLeft" => ">",
            "sep.asciiRight" => "<",
            "sep.dot" => " · ",
            "sep.slash" => " / ",
            "sep.pipe" => " │ ",
            "icon.model" => "⬢",
            "icon.plan" => "🗺",
            "icon.prewalk" => "🏃",
            "icon.goal" => "🎯",
            "icon.pause" => "⏸",
            "icon.loop" => "↻",
            "icon.folder" => "📁",
            "icon.worktree" => "🌳",
            "icon.scratchFolder" => "🗑",
            "icon.git" => "⎇",
            "icon.branch" => "⑂",
            "icon.pr" => "⤴",
            "icon.pin" => "📌",
            "icon.tokens" => "🪙",
            "icon.context" => "◫",
            "icon.subscription" => "(sub)",
            "icon.advisor" => "👁",
            "icon.advisorClosed" => "🙈",
            "icon.time" => "⏱",
            "icon.omp" => "π",
            "icon.ghost" => "👻",
            "icon.agents" => "👥",
            "icon.job" => "⚙",
            "icon.cache" => "💾",
            "icon.cacheWarm" => "●",
            "icon.cacheCold" => "⊘",
            "icon.input" => "⤵",
            "icon.output" => "⤴",
            "icon.throughput" => "⚡",
            "icon.host" => "🖥",
            "icon.session" => "🆔",
            "icon.auto" => "⟲",
            "icon.fast" => "⚡",
            "icon.vimNormal" => "■",
            "icon.vimInsert" => "▎",
            "icon.vimVisual" => "◉",
            "icon.vimVisualLine" => "≡",
            "icon.vimReplace" => "▁",
            "thinking.minimal" => "○ min",
            "thinking.low" => "◔ low",
            "thinking.medium" => "◑ med",
            "thinking.high" => "◒ high",
            "thinking.xhigh" => "◕ xhigh",
            "thinking.max" => "◉ max",
            "thinking.autoPending" => "⟳",
            _ => "",
        }
    }

    fn color(&self, token: &str) -> &'static str {
        match token {
            "accent" | "statusLineModel" | "statusLineSubagents" | "thinkingHigh" => {
                "\x1b[38;2;254;188;56m"
            }
            "border" | "statusLineSep" => "\x1b[38;2;23;143;185m",
            "borderAccent" => "\x1b[38;2;0;136;250m",
            "borderMuted" => "\x1b[38;2;61;66;74m",
            "success" | "statusLineGitClean" | "statusLineStaged" => "\x1b[38;2;137;210;129m",
            "error" | "statusLineDirty" | "statusLineGitDirty" => "\x1b[38;2;252;58;75m",
            "warning" | "statusLineUntracked" => "\x1b[38;2;228;192;15m",
            "muted" | "thinkingText" | "statusLineContext" | "statusLineSpend"
            | "statusLineOutput" | "statusLineCost" => "\x1b[38;2;119;125;136m",
            "dim" => "\x1b[38;2;95;102;115m",
            "customMessageLabel" => "\x1b[38;2;178;129;214m",
            _ => "\x1b[39m",
        }
    }
}

impl StatusTheme for FallbackTheme {
    fn fg_ansi(&self, token: &str) -> String {
        self.color(token).to_string()
    }

    fn bg_ansi(&self, token: &str) -> String {
        match token {
            "statusLineBg" => "\x1b[48;2;18;18;30m".to_string(),
            _ => "\x1b[49m".to_string(),
        }
    }

    fn symbol(&self, key: &str) -> String {
        Self::unicode_symbol(key).to_string()
    }

    fn sep(&self, name: &str) -> String {
        let key = match name {
            "powerline" => "sep.powerline",
            "powerline-thin" => "sep.powerlineThin",
            "powerline-left" => "sep.powerlineLeft",
            "powerline-right" => "sep.powerlineRight",
            "powerline-thin-left" => "sep.powerlineThinLeft",
            "powerline-thin-right" => "sep.powerlineThinRight",
            "powerline-cap-left" => "sep.powerlineCapLeft",
            "block" => "sep.block",
            "space" => "sep.space",
            "ascii-left" => "sep.asciiLeft",
            "ascii-right" => "sep.asciiRight",
            "dot" => "sep.dot",
            "slash" => "sep.slash",
            "pipe" => "sep.pipe",
            _ => "",
        };
        Self::unicode_symbol(key).to_string()
    }

    fn horizontal(&self) -> String {
        "─".to_string()
    }

    fn spinner_frames(&self, kind: &str) -> Vec<String> {
        match kind {
            "status" => ["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            "activity" => ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            _ => Vec::new(),
        }
    }
}
