//! Terminal capability detection and the process-wide [`TERMINAL`] state.
//!
//! Port of `l/src/terminal-capabilities.ts` (the I/O-free capability core) plus
//! the multiplexer-session classification it depends on. Image pixel encoding,
//! [`crate::render::sixel`] passthrough and the notification subprocess routing
//! stay in their own modules.
//!
//! # Environment injection
//!
//! The TypeScript original threads `NodeJS.ProcessEnv` (defaulting to `Bun.env`)
//! through every helper. Rust has no ambient environment object, so detection
//! takes an [`EnvReader`]: [`ProcessEnv`] reads `std::env`, [`TestEnv`] is an
//! in-memory override for deterministic tests and embedders.
//!
//! # Simplifications forced by Rust
//!
//! - `$env`/`Bun.env` → [`EnvReader`] (see above).
//! - `process.platform` → [`Platform`]; `isWsl` checks the `WSL_DISTRO_NAME` /
//!   `WSL_INTEROP` markers only (no `/proc/version` sniffing).
//! - `isTerminalHeadless` → `stdout().is_terminal()`.
//! - `isBunTestRuntime` → `cfg!(test)`.
//! - `crypto.randomUUID` notification id prefix → `std::process::id()`.
//! - The tmux `display-message` / `show-options` queries are subprocess I/O and
//!   are not part of this detection core; see [`resolve_tmux_client_terminal_names`]
//!   and [`tmux_pane_allows_passthrough`] for the pure seams.

use std::collections::HashMap;
use std::env;
use std::io::IsTerminal;
use std::sync::{LazyLock, Mutex, RwLock};

use base64::Engine as _;

use crate::kitty_graphics::{
	detect_kitty_unicode_placeholders_support, set_kitty_graphics, KittyGraphicsFeatures, KITTY_PLACEHOLDER,
};

/// Read-only view of process environment variables and TTY status.
///
/// Mirrors the `NodeJS.ProcessEnv` parameter the TypeScript helpers default to
/// `Bun.env`; implementors can supply a test double without touching the real
/// environment.
pub trait EnvReader {
	/// Value of `key`, or `None` when unset.
	fn get(&self, key: &str) -> Option<String>;
	/// Whether stdout is a terminal (`process.stdout.isTTY` upstream).
	fn is_tty(&self) -> bool;
}

/// [`EnvReader`] backed by the real process environment.
pub struct ProcessEnv;

impl EnvReader for ProcessEnv {
	fn get(&self, key: &str) -> Option<String> {
		crate::env_lookup(key).and_then(|value| value.into_string().ok())
	}

	fn is_tty(&self) -> bool {
		std::io::stdout().is_terminal()
	}
}

/// In-memory [`EnvReader`] for tests and embedders.
#[derive(Clone, Debug, Default)]
pub struct TestEnv {
	vars: HashMap<String, String>,
	tty: bool,
}

impl TestEnv {
	/// Empty environment with no TTY.
	pub fn new() -> Self {
		Self::default()
	}

	/// Set `key` to `value` and return `self` for chaining.
	pub fn insert(&mut self, key: &str, value: &str) -> &mut Self {
		self.vars.insert(key.to_string(), value.to_string());
		self
	}

	/// Remove `key` and return `self` for chaining.
	pub fn remove(&mut self, key: &str) -> &mut Self {
		self.vars.remove(key);
		self
	}

	/// Override the TTY status and return `self` for chaining.
	pub fn set_tty(&mut self, tty: bool) -> &mut Self {
		self.tty = tty;
		self
	}
}

impl EnvReader for TestEnv {
	fn get(&self, key: &str) -> Option<String> {
		self.vars.get(key).cloned()
	}

	fn is_tty(&self) -> bool {
		self.tty
	}
}

/// Value of `key` when it is set to a non-empty string.
fn non_empty(env: &impl EnvReader, key: &str) -> Option<String> {
	env.get(key).filter(|value| !value.is_empty())
}

/// Host platform, mirroring `NodeJS.Platform` for the capabilities that branch on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
	Windows,
	Macos,
	Linux,
	Other,
}

impl Platform {
	/// Platform the process is running on.
	pub fn current() -> Self {
		Self::from_os(env::consts::OS)
	}

	/// Map a `std::env::consts::OS` value.
	pub fn from_os(os: &str) -> Self {
		match os {
			"windows" => Platform::Windows,
			"macos" => Platform::Macos,
			"linux" => Platform::Linux,
			_ => Platform::Other,
		}
	}

	/// Stable lowercase name (`win32`/`darwin`/`linux` upstream).
	pub fn as_str(self) -> &'static str {
		match self {
			Platform::Windows => "win32",
			Platform::Macos => "darwin",
			Platform::Linux => "linux",
			Platform::Other => "other",
		}
	}

	/// Whether this is Windows.
	pub fn is_windows(self) -> bool {
		self == Platform::Windows
	}

	/// Whether this is macOS.
	pub fn is_macos(self) -> bool {
		self == Platform::Macos
	}

	/// Whether this is Linux.
	pub fn is_linux(self) -> bool {
		self == Platform::Linux
	}
}

impl Default for Platform {
	fn default() -> Self {
		Self::current()
	}
}

/// Whether the process runs under WSL.
///
/// Upstream also sniffs `/proc/version`; that filesystem read is omitted so the
/// helper stays I/O-free. The `WSL_DISTRO_NAME`/`WSL_INTEROP` markers are the
/// reliable signals and are always present in a WSL session.
pub fn is_wsl(platform: Platform, env: &impl EnvReader) -> bool {
	if !platform.is_linux() {
		return false;
	}
	non_empty(env, "WSL_DISTRO_NAME").is_some() || non_empty(env, "WSL_INTEROP").is_some()
}

/// Whether the process has no interactive terminal attached.
pub fn is_terminal_headless() -> bool {
	!std::io::stdout().is_terminal()
}

/// Inline image protocol an image is rendered through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageProtocol {
	Kitty,
	Iterm2,
	Sixel,
}

impl ImageProtocol {
	/// Escape-sequence marker that identifies a rendered image of this protocol.
	pub fn marker(self) -> &'static str {
		match self {
			ImageProtocol::Kitty => "\x1b_G",
			ImageProtocol::Iterm2 => "\x1b]1337;File=",
			ImageProtocol::Sixel => "\x1bPq",
		}
	}
}

/// Desktop-notification protocol a terminal supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyProtocol {
	Bell,
	Osc99,
	Osc9,
}

impl Default for NotifyProtocol {
	fn default() -> Self {
		NotifyProtocol::Bell
	}
}

impl NotifyProtocol {
	/// Prefix every notification formatted for this protocol.
	pub fn marker(self) -> &'static str {
		match self {
			NotifyProtocol::Bell => "\x07",
			NotifyProtocol::Osc99 => "\x1b]99;;",
			NotifyProtocol::Osc9 => "\x1b]9;",
		}
	}
}

/// Terminal emulator identity resolved from environment markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalId {
	Kitty,
	Ghostty,
	Wezterm,
	ITerm2,
	VSCode,
	Alacritty,
	Warp,
	Orca,
	Otty,
	Rio,
	Tern,
	Monstar,
	Rex,
	Base,
	TrueColor,
}

impl Default for TerminalId {
	fn default() -> Self {
		TerminalId::Base
	}
}

impl TerminalId {
	/// Stable lowercase name used by the allowlists.
	pub fn as_str(self) -> &'static str {
		match self {
			TerminalId::Kitty => "kitty",
			TerminalId::Ghostty => "ghostty",
			TerminalId::Wezterm => "wezterm",
			TerminalId::ITerm2 => "iterm2",
			TerminalId::VSCode => "vscode",
			TerminalId::Alacritty => "alacritty",
			TerminalId::Warp => "warp",
			TerminalId::Orca => "orca",
			TerminalId::Otty => "otty",
			TerminalId::Rio => "rio",
			TerminalId::Tern => "tern",
			TerminalId::Monstar => "monstar",
			TerminalId::Rex => "rex",
			TerminalId::Base => "base",
			TerminalId::TrueColor => "trueColor",
		}
	}

	/// Resolve an identity from `TERM_PROGRAM`/`LC_TERMINAL` style values.
	pub fn from_program(program: &str) -> Option<Self> {
		match program.trim().to_ascii_lowercase().as_str() {
			"kitty" => Some(TerminalId::Kitty),
			"ghostty" => Some(TerminalId::Ghostty),
			"wezterm" => Some(TerminalId::Wezterm),
			"iterm.app" | "iterm2" => Some(TerminalId::ITerm2),
			"vscode" => Some(TerminalId::VSCode),
			"alacritty" => Some(TerminalId::Alacritty),
			"warpterminal" => Some(TerminalId::Warp),
			"orca" => Some(TerminalId::Orca),
			"otty" => Some(TerminalId::Otty),
			"rio" => Some(TerminalId::Rio),
			"tern" => Some(TerminalId::Tern),
			"monstar" => Some(TerminalId::Monstar),
			"rex" => Some(TerminalId::Rex),
			_ => None,
		}
	}
}

/// Hangul Compatibility Jamo (U+3131..=U+318E) cell width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HangulCompatibilityJamoWidth {
	/// Keep the OS default (macOS narrow, otherwise UAX#11).
	#[default]
	Platform,
	/// One cell (Warp).
	One,
	/// Two cells, UAX#11 (Ghostty, Orca, Monstar).
	Two,
}

/// OSC 8 hyperlink policy inside a multiplexer session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiplexerHyperlinks {
	/// The multiplexer draws and opens links itself.
	Render,
	/// The session never forwards OSC 8; its presence disables links.
	Drop,
	/// Forwards OSC 8 once the session self-reports at least `major.minor`.
	Forward { term_program: &'static str, major: u32, minor: u32 },
}

/// A terminal multiplexer maple recognizes, in upstream registry order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MultiplexerSession {
	Herdr,
	Tmux,
	Screen,
	Zellij,
	Cmux,
	Wmux,
	Orca,
}

