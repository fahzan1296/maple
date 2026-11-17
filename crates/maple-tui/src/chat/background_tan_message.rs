//! Single-line transcript pill for a `/tan` background-dispatch breadcrumb.
//!
//! Reference: `l/src/chat/background-tan-message.ts`.
//!
//! The full system-notice content (the persisted `content`) is for the model
//! only; the user sees one compact line, so the raw `<system-notice>` block
//! never reaches the transcript.
//!
//! Deviations:
//! - Details come from the custom message's JSON payload, because the crate's
//!   [`BackgroundTanDispatchDetails`] has no serde derive; pass the struct to
//!   [`create_background_tan_dispatch_block_for`] when it is already parsed.

use crate::chat::messages::{
	BackgroundTanDispatchDetails, CustomMessage, BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE,
};
use crate::chrome::transcript_notice::NoticeTone;
use crate::chrome::transcript_status::{
	TranscriptStatusBlock, TranscriptStatusNative, TranscriptStatusPart, TranscriptStatusRow,
};
use crate::native::describe::span;
use crate::render::render_utils::replace_tabs;
use crate::theme::Theme;

/// Longest work preview the pill shows before eliding.
const TAN_WORK_PREVIEW_LENGTH: usize = 56;

/// One-line, whitespace-collapsed preview of the dispatched work.
pub fn preview_work(work: &str) -> String {
	let single_line = collapse_whitespace(&replace_tabs(work));
	if single_line.len() <= TAN_WORK_PREVIEW_LENGTH {
		return single_line;
	}
	format!(
		"{}…",
		&single_line[..TAN_WORK_PREVIEW_LENGTH - 1]
	)
}

fn collapse_whitespace(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut in_space = false;
	for ch in text.trim().chars() {
		if ch.is_whitespace() {
			if !in_space {
				out.push(' ');
			}
			in_space = true;
		} else {
			out.push(ch);
			in_space = false;
		}
	}
	out
}

/// The pill for a `/tan` breadcrumb, reading `details` from the message JSON.
pub fn create_background_tan_dispatch_block(message: &CustomMessage, theme: &Theme) -> TranscriptStatusBlock {
	let job_id = message
		.details
		.as_ref()
		.and_then(|details| details.get("jobId"))
		.and_then(|job_id| job_id.as_str())
		.unwrap_or("unknown")
		.to_string();
	let work = message
		.details
		.as_ref()
		.and_then(|details| details.get("work"))
		.and_then(|work| work.as_str())
		.map(preview_work);
	create_block(&job_id, work.as_deref(), theme)
}

/// The pill for already-parsed details.
pub fn create_background_tan_dispatch_block_for(
	details: Option<&BackgroundTanDispatchDetails>,
	theme: &Theme,
) -> TranscriptStatusBlock {
	let job_id = details
		.map(|details| details.job_id.as_str())
		.filter(|job_id| !job_id.is_empty())
		.unwrap_or("unknown")
		.to_string();
	let work = details
		.map(|details| details.work.as_str())
		.filter(|work| !work.is_empty())
		.map(preview_work);
	create_block(&job_id, work.as_deref(), theme)
}

/// Whether `message` is the background-dispatch breadcrumb this module draws.
pub fn is_background_tan_dispatch(message: &CustomMessage) -> bool {
	message.custom_type == BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE
}

fn create_block(job_id: &str, work: Option<&str>, theme: &Theme) -> TranscriptStatusBlock {
	let icon = theme
		.symbol("output")
		.to_string();
	let dash = theme.symbol("separator").to_string();
	let mut parts = vec![
		Some(TranscriptStatusPart {
			text: if icon.is_empty() {
				"Tangent dispatched".to_string()
			} else {
				format!("{icon} Tangent dispatched")
			},
			color: Some("muted".to_string()),
		}),
		Some(TranscriptStatusPart {
			text: "[task]".to_string(),
			color: Some("dim".to_string()),
		}),
		Some(TranscriptStatusPart {
			text: job_id.to_string(),
			color: Some("accent".to_string()),
		}),
	];
	if let Some(work) = work {
		parts.push(Some(TranscriptStatusPart {
			text: format!("{dash} {work}"),
			color: Some("dim".to_string()),
		}));
	}
	let mut spans = vec![
		span("Tangent dispatched", None),
		span(" · ", Some("dim")),
		span(format!("task {job_id}"), Some("dim")),
	];
	if let Some(work) = work {
		spans.push(span(" · ", Some("dim")));
		spans.push(span(work.to_string(), Some("dim")));
	}
	let row = TranscriptStatusRow {
		parts,
		indent: None,
		native: TranscriptStatusNative {
			tone: NoticeTone::Info,
			icon: Some("output".to_string()),
			message: spans,
		},
	};
	TranscriptStatusBlock::new(theme, &[row])
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};
	use crate::theme::theme_class::ColorMode;
	use serde_json::json;

	fn theme() -> Theme {
		create_theme(
			builtin_theme_json("dark").unwrap(),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.unwrap()
	}

	fn message(details: serde_json::Value) -> CustomMessage {
		CustomMessage {
			custom_type: BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE.to_string(),
			content: crate::chat::messages::CustomMessageContent::Text(
				"<system-notice>raw</system-notice>".to_string(),
			),
			details: Some(details),
			..CustomMessage::default()
		}
	}

	#[test]
	fn preview_collapses_whitespace_and_elides_past_the_limit() {
		assert_eq!(preview_work("fix\t\tthe   bug"), "fix the bug");
		let long = "word ".repeat(40);
		let preview = preview_work(&long);
		assert_eq!(preview.chars().count(), TAN_WORK_PREVIEW_LENGTH);
		assert!(preview.ends_with('…'));
	}

	#[test]
	fn pills_name_the_job_and_preview_the_work() {
		let mut block = create_background_tan_dispatch_block(
			&message(json!({ "jobId": "job-7", "work": "port the overlay" })),
			&theme(),
		);
		let rows = block.render(80);
		let plain = crate::ansi_scan::strip_ansi(&rows.concat());
		assert!(plain.contains("Tangent dispatched"), "{plain}");
		assert!(plain.contains("job-7"), "{plain}");
		assert!(plain.contains("port the overlay"), "{plain}");
		assert!(!plain.contains("system-notice"), "raw notice text stays out of the pill");
	}

	#[test]
	fn missing_details_fall_back_to_an_unknown_job() {
		let mut block = create_background_tan_dispatch_block(&message(json!({})), &theme());
		assert!(crate::ansi_scan::strip_ansi(&block.render(80).concat()).contains("unknown"));
	}

	#[test]
	fn detail_type_recognized() {
		assert!(is_background_tan_dispatch(
			&message(json!({ "jobId": "j" }))
		));
		let block = create_background_tan_dispatch_block_for(
			Some(&BackgroundTanDispatchDetails {
				job_id: "job-9".to_string(),
				work: "build".to_string(),
				session_file: "job-9.jsonl".to_string(),
			}),
			&theme(),
		);
		let mut block = block;
		assert!(crate::ansi_scan::strip_ansi(&block.render(80).concat()).contains("job-9"));
	}
}
