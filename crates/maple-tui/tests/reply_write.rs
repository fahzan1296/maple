// Ported subset of l/test/reply-write.test.ts to Rust reply_write APIs.
// Deviations: ReplyWriteComponent takes ReplyWriteOptions (no TUI link/image budget plumbing),
// result/error handling and rendering are Rust-specific; TranscriptContainer/tool-visibility case
// depends on transcript wiring and is omitted.

use maple_tui::chat::messages::{MessageBlock, StopReason, TextContent};
use maple_tui::chat::reply_write::{
    is_reply_write, reply_display_message, result_text_blocks, tool_result_replies, ReplyWriteComponent,
    ReplyWriteOptions,
};
use serde_json::json;

#[test]
fn is_reply_write_detects_reply_targets_and_undecided_prefixes() {
    assert_eq!(is_reply_write("read", Some(&json!({"path": "out://x"}))), Some(false));
    assert_eq!(is_reply_write("write", None), None);
    assert_eq!(is_reply_write("write", Some(&json!("str"))), None);
    assert_eq!(is_reply_write("write", Some(&json!({}))), None);
    assert_eq!(is_reply_write("write", Some(&json!({"path": "ou"}))), None);
    assert_eq!(is_reply_write("write", Some(&json!({"path": "out:/"}))), None);
    assert_eq!(is_reply_write("write", Some(&json!({"path": "src/main.rs"}))), Some(false));
    assert_eq!(is_reply_write("write", Some(&json!({"file_path": "src/main.rs"}))), Some(false));
}

#[test]
fn tool_result_replies_collects_text_replies_in_order() {
    let details = json!({"replies": ["one", 3, "two"]});
    assert_eq!(tool_result_replies(Some(&details)), vec!["one".to_string(), "two".to_string()]);
    assert!(tool_result_replies(None).is_empty());
    assert!(tool_result_replies(Some(&json!({"replies": "x"}))).is_empty());
    assert!(tool_result_replies(Some(&json!("x"))).is_empty());
}

#[test]
fn reply_display_messages_carry_no_provider_identity() {
    let message = reply_display_message("hi", None);
    assert_eq!(message.stop_reason, StopReason::Stop);
    assert_eq!(message.api, "");
    assert_eq!(message.timestamp, 0);
    assert_eq!(message.content[0].as_text(), Some("hi"));
    let failed = reply_display_message("hi", Some("boom"));
    assert_eq!(failed.stop_reason, StopReason::Error);
    assert_eq!(failed.error_message.as_deref(), Some("boom"));
}

#[test]
fn component_streams_args_seals_and_ignores_late_updates() {
    let mut component = ReplyWriteComponent::new(ReplyWriteOptions::default());
    component.update_args(Some(&json!({"content": "part"})));
    assert_eq!(component.text(), "part");
    assert!(!component.is_transcript_block_finalized());
    component.set_args_complete();
    component.update_result(Some("failed".into()));
    assert!(component.is_transcript_block_finalized());
    assert_eq!(component.message().error_message.as_deref(), Some("failed"));
    component.update_args(Some(&json!({"content": "late"})));
    assert_eq!(component.text(), "part");
}

#[test]
fn result_text_blocks_join_text_only() {
    let content = vec![
        MessageBlock::Text(TextContent { text: "a".into(), ..TextContent::default() }),
        MessageBlock::Text(TextContent { text: "b".into(), ..TextContent::default() }),
    ];
    assert_eq!(result_text_blocks(&content), "a\nb");
}
