//! Append-only session store: create/open/append/load/list/branch + blob spill.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::blob;
use super::path::{self, lock_path};
use super::types::{SessionHeader, SessionEntry};

/// Bytes of the file read to find the header line.
pub const HEADER_SCAN: u64 = 4 * 1024;
/// Hard cap for a single JSONL header line, including its newline (`read_line`
/// is unbounded and would otherwise accept an arbitrarily large first line).
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Bytes of tail kept when a session exceeds [`STREAM_THRESHOLD`].
pub const TAIL_SCAN: u64 = 32 * 1024;
/// Sessions at or above this size are streamed (header + tail only).
pub const STREAM_THRESHOLD: u64 = 8 * 1024 * 1024;
/// Maximum number of entries returned by a load; the oldest are dropped so a
/// huge transcript cannot exhaust memory.
pub const MAX_ENTRIES: usize = 5_000;
/// Characters allowed in a file stem passed to [`Store::open`].
const SAFE_ARG: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-";
/// Lock reclamation attempts before giving up (no busy-looping).
const LOCK_RETRIES: u32 = 3;

pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub fn new_session_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Short 8-char id for transcript rows (visual identity, not uniqueness-critical).
pub fn new_entry_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()[..8].to_string()
}

fn session_file_stem(timestamp: &str, session_id: &str) -> String {
    let digits: String = timestamp.chars().filter(|c| c.is_ascii_digit()).collect();
    let compact = &digits[..digits.len().min(14)];
    format!("{}_{}", compact, session_id)
}

pub struct SessionFile {
    pub header: SessionHeader,
    pub path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub session_id: String,
    pub file: String,
    pub cwd: String,
    pub created_at: String,
    pub title: Option<String>,
    pub parent_session: Option<String>,
    pub entries: usize,
}

/// Quality report for a transcript load. Corrupt lines are never silently
/// dropped anymore: callers can see how many were skipped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadStats {
    /// Lines that failed to parse as JSON.
    pub dropped: usize,
    /// The file was longer than the scanned window, so older entries are absent.
    pub truncated: bool,
}

impl LoadStats {
    fn with_truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }
}

pub struct Store {
    home: PathBuf,
    cwd: String,
}

impl Store {
    pub fn new(cwd: &str) -> Self {
        Self { home: path::agent_home(), cwd: cwd.to_string() }
    }

    pub fn with_home(home: PathBuf, cwd: &str) -> Self {
        Self { home, cwd: cwd.to_string() }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.home
            .join("agent")
            .join("sessions")
            .join(path::encode_cwd(&self.cwd))
    }

    fn blobs_dir(&self) -> PathBuf {
        self.home.join("agent").join("blobs")
    }

    pub fn create(&self, title: Option<&str>) -> Result<SessionFile> {
        self.create_inner(title, None)
    }

    /// Fork `parent` into a new session; lineage is recorded, never enforced.
    pub fn branch(&self, parent: &str, title: Option<&str>) -> Result<SessionFile> {
        let parent_file = self.open(parent)?;
        self.create_inner(title, Some((parent_file.header, parent_file.path)))
    }

    fn create_inner(
        &self,
        title: Option<&str>,
        parent: Option<(SessionHeader, PathBuf)>,
    ) -> Result<SessionFile> {
        let timestamp = now_rfc3339();
        let id = new_session_id();
        let mut header = SessionHeader::new(id, timestamp.clone(), self.cwd.clone());
        header.title = title.map(str::to_string);
        header.title_source = title.map(|_| "user".to_string());
        if let Some((parent_header, parent_path)) = parent {
            header.parent_session = Some(parent_header.id);
            header.previous_session_files =
                Some(vec![parent_path.to_string_lossy().into_owned()]);
        }
        let stem = session_file_stem(&timestamp, &header.id);
        let path = self.sessions_dir().join(format!("{stem}.jsonl"));
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let _lease = Lease::acquire(&path)?;
        let mut f = File::create(&path).with_context(|| format!("create {}", path.display()))?;
        // One serialized line, one write: a concurrent reader never sees a torn header.
        let line = serde_json::to_string(&header)? + "\n";
        f.write_all(line.as_bytes()).context("write session header")?;
        f.flush()?;
        Ok(SessionFile { header, path })
    }

