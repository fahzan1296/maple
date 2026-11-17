//! tmux session queries (port of `l/src/tmux.ts`).
//!
//! Reference: `l/src/tmux.ts`.
//!
//! The pure detection helpers (`wrap_tmux_passthrough`,
//! `wrap_tmux_passthrough_if_needed`, `resolve_tmux_client_terminal_names`,
//! `tmux_pane_allows_passthrough`) live in [`crate::terminal_capabilities`],
//! the I/O-free detection core. This module holds the subprocess half of the
//! reference file: the `tmux` CLI queries and the client-report parser.
//!
//! Deviations forced by Rust:
//!
//! - `NodeJS.ProcessEnv` → [`EnvReader`]; the spawned command inherits the
//!   process environment (the crate's notifier spawns do the same).
//! - The reference's per-process caches (`cachedClientTerminalNames`,
//!   `cachedClientSixel`, `cachedPanePassthrough`) are omitted; callers that
//!   need memoization key on [`crate::terminal_capabilities::terminal`].
//! - `$which("tmux")` → a `PATH` scan for a regular file named `tmux`.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::terminal_capabilities::EnvReader;

/// Client `TERM` values that name their emulator; tmux records this on every
/// version.
const CLIENT_TERMNAME_PROGRAMS: &[(&str, &str)] = &[
	("xterm-kitty", "kitty"),
	("xterm-ghostty", "ghostty"),
	("xterm-rex", "rex"),
	("wezterm", "wezterm"),
];

/// One `tmux display-message` round trip, bounded so startup cannot stall.
const CLIENT_TERMINAL_TIMEOUT: Duration = Duration::from_millis(2_000);

/// Bound for the short `tmux` option queries.
const TMUX_COMMAND_TIMEOUT: Duration = Duration::from_millis(500);

fn which_tmux(env: &impl EnvReader) -> Option<PathBuf> {
	let path = env.get("PATH").or_else(|| std::env::var("PATH").ok())?;
	path.split(':')
		.map(str::trim)
		.filter(|dir| !dir.is_empty())
		.map(|dir| PathBuf::from(dir).join("tmux"))
		.find(|candidate| candidate.is_file())
}

/// Run a `tmux` command against the current server; its stdout, or `None` on
/// failure or timeout.
fn run_tmux(args: &[&str], env: &impl EnvReader, timeout: Duration) -> Option<String> {
	let program = which_tmux(env)?;
	let mut child = Command::new(program)
		.args(args)
		.stdout(Stdio::piped())
		.stderr(Stdio::null())
		.spawn()
		.ok()?;
	let deadline = Instant::now() + timeout;
	loop {
		match child.try_wait() {
			Ok(Some(status)) => {
				if !status.success() {
					return None;
				}
				let mut out = String::new();
				if let Some(mut stdout) = child.stdout.take() {
					let _ = stdout.read_to_string(&mut out);
				}
				return Some(out);
			}
			Ok(None) => {
				if Instant::now() >= deadline {
					let _ = child.kill();
					let _ = child.wait();
					return None;
				}
				std::thread::sleep(Duration::from_millis(5));
			}
			Err(_) => return None,
		}
	}
}

/// Leading program name of a `#{client_termtype}` reply (`tmux 3.4` → `tmux`).
fn client_termtype_name(termtype: &str) -> Option<&str> {
	let bytes = termtype.as_bytes();
	if bytes.first().map_or(true, |byte| !byte.is_ascii_alphabetic()) {
		return None;
	}
	let mut end = 1;
	while let Some(byte) = bytes.get(end) {
		if byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'+' | b'-') {
			end += 1;
		} else {
			break;
		}
	}
	Some(&termtype[..end])
}

/// Emulator name candidates from a `#{client_termtype}<TAB>#{client_termname}`
/// line, best first: the terminal-type reply, then the emulator the client's
/// `TERM` names when it ships its own terminfo entry.
pub fn tmux_client_programs_from_report(report: &str) -> Vec<String> {
	let mut fields = report.splitn(2, '\t');
	let termtype = fields.next().unwrap_or("").trim();
	let termname = fields.next().unwrap_or("").trim().to_ascii_lowercase();
	let mut programs = Vec::new();
	if let Some(program) = client_termtype_name(termtype) {
		programs.push(program.to_string());
	}
	if let Some((_, program)) = CLIENT_TERMNAME_PROGRAMS
		.iter()
		.find(|(name, _)| *name == termname.as_str())
	{
		programs.push((*program).to_string());
	}
	programs
}

