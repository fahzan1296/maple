//! Context snapshot shapes shared by the session snapshot builder and the
//! context viewer.
//!
//! Reference: `l/src/chrome/context-view-model.ts`.
//!
//! Deviations:
//! - `ContextBreakdown` and the snapshot builder live in the status-line
//!   context-usage module; this file carries the view shapes only.

/// Context categories in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextCategoryId {
	SystemPrompt,
	InstructionFiles,
	Skills,
	BuiltInTools,
	CustomTools,
	UserMessages,
	AssistantMessages,
	AssistantThinking,
	ToolCalls,
	ToolOutput,
}

/// A shared thinking block aggregated across entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextThinkingGroup {
	/// Stable group id.
	pub id: String,
	/// Display label.
	pub label: String,
	/// Aggregation weight for the unscaled replay estimate.
	pub tokens: usize,
	/// The group was cut off before completion.
	pub interrupted: bool,
}

/// UTF-16 offsets into entry text of a viewer-only marker standing in for
/// content shown in another category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextViewSourceMarker {
    /// First offset, in UTF-16 code units.
    pub start: usize,
    /// End offset, exclusive, in UTF-16 code units.
    pub end: usize,
    /// Category whose color draws the marker.
    pub category: ContextCategoryId,
}

/// One viewer entry: a labelled block of context text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextViewEntry {
	/// Stable entry id.
	pub id: String,
	/// Display label.
	pub label: String,
	/// Entry body text.
	pub text: String,
	/// Aggregation weight for the unscaled replay estimate.
	pub tokens: usize,
	/// The entry was cut off before completion.
	pub interrupted: bool,
	/// Shared thinking group this entry belongs to.
	pub thinking_group: Option<ContextThinkingGroup>,
	/// Source path, when the entry came from a file.
	pub path: Option<String>,
	/// Extra metadata shown under the body.
	pub info: Option<String>,
	/// Event timestamp (epoch milliseconds).
	pub timestamp: Option<i64>,
	/// Tool call id, when the entry belongs to one.
    pub call_id: Option<String>,
    /// Sorted, non-overlapping markers in `text`.
    pub source_markers: Option<Vec<ContextViewSourceMarker>>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn entry_defaults_are_empty() {
		let entry = ContextViewEntry::default();
		assert!(entry.id.is_empty());
		assert!(entry.label.is_empty());
		assert!(entry.text.is_empty());
		assert_eq!(entry.tokens, 0);
		assert!(!entry.interrupted);
		assert!(entry.thinking_group.is_none());
		assert!(entry.path.is_none());
		assert!(entry.info.is_none());
		assert!(entry.timestamp.is_none());
		assert!(entry.call_id.is_none());
		assert!(entry.source_markers.is_none());
	}

	#[test]
	fn entry_equality_covers_every_field() {
		let base = ContextViewEntry {
			id: "e1".to_string(),
			label: "System Prompt".to_string(),
			text: "hello".to_string(),
			tokens: 12,
			interrupted: true,
			thinking_group: Some(ContextThinkingGroup {
				id: "g1".to_string(),
				label: "thinking".to_string(),
				tokens: 5,
				interrupted: true,
			}),
			path: Some("AGENTS.md".to_string()),
			info: Some("2 lines".to_string()),
			timestamp: Some(7),
			call_id: Some("call-1".to_string()),
			source_markers: Some(vec![ContextViewSourceMarker {
				start: 1,
				end: 3,
				category: ContextCategoryId::SystemPrompt,
			}]),
		};
		assert_eq!(base, base.clone());
		let mut changed = base.clone();
		changed.tokens = 13;
		assert_ne!(base, changed);
	}

	#[test]
	fn source_markers_carry_category_and_offsets() {
		let marker = ContextViewSourceMarker {
			start: 2,
			end: 9,
			category: ContextCategoryId::ToolOutput,
		};
		assert_eq!(marker.start, 2);
		assert_eq!(marker.end, 9);
		let copied = marker.category;
		assert_eq!(copied, ContextCategoryId::ToolOutput);
	}

	#[test]
	fn thinking_group_defaults_are_incomplete() {
		let group = ContextThinkingGroup::default();
		assert!(group.id.is_empty());
		assert!(group.label.is_empty());
		assert_eq!(group.tokens, 0);
		assert!(!group.interrupted);
	}
}
