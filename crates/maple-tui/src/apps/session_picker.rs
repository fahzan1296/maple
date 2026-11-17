//! Standalone TUI session picker.
//!
//! Reference: `l/src/apps/session-picker.ts`.
//!
//! Deviations:
//! - The TS implementation depends on `@oh-my-pi/pi-utils/logger`, the
//!   `SessionSelectorComponent` overlay, and `runStandaloneTui` (async TUI
//!   lifecycle with overlays/fullscreen). None exist in this crate.
//! - Async host methods and [`select_session`] are expressed with [`Future`]
//!   return types and host/capability traits; actual TUI rendering and history
//!   ranking are host responsibilities. [`rank_sessions`] provides a
//!   deterministic, pure fallback ordering (pinned first, then original order,
//!   with history-query promotion when provided) so behavior is testable.
//! - Interactive selection is injected via [`SessionPickerUi`] because the TUI
//!   selector component is not ported; implementations may be interactive or
//!   scripted tests.
//! - `SessionHistoryMatcher` is modeled as `Box<dyn Fn(&str) -> Vec<String>>`.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

/// Boxed future used by async host operations.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Minimal session entry fields required by the picker.
pub trait SessionSelectorEntry: Clone {
	fn id(&self) -> &str;
}

/// History matcher: query -> matching session ids (ranked).
pub type SessionHistoryMatcher = Box<dyn Fn(&str) -> Vec<String>>;

/// Session listing payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionListing<T> {
	pub sessions: Vec<T>,
}

/// Persistence and history capabilities supplied by the session-owning host.
pub trait SessionPickerHost<T: SessionSelectorEntry> {
	fn load_pinned_ids(&self) -> BoxFuture<'_, Option<HashSet<String>>> {
		Box::pin(async { None })
	}
	fn load_history_matcher(&self) -> Option<SessionHistoryMatcher> {
		None
	}
	fn delete_session(&self, _session: &T) -> BoxFuture<'_, bool> {
		Box::pin(async { false })
	}
	/// Rename the session and resolve with the title as stored, or `None`.
	fn rename_session(&self, _session: &T, _title: &str) -> BoxFuture<'_, Option<String>> {
		Box::pin(async { None })
	}
	fn set_session_pinned(&self, _session: &T, _pinned: bool) -> BoxFuture<'_, ()> {
		Box::pin(async {})
	}
	fn load_all_sessions(&self) -> BoxFuture<'_, Option<SessionListing<T>>> {
		Box::pin(async { None })
	}
}

/// Presentation and capability controls for the standalone session picker.
#[derive(Clone, Debug, Default)]
pub struct SessionPickerOptions {
	pub title: Option<String>,
	/// `None` means default scope label; `Some(None)` in TS disables it.
	pub scope_label: Option<Option<String>>,
	pub show_cwd: bool,
	pub allow_delete: bool,
	pub allow_rename: bool,
	pub allow_global_scope: bool,
	pub history_search: bool,
	pub pinned_ids: Option<HashSet<String>>,
}

impl SessionPickerOptions {
	pub fn defaults() -> Self {
		Self {
			title: None,
			scope_label: None,
			show_cwd: false,
			allow_delete: true,
			allow_rename: true,
			allow_global_scope: true,
			history_search: true,
			pinned_ids: None,
		}
	}
}

/// UI capability for choosing a session (injected, TUI runtime unavailable).
pub trait SessionPickerUi<T: SessionSelectorEntry> {
	/// Return chosen session id, or `None` if cancelled.
	fn choose_session(
		&mut self,
		sessions: &[T],
		options: &SessionPickerOptions,
		ranked_ids: &[String],
	) -> BoxFuture<'_, Option<String>>;
}

/// Rank sessions: pinned ids first (stable among themselves by input order),
/// then remaining in input order. A `history_matcher` query is accepted for
/// API parity; when provided it promotes matched ids in that order.
pub fn rank_sessions<T: SessionSelectorEntry>(
	sessions: &[T],
	pinned_ids: Option<&HashSet<String>>,
	history_matcher: Option<&SessionHistoryMatcher>,
	query: Option<&str>,
) -> Vec<T> {
	let mut promoted: Vec<String> = Vec::new();
	if let (Some(m), Some(q)) = (history_matcher, query) {
		if !q.is_empty() {
			promoted = m(q);
		}
	}
	let mut ordered = Vec::with_capacity(sessions.len());
	let mut used: HashSet<String> = HashSet::new();
	let mut push = |pred: &dyn Fn(&T) -> bool, ordered: &mut Vec<T>, used: &mut HashSet<String>| {
		for s in sessions {
			let id = s.id().to_string();
			if used.contains(&id) {
				continue;
			}
			if pred(s) {
				used.insert(id);
				ordered.push(s.clone());
			}
		}
	};
	if !promoted.is_empty() {
		for pid in promoted {
			push(&|s| s.id() == pid, &mut ordered, &mut used);
		}
	}
	if let Some(pinned) = pinned_ids {
		push(&|s| pinned.contains(s.id()), &mut ordered, &mut used);
	}
	push(&|_| true, &mut ordered, &mut used);
	ordered
}

pub use select_session as selectSession;

