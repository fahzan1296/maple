//! Port of `l/test/btw-inject.test.ts` (pure `<inject>` helper describes only;
//! the `BtwPanelComponent` rendering describes need the panel component).

use maple_tui::overlays::btw_history::{
	get_btw_inject, get_btw_record_inject, split_btw_inject, BtwHistoryRecord, BtwHistoryTurn,
};

#[test]
fn takes_the_complete_block_even_when_prose_mentions_the_tag() {
	assert_eq!(
		get_btw_inject("I'll wrap it in <inject> tags:\n<inject>\n  Stop retrying the build.\n</inject>"),
		Some("Stop retrying the build.".to_string())
	);
	assert_eq!(
		get_btw_inject("<inject>Stop retrying.</inject>\nThe <inject> block above is ready."),
		Some("Stop retrying.".to_string())
	);
	// A later stray closing tag must not pull the prose between into the prompt.
	assert_eq!(
		get_btw_inject("<inject>Stop.</inject> The delimiter is </inject>."),
		Some("Stop.".to_string())
	);
}

#[test]
fn sends_nothing_until_the_block_closes_and_nothing_for_a_blank_block() {
	assert_eq!(
		split_btw_inject("Status ok.\n<inject>Stop ret"),
		Some(maple_tui::overlays::btw_history::BtwInjectParts {
			before: "Status ok.\n".to_string(),
			inject: "Stop ret".to_string(),
			after: String::new(),
			closed: false,
		})
	);
	assert_eq!(get_btw_inject("Status ok.\n<inject>Stop ret"), None);
	assert_eq!(get_btw_inject("<inject> \n </inject>"), None);
	assert_eq!(get_btw_inject("No steering needed."), None);
}

#[test]
fn sends_exactly_what_the_block_shows_without_control_sequences() {
	assert_eq!(
		get_btw_inject("<inject>Stop \x1b[31mnow\x1b[0m.</inject>"),
		Some("Stop now.".to_string())
	);
	// Only control content: nothing visible, so nothing to send.
	assert_eq!(get_btw_inject("<inject>\x1b[31m\x1b[0m</inject>"), None);
}

#[test]
fn offers_a_topics_block_only_from_its_completed_latest_turn() {
	let follow_up = BtwHistoryTurn {
		question: "Again?".to_string(),
		answer: "<inject>New prompt</inject>".to_string(),
		status: "running".to_string(),
		created_at: 2,
		updated_at: 2,
		..Default::default()
	};
	let record = BtwHistoryRecord {
		id: "r1".to_string(),
		leaf_id: None,
		question: "Status?".to_string(),
		answer: "<inject>Old prompt</inject>".to_string(),
		status: "complete".to_string(),
		created_at: 1,
		updated_at: 1,
		follow_ups: vec![follow_up.clone()],
		..Default::default()
	};
	assert_eq!(get_btw_record_inject(&record), None);

	let mut completed = follow_up;
	completed.status = "complete".to_string();
	let mut record = record;
	record.follow_ups = vec![completed];
	assert_eq!(get_btw_record_inject(&record), Some("New prompt".to_string()));
}
