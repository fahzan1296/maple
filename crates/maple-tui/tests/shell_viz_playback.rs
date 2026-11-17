//! Port of `l/test/shell-viz-playback.test.ts` (portable subset).
//!
//! Deviations: the reference drives `ToolExecutionComponent` and the Bun-only
//! `RunTrace`/`Shell` boundary. The crate exposes the deterministic replay
//! controller (`ShellVizPlayback`) plus the process-wide display preferences;
//! the live native command lifecycle is not portable, so traces are built from
//! canonical retained lifecycle events.

use std::sync::Mutex;

use maple_tui::chat::display_preferences::{
	chat_transcript_display_preferences, set_chat_transcript_display_preferences,
	ChatTranscriptDisplayPreferencesPatch,
};
use maple_tui::theme::default_theme;
use maple_tui::tools::shell_viz::{
	default_shell_viz_preferences, ShellTrace, ShellTraceEvent, ShellVizPlayback,
};

static PREFS_LOCK: Mutex<()> = Mutex::new(());

fn lock_prefs() -> std::sync::MutexGuard<'static, ()> {
	PREFS_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn patch(shell_visualization: bool) -> ChatTranscriptDisplayPreferencesPatch {
	ChatTranscriptDisplayPreferencesPatch {
		shell_visualization: Some(shell_visualization),
		..ChatTranscriptDisplayPreferencesPatch::default()
	}
}

fn event(kind: &str, seq: u64, t_ns: u64) -> ShellTraceEvent {
	ShellTraceEvent {
		kind: kind.to_string(),
		seq,
		t_ns,
		text: None,
		dur_ns: None,
		start: Some(0),
		end: Some(15),
	}
}

/// Canonical lifecycle stretched over a deterministic 10s playback clock.
fn completed_trace(output: &str) -> ShellTrace {
	ShellTrace {
		seq: 5,
		t_ns: 10_000_000_000,
		script: "printf 'done\\n'".to_string(),
		history: vec![
			event("RunStart", 1, 0),
			event("CmdStart", 2, 0),
			ShellTraceEvent {
				kind: "Output".to_string(),
				seq: 3,
				t_ns: 10_000_000_000,
				text: Some(output.to_string()),
				dur_ns: None,
				start: Some(0),
				end: Some(15),
			},
			ShellTraceEvent {
				kind: "CmdEnd".to_string(),
				seq: 4,
				t_ns: 10_000_000_000,
				text: None,
				dur_ns: Some(10_000_000_000),
				start: Some(0),
				end: Some(15),
			},
			event("RunEnd", 5, 10_000_000_000),
		],
		history_truncated: false,
	}
}

fn playback(trace: ShellTrace) -> ShellVizPlayback {
	let mut playback = ShellVizPlayback::new(trace, default_shell_viz_preferences());
	playback.set_visualization_enabled(chat_transcript_display_preferences().shell_visualization);
	playback
}

#[test]
fn plain_commands_and_output_when_visualization_disabled() {
	let _guard = lock_prefs();
	set_chat_transcript_display_preferences(patch(false));
	let trace = completed_trace("done\n");

	let mut pending = playback(trace.clone());
	pending.set_result_output("final output");
	assert!(pending.presentation().is_none());
	let text = pending.render_lines(&default_theme(), 100).join("\n");
	assert!(text.contains("printf"));
	assert!(text.contains("final output"));

	let mut component = playback(trace);
	component.set_result_output("final output");
	assert!(component.presentation().is_none());
	let text = component.render_lines(&default_theme(), 100).join("\n");
	assert!(text.contains("printf"));
	assert!(text.contains("final output"));

	set_chat_transcript_display_preferences(patch(true));
	pending.set_visualization_enabled(true);
	component.set_visualization_enabled(true);
	assert!(pending.presentation().is_some());
	assert!(component.presentation().is_some());
}

#[test]
fn unreachable_offsets_fall_back_to_plain_render() {
	let _guard = lock_prefs();
	let mut trace = completed_trace("done\n");
	// A tool_result hook rewrote `script`; the retained offsets now point past
	// its end, so the engine cannot restore the visualization.
	trace.script = trace.script.as_str()[..trace.script.len() - 4].to_string();
	let mut playback = playback(trace);
	playback.set_result_output("final output");
	assert!(!playback.restorable());
	assert!(playback.presentation().is_none());
	let text = playback.render_lines(&default_theme(), 100).join("\n");
	assert!(text.contains("printf"));
	assert!(text.contains("final output"));
}

#[test]
fn pause_resumes_the_remaining_gap_instead_of_restarting() {
	let _guard = lock_prefs();
	let mut playback = playback(completed_trace("done\n"));
	playback.action("play");
	playback.advance(8_000.0);
	let presentation = playback.presentation().expect("presentation");
	assert_eq!(presentation.output, "");
	assert_eq!(presentation.running, Some(true));
	playback.action("pause");
	playback.advance(1_000.0);
	playback.action("play");
	playback.advance(2_000.0);
	let presentation = playback.presentation().expect("presentation");
	assert_eq!(presentation.output, "done\n");
	assert!(!presentation.preferences.playing);
}

#[test]
fn trimmed_playback_starts_at_first_retained_timestamp() {
	let _guard = lock_prefs();
	let mut trace = completed_trace("done\n");
	trace.history_truncated = true;
	trace.t_ns = 301_000_000_000;
	trace.history = vec![
		ShellTraceEvent {
			kind: "Output".to_string(),
			seq: 101,
			t_ns: 300_000_000_000,
			text: Some("retained\n".to_string()),
			dur_ns: None,
			start: Some(0),
			end: Some(15),
		},
		event("RunEnd", 102, 301_000_000_000),
	];
	let mut playback = playback(trace);
	playback.action("reset");
	playback.action("play");
	playback.advance(80.0);
	let presentation = playback.presentation().expect("presentation");
	assert_eq!(presentation.output, "retained\n");
	assert_eq!(presentation.timeline.cursor, 1.0);
}

#[test]
fn replay_strips_unsafe_controls_without_mutating_retained_bytes() {
	let _guard = lock_prefs();
	let unsafe_text = "safe\x1b[2J\x1b]52;c;YWJj\x07tail\n";
	let trace = completed_trace(unsafe_text);
	let mut playback = playback(trace.clone());
	playback.action("seek:3");
	let rendered = playback.render_lines(&default_theme(), 100).join("\n");
	assert!(!rendered.contains("\x1b[2J"));
	assert!(!rendered.contains("\x1b]52"));
	assert!(rendered.contains("safetail"));
	assert_eq!(trace.history[2].text.as_deref(), Some(unsafe_text));
}
