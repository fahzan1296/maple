//! Think/scratchpad transcript rendering types.
//!
//! Reference: `l/src/tools/think.ts`.
//!
//! Deviations:
//! - The TS renderer produces TUI `Component`s and native `md` nodes via the
//!   runtime theme/markdown stack. Rust exposes the render-call args and a pure
//!   description shape; actual Markdown/component rendering is left to the host.

/// Streamed private scratchpad text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThinkRenderArgs {
	/// Thoughts text.
	pub thoughts: Option<String>,
}

/// Native description of a think call (inline body only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThinkNativeView {
	/// Always inline.
	pub inline: bool,
	/// Markdown body text when present.
	pub body_markdown: Option<String>,
	/// Whether the body is still streaming.
	pub streaming: bool,
}

/// Describe a think call for native rendering.
pub fn describe_think_call(args: &ThinkRenderArgs, args_complete: bool) -> ThinkNativeView {
	let thoughts = args.thoughts.clone().unwrap_or_default();
	ThinkNativeView {
		inline: true,
		body_markdown: if thoughts.is_empty() { None } else { Some(thoughts) },
		streaming: !args_complete,
	}
}

/// Think results are not rendered.
pub fn describe_think_result() -> Option<ThinkNativeView> {
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn empty_thoughts_has_no_body() {
		let v = describe_think_call(&ThinkRenderArgs::default(), true);
		assert!(v.inline);
		assert!(v.body_markdown.is_none());
		assert!(!v.streaming);
	}

	#[test]
	fn streaming_flag_follows_args_complete() {
		let v = describe_think_call(
			&ThinkRenderArgs {
				thoughts: Some("hmm".into()),
			},
			false,
		);
		assert_eq!(v.body_markdown.as_deref(), Some("hmm"));
		assert!(v.streaming);
	}
}
