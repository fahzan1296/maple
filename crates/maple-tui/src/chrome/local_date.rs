//! Local calendar formatting for epoch timestamps.
//!
//! Reference: `l/src/chrome/local-date.ts`.
//!
//! Deviations: the reference takes a `Date` (defaulting to `new Date()`) and
//! reads the host timezone through `Date#getFullYear`/`getMonth`/`getDate`/
//! `getHours`/`getMinutes`/`getTimezoneOffset`. Rust's standard library has no
//! local-time API, so [`LocalTime`] wraps epoch milliseconds —
//! [`LocalTime::now`] replaces the default argument — and the parts come from
//! `libc::localtime_r`, whose `tm_gmtoff` is the inverse of JavaScript's
//! `getTimezoneOffset`. Non-Unix platforms fall back to UTC because neither
//! `std` nor `libc` exposes the host timezone there.

/// A point in time as epoch milliseconds, standing in for a JavaScript `Date`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    at_ms: i64,
}

impl LocalTime {
    /// The current wall-clock time (`new Date()`).
    pub fn now() -> Self {
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0);
        LocalTime { at_ms }
    }

    /// A time from epoch milliseconds.
    pub fn from_unix_millis(at_ms: i64) -> Self {
        LocalTime { at_ms }
    }

    /// Epoch milliseconds.
    pub fn unix_millis(self) -> i64 {
        self.at_ms
    }
}

/// `formatLocalCalendarDate`: `YYYY-MM-DD` in the host local timezone.
pub fn format_local_calendar_date(date: &LocalTime) -> String {
    let (year, month, day, _, _, _) =
        local_parts(date.at_ms).expect("epoch milliseconds resolve to a local date");
    format!("{year}-{month:02}-{day:02}")
}

/// `formatLocalDateTimeWithOffset`: a local date and minute plus a compact
/// numeric UTC offset, e.g. `2026-11-16 09:05 +05:30`.
pub fn format_local_date_time_with_offset(date: &LocalTime) -> String {
    let (year, month, day, hour, minute, offset_minutes) =
        local_parts(date.at_ms).expect("epoch milliseconds resolve to a local date");
    let sign = if offset_minutes <= 0 { '+' } else { '-' };
    let absolute = offset_minutes.abs();
    let offset_hours = absolute / 60;
    let offset_remainder = absolute % 60;
    format!(
        "{year}-{month:02}-{day:02} {hour:02}:{minute:02} {sign}{offset_hours:02}:{offset_remainder:02}"
    )
}

/// Local calendar parts: `(year, month, day, hour, minute, offset_minutes)`.
/// The offset follows JavaScript's `getTimezoneOffset` sign (positive west).
#[cfg(unix)]
pub(crate) fn local_parts(at_ms: i64) -> Option<(i64, u32, u32, u32, u32, i64)> {
    let secs = at_ms.div_euclid(1000) as libc::time_t;
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::localtime_r(&secs, &mut parts) };
    if result.is_null() {
        return None;
    }
    Some((
        i64::from(parts.tm_year) + 1900,
        parts.tm_mon as u32 + 1,
        parts.tm_mday as u32,
        parts.tm_hour as u32,
        parts.tm_min as u32,
        -(parts.tm_gmtoff / 60),
    ))
}

/// UTC calendar parts (non-Unix fallback; see the module doc).
#[cfg(not(unix))]
pub(crate) fn local_parts(at_ms: i64) -> Option<(i64, u32, u32, u32, u32, i64)> {
    let secs = at_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let remainder = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Some((
        year,
        month,
        day,
        (remainder / 3_600) as u32,
        ((remainder % 3_600) / 60) as u32,
        0,
    ))
}

/// Days since the Unix epoch to a proleptic Gregorian date.
#[cfg(not(unix))]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed instant; the local parts depend on the host timezone, so every
    /// expectation is derived from [`local_parts`] rather than hard-coded.
    const AT_MS: i64 = 1_767_571_200_000;

    #[test]
    fn calendar_date_zero_pads_month_and_day() {
        let (year, month, day, _, _, _) = local_parts(AT_MS).expect("parts");
        let date = LocalTime::from_unix_millis(AT_MS);
        assert_eq!(
            format_local_calendar_date(&date),
            format!("{year}-{month:02}-{day:02}")
        );
    }

    #[test]
    fn date_time_includes_zero_padded_clock_and_signed_offset() {
        let (year, month, day, hour, minute, offset) = local_parts(AT_MS).expect("parts");
        let sign = if offset <= 0 { '+' } else { '-' };
        let absolute = offset.abs();
        let date = LocalTime::from_unix_millis(AT_MS);
        assert_eq!(
            format_local_date_time_with_offset(&date),
            format!(
                "{year}-{month:02}-{day:02} {hour:02}:{minute:02} {sign}{:02}:{:02}",
                absolute / 60,
                absolute % 60
            )
        );
    }

    #[test]
    fn offset_sign_matches_javascript_convention() {
        let date = LocalTime::from_unix_millis(AT_MS);
        let (_, _, _, _, _, offset) = local_parts(date.at_ms).expect("parts");
        // East of UTC (positive gmtoff) reports a negative getTimezoneOffset.
        #[cfg(unix)]
        {
            let secs = date.at_ms.div_euclid(1000) as libc::time_t;
            let mut parts: libc::tm = unsafe { std::mem::zeroed() };
            assert!(!unsafe { libc::localtime_r(&secs, &mut parts) }.is_null());
            assert_eq!(offset, -(parts.tm_gmtoff / 60));
        }
        let formatted = format_local_date_time_with_offset(&date);
        let sign = if offset <= 0 { '+' } else { '-' };
        assert!(formatted.ends_with(&format!(
            "{sign}{:02}:{:02}",
            offset.abs() / 60,
            offset.abs() % 60
        )));
    }

    #[test]
    fn unix_millis_round_trips() {
        let date = LocalTime::from_unix_millis(42);
        assert_eq!(date.unix_millis(), 42);
    }
}
