//! Port of `l/test/html-form.test.ts`.
//!
//! The crate ports the pure model (`html_form::parse`/`data`/`paginate`/`media`).
//! The interactive dialog is key-driven in the reference (`handleInput`) and has
//! no Rust equivalent, so the option-note, dialog and Kitty-placement tests stay
//! ignored.

use maple_tui::html_form::data::{
	default_form_data, field_error, form_answers, HtmlFormData, HtmlFormValue,
};
use maple_tui::html_form::media::{is_svg_media, load_form_image, FormMediaOptions};
use maple_tui::html_form::paginate::{paginate, BreakRank, Page, PageAtom};
use maple_tui::html_form::parse::{parse_html_form, spans_text, FormBlock, FormField, FormSpan};

const DB_FORM: &str = "<form>
<fieldset><legend>Database</legend>
<p>Which store should <code>ledger</code> use?</p>
<label><input type=\"radio\" name=\"db\" value=\"sqlite\" checked> SQLite</label><br>
<label><input type=\"radio\" name=\"db\" value=\"pg\"> Postgres</label>
</fieldset>
<fieldset><legend>Extras</legend>
<label><input type=\"checkbox\" name=\"extras\" value=\"ci\" checked> CI</label>
<label><input type=\"checkbox\" name=\"extras\" value=\"docker\"> Docker</label>
<label>Region <select name=\"region\"><option value=\"us\">US</option><option value=\"eu\">EU</option><option value=\"ap\">AP</option></select></label>
<label>Workers <input type=\"number\" name=\"workers\" value=\"4\" min=\"1\" max=\"8\" required></label>
<label>Notes<textarea name=\"notes\" placeholder=\"anything else\"></textarea></label>
</fieldset>
</form>";

const PNG_1PX: &[u8] = &[
	0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
	0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c,
	0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0x00, 0x00,
	0x00, 0x06, 0x00, 0x02, 0x30, 0x81, 0xd0, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
	0xae, 0x42, 0x60, 0x82,
];

#[test]
fn joins_radios_split_across_a_decision_table() {
	let form = parse_html_form(
		"<form><table><tr><th>Test</th><th>Fix</th><th>Skip</th></tr>\
		<tr><td>session refresh</td><td><input type=\"radio\" name=\"t1\" value=\"fix\" checked></td>\
		<td><input type=\"radio\" name=\"t1\" value=\"skip\"></td></tr></table></form>",
	);
	assert_eq!(form.fields.len(), 1);
	let FormField::Choice(choice) = &form.fields[0] else {
		panic!("expected a choice field");
	};
	let options: Vec<(String, String)> = choice
		.options
		.iter()
		.map(|option| (option.value.clone(), spans_text(&option.label)))
		.collect();
	assert_eq!(options, [("fix".to_string(), "Fix".to_string()), ("skip".to_string(), "Skip".to_string())]);
	assert_eq!(
		form.sections[0].blocks[0],
		FormBlock::Text {
			spans: vec![FormSpan { text: "session refresh".to_string(), style: None }],
			heading: false,
		}
	);
}

#[test]
fn offers_other_only_where_the_reference_does() {
	let form = parse_html_form(
		"<form>\
		<label><input type=\"radio\" name=\"a\" value=\"1\"> One</label><label><input type=\"radio\" name=\"a\" value=\"2\"> Two</label>\
		<label><input type=\"radio\" name=\"b\" value=\"1\"> One</label><label><input type=\"radio\" name=\"b\" value=\"x\"> Other (say which)</label>\
		<label><input type=\"checkbox\" name=\"c\" value=\"on\"> Lone</label>\
		<select name=\"d\"><option>One</option><option>Two</option></select></form>",
	);
	let flags: Vec<(&str, bool)> = form
		.fields
		.iter()
		.map(|field| {
			let other = match field {
				FormField::Choice(choice) => choice.other,
				FormField::Text(_) => false,
			};
			(field.name(), other)
		})
		.collect();
	assert_eq!(flags, [("a", true), ("b", false), ("c", false), ("d", false)]);
}

