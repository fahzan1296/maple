//! Port of `l/test/vibe-render.test.ts` (pure helper subset).
//!
//! The reference renders live TV-wall/composer components and checks blinking
//! cursors, spinner repaints, and transcript batching through a PTY harness.
//! The crate ports the plain data model and text helpers, so those are asserted
//! here; the animated rendering itself is host concern.

use maple_tui::tools::vibe::{
	composer_rows_plain, describe_call, describe_vibe_result_plain, frame_text,
	state_color_token, state_icon_token, tv_screen_header_line, vibe_native_label, VibeCli,
	VibeDescribeResult, VibeOp, VibeRenderArgs, VibeScreenSnapshot, VibeSendMode, VibeSendOutcome,
	VibeSessionState, VibeToolDetails, VibeWaitOutcome, CURSOR_GLYPH,
};

fn args(session: &str) -> VibeRenderArgs {
	VibeRenderArgs {
		session: Some(session.to_string()),
		..Default::default()
	}
}

fn screen(id: &str) -> VibeScreenSnapshot {
	VibeScreenSnapshot {
		id: id.to_string(),
		cli: Some(VibeCli::Fast),
		state: Some(VibeSessionState::Running),
		turns: 1,
		last_activity_at: 1_800_000_000_000,
		..Default::default()
	}
}

#[test]
fn composer_types_the_message_into_cli_rows_with_a_blinking_cursor() {
	let rows = composer_rows_plain("Focus on the API first.\nThen tests.", true, false);
	assert_eq!(rows[0], "> Focus on the API first.");
	assert_eq!(rows[1], format!("  Then tests.{CURSOR_GLYPH}"));

	// Odd frame: cursor blinks off.
	let off = composer_rows_plain("Hi", false, false);
	assert_eq!(off, vec!["> Hi".to_string()]);
}

#[test]
fn composer_caps_rows_and_marks_the_overflow() {
	let message = "a\nb\nc\nd";
	let collapsed = composer_rows_plain(message, false, false);
	assert_eq!(collapsed.len(), 2);
	assert!(collapsed[1].ends_with('…'));
	let expanded = composer_rows_plain(message, true, true);
	assert_eq!(expanded.len(), 4);
	assert!(expanded.last().unwrap().ends_with(CURSOR_GLYPH));
}

#[test]
fn native_and_describe_labels_name_the_operation_and_target() {
	let mut render_args = VibeRenderArgs {
		cli: Some(VibeCli::Good),
		name: Some("worker".into()),
		..Default::default()
	};
	assert_eq!(vibe_native_label(VibeOp::Spawn, Some(&render_args)), "spawn good · worker");
	render_args.cli = None;
	render_args.name = None;
	assert_eq!(vibe_native_label(VibeOp::Spawn, Some(&render_args)), "spawn ?");

	let send = args("Anna");
	assert_eq!(vibe_native_label(VibeOp::Send, Some(&send)), "send → Anna");
	assert_eq!(
		vibe_native_label(VibeOp::Wait, Some(&VibeRenderArgs { sessions: vec!["Anna".into(), "Bob".into()], ..Default::default() })),
		"wait on Anna, Bob"
	);
	assert_eq!(
		vibe_native_label(VibeOp::Wait, Some(&VibeRenderArgs::default())),
		"wait on running sessions"
	);
	assert_eq!(vibe_native_label(VibeOp::Kill, Some(&args("Anna"))), "kill Anna");
	assert_eq!(vibe_native_label(VibeOp::List, None), "sessions");

	assert_eq!(describe_call(VibeOp::Send, Some(&send)), "send → Anna");
	assert_eq!(
		describe_call(VibeOp::Wait, Some(&VibeRenderArgs::default())),
		"wait on running sessions"
	);
	let long = describe_call(
		VibeOp::Kill,
		Some(&args(&"x".repeat(60))),
	);
	assert!(long.ends_with('…'));
}

