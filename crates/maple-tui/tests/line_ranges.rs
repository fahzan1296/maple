//! Port of `l/test/line-ranges.test.ts`.

use maple_tui::tools::line_ranges::{
	parse_line_range_chunk, parse_line_ranges, selector_line_ranges, BareLineMode, LineRange,
};

fn r(start: u64, end: Option<u64>) -> LineRange {
	LineRange {
		start_line: start,
		end_line: end,
	}
}

#[test]
fn reads_a_lone_bare_number_from_that_line_onward() {
	assert_eq!(parse_line_ranges("50", BareLineMode::Onward).unwrap(), Some(vec![r(50, None)]));
	assert_eq!(parse_line_range_chunk("50").unwrap(), Some(r(50, None)));
}

#[test]
fn pins_bare_numbers_in_a_comma_list_to_single_lines() {
	assert_eq!(
		parse_line_ranges("59,19", BareLineMode::Onward).unwrap(),
		Some(vec![r(19, Some(19)), r(59, Some(59))])
	);
	assert_eq!(
		parse_line_ranges("L19,L59", BareLineMode::Onward).unwrap(),
		Some(vec![r(19, Some(19)), r(59, Some(59))])
	);
}

#[test]
fn keeps_explicit_open_ended_chunks_open_inside_a_list() {
	assert_eq!(
		parse_line_ranges("19,59-", BareLineMode::Onward).unwrap(),
		Some(vec![r(19, Some(19)), r(59, None)])
	);
}

#[test]
fn merges_a_pinned_line_into_an_adjacent_range() {
	assert_eq!(
		parse_line_ranges("5-10,11", BareLineMode::Onward).unwrap(),
		Some(vec![r(5, Some(11))])
	);
}

#[test]
fn pins_a_lone_bare_number_compounded_with_raw_to_that_line() {
	assert_eq!(selector_line_ranges(Some("17:raw")).unwrap(), Some(vec![r(17, Some(17))]));
	assert_eq!(selector_line_ranges(Some("raw:17")).unwrap(), Some(vec![r(17, Some(17))]));
	assert_eq!(selector_line_ranges(Some("17-:raw")).unwrap(), Some(vec![r(17, None)]));
	assert_eq!(selector_line_ranges(Some("17")).unwrap(), Some(vec![r(17, None)]));
}
