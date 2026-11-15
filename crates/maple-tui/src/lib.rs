//! # maple-tui
//!
//! Terminal user-interface toolkit: a component model with differential
//! rendering, ANSI-aware text shaping, themes, and terminal backends.
//!
//! Clean-room Rust implementation inspired by `@oh-my-pi/pi-tui` behavior.
//! No TypeScript source is copied; contracts and wire formats are reproduced.

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







pub use component::Component;
pub use keys::Key;
pub use terminal::{process::ProcessTerminal, virt::VirtualTerminal, Terminal, TtyEvent};
pub use text_shape::{wrap_text_with_ansi, Ellipsis};
pub use tui::Tui;
pub use trigger::Trigger;

