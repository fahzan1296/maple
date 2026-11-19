//! Reference: `l/src/prompt/interactive-context-helpers.ts`.
//!
//! Shared assistant transcript construction and model-authored link caching:
//! the distinct Markdown destinations a reply authored, their resolution against
//! the session (cached so synchronous transcript rebuilds stay cheap), and the
//! display flags a transcript segment is built with.
//!
//! Deviations:
//! - The `AssistantMessageComponent` / `ReplyWriteComponent` constructors have no
//!   port in this crate yet; [`assistant_display_state`] returns the flags those
//!   constructors would apply, so the wiring lands with the components.
//! - `ui.requestRender` and `imageBudget` are host rendering concerns and are
//!   omitted from [`AssistantMessageHost`].
//! - The session-scoped `Symbol` cache becomes an explicit
//!   [`LinkTargetCache`] passed by the caller (Rust passes state explicitly).
//! - The `WeakMap` memo of per-block hrefs disappears: [`TextContent`] is a value
//!   type here, and the memo was keyed on the block object's identity.

use std::collections::{HashMap, HashSet};

use crate::chat::messages::{AssistantContent, AssistantMessage, TextContent};
use crate::components::markdown::Markdown;

/// Session display capabilities supplied unchanged by the interactive host.
#[derive(Clone, Debug, Default)]
pub struct AssistantMessageSession {
	pub model: Option<ModelCompat>,
}

/// The model compatibility facets these helpers read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelCompat {
	/// `"possible"` when the wire may rewrite text it already streamed.
	pub stream_revision: Option<String>,
}

impl ModelCompat {
	/// Whether the wire can revise already-streamed text.
	pub fn wire_revisable(&self) -> bool {
		self.stream_revision.as_deref() == Some("possible")
	}
}

/// Host state required to construct and refresh assistant transcript segments.
pub struct AssistantMessageHost<'a> {
	pub view_session: AssistantMessageSession,
	pub effective_hide_thinking_block: bool,
	pub prose_only_thinking: bool,
	pub expand_thinking_blocks: bool,
	pub assistant_images_visible: bool,
	/// Whether the viewed session's tables get charts: the main session's do, a
	/// focused subagent's do not.
	pub table_charts_visible: bool,
	pub hide_tool_activity: bool,
	pub tool_output_expanded: bool,
	/// Resolve already-extracted Markdown link destinations to local targets.
	pub resolve_assistant_message_link_hrefs:
		&'a dyn Fn(&[String]) -> HashMap<String, String>,
}

/// Session-scoped cache of resolved prose link destinations.
#[derive(Clone, Debug, Default)]
pub struct LinkTargetCache {
	targets: HashMap<String, String>,
}

impl LinkTargetCache {
	/// Current resolved destinations for synchronous component construction.
	pub fn targets(&self) -> &HashMap<String, String> {
		&self.targets
	}

	/// Replace the cache contents.
	pub fn set_targets(&mut self, targets: HashMap<String, String>) {
		self.targets = targets;
	}
}

/// Distinct Markdown link destinations authored in `message`'s text blocks.
pub fn assistant_message_link_hrefs(message: &AssistantMessage) -> Vec<String> {
	let mut hrefs: HashSet<String> = HashSet::new();
	for content in &message.content {
		let AssistantContent::Text(text) = content else {
			continue;
		};
		for href in text_block_hrefs(text) {
			hrefs.insert(href);
		}
	}
	hrefs.into_iter().collect()
}

/// Markdown link destinations of one text block, through the crate's renderer
/// grammar (the reference's `getMarkdownLinkUrls`).
fn text_block_hrefs(text: &TextContent) -> Vec<String> {
	let mut markdown = Markdown::new();
	markdown.set_text(text.text.clone());
	markdown.links()
}

/// Resolve and cache the current session's model-authored prose links. Existing
/// entries remain available to synchronous transcript rebuilds; links present in
/// this batch are replaced atomically so missing resources cannot retain a stale
/// destination.
pub fn refresh_assistant_message_link_targets(
	ctx: &AssistantMessageHost<'_>,
	messages: &[AssistantMessage],
	cache: &mut LinkTargetCache,
) -> HashMap<String, String> {
	let mut hrefs: HashSet<String> = HashSet::new();
	for message in messages {
		for href in assistant_message_link_hrefs(message) {
			hrefs.insert(href);
		}
	}
	if hrefs.is_empty() {
		return cache.targets.clone();
	}
	let requested: Vec<String> = hrefs.iter().cloned().collect();
	let resolved = (ctx.resolve_assistant_message_link_hrefs)(&requested);

	let mut next = cache.targets.clone();
	for href in &hrefs {
		if cache.targets.get(href) != resolved.get(href) {
			next = cache.targets.clone();
			break;
		}
	}
	if next == cache.targets {
		return cache.targets.clone();
	}
	for href in &hrefs {
		next.remove(href);
	}
	for (href, target) in resolved {
		next.insert(href, target);
	}
	cache.set_targets(next.clone());
	next
}

/// Current resolved destinations for synchronous component construction.
pub fn get_assistant_message_link_targets(cache: &LinkTargetCache) -> HashMap<String, String> {
	cache.targets().clone()
}

/// Limit a session snapshot to destinations authored by one rendered segment.
pub fn assistant_message_link_targets(
	message: &AssistantMessage,
	targets: &HashMap<String, String>,
) -> HashMap<String, String> {
	let mut selected = HashMap::new();
	for href in assistant_message_link_hrefs(message) {
		if let Some(target) = targets.get(&href) {
			selected.insert(href, target.clone());
		}
	}
	selected
}

