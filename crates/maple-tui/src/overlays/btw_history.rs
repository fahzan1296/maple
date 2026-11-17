//! Side-question (BTW) history data and inject-block helpers.
//!
//! Reference: `l/src/overlays/btw-history.ts`.
//!
//! Deviations:
//! - `ImageContent` from `@oh-my-pi/pi-ai` is abstracted as opaque host data
//!   (`BtwImage` = `String` payload/ref) because media types are runtime-specific.
//! - `sanitizeDisplayText` dependency is not imported from extension runtime;
//!   `format_btw_inject_text` performs local control-tab cleanup and trim.

/// Opaque image content attached to a side question.
pub type BtwImage = String;

/// One turn of a side-question topic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BtwHistoryTurn {
	pub question: String,
	/// Images attached to the question, sent again on later follow-ups.
	pub images: Vec<BtwImage>,
	pub answer: String,
	/// "running" | "complete" | "cancelled" | "error" | "interrupted"
	pub status: String,
	pub created_at: i64,
	pub updated_at: i64,
	pub error: Option<String>,
}

/// A side-question topic record with optional follow-ups.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BtwHistoryRecord {
	pub id: String,
	pub leaf_id: Option<String>,
	/// Display heading for topic (e.g. `/sitrep`); `question` keeps full sent text.
	pub title: Option<String>,
	pub follow_ups: Vec<BtwHistoryTurn>,
	// flattened base turn fields
	pub question: String,
	pub images: Vec<BtwImage>,
	pub answer: String,
	pub status: String,
	pub created_at: i64,
	pub updated_at: i64,
	pub error: Option<String>,
}

impl BtwHistoryRecord {
	/// Base turn view.
	#[must_use]
	pub fn base_turn(&self) -> BtwHistoryTurn {
		BtwHistoryTurn {
			question: self.question.clone(),
			images: self.images.clone(),
			answer: self.answer.clone(),
			status: self.status.clone(),
			created_at: self.created_at,
			updated_at: self.updated_at,
			error: self.error.clone(),
		}
	}
}

/// The topic's display heading: its title, else the opening question.
#[must_use]
pub fn get_btw_topic_label(record: &BtwHistoryRecord) -> &str {
	record.title.as_deref().unwrap_or(record.question.as_str())
}

/// Latest turn of a topic.
#[must_use]
pub fn get_btw_latest_turn(record: &BtwHistoryRecord) -> BtwHistoryTurn {
	record.follow_ups.last().cloned().unwrap_or_else(|| record.base_turn())
}

/// All turns of a topic in order.
#[must_use]
pub fn get_btw_turns(record: &BtwHistoryRecord) -> Vec<BtwHistoryTurn> {
	let mut turns = vec![record.base_turn()];
	turns.extend(record.follow_ups.iter().cloned());
	turns
}

/// Identifies one turn of a topic (latest by default).
#[must_use]
pub fn get_btw_turn_key(record: &BtwHistoryRecord, turn_index: Option<usize>) -> String {
	let idx = turn_index.unwrap_or_else(|| record.follow_ups.len());
	format!("{}:{idx}", record.id)
}

/// Copy the most recent nonblank answer, preserving original whitespace.
#[must_use]
pub fn get_btw_copy_text(record: &BtwHistoryRecord) -> Option<String> {
	for turn in record.follow_ups.iter().rev() {
		if !turn.answer.trim().is_empty() {
			return Some(turn.answer.clone());
		}
	}
	if !record.answer.trim().is_empty() {
		Some(record.answer.clone())
	} else {
		None
	}
}

const INJECT_OPEN: &str = "<inject>";
const INJECT_CLOSE: &str = "</inject>";

/// An answer around its `<inject>` block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtwInjectParts {
	pub before: String,
	pub inject: String,
	pub after: String,
	/// False while no opener has a close (streaming/unclosed block runs to end).
	pub closed: bool,
}

/// Split an answer at its last closed `<inject>` block (last opener with a close after it,
/// paired with first close after it). Without a closed block, last opener runs to end.
#[must_use]
pub fn split_btw_inject(answer: &str) -> Option<BtwInjectParts> {
	let last_close = answer.rfind(INJECT_CLOSE);
	if let Some(last_close_idx) = last_close {
		let open_search_end = last_close_idx;
		if let Some(open_rel) = answer[..open_search_end].rfind(INJECT_OPEN) {
			let open_end = open_rel + INJECT_OPEN.len();
			if let Some(close_rel) = answer[open_end..].find(INJECT_CLOSE) {
				let close_idx = open_end + close_rel;
				return Some(BtwInjectParts {
					before: answer[..open_rel].to_string(),
					inject: answer[open_end..close_idx].to_string(),
					after: answer[close_idx + INJECT_CLOSE.len()..].to_string(),
					closed: true,
				});
			}
		}
	}
	let streaming = answer.rfind(INJECT_OPEN)?;
	Some(BtwInjectParts {
		before: answer[..streaming].to_string(),
		inject: answer[streaming + INJECT_OPEN.len()..].to_string(),
		after: String::new(),
		closed: false,
	})
}

