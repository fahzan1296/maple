//! Port of the `ImageBudget` block of `l/test/image-budget.test.ts`.
//!
//! The budget accounting is pure (no terminal), so the cap/retry/demotion,
//! stable-id, key-eviction, `set_cap` and stable-pass contracts port directly.
//! The TUI integration, kitty wire-format and passthrough blocks stay unported.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use maple_tui::components::image::ImageBudget;

fn budget(cap: u32) -> (ImageBudget, Arc<AtomicUsize>) {
	let renders = Arc::new(AtomicUsize::new(0));
	let counter = renders.clone();
	let request_render = Arc::new(move || {
		counter.fetch_add(1, Ordering::SeqCst);
	});
	(ImageBudget::new(cap, request_render), renders)
}

struct Pass {
	suppressed: Vec<bool>,
	retry: bool,
	purge: Vec<u32>,
}

/// One render pass observing ids `1..=count` in display order.
fn pass(budget: &mut ImageBudget, count: u32) -> Pass {
	budget.begin_pass(false, false);
	let suppressed = (1..=count).map(|id| budget.observe(id)).collect();
	let retry = budget.end_pass();
	let purge = budget.take_purge_ids();
	Pass {
		suppressed,
		retry,
		purge,
	}
}

#[test]
fn keeps_every_image_live_while_at_or_under_the_cap() {
	let (mut budget, _) = budget(3);
	let first = pass(&mut budget, 2);
	assert_eq!(first.suppressed, vec![false, false]);
	assert!(!first.retry);

	let second = pass(&mut budget, 3);
	assert_eq!(second.suppressed, vec![false, false, false]);
	assert!(!second.retry);
	assert!(second.purge.is_empty());
}

#[test]
fn retries_an_over_cap_pass_then_demotes_and_purges_the_oldest_id() {
	let (mut budget, renders) = budget(2);

	assert_eq!(pass(&mut budget, 2).suppressed, vec![false, false]);

	let overflow = pass(&mut budget, 3);
	assert_eq!(overflow.suppressed, vec![false, false, false]);
	assert!(overflow.retry);
	assert_eq!(renders.load(Ordering::SeqCst), 1);

	let demote = pass(&mut budget, 3);
	assert_eq!(demote.suppressed, vec![true, false, false]);
	assert!(!demote.retry);
	assert_eq!(demote.purge, vec![1]);

	let steady = pass(&mut budget, 3);
	assert_eq!(steady.suppressed, vec![true, false, false]);
	assert!(!steady.retry);
	assert!(steady.purge.is_empty());
}

#[test]
fn keeps_exactly_cap_images_live_as_more_arrive() {
	let (mut budget, _) = budget(2);
	for count in 3..=5 {
		pass(&mut budget, count);
		pass(&mut budget, count);
	}
	let settled = pass(&mut budget, 5);
	assert_eq!(settled.suppressed, vec![true, true, true, false, false]);
}

#[test]
fn treats_cap_zero_as_unlimited() {
	let (mut budget, renders) = budget(0);
	assert!(!budget.enabled());
	let result = pass(&mut budget, 6);
	assert_eq!(result.suppressed, vec![false; 6]);
	assert!(!result.retry);
	assert!(result.purge.is_empty());
	assert_eq!(renders.load(Ordering::SeqCst), 0);
}

#[test]
fn hands_back_a_stable_id_per_key_and_fresh_ids_without_one() {
	let (mut budget, _) = budget(3);
	let a1 = budget.acquire_id(Some("tool:0"));
	let a2 = budget.acquire_id(Some("tool:0"));
	let b = budget.acquire_id(Some("tool:1"));
	assert_eq!(a1, a2);
	assert_ne!(b, a1);
	assert_ne!(budget.acquire_id(None), budget.acquire_id(None));

	let (mut first, _) = self::budget(3);
	let (mut second, _) = self::budget(3);
	assert_ne!(first.acquire_id(None), second.acquire_id(None));
}

#[test]
fn evicts_demoted_ids_from_the_key_map() {
	let (mut budget, _) = budget(2);
	let id1 = budget.acquire_id(Some("keyA"));
	let id2 = budget.acquire_id(Some("keyB"));
	let id3 = budget.acquire_id(Some("keyC"));

	budget.begin_pass(false, false);
	budget.observe(id1);
	budget.observe(id2);
	budget.end_pass();
	assert_eq!(budget.acquire_id(Some("keyA")), id1);
	assert_eq!(budget.acquire_id(Some("keyB")), id2);

	budget.begin_pass(false, false);
	budget.observe(id1);
	budget.observe(id2);
	budget.observe(id3);
	budget.end_pass();
	assert_eq!(budget.acquire_id(Some("keyA")), id1);

	budget.begin_pass(false, false);
	budget.observe(id1);
	budget.observe(id2);
	budget.observe(id3);
	assert!(!budget.end_pass());
	assert_ne!(budget.acquire_id(Some("keyA")), id1);
	assert_eq!(budget.acquire_id(Some("keyB")), id2);
	assert_eq!(budget.acquire_id(Some("keyC")), id3);
}

#[test]
fn set_cap_zero_clears_a_previously_applied_demotion_threshold() {
	let (mut budget, _) = budget(2);
	pass(&mut budget, 3);
	assert_eq!(pass(&mut budget, 3).suppressed, vec![true, false, false]);

	budget.set_cap(0);
	let result = pass(&mut budget, 3);
	assert_eq!(result.suppressed, vec![false, false, false]);
}

#[test]
fn stable_pass_replays_the_committed_split_by_id() {
	let (mut budget, _) = budget(2);
	pass(&mut budget, 4);
	pass(&mut budget, 4);
	assert_eq!(pass(&mut budget, 4).suppressed, vec![true, true, false, false]);

	budget.begin_pass(true, false);
	assert!(!budget.observe(4));
	assert!(!budget.observe(3));
	assert!(budget.observe(2));
	assert!(budget.observe(1));
	assert!(!budget.observe(99));
	budget.end_pass();

	let after = pass(&mut budget, 4);
	assert_eq!(after.suppressed, vec![true, true, false, false]);
	assert!(!after.retry);
	assert!(after.purge.is_empty());
}