/// Display flags a transcript segment is built with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistantDisplayState {
	pub images_visible: bool,
	pub table_charts_visible: bool,
	pub tool_result_images_visible: bool,
	pub expanded: bool,
	/// A wire the `stream-revision` axis marks `possible` can rewrite text it has
	/// already streamed; published rows are unrecoverable once they reach native
	/// scrollback, so those wires keep finished lines in the live viewport.
	pub mid_stream_publication: bool,
}

/// Apply the context's image, chart, expansion, and scrollback-publication
/// display state.
pub fn assistant_display_state(ctx: &AssistantMessageHost<'_>) -> AssistantDisplayState {
	let wire_revisable = ctx
		.view_session
		.model
		.as_ref()
		.is_some_and(ModelCompat::wire_revisable);
	AssistantDisplayState {
		images_visible: ctx.assistant_images_visible,
		table_charts_visible: ctx.table_charts_visible,
		tool_result_images_visible: !ctx.hide_tool_activity,
		expanded: ctx.tool_output_expanded,
		mid_stream_publication: !wire_revisable,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::messages::TextContent;

	fn reply(blocks: &[&str]) -> AssistantMessage {
		AssistantMessage {
			content: blocks
				.iter()
				.map(|text| {
					AssistantContent::Text(TextContent {
						text: (*text).to_string(),
						text_signature: None,
					})
				})
				.collect(),
			..AssistantMessage::default()
		}
	}

	fn host<'a>(
		resolver: &'a dyn Fn(&[String]) -> HashMap<String, String>,
	) -> AssistantMessageHost<'a> {
		AssistantMessageHost {
			view_session: AssistantMessageSession::default(),
			effective_hide_thinking_block: false,
			prose_only_thinking: false,
			expand_thinking_blocks: false,
			assistant_images_visible: true,
			table_charts_visible: true,
			hide_tool_activity: false,
			tool_output_expanded: false,
			resolve_assistant_message_link_hrefs: resolver,
		}
	}

	#[test]
	fn collects_distinct_hrefs_across_blocks() {
		let message = reply(&[
			"see [docs](local://docs/a.md) and [b](local://docs/b.md)",
			"again [a](local://docs/a.md)",
		]);
		let mut hrefs = assistant_message_link_hrefs(&message);
		hrefs.sort();
		assert_eq!(hrefs, vec!["local://docs/a.md", "local://docs/b.md"]);
	}

	#[test]
	fn refresh_caches_only_resolved_hrefs() {
		let resolve = |hrefs: &[String]| {
			hrefs
				.iter()
				.filter(|href| href.contains("a.md"))
				.map(|href| (href.clone(), "/repo/a.md".to_string()))
				.collect::<HashMap<_, _>>()
		};
		let mut cache = LinkTargetCache::default();
		let ctx = host(&resolve);
		let messages = vec![reply(&["[a](local://docs/a.md) [b](local://docs/b.md)"])];
		let targets = refresh_assistant_message_link_targets(&ctx, &messages, &mut cache);
		assert_eq!(targets.len(), 1);
		assert_eq!(targets["local://docs/a.md"], "/repo/a.md");
		assert_eq!(get_assistant_message_link_targets(&cache), targets);

		// Nothing authored: the previous cache is returned untouched.
		let empty = vec![reply(&["no links"])];
		assert_eq!(
			refresh_assistant_message_link_targets(&ctx, &empty, &mut cache),
			targets
		);
	}

	#[test]
	fn refresh_replaces_batch_entries() {
		let mut cache = LinkTargetCache::default();
		cache.set_targets(HashMap::from([
			("local://docs/a.md".to_string(), "/old/a.md".to_string()),
			("local://docs/keep.md".to_string(), "/keep".to_string()),
		]));
		let resolve =
			|_hrefs: &[String]| HashMap::from([("local://docs/a.md".to_string(), "/new/a.md".to_string())]);
		let ctx = host(&resolve);
		let messages = vec![reply(&["[a](local://docs/a.md)"])];
		let targets = refresh_assistant_message_link_targets(&ctx, &messages, &mut cache);
		assert_eq!(targets["local://docs/a.md"], "/new/a.md");
		assert_eq!(targets["local://docs/keep.md"], "/keep");
	}

	#[test]
	fn segment_targets_are_filtered_to_authored_links() {
		let message = reply(&["[a](local://docs/a.md)"]);
		let targets = HashMap::from([
			("local://docs/a.md".to_string(), "/a".to_string()),
			("local://docs/b.md".to_string(), "/b".to_string()),
		]);
		let selected = assistant_message_link_targets(&message, &targets);
		assert_eq!(selected.len(), 1);
		assert_eq!(selected["local://docs/a.md"], "/a");
	}

	#[test]
	fn display_state_honours_wire_revision() {
		let resolve = |_hrefs: &[String]| HashMap::new();
		let mut ctx = host(&resolve);
		let state = assistant_display_state(&ctx);
		assert!(state.images_visible);
		assert!(state.mid_stream_publication);

		ctx.hide_tool_activity = true;
		ctx.view_session.model = Some(ModelCompat {
			stream_revision: Some("possible".to_string()),
		});
		let state = assistant_display_state(&ctx);
		assert!(!state.tool_result_images_visible);
		assert!(!state.mid_stream_publication);
	}
}
