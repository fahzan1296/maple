//! Filesystem layout: `~/.maple/agent/sessions/<encoded-cwd>/<ts>_<id>.jsonl`.

use std::path::{Path, PathBuf};

/// Root agent state directory. Override with `MAPLE_HOME` for tests/ports.
pub fn agent_home() -> PathBuf {
    if let Some(home) = std::env::var_os("MAPLE_HOME") {
        return PathBuf::from(home);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".maple");
    }
    PathBuf::from(".maple")
}

/// Filesystem-safe, reversible encoding of a cwd (hex of the UTF-8 bytes).
pub fn encode_cwd(cwd: &str) -> String {
    cwd.bytes().map(|b| format!("{:02x}", b)).collect()
}

/// Sidecar lock used to keep one writer per session file.
pub fn lock_path(session_file: &Path) -> PathBuf {
    let mut raw = session_file.as_os_str().to_os_string();
    raw.push(".lock");
    PathBuf::from(raw)
}