/// Show the TUI session selector and return the selected session, or `None`
/// if cancelled.
///
/// Requires an injected UI implementation because the terminal/TUI selector is
/// not available in this crate. Host capabilities (pinned ids, history matcher)
/// are consulted as in the reference flow.
pub async fn select_session<T: SessionSelectorEntry, H: SessionPickerHost<T>, U: SessionPickerUi<T>>(
	sessions: Vec<T>,
	mut options: SessionPickerOptions,
	host: &H,
	ui: &mut U,
) -> Option<T> {
	if options.pinned_ids.is_none() {
		if let Some(ids) = host.load_pinned_ids().await {
			options.pinned_ids = Some(ids);
		}
	}
	let history_matcher = if options.history_search {
		host.load_history_matcher()
	} else {
		None
	};
	let ranked = rank_sessions(&sessions, options.pinned_ids.as_ref(), history_matcher.as_ref(), None);
	let ranked_ids: Vec<String> = ranked.iter().map(|s| s.id().to_string()).collect();
	let chosen = ui.choose_session(&ranked, &options, &ranked_ids).await?;
	ranked.into_iter().find(|s| s.id() == chosen)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[derive(Clone, Debug, PartialEq, Eq)]
	struct S(String);
	impl SessionSelectorEntry for S {
		fn id(&self) -> &str { &self.0 }
	}

	struct H {
		pinned: Option<HashSet<String>>,
	}
	impl SessionPickerHost<S> for H {
		fn load_pinned_ids(&self) -> BoxFuture<'_, Option<HashSet<String>>> {
			let p = self.pinned.clone();
			Box::pin(async move { p })
		}
		fn load_history_matcher(&self) -> Option<SessionHistoryMatcher> {
			Some(Box::new(|q: &str| {
				if q == "hit" { vec!["b".into()] } else { vec![] }
			}))
		}
	}

	struct Ui {
		pick: Option<String>,
		seen_ranked: Vec<String>,
		seen_title: Option<String>,
	}
	impl SessionPickerUi<S> for Ui {
		fn choose_session(
			&mut self,
			_sessions: &[S],
			options: &SessionPickerOptions,
			ranked_ids: &[String],
		) -> BoxFuture<'_, Option<String>> {
			self.seen_ranked = ranked_ids.to_vec();
			self.seen_title = options.title.clone();
			let p = self.pick.clone();
			Box::pin(async move { p })
		}
	}

	#[test]
	fn rank_pinned_first() {
		let sess = vec![S("a".into()), S("b".into()), S("c".into())];
		let mut pinned = HashSet::new();
		pinned.insert("c".into());
		let ranked = rank_sessions(&sess, Some(&pinned), None, None);
		assert_eq!(ranked.iter().map(|s| s.id()).collect::<Vec<_>>(), vec!["c", "a", "b"]);
	}

	#[test]
	fn rank_history_promotes() {
		let sess = vec![S("a".into()), S("b".into())];
		let matcher: SessionHistoryMatcher = Box::new(|_: &str| vec!["b".into()]);
		let ranked = rank_sessions(&sess, None, Some(&matcher), Some("x"));
		assert_eq!(ranked[0].id(), "b");
	}

	#[test]
	fn options_defaults() {
		let o = SessionPickerOptions::defaults();
		assert!(o.allow_delete && o.allow_global_scope && o.history_search);
	}

	#[test]
	fn select_session_uses_host_pinned_and_ui_choice() {
		let rt = futures_lite_future();
		let mut pinned = HashSet::new();
		pinned.insert("b".into());
		let host = H { pinned: Some(pinned) };
		let mut ui = Ui { pick: Some("b".into()), seen_ranked: vec![], seen_title: None };
		let mut opts = SessionPickerOptions::defaults();
		opts.title = Some("Pick".into());
		let sess = vec![S("a".into()), S("b".into())];
		let got = rt(sess, opts, host, ui);
		assert_eq!(got.as_ref().map(|s| s.id()), Some("b"));
	}

	fn futures_lite_future() -> impl Fn(Vec<S>, SessionPickerOptions, H, Ui) -> Option<S> {
		|sess, opts, host, mut ui| {
			// minimal single-threaded poll for test
			use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
			fn noop_raw() -> RawWaker {
				fn no(_: *const ()) {}
				fn clone(_: *const ()) -> RawWaker { noop_raw() }
				const V: RawWakerVTable = RawWakerVTable::new(clone, no, no, no);
				RawWaker::new(std::ptr::null(), &V)
			}
			let waker = unsafe { Waker::from_raw(noop_raw()) };
			let mut cx = Context::from_waker(&waker);
			let mut fut = Box::pin(select_session(sess, opts, &host, &mut ui));
			loop {
				match fut.as_mut().poll(&mut cx) {
					Poll::Ready(v) => break v,
					Poll::Pending => continue,
				}
			}
		}
	}

	#[test]
	fn select_session_cancel_returns_none() {
		let rt = futures_lite_future();
		let host = H { pinned: None };
		let ui = Ui { pick: None, seen_ranked: vec![], seen_title: None };
		let sess = vec![S("a".into())];
		assert!(rt(sess, SessionPickerOptions::defaults(), host, ui).is_none());
	}
}
