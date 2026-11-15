//! Wire types for the transcript format (`PRD.md` §13).

use serde::{Deserialize, Serialize};

/// Current transcript schema version.
pub const SESSION_VERSION: u32 = 3;

/// First line of every session file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    #[serde(rename = "type")]
    pub kind: String,
    pub version: u32,
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_session_files: Option<Vec<String>>,
    /// Lineage only: a forked session points at its parent. Never a coordination gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
}

impl SessionHeader {
    pub fn new(id: String, timestamp: String, cwd: String) -> Self {
        Self {
            kind: "session".to_string(),
            version: SESSION_VERSION,
            id,
            timestamp,
            cwd,
            title: None,
            title_source: None,
            additional_directories: None,
            previous_session_files: None,
            parent_session: None,
        }
    }
}

/// One transcript row. `payload` carries the type-specific fields flattened.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(flatten)]
    pub payload: serde_json::Value,
}

impl SessionEntry {
    pub fn new(
        kind: impl Into<String>,
        id: String,
        parent_id: Option<String>,
        timestamp: String,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            kind: kind.into(),
            id,
            parent_id,
            timestamp,
            payload,
        }
    }
}

/// One process = one session = one identity (`PRD.md` §6).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub name: String,
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}