impl MultiplexerSession {
	/// Every recognized session, in registry order.
	pub const ALL: [MultiplexerSession; 7] = [
		MultiplexerSession::Herdr,
		MultiplexerSession::Tmux,
		MultiplexerSession::Screen,
		MultiplexerSession::Zellij,
		MultiplexerSession::Cmux,
		MultiplexerSession::Wmux,
		MultiplexerSession::Orca,
	];

	/// Stable lowercase id.
	pub fn id(self) -> &'static str {
		match self {
			MultiplexerSession::Herdr => "herdr",
			MultiplexerSession::Tmux => "tmux",
			MultiplexerSession::Screen => "screen",
			MultiplexerSession::Zellij => "zellij",
			MultiplexerSession::Cmux => "cmux",
			MultiplexerSession::Wmux => "wmux",
			MultiplexerSession::Orca => "orca",
		}
	}

	/// Lowercase `TERM` prefix that identifies the session when markers were
	/// stripped, if it has one.
	pub fn term_prefix(self) -> Option<&'static str> {
		match self {
			MultiplexerSession::Tmux => Some("tmux"),
			MultiplexerSession::Screen => Some("screen"),
			_ => None,
		}
	}

	/// Whether the session owns the screen grid.
	pub fn owns_screen_grid(self) -> bool {
		!matches!(self, MultiplexerSession::Orca)
	}

	/// OSC 8 policy inside this session, if it defines one.
	pub fn hyperlinks(self) -> Option<MultiplexerHyperlinks> {
		match self {
			MultiplexerSession::Herdr => Some(MultiplexerHyperlinks::Render),
			MultiplexerSession::Screen => Some(MultiplexerHyperlinks::Drop),
			MultiplexerSession::Tmux => Some(MultiplexerHyperlinks::Forward { term_program: "tmux", major: 3, minor: 4 }),
			_ => None,
		}
	}

	/// Whether the session honors DEC 2026 synchronized output regardless of its
	/// DECRQM reply. Only Herdr declares it upstream.
	pub fn honors_synchronized_output(self) -> bool {
		matches!(self, MultiplexerSession::Herdr)
	}

	/// Whether the session owns the pane grid without proving the attached
	/// client renders graphics, so only an explicit override is safe.
	pub fn images_require_override(self) -> bool {
		matches!(self, MultiplexerSession::Herdr)
	}

	/// Platforms on which the session breaks OSC 11 background passthrough.
	pub fn background_query_broken_on(self) -> &'static [Platform] {
		match self {
			MultiplexerSession::Zellij => &[Platform::Macos],
			_ => &[],
		}
	}

	/// Whether an explicit session marker identifies this provider.
	pub fn is_inside(self, env: &impl EnvReader) -> bool {
		match self {
			MultiplexerSession::Herdr => {
				env.get("HERDR_ENV").as_deref() == Some("1")
					|| non_empty(env, "HERDR_PANE_ID").is_some()
					|| non_empty(env, "HERDR_TAB_ID").is_some()
					|| non_empty(env, "HERDR_WORKSPACE_ID").is_some()
			}
			MultiplexerSession::Tmux => non_empty(env, "TMUX").is_some(),
			MultiplexerSession::Screen => non_empty(env, "STY").is_some(),
			MultiplexerSession::Zellij => non_empty(env, "ZELLIJ").is_some(),
			MultiplexerSession::Cmux => {
				non_empty(env, "CMUX_WORKSPACE_ID").is_some()
					|| non_empty(env, "CMUX_SURFACE_ID").is_some()
					|| non_empty(env, "CMUX_REMOTE_TRANSPORT").is_some()
			}
			MultiplexerSession::Wmux => env.get("WMUX").as_deref() == Some("1") || non_empty(env, "WMUX_SURFACE_ID").is_some(),
			MultiplexerSession::Orca => non_empty(env, "ORCA_PANE_KEY").is_some() && non_empty(env, "ORCA_WORKTREE_ID").is_some(),
		}
	}
}

/// Every multiplexer whose explicit session markers are present, in registry order.
pub fn terminal_multiplexer_sessions(env: &impl EnvReader) -> Vec<MultiplexerSession> {
	MultiplexerSession::ALL.into_iter().filter(|session| session.is_inside(env)).collect()
}

/// Whether an explicit session marker identifies `multiplexer`.
pub fn has_terminal_multiplexer_session(env: &impl EnvReader, multiplexer: MultiplexerSession) -> bool {
	multiplexer.is_inside(env)
}

/// The multiplexer `TERM` names, whether or not its session markers survived.
pub fn terminal_multiplexer_for_term(env: &impl EnvReader) -> Option<MultiplexerSession> {
	let term = env.get("TERM").unwrap_or_default().to_ascii_lowercase();
	MultiplexerSession::ALL
		.into_iter()
		.find(|session| session.term_prefix().is_some_and(|prefix| term.starts_with(prefix)))
}

/// Registry entry for the multiplexer hosting the current process.
pub fn classify_terminal_multiplexer(env: &impl EnvReader) -> Option<MultiplexerSession> {
	MultiplexerSession::ALL
		.into_iter()
		.find(|session| session.owns_screen_grid() && session.is_inside(env))
		.or_else(|| terminal_multiplexer_for_term(env))
		.or_else(|| MultiplexerSession::ALL.into_iter().find(|session| session.is_inside(env)))
}

/// True when the classified multiplexer owns the current screen grid.
pub fn is_inside_terminal_multiplexer(env: &impl EnvReader) -> bool {
	classify_terminal_multiplexer(env).is_some_and(|session| session.owns_screen_grid())
}

/// Whether OSC 11 background replies are trusted on `platform`.
pub fn is_terminal_background_query_trusted(env: &impl EnvReader, platform: Platform) -> bool {
	!terminal_multiplexer_sessions(env).iter().any(|session| session.background_query_broken_on().contains(&platform))
}

/// Wrap `payload` in tmux's DCS passthrough envelope.
pub fn wrap_tmux_passthrough(payload: &str) -> String {
	let mut out = String::with_capacity(payload.len() + 16);
	out.push_str("\x1bPtmux;");
	for ch in payload.chars() {
		if ch == '\x1b' {
			out.push_str("\x1b\x1b");
		} else {
			out.push(ch);
		}
	}
	out.push_str("\x1b\\");
	out
}

/// Pass a control sequence through tmux, leaving direct-terminal output unchanged.
pub fn wrap_tmux_passthrough_if_needed(payload: &str, env: &impl EnvReader) -> String {
	if MultiplexerSession::Tmux.is_inside(env) {
		wrap_tmux_passthrough(payload)
	} else {
		payload.to_string()
	}
}

/// Emulator names recorded for the attached tmux client.
///
/// Upstream runs `tmux display-message -p '#{client_termtype}\t#{client_termname}'`,
/// which is subprocess I/O and therefore outside this I/O-free detection core.
/// The pure seam is the comma-separated `MAPLE_TMUX_CLIENT_TERMINAL_NAMES` override,
/// which embedders and tests can populate with the client's `TERM_PROGRAM`/`TERM`
/// names. Returns an empty list otherwise.
pub fn resolve_tmux_client_terminal_names(env: &impl EnvReader) -> Vec<String> {
		env.get("MAPLE_TMUX_CLIENT_TERMINAL_NAMES")
		.map(|value| {
			value
				.split(',')
				.map(|name| name.trim().to_string())
				.filter(|name| !name.is_empty())
				.collect()
		})
		.unwrap_or_default()
}

/// Whether tmux forwards this pane's `\x1bPtmux;…\x1b\\` passthrough envelopes.
///
/// Upstream resolves the effective `allow-passthrough` option with a
/// `tmux show-options` subprocess. That query is I/O and is intentionally not
/// part of this core; the pure seam is `MAPLE_TMUX_ALLOW_PASSTHROUGH`
/// (`1`/`true`/`on`/`all`). Without it the tmux 3.3+ default of `off` is
/// reported, which is the safe choice for Kitty graphics.
pub fn tmux_pane_allows_passthrough(env: &impl EnvReader) -> bool {
	matches!(
		env.get("MAPLE_TMUX_ALLOW_PASSTHROUGH")
			.map(|value| value.trim().to_ascii_lowercase())
			.as_deref(),
		Some("1") | Some("true") | Some("on") | Some("all")
	)
}

/// Terminal capability details used for rendering and protocol selection.
#[derive(Clone, Debug)]
pub struct TerminalInfo {
	/// Resolved terminal identity.
	pub id: TerminalId,
	/// Inline image protocol the terminal supports, if any.
	pub image_protocol: Option<ImageProtocol>,
	/// Whether the terminal renders 24-bit color.
	pub true_color: bool,
	/// Whether OSC 8 hyperlinks are enabled.
	pub hyperlinks: bool,
	/// Desktop-notification protocol.
	pub notify_protocol: NotifyProtocol,
	/// Whether Kitty-style DECCARA rectangular SGR changes apply.
	pub deccara: bool,
	/// Whether screen-to-scrollback clears are supported.
	pub supports_screen_to_scrollback: bool,
	/// Whether the terminal renders the Kitty OSC 66 text-sizing protocol.
	pub supports_text_sizing: bool,
	/// Hangul Compatibility Jamo cell width.
	pub hangul_jamo_width: HangulCompatibilityJamoWidth,
	/// Whether OSC 66 text sizing is currently enabled at runtime.
	pub text_sizing: bool,
	/// Whether colon-subparameter styled underlines are supported.
	pub styled_underlines: bool,
	/// Whether the Glyph Protocol probe confirmed `glyf` support.
	pub glyph_protocol: bool,
}