    /// Open by file stem, falling back to a session id or id prefix.
    ///
    /// `arg` is treated as a filename, so it must be a bare `[A-Za-z0-9._-]`
    /// name; anything path-shaped (`/`, `\`, `..`, absolute) is rejected.
    pub fn open(&self, arg: &str) -> Result<SessionFile> {
        validate_arg(arg)?;
        let direct = self.sessions_dir().join(format!("{arg}.jsonl"));
        let path = if direct.exists() {
            direct
        } else {
            self.find_by_id(arg)?
        };
        let header = read_header(&path)?;
        Ok(SessionFile { header, path })
    }

    /// Match `id` as an exact session id first, then as an unambiguous prefix.
    fn find_by_id(&self, id: &str) -> Result<PathBuf> {
        let files = self.session_files()?;
        for file in &files {
            if read_header(file).map(|h| h.id == id).unwrap_or(false) {
                return Ok(file.clone());
            }
        }
        let mut matches = Vec::new();
        for file in &files {
            if let Ok(header) = read_header(file) {
                if header.id.starts_with(id) {
                    matches.push(file.clone());
                }
            }
        }
        match matches.len() {
            0 => bail!("session not found: {id}"),
            1 => Ok(matches.remove(0)),
            n => bail!(
                "ambiguous session id '{id}': {n} matches, pass more characters"
            ),
        }
    }

