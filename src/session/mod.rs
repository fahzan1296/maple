//! Session core: append-only JSONL transcripts, bounded scans, blob spill.

pub mod blob;
pub mod path;
pub mod store;
pub mod types;

pub use store::{SessionFile, SessionSummary, Store, HEADER_SCAN, STREAM_THRESHOLD, TAIL_SCAN};
pub use types::{Identity, SessionEntry, SessionHeader, SESSION_VERSION};