impl Default for TerminalInfo {
	fn default() -> Self {
		Self {
			id: TerminalId::Base,
			image_protocol: None,
			true_color: false,
			hyperlinks: false,
			notify_protocol: NotifyProtocol::Bell,
			deccara: false,
			supports_screen_to_scrollback: false,
			supports_text_sizing: false,
			hangul_jamo_width: HangulCompatibilityJamoWidth::Platform,
			text_sizing: false,
			styled_underlines: false,
			glyph_protocol: false,
		}
	}
}

impl TerminalInfo {
	/// Build the static capability profile for `id`.
	pub fn new(id: TerminalId, image_protocol: Option<ImageProtocol>, true_color: bool, hyperlinks: bool) -> Self {
		Self { id, image_protocol, true_color, hyperlinks, ..Self::default() }
	}

	/// Whether an image marker begins at byte index `start`.
	///
	/// Shared primitive for standalone image checks and the renderer's combined
	/// ANSI/width scan, so their protocol windows and sixel grammar cannot drift.
	pub fn has_image_marker_at(&self, line: &str, start: usize) -> bool {
		let Some(protocol) = self.image_protocol else {
			return false;
		};
		let bytes = line.as_bytes();
		if protocol == ImageProtocol::Sixel {
			let limit = bytes.len().min(SIXEL_MARKER_SCAN_LIMIT);
			if start + 3 > limit || bytes.get(start) != Some(&0x1b) || bytes.get(start + 1) != Some(&0x50) {
				return false;
			}
			let mut i = start + 2;
			while i < limit {
				let code = bytes[i];
				if code.is_ascii_digit() || code == b';' {
					i += 1;
					continue;
				}
				break;
			}
			return i < limit && bytes[i] == b'q';
		}

		let limit = bytes.len().min(IMAGE_MARKER_SCAN_LIMIT);
		let marker = protocol.marker().as_bytes();
		let mut protocol_matches = start + marker.len() <= limit;
		let mut offset = 0;
		while protocol_matches && offset < marker.len() {
			protocol_matches = bytes.get(start + offset) == marker.get(offset);
			offset += 1;
		}
		protocol_matches
			|| (start + KITTY_PLACEHOLDER.len_utf8() <= limit
				&& line.get(start..).is_some_and(|tail| tail.starts_with(KITTY_PLACEHOLDER)))
	}

	/// Whether `line` contains an image marker for the resolved protocol.
	pub fn is_image_line(&self, line: &str) -> bool {
		let Some(protocol) = self.image_protocol else {
			return false;
		};
		let limit = line.len().min(if protocol == ImageProtocol::Sixel {
			SIXEL_MARKER_SCAN_LIMIT
		} else {
			IMAGE_MARKER_SCAN_LIMIT
		});
		for (index, ch) in line.char_indices() {
			if index >= limit {
				break;
			}
			if ch != '\u{1b}' && ch != KITTY_PLACEHOLDER {
				continue;
			}
			if self.has_image_marker_at(line, index) {
				return true;
			}
		}
		false
	}

	/// Format `message` for the resolved notification protocol.
	pub fn format_notification(&self, message: Notification<'_>) -> String {
		if self.notify_protocol == NotifyProtocol::Bell {
			return NotifyProtocol::Bell.marker().to_string();
		}
		match message {
			Notification::Structured(notification) => {
				if self.notify_protocol == NotifyProtocol::Osc99 && is_osc99_supported() {
					return format_osc99_notification(notification);
				}
				format!("{}{}\x1b\\", self.notify_protocol.marker(), notification_to_line(notification))
			}
			Notification::Message(message) => format!("{}{}\x1b\\", self.notify_protocol.marker(), message),
		}
	}
}

/// The mutable singleton type: [`TerminalInfo`] with runtime-resolved fields.
pub type RuntimeTerminal = TerminalInfo;

const IMAGE_MARKER_SCAN_LIMIT: usize = 512;
const SIXEL_MARKER_SCAN_LIMIT: usize = 128;

fn warp_terminal_info(platform: Platform, env: &impl EnvReader) -> TerminalInfo {
	TerminalInfo {
		id: TerminalId::Warp,
		image_protocol: resolve_warp_image_protocol(platform, env),
		true_color: true,
		hyperlinks: false,
		notify_protocol: NotifyProtocol::Osc9,
		hangul_jamo_width: HangulCompatibilityJamoWidth::One,
		..TerminalInfo::default()
	}
}

/// Static capability profile for `terminal_id`.
pub fn get_terminal_info(terminal_id: TerminalId, platform: Platform, env: &impl EnvReader) -> TerminalInfo {
	match terminal_id {
		TerminalId::Warp => warp_terminal_info(platform, env),
		TerminalId::Base => TerminalInfo::new(TerminalId::Base, None, false, false),
		TerminalId::TrueColor => TerminalInfo::new(TerminalId::TrueColor, None, true, false),
		TerminalId::Kitty => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			notify_protocol: NotifyProtocol::Osc99,
			deccara: true,
			supports_screen_to_scrollback: true,
			supports_text_sizing: true,
			..TerminalInfo::new(TerminalId::Kitty, None, true, true)
		},
		TerminalId::Ghostty => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			notify_protocol: NotifyProtocol::Osc9,
			hangul_jamo_width: HangulCompatibilityJamoWidth::Two,
			..TerminalInfo::new(TerminalId::Ghostty, None, true, true)
		},
		TerminalId::Wezterm => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			notify_protocol: NotifyProtocol::Osc9,
			..TerminalInfo::new(TerminalId::Wezterm, None, true, true)
		},
		TerminalId::ITerm2 => TerminalInfo {
			image_protocol: Some(ImageProtocol::Iterm2),
			true_color: true,
			hyperlinks: true,
			notify_protocol: NotifyProtocol::Osc9,
			..TerminalInfo::new(TerminalId::ITerm2, None, true, true)
		},
		TerminalId::VSCode => TerminalInfo::new(TerminalId::VSCode, None, true, true),
		TerminalId::Alacritty => TerminalInfo::new(TerminalId::Alacritty, None, true, true),
		TerminalId::Orca => TerminalInfo {
			true_color: true,
			hangul_jamo_width: HangulCompatibilityJamoWidth::Two,
			..TerminalInfo::new(TerminalId::Orca, None, true, false)
		},
		TerminalId::Otty => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			notify_protocol: NotifyProtocol::Osc99,
			..TerminalInfo::new(TerminalId::Otty, None, true, true)
		},
		TerminalId::Rio => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			..TerminalInfo::new(TerminalId::Rio, None, true, true)
		},
		TerminalId::Rex => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			..TerminalInfo::new(TerminalId::Rex, None, true, true)
		},
		TerminalId::Tern => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			notify_protocol: NotifyProtocol::Osc99,
			..TerminalInfo::new(TerminalId::Tern, None, true, true)
		},
		TerminalId::Monstar => TerminalInfo {
			image_protocol: Some(ImageProtocol::Kitty),
			true_color: true,
			hyperlinks: true,
			notify_protocol: NotifyProtocol::Osc9,
			hangul_jamo_width: HangulCompatibilityJamoWidth::Two,
			..TerminalInfo::new(TerminalId::Monstar, None, true, true)
		},
	}
}

