//! Port of `l/test/background-tan-message.test.ts`.

use maple_tui::chat::background_tan_message::{
	create_background_tan_dispatch_block, is_background_tan_dispatch,
};
use maple_tui::chat::messages::{
	CustomMessage, CustomMessageContent, MessageAttribution, BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE,
};
use maple_tui::component::Component;
use maple_tui::theme::default_theme;

fn dispatch_message(details: serde_json::Value) -> CustomMessage {
	CustomMessage {
		custom_type: BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE.to_string(),
		// The persisted content is the full system-notice the model reads; the
		// renderer must NOT surface it in the transcript.
		content: CustomMessageContent::Text(
			"<system-notice reason=\"background_task_dispatched\">raw block</system-notice>".to_string(),
		),
		display: true,
		details: Some(details),
		attribution: MessageAttribution::User,
		timestamp: 1_700_000_000_000,
	}
}

#[test]
fn renders_one_compact_line_with_job_and_work_not_the_raw_notice() {
	let message = dispatch_message(
		serde_json::json!({ "jobId": "job-42", "work": "investigate the cache reuse path" }),
	);
	assert!(is_background_tan_dispatch(&message));
	let mut block = create_background_tan_dispatch_block(&message, &default_theme());
	let rows = block.render(120);
	let lines: Vec<&String> = rows.iter().filter(|line| !line.trim().is_empty()).collect();
	assert_eq!(lines.len(), 1);
	let plain = maple_tui::strip_ansi(lines[0]);
	assert!(plain.contains("job-42"), "{plain}");
	assert!(plain.contains("investigate the cache reuse path"), "{plain}");
	assert!(!plain.contains("system-notice"), "raw notice text stays out of the pill");
}

#[test]
fn truncates_an_overlong_work_preview_so_the_line_stays_a_single_pill() {
	let message = dispatch_message(serde_json::json!({ "jobId": "job-7", "work": "x".repeat(200) }));
	let mut block = create_background_tan_dispatch_block(&message, &default_theme());
	let line = block
		.render(120)
		.into_iter()
		.find(|rendered| rendered.contains("job-7"))
		.unwrap_or_default();
	assert!(line.contains('…'), "{line}");
	assert!(!line.contains(&"x".repeat(80)), "{line}");
}
