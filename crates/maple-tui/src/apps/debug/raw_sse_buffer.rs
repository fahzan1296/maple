//! Bounded capture of raw provider responses and SSE frames for debug views.
//!
//! Reference: `l/src/apps/debug/raw-sse-buffer.ts`.
//!
//! Deviations:
//! - `@oh-my-pi/pi-ai` types (`Model`, `ProviderResponseMetadata`,
//!   `RawSseEvent`) are not ported; minimal local structs/enums are declared
//!   with the fields this module reads.
//! - `materializeString` (Bun/JS rope detachment) has no Rust equivalent;
//!   trimming clones strings where needed (`String::from`/`clone`).
//! - JS `Date.now()`/`toISOString()` formatting uses a simple UTC formatter
//!   over epoch milliseconds (no chrono dep). Output is ISO-8601-like.
//! - Owner-attached buffer resolution uses a global fallback registry keyed by
//!   a caller-supplied `owner_id` (`usize`) instead of JS `Symbol` properties
//!   on arbitrary objects.
//! - `queueMicrotask` coalescing becomes immediate fan-out for multi-listener
//!   cases (Rust has no microtask queue here); bounded buffer semantics are
//!   preserved.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const MAX_RAW_SSE_EVENTS: usize = 1_000;
const MAX_RAW_SSE_CHARS: usize = 512_000;
const MAX_RAW_SSE_EVENT_CHARS: usize = 64_000;
// Reserve room for the `: omp-debug-truncated` / `: omp-debug-elided` marker
// lines so a trimmed event stays within MAX_RAW_SSE_EVENT_CHARS overall.
const TRIM_MARKER_RESERVE: usize = 200;
// Caps applied to individual tool entries when compacting a `tools` array
// inside an oversized `data:` payload.
const MAX_TOOL_SCHEMA_CHARS: usize = 200;
const MAX_TOOL_DESCRIPTION_CHARS: usize = 200;

/// Minimal model identity carried with captured records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Model {
	pub provider: Option<String>,
	pub id: Option<String>,
	pub api: Option<String>,
}

/// Minimal provider response metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderResponseMetadata {
	pub status: u16,
	pub request_id: Option<String>,
	pub last_transport: Option<String>,
}

/// One raw SSE frame as delivered by the transport.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawSseEvent {
	pub event: Option<String>,
	pub raw: Vec<String>,
}

/// A grow-only buffer of raw SSE wire lines for the debug viewer.
#[derive(Clone, Debug, Default)]
pub struct RawSseBuffer {
	lines: Vec<String>,
}

impl RawSseBuffer {
	/// An empty buffer.
	pub fn new() -> Self {
		RawSseBuffer { lines: Vec::new() }
	}

	/// Append one wire line.
	pub fn push_line(&mut self, line: &str) {
		self.lines.push(line.to_string());
	}

	/// The captured wire lines in order.
	pub fn lines(&self) -> &[String] {
		&self.lines
	}
}

/// A captured provider response or raw server-sent event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawSseDebugRecord {
	Response {
		sequence: u64,
		timestamp: u64,
		provider: Option<String>,
		model: Option<String>,
		api: Option<String>,
		status: u16,
		request_id: Option<String>,
		transport: Option<String>,
	},
	Event {
		sequence: u64,
		timestamp: u64,
		provider: Option<String>,
		model: Option<String>,
		api: Option<String>,
		event: Option<String>,
		raw: Vec<String>,
		truncated: bool,
		original_chars: usize,
	},
}

impl RawSseDebugRecord {
	pub fn sequence(&self) -> u64 {
		match self {
			RawSseDebugRecord::Response { sequence, .. }
			| RawSseDebugRecord::Event { sequence, .. } => *sequence,
		}
	}
	pub fn kind(&self) -> &'static str {
		match self {
			RawSseDebugRecord::Response { .. } => "response",
			RawSseDebugRecord::Event { .. } => "event",
		}
	}
}

/// Retained events and eviction counters at a point in time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawSseDebugSnapshot {
	pub records: Vec<RawSseDebugRecord>,
	pub dropped_records: usize,
	pub dropped_chars: usize,
	pub total_events: usize,
	pub last_updated_at: Option<u64>,
}

#[derive(Clone, Debug)]
struct TrimResult {
	raw: Vec<String>,
	truncated: bool,
	original_chars: usize,
	chars: usize,
}

