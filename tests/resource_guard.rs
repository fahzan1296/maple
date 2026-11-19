//! Negative tests for the runtime resource guard (ADR-0002 / SOP-RESLIM.3).

use maple::resource_guard::{collect_snapshot, over_limit, preflight, Snapshot};

const PEAK: u64 = 256;

fn ok_snapshot() -> Snapshot {
    Snapshot {
        avail_mb: 2 * PEAK,
        swap_used_mb: 0,
        load_avg: 0.5,
        nproc: 8,
        free_disk_mb: 64 * 1024,
        disk_pct: 40,
        fd_limit: 512,
        proc_limit: 128,
    }
}

#[test]
fn swap_in_use_aborts() {
    let mut s = ok_snapshot();
    s.swap_used_mb = 1;
    assert!(preflight(&s, PEAK, true).is_err());
}

#[test]
fn low_available_memory_aborts() {
    let mut s = ok_snapshot();
    s.avail_mb = 2 * PEAK - 1;
    assert!(preflight(&s, PEAK, true).is_err());
    // exactly 2x peak is the boundary: still allowed.
    s.avail_mb = 2 * PEAK;
    assert!(preflight(&s, PEAK, true).is_ok());
}

#[test]
fn load_above_half_nproc_aborts() {
    let mut s = ok_snapshot();
    s.nproc = 8;
    s.load_avg = 4.1;
    assert!(preflight(&s, PEAK, true).is_err());
    s.load_avg = 4.0;
    assert!(preflight(&s, PEAK, true).is_ok());
}

#[test]
fn disk_below_threshold_aborts() {
    let mut s = ok_snapshot();
    s.free_disk_mb = 100;
    s.disk_pct = 5;
    assert!(preflight(&s, PEAK, true).is_err());
    // unknown disk must not abort
    s.free_disk_mb = 0;
    s.disk_pct = 0;
    assert!(preflight(&s, PEAK, true).is_ok());
}

#[test]
fn missing_timeout_aborts() {
    assert!(preflight(&ok_snapshot(), PEAK, false).is_err());
}

#[test]
fn over_limit_boundaries() {
    // 80% of 256 MB = 204.8 MiB = 209715.2 kB
    assert!(over_limit(209_716, PEAK));
    assert!(!over_limit(209_715, PEAK));
    assert!(!over_limit(209_714, PEAK));
}

#[test]
fn collect_snapshot_does_not_panic() {
    let s = collect_snapshot();
    // macOS: /proc absent, everything unknown, never aborts.
    if cfg!(target_os = "linux") {
        assert!(s.nproc > 0);
    }
}
