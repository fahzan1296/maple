//! Runtime resource guard (ADR-0002, `KONTRAK.md` §SOP-RESLIM, `PRD.md` §11).
//!
//! Single-process scope: snapshot pre-flight, an 80% over-limit predicate, and
//! RSS sampling. No cgroups/containers. macOS has no `/proc`, so snapshots
//! degrade to values that never abort and RSS sampling returns `None`.
//! A check that cannot run is not silently treated as a pass by the caller.

use std::fs;

/// Resource snapshot. Unknown values are `0` and never trigger an abort
/// (`0` swap, `0` load, `0` free disk = unknown, limits `0` = unset).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapshot {
    pub avail_mb: u64,
    pub swap_used_mb: u64,
    pub load_avg: f64,
    pub nproc: u32,
    pub free_disk_mb: u64,
    pub disk_pct: u64,
    pub fd_limit: u64,
    pub proc_limit: u64,
}

/// SOP-RESLIM.2 disk floor: ≥20% free OR ≥4 GB free, whichever is larger.
pub const DISK_MIN_MB: u64 = 4 * 1024;
/// SOP-RESLIM.2 disk floor in percent.
pub const DISK_MIN_PCT: u64 = 20;

fn read_lines(path: &str) -> Option<Vec<String>> {
    fs::read_to_string(path).ok().map(|t| t.lines().map(|l| l.to_string()).collect())
}

fn meminfo_mb(field: &str) -> u64 {
    let lines = match read_lines("/proc/meminfo") {
        Some(lines) => lines,
        None => return 0,
    };
    for line in lines {
        if let Some(rest) = line.strip_prefix(field) {
            let kb: u64 = rest
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .unwrap_or(0);
            return kb / 1024;
        }
    }
    0
}

fn load_avg() -> f64 {
    read_lines("/proc/loadavg")
        .and_then(|l| l.first().and_then(|s| s.split_whitespace().next()).and_then(|v| v.parse().ok()))
        .unwrap_or(0.0)
}

fn nproc() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1)
}

fn rlimit(res: u32) -> u64 {
    // SAFETY: getrlimit only fills the provided struct; no side effects.
    unsafe {
        let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        if libc::getrlimit(res, &mut lim) == 0 {
            lim.rlim_cur as u64
        } else {
            0
        }
    }
}

/// Disk space of the current directory. `(0, 0)` when it cannot be determined.
fn disk_mb() -> (u64, u64) {
    // SAFETY: statvfs fills the provided struct; path is a valid CString.
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        let c = match std::ffi::CString::new(".") {
            Ok(c) => c,
            Err(_) => return (0, 0),
        };
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return (0, 0);
        }
        let free = st.f_bavail as u64 * st.f_frsize as u64;
        let total = st.f_blocks as u64 * st.f_frsize as u64;
        let pct = if total == 0 { 0 } else { free * 100 / total };
        (free / 1024 / 1024, pct)
    }
}

pub fn collect_snapshot() -> Snapshot {
    let (free_disk_mb, disk_pct) = disk_mb();
    Snapshot {
        avail_mb: meminfo_mb("MemAvailable:"),
        swap_used_mb: meminfo_mb("SwapTotal:").saturating_sub(meminfo_mb("SwapFree:")),
        load_avg: load_avg(),
        nproc: nproc(),
        free_disk_mb,
        disk_pct,
        fd_limit: rlimit(libc::RLIMIT_NOFILE),
        proc_limit: rlimit(libc::RLIMIT_NPROC),
    }
}

fn disk_short(s: &Snapshot) -> bool {
    // Only judge disk when it is known; unknown (0 free, 0%) never aborts.
    if s.free_disk_mb == 0 && s.disk_pct == 0 {
        return false;
    }
    // Abort below 20% AND below 4 GB ("whichever is larger" floor).
    s.free_disk_mb < DISK_MIN_MB && s.disk_pct < DISK_MIN_PCT
}

/// SOP-RESLIM.3 abort conditions. Returns the abort reason, or `Ok(())`.
pub fn preflight(s: &Snapshot, expected_peak_mb: u64, timeout_set: bool) -> Result<(), String> {
    let need = expected_peak_mb.saturating_mul(2);
    if s.avail_mb < need {
        return Err(format!(
            "resource guard: available memory {} MB < 2x expected peak {} MB",
            s.avail_mb, expected_peak_mb
        ));
    }
    if s.swap_used_mb > 0 {
        return Err(format!("resource guard: swap in use ({} MB)", s.swap_used_mb));
    }
    if s.nproc > 0 && s.load_avg > (s.nproc as f64) / 2.0 {
        return Err(format!(
            "resource guard: load {:.2} > nproc/2 ({})",
            s.load_avg, s.nproc / 2
        ));
    }
    if disk_short(s) {
        return Err(format!(
            "resource guard: free disk {} MB ({}%) below threshold {} MB / {}%",
            s.free_disk_mb, s.disk_pct, DISK_MIN_MB, DISK_MIN_PCT
        ));
    }
    if !timeout_set {
        return Err("resource guard: no explicit timeout".to_string());
    }
    Ok(())
}

/// True when RSS reaches 80% of the limit (SOP-RESLIM.4).
pub fn over_limit(rss_kb: u64, limit_mb: u64) -> bool {
    rss_kb as f64 >= limit_mb as f64 * 0.8 * 1024.0
}

/// Current process RSS in kB from `/proc/<pid>/status` (`VmRSS`). macOS: `None`.
pub fn sample_rss_kb(pid: i32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb);
        }
    }
    None
}