fn count_chars(lines: &[String]) -> usize {
	// Historical formula: reduce(line.length + 1, init = 1).
	lines.iter().fold(1usize, |acc, line| acc + line.len() + 1)
}

fn elide_text(text: &str, max: usize) -> String {
	if text.len() <= max {
		return text.to_string();
	}
	if max <= 1 {
		return "…".to_string();
	}
	let keep = max - 1;
	// Cut on char boundary.
	let mut end = keep.min(text.len());
	while end > 0 && !text.is_char_boundary(end) {
		end -= 1;
	}
	format!("{}…", &text[..end])
}

/// Format a captured event timestamp as an ISO instant (UTC).
pub fn format_raw_sse_iso_time(timestamp: u64) -> String {
	// Civil-from-days algorithm (Howard Hinnant) over milliseconds.
	let ms = timestamp as i64;
	let days = ms.div_euclid(86_400_000);
	let sod = ms.rem_euclid(86_400_000);
	let z = days + 719_468;
	let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let mut y = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = if mp < 10 { mp + 3 } else { mp - 9 };
	if m <= 2 {
		y += 1;
	}
	let hour = sod / 3_600_000;
	let minute = (sod % 3_600_000) / 60_000;
	let second = (sod % 60_000) / 1_000;
	let milli = sod % 1_000;
	format!(
		"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
		y, m, d, hour, minute, second, milli
	)
}

/// Encode response metadata as an SSE comment.
pub fn format_raw_sse_response_comment(record: &RawSseDebugRecord) -> String {
	let RawSseDebugRecord::Response {
		timestamp,
		provider,
		model,
		api,
		status,
		request_id,
		transport,
		..
	} = record
	else {
		return String::new();
	};
	let mut fields: Vec<String> = vec![
		"omp-response".to_string(),
		format!("ts={}", format_raw_sse_iso_time(*timestamp)),
		format!("status={status}"),
	];
	if let Some(v) = provider {
		fields.push(format!("provider={v}"));
	}
	if let Some(v) = model {
		fields.push(format!("model={v}"));
	}
	if let Some(v) = api {
		fields.push(format!("api={v}"));
	}
	if let Some(v) = request_id {
		fields.push(format!("requestId={v}"));
	}
	if let Some(v) = transport {
		fields.push(format!("transport={v}"));
	}
	format!(": {}", fields.join(" "))
}

/// Return wire-format lines for a captured record.
pub fn raw_sse_record_lines(record: &RawSseDebugRecord) -> Vec<String> {
	match record {
		RawSseDebugRecord::Response { .. } => vec![format_raw_sse_response_comment(record)],
		RawSseDebugRecord::Event { raw, .. } => raw.clone(),
	}
}

fn raw_record_text(record: &RawSseDebugRecord) -> String {
	let mut s = raw_sse_record_lines(record).join("\n");
	s.push('\n');
	s
}

// Minimal JSON helpers (no serde_json dep required by this module's core path;
// we keep logic simple and deterministic). For tool compaction we do a
// best-effort structural scan only when clearly JSON-like.

fn compact_tool_object_text(obj_text: &str) -> Option<String> {
	// Best-effort: detect `"parameters"`/`"input_schema"`/`"description"` string
	// or object values and elide. If structure is ambiguous, return None.
	// This is intentionally conservative to avoid corrupting payloads.
	let mut out = obj_text.to_string();
	let mut changed = false;
	// Elide long "description":"..." values.
	if let Some((start, end, val)) = find_json_string_field(&out, "description") {
		if val.len() > MAX_TOOL_DESCRIPTION_CHARS {
			let rep = json_string(&elide_text(&val, MAX_TOOL_DESCRIPTION_CHARS));
			out.replace_range(start..end, &rep);
			changed = true;
		}
	}
	// Replace large parameters/input_schema values with elided JSON string.
	for key in ["parameters", "input_schema"] {
		if let Some((start, end, raw_val)) = find_json_value_field(&out, key) {
			if raw_val.len() > MAX_TOOL_SCHEMA_CHARS {
				let rep = json_string(&elide_text(&raw_val, MAX_TOOL_SCHEMA_CHARS));
				out.replace_range(start..end, &rep);
				changed = true;
			}
		}
	}
	if changed {
		Some(out)
	} else {
		None
	}
}

fn json_string(s: &str) -> String {
	let mut out = String::with_capacity(s.len() + 2);
	out.push('"');
	for c in s.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c if c < '\u{20}' => out.push_str(&format!("\\u{:04x}", c as u32)),
			c => out.push(c),
		}
	}
	out.push('"');
	out
}

