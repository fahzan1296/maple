//! Session core: append-only JSONL transcripts, bounded scans, blob spill.

pub mod blob;
pub mod path;
pub mod store;
pub mod types;

pub use store::{
    new_entry_id, now_rfc3339, LoadStats, SessionFile, SessionSummary, Store, HEADER_SCAN,
    MAX_ENTRIES, MAX_HEADER_BYTES, STREAM_THRESHOLD, TAIL_SCAN,
};
pub use types::{Identity, SessionEntry, SessionHeader, SESSION_VERSION};
