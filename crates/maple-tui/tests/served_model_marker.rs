// Ported subset of l/test/served-model-marker.test.ts to Rust served_model_marker APIs.
// Deviations: AssistantMessage/Usage shapes differ (no api/cost/premiumRequests fields);
// tracker/check and mismatch fields/labels follow Rust implementation semantics.

use maple_tui::chat::messages::{AssistantMessage, StopReason, Usage};
use maple_tui::chat::served_model_marker::{
    detect_served_model_mismatch, ServedModelMismatch, ServedModelTracker,
};

fn turn(model: &str, served: Option<&str>, provider: &str, upstream_provider: Option<&str>) -> AssistantMessage {
    let mut msg = AssistantMessage::default();
    msg.model = model.to_string();
    msg.provider = provider.to_string();
    msg.api = "anthropic-messages".to_string();
    msg.upstream_model = served.map(str::to_string);
    msg.upstream_provider = upstream_provider.map(str::to_string);
    msg.stop_reason = StopReason::Stop;
    msg.usage = Usage::default();
    msg
}

#[test]
fn flags_a_different_family_served_under_the_requested_id_naming_the_route() {
    assert_eq!(
        detect_served_model_mismatch(&turn(
            "anthropic/claude-opus-5",
            Some("claude-haiku-4-5-20251001"),
            "openrouter",
            Some("Amazon Bedrock"),
        )),
        Some(ServedModelMismatch {
            requested: "anthropic/claude-opus-5".to_string(),
            served: "claude-haiku-4-5-20251001".to_string(),
            provider: "openrouter".to_string(),
            upstream_provider: Some("Amazon Bedrock".to_string()),
        })
    );
}

#[test]
fn flags_a_different_revision_of_the_same_family() {
    let mismatch = detect_served_model_mismatch(&turn("claude-opus-4-6", Some("claude-opus-4-7"), "openrouter", None));
    assert_eq!(mismatch.map(|m| m.served), Some("claude-opus-4-7".to_string()));
}

#[test]
fn treats_dated_snapshot_or_gateway_prefix_of_requested_model_as_same() {
    assert!(detect_served_model_mismatch(&turn(
        "anthropic/claude-haiku-4.5",
        Some("claude-haiku-4-5-20251001"),
        "openrouter",
        None,
    ))
    .is_none());
}

#[test]
fn treats_unclassifiable_served_id_as_unverifiable_not_substitution() {
    assert!(detect_served_model_mismatch(&turn(
        "claude-opus-4-6",
        Some("numbat-v6-efforts-20-40-80-ab-prod"),
        "anthropic",
        None,
    ))
    .is_none());
}

#[test]
fn stays_silent_when_no_served_id_was_recovered() {
    assert!(detect_served_model_mismatch(&turn("claude-fable-5-1", None, "openrouter", None)).is_none());
}

#[test]
fn tracker_reports_each_substitution_pair_once_and_after_model_switch() {
    let mut tracker = ServedModelTracker::new();
    let swapped = turn("claude-opus-5", Some("claude-haiku-4-5"), "openrouter", None);
    assert!(tracker.check(&swapped).is_some());
    assert!(tracker.check(&swapped).is_none());
    assert!(tracker.check(&turn("claude-opus-5", Some("claude-opus-5"), "openrouter", None)).is_none());
    assert_eq!(
        tracker
            .check(&turn("claude-sonnet-5", Some("claude-haiku-4-5"), "openrouter", None))
            .map(|m| m.requested),
        Some("claude-sonnet-5".to_string())
    );
    assert!(tracker.check(&swapped).is_none());
}

#[test]
fn mismatch_helpers_format_via_and_labels() {
    let mismatch = ServedModelMismatch {
        requested: "a".to_string(),
        served: "b".to_string(),
        provider: "p".to_string(),
        upstream_provider: Some("up".to_string()),
    };
    assert_eq!(mismatch.via(), "p/up");
    assert!(mismatch.native_label().contains("Served b"));
    assert!(mismatch.native_label().contains("requested a"));
    assert!(mismatch.native_label().contains("via p/up"));
}
