//! Ring buffer for Emacs-style kill/yank operations.
//!
//! Reference: `l/src/kill-ring.ts`.

/// Maximum number of entries kept; the oldest is dropped beyond this.
pub const MAX_ENTRIES: usize = 60;

/// How [`KillRing::push`] merges text with the most recent entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushOptions {
    /// Prepend when accumulating (backward deletion) or append (forward).
    pub prepend: bool,
    /// Merge with the most recent entry instead of creating a new one.
    pub accumulate: bool,
}

impl PushOptions {
    /// Options for a fresh entry.
    pub fn new(prepend: bool) -> Self {
        Self {
            prepend,
            accumulate: false,
        }
    }

    /// Mark this push as accumulating with the previous entry.
    pub fn accumulating(mut self) -> Self {
        self.accumulate = true;
        self
    }
}

/// Tracks killed (deleted) text entries. Consecutive kills can accumulate into
/// a single entry; yank takes the most recent and yank-pop cycles older ones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KillRing {
    ring: Vec<String>,
}

impl KillRing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add text to the kill ring.
    pub fn push(&mut self, text: &str, opts: PushOptions) {
        if text.is_empty() {
            return;
        }
        if opts.accumulate && !self.ring.is_empty() {
            let last = self.ring.pop().unwrap_or_default();
            if opts.prepend {
                self.ring.push(format!("{text}{last}"));
            } else {
                self.ring.push(format!("{last}{text}"));
            }
        } else {
            self.ring.push(text.to_string());
            if self.ring.len() > MAX_ENTRIES {
                self.ring.remove(0);
            }
        }
    }

    /// Most recent entry without modifying the ring.
    pub fn peek(&self) -> Option<&str> {
        self.ring.last().map(String::as_str)
    }

    /// Move the last entry to the front (yank-pop cycling).
    pub fn rotate(&mut self) {
        if self.ring.len() > 1 {
            if let Some(last) = self.ring.pop() {
                self.ring.insert(0, last);
            }
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// True when the ring holds no entries.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_ignored() {
        let mut ring = KillRing::new();
        ring.push("", PushOptions::new(false));
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.peek(), None);
    }

    #[test]
    fn accumulate_appends_or_prepends() {
        let mut ring = KillRing::new();
        ring.push("ab", PushOptions::new(false).accumulating());
        ring.push("cd", PushOptions::new(false).accumulating());
        assert_eq!(ring.peek(), Some("abcd"));

        let mut ring = KillRing::new();
        ring.push("ab", PushOptions::new(false).accumulating());
        ring.push("cd", PushOptions::new(true).accumulating());
        assert_eq!(ring.peek(), Some("cdab"));
    }

    #[test]
    fn non_accumulating_pushes_stack_entries() {
        let mut ring = KillRing::new();
        ring.push("first", PushOptions::new(false));
        ring.push("second", PushOptions::new(false));
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.peek(), Some("second"));
    }

    #[test]
    fn rotate_cycles_only_with_multiple_entries() {
        let mut ring = KillRing::new();
        ring.push("only", PushOptions::new(false));
        ring.rotate();
        assert_eq!(ring.peek(), Some("only"));

        ring.push("next", PushOptions::new(false));
        ring.rotate();
        assert_eq!(ring.peek(), Some("only"));
        ring.rotate();
        assert_eq!(ring.peek(), Some("next"));
    }

    #[test]
    fn drops_oldest_beyond_max_entries() {
        let mut ring = KillRing::new();
        for index in 0..MAX_ENTRIES + 1 {
            ring.push(&index.to_string(), PushOptions::new(false));
        }
        assert_eq!(ring.len(), MAX_ENTRIES);
        assert_eq!(ring.peek(), Some("60"));
        assert_eq!(ring.ring[0], "1");
    }
}
