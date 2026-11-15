use maple::session::store::{new_entry_id, now_rfc3339, Lease, Store};
use maple::session::types::SessionEntry;
use maple::session::{path::encode_cwd, SESSION_VERSION};
use serde_json::json;
use std::path::Path;

fn temp_home(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("maple-test-{}-{}", tag, uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cleanup(p: &Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_str().unwrap().to_string()
}

#[test]
fn encode_cwd_is_deterministic_and_filesystem_safe() {
    let a = encode_cwd("/work/maple");
    let b = encode_cwd("/work/maple");
    assert_eq!(a, b);
    assert!(!a.contains('/'));
    assert_eq!(a.len(), "/work/maple".len() * 2);
}

#[test]
fn create_append_load_roundtrip() {
    let home = temp_home("roundtrip");
    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(Some("test session")).unwrap();
    assert_eq!(file.header.version, SESSION_VERSION);
    assert_eq!(file.header.title.as_deref(), Some("test session"));

    for text in ["hello", "world"] {
        store
            .append(
                &file.path,
                &SessionEntry::new(
                    "message",
                    new_entry_id(),
                    None,
                    now_rfc3339(),
                    json!({ "text": text }),
                ),
            )
            .unwrap();
    }

    let (header, entries) = store.load(&file.path).unwrap();
    assert_eq!(header.id, file.header.id);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].payload["text"], "hello");
    assert_eq!(entries[1].payload["text"], "world");
    cleanup(&home);
}

#[test]
fn branch_records_lineage_without_locking_it() {
    let home = temp_home("branch");
    let store = Store::with_home(home.clone(), "/work/maple");
    let parent = store.create(None).unwrap();
    let child = store.branch(&stem(&parent.path), None).unwrap();
    assert_eq!(
        child.header.parent_session.as_deref(),
        Some(parent.header.id.as_str())
    );
    assert_eq!(child.header.previous_session_files.as_ref().unwrap().len(), 1);
    // Both sessions remain independently writable.
    store
        .append(
            &parent.path,
            &SessionEntry::new("message", new_entry_id(), None, now_rfc3339(), json!({})),
        )
        .unwrap();
    store
        .append(
            &child.path,
            &SessionEntry::new("message", new_entry_id(), None, now_rfc3339(), json!({})),
        )
        .unwrap();
    cleanup(&home);
}

#[test]
fn open_by_stem_and_by_id() {
    let home = temp_home("open");
    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(None).unwrap();
    assert_eq!(store.open(&stem(&file.path)).unwrap().header.id, file.header.id);
    assert_eq!(store.open(&file.header.id).unwrap().header.id, file.header.id);
    assert!(store.open("does-not-exist").is_err());
    cleanup(&home);
}

#[test]
fn list_returns_headers_newest_first() {
    let home = temp_home("list");
    let store = Store::with_home(home.clone(), "/work/maple");
    store.create(None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    store.create(None).unwrap();
    let list = store.list().unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[0].timestamp >= list[1].timestamp);
    cleanup(&home);
}

#[test]
fn blob_roundtrip_is_content_addressed() {
    let home = temp_home("blob");
    let store = Store::with_home(home.clone(), "/work/maple");
    let hex = store.store_blob(b"hello blob").unwrap();
    assert_eq!(hex.len(), 64);
    assert_eq!(store.read_blob(&hex).unwrap(), b"hello blob");
    assert_eq!(store.store_blob(b"hello blob").unwrap(), hex);
    cleanup(&home);
}

#[test]
fn lease_rejects_a_second_live_holder() {
    let home = temp_home("lease");
    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(None).unwrap();
    let lease = Lease::acquire(&file.path).unwrap();
    assert!(Lease::acquire(&file.path).is_err());
    drop(lease);
    assert!(Lease::acquire(&file.path).is_ok());
    cleanup(&home);
}

#[test]
fn bounded_scan_keeps_only_the_tail_for_large_sessions() {
    let home = temp_home("large");
    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(None).unwrap();
    let payload = "x".repeat(4 * 1024);
    for _ in 0..2200 {
        store
            .append(
                &file.path,
                &SessionEntry::new(
                    "blob",
                    new_entry_id(),
                    None,
                    now_rfc3339(),
                    json!({ "data": payload }),
                ),
            )
            .unwrap();
    }
    let (_header, entries) = store.load(&file.path).unwrap();
    assert!(!entries.is_empty());
    assert!(
        entries.len() < 2200,
        "bounded scan should drop older entries, got {}",
        entries.len()
    );
    cleanup(&home);
}
