//! Port of `l/test/composer-click.test.ts` (the pure `routeViewportClick`
//! describe; the component describes need the composer component tree).

use std::sync::{
	atomic::{AtomicUsize, Ordering},
	Arc,
};

use maple_tui::prompt::composer::{route_viewport_click, ViewportClickSpan};

fn span(start: usize, end: usize, ids: &[&str]) -> ViewportClickSpan {
	let ids: Vec<String> = ids.iter().map(|id| (*id).to_string()).collect();
	ViewportClickSpan {
		start,
		end,
		candidates: Arc::new(move |_| ids.clone()),
	}
}

#[test]
fn returns_the_hit_spans_candidates_with_the_span_local_row() {
	let local = Arc::new(AtomicUsize::new(usize::MAX));
	let spans = vec![
		ViewportClickSpan {
			start: 0,
			end: 2,
			candidates: {
				let local = Arc::clone(&local);
				Arc::new(move |row| {
					local.store(row, Ordering::SeqCst);
					vec!["CardAgent".to_string()]
				})
			},
		},
		span(3, 5, &["HudAgent"]),
	];
	assert_eq!(route_viewport_click(&spans, 1), vec!["CardAgent".to_string()]);
	assert_eq!(local.load(Ordering::SeqCst), 1);
	assert_eq!(route_viewport_click(&spans, 4), vec!["HudAgent".to_string()]);
}

#[test]
fn misses_separators_and_out_of_range_rows() {
	let spans = vec![span(0, 2, &["A"]), span(3, 4, &["B"])];
	// The reference also probes -1 and NaN; `usize` cannot represent them, so
	// the largest value stands in for "not a valid row".
	assert!(route_viewport_click(&spans, 2).is_empty());
	assert!(route_viewport_click(&spans, usize::MAX).is_empty());
	assert!(route_viewport_click(&spans, 99).is_empty());
}

#[test]
fn lets_the_first_overlapping_span_win() {
	let spans = vec![span(0, 5, &["A"]), span(2, 4, &["B"])];
	assert_eq!(route_viewport_click(&spans, 3), vec!["A".to_string()]);
}
