//! Debug log formatting helpers.
//! Reference: `l/src/apps/debug/log-formatting.ts`.

use crate::overlays::extensions::display_text::sanitize_display_text;
use crate::text_shape::wrap_text_with_ansi;

/// Sanitize and wrap an expanded log entry into display rows.
pub fn format_debug_log_expanded_lines(line: &str, max_width: usize) -> Vec<String> {
	let normalized = sanitize_display_text(line);
	let width = max_width.max(1);
	if normalized.is_empty() {
		return vec![String::new()];
	}
	normalized
		.split('\n')
		.flat_map(|segment| wrap_text_with_ansi(segment, width))
		.collect()
}

/// Read a valid timestamp from a structured log entry.
pub fn parse_debug_log_timestamp_ms(line: &str) -> Option<i64> {
	let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
	let ts = parsed.get("timestamp")?.as_str()?;
	parse_iso_millis(ts)
}

fn parse_iso_millis(s: &str) -> Option<i64> {
	let b = s.as_bytes();
	if b.len() < 19 {
		return None;
	}
	let year: i32 = s.get(0..4)?.parse().ok()?;
	let month: i32 = s.get(5..7)?.parse().ok()?;
	let day: i32 = s.get(8..10)?.parse().ok()?;
	let sep = *b.get(10)?;
	if sep != b'T' && sep != b't' && sep != b' ' {
		return None;
	}
	let hour: i32 = s.get(11..13)?.parse().ok()?;
	let min: i32 = s.get(14..16)?.parse().ok()?;
	let sec: i32 = s.get(17..19)?.parse().ok()?;
	let mut i = 19usize;
	let mut frac_ms: i64 = 0;
	if b.get(i) == Some(&b'.') {
		i += 1;
		let start = i;
		while b.get(i).map(|c| c.is_ascii_digit()).unwrap_or(false) {
			i += 1;
		}
		let frac = &s[start..i];
		let mut ms_str = frac.chars().take(3).collect::<String>();
		while ms_str.len() < 3 {
			ms_str.push('0');
		}
		frac_ms = ms_str.parse().unwrap_or(0);
	}
	let mut offset_min: i64 = 0;
	if let Some(&c) = b.get(i) {
		if c == b'Z' || c == b'z' {
			// utc
		} else if c == b'+' || c == b'-' {
			let sign: i64 = if c == b'+' { 1 } else { -1 };
			i += 1;
			let oh: i64 = s.get(i..i + 2)?.parse().ok()?;
			i += 2;
			if b.get(i) == Some(&b':') {
				i += 1;
			}
			let om: i64 = s.get(i..i + 2).and_then(|x| x.parse().ok()).unwrap_or(0);
			offset_min = sign * (oh * 60 + om);
		}
	}
	let days = days_from_civil(year, month, day)?;
	let mut secs = days * 86_400 + hour as i64 * 3_600 + min as i64 * 60 + sec as i64;
	secs -= offset_min * 60;
	Some(secs * 1000 + frac_ms)
}

fn days_from_civil(y: i32, m: i32, d: i32) -> Option<i64> {
	if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
		return None;
	}
	let y = y - if m <= 2 { 1 } else { 0 };
	let era = if y >= 0 { y } else { y - 399 } / 400;
	let yoe = y - era * 400;
	let mp = m + if m > 2 { -3 } else { 9 };
	let doy = (153 * mp + 2) / 5 + d - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	Some(era as i64 * 146_097 + doe as i64 - 719_468)
}

/// Read a finite process identifier from a structured log entry.
pub fn parse_debug_log_pid(line: &str) -> Option<i64> {
	let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
	let pid = parsed.get("pid")?;
	if let Some(n) = pid.as_i64() {
		return Some(n);
	}
	if let Some(n) = pid.as_u64() {
		return i64::try_from(n).ok();
	}
	if let Some(f) = pid.as_f64() {
		if f.is_finite() {
			return Some(f as i64);
		}
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_expanded_lines() {
		let lines = format_debug_log_expanded_lines("hello\nworld", 20);
		assert_eq!(lines, vec!["hello", "world"]);
		let empty = format_debug_log_expanded_lines("", 10);
		assert_eq!(empty, vec![""]);
	}

	#[test]
	fn parses_timestamp_and_pid() {
		let line = r#"{"timestamp":"2024-01-02T03:04:05.006Z","pid":123}"#;
		assert_eq!(parse_debug_log_pid(line), Some(123));
		let ts = parse_debug_log_timestamp_ms(line).unwrap();
		assert!(ts > 0);
		assert_eq!(parse_debug_log_pid("not json"), None);
		assert_eq!(parse_debug_log_timestamp_ms("{}"), None);
	}
}
