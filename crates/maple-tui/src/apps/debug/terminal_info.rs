//! Terminal state collection for the debug menu.
//!
//! Reference: `l/src/apps/debug/terminal-info.ts`.
//!
//! Surfaces the detected terminal, the established subprotocols the renderer
//! negotiated (graphics, desktop notifications, hyperlinks, true color), the
//! scrollback/erase strategy, and the live geometry — the details that decide
//! which escape sequences the renderer emits.
//!
//! Deviations:
//! - The reference reads `Bun.env` directly; [`collect_terminal_state`] reads
//!   the process environment and [`collect_terminal_state_with_env`] accepts an
//!   [`EnvReader`] so tests need not touch the real environment.
//! - `classifyTerminalMultiplexer` becomes
//!   [`classify_terminal_multiplexer`], returning the session id.

use crate::terminal_capabilities::{
	classify_terminal_multiplexer, cell_dimensions, is_osc99_supported, terminal, terminal_id,
	EnvReader, ImageProtocol, MultiplexerSession, NotifyProtocol, ProcessEnv,
};

/// Live values the debug view reads off the running TUI, not the static
/// capability table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalRuntimeState {
	/// Live column count.
	pub columns: u16,
	/// Live row count.
	pub rows: u16,
	/// Whether DEC 2026 synchronized-output wrappers are currently emitted.
	pub synchronized_output: bool,
}

/// Detection-signal environment values shown under `Detection signals`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalStateEnv {
	/// `TERM`.
	pub term: Option<String>,
	/// `TERM_PROGRAM`.
	pub term_program: Option<String>,
	/// `TERM_PROGRAM_VERSION`.
	pub term_program_version: Option<String>,
	/// `COLORTERM`.
	pub colorterm: Option<String>,
}

/// Terminal capabilities, geometry, and detection environment.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalStateInfo {
	/// Detected terminal id.
	pub detected_id: String,
	/// Live column count.
	pub columns: u16,
	/// Live row count.
	pub rows: u16,
	/// Cell width in pixels.
	pub cell_width_px: u32,
	/// Cell height in pixels.
	pub cell_height_px: u32,
	/// Whether the terminal renders 24-bit color.
	pub true_color: bool,
	/// Human-readable image protocol, or `none`.
	pub image_protocol: String,
	/// Human-readable notification protocol.
	pub notify_protocol: String,
	/// Whether OSC 99 was confirmed by a DA probe.
	pub osc99_confirmed: bool,
	/// Whether OSC 8 hyperlinks are enabled.
	pub hyperlinks: bool,
	/// Whether Kitty-style DECCARA rectangular SGR fills apply.
	pub deccara: bool,
	/// Whether screen-to-scrollback clears are supported.
	pub screen_to_scrollback: bool,
	/// Whether DEC 2026 synchronized output is currently emitted.
	pub synchronized_output: bool,
	/// Classified multiplexer id, when one is present.
	pub multiplexer: Option<String>,
	/// Detection-signal environment values.
	pub env: TerminalStateEnv,
}

const IMAGE_PROTOCOL_NAMES: &str = "none";

fn image_protocol_name(protocol: Option<ImageProtocol>) -> &'static str {
	match protocol {
		None => IMAGE_PROTOCOL_NAMES,
		Some(ImageProtocol::Kitty) => "Kitty graphics",
		Some(ImageProtocol::Iterm2) => "iTerm2 inline images",
		Some(ImageProtocol::Sixel) => "Sixel",
	}
}

fn notify_protocol_name(protocol: NotifyProtocol) -> &'static str {
	match protocol {
		NotifyProtocol::Bell => "BEL (\\a)",
		NotifyProtocol::Osc99 => "OSC 99 (kitty desktop notifications)",
		NotifyProtocol::Osc9 => "OSC 9 (iTerm2/WezTerm)",
	}
}

fn multiplexer_name(multiplexer: Option<MultiplexerSession>) -> Option<&'static str> {
	multiplexer.map(|session| session.id())
}

fn yes_no(value: bool) -> &'static str {
	if value {
		"yes"
	} else {
		"no"
	}
}

fn or_unset(value: &Option<String>) -> &str {
	value.as_deref().unwrap_or("(unset)")
}

