//! Port of `l/test/bash-execution-pty.test.ts`.
//!
//! `BashExecutionComponent` (chat/bash-execution) and `TranscriptContainer` are
//! not ported, so the PTY replay/finalization assertions are ignored. The
//! transcript record they render — `BashExecutionMessage` — is pinned.

use maple_tui::chat::messages::BashExecutionMessage;

#[test]
fn a_default_record_carries_no_output_and_no_exit_code() {
	let message = BashExecutionMessage::default();
	assert_eq!(message.command, "");
	assert_eq!(message.output, "");
	assert_eq!(message.exit_code, None);
	assert!(!message.cancelled);
	assert!(!message.truncated);
	assert!(message.images.is_empty());
	assert!(message.meta.is_none());
	assert!(!message.exclude_from_context);
}

#[test]
fn records_the_command_output_and_exit_code() {
	let message = BashExecutionMessage {
		command: "lg".to_string(),
		output: "red plain\nsecond".to_string(),
		exit_code: Some(0),
		timestamp: 7,
		..BashExecutionMessage::default()
	};
	assert_eq!(message.command, "lg");
	assert_eq!(message.output, "red plain\nsecond");
	assert_eq!(message.exit_code, Some(0));
	assert_eq!(message.timestamp, 7);
}

#[test]
fn exit_7_is_preserved_as_a_distinct_completed_run() {
	let message = BashExecutionMessage {
		command: "markers".to_string(),
		output: "OUT-MARKER\nERR-MARKER".to_string(),
		exit_code: Some(7),
		..BashExecutionMessage::default()
	};
	assert_eq!(message.exit_code, Some(7));
	assert_eq!(message.output, "OUT-MARKER\nERR-MARKER");
}

#[test]
fn cancelled_and_truncated_flags_are_reported() {
	let message = BashExecutionMessage {
		cancelled: true,
		truncated: true,
		..BashExecutionMessage::default()
	};
	assert!(message.cancelled);
	assert!(message.truncated);
}

#[test]
#[ignore = "missing facility: BashExecutionComponent (chat/bash-execution) is not ported, so vterm PTY replay cannot be observed"]
fn replays_raw_pty_bytes_with_safe_color_preserved() {
	let message = BashExecutionMessage {
		output: "red plain\nsecond".to_string(),
		exit_code: Some(0),
		..BashExecutionMessage::default()
	};
	assert_eq!(message.exit_code, Some(0));
}

#[test]
#[ignore = "missing facility: BashExecutionComponent is not ported, so carriage-return progress collapsing cannot be observed"]
fn collapses_carriage_return_progress_overwrites() {
	let message = BashExecutionMessage {
		output: "100%\ndone".to_string(),
		..BashExecutionMessage::default()
	};
	assert!(message.output.contains("100%"));
}

#[test]
#[ignore = "missing facility: TranscriptContainer/BashExecutionComponent finalization is not ported"]
fn keeps_exit_output_mutable_until_queued_replay_drains() {
	let message = BashExecutionMessage {
		output: "OUT-MARKER\nERR-MARKER".to_string(),
		exit_code: Some(0),
		..BashExecutionMessage::default()
	};
	assert_eq!(message.exit_code, Some(0));
}

#[test]
#[ignore = "missing facility: TranscriptContainer/BashExecutionComponent finalization is not ported"]
fn keeps_exit_7_output_mutable_until_queued_replay_drains() {
	let message = BashExecutionMessage {
		output: "OUT-MARKER\nERR-MARKER".to_string(),
		exit_code: Some(7),
		..BashExecutionMessage::default()
	};
	assert_eq!(message.exit_code, Some(7));
}