fn find_json_string_field(text: &str, key: &str) -> Option<(usize, usize, String)> {
	let needle = format!("\"{key}\"");
	let pos = text.find(&needle)?;
	let after = &text[pos + needle.len()..];
	let colon = after.find(':')?;
	let rest = &after[colon + 1..];
	let qrel = rest.find('"')?;
	let q = pos + needle.len() + colon + 1 + qrel;
	let bytes = text.as_bytes();
	let mut i = q + 1;
	let mut val = String::new();
	while i < bytes.len() {
		match bytes[i] {
			b'\\' if i + 1 < bytes.len() => {
				val.push(bytes[i] as char);
				val.push(bytes[i + 1] as char);
				i += 2;
			}
			b'"' => {
				return Some((q, i + 1, unescape_json_string_like(&val)));
			}
			b => {
				val.push(b as char);
				i += 1;
			}
		}
	}
	None
}

fn unescape_json_string_like(s: &str) -> String {
	// Approximate for length/inspection; not used for reparse.
	s.replace("\\n", "\n")
		.replace("\\r", "\r")
		.replace("\\t", "\t")
		.replace("\\\"", "\"")
		.replace("\\\\", "\\")
}

fn find_json_value_field(text: &str, key: &str) -> Option<(usize, usize, String)> {
	let needle = format!("\"{key}\"");
	let pos = text.find(&needle)?;
	let after = &text[pos + needle.len()..];
	let colon = after.find(':')?;
	let rest = &after[colon + 1..];
	let vrel = rest.find(|c: char| !c.is_whitespace())?;
	let vstart = pos + needle.len() + colon + 1 + vrel;
	let vend = match text.as_bytes()[vstart] {
		b'"' => string_end(text, vstart)?,
		b'{' | b'[' => bracket_end(text, vstart)?,
		_ => simple_value_end(text, vstart),
	};
	Some((vstart, vend, text[vstart..vend].to_string()))
}

fn string_end(text: &str, start: usize) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut i = start + 1;
	while i < bytes.len() {
		match bytes[i] {
			b'\\' => i += 2,
			b'"' => return Some(i + 1),
			_ => i += 1,
		}
	}
	None
}

fn bracket_end(text: &str, start: usize) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut stack: Vec<u8> = vec![bytes[start]];
	let mut i = start + 1;
	while i < bytes.len() {
		match bytes[i] {
			b'"' => {
				i = string_end(text, i)?;
				continue;
			}
			b'{' | b'[' => stack.push(bytes[i]),
			b'}' => {
				if stack.pop()? != b'{' {
					return None;
				}
				if stack.is_empty() {
					return Some(i + 1);
				}
			}
			b']' => {
				if stack.pop()? != b'[' {
					return None;
				}
				if stack.is_empty() {
					return Some(i + 1);
				}
			}
			_ => {}
		}
		i += 1;
	}
	None
}

fn simple_value_end(text: &str, start: usize) -> usize {
	let bytes = text.as_bytes();
	let mut i = start;
	while i < bytes.len() && !matches!(bytes[i], b',' | b'}' | b']' | b'\n' | b'\r') {
		i += 1;
	}
	i
}

fn compact_tool_lines(raw: &[String]) -> Option<Vec<String>> {
	let mut changed = false;
	let mut out = Vec::with_capacity(raw.len());
	for line in raw {
		if !line.starts_with("data:") || line.len() <= MAX_TOOL_SCHEMA_CHARS {
			out.push(line.clone());
			continue;
		}
		// Best-effort compaction for oversized data lines containing tools.
		if !line.contains("\"tools\"") {
			out.push(line.clone());
			continue;
		}
		if let Some(compacted) = compact_tool_object_text(line) {
			out.push(compacted);
			changed = true;
		} else {
			out.push(line.clone());
		}
	}
	if changed {
		Some(out)
	} else {
		None
	}
}