/// Snapshot the active terminal capabilities and the live runtime geometry.
pub fn collect_terminal_state(runtime: &TerminalRuntimeState) -> TerminalStateInfo {
	collect_terminal_state_with_env(runtime, &ProcessEnv)
}

/// [`collect_terminal_state`] against an explicit environment reader.
pub fn collect_terminal_state_with_env(
	runtime: &TerminalRuntimeState,
	env: &impl EnvReader,
) -> TerminalStateInfo {
	let capabilities = terminal();
	let cell = cell_dimensions();
	TerminalStateInfo {
		detected_id: terminal_id().as_str().to_string(),
		columns: runtime.columns,
		rows: runtime.rows,
		cell_width_px: cell.width_px,
		cell_height_px: cell.height_px,
		true_color: capabilities.true_color,
		image_protocol: image_protocol_name(capabilities.image_protocol).to_string(),
		notify_protocol: notify_protocol_name(capabilities.notify_protocol).to_string(),
		osc99_confirmed: is_osc99_supported(),
		hyperlinks: capabilities.hyperlinks,
		deccara: capabilities.deccara,
		screen_to_scrollback: capabilities.supports_screen_to_scrollback,
		synchronized_output: runtime.synchronized_output,
		multiplexer: classify_terminal_multiplexer(env).map(|session| session.id().to_string()),
		env: TerminalStateEnv {
			term: env.get("TERM"),
			term_program: env.get("TERM_PROGRAM"),
			term_program_version: env.get("TERM_PROGRAM_VERSION"),
			colorterm: env.get("COLORTERM"),
		},
	}
}

