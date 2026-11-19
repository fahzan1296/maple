//! Port of `l/test/xd-url.test.ts`.

use maple_tui::tools::xd_url::parse_xd_url;

#[test]
fn parses_a_bare_device_url() {
	assert_eq!(
		parse_xd_url("xd://wiki_observe").map(|url| url.name),
		Some(Some("wiki_observe".to_string()))
	);
}

#[test]
fn keeps_a_trailing_raw_or_conflicts_suffix_in_the_device_name() {
	assert_eq!(
		parse_xd_url("xd://wiki_observe:raw").map(|url| url.name),
		Some(Some("wiki_observe:raw".to_string()))
	);
	assert_eq!(
		parse_xd_url("xd://wiki_observe:RAW").map(|url| url.name),
		Some(Some("wiki_observe:RAW".to_string()))
	);
	assert_eq!(
		parse_xd_url("xd://ast_edit:conflicts").map(|url| url.name),
		Some(Some("ast_edit:conflicts".to_string()))
	);
	assert_eq!(parse_xd_url("xd://:raw").map(|url| url.name), Some(Some(":raw".to_string())));
}

#[test]
fn rejects_path_and_query_characters() {
	assert_eq!(parse_xd_url("xd://wiki_observe/docs"), None);
	assert_eq!(parse_xd_url("xd://wiki_observe?q=1"), None);
}