/// Parse a `major.minor` version prefix, or `None` when absent/unparseable.
fn parse_major_minor_version(version_raw: Option<&str>) -> Option<(u32, u32)> {
	let raw = version_raw?.trim();
	let dot = raw.find('.')?;
	let major = raw[..dot].parse::<u32>().ok()?;
	let minor_text: String = raw[dot + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
	if minor_text.is_empty() {
		return None;
	}
	Some((major, minor_text.parse::<u32>().ok()?))
}

/// Whether the process runs in an SSH session, so the terminal emulator is
/// remote and host-local input/keyboard modes cannot be assumed.
pub fn is_ssh_session(env: &impl EnvReader) -> bool {
	non_empty(env, "SSH_CONNECTION").is_some() || non_empty(env, "SSH_TTY").is_some() || non_empty(env, "SSH_CLIENT").is_some()
}

/// Whether notifications are suppressed with `MAPLE_NOTIFICATIONS`.
pub fn is_notification_suppressed() -> bool {
	let Some(value) = crate::env_lookup("MAPLE_NOTIFICATIONS").and_then(|value| value.into_string().ok()) else {
		return false;
	};
	matches!(value.as_str(), "off" | "0" | "false")
}

/// Resolved `MAPLE_FORCE_IMAGE_PROTOCOL` value: outer `None` = unset, inner `None`
/// = forced off.
fn forced_image_protocol(env: &impl EnvReader) -> Option<Option<ImageProtocol>> {
	let raw = non_empty(env, "MAPLE_FORCE_IMAGE_PROTOCOL")?.trim().to_ascii_lowercase();
	match raw.as_str() {
		"kitty" => Some(Some(ImageProtocol::Kitty)),
		"iterm2" | "iterm" => Some(Some(ImageProtocol::Iterm2)),
		"sixel" => Some(Some(ImageProtocol::Sixel)),
		"off" | "none" | "0" | "false" => Some(None),
		_ => Some(None),
	}
}

/// Whether `MAPLE_FORCE_IMAGE_PROTOCOL` pins the image protocol, including its
/// `off`/`none` kill switch.
pub fn is_image_protocol_forced() -> bool {
	forced_image_protocol(&ProcessEnv).is_some()
}

/// Returns true when running in Windows Terminal with known SIXEL support.
///
/// Windows Terminal introduced SIXEL support in preview 1.22.
pub fn is_windows_terminal_preview_sixel_supported(env: &impl EnvReader, platform: Platform) -> bool {
	if !platform.is_windows() || non_empty(env, "WT_SESSION").is_none() {
		return false;
	}
	if let Some(program) = env.get("TERM_PROGRAM") {
		if !program.eq_ignore_ascii_case("windows_terminal") {
			return false;
		}
	}
	let Some(version) = parse_major_minor_version(env.get("TERM_PROGRAM_VERSION").as_deref()) else {
		return false;
	};
	version.0 > 1 || (version.0 == 1 && version.1 >= 22)
}

/// Explicit user override for DEC 2026 synchronized output: `Some(false)` opt-out,
/// `Some(true)` force-on, `None` no preference. An opt-out beats a force-on.
pub fn synchronized_output_user_override(env: &impl EnvReader) -> Option<bool> {
	if non_empty(env, "MAPLE_NO_SYNC_OUTPUT").is_some() || env.get("MAPLE_TUI_SYNC_OUTPUT").as_deref() == Some("0") {
		return Some(false);
	}
	if env.get("MAPLE_FORCE_SYNC_OUTPUT").as_deref() == Some("1") || env.get("MAPLE_TUI_SYNC_OUTPUT").as_deref() == Some("1") {
		return Some(true);
	}
	None
}

/// Whether `TERM_FEATURES` advertises DEC 2026 synchronized output (`Sy`).
fn advertises_synchronized_output(term_features: Option<&str>) -> bool {
	term_features.is_some_and(|features| features.contains("Sy"))
}

/// Whether DEC 2026 synchronized-output wrappers should be enabled by default.
///
/// Policy (highest precedence first): user override, positive `TERM_FEATURES`
/// advertisement, Windows Terminal 1.24+, a session whose pane VTE honors DEC 2026,
/// then known direct terminals. Everything else (including risky multiplexers)
/// starts off until the runtime DECRQM probe proves support.
pub fn should_enable_synchronized_output_by_default(env: &impl EnvReader, terminal_id: TerminalId) -> bool {
	if let Some(override_value) = synchronized_output_user_override(env) {
		return override_value;
	}
	if advertises_synchronized_output(env.get("TERM_FEATURES").as_deref()) {
		return true;
	}
	if non_empty(env, "WT_SESSION").is_some()
		&& env
			.get("TERM_PROGRAM")
			.map(|program| program.eq_ignore_ascii_case("windows_terminal"))
			.unwrap_or(true)
	{
		return true;
	}
	if terminal_multiplexer_sessions(env).iter().any(|session| session.honors_synchronized_output()) {
		return true;
	}
	if is_inside_terminal_multiplexer(env) {
		return false;
	}
	matches!(
		terminal_id,
		TerminalId::Kitty
			| TerminalId::Ghostty
			| TerminalId::Monstar
			| TerminalId::Wezterm
			| TerminalId::ITerm2
			| TerminalId::Alacritty
			| TerminalId::VSCode
	)
}

/// Whether the terminal applies Kitty-style DECCARA rectangular SGR changes
/// extended to background color.
///
/// Verified against terminal sources rather than terminfo: Kitty implements it
/// for all SGR attributes including background; Ghostty does not. Disabled under
/// tmux/screen/zellij multiplexers and via the `MAPLE_NO_DECCARA` kill switch.
pub fn detect_rectangular_sgr_support(terminal_id: TerminalId, env: &impl EnvReader) -> bool {
	if terminal_id != TerminalId::Kitty {
		return false;
	}
	if let Some(kill) = env.get("MAPLE_NO_DECCARA") {
		if kill != "0" && !kill.eq_ignore_ascii_case("false") {
			return false;
		}
	}
	if is_inside_terminal_multiplexer(env) {
		return false;
	}
	true
}

/// Whether the terminal implements colon-subparameter SGR styled underlines
/// (`CSI 4 : 3 m` plus `CSI 58`/`CSI 59` underline color).
///
/// Keyed on the detected terminal, never on `TERM`/`COLORTERM`, and disabled
/// under any multiplexer because the outer terminal's id leaks through the
/// session env without proving the bytes survive.
pub fn detect_styled_underline_support(terminal_id: TerminalId, env: &impl EnvReader) -> bool {
	if is_inside_terminal_multiplexer(env) {
		return false;
	}
	match terminal_id {
		TerminalId::Kitty | TerminalId::Ghostty | TerminalId::Monstar | TerminalId::Wezterm => true,
		TerminalId::ITerm2 => {
			let version = parse_major_minor_version(env.get("TERM_PROGRAM_VERSION").as_deref())
				.or_else(|| parse_major_minor_version(env.get("LC_TERMINAL_VERSION").as_deref()));
			version.is_some_and(|(major, minor)| major > 3 || (major == 3 && minor >= 5))
		}
		_ => false,
	}
}

/// Explicit user override for OSC 8 hyperlinks: `Some(false)` opt-out,
/// `Some(true)` force-on, `None` no preference. Opt-out wins ties.
pub fn hyperlinks_user_override(env: &impl EnvReader) -> Option<bool> {
	if env.get("MAPLE_NO_HYPERLINKS").as_deref() == Some("1") {
		return Some(false);
	}
	if env.get("MAPLE_FORCE_HYPERLINKS").as_deref() == Some("1") {
		return Some(true);
	}
	None
}

/// Whether OSC 8 hyperlinks should be enabled by default.
///
/// Folds the static per-terminal flag into the multiplexer policies: `render`
/// sessions enable links, `drop` sessions disable them, `forward` sessions need
/// a self-reported version, and a TERM-only multiplexer match disables them.
pub fn should_enable_hyperlinks_by_default(env: &impl EnvReader, terminal_id: TerminalId) -> bool {
	if let Some(override_value) = hyperlinks_user_override(env) {
		return override_value;
	}
	let policies: Vec<MultiplexerHyperlinks> = terminal_multiplexer_sessions(env)
		.iter()
		.filter_map(|session| session.hyperlinks())
		.collect();
	if !policies.is_empty() && policies.iter().all(|policy| *policy == MultiplexerHyperlinks::Render) {
		return true;
	}
	if !get_terminal_info(terminal_id, Platform::current(), env).hyperlinks {
		return false;
	}
	if policies.iter().any(|policy| *policy == MultiplexerHyperlinks::Drop) {
		return false;
	}
	let forwarders: Vec<(&'static str, u32, u32)> = policies
		.iter()
		.filter_map(|policy| match policy {
			MultiplexerHyperlinks::Forward { term_program, major, minor } => Some((*term_program, *major, *minor)),
			_ => None,
		})
		.collect();
	if !forwarders.is_empty() {
		let term_program = env.get("TERM_PROGRAM").unwrap_or_default().to_ascii_lowercase();
		return forwarders.iter().all(|(program, major, minor)| {
			if term_program != *program {
				return false;
			}
			let Some((resolved_major, resolved_minor)) = parse_major_minor_version(env.get("TERM_PROGRAM_VERSION").as_deref())
			else {
				return false;
			};
			resolved_major > *major || (resolved_major == *major && resolved_minor >= *minor)
		});
	}
	if terminal_multiplexer_for_term(env).is_some() {
		return false;
	}
	true
}

/// Fallback image protocol for terminals whose static profile has none.
fn get_fallback_image_protocol(terminal_id: TerminalId, env: &impl EnvReader, is_tty: bool) -> Option<ImageProtocol> {
	if !is_tty {
		return None;
	}
	if matches!(terminal_id, TerminalId::VSCode | TerminalId::Alacritty) {
		return None;
	}
	let term = env.get("TERM").unwrap_or_default().to_ascii_lowercase();
	// tmux TERM names the pane, not the attached client's graphics support.
	if non_empty(env, "TMUX").is_some() || term.contains("tmux") {
		return None;
	}
	if term.contains("screen") || term.contains("ghostty") {
		return Some(ImageProtocol::Kitty);
	}
	None
}

/// Warp implements the Kitty graphics protocol only on macOS/Linux; its Windows
/// build (including Warp-hosted WSL shells) renders the same APC sequences as
/// visible garbage.
pub fn resolve_warp_image_protocol(platform: Platform, env: &impl EnvReader) -> Option<ImageProtocol> {
	if platform.is_windows() || is_wsl(platform, env) {
		None
	} else {
		Some(ImageProtocol::Kitty)
	}
}

/// Paseo (getpaseo/paseo) hardcodes `TERM_PROGRAM=kitty` into every PTY while its
/// xterm.js renderer implements neither Kitty graphics nor Unicode placeholders.
pub fn is_paseo_embedder(env: &impl EnvReader) -> bool {
	non_empty(env, "PASEO_TERMINAL_ID").is_some()
}

/// Resolve the image protocol for a non-forced runtime: static per-terminal
/// support (with Warp's platform carve-out), then the multiplexer fallback, then
/// host carve-outs.
pub fn resolve_image_protocol(terminal_id: TerminalId, env: &impl EnvReader, is_tty: bool) -> Option<ImageProtocol> {
	let image_protocol = if terminal_id == TerminalId::Warp {
		resolve_warp_image_protocol(Platform::current(), env)
	} else {
		get_terminal_info(terminal_id, Platform::current(), env)
			.image_protocol
			.or_else(|| get_fallback_image_protocol(terminal_id, env, is_tty))
	};
	// Paseo's renderer draws neither Kitty APC nor placeholders; applied after
	// the multiplexer fallback so tmux/screen inside a Paseo pane cannot restore it.
	if image_protocol.is_some() && is_paseo_embedder(env) {
		return None;
	}
	// Some multiplexers own the pane grid without exposing whether the attached
	// client renders graphics; only an explicit override is safe.
	if image_protocol.is_some() && terminal_multiplexer_sessions(env).iter().any(|session| session.images_require_override()) {
		return None;
	}
	// Kitty graphics reach a tmux client only through the DCS passthrough
	// envelope, which tmux drops unless `allow-passthrough` is enabled.
	if image_protocol == Some(ImageProtocol::Kitty) && non_empty(env, "TMUX").is_some() && !tmux_pane_allows_passthrough(env) {
		return None;
	}
	image_protocol
}

/// Detected terminal background appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BackgroundAppearance {
	#[default]
	Dark,
	Light,
}

static TERMINAL_BACKGROUND_COLOR: LazyLock<Mutex<Option<u32>>> = LazyLock::new(|| Mutex::new(None));

/// Trusted terminal background when reported; otherwise black for detected dark
/// appearance or white for detected light appearance.
pub fn get_terminal_background_color() -> u32 {
	let env = ProcessEnv;
	let platform = Platform::current();
	let stored = *TERMINAL_BACKGROUND_COLOR.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	if is_terminal_background_query_trusted(&env, platform) {
		if let Some(color) = stored {
			return color;
		}
	}
	if detect_terminal_background_appearance(&env, platform) == BackgroundAppearance::Light {
		0xffffff
	} else {
		0x000000
	}
}

/// Store an OSC 11 background, or reset to appearance detection (also for tests).
pub fn set_terminal_background_color(color: Option<u32>) {
	*TERMINAL_BACKGROUND_COLOR.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = color;
}

/// Detect the terminal background appearance.
///
/// Tier 1 is the terminal-reported OSC 11 color (BT.601 luminance, matching the
/// upstream terminal layer); tier 2 is `COLORFGBG`. The macOS host-appearance
/// tier is not reproduced because `theme/` exposes no appearance observer.
pub fn detect_terminal_background_appearance(env: &impl EnvReader, platform: Platform) -> BackgroundAppearance {
	let stored = *TERMINAL_BACKGROUND_COLOR.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	if is_terminal_background_query_trusted(env, platform) {
		if let Some(color) = stored {
			let r = ((color >> 16) & 0xff) as f64 / 255.0;
			let g = ((color >> 8) & 0xff) as f64 / 255.0;
			let b = (color & 0xff) as f64 / 255.0;
			return if 0.299 * r + 0.587 * g + 0.114 * b < 0.5 {
				BackgroundAppearance::Dark
			} else {
				BackgroundAppearance::Light
			};
		}
	}
	if let Some(colorfgbg) = env.get("COLORFGBG") {
		let parts: Vec<&str> = colorfgbg.split(';').collect();
		if parts.len() >= 2 {
			if let Ok(bg) = parts[1].trim().parse::<i64>() {
				return if bg < 8 { BackgroundAppearance::Dark } else { BackgroundAppearance::Light };
			}
		}
	}
	BackgroundAppearance::Dark
}

/// Resolve terminal identity from environment markers used by common emulators.
pub fn detect_terminal_id(env: &impl EnvReader) -> TerminalId {
	if non_empty(env, "KITTY_WINDOW_ID").is_some() {
		return TerminalId::Kitty;
	}
	if non_empty(env, "GHOSTTY_RESOURCES_DIR").is_some() {
		return TerminalId::Ghostty;
	}
	if non_empty(env, "WEZTERM_PANE").is_some() {
		return TerminalId::Wezterm;
	}
	if non_empty(env, "ITERM_SESSION_ID").is_some() {
		return TerminalId::ITerm2;
	}
	if non_empty(env, "VSCODE_PID").is_some() {
		return TerminalId::VSCode;
	}
	if non_empty(env, "ALACRITTY_WINDOW_ID").is_some() {
		return TerminalId::Alacritty;
	}
	if let Some(program_id) = env.get("TERM_PROGRAM").and_then(|program| TerminalId::from_program(&program)) {
		return program_id;
	}
	// iTerm2 sets `LC_TERMINAL=iTerm2` so it survives SSH, where `TERM_PROGRAM`
	// is stripped. Gated on the absence of a multiplexer because iTerm2's encoder
	// is the only one that does not wrap its payload in a passthrough envelope.
	if env.get("LC_TERMINAL").is_some_and(|terminal| terminal.eq_ignore_ascii_case("iTerm2"))
		&& !is_inside_terminal_multiplexer(env)
	{
		return TerminalId::ITerm2;
	}
	// tmux overwrites pane identity variables; the attached client's recorded
	// terminal type is the only surviving signal.
	for client_program in resolve_tmux_client_terminal_names(env) {
		if let Some(client_id) = TerminalId::from_program(&client_program) {
			return client_id;
		}
	}
	if let Some(term) = env.get("TERM") {
		let term = term.to_ascii_lowercase();
		if term.contains("ghostty") {
			return TerminalId::Ghostty;
		}
		if term == "monstar" {
			return TerminalId::Monstar;
		}
		if term == "xterm-rex" {
			return TerminalId::Rex;
		}
	}
	if let Some(colorterm) = env.get("COLORTERM") {
		if colorterm.eq_ignore_ascii_case("truecolor") || colorterm.eq_ignore_ascii_case("24bit") {
			return TerminalId::TrueColor;
		}
	}
	TerminalId::Base
}

/// Detected terminal id for the current process.
pub fn terminal_id() -> TerminalId {
	detect_terminal_id(&ProcessEnv)
}

/// Resolve the runtime capability state for the current process.
fn resolve_terminal() -> TerminalInfo {
	let env = ProcessEnv;
	let platform = Platform::current();
	let id = detect_terminal_id(&env);
	let mut info = get_terminal_info(id, platform, &env);
	// Detection records support; hosts opt into OSC 66 separately.
	info.text_sizing = false;
	info.image_protocol = match forced_image_protocol(&env) {
		Some(forced) => forced,
		None => resolve_image_protocol(info.id, &env, env.is_tty()),
	};
	info.hyperlinks = should_enable_hyperlinks_by_default(&env, info.id);
	info.deccara = detect_rectangular_sgr_support(info.id, &env) && !cfg!(test);
	info.styled_underlines = detect_styled_underline_support(info.id, &env);
	info.glyph_protocol = false;
	info
}

static TERMINAL: LazyLock<RwLock<TerminalInfo>> = LazyLock::new(|| {
	let info = resolve_terminal();
	set_kitty_graphics(KittyGraphicsFeatures {
		unicode_placeholders: detect_kitty_unicode_placeholders_support(info.id, &ProcessEnv),
	});
	RwLock::new(info)
});

/// The process-wide [`TERMINAL`] capability state.
pub fn terminal() -> RuntimeTerminal {
	TERMINAL.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// Cell size in pixels, updated when the terminal answers a cell-size query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellDimensions {
	/// Cell width in pixels.
	pub width_px: u32,
	/// Cell height in pixels.
	pub height_px: u32,
}

impl Default for CellDimensions {
	fn default() -> Self {
		Self { width_px: 9, height_px: 18 }
	}
}

static CELL_DIMENSIONS: LazyLock<RwLock<CellDimensions>> = LazyLock::new(|| RwLock::new(CellDimensions::default()));

/// The current cell size in pixels (9x18 until a cell-size query answers).
pub fn cell_dimensions() -> CellDimensions {
	*CELL_DIMENSIONS.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Record the cell size reported by the terminal.
pub fn set_cell_dimensions(dimensions: CellDimensions) {
	*CELL_DIMENSIONS.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = dimensions;
}

#[cfg(test)]
mod cell_dimensions_tests {
	use super::*;

	#[test]
	fn defaults_to_9x18_and_roundtrips() {
		assert_eq!(cell_dimensions(), CellDimensions { width_px: 9, height_px: 18 });
		set_cell_dimensions(CellDimensions { width_px: 10, height_px: 20 });
		assert_eq!(cell_dimensions(), CellDimensions { width_px: 10, height_px: 20 });
	}
}

/// Override terminal image protocol at runtime after capability probes complete.
pub fn set_terminal_image_protocol(image_protocol: Option<ImageProtocol>) {
	TERMINAL.write().unwrap_or_else(|poisoned| poisoned.into_inner()).image_protocol = image_protocol;
}

/// Override DECCARA rectangular-SGR capability at runtime.
pub fn set_terminal_deccara(enabled: bool) {
	TERMINAL.write().unwrap_or_else(|poisoned| poisoned.into_inner()).deccara = enabled;
}

/// Record the Glyph Protocol probe result (called by the terminal backend).
pub fn set_terminal_glyph_protocol(supported: bool) {
	TERMINAL.write().unwrap_or_else(|poisoned| poisoned.into_inner()).glyph_protocol = supported;
}

/// Override screen-to-scrollback clear support for targeted renderer tests.
pub fn set_terminal_screen_to_scrollback(enabled: bool) {
	TERMINAL.write().unwrap_or_else(|poisoned| poisoned.into_inner()).supports_screen_to_scrollback = enabled;
}

/// Enable/disable OSC 66 text-sizing at runtime.
pub fn set_terminal_text_sizing(enabled: bool) {
	TERMINAL.write().unwrap_or_else(|poisoned| poisoned.into_inner()).text_sizing = enabled;
}

/// Override OSC 8 hyperlink capability at runtime.
pub fn set_terminal_hyperlinks(enabled: bool) {
	TERMINAL.write().unwrap_or_else(|poisoned| poisoned.into_inner()).hyperlinks = enabled;
}

static OSC99_CONFIRMED: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));
static OSC99_NEXT_ID: LazyLock<Mutex<u32>> = LazyLock::new(|| Mutex::new(1));