/// Format terminal state for display in the debug menu.
pub fn format_terminal_state(info: &TerminalStateInfo) -> String {
	let multiplexer = info.multiplexer.as_deref().unwrap_or("none");
	let mut lines = vec![
		"Terminal State".to_string(),
		"━━━━━━━━━━━━━━".to_string(),
		format!("Detected:     {}", info.detected_id),
		format!(
			"Geometry:     {}x{} cells · cell {}x{}px",
			info.columns, info.rows, info.cell_width_px, info.cell_height_px
		),
		format!("Multiplexer:  {multiplexer}"),
		String::new(),
		"Subprotocols".to_string(),
		format!("  Graphics:     {}", info.image_protocol),
		format!(
			"  Notify:       {}{}",
			info.notify_protocol,
			if info.osc99_confirmed { " · confirmed via DA" } else { "" }
		),
		format!("  Hyperlinks:   {} (OSC 8)", yes_no(info.hyperlinks)),
		format!("  True color:   {} (24-bit SGR)", yes_no(info.true_color)),
		format!("  DECCARA:      {} (rectangular-SGR background fills)", yes_no(info.deccara)),
		format!("  Sync output:  {} (DEC 2026)", yes_no(info.synchronized_output)),
		String::new(),
		"Scrollback".to_string(),
		format!(
			"  Screen->history clear: {}",
			if info.screen_to_scrollback { "CSI 22 J" } else { "CSI 2 J (redraw)" }
		),
		String::new(),
		"Detection signals".to_string(),
		format!("  TERM:                 {}", or_unset(&info.env.term)),
		format!("  TERM_PROGRAM:         {}", or_unset(&info.env.term_program)),
		format!("  TERM_PROGRAM_VERSION: {}", or_unset(&info.env.term_program_version)),
		format!("  COLORTERM:            {}", or_unset(&info.env.colorterm)),
	];
	lines.join("\n")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::terminal_capabilities::TestEnv;

	fn runtime() -> TerminalRuntimeState {
		TerminalRuntimeState { columns: 120, rows: 40, synchronized_output: true }
	}

	#[test]
	fn protocol_names_match_the_reference_labels() {
		assert_eq!(image_protocol_name(None), "none");
		assert_eq!(image_protocol_name(Some(ImageProtocol::Kitty)), "Kitty graphics");
		assert_eq!(image_protocol_name(Some(ImageProtocol::Iterm2)), "iTerm2 inline images");
		assert_eq!(image_protocol_name(Some(ImageProtocol::Sixel)), "Sixel");
		assert_eq!(notify_protocol_name(NotifyProtocol::Bell), "BEL (\\a)");
		assert_eq!(
			notify_protocol_name(NotifyProtocol::Osc99),
			"OSC 99 (kitty desktop notifications)"
		);
		assert_eq!(notify_protocol_name(NotifyProtocol::Osc9), "OSC 9 (iTerm2/WezTerm)");
	}

	#[test]
	fn collects_geometry_env_and_multiplexer() {
		let mut env = TestEnv::new();
		env.insert("TERM", "tmux-256color");
		env.insert("TERM_PROGRAM", "tmux");
		env.insert("TERM_PROGRAM_VERSION", "3.4");
		env.insert("COLORTERM", "truecolor");
		let info = collect_terminal_state_with_env(&runtime(), &env);
		assert_eq!(info.columns, 120);
		assert_eq!(info.rows, 40);
		assert!(info.synchronized_output);
		assert_eq!(info.env.term.as_deref(), Some("tmux-256color"));
		assert_eq!(info.env.term_program.as_deref(), Some("tmux"));
		assert_eq!(info.env.term_program_version.as_deref(), Some("3.4"));
		assert_eq!(info.env.colorterm.as_deref(), Some("truecolor"));
		assert_eq!(info.multiplexer.as_deref(), Some("tmux"));
		assert_eq!(info.detected_id, terminal_id().as_str());
	}

	#[test]
	fn unset_detection_signals_render_as_unset() {
		let env = TestEnv::new();
		let info = collect_terminal_state_with_env(&runtime(), &env);
		assert!(info.env.term.is_none());
		let text = format_terminal_state(&info);
		assert!(text.contains("  TERM:                 (unset)"));
		assert!(text.contains("  COLORTERM:            (unset)"));
	}

	#[test]
	fn format_matches_the_reference_layout() {
		let info = TerminalStateInfo {
			detected_id: "kitty".to_string(),
			columns: 80,
			rows: 24,
			cell_width_px: 9,
			cell_height_px: 18,
			true_color: true,
			image_protocol: "Kitty graphics".to_string(),
			notify_protocol: "OSC 99 (kitty desktop notifications)".to_string(),
			osc99_confirmed: true,
			hyperlinks: true,
			deccara: true,
			screen_to_scrollback: true,
			synchronized_output: false,
			multiplexer: Some("tmux".to_string()),
			env: TerminalStateEnv {
				term: Some("xterm-256color".to_string()),
				term_program: None,
				term_program_version: None,
				colorterm: None,
			},
		};
		let expected = "Terminal State\n\
━━━━━━━━━━━━━━\n\
Detected:     kitty\n\
Geometry:     80x24 cells · cell 9x18px\n\
Multiplexer:  tmux\n\
\n\
Subprotocols\n\
  Graphics:     Kitty graphics\n\
  Notify:       OSC 99 (kitty desktop notifications) · confirmed via DA\n\
  Hyperlinks:   yes (OSC 8)\n\
  True color:   yes (24-bit SGR)\n\
  DECCARA:      yes (rectangular-SGR background fills)\n\
  Sync output:  no (DEC 2026)\n\
\n\
Scrollback\n\
  Screen->history clear: CSI 22 J\n\
\n\
Detection signals\n\
  TERM:                 xterm-256color\n\
  TERM_PROGRAM:         (unset)\n\
  TERM_PROGRAM_VERSION: (unset)\n\
  COLORTERM:            (unset)";
		assert_eq!(format_terminal_state(&info), expected);
	}

	#[test]
	fn no_multiplexer_and_redraw_erase_render_the_fallback_lines() {
		let info = TerminalStateInfo {
			detected_id: "base".to_string(),
			columns: 1,
			rows: 1,
			cell_width_px: 1,
			cell_height_px: 1,
			true_color: false,
			image_protocol: "none".to_string(),
			notify_protocol: "BEL (\\a)".to_string(),
			osc99_confirmed: false,
			hyperlinks: false,
			deccara: false,
			screen_to_scrollback: false,
			synchronized_output: false,
			multiplexer: None,
			env: TerminalStateEnv::default(),
		};
		let text = format_terminal_state(&info);
		assert!(text.contains("Multiplexer:  none"));
		assert!(text.contains("  Screen->history clear: CSI 2 J (redraw)"));
		assert!(text.contains("  Notify:       BEL (\\a)\n"));
	}
}
