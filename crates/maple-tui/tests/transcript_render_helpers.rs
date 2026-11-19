// Ported from l/test/transcript-render-helpers.test.ts (assistantUsageIsBilled).

use maple_tui::chat::messages::Usage;
use maple_tui::chat::transcript_render_helpers::assistant_usage_is_billed;

fn usage(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Usage {
    Usage {
        input,
        output,
        cache_read,
        cache_write,
        total_tokens: input + output + cache_read + cache_write,
        reasoning_tokens: None,
    }
}

#[test]
fn suppresses_the_token_badge_only_for_turns_that_consumed_nothing() {
    assert!(!assistant_usage_is_billed(&usage(0, 0, 0, 0)));
}

#[test]
fn preserves_cost_transparency_for_turns_that_still_consumed_tokens() {
    assert!(assistant_usage_is_billed(&usage(321, 0, 0, 0)));
    assert!(assistant_usage_is_billed(&usage(0, 0, 512, 0)));
    assert!(assistant_usage_is_billed(&usage(0, 0, 0, 128)));
    // TS also covered premiumRequests; Rust Usage has no premium-request bucket.
}