#[test]
fn keeps_pictures_as_blocks_and_uses_an_alt_text_inside_a_label() {
	let form = parse_html_form(
		"<form><p>Before <img src=\"a.png\" alt=\"flow\"> after</p>\
		<svg viewBox=\"0 0 10 10\"><title>Box</title><rect width=\"10\" height=\"10\"/></svg>\
		<label><input type=\"radio\" name=\"logo\" value=\"new\"> <img src=\"new.png\" alt=\"New logo\"></label>\
		<label><input type=\"radio\" name=\"logo\" value=\"old\"> Old</label></form>",
	);
	let blocks = &form.sections[0].blocks;
	assert!(blocks.iter().any(|block| matches!(block, FormBlock::Image { src, alt } if src == "a.png" && alt == "flow")));
	assert!(blocks.iter().any(|block| matches!(block, FormBlock::Svg { alt, .. } if alt == "Box")));
	assert!(blocks.iter().any(|block| matches!(block, FormBlock::Field { .. })));
	let FormField::Choice(logo) = &form.fields[0] else {
		panic!("expected the logo choice field");
	};
	assert_eq!(spans_text(&logo.options[0].label), "New logo");
	assert_eq!(logo.options[1].value, "old");
}

#[test]
fn drops_buttons_and_layout_markup_and_labels_by_for() {
	let form = parse_html_form(
		"<p>intro</p><form><input type=\"hidden\" name=\"kind\" value=\"deploy\">\
		<label for=\"d\">Domain</label><input id=\"d\" name=\"domain\" placeholder=\"app.example.com\">\
		<button type=\"submit\">Go</button></form>",
	);
	let fields: Vec<(String, String)> = form
		.fields
		.iter()
		.map(|field| (field.name().to_string(), spans_text(field.label())))
		.collect();
	assert_eq!(fields, [("domain".to_string(), "Domain".to_string())]);
	let data = default_form_data(&form);
	assert_eq!(data.get("kind"), Some(&HtmlFormValue::Text("deploy".to_string())));
	assert_eq!(data.get("domain"), Some(&HtmlFormValue::Text(String::new())));
}

#[test]
fn reports_defaults_with_html_semantics() {
	let form = parse_html_form(DB_FORM);
	let data = default_form_data(&form);
	assert_eq!(data.get("db"), Some(&HtmlFormValue::Text("sqlite".to_string())));
	assert_eq!(data.get("extras"), Some(&HtmlFormValue::Array(vec!["ci".to_string()])));
	assert_eq!(data.get("region"), Some(&HtmlFormValue::Text("us".to_string())));
	assert_eq!(data.get("workers"), Some(&HtmlFormValue::Number(4.0)));
	assert_eq!(data.get("notes"), Some(&HtmlFormValue::Text(String::new())));

	let answers = HtmlFormData::from([
		("db".to_string(), HtmlFormValue::Text("pg".to_string())),
		("extras".to_string(), HtmlFormValue::Array(vec![])),
		("region".to_string(), HtmlFormValue::Text("eu".to_string())),
		("workers".to_string(), HtmlFormValue::Number(2.0)),
		("notes".to_string(), HtmlFormValue::Text(String::new())),
	]);
	let listed = form_answers(&form, &answers);
	let rows: Vec<(String, String, String)> = listed
		.iter()
		.map(|answer| (answer.name.clone(), answer.question.clone(), answer.answer.clone()))
		.collect();
	assert_eq!(
		rows,
		[
			("db".to_string(), "Which store should ledger use?".to_string(), "Postgres".to_string()),
			("extras".to_string(), "Extras".to_string(), String::new()),
			("region".to_string(), "Region".to_string(), "EU".to_string()),
			("workers".to_string(), "Workers".to_string(), "2".to_string()),
			("notes".to_string(), "Notes".to_string(), String::new()),
		]
	);
}

#[test]
fn validates_required_bounds_and_patterns() {
	let form = parse_html_form(
		"<form>\
		<input name=\"n\" type=\"number\" min=\"1\" max=\"8\" required>\
		<input name=\"code\" pattern=\"[A-Z]{3}\">\
		<input name=\"bad\" pattern=\"[\">\
		<label><input type=\"checkbox\" name=\"pick\" value=\"a\" required> A</label>\
		<label><input type=\"checkbox\" name=\"pick\" value=\"b\"> B</label></form>",
	);
	let text = |value: &str| Some(HtmlFormValue::Text(value.to_string()));
	assert_eq!(field_error(&form.fields[0], text("")), Some("Required".to_string()));
	assert_eq!(field_error(&form.fields[0], text("9")), Some("Must be at most 8".to_string()));
	assert_eq!(field_error(&form.fields[0], text("x")), Some("Enter a number".to_string()));
	let pattern_error = field_error(&form.fields[1], text("ABCD")).expect("pattern error");
	assert!(pattern_error.starts_with("Must match"), "{pattern_error}");
	assert_eq!(field_error(&form.fields[1], text("ABC")), None);
	assert_eq!(field_error(&form.fields[2], text("anything")), None);
	assert_eq!(
		field_error(&form.fields[3], Some(&HtmlFormValue::Array(vec![]))),
		Some("Select at least one".to_string())
	);
}

