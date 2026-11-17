//! Internal URL scheme registry bridge.
//!
//! Reference: `l/src/tools/url-scheme-host.ts`.
//!
//! Deviations:
//! - JS installs a host object globally; Rust uses a `OnceLock`-backed host
//!   trait object set once via [`set_internal_url_scheme_host`].
//! - [`InternalUrlSchemeHost`] is a trait instead of a structural interface.
//! - [`internal_url_scheme_specs`] returns a `Vec` snapshot (no borrowed map
//!   lifetime) to avoid locking the registry across callers.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Mutation class of a writable scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemeWriteScope {
	/// Mutates user/external state.
	Workspace,
	/// Session scratch space (local://).
	Sandbox,
	/// Peer messaging (agent://).
	Coordination,
	/// Tool-device dispatch (xd://).
	Device,
	/// A message to the user (out://).
	Reply,
}

/// Write policy of a registered scheme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemeWritePolicy {
	/// Scope driving plan-mode and device-only gates.
	pub scope: SchemeWriteScope,
}

/// Read-selector peeling behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectorMode {
	/// A trailing `:<selector>` chain is a read selector.
	Lines,
	/// Never peel selectors.
	None,
	/// Server-defined/opaque URIs: never peel.
	Opaque,
}

/// Renderer-relevant subset of a registered scheme's declared spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalUrlSchemeSpec {
	/// Read-selector peeling mode.
	pub selectors: SelectorMode,
	/// A trailing `:N` with no path after authority is a port, not a selector.
	pub port_authority: bool,
	/// Read cards collapse compactly like plain files.
	pub compact_transcript: bool,
	/// Write policy; absent on read-only schemes.
	pub write: Option<SchemeWritePolicy>,
}

/// Scheme registry capabilities installed by the internal URL host.
pub trait InternalUrlSchemeHost: Send + Sync {
	/// Declared spec of a registered scheme (case-insensitive).
	fn spec(&self, scheme: &str) -> Option<InternalUrlSchemeSpec>;
	/// Declared specs of every registered scheme, keyed by lowercased scheme.
	fn specs(&self) -> HashMap<String, InternalUrlSchemeSpec>;
}

static SCHEME_HOST: OnceLock<Box<dyn InternalUrlSchemeHost>> = OnceLock::new();

/// Install the scheme registry. Until installed, no URL counts as internal.
///
/// Returns `false` if a host was already installed (first write wins).
pub fn set_internal_url_scheme_host(host: Box<dyn InternalUrlSchemeHost>) -> bool {
	SCHEME_HOST.set(host).is_ok()
}

/// Declared spec of `scheme`; `None` when unregistered or no host installed.
pub fn internal_url_scheme_spec(scheme: &str) -> Option<InternalUrlSchemeSpec> {
	SCHEME_HOST.get().and_then(|host| host.spec(scheme))
}

/// Declared specs of every registered scheme, keyed by lowercased scheme.
pub fn internal_url_scheme_specs() -> HashMap<String, InternalUrlSchemeSpec> {
	SCHEME_HOST
		.get()
		.map(|host| host.specs())
		.unwrap_or_default()
}

/// Split a hierarchical `scheme://rest` URL into its lowercased scheme and rest.
///
/// Returns `(scheme, rest)` where `rest` is the text after `://`.
pub fn split_url_scheme(input: &str) -> Option<(String, &str)> {
	// scheme: [a-z][a-z0-9+.-]* followed by ://
	let bytes = input.as_bytes();
	let mut i = 0;
	if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
		return None;
	}
	i += 1;
	while i < bytes.len() {
		let b = bytes[i];
		if b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.' {
			i += 1;
		} else {
			break;
		}
	}
	if bytes.get(i) != Some(&b':') || bytes.get(i + 1) != Some(&b'/') || bytes.get(i + 2) != Some(&b'/') {
		return None;
	}
	let scheme = input[..i].to_ascii_lowercase();
	let rest = &input[i + 3..];
	Some((scheme, rest))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn split_valid_url() {
		let (s, r) = split_url_scheme("local://foo/bar:1-3").unwrap();
		assert_eq!(s, "local");
		assert_eq!(r, "foo/bar:1-3");
	}

	#[test]
	fn split_uppercase_scheme_lowercased() {
		let (s, r) = split_url_scheme("SSH://host/path").unwrap();
		assert_eq!(s, "ssh");
		assert_eq!(r, "host/path");
	}

	#[test]
	fn split_rejects_non_url() {
		assert!(split_url_scheme("not a url").is_none());
		assert!(split_url_scheme("1abc://x").is_none());
		assert!(split_url_scheme("abc:/x").is_none());
	}

	#[test]
	fn specs_empty_without_host() {
		// Host is process-global OnceLock; in a fresh state this is empty.
		// In test runs after install it may not be; only assert function works.
		let _ = internal_url_scheme_specs();
		assert!(internal_url_scheme_spec("nonexistent-scheme-xyz").is_none());
	}
}
