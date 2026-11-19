//! Memoization for `describe()` output.
//! Reference: `l/src/native/memo.ts`.
//!
//! An unchanged component must hand the reconciler the same node objects so it
//! can skip the subtree. [`Memo`] is the primitive; [`OwnerMemo`] keys one memo
//! per owner id for stateless renderers that re-describe the same args/result.

use std::collections::HashMap;

use serde_json::Value;

pub use super::state::{same_items, same_props, Memo};

/// One [`Memo`] per owner id, for renderers shared by several call sites.
///
/// The TS original stores the memo in a symbol slot on the owner object. Rust
/// components pass a stable owner id instead; call [`OwnerMemo::forget`] when
/// that owner is dropped so the entry does not outlive it.
#[derive(Debug)]
pub struct OwnerMemo<T> {
    memos: HashMap<usize, Memo<T>>,
}

impl<T> OwnerMemo<T> {
    /// An owner memo with no cached values.
    pub fn new() -> Self {
        OwnerMemo {
            memos: HashMap::new(),
        }
    }

    /// The cached value for `owner`, rebuilt when `deps` changed.
    pub fn get<F: FnOnce() -> T>(&mut self, owner: usize, deps: &[Value], build: F) -> &T {
        self.memos.entry(owner).or_insert_with(Memo::new).get(deps, build)
    }

    /// Drop one owner's cached value.
    pub fn forget(&mut self, owner: usize) {
        self.memos.remove(&owner);
    }

    /// Drop every cached value (theme, glyph preset or keybindings changed).
    pub fn clear(&mut self) {
        self.memos.clear();
    }

    /// Whether `owner` has a cached value.
    pub fn contains(&self, owner: usize) -> bool {
        self.memos.contains_key(&owner)
    }
}

impl<T> Default for OwnerMemo<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn caches_per_owner_until_deps_change() {
        let mut memo = OwnerMemo::new();
        let builds = Cell::new(0);
        let first = *memo.get(1, &[Value::from(1)], || {
            builds.set(builds.get() + 1);
            10
        });
        let again = *memo.get(1, &[Value::from(1)], || {
            builds.set(builds.get() + 1);
            20
        });
        let other = *memo.get(2, &[Value::from(1)], || {
            builds.set(builds.get() + 1);
            30
        });
        assert_eq!((first, again, other), (10, 10, 30));
        assert_eq!(builds.get(), 2);
    }

    #[test]
    fn forget_drops_only_the_named_owner() {
        let mut memo = OwnerMemo::new();
        memo.get(1, &[Value::from(1)], || 1);
        memo.get(2, &[Value::from(1)], || 2);
        memo.forget(1);
        assert!(!memo.contains(1));
        assert!(memo.contains(2));
        memo.clear();
        assert!(!memo.contains(2));
    }
}
