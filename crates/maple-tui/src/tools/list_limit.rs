//! List/result limit application with truncation metadata.
//!
//! Reference: `l/src/tools/list-limit.ts`.
//!
//! Deviations:
//! - `LimitsMeta` is not yet ported in this crate; limit facts are carried in
//!   [`ListLimitsMeta`] as plain optional fields.
//! - `Partial<LimitsMeta>` becomes an explicit struct with `Option` fields.

/// Items retained after applying output limits and their metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLimitResult<T> {
	/// Items kept after limit and head cap.
	pub items: Vec<T>,
	/// The effective limit reached, when truncation happened at that stage.
	pub limit_reached: Option<usize>,
	/// Truncation metadata for the applied limits.
	pub meta: ListLimitsMeta,
}

/// Limits applied to result lists and display previews.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListLimitOptions {
	/// Maximum number of result items to keep.
	pub limit: Option<usize>,
	/// Maximum number of items to show in the preview/head.
	pub head_limit: Option<usize>,
	/// Which result-limit bucket to record.
	pub limit_type: LimitType,
}

/// Which limit field a reached result limit populates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LimitType {
	/// Record into `match_limit`.
	Match,
	/// Record into `result_limit`.
	#[default]
	Result,
}

/// Suggestion accompanying a reached limit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LimitSuggestion {
	/// The limit value that was reached.
	pub reached: usize,
	/// Suggested larger limit; omitted when already at a hard cap.
	pub suggestion: Option<usize>,
}

/// Truncation metadata produced by [`apply_list_limit`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListLimitsMeta {
	/// Match-count limit metadata.
	pub match_limit: Option<LimitSuggestion>,
	/// Result-count limit metadata.
	pub result_limit: Option<LimitSuggestion>,
	/// Head/preview limit metadata.
	pub head_limit: Option<LimitSuggestion>,
}

/// Apply result and preview limits while retaining truncation metadata.
pub fn apply_list_limit<T>(items: Vec<T>, options: ListLimitOptions) -> ListLimitResult<T> {
	let mut meta = ListLimitsMeta::default();
	let effective_limit = positive(options.limit);
	let effective_head_limit = positive(options.head_limit);
	let mut limited = items;
	let mut limit_reached = None;

	if let Some(limit) = effective_limit {
		if limited.len() >= limit {
			limited.truncate(limit);
			limit_reached = Some(limit);
			let suggestion = LimitSuggestion {
				reached: limit,
				suggestion: Some(limit.saturating_mul(2)),
			};
			match options.limit_type {
				LimitType::Match => meta.match_limit = Some(suggestion),
				LimitType::Result => meta.result_limit = Some(suggestion),
			}
		}
	}

	if let Some(head) = effective_head_limit {
		if limited.len() > head {
			limited.truncate(head);
			meta.head_limit = Some(LimitSuggestion {
				reached: head,
				suggestion: Some(head.saturating_mul(2)),
			});
		}
	}

	ListLimitResult {
		items: limited,
		limit_reached,
		meta,
	}
}

fn positive(value: Option<usize>) -> Option<usize> {
	match value {
		Some(v) if v > 0 => Some(v),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn no_limits_returns_all() {
		let out = apply_list_limit(vec![1, 2, 3], ListLimitOptions::default());
		assert_eq!(out.items, vec![1, 2, 3]);
		assert_eq!(out.limit_reached, None);
		assert_eq!(out.meta, ListLimitsMeta::default());
	}

	#[test]
	fn result_limit_truncates_and_records() {
		let out = apply_list_limit(
			vec![1, 2, 3, 4],
			ListLimitOptions {
				limit: Some(3),
				head_limit: None,
				limit_type: LimitType::Result,
			},
		);
		assert_eq!(out.items, vec![1, 2, 3]);
		assert_eq!(out.limit_reached, Some(3));
		assert_eq!(
			out.meta.result_limit,
			Some(LimitSuggestion {
				reached: 3,
				suggestion: Some(6),
			})
		);
		assert!(out.meta.match_limit.is_none());
	}

	#[test]
	fn match_limit_records_in_match_bucket() {
		let out = apply_list_limit(
			vec![1, 2, 3],
			ListLimitOptions {
				limit: Some(2),
				head_limit: None,
				limit_type: LimitType::Match,
			},
		);
		assert_eq!(out.limit_reached, Some(2));
		assert!(out.meta.match_limit.is_some());
		assert!(out.meta.result_limit.is_none());
	}

	#[test]
	fn head_limit_applies_after_result_limit() {
		let out = apply_list_limit(
			vec![1, 2, 3, 4, 5],
			ListLimitOptions {
				limit: Some(4),
				head_limit: Some(2),
				limit_type: LimitType::Result,
			},
		);
		assert_eq!(out.items, vec![1, 2]);
		assert_eq!(out.limit_reached, Some(4));
		assert_eq!(
			out.meta.head_limit,
			Some(LimitSuggestion {
				reached: 2,
				suggestion: Some(4),
			})
		);
	}

	#[test]
	fn zero_limits_are_ignored() {
		let out = apply_list_limit(
			vec![1, 2],
			ListLimitOptions {
				limit: Some(0),
				head_limit: Some(0),
				limit_type: LimitType::Result,
			},
		);
		assert_eq!(out.items, vec![1, 2]);
		assert_eq!(out.limit_reached, None);
	}

	#[test]
	fn limit_only_triggers_when_length_at_least_limit() {
		let out = apply_list_limit(
			vec![1, 2],
			ListLimitOptions {
				limit: Some(3),
				head_limit: None,
				limit_type: LimitType::Result,
			},
		);
		assert_eq!(out.items, vec![1, 2]);
		assert_eq!(out.limit_reached, None);
	}
}