fn head_tail_trim(lines: &[String], budget: usize, elided_total: usize) -> Vec<String> {
	let head_budget = budget / 2;
	let tail_budget = budget - head_budget;

	let mut i = 0usize;
	let mut head_remaining = head_budget;
	let mut out: Vec<String> = Vec::new();
	while i < lines.len() && lines[i].len() + 1 <= head_remaining {
		head_remaining -= lines[i].len() + 1;
		out.push(lines[i].clone());
		i += 1;
	}

	let mut j = lines.len().saturating_sub(1);
	let mut tail_remaining = tail_budget;
	let mut tail: Vec<String> = Vec::new();
	while i < lines.len() && j >= i && lines[j].len() + 1 <= tail_remaining {
		tail_remaining -= lines[j].len() + 1;
		tail.push(lines[j].clone());
		if j == 0 {
			break;
		}
		j -= 1;
	}
	tail.reverse();

	let mut elided = elided_total as isize - count_chars(&out) as isize - count_chars(&tail) as isize;
	if i < lines.len() && (j >= i) {
		let head_slice_len = head_remaining.saturating_sub(2).min(lines[i].len());
		let head_slice = &lines[i][..head_slice_len];
		let tail_start = if i == j {
			head_slice_len.max(lines[j].len().saturating_sub(tail_remaining.saturating_sub(2)))
		} else {
			lines[j].len().saturating_sub(tail_remaining.saturating_sub(2))
		}
		.min(lines[j].len());
		let tail_slice = &lines[j][tail_start..];
		elided -= (head_slice.len() + tail_slice.len()) as isize;
		if !head_slice.is_empty() {
			out.push(format!("{head_slice}…"));
		}
		out.push(format!(": omp-debug-elided chars={}", elided.max(0)));
		if !tail_slice.is_empty() {
			out.push(format!("…{tail_slice}"));
		}
	} else if elided > 0 {
		out.push(format!(": omp-debug-elided chars={elided}"));
	}
	out.extend(tail);
	out
}

fn trim_raw_lines(raw: &[String]) -> TrimResult {
	let original_chars = count_chars(raw);
	if original_chars <= MAX_RAW_SSE_EVENT_CHARS {
		return TrimResult {
			raw: raw.to_vec(),
			truncated: false,
			original_chars,
			chars: original_chars + 1,
		};
	}

	let budget = MAX_RAW_SSE_EVENT_CHARS.saturating_sub(TRIM_MARKER_RESERVE);
	let mut lines = compact_tool_lines(raw).unwrap_or_else(|| raw.to_vec());
	let compacted_chars = count_chars(&lines);
	if compacted_chars > budget {
		lines = head_tail_trim(&lines, budget, compacted_chars);
	}
	lines.push(format!(": omp-debug-truncated originalChars={original_chars}"));
	let chars = count_chars(&lines) + 1;
	TrimResult {
		raw: lines,
		truncated: true,
		original_chars,
		chars,
	}
}

/// Bounded, observable capture of raw provider responses and SSE frames.
#[derive(Clone, Debug, Default)]
pub struct RawSseDebugBuffer {
	records: Vec<RawSseDebugRecord>,
	record_chars: Vec<usize>,
	head: usize,
	total_chars: usize,
	dropped_records: usize,
	dropped_chars: usize,
	total_events: usize,
	last_updated_at: Option<u64>,
	next_sequence: u64,
	revision: u64,
}

impl RawSseDebugBuffer {
	pub fn new() -> Self {
		Self {
			next_sequence: 1,
			..Self::default()
		}
	}

	/// Bumped on every change to the captured window.
	pub fn revision(&self) -> u64 {
		self.revision
	}

	pub fn record_response(&mut self, response: &ProviderResponseMetadata, model: Option<&Model>) {
		let record = RawSseDebugRecord::Response {
			sequence: self.next_sequence,
			timestamp: now_ms(),
			provider: model.and_then(|m| m.provider.clone()),
			model: model.and_then(|m| m.id.clone()),
			api: model.and_then(|m| m.api.clone()),
			status: response.status,
			request_id: response.request_id.clone(),
			transport: response.last_transport.clone(),
		};
		self.next_sequence += 1;
		let chars = format_raw_sse_response_comment(&record).len() + 1;
		self.append(record, chars);
	}

	pub fn record_event(&mut self, event: &RawSseEvent, model: Option<&Model>) {
		let trimmed = trim_raw_lines(&event.raw);
		self.total_events += 1;
		let record = RawSseDebugRecord::Event {
			sequence: self.next_sequence,
			timestamp: now_ms(),
			provider: model.and_then(|m| m.provider.clone()),
			model: model.and_then(|m| m.id.clone()),
			api: model.and_then(|m| m.api.clone()),
			event: event.event.clone(),
			raw: trimmed.raw,
			truncated: trimmed.truncated,
			original_chars: trimmed.original_chars,
		};
		self.next_sequence += 1;
		self.append(record, trimmed.chars);
	}

