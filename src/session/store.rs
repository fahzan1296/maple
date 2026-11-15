//! Append-only session store: create/open/append/load/list/branch + blob spill.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::blob;
use super::path::{self, lock_path};
use super::types::{SessionEntry, SessionHeader};

/// Bytes read to find the header line.
pub const HEADER_SCAN: u64 = 4 * 1024;
/// Bytes of tail kept when a session exceeds [`STREAM_THRESHOLD`].
pub const TAIL_SCAN: u64 = 32 * 1024;
/// Sessions at or above this size are streamed (header + tail only).
pub const STREAM_THRESHOLD: u64 = 8 * 1024 * 1024;

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
        let path = self.sessions_dir().join(format!("{}.jsonl", stem));
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let _lease = Lease::acquire(&path)?;
        let mut f = File::create(&path).with_context(|| format!("create {}", path.display()))?;
        writeln!(f, "{}", serde_json::to_string(&header)?).context("write session header")?;
        f.flush()?;
        Ok(SessionFile { header, path })
    }

    /// Open by file stem, falling back to a session id.
    pub fn open(&self, arg: &str) -> Result<SessionFile> {
        let direct = self.sessions_dir().join(format!("{}.jsonl", arg));
        let path = if direct.exists() { direct } else { self.find_by_id(arg)? };
        let header = read_header(&path)?;
        Ok(SessionFile { header, path })
    }

    fn find_by_id(&self, id: &str) -> Result<PathBuf> {
        for file in self.session_files()? {
            if read_header(&file).map(|h| h.id == id).unwrap_or(false) {
                return Ok(file);
            }
        }
        bail!("session not found: {}", id)
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
        let mut out = Vec::new();
        for file in self.session_files()? {
            out.push(read_header(&file)?);
        }
        out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(out)
    }

    /// Bounded read: header always; whole file when small, tail only when large.
    pub fn load(&self, path: &Path) -> Result<(SessionHeader, Vec<SessionEntry>)> {
        let len = fs::metadata(path)?.len();
        let mut file = File::open(path)?;
        let header = {
            let mut buf = vec![0u8; HEADER_SCAN.min(len.max(1)) as usize];
            let n = file.read(&mut buf).context("read session header")?;
            let text = String::from_utf8_lossy(&buf[..n]);
            let first = text.lines().next().context("empty session file")?;
            serde_json::from_str::<SessionHeader>(first).context("parse session header")?
        };
        let entries = if len >= STREAM_THRESHOLD {
            let seek = (len - TAIL_SCAN).max(0);
            file.seek(SeekFrom::Start(seek))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            parse_entries(&String::from_utf8_lossy(&buf), true)
        } else {
            file.seek(SeekFrom::Start(0))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            parse_entries(&String::from_utf8_lossy(&buf), true)
        };
        Ok((header, entries))
    }

    pub fn append(&self, path: &Path, entry: &SessionEntry) -> Result<()> {
        let _lease = Lease::acquire(path)?;
        let mut f = OpenOptions::new()
            .append(true)
            .open(path)
            .with_context(|| format!("open {} for append", path.display()))?;
        writeln!(f, "{}", serde_json::to_string(entry)?).context("write session entry")?;
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

pub fn read_header(path: &Path) -> Result<SessionHeader> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).context("read session header")?;
    Ok(serde_json::from_str(line.trim()).context("parse session header")?)
}

fn parse_entries(text: &str, skip_first: bool) -> Vec<SessionEntry> {
    let mut lines = text.lines();
    if skip_first {
        let _ = lines.next();
    }
    lines
        .filter_map(|l| serde_json::from_str::<SessionEntry>(l).ok())
        .collect()
}

/// One-writer guard. Stale locks (dead pid) are reclaimed.
pub struct Lease {
    path: PathBuf,
}

impl Lease {
    pub fn acquire(session_file: &Path) -> Result<Self> {
        let path = lock_path(session_file);
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(pid) = raw.trim().parse::<i32>() {
                if pid > 0 && pid_alive(pid) {
                    bail!("session {} is leased by live pid {}", session_file.display(), pid);
                }
            }
            let _ = fs::remove_file(&path);
        }
        fs::write(&path, std::process::id().to_string())
            .with_context(|| format!("write lock {}", path.display()))?;
        Ok(Self { path })
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