/// Block text as shown/sent: control chars stripped (except newline), tabs expanded, trimmed.
#[must_use]
pub fn format_btw_inject_text(inject: &str) -> String {
	let mut out = String::with_capacity(inject.len());
	for ch in inject.chars() {
		match ch {
			'\t' => out.push_str("    "),
			'\n' => out.push('\n'),
			c if c.is_control() => {}
			c => out.push(c),
		}
	}
	out.trim().to_string()
}

/// Steering prompt to send; undefined while unclosed or blank.
#[must_use]
pub fn get_btw_inject(answer: &str) -> Option<String> {
	let parts = split_btw_inject(answer)?;
	if !parts.closed {
		return None;
	}
	let text = format_btw_inject_text(&parts.inject);
	if text.is_empty() { None } else { Some(text) }
}

/// A topic's steering prompt: only its latest turn's, once completed.
#[must_use]
pub fn get_btw_record_inject(record: &BtwHistoryRecord) -> Option<String> {
	let latest = get_btw_latest_turn(record);
	if latest.status == "complete" {
		get_btw_inject(&latest.answer)
	} else {
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn base_record() -> BtwHistoryRecord {
		BtwHistoryRecord {
			id: "r1".into(),
			question: "q".into(),
			answer: "a1".into(),
			status: "complete".into(),
			created_at: 1,
			updated_at: 2,
			..Default::default()
		}
	}

	#[test]
	fn topic_label_turns_and_key() {
		let mut r = base_record();
		assert_eq!(get_btw_topic_label(&r), "q");
		r.title = Some("/sitrep".into());
		assert_eq!(get_btw_topic_label(&r), "/sitrep");
		r.follow_ups.push(BtwHistoryTurn {
			question: "f".into(),
			answer: "a2".into(),
			status: "complete".into(),
			..Default::default()
		});
		assert_eq!(get_btw_turns(&r).len(), 2);
		assert_eq!(get_btw_latest_turn(&r).answer, "a2");
		assert_eq!(get_btw_turn_key(&r, None), "r1:1");
		assert_eq!(get_btw_turn_key(&r, Some(0)), "r1:0");
	}

	#[test]
	fn copy_text_prefers_latest_nonblank() {
		let mut r = base_record();
		r.follow_ups.push(BtwHistoryTurn {
			answer: "   ".into(),
			..Default::default()
		});
		r.follow_ups.push(BtwHistoryTurn {
			answer: "  keep\n".into(),
			..Default::default()
		});
		assert_eq!(get_btw_copy_text(&r).as_deref(), Some("  keep\n"));
		let mut r2 = base_record();
		r2.answer = "  ".into();
		assert_eq!(get_btw_copy_text(&r2), None);
	}

	#[test]
	fn split_inject_closed_and_streaming() {
		let closed = "before<inject>do it</inject>after";
		let p = split_btw_inject(closed).unwrap();
		assert!(p.closed);
		assert_eq!(p.before, "before");
		assert_eq!(p.inject, "do it");
		assert_eq!(p.after, "after");
		let streaming = "before<inject>pending";
		let p = split_btw_inject(streaming).unwrap();
		assert!(!p.closed);
		assert_eq!(p.inject, "pending");
		assert_eq!(p.after, "");
		assert!(split_btw_inject("no tags").is_none());
		// last opener with a close after it
		let tricky = "<inject>a</inject>x<inject>b";
		let p = split_btw_inject(tricky).unwrap();
		assert!(!p.closed);
		assert_eq!(p.before, "<inject>a</inject>x");
		assert_eq!(p.inject, "b");
	}

	#[test]
	fn inject_text_and_record_inject() {
		assert_eq!(format_btw_inject_text(" \t hi\n"), "hi");
		assert_eq!(get_btw_inject("<inject> \n </inject>").as_deref(), None);
		assert_eq!(get_btw_inject("<inject>do</inject>").as_deref(), Some("do"));
		assert_eq!(get_btw_inject("<inject>do"), None);
		let mut r = base_record();
		r.answer = "x<inject>ship</inject>".into();
		r.status = "complete".into();
		assert_eq!(get_btw_record_inject(&r).as_deref(), Some("ship"));
		r.status = "running".into();
		assert_eq!(get_btw_record_inject(&r), None);
	}
}