#[test]
fn tv_header_summarizes_flavor_state_turns_and_model() {
	let mut snap = screen("Anna");
	snap.model = Some("prov/fast-model".into());
	assert_eq!(
		tv_screen_header_line(&snap, None),
		"[fast] Anna running 1t prov/fast-model"
	);
	snap.queued = 2;
	assert_eq!(
		tv_screen_header_line(&snap, None),
		"[fast] Anna running 1t+2q prov/fast-model"
	);
	// A settled wait overrides the live state.
	assert_eq!(
		tv_screen_header_line(&snap, Some("completed")),
		"[fast] Anna completed 1t+2q prov/fast-model"
	);
}

#[test]
fn describes_results_per_operation() {
	let send_details = VibeToolDetails {
		op: Some(VibeOp::Send),
		send: Some(VibeSendOutcome {
			id: "Anna".into(),
			mode: Some(VibeSendMode::Steered),
			job_id: None,
		}),
		..Default::default()
	};
	let VibeDescribeResult { label, inline, is_error, body_lines } =
		describe_vibe_result_plain(VibeOp::Send, Some(&args("Anna")), Some(&send_details), false, "");
	assert_eq!(label, "send → Anna");
	assert!(inline);
	assert!(!is_error);
	assert_eq!(body_lines, vec!["steered into the running turn".to_string()]);

	let queued = VibeToolDetails {
		send: Some(VibeSendOutcome {
			mode: Some(VibeSendMode::Queued),
			..Default::default()
		}),
		..Default::default()
	};
	assert_eq!(
		describe_vibe_result_plain(VibeOp::Send, None, Some(&queued), false, "").body_lines,
		vec!["mid-turn — queued as the next turn".to_string()]
	);

	let wait = VibeToolDetails {
		op: Some(VibeOp::Wait),
		screens: vec![screen("Anna"), screen("Bob")],
		wait: Some(VibeWaitOutcome {
			still_running: vec!["Anna".into()],
			..Default::default()
		}),
		..Default::default()
	};
	let described = describe_vibe_result_plain(VibeOp::Wait, None, Some(&wait), false, "");
	assert_eq!(described.body_lines.len(), 2);
	assert!(described.body_lines[0].starts_with("[fast] Anna running 1t"));

	let empty = describe_vibe_result_plain(VibeOp::Wait, None, Some(&VibeToolDetails::default()), false, "");
	assert_eq!(empty.body_lines, vec!["no sessions".to_string()]);

	let errored = describe_vibe_result_plain(VibeOp::List, None, None, true, "  ");
	assert_eq!(errored.label, "sessions");
	assert!(errored.is_error);
	assert_eq!(errored.body_lines, vec!["vibe failed".to_string()]);
	assert_eq!(
		describe_vibe_result_plain(VibeOp::List, None, None, true, "boom").body_lines,
		vec!["boom".to_string()]
	);
}

#[test]
fn maps_states_to_status_tokens() {
	assert_eq!(state_icon_token(VibeSessionState::Starting), "pending");
	assert_eq!(state_icon_token(VibeSessionState::Running), "running");
	assert_eq!(state_icon_token(VibeSessionState::Idle), "success");
	assert_eq!(state_icon_token(VibeSessionState::Dead), "aborted");
	assert_eq!(state_color_token(VibeSessionState::Running), "accent");
	assert_eq!(state_color_token(VibeSessionState::Starting), "accent");
	assert_eq!(state_color_token(VibeSessionState::Idle), "success");
	assert_eq!(state_color_token(VibeSessionState::Dead), "muted");
}

#[test]
fn frames_and_truncates_text_to_a_width() {
	assert_eq!(frame_text("a\n\t b", 20), "a b");
	assert_eq!(frame_text("hello world", 6), "hello…");
	assert_eq!(frame_text("x".repeat(200).as_str(), 110).chars().count(), 110);
	assert!(frame_text("x".repeat(200).as_str(), 110).ends_with('…'));
}