/// Record the OSC 99 capability-probe result (called by the terminal backend).
pub fn set_osc99_supported(supported: bool) {
	*OSC99_CONFIRMED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = supported;
}

/// True when OSC 99 structured notifications have been confirmed available.
pub fn is_osc99_supported() -> bool {
	*OSC99_CONFIRMED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Notification urgency for OSC 99.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NotificationUrgency {
	Low,
	#[default]
	Normal,
	Critical,
}

/// Notification action policy for OSC 99.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NotificationActions {
	#[default]
	Focus,
	Report,
	FocusReport,
	None,
}

/// Notification sound for OSC 99.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NotificationSound {
	#[default]
	Silent,
	System,
	Info,
	Warning,
	Error,
	Question,
}

impl NotificationSound {
	/// OSC 99 `s=` value.
	pub fn as_str(self) -> &'static str {
		match self {
			NotificationSound::Silent => "silent",
			NotificationSound::System => "system",
			NotificationSound::Info => "info",
			NotificationSound::Warning => "warning",
			NotificationSound::Error => "error",
			NotificationSound::Question => "question",
		}
	}
}

/// Structured terminal notification. Rich fields are honored only by OSC 99
/// (Kitty) once support is confirmed; other protocols collapse to one line.
#[derive(Clone, Debug, Default)]
pub struct TerminalNotification {
	/// Short title.
	pub title: Option<String>,
	/// Longer body.
	pub body: Option<String>,
	/// Stable notification id; kitty matches ids process-wide.
	pub id: Option<String>,
	/// Notification type(s), base64-encoded by OSC 99.
	pub notification_type: Option<Vec<String>>,
	/// Urgency.
	pub urgency: Option<NotificationUrgency>,
	/// Icon name.
	pub icon_name: Option<String>,
	/// Sound.
	pub sound: Option<NotificationSound>,
	/// Action policy.
	pub actions: Option<NotificationActions>,
	/// Expiry in milliseconds; negative means "no expiry".
	pub expires_ms: Option<f64>,
}

