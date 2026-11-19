//! Prompt-cache text beyond the `cache` marker: the hover card's two lines and
//! the rows of `/session`'s Prompt Cache section.
//!
//! Reference: `l/src/status-line/prompt-cache.ts`.

use crate::chrome::local_date::local_parts;

use super::host::{
    PromptCacheColdReason, PromptCacheState, PromptCacheStatus, PromptCacheWarming,
    PromptCacheWarmingState,
};
use super::segments::{format_duration, format_number, sanitize_status_text};

/// One `label: value` line of the prompt-cache details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptCacheDetailRow {
    /// Row label.
    pub label: String,
    /// Row value.
    pub value: String,
    /// `Some("warning")` for a row that explains a cold cache.
    pub tone: Option<&'static str>,
}

fn parts(at: i64) -> Option<(i64, u32, u32, u32, u32)> {
    local_parts(at).map(|(year, month, day, hour, minute, _)| (year, month, day, hour, minute))
}

fn same_day(at: i64, now_ms: i64) -> bool {
    match (parts(at), parts(now_ms)) {
        (Some((ay, am, ad, _, _)), Some((ny, nm, nd, _, _))) => {
            (ay, am, ad) == (ny, nm, nd)
        }
        _ => false,
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn clock(hour: u32, minute: u32) -> String {
    let suffix = if hour >= 12 { "PM" } else { "AM" };
    let hour = match hour % 12 {
        0 => 12,
        hour => hour,
    };
    format!("{hour}:{minute:02} {suffix}")
}

/// `4:53 PM` on `now_ms`'s local day; `Oct 17, 5:12 PM` on any other.
fn when(at: i64, now_ms: i64) -> String {
    let Some((year, month, day, hour, minute)) = parts(at) else {
        return String::new();
    };
    if same_day(at, now_ms) {
        return clock(hour, minute);
    }
    let month = MONTHS
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("Jan");
    let _ = year;
    format!("{month} {day}, {}", clock(hour, minute))
}

/// ` (in 41 min)` / ` (3 h ago)` / ` (12 days ago)`.
fn relative(at: i64, now_ms: i64) -> String {
    let minutes = ((at - now_ms).abs() as f64 / 60_000.0).round() as i64;
    if minutes == 0 {
        return if at >= now_ms {
            " (in under a minute)".to_string()
        } else {
            " (just now)".to_string()
        };
    }
    let span = if minutes < 60 {
        format!("{minutes} min")
    } else if minutes < 48 * 60 {
        format!("{} h", (minutes as f64 / 60.0).round() as i64)
    } else {
        format!("{} days", (minutes as f64 / (24.0 * 60.0)).round() as i64)
    };
    if at >= now_ms {
        format!(" (in {span})")
    } else {
        format!(" ({span} ago)")
    }
}

/// Why the next message re-sends the prefix, as one phrase.
fn cold_reason_text(reason: &PromptCacheColdReason, now_ms: Option<i64>) -> String {
    match reason {
        PromptCacheColdReason::Expired { at } => match now_ms {
            None => "expired".to_string(),
            Some(now_ms) => {
                let at_word = if same_day(*at, now_ms) { "at " } else { "" };
                format!(
                    "expired {at_word}{}{}",
                    when(*at, now_ms),
                    relative(*at, now_ms)
                )
            }
        },
        PromptCacheColdReason::RefreshMissed => "omp's refresh found the entry gone".to_string(),
        PromptCacheColdReason::Model { from, to } => format!(
            "model changed: {} → {}",
            sanitize_status_text(from),
            sanitize_status_text(to)
        ),
        PromptCacheColdReason::Thinking { from, to } => format!(
            "thinking level changed: {} → {}",
            sanitize_status_text(from),
            sanitize_status_text(to)
        ),
        PromptCacheColdReason::Tools => "tool set changed".to_string(),
        PromptCacheColdReason::SystemPrompt => "system prompt changed".to_string(),
        PromptCacheColdReason::Retention => "cache retention setting changed".to_string(),
        PromptCacheColdReason::Compaction => "context was compacted".to_string(),
    }
}

fn warming_text(warming: &PromptCacheWarming, now_ms: i64) -> String {
    match warming.state {
        PromptCacheWarmingState::Scheduled => {
            let next = match warming.next_warm_at {
                Some(at) => format!("next refresh {}", when(at, now_ms)),
                None => "refresh scheduled".to_string(),
            };
            match warming.stops_at {
                Some(at) => format!("{next}, gives up by {}", when(at, now_ms)),
                None => next,
            }
        }
        PromptCacheWarmingState::Refreshing => "refreshing now".to_string(),
        PromptCacheWarmingState::Inactive => match &warming.reason {
            Some(reason) => {
                let reason = sanitize_status_text(reason);
                if reason.is_empty() {
                    "off".to_string()
                } else {
                    format!("off: {reason}")
                }
            }
            None => "off".to_string(),
        },
    }
}

/// What the next message re-sends uncached.
fn resend_text(status: &PromptCacheStatus) -> String {
    let tokens = format!("~{}", format_number(status.next_tokens as f64));
    match &status.reason {
        Some(
            PromptCacheColdReason::Expired { .. }
            | PromptCacheColdReason::RefreshMissed
            | PromptCacheColdReason::Model { .. }
            | PromptCacheColdReason::Tools,
        ) => format!("re-sends {tokens}"),
        _ => format!("re-sends up to {tokens}"),
    }
}

/// The hover card's two lines: whether the next message reads the cache, then
/// what it sends.
pub fn prompt_cache_brief(status: &PromptCacheStatus, now_ms: i64) -> [String; 2] {
    if status.state == PromptCacheState::Cold {
        let why = status
            .reason
            .as_ref()
            .map(|reason| format!(" — {}", cold_reason_text(reason, None)))
            .unwrap_or_default();
        return [
            format!("Not cached{why}"),
            format!("Next message {}", resend_text(status)),
        ];
    }
    let cached = if status.inferred {
        "Likely cached"
    } else {
        "Cached"
    };
    let until = match status.ttl_ms {
        Some(ttl) => format!(" until {}", when(status.touched_at + ttl, now_ms)),
        None => ", no expiry".to_string(),
    };
    let reads = format_number(status.cached_tokens.min(status.next_tokens) as f64);
    [
        format!("{cached}{until}"),
        format!("Next message reads ~{reads} from cache"),
    ]
}

/// The `/session` rows, in display order.
pub fn prompt_cache_details(status: &PromptCacheStatus, now_ms: i64) -> Vec<PromptCacheDetailRow> {
    let tier = PromptCacheDetailRow {
        label: "Tier".to_string(),
        value: match status.ttl_ms {
            Some(ttl) => format_duration(ttl.max(0) as u64),
            None => "no expiry".to_string(),
        },
        tone: None,
    };
    let warming: Vec<PromptCacheDetailRow> = status
        .warming
        .as_ref()
        .map(|warming| PromptCacheDetailRow {
            label: "Warming".to_string(),
            value: warming_text(warming, now_ms),
            tone: None,
        })
        .into_iter()
        .collect();

    if status.state == PromptCacheState::Cold {
        let mut rows = vec![PromptCacheDetailRow {
            label: "State".to_string(),
            value: "Not cached".to_string(),
            tone: Some("warning"),
        }];
        if let Some(reason) = &status.reason {
            rows.push(PromptCacheDetailRow {
                label: "Why not cached".to_string(),
                value: cold_reason_text(reason, Some(now_ms)),
                tone: Some("warning"),
            });
        }
        rows.push(tier);
        rows.extend(warming);
        rows.push(PromptCacheDetailRow {
            label: "Next message".to_string(),
            value: format!("{} uncached", resend_text(status)),
            tone: None,
        });
        return rows;
    }

    let expires_at = status.ttl_ms.map(|ttl| status.touched_at + ttl);
    let reads = status.cached_tokens.min(status.next_tokens);
    let mut rows = vec![
        PromptCacheDetailRow {
            label: "State".to_string(),
            value: if status.inferred {
                "Likely cached".to_string()
            } else {
                "Cached".to_string()
            },
            tone: None,
        },
        PromptCacheDetailRow {
            label: "Expires".to_string(),
            value: match expires_at {
                None => "never (the provider keeps it)".to_string(),
                Some(at) => format!("{}{}", when(at, now_ms), relative(at, now_ms)),
            },
            tone: None,
        },
        tier,
    ];
    rows.extend(warming);
    rows.push(PromptCacheDetailRow {
        label: "Next message".to_string(),
        value: format!(
            "reads ~{} from cache, sends ~{} new",
            format_number(reads as f64),
            format_number(status.next_tokens.saturating_sub(reads) as f64)
        ),
        tone: None,
    });
    rows
}


#[cfg(test)]
mod tests {
    use super::super::host::PromptCacheTier;
    use super::*;

    fn base_status(state: PromptCacheState) -> PromptCacheStatus {
        PromptCacheStatus {
            state,
            reason: None,
            inferred: false,
            tier: PromptCacheTier::Short,
            touched_at: 0,
            ttl_ms: None,
            cached_tokens: 1_000,
            next_tokens: 500,
            warming: None,
        }
    }

    #[test]
    fn cold_brief_and_details_explain_the_resend() {
        let mut status = base_status(PromptCacheState::Cold);
        status.reason = Some(PromptCacheColdReason::Model {
            from: "old\nmodel".to_string(),
            to: "new".to_string(),
        });
        assert_eq!(
            prompt_cache_brief(&status, 0),
            [
                "Not cached — model changed: old model → new",
                "Next message re-sends ~500",
            ]
        );
        let rows = prompt_cache_details(&status, 0);
        assert_eq!(
            rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(),
            ["State", "Why not cached", "Tier", "Next message"]
        );
        assert_eq!(rows[0].tone, Some("warning"));
        assert_eq!(rows[2].value, "no expiry");
        assert_eq!(rows[3].value, "re-sends ~500 uncached");
    }

    #[test]
    fn warm_brief_reports_the_cached_share() {
        let status = base_status(PromptCacheState::Warm);
        assert_eq!(
            prompt_cache_brief(&status, 0),
            [
                "Cached, no expiry",
                "Next message reads ~500 from cache",
            ]
        );
        let rows = prompt_cache_details(&status, 0);
        assert_eq!(rows[0].value, "Cached");
        assert_eq!(rows[1].value, "never (the provider keeps it)");
        assert_eq!(
            rows.last().map(|row| row.value.as_str()),
            Some("reads ~500 from cache, sends ~0 new")
        );
    }
}