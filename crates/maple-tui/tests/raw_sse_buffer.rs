// Ported core cases from l/test/raw-sse-buffer.test.ts to Rust raw_sse_buffer APIs.
// Deviations: Rust uses RawSseDebugBuffer/Model/ProviderResponseMetadata/RawSseEvent types,
// resolve_raw_sse_debug_buffer takes owner_id Option<usize>, timestamps are generated, and
// formatting/truncation/compaction internals differ; assertions target equivalent behaviors.

use std::rc::Rc;

use maple_tui::apps::debug::raw_sse_buffer::{
    raw_sse_record_lines, resolve_raw_sse_debug_buffer, Model, ProviderResponseMetadata, RawSseDebugBuffer,
    RawSseEvent,
};

fn model() -> Model {
    Model {
        provider: Some("anthropic".to_string()),
        id: Some("claude-test".to_string()),
        api: Some("anthropic-messages".to_string()),
    }
}

#[test]
fn records_response_metadata_and_raw_sse_frame_lines_for_diagnostics() {
    let mut buffer = RawSseDebugBuffer::new();
    buffer.record_response(
        &ProviderResponseMetadata {
            status: 200,
            request_id: Some("req_123".to_string()),
            last_transport: Some("sse".to_string()),
        },
        Some(&model()),
    );
    buffer.record_event(
        &RawSseEvent {
            event: Some("content_block_delta".to_string()),
            raw: vec![
                "event: content_block_delta".to_string(),
                "data: {\"type\":\"content_block_delta\"}".to_string(),
            ],
        },
        Some(&model()),
    );

    let snapshot = buffer.snapshot();
    assert_eq!(snapshot.total_events, 1);
    assert_eq!(snapshot.records.len(), 2);
    let response_line = raw_sse_record_lines(&snapshot.records[0]).join("\n");
    assert!(response_line.contains("provider=anthropic"));
    assert!(response_line.contains("model=claude-test"));
    assert_eq!(
        raw_sse_record_lines(&snapshot.records[1]),
        vec![
            "event: content_block_delta".to_string(),
            "data: {\"type\":\"content_block_delta\"}".to_string(),
        ]
    );
    assert!(buffer.to_raw_text().contains("event: content_block_delta"));
}

#[test]
fn snapshot_tracks_counts_and_clear_resets_state() {
    let mut buffer = RawSseDebugBuffer::new();
    buffer.record_event(&RawSseEvent { event: None, raw: vec!["data: {}".to_string()] }, Some(&model()));
    buffer.record_event(&RawSseEvent { event: None, raw: vec!["data: {}".to_string()] }, Some(&model()));
    assert_eq!(buffer.snapshot().total_events, 2);
    assert_eq!(buffer.snapshot().records.len(), 2);
    buffer.clear();
    let snap = buffer.snapshot();
    assert!(snap.records.is_empty());
    assert_eq!(snap.total_events, 0);
    assert_eq!(snap.dropped_records, 0);
}

#[test]
fn resolves_same_buffer_for_same_owner_and_shared_global_fallback() {
    let a = resolve_raw_sse_debug_buffer(Some(9001));
    let b = resolve_raw_sse_debug_buffer(Some(9001));
    assert!(Rc::ptr_eq(&a, &b));
    let c = resolve_raw_sse_debug_buffer(None);
    let d = resolve_raw_sse_debug_buffer(None);
    assert!(Rc::ptr_eq(&c, &d));
}

#[test]
fn evicts_oldest_records_past_event_limit_and_emits_dropped_header() {
    let mut buffer = RawSseDebugBuffer::new();
    let appends = 1_005usize; // > MAX_RAW_SSE_EVENTS (1000)
    for i in 1..=appends {
        buffer.record_event(&RawSseEvent { event: None, raw: vec![format!("data: {i}")] }, Some(&model()));
    }
    let snapshot = buffer.snapshot();
    assert_eq!(snapshot.records.len(), 1_000);
    assert_eq!(snapshot.dropped_records, 5);
    assert_eq!(snapshot.total_events, appends);
    assert_eq!(snapshot.records[0].sequence(), 6);
    assert_eq!(snapshot.records.last().unwrap().sequence(), 1_005);
    let text = buffer.to_raw_text();
    assert!(text.starts_with(": omp-debug-dropped records=5"));
}
