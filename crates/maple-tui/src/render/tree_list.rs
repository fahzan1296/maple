//! Hierarchical tree list rendering.
//!
//! Reference: l/src/render/tree-list.ts.
//!
//! Deviation: the reference produces its rows through the shared `TreeView`
//! component (`renderTreeListRows`) and returns pre-styled terminal strings.
//! This port keeps that row geometry — dim branch gutters, the collapse budget,
//! and the muted "… N more" row — but lays the rows out directly instead of
//! through a component instance. Items are flat roots, as in the reference: it
//! never asks `getChildren` for descendants, so nesting stays the caller's
//! concern. Each item renders exactly one row, matching the reference's
//! single-string `renderItem` path.

use crate::render::render_utils::{
	format_more_items, get_tree_branch, get_tree_continue_prefix, paint, replace_tabs,
};
use crate::render::types::TreeContext;
use crate::text_shape::visible_width;
use crate::theme::{Theme, ThemeColor};

/// Which end of a collapsed list is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TruncateFrom {
	/// Keep the first rows; the hidden remainder follows.
	#[default]
	End,
	/// Keep the last rows; the hidden remainder precedes them.
	Start,
}

/// Tree items and callbacks controlling their presentation.
pub struct TreeListOptions<T, F> {
	/// Rows to render, in display order.
	pub items: Vec<T>,
	/// Show every row.
	pub expanded: bool,
	/// Maximum rows when collapsed.
	pub max_collapsed: usize,
	/// Strict total-line budget when collapsed.
	pub max_collapsed_lines: Option<usize>,
	/// Pluralizable label used by the "more" row.
	pub item_type: String,
	/// Which end the collapse keeps.
	pub truncate_from: TruncateFrom,
	/// Caller-supplied trailing summary row; bypasses the built-in cap.
	pub trailing_summary: Option<String>,
	/// Row renderer; the line count must not vary with `is_last`.
	pub render_item: F,
}

impl<T, F> TreeListOptions<T, F> {
	/// Collapsed presentation over `items`.
	pub fn new(items: Vec<T>, render_item: F) -> Self {
		Self {
			items,
			expanded: false,
			max_collapsed: 8,
			max_collapsed_lines: None,
			item_type: "item".to_string(),
			truncate_from: TruncateFrom::End,
			trailing_summary: None,
			render_item,
		}
	}
}

/// One root row before gutter layout.
enum RootRow {
	Item(String),
	Summary(String),
}

/// Render tree items with themed branches and optional expansion.
pub fn render_tree_list<T, F>(options: &TreeListOptions<T, F>, theme: &Theme) -> Vec<String>
where
	F: Fn(&T, &TreeContext) -> String,
{
	let expanded = options.expanded;
	let max_items = if expanded {
		options.items.len()
	} else {
		options.items.len().min(options.max_collapsed)
	};
	let lines_budget = if expanded {
		None
	} else {
		options.max_collapsed_lines
	};

	let branch_first = format!(
		"{} ",
		paint(theme, ThemeColor::Dim, &get_tree_branch(false, theme))
	);
	let last_first = format!(
		"{} ",
		paint(theme, ThemeColor::Dim, &get_tree_branch(true, theme))
	);
	let branch_continue = paint(
		theme,
		ThemeColor::Dim,
		&get_tree_continue_prefix(false, theme),
	);
	let last_continue = paint(
		theme,
		ThemeColor::Dim,
		&get_tree_continue_prefix(true, theme),
	);
	let prefix_width = [
		branch_first.as_str(),
		last_first.as_str(),
		branch_continue.as_str(),
		last_continue.as_str(),
	]
	.iter()
	.map(|prefix| visible_width(prefix))
	.max();

	// Caller-driven collapse: render exactly the provided items plus a summary.
	if !expanded && options.trailing_summary.is_some() {
		let mut roots: Vec<RootRow> = Vec::with_capacity(options.items.len() + 1);
		for (index, item) in options.items.iter().enumerate() {
			let is_last = index + 1 == options.items.len();
			roots.push(RootRow::Item(render_row(
				&options.render_item,
				item,
				index,
				is_last,
				prefix_width,
				theme,
			)));
		}
		if let Some(summary) = options.trailing_summary.as_deref() {
			if !summary.is_empty() {
				roots.push(RootRow::Summary(paint(theme, ThemeColor::Muted, summary)));
			}
		}
		return finish_rows(&roots, &branch_first, &last_first, theme);
	}

	let candidates: Vec<usize> = match options.truncate_from {
		TruncateFrom::Start => {
			let start = options.items.len().saturating_sub(max_items);
			(start..options.items.len()).collect()
		}
		TruncateFrom::End => (0..max_items).collect(),
	};

	let (displayed, remaining, fitted_lines) = fit_candidates(
		&candidates,
		options.items.len(),
		options.truncate_from,
		lines_budget,
	);

	let summary_fits = match lines_budget {
		Some(budget) => fitted_lines < budget,
		None => true,
	};
	let has_summary = !expanded && remaining > 0 && summary_fits;
	let summary_last = has_summary && options.truncate_from == TruncateFrom::End;
	let summary_first = has_summary && options.truncate_from == TruncateFrom::Start;
	let total_roots = displayed.len() + usize::from(has_summary);

	let mut roots: Vec<RootRow> = Vec::with_capacity(total_roots);
	if summary_first {
		roots.push(more_row(remaining, &options.item_type, theme));
	}
	for (offset, &index) in displayed.iter().enumerate() {
		let root_position = offset + usize::from(summary_first);
		let is_last = root_position + 1 == total_roots;
		roots.push(RootRow::Item(render_row(
			&options.render_item,
			&options.items[index],
			index,
			is_last,
			prefix_width,
			theme,
		)));
	}
	if summary_last {
		roots.push(more_row(remaining, &options.item_type, theme));
	}
	finish_rows(&roots, &branch_first, &last_first, theme)
}