/// Whether the terminal hosting this tmux session draws TSP programs natively:
/// it sets the session user option `@tern-tsp` while it watches the session.
pub fn tmux_session_speaks_tsp(env: &impl EnvReader) -> bool {
	run_tmux(
		&["show-options", "-qv", "@tern-tsp"],
		env,
		TMUX_COMMAND_TIMEOUT,
	)
	.is_some_and(|out| !out.trim().is_empty())
}

/// Stop (`on` false) or restore (`on` true) tmux turning this pane's APC
/// strings into its title, which would otherwise swallow every TSP message.
pub fn set_tmux_pane_apc_titles(on: bool, env: &impl EnvReader) {
	let Some(pane) = env.get("TMUX_PANE") else {
		return;
	};
	if pane.is_empty() {
		return;
	}
	let args: &[&str] = if on {
		&["set-option", "-pu", "-t", pane.as_str(), "allow-set-title"]
	} else {
		&["set-option", "-p", "-t", pane.as_str(), "allow-set-title", "off"]
	};
	run_tmux(args, env, TMUX_COMMAND_TIMEOUT);
}

/// Whether a tmux terminal description carries the `Sxl` SIXEL capability.
fn sixel_capability(messages: &str) -> bool {
	messages.lines().any(|line| {
		let trimmed = line.trim_start();
		let Some((number, tail)) = trimmed.split_once(':') else {
			return false;
		};
		if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
			return false;
		}
		tail == " Sxl: (flag) true"
	})
}

/// Whether tmux draws SIXEL to the client attached to this session: its
/// effective terminal description for that client carries `Sxl`.
pub fn tmux_client_renders_sixel(env: &impl EnvReader) -> bool {
	let Some(tmux) = env.get("TMUX") else {
		return false;
	};
	if tmux.is_empty() {
		return false;
	}
	let client = run_tmux(
		&["display-message", "-p", "#{client_name}"],
		env,
		CLIENT_TERMINAL_TIMEOUT,
	)
	.map(|out| out.trim().to_string())
	.unwrap_or_default();
	if client.is_empty() {
		return false;
	}
	let messages = run_tmux(
		&["show-messages", "-T", "-t", client.as_str()],
		env,
		CLIENT_TERMINAL_TIMEOUT,
	)
	.unwrap_or_default();
	sixel_capability(&messages)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::terminal_capabilities::TestEnv;

	#[test]
	fn client_programs_from_report_are_best_first() {
		assert_eq!(
			tmux_client_programs_from_report("xterm-kitty\txterm-kitty"),
			vec!["xterm-kitty", "kitty"]
		);
		assert_eq!(
			tmux_client_programs_from_report("\txterm-ghostty"),
			vec!["ghostty"]
		);
		assert_eq!(
			tmux_client_programs_from_report("tmux 3.4\tscreen-256color"),
			vec!["tmux"]
		);
		assert_eq!(
			tmux_client_programs_from_report("wezterm\twezterm"),
			vec!["wezterm", "wezterm"]
		);
		assert!(tmux_client_programs_from_report("").is_empty());
	}

	#[test]
	fn sixel_capability_matches_the_terminal_description_line() {
		assert!(sixel_capability("  12: Sxl: (flag) true\n"));
		assert!(!sixel_capability("12: Sxl: (flag) false\n"));
		assert!(!sixel_capability("no capabilities here"));
	}

	#[test]
	fn sixel_query_requires_a_tmux_session() {
		let env = TestEnv::new();
		assert!(!tmux_client_renders_sixel(&env));
	}

	#[test]
	fn apc_title_change_requires_a_pane() {
		let env = TestEnv::new();
		set_tmux_pane_apc_titles(false, &env);
		set_tmux_pane_apc_titles(true, &env);
	}
}
