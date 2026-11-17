//! Port of `l/test/chat-block.test.ts`.
//!
//! The reference subclasses `ChatBlock` to observe `onMount` and uses
//! `addChild`; the crate's standalone `ChatBlock` has neither. Mount idempotence
//! is observed through host ownership (a second mount must not drop the first
//! host), and the child-propagation test is omitted.

use std::sync::{
	atomic::{AtomicUsize, Ordering},
	Arc,
};

use maple_tui::chrome::chat_block::{ChatBlock, ChatBlockHost};

struct Host {
	renders: Arc<AtomicUsize>,
	drops: Arc<AtomicUsize>,
}

impl ChatBlockHost for Host {
	fn request_render(&mut self) {
		self.renders.fetch_add(1, Ordering::SeqCst);
	}
}

impl Drop for Host {
	fn drop(&mut self) {
		self.drops.fetch_add(1, Ordering::SeqCst);
	}
}

fn host(renders: &Arc<AtomicUsize>, drops: &Arc<AtomicUsize>) -> Host {
	Host {
		renders: Arc::clone(renders),
		drops: Arc::clone(drops),
	}
}

#[test]
fn mounts_once_and_a_second_mount_is_a_no_op() {
	let renders = Arc::new(AtomicUsize::new(0));
	let drops = Arc::new(AtomicUsize::new(0));
	let mut block = ChatBlock::new();
	assert!(block.is_transcript_block_finalized());
	assert!(!block.active());

	block.mount(host(&renders, &drops));
	block.mount(host(&renders, &drops));

	assert!(block.active());
	assert!(!block.is_transcript_block_finalized());
	// The second (temporary) host is dropped, the mounted one is retained.
	assert_eq!(drops.load(Ordering::SeqCst), 1);
	drop(block);
	assert_eq!(drops.load(Ordering::SeqCst), 2);
	assert_eq!(renders.load(Ordering::SeqCst), 0);
}

#[test]
fn is_finalized_until_mounted_live_while_active_and_finalized_after_finish() {
	let renders = Arc::new(AtomicUsize::new(0));
	let drops = Arc::new(AtomicUsize::new(0));
	let mut block = ChatBlock::new();
	assert!(block.is_transcript_block_finalized());
	block.mount(host(&renders, &drops));
	assert!(!block.is_transcript_block_finalized());
	block.finish();
	assert!(block.is_transcript_block_finalized());
	assert!(!block.active());
}

#[test]
fn finish_runs_cleanups_once() {
	let renders = Arc::new(AtomicUsize::new(0));
	let drops = Arc::new(AtomicUsize::new(0));
	let count = Arc::new(AtomicUsize::new(0));
	let mut block = ChatBlock::new();
	block.mount(host(&renders, &drops));
	block.on_cleanup({
		let count = Arc::clone(&count);
		move || {
			count.fetch_add(1, Ordering::SeqCst);
		}
	});
	block.finish();
	assert_eq!(count.load(Ordering::SeqCst), 1);
	block.finish();
	assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn dispose_runs_cleanups_once_is_idempotent_and_finalizes() {
	let renders = Arc::new(AtomicUsize::new(0));
	let drops = Arc::new(AtomicUsize::new(0));
	let count = Arc::new(AtomicUsize::new(0));
	let mut block = ChatBlock::new();
	block.mount(host(&renders, &drops));
	block.on_cleanup({
		let count = Arc::clone(&count);
		move || {
			count.fetch_add(1, Ordering::SeqCst);
		}
	});
	block.dispose_block();
	assert_eq!(count.load(Ordering::SeqCst), 1);
	assert!(block.is_transcript_block_finalized());
	block.dispose_block();
	assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn finish_then_dispose_does_not_double_run_cleanups() {
	let renders = Arc::new(AtomicUsize::new(0));
	let drops = Arc::new(AtomicUsize::new(0));
	let count = Arc::new(AtomicUsize::new(0));
	let mut block = ChatBlock::new();
	block.mount(host(&renders, &drops));
	block.on_cleanup({
		let count = Arc::clone(&count);
		move || {
			count.fetch_add(1, Ordering::SeqCst);
		}
	});
	block.finish();
	block.dispose_block();
	assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn request_render_routes_to_the_host_only_between_mount_and_dispose() {
	let renders = Arc::new(AtomicUsize::new(0));
	let drops = Arc::new(AtomicUsize::new(0));
	let mut block = ChatBlock::new();
	block.request_render();
	assert_eq!(renders.load(Ordering::SeqCst), 0);
	block.mount(host(&renders, &drops));
	block.request_render();
	assert_eq!(renders.load(Ordering::SeqCst), 1);
	block.dispose_block();
	block.request_render();
	assert_eq!(renders.load(Ordering::SeqCst), 1);
}

#[test]
fn on_cleanup_registered_after_dispose_runs_immediately() {
	let renders = Arc::new(AtomicUsize::new(0));
	let drops = Arc::new(AtomicUsize::new(0));
	let ran = Arc::new(AtomicUsize::new(0));
	let mut block = ChatBlock::new();
	block.mount(host(&renders, &drops));
	block.dispose_block();
	block.on_cleanup({
		let ran = Arc::clone(&ran);
		move || {
			ran.fetch_add(1, Ordering::SeqCst);
		}
	});
	assert_eq!(ran.load(Ordering::SeqCst), 1);
}
