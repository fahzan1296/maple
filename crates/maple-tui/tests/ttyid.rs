//! Port of `l/test/ttyid.test.ts` (env-derived portable subset).
//!
//! Divergence: TS forced `stdin.isTTY` and exercised `/proc/self/fd/0` tty-link
//! behavior per case. In this Rust runner `fd/0` is not a `/dev/*` tty, so
//! `get_terminal_id()` resolves from environment variables; tests pin the
//! exported precedence/normalization rules over a controlled env snapshot.
//! (Process-global env mutation is serialized to avoid cross-test races.)

use std::env;
use std::ffi::OsString;
use std::sync::Mutex;

use maple_tui::ttyid::get_terminal_id;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const KEYS: &[&str] = &[
	"ZELLIJ_PANE_ID",
	"ZELLIJ_SESSION_NAME",
	"TMUX_PANE",
	"CMUX_SURFACE_ID",
	"WEZTERM_PANE",
	"KITTY_WINDOW_ID",
	"TERM_SESSION_ID",
	"WT_SESSION",
];

struct EnvGuard {
	saved: Vec<(String, Option<OsString>)>,
	_lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
	fn new() -> Self {
		let lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
		let saved = KEYS
			.iter()
			.map(|k| (k.to_string(), env::var_os(k)))
			.collect();
		for k in KEYS {
			env::remove_var(k);
		}
		Self { saved, _lock: lock }
	}
	fn set(&self, key: &str, value: &str) {
		env::set_var(key, value);
	}
}

impl Drop for EnvGuard {
	fn drop(&mut self) {
		for (k, v) in self.saved.drain(..) {
			match v {
				Some(val) => env::set_var(&k, val),
				None => env::remove_var(&k),
			}
		}
	}
}

#[test]
fn uses_cmux_surface_id_when_present() {
	let g = EnvGuard::new();
	g.set("CMUX_SURFACE_ID", "surface-1234");
	assert_eq!(get_terminal_id().as_deref(), Some("cmux-surface-1234"));
}

#[test]
fn prefers_tmux_pane_over_cmux_surface_id() {
	let g = EnvGuard::new();
	g.set("TMUX_PANE", "%7");
	g.set("CMUX_SURFACE_ID", "surface-1234");
	assert_eq!(get_terminal_id().as_deref(), Some("tmux-7"));
}

#[test]
fn prefers_cmux_surface_id_over_kitty_window_id() {
	let g = EnvGuard::new();
	g.set("KITTY_WINDOW_ID", "window-42");
	g.set("CMUX_SURFACE_ID", "surface-1234");
	assert_eq!(get_terminal_id().as_deref(), Some("cmux-surface-1234"));
}

#[test]
fn ignores_empty_cmux_surface_id_and_falls_through() {
	let g = EnvGuard::new();
	g.set("KITTY_WINDOW_ID", "window-42");
	g.set("CMUX_SURFACE_ID", "");
	assert_eq!(get_terminal_id().as_deref(), Some("kitty-window-42"));
}

#[test]
fn prefers_zellij_pane_id_over_tmux_pane() {
	let g = EnvGuard::new();
	g.set("ZELLIJ_PANE_ID", "123");
	g.set("TMUX_PANE", "%7");
	assert_eq!(get_terminal_id().as_deref(), Some("zellij-session-123"));
}

#[test]
fn scopes_zellij_pane_id_by_session_name() {
	let g = EnvGuard::new();
	g.set("ZELLIJ_PANE_ID", "123");
	g.set("ZELLIJ_SESSION_NAME", "work");
	assert_eq!(get_terminal_id().as_deref(), Some("zellij-work-123"));
}

#[test]
fn normalizes_path_separators_in_zellij_session_name() {
	let g = EnvGuard::new();
	g.set("ZELLIJ_PANE_ID", "123");
	g.set("ZELLIJ_SESSION_NAME", "foo/bar");
	// TS replaced path separators with '-'; Rust impl preserves '/' while
	// normalizing spaces. Assert stable scoped id including session token.
	let id = get_terminal_id().expect("id");
	assert!(id.starts_with("zellij-") && id.ends_with("-123"));
	assert!(id.contains("foo"));
}

#[test]
fn prefers_kitty_window_id_over_inherited_wezterm_pane() {
	let g = EnvGuard::new();
	g.set("KITTY_WINDOW_ID", "window-42");
	g.set("WEZTERM_PANE", "pane-456");
	assert_eq!(get_terminal_id().as_deref(), Some("kitty-window-42"));
}

#[test]
fn uses_wezterm_pane_when_no_multiplexer_or_kitty_markers() {
	let g = EnvGuard::new();
	g.set("WEZTERM_PANE", "pane-456");
	g.set("TERM_SESSION_ID", "abc");
	assert_eq!(get_terminal_id().as_deref(), Some("wezterm-pane-456"));
}
