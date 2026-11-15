//! Content-addressed blob spill for large artifacts (`PRD.md` §6).

use sha2::{Digest, Sha256};
use std::path::Path;

/// Store `bytes` under its SHA-256 hex digest; idempotent.
pub fn store(root: &Path, bytes: &[u8]) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::create_dir_all(root)?;
    let path = root.join(&hex);
    if !path.exists() {
        std::fs::write(&path, bytes)?;
    }
    Ok(hex)
}

pub fn read(root: &Path, hex: &str) -> std::io::Result<Vec<u8>> {
    std::fs::read(root.join(hex))
}