    fn session_files(&self) -> Result<Vec<PathBuf>> {
        let dir = self.sessions_dir();
        let mut out = Vec::new();
        if !dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                out.push(path);
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn list(&self) -> Result<Vec<SessionHeader>> {
        self.list_limited(None)
    }

    /// Bounded variant of [`Store::list`]: newest first, at most `limit`.
    pub fn list_limited(&self, limit: Option<usize>) -> Result<Vec<SessionHeader>> {
        let mut out = Vec::new();
        for file in self.session_files()? {
            out.push(read_header(&file)?);
        }
        out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        if let Some(limit) = limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    /// Headers with their paths, newest first (roster scan).
    pub fn list_files(&self) -> Result<Vec<SessionFile>> {
        self.list_files_limited(None)
    }

    /// Bounded variant of [`Store::list_files`] (roster scan).
    pub fn list_files_limited(&self, limit: Option<usize>) -> Result<Vec<SessionFile>> {
        let mut out = Vec::new();
        for file in self.session_files()? {
            out.push(SessionFile { header: read_header(&file)?, path: file });
        }
        out.sort_by(|a, b| b.header.timestamp.cmp(&a.header.timestamp));
        if let Some(limit) = limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    /// Backwards-compatible load: header always, whole file when small, tail
    /// when large. Dropped corrupt lines are discarded; use
    /// [`Store::load_with_stats`] to see them.
    pub fn load(&self, path: &Path) -> Result<(SessionHeader, Vec<SessionEntry>)> {
        Ok(self.load_with_stats(path)?.0)
    }

    /// Like [`Store::load`] but also reports dropped/corrupt lines and whether
    /// the oldest entries were cut off.
    pub fn load_with_stats(
        &self,
        path: &Path,
    ) -> Result<((SessionHeader, Vec<SessionEntry>), LoadStats)> {
        let len = fs::metadata(path)?.len();
        let mut file = File::open(path)?;
        let header = {
            let mut buf = vec![0u8; HEADER_SCAN.min(len.max(1)) as usize];
            let n = file.read(&mut buf).context("read session header")?;
            // A truncated read means the first line is still open: keep reading
            // under a hard cap instead of parsing a partial header.
            if !buf[..n].contains(&b'\n') {
                let mut rest = Vec::new();
                file.read_to_end(&mut rest).context("read session header")?;
                buf.extend_from_slice(&rest);
                if buf.len() > MAX_HEADER_BYTES {
                    bail!(
                        "session header too long (>{MAX_HEADER_BYTES} bytes): {}",
                        path.display()
                    );
                }
            }
            let text = String::from_utf8_lossy(&buf);
            let first = text.lines().next().context("empty session file")?;
            serde_json::from_str::<SessionHeader>(first).context("parse session header")?
        };
        let (text, truncated) = if len >= STREAM_THRESHOLD {
            // Seek back far enough for the tail, then skip to the first newline
            // after the seek so no valid entry is split across the boundary.
            let seek = (len - TAIL_SCAN).max(0);
            file.seek(SeekFrom::Start(seek))?;
            let mut raw = Vec::new();
            file.read_to_end(&mut raw)?;
            let mut text = String::from_utf8_lossy(&raw).into_owned();
            if seek > 0 {
                if let Some(pos) = text.find('\n') {
                    text.drain(..=pos);
                } else {
                    // No newline in the window at all: nothing usable here.
                    text.clear();
                }
            }
            (text, true)
        } else {
            file.seek(SeekFrom::Start(0))?;
            let mut raw = Vec::new();
            file.read_to_end(&mut raw)?;
            (String::from_utf8_lossy(&raw).into_owned(), false)
        };
        let (entries, stats) = parse_entries(&text);
        let entries = bound_entries(entries);
        Ok(((header, entries), stats.with_truncated(truncated)))
    }

    pub fn append(&self, path: &Path, entry: &SessionEntry) -> Result<()> {
        let _lease = Lease::acquire(path)?;
        let mut f = OpenOptions::new()
            .append(true)
            .open(path)
            .with_context(|| format!("open {} for append", path.display()))?;
        // Serialize fully, then one write + flush: readers see either the old
        // file or the complete new line, never a torn mix.
        let line = serde_json::to_string(entry)? + "\n";
        f.write_all(line.as_bytes())
            .context("write session entry")?;
        f.flush()?;
        Ok(())
    }

    pub fn store_blob(&self, bytes: &[u8]) -> Result<String> {
        blob::store(&self.blobs_dir(), bytes).context("store blob")
    }

    pub fn read_blob(&self, hex: &str) -> Result<Vec<u8>> {
        blob::read(&self.blobs_dir(), hex).context("read blob")
    }
}

/// Reject anything that could escape the session directory.
fn validate_arg(arg: &str) -> Result<()> {
    if arg.is_empty() || !arg.bytes().all(|b| SAFE_ARG.contains(b as char)) {
        bail!(
            "invalid session name '{arg}': expected [A-Za-z0-9._-], got a path or unsupported character"
        );
    }
    Ok(())
}

/// Keep the newest entries when a transcript exceeds [`MAX_ENTRIES`].
fn bound_entries(mut entries: Vec<SessionEntry>) -> Vec<SessionEntry> {
    if entries.len() > MAX_ENTRIES {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    entries
}

pub fn read_header(path: &Path) -> Result<SessionHeader> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let n = reader.read_line(&mut line).context("read session header")?;
    if n == 0 {
        bail!("empty session file: {}", path.display());
    }
    if !line.ends_with('\n') || n > MAX_HEADER_BYTES {
        bail!(
            "session header is not a complete line (>{MAX_HEADER_BYTES} bytes): {}",
            path.display()
        );
    }
    Ok(serde_json::from_str(line.trim()).context("parse session header")?)
}

/// Parse every JSONL line after the header. Invalid lines are counted, not
/// swallowed.
fn parse_entries(text: &str) -> (Vec<SessionEntry>, LoadStats) {
    let mut entries = Vec::new();
    let mut dropped = 0usize;
    let mut lines = text.lines();
    let _ = lines.next(); // header line
    for line in lines {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<SessionEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => dropped += 1,
        }
    }
    (entries, LoadStats { dropped, truncated: false })
}

/// One-writer guard. The lock file is created atomically with `create_new`;
/// stale locks (dead pid) are reclaimed.
pub struct Lease {
    path: PathBuf,
}

impl Lease {
    pub fn acquire(session_file: &Path) -> Result<Self> {
        let path = lock_path(session_file);
        for attempt in 0..LOCK_RETRIES {
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(mut f) => {
                    f.write_all(std::process::id().to_string().as_bytes())
                        .with_context(|| format!("write lock {}", path.display()))?;
                    return Ok(Self { path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    let held_by = match fs::read_to_string(&path) {
                        Ok(raw) => raw.trim().parse::<i32>().ok(),
                        Err(_) => None,
                    };
                    match held_by {
                        Some(pid) if pid > 0 && pid_alive(pid) => {
                            bail!("session {} is leased by live pid {}", session_file.display(), pid);
                        }
                        _ => {
                            let _ = fs::remove_file(&path);
                            if attempt + 1 < LOCK_RETRIES {
                                continue;
                            }
                            bail!(
                                "session {} lock {} is stale and could not be reclaimed",
                                session_file.display(),
                                path.display()
                            );
                        }
                    }
                }
                Err(err) => {
                    return Err(err).with_context(|| format!("write lock {}", path.display()));
                }
            }
        }
        bail!("session {} lock could not be acquired", session_file.display());
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn pid_alive(pid: i32) -> bool {
    // SAFETY: kill(pid, 0) performs error checking only; no signal is delivered.
    unsafe { libc::kill(pid, 0) == 0 }
}
