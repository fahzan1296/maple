//! Lifecycle-aware transcript block primitive.
//!
//! Reference: `l/src/chrome/chat-block.ts`.
//!
//! Deviations:
//! - The reference extends a `Container` base class in the TUI component tree
//!   and relies on host mounting/disposal conventions. The crate's component
//!   hierarchy differs, so this port provides a standalone [`ChatBlock`] base
//!   with the same lifecycle contract (mount/cleanup/finish/dispose,
//!   active/finalized semantics) that concrete blocks can embed or wrap.
//! - `ChatBlockHost` is a trait so hosts can supply `request_render()` without
//!   depending on a specific TUI type.
//! - ANSI rendering is delegated via [`ChatBlock::render_rows`] (default empty)
//!   rather than inheriting from `Container`.

use crate::component::Component;

/// Capabilities a mounted [`ChatBlock`] may use against its host transcript.
pub trait ChatBlockHost {
	/// Schedule a repaint of the transcript.
	fn request_render(&mut self);
}

/// Lifecycle-aware transcript block base.
///
/// While mounted and unfinished a block reports finalized = `false`; after
/// `finish()`/`dispose()` it reports `true` and freezes at its final content.
pub struct ChatBlock {
	host: Option<Box<dyn ChatBlockHost + Send>>,
	cleanups: Vec<Box<dyn FnOnce() + Send>>,
	active: bool,
	disposed: bool,
}

impl Default for ChatBlock {
	fn default() -> Self {
		Self::new()
	}
}

impl ChatBlock {
	/// Create an inactive, undisposed block.
	pub fn new() -> Self {
		ChatBlock {
			host: None,
			cleanups: Vec::new(),
			active: false,
			disposed: false,
		}
	}

	/// Run setup after the block is in the transcript. Default: no-op.
	pub fn on_mount(&mut self) {}

	/// Register a teardown to run on [`Self::finish`]/[`Self::dispose_block`].
	///
	/// If already disposed, the cleanup runs immediately.
	pub fn on_cleanup<F>(&mut self, cleanup: F)
	where
		F: FnOnce() + Send + 'static,
	{
		if self.disposed {
			cleanup();
		} else {
			self.cleanups.push(Box::new(cleanup));
		}
	}

	/// Ask the host to repaint. No-op before mount or after dispose.
	pub fn request_render(&mut self) {
		if let Some(host) = self.host.as_mut() {
			host.request_render();
		}
	}

	/// True between [`Self::mount`] and [`Self::finish`]/[`Self::dispose_block`].
	pub fn active(&self) -> bool {
		self.active
	}

	/// Host-only: attach the host and run [`Self::on_mount`]. Idempotent.
	pub fn mount<H>(&mut self, host: H)
	where
		H: ChatBlockHost + Send + 'static,
	{
		if self.active || self.disposed {
			return;
		}
		self.host = Some(Box::new(host));
		self.active = true;
		self.on_mount();
	}

	/// Self-complete: stop ongoing effects and freeze at current content.
	pub fn finish(&mut self) {
		if !self.active {
			return;
		}
		self.active = false;
		self.run_cleanups();
	}

	/// Host-only teardown: release everything. Idempotent.
	pub fn dispose_block(&mut self) {
		if self.disposed {
			return;
		}
		self.disposed = true;
		self.active = false;
		self.host = None;
		self.run_cleanups();
	}

	/// Live blocks stay repaintable; finished/disposed ones may freeze.
	pub fn is_transcript_block_finalized(&self) -> bool {
		!self.active
	}

	/// ANSI rows for this block. Default empty.
	pub fn render_rows(&mut self, _width: u16) -> Vec<String> {
		Vec::new()
	}

	fn run_cleanups(&mut self) {
		for cleanup in self.cleanups.drain(..) {
			cleanup();
		}
	}
}

impl Component for ChatBlock {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.render_rows(width)
	}

	fn dispose(&mut self) {
		self.dispose_block();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	};

	#[derive(Default)]
	struct Host(Arc<AtomicUsize>);
	impl ChatBlockHost for Host {
		fn request_render(&mut self) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	#[test]
	fn mount_is_idempotent_and_tracks_active() {
		let renders = Arc::new(AtomicUsize::new(0));
		let mut block = ChatBlock::new();
		assert!(!block.active());
		assert!(block.is_transcript_block_finalized());
		block.mount(Host(Arc::clone(&renders)));
		assert!(block.active());
		assert!(!block.is_transcript_block_finalized());
		block.request_render();
		assert_eq!(renders.load(Ordering::SeqCst), 1);
		block.mount(Host(Arc::new(AtomicUsize::new(0))));
		assert!(block.active());
	}

	#[test]
	fn finish_runs_cleanups_once_and_finalizes() {
		let mut block = ChatBlock::new();
		block.mount(Host::default());
		let hits = Arc::new(AtomicUsize::new(0));
		let h = Arc::clone(&hits);
		block.on_cleanup(move || {
			h.fetch_add(1, Ordering::SeqCst);
		});
		block.finish();
		assert!(!block.active());
		assert!(block.is_transcript_block_finalized());
		assert_eq!(hits.load(Ordering::SeqCst), 1);
		block.finish();
		assert_eq!(hits.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn dispose_is_idempotent_and_runs_cleanup() {
		let mut block = ChatBlock::new();
		block.mount(Host::default());
		let hits = Arc::new(AtomicUsize::new(0));
		let h = Arc::clone(&hits);
		block.on_cleanup(move || {
			h.fetch_add(1, Ordering::SeqCst);
		});
		block.dispose_block();
		assert!(block.is_transcript_block_finalized());
		assert_eq!(hits.load(Ordering::SeqCst), 1);
		block.dispose_block();
		assert_eq!(hits.load(Ordering::SeqCst), 1);
	}

	#[test]
	fn cleanup_runs_immediately_when_disposed() {
		let mut block = ChatBlock::new();
		block.mount(Host::default());
		block.dispose_block();
		let hits = Arc::new(AtomicUsize::new(0));
		let h = Arc::clone(&hits);
		block.on_cleanup(move || {
			h.fetch_add(1, Ordering::SeqCst);
		});
		assert_eq!(hits.load(Ordering::SeqCst), 1);
	}
}