	pub fn snapshot(&self) -> RawSseDebugSnapshot {
		RawSseDebugSnapshot {
			records: self.records[self.head..].to_vec(),
			dropped_records: self.dropped_records,
			dropped_chars: self.dropped_chars,
			total_events: self.total_events,
			last_updated_at: self.last_updated_at,
		}
	}

	pub fn to_raw_text(&self) -> String {
		let live = &self.records[self.head..];
		let body: String = live.iter().map(raw_record_text).collect::<Vec<_>>().join("\n");
		if self.dropped_records == 0 {
			return body;
		}
		let dropped = format!(
			": omp-debug-dropped records={} chars={}\n\n",
			self.dropped_records, self.dropped_chars
		);
		if body.is_empty() {
			dropped
		} else {
			format!("{dropped}{body}")
		}
	}

	pub fn clear(&mut self) {
		self.records.clear();
		self.record_chars.clear();
		self.head = 0;
		self.total_chars = 0;
		self.dropped_records = 0;
		self.dropped_chars = 0;
		self.total_events = 0;
		self.last_updated_at = None;
		self.revision += 1;
	}

	fn append(&mut self, record: RawSseDebugRecord, chars: usize) {
		self.last_updated_at = Some(match &record {
			RawSseDebugRecord::Response { timestamp, .. }
			| RawSseDebugRecord::Event { timestamp, .. } => *timestamp,
		});
		self.records.push(record);
		self.record_chars.push(chars);
		self.total_chars += chars;
		self.enforce_limits();
		self.revision += 1;
	}

	fn enforce_limits(&mut self) {
		while self.records.len().saturating_sub(self.head) > MAX_RAW_SSE_EVENTS
			|| self.total_chars > MAX_RAW_SSE_CHARS
		{
			if self.records.len() <= self.head {
				break;
			}
			let chars = self.record_chars.get(self.head).copied().unwrap_or(0);
			self.head += 1;
			self.total_chars = self.total_chars.saturating_sub(chars);
			self.dropped_records += 1;
			self.dropped_chars += chars;
		}
		let live_count = self.records.len().saturating_sub(self.head);
		if self.head >= MAX_RAW_SSE_EVENTS || (self.head > live_count && self.head > 0) {
			self.records = self.records[self.head..].to_vec();
			self.record_chars = self.record_chars[self.head..].to_vec();
			self.head = 0;
		}
	}
}

fn now_ms() -> u64 {
	// Deterministic-enough fallback for non-time-critical debug capture; tests
	// construct records directly. Real integration can supply timestamps.
	use std::time::{SystemTime, UNIX_EPOCH};
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_millis() as u64)
		.unwrap_or(0)
}

thread_local! {
	static OWNER_BUFFERS: RefCell<HashMap<usize, Rc<RefCell<RawSseDebugBuffer>>>> = RefCell::new(HashMap::new());
	static GLOBAL_FALLBACK: RefCell<Rc<RefCell<RawSseDebugBuffer>>> =
		RefCell::new(Rc::new(RefCell::new(RawSseDebugBuffer::new())));
}

