//! Port of `l/test/cache-invalidation-marker.test.ts` detection section
//! (the component render section needs the global theme singleton).

use maple_tui::chat::cache_invalidation_marker::detect_cache_invalidation;
use maple_tui::chat::messages::Usage;

fn usage(parts: (u64, u64, u64, u64)) -> Usage {
	Usage {
		input: parts.0,
		output: parts.1,
		cache_read: parts.2,
		cache_write: parts.3,
		..Usage::default()
	}
}

#[test]
fn does_not_flag_the_first_turn() {
	assert_eq!(
		detect_cache_invalidation(None, &usage((2, 0, 0, 50_000))),
		None
	);
}

#[test]
fn flags_a_cache_read_collapse_after_a_warm_turn() {
	let prev = usage((99, 79, 49_837, 980));
	let current = usage((99, 99, 0, 50_900));
	assert_eq!(
		detect_cache_invalidation(Some(&prev), &current),
		Some(maple_tui::chat::cache_invalidation_marker::CacheInvalidation {
			reprocessed_tokens: 50_999,
		})
	);
}

#[test]
fn does_not_flag_a_cold_turn_after_a_write_only_predecessor() {
	let prev = usage((99, 0, 0, 50_900));
	let current = usage((16, 0, 0, 51_113));
	assert_eq!(detect_cache_invalidation(Some(&prev), &current), None);
}

#[test]
fn does_not_flag_a_turn_that_read_back_the_warm_prefix() {
	let prev = usage((0, 0, 50_900, 980));
	let current = usage((2, 0, 50_900, 3_459));
	assert_eq!(detect_cache_invalidation(Some(&prev), &current), None);
}

#[test]
fn flags_a_collapse_that_still_reads_the_shared_system_block() {
	let prev = usage((1, 0, 369_518, 1_983));
	let current = usage((3, 0, 15_282, 364_343));
	assert_eq!(
		detect_cache_invalidation(Some(&prev), &current),
		Some(maple_tui::chat::cache_invalidation_marker::CacheInvalidation {
			reprocessed_tokens: 364_346,
		})
	);
}

#[test]
fn does_not_flag_expiry_behind_a_predecessor_that_read_only_the_shared_block() {
	let prev = usage((4, 0, 15_282, 35_000));
	let current = usage((2, 0, 15_282, 36_100));
	assert_eq!(detect_cache_invalidation(Some(&prev), &current), None);
}

#[test]
fn does_not_flag_implicit_best_effort_caches_reporting_no_cache_write() {
	let prev = usage((1_069, 353, 40_789, 0));
	let current = usage((43_102, 58, 0, 0));
	assert_eq!(detect_cache_invalidation(Some(&prev), &current), None);
}

#[test]
fn ignores_collapses_when_the_prior_footprint_was_below_the_cacheable_floor() {
	let prev = usage((500, 0, 0, 0));
	assert_eq!(
		detect_cache_invalidation(Some(&prev), &usage((600, 0, 0, 0))),
		None
	);
}

#[test]
fn ignores_a_cold_turn_that_reprocessed_only_a_trivial_prompt() {
	let prev = usage((0, 0, 40_000, 1_000));
	assert_eq!(
		detect_cache_invalidation(Some(&prev), &usage((12, 0, 0, 0))),
		None
	);
}