/// Fit candidate rows into the line budget, returning the displayed slice, the
/// number of hidden rows, and the lines the displayed rows occupy.
fn fit_candidates(
	candidates: &[usize],
	total: usize,
	truncate_from: TruncateFrom,
	lines_budget: Option<usize>,
) -> (&[usize], usize, usize) {
	let mut fitting = candidates.len();
	let mut fitted_lines = 0usize;
	if let Some(budget) = lines_budget {
		fitting = 0;
		match truncate_from {
			TruncateFrom::Start => {
				for offset in (0..candidates.len()).rev() {
					let reserved = usize::from(candidates[offset] > 0);
					if fitted_lines + 1 + reserved > budget {
						break;
					}
					fitted_lines += 1;
					fitting += 1;
				}
			}
			TruncateFrom::End => {
				for offset in 0..candidates.len() {
					let remaining_after = total.saturating_sub(offset + 1);
					let reserved = usize::from(remaining_after > 0);
					if fitted_lines + 1 + reserved > budget {
						break;
					}
					fitted_lines += 1;
					fitting = offset + 1;
				}
			}
		}
	}

	let displayed = match truncate_from {
		TruncateFrom::Start => &candidates[candidates.len() - fitting..],
		TruncateFrom::End => &candidates[..fitting],
	};
	let remaining = match truncate_from {
		TruncateFrom::Start => displayed.first().copied().unwrap_or(0),
		TruncateFrom::End => total - fitting,
	};
	(displayed, remaining, fitted_lines)
}

/// The muted "… N more" root.
fn more_row(remaining: usize, item_type: &str, theme: &Theme) -> RootRow {
	RootRow::Summary(paint(
		theme,
		ThemeColor::Muted,
		&format_more_items(remaining, item_type),
	))
}

/// Attach each root's gutter. Empty item rows are dropped, as the reference's
/// `TreeView` skips them before drawing.
fn finish_rows(
	roots: &[RootRow],
	branch_first: &str,
	last_first: &str,
	theme: &Theme,
) -> Vec<String> {
	let mut rows = Vec::with_capacity(roots.len());
	for (position, root) in roots.iter().enumerate() {
		let is_last = position + 1 == roots.len();
		match root {
			RootRow::Item(text) => {
				if text.is_empty() {
					continue;
				}
				let gutter = if is_last { last_first } else { branch_first };
				rows.push(format!("{gutter}{}", replace_tabs(text)));
			}
			RootRow::Summary(summary) => {
				let connector = get_tree_branch(is_last, theme);
				rows.push(format!(
					"{} {summary}",
				paint(theme, ThemeColor::Dim, &connector)
				));
			}
		}
	}
	rows
}

