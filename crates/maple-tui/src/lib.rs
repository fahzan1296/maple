//! # maple-tui
//!
//! Terminal user-interface toolkit: a component model with differential
//! rendering, ANSI-aware text shaping, themes, and terminal backends.
//!
//! Clean-room Rust implementation of the `l/` reference tree. No TypeScript
//! source is copied; contracts and wire formats are reproduced.

mod ansi_scan;
pub mod active_terminal;

pub mod components;


pub mod component;
pub mod bracketed_paste;
pub mod debug_server;
pub mod deccara;
pub mod desktop_notify;
pub mod frame_rate;
pub mod keybindings;
pub mod keys;
pub mod loop_watchdog;
pub mod mouse;
pub mod mouse_sgr;
pub mod multiplexers;
pub mod screen;
pub mod style;
pub mod terminal;
pub mod ttyid;
pub mod spinner_clock;
pub mod symbols;
pub mod text_shape;
pub mod windows_altgr;

pub mod trigger;
pub mod tui;
pub mod windows_input;
pub mod fuzzy;
pub mod autocomplete;
pub mod chrome;
pub mod lang_from_path;
pub mod native;
pub mod render;
pub mod status_line;
pub mod theme;
pub mod glyph_protocol;
pub mod tools;
pub mod kitty_graphics;
pub mod latex_to_unicode;
pub mod terminal_capabilities;

/// Look up a maple-branded environment variable, falling back to the legacy
/// `PI_`-prefixed name so existing deployments and dotfiles keep working.
///
/// `env_lookup("MAPLE_TUI_THEMES_DIR")` therefore reads `MAPLE_TUI_THEMES_DIR`
/// first and `PI_TUI_THEMES_DIR` second.
pub fn env_lookup(name: &str) -> Option<std::ffi::OsString> {
	if let Some(value) = std::env::var_os(name) {
		return Some(value);
	}
	let legacy = name.replacen("MAPLE_", "PI_", 1);
	if legacy != name {
		return std::env::var_os(legacy);
	}
	None
}







pub use component::Component;
pub use ansi_scan::strip_ansi;
pub use keys::Key;
pub use terminal::{process::ProcessTerminal, virt::VirtualTerminal, Terminal, TtyEvent};
pub use text_shape::{wrap_text_with_ansi, Ellipsis};
pub use tui::Tui;
pub use trigger::Trigger;