/// A notification to format: either a plain message or structured OSC 99 fields.
#[derive(Clone, Copy, Debug)]
pub enum Notification<'a> {
	/// Plain single-line message.
	Message(&'a str),
	/// Structured notification.
	Structured(&'a TerminalNotification),
}

const OSC99_MAX_PAYLOAD_BYTES: usize = 2048;
const OSC99_APP_NAME: &str = "omp";

fn base64_encode(value: &str) -> String {
	base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

fn sanitize_osc99_id(id: Option<&str>) -> String {
	let Some(id) = id else {
		return String::new();
	};
	let safe: String = id
		.chars()
		.filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '.'))
		.collect();
	if safe == "0" {
		String::new()
	} else {
		safe
	}
}

fn osc99_id(id: Option<&str>) -> String {
	let safe = sanitize_osc99_id(id);
	if !safe.is_empty() {
		return safe;
	}
	let mut next = OSC99_NEXT_ID.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	let counter = *next;
	*next += 1;
	format!("omp-{}-{counter}", std::process::id())
}

fn utf8_code_point_bytes(ch: char) -> usize {
	ch.len_utf8()
}

fn chunk_utf8(payload: &str) -> Vec<String> {
	if payload.is_empty() {
		return vec![String::new()];
	}
	let mut chunks = Vec::new();
	let mut start = 0usize;
	let mut bytes = 0usize;
	for (index, ch) in payload.char_indices() {
		let char_bytes = utf8_code_point_bytes(ch);
		if bytes > 0 && bytes + char_bytes > OSC99_MAX_PAYLOAD_BYTES {
			chunks.push(payload[start..index].to_string());
			start = index;
			bytes = 0;
		}
		bytes += char_bytes;
	}
	chunks.push(payload[start..].to_string());
	chunks
}

fn osc99_unsafe(payload: &str) -> bool {
	payload
		.chars()
		.any(|c| matches!(c, '\u{0000}'..='\u{001f}' | '\u{007f}' | '\u{0080}'..='\u{009f}'))
}

fn osc99_chunk(meta: &[String], payload: &str) -> String {
	if osc99_unsafe(payload) {
		format!("\x1b]99;{}:e=1;{}\x1b\\", meta.join(":"), base64_encode(payload))
	} else {
		format!("\x1b]99;{};{}\x1b\\", meta.join(":"), payload)
	}
}

fn osc99_payload(meta: &[String], payload: &str, hold_until_later_payload: bool) -> String {
	let chunks = chunk_utf8(payload);
	let mut out = String::new();
	for (index, chunk) in chunks.iter().enumerate() {
		let mut chunk_meta = meta.to_vec();
		if hold_until_later_payload || index < chunks.len() - 1 {
			chunk_meta.push("d=0".to_string());
		}
		out.push_str(&osc99_chunk(&chunk_meta, chunk));
	}
	out
}

fn osc99_urgency(urgency: NotificationUrgency) -> &'static str {
	match urgency {
		NotificationUrgency::Low => "0",
		NotificationUrgency::Normal => "1",
		NotificationUrgency::Critical => "2",
	}
}

fn osc99_actions(actions: NotificationActions) -> &'static str {
	match actions {
		NotificationActions::Focus => "focus",
		NotificationActions::Report => "report",
		NotificationActions::FocusReport => "focus,report",
		NotificationActions::None => "-focus",
	}
}

/// Collapse a structured notification to a single line for non-OSC-99 sinks.
fn notification_to_line(n: &TerminalNotification) -> String {
	match (&n.title, &n.body) {
		(Some(title), Some(body)) => format!("{title}: {body}"),
		(Some(title), None) => title.clone(),
		(None, Some(body)) => body.clone(),
		(None, None) => String::new(),
	}
}

/// Format a structured notification as OSC 99 title/body payloads.
///
/// Title and body chunks share one id. Every non-final chunk carries `d=0`; the
/// final title or body chunk displays the notification. Metadata values that
/// require it are base64-encoded.
pub fn format_osc99_notification(n: &TerminalNotification) -> String {
	let id = osc99_id(n.id.as_deref());
	let mut meta = vec![format!("i={id}"), format!("f={}", base64_encode(OSC99_APP_NAME))];
	if let Some(actions) = n.actions {
		meta.push(format!("a={}", osc99_actions(actions)));
	}
	if let Some(urgency) = n.urgency {
		meta.push(format!("u={}", osc99_urgency(urgency)));
	}
	if let Some(types) = &n.notification_type {
		for notification_type in types {
			meta.push(format!("t={}", base64_encode(notification_type)));
		}
	}
	if let Some(icon_name) = &n.icon_name {
		meta.push(format!("n={}", base64_encode(icon_name)));
	}
	if let Some(sound) = n.sound {
		meta.push(format!("s={}", base64_encode(sound.as_str())));
	}
	if let Some(expires_ms) = n.expires_ms {
		if expires_ms.is_finite() {
			meta.push(format!("w={}", expires_ms.max(-1.0).trunc() as i64));
		}
	}

	let title = n.title.clone().or_else(|| n.body.clone()).unwrap_or_default();
	let body = if n.title.is_some() { n.body.clone() } else { None };

	if let Some(body) = body.filter(|body| !body.is_empty()) {
		return format!(
			"{}{}",
			osc99_payload(&meta, &title, true),
			osc99_payload(&[format!("i={id}"), "p=body".to_string()], &body, false)
		);
	}
	osc99_payload(&meta, &title, false)
}

/**
 * Give an empty first band an explicit transparent width. Zellij derives the
 * decoded image width from its first pixel row, ignoring the raster minimum for
 * transparent images; an omitted band otherwise makes the image invisible.
 * Palette definitions stay before drawing so cropping can still parse them.
 *
 * Port of `ensureSixelRasterWidth` from `l/src/sixel.ts`: the upstream regex uses
 * a lookahead, which the `regex` crate does not support, so the grammar is
 * walked by hand.
 */