/// Render one item with the shared row context.
fn render_row<T, F>(
	render_item: &F,
	item: &T,
	index: usize,
	is_last: bool,
	prefix_width: Option<usize>,
	theme: &Theme,
) -> String
where
	F: Fn(&T, &TreeContext) -> String,
{
	let context = TreeContext {
		index,
		is_last,
		depth: 0,
		theme: theme.clone(),
		prefix: String::new(),
		continue_prefix: String::new(),
		prefix_width,
	};
	render_item(item, &context)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::native::spans::plain_text;

	fn theme() -> Theme {
		crate::render::render_utils::tests::sample_theme()
	}

	fn visible(text: &str) -> String {
		plain_text(text)
	}

	fn gutter(theme: &Theme, glyph: &str) -> String {
		format!("{} ", paint(theme, ThemeColor::Dim, glyph))
	}

	fn labels<'a>(values: &'a [&'a str]) -> Vec<&'a str> {
		values.to_vec()
	}

	#[test]
	fn flat_list_draws_root_gutters() {
		let theme = theme();
		let mut options = TreeListOptions::new(
			labels(&["alpha", "beta", "gamma"]),
			|label: &&str, _ctx: &TreeContext| (*label).to_string(),
		);
		options.expanded = true;
		let rows = render_tree_list(&options, &theme);

		assert_eq!(rows.len(), 3);
		assert_eq!(visible(&rows[0]), "├─ alpha");
		assert_eq!(visible(&rows[1]), "├─ beta");
		assert_eq!(visible(&rows[2]), "└─ gamma");
		assert_eq!(rows[2], format!("{}gamma", gutter(&theme, "└─")));
	}

	#[test]
	fn collapsed_list_appends_muted_more_row() {
		let theme = theme();
		let mut options = TreeListOptions::new(
			labels(&["a", "b", "c", "d"]),
			|label: &&str, _ctx: &TreeContext| (*label).to_string(),
		);
		options.max_collapsed = 2;
		options.item_type = "file".to_string();
		let rows = render_tree_list(&options, &theme);

		assert_eq!(rows.len(), 3);
		assert_eq!(visible(&rows[0]), "├─ a");
		assert_eq!(visible(&rows[1]), "├─ b");
		assert_eq!(visible(&rows[2]), "└─ … 2 more files");
		assert_eq!(
			rows[2],
			format!(
				"{}{}",
				gutter(&theme, "└─"),
				paint(&theme, ThemeColor::Muted, "… 2 more files")
			)
		);
	}

	#[test]
	fn collapsed_list_from_start_prepends_the_more_row() {
		let theme = theme();
		let mut options = TreeListOptions::new(
			labels(&["a", "b", "c", "d"]),
			|label: &&str, _ctx: &TreeContext| (*label).to_string(),
		);
		options.max_collapsed = 2;
		options.truncate_from = TruncateFrom::Start;
		let rows = render_tree_list(&options, &theme);

		assert_eq!(rows.len(), 3);
		assert_eq!(visible(&rows[0]), "├─ … 2 more items");
		assert_eq!(visible(&rows[1]), "├─ c");
		assert_eq!(visible(&rows[2]), "└─ d");
	}

	#[test]
	fn line_budget_reserves_room_for_the_summary_row() {
		let theme = theme();
		let mut options = TreeListOptions::new(
			labels(&["one", "two", "three"]),
			|label: &&str, _ctx: &TreeContext| (*label).to_string(),
		);
		options.max_collapsed_lines = Some(2);
		let rows = render_tree_list(&options, &theme);

		assert_eq!(rows.len(), 2);
		assert_eq!(visible(&rows[0]), "├─ one");
		assert_eq!(visible(&rows[1]), "└─ … 2 more items");
	}

	#[test]
	fn trailing_summary_bypasses_the_item_cap() {
		let theme = theme();
		let mut options = TreeListOptions::new(
			labels(&["a", "b", "c"]),
			|label: &&str, _ctx: &TreeContext| (*label).to_string(),
		);
		options.max_collapsed = 1;
		options.trailing_summary = Some("3 total".to_string());
		let rows = render_tree_list(&options, &theme);

		assert_eq!(rows.len(), 4);
		assert_eq!(visible(&rows[0]), "├─ a");
		assert_eq!(visible(&rows[1]), "├─ b");
		assert_eq!(visible(&rows[2]), "├─ c");
		assert_eq!(visible(&rows[3]), "└─ 3 total");
	}

	#[test]
	fn empty_rows_are_skipped() {
		let theme = theme();
		let mut options = TreeListOptions::new(
			labels(&["a", "skip", "c"]),
			|label: &&str, _ctx: &TreeContext| {
				if *label == "skip" {
					String::new()
				} else {
					(*label).to_string()
				}
			},
		);
		options.expanded = true;
		let rows = render_tree_list(&options, &theme);

		assert_eq!(rows.len(), 2);
		assert_eq!(visible(&rows[0]), "├─ a");
		assert_eq!(visible(&rows[1]), "└─ c");
	}

	#[test]
	fn render_item_sees_index_and_last_flag() {
		let theme = theme();
		let seen = std::cell::RefCell::new(Vec::new());
		let mut options = TreeListOptions::new(
			labels(&["a", "b"]),
			|label: &&str, ctx: &TreeContext| {
				seen.borrow_mut().push((label.to_string(), ctx.index, ctx.is_last));
				label.to_string()
			},
		);
		options.expanded = true;
		let rows = render_tree_list(&options, &theme);

		assert_eq!(visible(&rows[0]), "├─ a");
		assert_eq!(visible(&rows[1]), "└─ b");
		assert_eq!(
			seen.into_inner(),
			vec![("a".to_string(), 0, false), ("b".to_string(), 1, true)]
		);
	}
}