fn atom(rows: usize, break_rank: BreakRank) -> PageAtom {
	PageAtom {
		lines: vec!["x".to_string(); rows],
		gap: 0,
		break_rank,
		continuation: None,
	}
}

#[test]
fn pagination_prefers_a_section_boundary_unless_it_strands_most_of_the_page() {
	let atoms = vec![
		atom(3, BreakRank::Section),
		atom(1, BreakRank::Section),
		(0..9).map(|_| atom(1, BreakRank::Block)).collect(),
	]
	.into_iter()
	.flatten()
	.collect::<Vec<_>>();
	assert_eq!(paginate(&atoms, 10)[0], Page { start: 0, end: 8 });

	let later = vec![
		atom(6, BreakRank::Section),
		atom(1, BreakRank::Section),
		(0..9).map(|_| atom(1, BreakRank::Inside)).collect(),
	]
	.into_iter()
	.flatten()
	.collect::<Vec<_>>();
	assert_eq!(paginate(&later, 10)[0], Page { start: 0, end: 1 });
}

#[test]
fn pagination_never_breaks_before_a_never_atom_unless_nothing_fits() {
	let atoms = vec![atom(4, BreakRank::Section), atom(4, BreakRank::Block), atom(4, BreakRank::Never)];
	assert_eq!(
		paginate(&atoms, 10),
		[
			Page { start: 0, end: 1 },
			Page { start: 1, end: 3 },
		]
	);
	let oversized = vec![atom(25, BreakRank::Section), atom(1, BreakRank::Block)];
	assert_eq!(
		paginate(&oversized, 10),
		[
			Page { start: 0, end: 1 },
			Page { start: 1, end: 2 },
		]
	);
}

#[test]
fn loads_images_from_data_uris_and_files_and_refuses_other_bytes() {
	let dir = std::env::temp_dir().join(format!("html-form-test-{}", std::process::id()));
	std::fs::create_dir_all(&dir).expect("temp dir");
	let result = (|| -> Result<(), String> {
		let options = FormMediaOptions { cwd: dir.clone() };
		std::fs::write(dir.join("dot.png"), PNG_1PX).map_err(|e| e.to_string())?;
		std::fs::write(dir.join("notes.txt"), "not a picture").map_err(|e| e.to_string())?;

		let from_file = load_form_image("dot.png", &options)?;
		assert_eq!(from_file.mime, "image/png");
		let from_data = load_form_image(
			"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
			&options,
		)?;
		assert_eq!(from_data.mime, "image/png");

		let svg = load_form_image(
			"data:image/svg+xml;utf8,%3Csvg viewBox='0 0 4 2'%3E%3C/svg%3E",
			&options,
		)?;
		assert!(is_svg_media(&svg));
		let markup = String::from_utf8(svg.bytes).expect("utf8");
		assert!(
			markup.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"2\""),
			"{markup}"
		);

		let refused = load_form_image("notes.txt", &options).expect_err("text is not an image");
		assert!(refused.contains("not a PNG"), "{refused}");
		assert!(load_form_image("missing.png", &options).is_err());
		Ok(())
	})();
	let _ = std::fs::remove_dir_all(&dir);
	result.expect("media assertions");
}

#[test]
#[ignore = "missing API: HtmlFormDialog has no key input (handle_input); the option-note and dialog suites are key-driven"]
fn option_notes_track_dropdown_options() {}

#[test]
#[ignore = "missing API: HtmlFormDialog has no key input (handle_input)"]
fn dialog_submits_the_keys_answers() {}

#[test]
#[ignore = "missing API: HtmlFormDialog has no key input (handle_input) or describe tree paths used by the native dialog tests"]
fn native_tabs_and_actions_follow_the_form() {}

#[test]
#[ignore = "missing API: no Kitty graphics placement harness in the Rust port"]
fn kitty_places_and_deletes_pictures() {}
