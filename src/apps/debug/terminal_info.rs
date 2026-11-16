//! Terminal state collection for the debug menu.
//!
//! Reference: `l/src/apps/debug/terminal-info.ts`. Surfaces the detected
//! terminal, the negotiated subprotocols, the scrollback/erase strategy and the
//! live geometry — the details that decide which escape sequences the renderer
//! emits.

use maple_tui::terminal_capabilities::{
	cell_dimensions, classify_terminal_multiplexer, is_osc99_supported, terminal, terminal_id, EnvReader,
	ImageProtocol, NotifyProtocol, ProcessEnv,
};

/// Live values the debug view reads off the running TUI, not the static
/// capability table.
#[derive(Clone, Copy, Debug, Default)]
pub struct TerminalRuntimeState {
	/// Live column count.
	pub columns: u16,
	/// Live row count.
	pub rows: u16,
	/// Whether DEC 2026 synchronized-output wrappers are currently emitted.
	pub synchronized_output: bool,
}

/// Environment signals shown by the debug view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalEnvSignals {
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
	/// Human-readable inline image protocol, or `none`.
	pub image_protocol: String,
	/// Human-readable desktop-notification protocol.
	pub notify_protocol: String,
	/// Whether OSC 99 was confirmed by a DA reply.
	pub osc99_confirmed: bool,
	/// Whether OSC 8 hyperlinks are enabled.
	pub hyperlinks: bool,
	/// Whether DECCARA rectangular-SGR fills apply.
	pub deccara: bool,
	/// Whether screen-to-scrollback clears are supported.
	pub screen_to_scrollback: bool,
	/// Whether DEC 2026 synchronized output is currently emitted.
	pub synchronized_output: bool,
	/// Multiplexer hosting the process, if any.
	pub multiplexer: Option<String>,
	/// Detection environment signals.
	pub env: TerminalEnvSignals,
}

/// Snapshot the active terminal capabilities and the live runtime geometry.
pub fn collect_terminal_state(runtime: TerminalRuntimeState) -> TerminalStateInfo {
	let env = ProcessEnv;
	let info = terminal();
	let cell = cell_dimensions();
	TerminalStateInfo {
		detected_id: terminal_id().as_str().to_string(),
		columns: runtime.columns,
		rows: runtime.rows,
		cell_width_px: cell.width_px,
		cell_height_px: cell.height_px,
		true_color: info.true_color,
		image_protocol: match info.image_protocol {
			Some(ImageProtocol::Kitty) => "Kitty graphics".to_string(),
			Some(ImageProtocol::Iterm2) => "iTerm2 inline images".to_string(),
			Some(ImageProtocol::Sixel) => "Sixel".to_string(),
			None => "none".to_string(),
		},
		notify_protocol: match info.notify_protocol {
			NotifyProtocol::Bell => "BEL (\\a)".to_string(),
			NotifyProtocol::Osc99 => "OSC 99 (kitty desktop notifications)".to_string(),
			NotifyProtocol::Osc9 => "OSC 9 (iTerm2/WezTerm)".to_string(),
		},
		osc99_confirmed: is_osc99_supported(),
		hyperlinks: info.hyperlinks,
		deccara: info.deccara,
		screen_to_scrollback: info.supports_screen_to_scrollback,
		synchronized_output: runtime.synchronized_output,
		multiplexer: classify_terminal_multiplexer(&env).map(|session| session.id().to_string()),
		env: TerminalEnvSignals {
			term: env.get("TERM"),
			term_program: env.get("TERM_PROGRAM"),
			term_program_version: env.get("TERM_PROGRAM_VERSION"),
			colorterm: env.get("COLORTERM"),
		},
	}
}

fn yes_no(value: bool) -> &'static str {
	if value { "yes" } else { "no" }
}

/// Format terminal state for display in the debug menu.
pub fn format_terminal_state(info: &TerminalStateInfo) -> String {
	[
		"Terminal State".to_string(),
		"━━━━━━━━━━━━━━".to_string(),
		format!("Detected:     {}", info.detected_id),
		format!(
			"Geometry:     {}x{} cells · cell {}x{}px",
			info.columns, info.rows, info.cell_width_px, info.cell_height_px
		),
		format!("Multiplexer:  {}", info.multiplexer.as_deref().unwrap_or("none")),
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
		format!("  TERM:                 {}", info.env.term.as_deref().unwrap_or("(unset)")),
		format!("  TERM_PROGRAM:         {}", info.env.term_program.as_deref().unwrap_or("(unset)")),
		format!(
			"  TERM_PROGRAM_VERSION: {}",
			info.env.term_program_version.as_deref().unwrap_or("(unset)")
		),
		format!("  COLORTERM:            {}", info.env.colorterm.as_deref().unwrap_or("(unset)")),
	]
	.join("\n")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn info() -> TerminalStateInfo {
		TerminalStateInfo {
			detected_id: "kitty".to_string(),
			columns: 120,
			rows: 40,
			cell_width_px: 9,
			cell_height_px: 18,
			true_color: true,
			image_protocol: "Kitty graphics".to_string(),
			notify_protocol: "OSC 99 (kitty desktop notifications)".to_string(),
			osc99_confirmed: true,
			hyperlinks: true,
			deccara: false,
			screen_to_scrollback: true,
			synchronized_output: true,
			multiplexer: Some("tmux".to_string()),
			env: TerminalEnvSignals {
				term: Some("xterm-kitty".to_string()),
				term_program: None,
				term_program_version: None,
				colorterm: Some("truecolor".to_string()),
			},
		}
	}

	#[test]
	fn format_reports_geometry_protocols_and_env() {
		let text = format_terminal_state(&info());
		assert!(text.starts_with("Terminal State\n━━━━━━━━━━━━━━\nDetected:     kitty\n"));
		assert!(text.contains("Geometry:     120x40 cells · cell 9x18px"));
		assert!(text.contains("Multiplexer:  tmux"));
		assert!(text.contains("  Graphics:     Kitty graphics"));
		assert!(text.contains("  Notify:       OSC 99 (kitty desktop notifications) · confirmed via DA"));
		assert!(text.contains("  Hyperlinks:   yes (OSC 8)"));
		assert!(text.contains("  True color:   yes (24-bit SGR)"));
		assert!(text.contains("  DECCARA:      no (rectangular-SGR background fills)"));
		assert!(text.contains("  Sync output:  yes (DEC 2026)"));
		assert!(text.contains("  Screen->history clear: CSI 22 J"));
		assert!(text.contains("  TERM:                 xterm-kitty"));
		assert!(text.contains("  TERM_PROGRAM:         (unset)"));
		assert!(text.contains("  COLORTERM:            truecolor"));
	}

	#[test]
	fn format_marks_absent_multiplexer_and_redraw_clear() {
		let mut state = info();
		state.multiplexer = None;
		state.screen_to_scrollback = false;
		let text = format_terminal_state(&state);
		assert!(text.contains("Multiplexer:  none"));
		assert!(text.contains("  Screen->history clear: CSI 2 J (redraw)"));
	}

	#[test]
	fn collect_reads_live_geometry_and_defaults() {
		let state = collect_terminal_state(TerminalRuntimeState { columns: 80, rows: 24, synchronized_output: false });
		assert_eq!((state.columns, state.rows), (80, 24));
		assert!(!state.synchronized_output);
		assert_eq!((state.cell_width_px, state.cell_height_px), (9, 18));
		assert!(!state.detected_id.is_empty());
	}
}
