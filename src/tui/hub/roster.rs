//! Roster peers: one per session file, metrics aggregated from the transcript.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::session::store::{SessionFile, Store};
use crate::session::types::SessionEntry;
use super::row::short_id;

/// Bound on roster scans so a large session directory cannot stall the TUI.
pub const MAX_PEERS: usize = 200;
/// Entries per session kept in the Activity feed.
pub const MAX_ACTIVITY_PER_SESSION: usize = 6;
/// Global cap on the Activity feed.
pub const MAX_ACTIVITY: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Active,
    Busy,
    Idle,
    Error,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Busy => "busy",
            Status::Idle => "idle",
            Status::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub session_id: String,
    pub title: Option<String>,
    pub path: std::path::PathBuf,
    pub cwd: String,
    pub model: String,
    pub status: Status,
    pub task: String,
    pub tool: String,
    pub cost: f64,
    pub duration_ms: u64,
    pub requests: u64,
    pub tools: u64,
    pub tokens: u64,
    pub parent: Option<String>,
    pub registered_at: String,
    /// File modification age in milliseconds; drives the `age` column.
    pub age_ms: u128,
}

/// One recent transcript entry, flattened for the hub Activity feed.
#[derive(Debug, Clone)]
pub struct Activity {
    pub session_id: String,
    pub title: String,
    pub path: std::path::PathBuf,
    pub kind: String,
    pub text: String,
    pub timestamp: String,
    pub model: String,
}

/// Roster scan result: peer rows plus the recent-entry feed.
#[derive(Debug, Default)]
pub struct Roster {
    pub peers: Vec<Peer>,
    pub activity: Vec<Activity>,
}

/// Aggregate one transcript into a peer row.
pub fn peer_from(file: &SessionFile, entries: &[SessionEntry], now_ms: u128) -> Peer {
    let mut cost = 0.0f64;
    let mut tokens = 0u64;
    let mut tools = 0u64;
    let mut requests = 0u64;
    let mut model = String::new();
    let mut task = String::new();
    let mut last_tool = String::new();
    let mut first_ms: Option<u128> = None;
    let mut last_ms: Option<u128> = None;

    for entry in entries {
        match entry.kind.as_str() {
            "usage" => {
                requests += 1;
                cost += entry.payload.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
                tokens += entry.payload.get("tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(name) = entry.payload.get("tool").and_then(|v| v.as_str()) {
                    last_tool = name.to_string();
                }
            }
            "tool" => {
                tools += 1;
                if let Some(name) = entry
                    .payload
                    .get("name")
                    .or_else(|| entry.payload.get("tool"))
                    .and_then(|v| v.as_str())
                {
                    last_tool = name.to_string();
                }
            }
            "message" => {
                if let Some(text) = entry.payload.get("text").and_then(|v| v.as_str()) {
                    task = single_line(text);
                }
            }
            _ => {}
        }
        if model.is_empty() {
            if let Some(value) = entry.payload.get("model").and_then(|v| v.as_str()) {
                model = value.to_string();
            }
        }
        if let Some(ms) = parse_ms(&entry.timestamp) {
            first_ms = Some(first_ms.map_or(ms, |prev| prev.min(ms)));
            last_ms = Some(last_ms.map_or(ms, |prev| prev.max(ms)));
        }
    }

    let duration_ms = match (first_ms, last_ms) {
        (Some(first), Some(last)) => last.saturating_sub(first) as u64,
        _ => 0,
    };
    let age_ms = modified_age_ms(&file.path, now_ms);
    let status = match entries.last() {
        Some(entry) if matches!(entry.kind.as_str(), "error" | "abort") => Status::Error,
        _ if age_ms < 5_000 => Status::Busy,
        _ if age_ms < 60_000 => Status::Active,
        _ => Status::Idle,
    };

    Peer {
        session_id: file.header.id.clone(),
        title: file.header.title.clone(),
        path: file.path.clone(),
        cwd: file.header.cwd.clone(),
        model: if model.is_empty() { "unknown".to_string() } else { model },
        status,
        task,
        tool: last_tool,
        cost,
        duration_ms,
        requests,
        tools,
        tokens,
        parent: file.header.parent_session.clone(),
        registered_at: file.header.timestamp.clone(),
        age_ms,
    }
}

/// Scan the session directory into peer rows plus a recent-entry feed.
pub fn scan(store: &Store) -> Roster {
    let now_ms = now_ms();
    let mut peers = Vec::new();
    let mut activity = Vec::new();
    for file in store.list_files().unwrap_or_default().into_iter().take(MAX_PEERS) {
        let Ok((_, entries)) = store.load(&file.path) else {
            continue;
        };
        let peer = peer_from(&file, &entries, now_ms);
        for entry in entries.iter().rev().take(MAX_ACTIVITY_PER_SESSION) {
            activity.push(Activity {
                session_id: peer.session_id.clone(),
                title: peer
                    .title
                    .clone()
                    .unwrap_or_else(|| short_id(&peer.session_id).to_string()),
                path: peer.path.clone(),
                kind: entry.kind.clone(),
                text: entry_text(entry),
                timestamp: entry.timestamp.clone(),
                model: peer.model.clone(),
            });
        }
        peers.push(peer);
    }
    activity.sort_by_key(|item| std::cmp::Reverse(parse_ms(&item.timestamp).unwrap_or(0)));
    activity.truncate(MAX_ACTIVITY);
    Roster { peers, activity }
}

/// Compact single-line text for an entry, used by the Activity feed.
fn entry_text(entry: &SessionEntry) -> String {
    match entry.kind.as_str() {
        "message" => single_line(
            entry
                .payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        ),
        "usage" => {
            let cost = entry
                .payload
                .get("cost")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let tokens = entry
                .payload
                .get("tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let tool = entry
                .payload
                .get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            single_line(&format!("${cost:.4} · {tokens} tok{tool}"))
        }
        "tool" => single_line(
            entry
                .payload
                .get("name")
                .or_else(|| entry.payload.get("tool"))
                .and_then(|value| value.as_str())
                .unwrap_or("tool"),
        ),
        _ => single_line(&entry.kind),
    }
}

fn modified_age_ms(path: &std::path::Path, now_ms: u128) -> u128 {
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|stamp| stamp.duration_since(UNIX_EPOCH).ok())
        .map(|delta| delta.as_millis())
        .unwrap_or(u128::MAX);
    now_ms.saturating_sub(modified)
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_millis())
        .unwrap_or(0)
}

fn single_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

/// Parse an RFC3339 timestamp into epoch millis.
pub fn parse_ms(stamp: &str) -> Option<u128> {
    let parsed =
        time::OffsetDateTime::parse(stamp.trim(), &time::format_description::well_known::Rfc3339)
            .ok()?;
    Some(u128::try_from(parsed.unix_timestamp_nanos() / 1_000_000).unwrap_or(0))
}
