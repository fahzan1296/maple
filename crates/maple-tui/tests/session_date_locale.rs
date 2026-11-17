//! Port of `l/test/session-date-locale.test.ts`.

use std::collections::HashMap;

use maple_tui::overlays::session_selector::system_date_locale;

fn env(entries: &[(&str, &str)]) -> HashMap<String, String> {
	entries
		.iter()
		.map(|(key, value)| ((*key).to_string(), (*value).to_string()))
		.collect()
}

#[test]
fn turns_a_posix_locale_into_a_bcp47_tag() {
	assert_eq!(
		system_date_locale(&env(&[("LANG", "en_GB.UTF-8")])),
		Some("en-GB".to_string())
	);
}

#[test]
fn prefers_lc_all_over_lc_time_over_lang() {
	assert_eq!(
		system_date_locale(&env(&[
			("LC_ALL", "fr_FR.UTF-8"),
			("LC_TIME", "de_DE.UTF-8"),
			("LANG", "en_GB.UTF-8")
		])),
		Some("fr-FR".to_string())
	);
	assert_eq!(
		system_date_locale(&env(&[("LC_TIME", "de_DE.UTF-8"), ("LANG", "en_GB.UTF-8")])),
		Some("de-DE".to_string())
	);
}

#[test]
fn returns_none_for_c_posix_unset_and_unparsable_values() {
	assert_eq!(
		system_date_locale(&env(&[("LC_ALL", "C.UTF-8"), ("LANG", "de_DE.UTF-8")])),
		None
	);
	assert_eq!(system_date_locale(&env(&[("LANG", "POSIX")])), None);
	assert_eq!(system_date_locale(&env(&[])), None);
	assert_eq!(system_date_locale(&env(&[("LANG", "garbage_@@")])), None);
}
