//! Structured debug-log helpers: expanded rows and JSON field extraction.
//!
//! Reference: `l/src/apps/debug/log-formatting.ts`. The reference reads
//! `timestamp` through `Date.parse`; this port accepts RFC 3339 plus
//! offset-less ISO-8601, interpreted as UTC.

use maple_tui::render::render_utils::replace_tabs;
use maple_tui::{strip_ansi, wrap_text_with_ansi};

/// Strip ANSI/control sequences, then expand tabs. Call before theming.
fn sanitize_display_text(text: &str) -> String {
    let stripped = strip_ansi(text);
    let cleaned: String = stripped
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\t' | '\n' | '\r'))
        .collect();
    replace_tabs(&cleaned)
}

/// Sanitize and wrap an expanded log entry into display rows.
pub fn format_debug_log_expanded_lines(line: &str, max_width: usize) -> Vec<String> {
    let normalized = sanitize_display_text(line);
    if normalized.is_empty() {
        return vec![String::new()];
    }
    let width = max_width.max(1);
    normalized
        .split('\n')
        .flat_map(|segment| wrap_text_with_ansi(segment, width))
        .collect()
}

/// Read a valid timestamp (ms since epoch) from a structured log entry.
pub fn parse_debug_log_timestamp_ms(line: &str) -> Option<i64> {
    let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
    let timestamp = parsed.get("timestamp")?.as_str()?;
    parse_timestamp_ms(timestamp)
}

/// Read a finite process identifier from a structured log entry.
pub fn parse_debug_log_pid(line: &str) -> Option<f64> {
    let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
    let pid = parsed.get("pid")?.as_f64()?;
    pid.is_finite().then_some(pid)
}

/// Parse RFC 3339, then offset-less ISO-8601 as UTC.
fn parse_timestamp_ms(text: &str) -> Option<i64> {
    if let Ok(stamp) = time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339) {
        return Some(to_millis(stamp));
    }
    const NAIVE: [&str; 3] = [
        "[year]-[month]-[day]T[hour]:[minute]:[second][subsecond]",
        "[year]-[month]-[day]T[hour]:[minute]:[second]",
        "[year]-[month]-[day]",
    ];
    for pattern in NAIVE {
        let Ok(format) = time::format_description::parse(pattern) else {
            continue;
        };
        if let Ok(stamp) = time::PrimitiveDateTime::parse(text, &format) {
            return Some(to_millis(stamp.assume_utc()));
        }
        if let Ok(date) = time::Date::parse(text, &format) {
            return Some(to_millis(date.midnight().assume_utc()));
        }
    }
    None
}

fn to_millis(stamp: time::OffsetDateTime) -> i64 {
    stamp.unix_timestamp() * 1000 + i64::from(stamp.nanosecond() / 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_lines_strip_ansi_and_wrap() {
        let rows = format_debug_log_expanded_lines("\u{1b}[31mhello world\u{1b}[0m", 5);
        assert_eq!(rows, vec!["hello", "world"]);
    }

    #[test]
    fn expanded_lines_keep_blank_row() {
        assert_eq!(format_debug_log_expanded_lines("", 10), vec![""]);
    }

    #[test]
    fn expanded_lines_expand_tabs_and_split_rows() {
        let rows = format_debug_log_expanded_lines("a\tb\nc", 20);
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].contains('\t'));
        assert!(rows[0].starts_with("a "));
        assert_eq!(rows[1], "c");
    }

    #[test]
    fn expanded_lines_never_use_zero_width() {
        assert_eq!(
            format_debug_log_expanded_lines("abcdef", 0),
            vec!["a", "b", "c", "d", "e", "f"]
        );
    }

    #[test]
    fn timestamps_accept_rfc3339_and_naive() {
        assert_eq!(
            parse_debug_log_timestamp_ms(r#"{"timestamp":"1970-01-01T00:00:01Z"}"#),
            Some(1000)
        );
        assert_eq!(
            parse_debug_log_timestamp_ms(r#"{"timestamp":"1970-01-01T00:00:01"}"#),
            Some(1000)
        );
        assert_eq!(
            parse_debug_log_timestamp_ms(r#"{"timestamp":"1970-01-02"}"#),
            Some(86_400_000)
        );
        assert_eq!(parse_debug_log_timestamp_ms("not json"), None);
        assert_eq!(parse_debug_log_timestamp_ms(r#"{"timestamp":5}"#), None);
        assert_eq!(parse_debug_log_timestamp_ms(r#"{"pid":1}"#), None);
    }

    #[test]
    fn pids_require_finite_numbers() {
        assert_eq!(parse_debug_log_pid(r#"{"pid":42}"#), Some(42.0));
        assert_eq!(parse_debug_log_pid(r#"{"pid":1.5}"#), Some(1.5));
        assert_eq!(parse_debug_log_pid(r#"{"pid":"42"}"#), None);
        assert_eq!(parse_debug_log_pid(r#"{}"#), None);
        assert_eq!(parse_debug_log_pid("not json"), None);
    }
}