pub fn ensure_sixel_raster_width(sequence: &str) -> String {
	let bytes = sequence.as_bytes();
	let len = bytes.len();
	let mut i = 2usize;
	if len < 4 || bytes[0] != 0x1b || bytes[1] != b'P' {
		return sequence.to_string();
	}
	while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b';') {
		i += 1;
	}
	if i >= len || bytes[i] != b'q' {
		return sequence.to_string();
	}
	i += 1;
	if i >= len || bytes[i] != b'"' {
		return sequence.to_string();
	}
	i += 1;

	let mut numbers = [0u32; 4];
	for slot in numbers.iter_mut() {
		let start = i;
		while i < len && bytes[i].is_ascii_digit() {
			i += 1;
		}
		if i == start {
			return sequence.to_string();
		}
		let text = std::str::from_utf8(&bytes[start..i]).unwrap_or("0");
		*slot = text.parse().unwrap_or(0);
		if i < len && bytes[i] == b';' {
			i += 1;
		} else {
			break;
		}
	}
	if i < len && bytes[i] == b';' {
		// A fifth number means the raster header is not the expected shape.
		return sequence.to_string();
	}
	let width = numbers[2];

	loop {
		if i < len && bytes[i] == b'#' {
			i += 1;
			let start = i;
			while i < len && bytes[i].is_ascii_digit() {
				i += 1;
			}
			if i == start {
				return sequence.to_string();
			}
			if i < len && bytes[i] == b';' {
				let save = i;
				i += 1;
				let mut matched = false;
				if i < len && (bytes[i] == b'1' || bytes[i] == b'2') {
					i += 1;
					matched = true;
					for _ in 0..3 {
						if i < len && bytes[i] == b';' {
							i += 1;
						} else {
							matched = false;
							break;
						}
						let number_start = i;
						while i < len && bytes[i].is_ascii_digit() {
							i += 1;
						}
						if i == number_start {
							matched = false;
							break;
						}
					}
				}
				if !matched {
					i = save;
				}
			}
			continue;
		}
		if i < len && bytes[i] == b'$' {
			i += 1;
			continue;
		}
		break;
	}
	if i >= len || bytes[i] != b'-' {
		return sequence.to_string();
	}

	format!("{}!{width}?$${}", &sequence[..i], &sequence[i..])
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::kitty_graphics::KITTY_PLACEHOLDER;

	fn env(vars: &[(&str, &str)]) -> TestEnv {
		let mut env = TestEnv::new();
		for (key, value) in vars {
			env.insert(key, value);
		}
		env
	}

	#[test]
	fn detects_terminal_id_from_markers_in_precedence_order() {
		assert_eq!(detect_terminal_id(&env(&[("KITTY_WINDOW_ID", "1")])), TerminalId::Kitty);
		assert_eq!(detect_terminal_id(&env(&[("GHOSTTY_RESOURCES_DIR", "/x")])), TerminalId::Ghostty);
		assert_eq!(detect_terminal_id(&env(&[("WEZTERM_PANE", "0")])), TerminalId::Wezterm);
		assert_eq!(detect_terminal_id(&env(&[("ITERM_SESSION_ID", "x")])), TerminalId::ITerm2);
		assert_eq!(detect_terminal_id(&env(&[("VSCODE_PID", "1")])), TerminalId::VSCode);
		assert_eq!(detect_terminal_id(&env(&[("ALACRITTY_WINDOW_ID", "1")])), TerminalId::Alacritty);
		assert_eq!(detect_terminal_id(&env(&[("TERM_PROGRAM", "iTerm.app")])), TerminalId::ITerm2);
		assert_eq!(detect_terminal_id(&env(&[("TERM_PROGRAM", "WarpTerminal")])), TerminalId::Warp);
		assert_eq!(detect_terminal_id(&env(&[("COLORTERM", "truecolor")])), TerminalId::TrueColor);
		assert_eq!(detect_terminal_id(&env(&[])), TerminalId::Base);
	}

	#[test]
	fn iterm2_lc_terminal_survives_ssh_but_not_a_multiplexer() {
		let ssh = env(&[("LC_TERMINAL", "iTerm2"), ("LC_TERMINAL_VERSION", "3.5.0")]);
		assert_eq!(detect_terminal_id(&ssh), TerminalId::ITerm2);
		let tmux = env(&[("LC_TERMINAL", "iTerm2"), ("TMUX", "x")]);
		assert_eq!(detect_terminal_id(&tmux), TerminalId::Base);
	}

	#[test]
	fn tmux_client_names_and_term_fallback_resolve_identity() {
		let client = env(&[("TMUX", "x"), ("MAPLE_TMUX_CLIENT_TERMINAL_NAMES", "xterm-kitty,kitty")]);
		assert_eq!(detect_terminal_id(&client), TerminalId::Kitty);
		assert_eq!(detect_terminal_id(&env(&[("TERM", "xterm-ghostty")])), TerminalId::Ghostty);
		assert_eq!(detect_terminal_id(&env(&[("TERM", "monstar")])), TerminalId::Monstar);
		assert_eq!(detect_terminal_id(&env(&[("TERM", "xterm-rex")])), TerminalId::Rex);
	}

	#[test]
	fn forced_image_protocol_parses_kill_switch_and_unknown_values() {
		assert_eq!(forced_image_protocol(&env(&[])), None);
		assert_eq!(forced_image_protocol(&env(&[("MAPLE_FORCE_IMAGE_PROTOCOL", "kitty")])), Some(Some(ImageProtocol::Kitty)));
		assert_eq!(forced_image_protocol(&env(&[("MAPLE_FORCE_IMAGE_PROTOCOL", "iterm")])), Some(Some(ImageProtocol::Iterm2)));
		assert_eq!(forced_image_protocol(&env(&[("MAPLE_FORCE_IMAGE_PROTOCOL", "sixel")])), Some(Some(ImageProtocol::Sixel)));
		assert_eq!(forced_image_protocol(&env(&[("MAPLE_FORCE_IMAGE_PROTOCOL", "off")])), Some(None));
		assert_eq!(forced_image_protocol(&env(&[("MAPLE_FORCE_IMAGE_PROTOCOL", "nonsense")])), Some(None));
	}

	#[test]
	fn windows_terminal_sixel_needs_win32_session_and_version() {
		let supported = env(&[("WT_SESSION", "x"), ("TERM_PROGRAM", "Windows_Terminal"), ("TERM_PROGRAM_VERSION", "1.22.1")]);
		assert!(is_windows_terminal_preview_sixel_supported(&supported, Platform::Windows));
		let old = env(&[("WT_SESSION", "x"), ("TERM_PROGRAM_VERSION", "1.21.0")]);
		assert!(!is_windows_terminal_preview_sixel_supported(&old, Platform::Windows));
		assert!(!is_windows_terminal_preview_sixel_supported(&supported, Platform::Linux));
	}

	#[test]
	fn synchronized_output_honors_override_features_and_multiplexers() {
		assert_eq!(synchronized_output_user_override(&env(&[])), None);
		assert_eq!(synchronized_output_user_override(&env(&[("MAPLE_NO_SYNC_OUTPUT", "1")])), Some(false));
		assert_eq!(synchronized_output_user_override(&env(&[("MAPLE_FORCE_SYNC_OUTPUT", "1"), ("MAPLE_NO_SYNC_OUTPUT", "1")])), Some(false));
		assert_eq!(synchronized_output_user_override(&env(&[("MAPLE_TUI_SYNC_OUTPUT", "1")])), Some(true));

		let features = env(&[("TERM_FEATURES", "Sy")]);
		assert!(should_enable_synchronized_output_by_default(&features, TerminalId::Base));
		let tmux = env(&[("TMUX", "x")]);
		assert!(!should_enable_synchronized_output_by_default(&tmux, TerminalId::Kitty));
		let herdr = env(&[("HERDR_ENV", "1")]);
		assert!(should_enable_synchronized_output_by_default(&herdr, TerminalId::Base));
		assert!(should_enable_synchronized_output_by_default(&env(&[]), TerminalId::Kitty));
		assert!(!should_enable_synchronized_output_by_default(&env(&[]), TerminalId::Base));
	}

	#[test]
	fn rectangular_sgr_is_kitty_only_and_killed_by_multiplexer() {
		assert!(detect_rectangular_sgr_support(TerminalId::Kitty, &env(&[])));
		assert!(!detect_rectangular_sgr_support(TerminalId::Ghostty, &env(&[])));
		assert!(!detect_rectangular_sgr_support(TerminalId::Kitty, &env(&[("MAPLE_NO_DECCARA", "1")])));
		assert!(detect_rectangular_sgr_support(TerminalId::Kitty, &env(&[("MAPLE_NO_DECCARA", "false")])));
		assert!(!detect_rectangular_sgr_support(TerminalId::Kitty, &env(&[("TMUX", "x")])));
	}

	#[test]
	fn styled_underlines_gate_iterm2_on_version() {
		let modern = env(&[("TERM_PROGRAM_VERSION", "3.5.0")]);
		assert!(detect_styled_underline_support(TerminalId::ITerm2, &modern));
		let old = env(&[("TERM_PROGRAM_VERSION", "3.4.9")]);
		assert!(!detect_styled_underline_support(TerminalId::ITerm2, &old));
		let missing = env(&[]);
		assert!(!detect_styled_underline_support(TerminalId::ITerm2, &missing));
		assert!(detect_styled_underline_support(TerminalId::Wezterm, &missing));
		let tmux = env(&[("TMUX", "x")]);
		assert!(!detect_styled_underline_support(TerminalId::Kitty, &tmux));
	}

	#[test]
	fn hyperlink_policies_follow_overrides_and_multiplexer_sessions() {
		assert_eq!(hyperlinks_user_override(&env(&[])), None);
		assert_eq!(hyperlinks_user_override(&env(&[("MAPLE_NO_HYPERLINKS", "1"), ("MAPLE_FORCE_HYPERLINKS", "1")])), Some(false));
		assert_eq!(hyperlinks_user_override(&env(&[("MAPLE_FORCE_HYPERLINKS", "1")])), Some(true));

		assert!(should_enable_hyperlinks_by_default(&env(&[]), TerminalId::Kitty));
		assert!(!should_enable_hyperlinks_by_default(&env(&[]), TerminalId::Base));
		let screen = env(&[("STY", "x")]);
		assert!(!should_enable_hyperlinks_by_default(&screen, TerminalId::Kitty));
		let herdr = env(&[("HERDR_ENV", "1")]);
		assert!(should_enable_hyperlinks_by_default(&herdr, TerminalId::Base));
		let tmux_old = env(&[("TMUX", "x"), ("TERM_PROGRAM", "tmux"), ("TERM_PROGRAM_VERSION", "3.3.0")]);
		assert!(!should_enable_hyperlinks_by_default(&tmux_old, TerminalId::Kitty));
		let tmux_new = env(&[("TMUX", "x"), ("TERM_PROGRAM", "tmux"), ("TERM_PROGRAM_VERSION", "3.4.0")]);
		assert!(should_enable_hyperlinks_by_default(&tmux_new, TerminalId::Kitty));
		let term_only = env(&[("TERM", "tmux-256color")]);
		assert!(!should_enable_hyperlinks_by_default(&term_only, TerminalId::Kitty));
	}

	#[test]
	fn image_protocol_selection_applies_warp_paseo_and_multiplexer_carveouts() {
	let mut tty = env(&[]);
	tty.set_tty(true);
		assert_eq!(resolve_image_protocol(TerminalId::Kitty, &tty, true), Some(ImageProtocol::Kitty));
		assert_eq!(resolve_image_protocol(TerminalId::VSCode, &tty, true), None);
		assert_eq!(resolve_image_protocol(TerminalId::Base, &tty, false), None);
		assert_eq!(resolve_image_protocol(TerminalId::Warp, &tty, true), Some(ImageProtocol::Kitty));
		assert_eq!(resolve_warp_image_protocol(Platform::Windows, &tty), None);
		assert_eq!(resolve_warp_image_protocol(Platform::Linux, &env(&[("WSL_DISTRO_NAME", "Ubuntu")])), None);

		let paseo = env(&[("TERM_PROGRAM", "kitty"), ("PASEO_TERMINAL_ID", "1")]);
		assert_eq!(resolve_image_protocol(TerminalId::Kitty, &paseo, true), None);

		let herdr = env(&[("HERDR_ENV", "1"), ("TERM_PROGRAM", "kitty")]);
		assert_eq!(resolve_image_protocol(TerminalId::Kitty, &herdr, true), None);

		let tmux = env(&[("TMUX", "x"), ("TERM_PROGRAM", "kitty")]);
		assert_eq!(resolve_image_protocol(TerminalId::Kitty, &tmux, true), None);
		let tmux_allowed = env(&[("TMUX", "x"), ("TERM_PROGRAM", "kitty"), ("MAPLE_TMUX_ALLOW_PASSTHROUGH", "on")]);
		assert_eq!(resolve_image_protocol(TerminalId::Kitty, &tmux_allowed, true), Some(ImageProtocol::Kitty));

	let mut screen = env(&[("TERM", "screen-256color")]);
	screen.set_tty(true);
		assert_eq!(resolve_image_protocol(TerminalId::Base, &screen, true), Some(ImageProtocol::Kitty));
	}

	#[test]
	fn fallback_protocol_needs_a_tty_and_avoids_tmux() {
	let mut tty = env(&[]);
	tty.set_tty(true);
		assert_eq!(get_fallback_image_protocol(TerminalId::Base, &tty, true), None);
		assert_eq!(get_fallback_image_protocol(TerminalId::Base, &tty, false), None);
	let mut ghostty = env(&[("TERM", "xterm-ghostty")]);
	ghostty.set_tty(true);
		assert_eq!(get_fallback_image_protocol(TerminalId::Base, &ghostty, true), Some(ImageProtocol::Kitty));
	let mut tmux = env(&[("TMUX", "x")]);
	tmux.set_tty(true);
		assert_eq!(get_fallback_image_protocol(TerminalId::Base, &tmux, true), None);
	}

	#[test]
	fn image_marker_detection_matches_protocol_grammar() {
		let kitty = TerminalInfo::new(TerminalId::Kitty, Some(ImageProtocol::Kitty), true, true);
		assert!(kitty.has_image_marker_at("\x1b_Ga=p,q=2\x1b\\", 0));
		assert!(kitty.has_image_marker_at("xx\x1b_Ga=p,q=2\x1b\\", 2));
		assert!(!kitty.has_image_marker_at("plain text", 0));
		assert!(kitty.has_image_marker_at(&format!("a{KITTY_PLACEHOLDER}b"), 1));

		let iterm = TerminalInfo::new(TerminalId::ITerm2, Some(ImageProtocol::Iterm2), true, true);
		assert!(iterm.has_image_marker_at("\x1b]1337;File=1:a\x07", 0));

		let sixel = TerminalInfo::new(TerminalId::Base, Some(ImageProtocol::Sixel), true, true);
		assert!(sixel.has_image_marker_at("\x1bP0;1q#1;2;3;4;5-~~~\x1b\\", 0));
		assert!(!sixel.has_image_marker_at("\x1bP0;1r#1;2;3;4;5~~~\x1b\\", 0));

		assert!(kitty.is_image_line(&format!("row {KITTY_PLACEHOLDER} text")));
		assert!(sixel.is_image_line("a\x1bPq#0;2;1;2;3-\x1b\\"));
		assert!(!kitty.is_image_line("no markers here"));
	}

	#[test]
	fn background_appearance_uses_reported_color_then_colorfgbg() {
		set_terminal_background_color(None);
		let dark = env(&[("COLORFGBG", "15;0")]);
		assert_eq!(detect_terminal_background_appearance(&dark, Platform::Linux), BackgroundAppearance::Dark);
		let light = env(&[("COLORFGBG", "0;15")]);
		assert_eq!(detect_terminal_background_appearance(&light, Platform::Linux), BackgroundAppearance::Light);
		let none = env(&[]);
		assert_eq!(detect_terminal_background_appearance(&none, Platform::Linux), BackgroundAppearance::Dark);

		set_terminal_background_color(Some(0xffffff));
		assert_eq!(detect_terminal_background_appearance(&none, Platform::Linux), BackgroundAppearance::Light);
		set_terminal_background_color(Some(0x000000));
		assert_eq!(detect_terminal_background_appearance(&none, Platform::Linux), BackgroundAppearance::Dark);
		set_terminal_background_color(None);
	}

	#[test]
	fn background_query_is_untrusted_on_broken_platform_sessions() {
		assert!(is_terminal_background_query_trusted(&env(&[]), Platform::Macos));
		let zellij = env(&[("ZELLIJ", "0")]);
		assert!(!is_terminal_background_query_trusted(&zellij, Platform::Macos));
		assert!(is_terminal_background_query_trusted(&zellij, Platform::Linux));
	}

	#[test]
	fn sixel_raster_width_inserts_transparent_first_band() {
		let sequence = "\x1bP0;1q\"2;3;5;7#1;2;1;2;3$-!5~\x1b\\";
		assert_eq!(
			ensure_sixel_raster_width(sequence),
			"\x1bP0;1q\"2;3;5;7#1;2;1;2;3$!5?$$-!5~\x1b\\"
		);
		assert_eq!(ensure_sixel_raster_width("not sixel"), "not sixel");
		assert_eq!(ensure_sixel_raster_width("\x1bPq\"1;1;1;1"), "\x1bPq\"1;1;1;1");
	}

	#[test]
	fn osc99_formatting_encodes_metadata_and_chunks_long_payloads() {
		let notification = TerminalNotification {
			title: Some("title".to_string()),
			body: Some("body".to_string()),
			id: Some("abc".to_string()),
			urgency: Some(NotificationUrgency::Critical),
			actions: Some(NotificationActions::FocusReport),
			expires_ms: Some(5000.0),
			..TerminalNotification::default()
		};
		let formatted = format_osc99_notification(&notification);
		assert!(formatted.contains("i=abc"));
		assert!(formatted.contains("u=2"));
		assert!(formatted.contains("a=focus,report"));
		assert!(formatted.contains("w=5000"));
		assert!(formatted.contains("p=body"));
		assert!(formatted.contains(&base64_encode("omp")));

		let unsafe_payload = TerminalNotification {
			title: Some("a\nb".to_string()),
			..TerminalNotification::default()
		};
		let formatted = format_osc99_notification(&unsafe_payload);
		assert!(formatted.contains("e=1;"));
		assert!(!formatted.contains('\n'));

		let long = "x".repeat(OSC99_MAX_PAYLOAD_BYTES * 2);
		let chunked = format_osc99_notification(&TerminalNotification {
			title: Some(long),
			..TerminalNotification::default()
		});
		assert!(chunked.matches("d=0").count() >= 1);
	}

	#[test]
	fn osc99_id_is_sanitized_or_generated() {
		assert_eq!(sanitize_osc99_id(Some("a b/c")), "abc");
		assert_eq!(sanitize_osc99_id(Some("0")), "");
		assert_eq!(sanitize_osc99_id(None), "");
		assert!(osc99_id(Some("ok")).contains("ok"));
		assert!(osc99_id(Some("0")).starts_with("omp-"));
	}

	#[test]
	fn notification_formatting_collapses_for_non_osc99_sinks() {
		let info = TerminalInfo {
			notify_protocol: NotifyProtocol::Osc9,
			..TerminalInfo::default()
		};
		assert_eq!(
			info.format_notification(Notification::Structured(&TerminalNotification {
				title: Some("t".to_string()),
				body: Some("b".to_string()),
				..TerminalNotification::default()
			})),
			"\x1b]9;t: b\x1b\\"
		);
		assert_eq!(info.format_notification(Notification::Message("hi")), "\x1b]9;hi\x1b\\");
		let bell = TerminalInfo::default();
		assert_eq!(bell.format_notification(Notification::Message("hi")), "\x07");
	}

	#[test]
	fn runtime_terminal_setters_mutate_the_singleton() {
		let saved = terminal();
		set_terminal_image_protocol(Some(ImageProtocol::Sixel));
		assert_eq!(terminal().image_protocol, Some(ImageProtocol::Sixel));
		set_terminal_deccara(true);
		assert!(terminal().deccara);
		set_terminal_hyperlinks(true);
		assert!(terminal().hyperlinks);
		set_terminal_text_sizing(true);
		assert!(terminal().text_sizing);
		set_terminal_glyph_protocol(true);
		assert!(terminal().glyph_protocol);
		set_terminal_screen_to_scrollback(true);
		assert!(terminal().supports_screen_to_scrollback);
		*TERMINAL.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = saved;
	}

	#[test]
	fn terminal_profiles_match_upstream_capabilities() {
		let kitty = get_terminal_info(TerminalId::Kitty, Platform::Linux, &env(&[]));
		assert_eq!(kitty.image_protocol, Some(ImageProtocol::Kitty));
		assert_eq!(kitty.notify_protocol, NotifyProtocol::Osc99);
		assert!(kitty.deccara && kitty.supports_text_sizing && kitty.supports_screen_to_scrollback);
		let ghostty = get_terminal_info(TerminalId::Ghostty, Platform::Linux, &env(&[]));
		assert_eq!(ghostty.hangul_jamo_width, HangulCompatibilityJamoWidth::Two);
		assert!(!ghostty.deccara);
		let base = get_terminal_info(TerminalId::Base, Platform::Linux, &env(&[]));
		assert_eq!(base.image_protocol, None);
		assert_eq!(base.notify_protocol, NotifyProtocol::Bell);
	}

	#[test]
	fn ssh_and_notification_suppression_flags_are_read_from_env() {
		assert!(is_ssh_session(&env(&[("SSH_TTY", "/dev/pts/0")])));
		assert!(!is_ssh_session(&env(&[])));
	}

	#[test]
	fn tmux_passthrough_escapes_escape_bytes() {
		assert_eq!(wrap_tmux_passthrough("\x1b_Ga=p\x1b\\"), "\x1bPtmux;\x1b\x1b_Ga=p\x1b\x1b\\\x1b\\");
		let direct = env(&[]);
		assert_eq!(wrap_tmux_passthrough_if_needed("x", &direct), "x");
		let tmux = env(&[("TMUX", "x")]);
		assert!(wrap_tmux_passthrough_if_needed("x", &tmux).starts_with("\x1bPtmux;"));
	}

	#[test]
	fn platform_maps_os_names() {
		assert_eq!(Platform::from_os("windows"), Platform::Windows);
		assert_eq!(Platform::from_os("macos"), Platform::Macos);
		assert_eq!(Platform::from_os("linux"), Platform::Linux);
		assert_eq!(Platform::from_os("freebsd"), Platform::Other);
		assert_eq!(Platform::Windows.as_str(), "win32");
	}
}