/// Resolve an owner's capture buffer or the process-wide fallback.
///
/// Deviation: JS resolves from an arbitrary owner object via symbols; Rust uses
/// an explicit `owner_id` key (e.g. pointer/value identity supplied by caller).
pub fn resolve_raw_sse_debug_buffer(owner_id: Option<usize>) -> Rc<RefCell<RawSseDebugBuffer>> {
	match owner_id {
		None => GLOBAL_FALLBACK.with(|g| g.borrow().clone()),
		Some(id) => OWNER_BUFFERS.with(|map| {
			let mut map = map.borrow_mut();
			map.entry(id)
				.or_insert_with(|| Rc::new(RefCell::new(RawSseDebugBuffer::new())))
				.clone()
		}),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn model() -> Model {
		Model {
			provider: Some("openai".into()),
			id: Some("gpt-x".into()),
			api: Some("responses".into()),
		}
	}

	#[test]
	fn format_iso_time_epoch() {
		assert_eq!(format_raw_sse_iso_time(0), "1970-01-01T00:00:00.000Z");
		assert_eq!(
			format_raw_sse_iso_time(1_700_000_000_123),
			"2023-11-14T22:13:20.123Z"
		);
	}

	#[test]
	fn response_comment_includes_known_fields() {
		let rec = RawSseDebugRecord::Response {
			sequence: 1,
			timestamp: 0,
			provider: Some("openai".into()),
			model: Some("gpt-x".into()),
			api: Some("responses".into()),
			status: 200,
			request_id: Some("req_1".into()),
			transport: Some("sse".into()),
		};
		let line = format_raw_sse_response_comment(&rec);
		assert!(line.starts_with(": omp-response"));
		assert!(line.contains("ts=1970-01-01T00:00:00.000Z"));
		assert!(line.contains("status=200"));
		assert!(line.contains("provider=openai"));
		assert!(line.contains("model=gpt-x"));
		assert!(line.contains("api=responses"));
		assert!(line.contains("requestId=req_1"));
		assert!(line.contains("transport=sse"));
	}

	#[test]
	fn record_lines_for_event_returns_raw() {
		let rec = RawSseDebugRecord::Event {
			sequence: 1,
			timestamp: 0,
			provider: None,
			model: None,
			api: None,
			event: Some("message".into()),
			raw: vec!["event: message".into(), "data: {}".into()],
			truncated: false,
			original_chars: 0,
		};
		assert_eq!(
			raw_sse_record_lines(&rec),
			vec!["event: message".to_string(), "data: {}".to_string()]
		);
	}

	#[test]
	fn buffer_records_response_and_event_and_snapshot() {
		let mut buf = RawSseDebugBuffer::new();
		buf.record_response(
			&ProviderResponseMetadata {
				status: 200,
				request_id: Some("r".into()),
				last_transport: Some("sse".into()),
			},
			Some(&model()),
		);
		buf.record_event(
			&RawSseEvent {
				event: Some("delta".into()),
				raw: vec!["data: hi".into()],
			},
			Some(&model()),
		);
		let snap = buf.snapshot();
		assert_eq!(snap.records.len(), 2);
		assert_eq!(snap.total_events, 1);
		assert_eq!(snap.dropped_records, 0);
		assert_eq!(snap.records[0].kind(), "response");
		assert_eq!(snap.records[1].kind(), "event");
	}

	#[test]
	fn to_raw_text_prefixes_dropped_marker() {
		let mut buf = RawSseDebugBuffer::new();
		buf.dropped_records = 2;
		buf.dropped_chars = 99;
		let text = buf.to_raw_text();
		assert!(text.contains(": omp-debug-dropped records=2 chars=99"));
	}

	#[test]
	fn clear_resets_state() {
		let mut buf = RawSseDebugBuffer::new();
		buf.record_event(&RawSseEvent { event: None, raw: vec!["x".into()] }, None);
		assert_eq!(buf.snapshot().records.len(), 1);
		buf.clear();
		let snap = buf.snapshot();
		assert!(snap.records.is_empty());
		assert_eq!(snap.total_events, 0);
		assert_eq!(snap.dropped_records, 0);
	}

	#[test]
	fn trim_marks_oversized_event() {
		let big = "x".repeat(MAX_RAW_SSE_EVENT_CHARS + 500);
		let trimmed = trim_raw_lines(&[big]);
		assert!(trimmed.truncated);
		assert!(trimmed
			.raw
			.iter()
			.any(|l| l.starts_with(": omp-debug-truncated originalChars=")));
	}

	#[test]
	fn resolve_buffer_returns_same_for_owner() {
		let a = resolve_raw_sse_debug_buffer(Some(4242));
		let b = resolve_raw_sse_debug_buffer(Some(4242));
		assert!(Rc::ptr_eq(&a, &b));
		let c = resolve_raw_sse_debug_buffer(None);
		let d = resolve_raw_sse_debug_buffer(None);
		assert!(Rc::ptr_eq(&c, &d));
	}

	#[test]
	fn enforce_limits_drops_oldest_when_over_event_count() {
		let mut buf = RawSseDebugBuffer::new();
		for i in 0..(MAX_RAW_SSE_EVENTS + 5) {
			buf.record_event(
				&RawSseEvent {
					event: None,
					raw: vec![format!("data: {i}")],
				},
				None,
			);
		}
		let snap = buf.snapshot();
		assert_eq!(snap.records.len(), MAX_RAW_SSE_EVENTS);
		assert_eq!(snap.dropped_records, 5);
		assert_eq!(snap.records[0].sequence(), 6);
	}
}
