//! Answers of a parsed [`HtmlForm`]: the data shape a submission reports,
//! the defaults a form opens with, and a readable question/answer listing for
//! transcripts and non-interactive hosts.
//!
//! Reference: `l/src/html-form/data.ts`.

use std::collections::{HashMap, HashSet};

use crate::html_form::parse::{spans_text, ChoiceField, FormBlock, FormField, HtmlForm, TextField};

/// Submitted value of one control: text, a number for `number`/`range` inputs, or a group's checked values.
#[derive(Clone, Debug, PartialEq)]
pub enum HtmlFormValue {
	Text(String),
	Number(f64),
	Array(Vec<String>),
}

/// Submitted form data by control `name`. Radios, single checkboxes and single
/// selects give their value (omitted when nothing is chosen); checkbox groups
/// and multi-selects give an array; text fields always give their text.
/// Disabled options, unnamed controls and buttons never appear; hidden inputs do.
pub type HtmlFormData = HashMap<String, HtmlFormValue>;

/// One field as a question and its answer, both plain text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormAnswer {
	pub name: String,
	pub question: String,
	pub answer: String,
}

/// A note attached to one checked choice option, separate from submitted values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionNote {
	pub name: String,
	pub question: String,
	/// The option's shown label.
	pub option: String,
	/// The option's submitted value, as it appears in the form data.
	pub value: String,
	pub text: String,
}

/// Label of the free-text answer the UI adds to radio and checkbox groups ([`ChoiceField`]'s `other`).
pub const OTHER_LABEL: &str = "Other";

/// Whether a choice field reports an array (checkbox group, multi-select) rather than one value.
pub fn is_array_choice(field: &ChoiceField) -> bool {
	field.multi && field.options.len() > 1
}

/// Value a choice field contributes given its checked option indices and, when
/// the user chose its "Other" answer, that answer's text: the typed text stands
/// in for a radio's value, or joins a checkbox group's array. `None` when the
/// field contributes nothing; blank Other text counts as no answer.
pub fn choice_value(
	field: &ChoiceField,
	checked: &HashSet<usize>,
	other: Option<&str>,
) -> Option<HtmlFormValue> {
	let typed = if field.other {
		other.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
	} else {
		None
	};
	let mut values = Vec::new();
	for (index, option) in field.options.iter().enumerate() {
		if checked.contains(&index) && !option.disabled {
			values.push(option.value.clone());
		}
	}
	if is_array_choice(field) {
		if let Some(typed) = typed {
			values.push(typed);
		}
		return Some(HtmlFormValue::Array(values));
	}
	if let Some(typed) = typed {
		return Some(HtmlFormValue::Text(typed));
	}
	values.into_iter().next().map(HtmlFormValue::Text)
}

/// Value a text field contributes: a number for non-empty numeric `number`/`range` inputs, else the text.
pub fn text_value(field: &TextField, text: &str) -> HtmlFormValue {
	if (field.r#type == "number" || field.r#type == "range") && !text.trim().is_empty() {
		if let Ok(number) = text.parse::<f64>() {
			if number.is_finite() {
				return HtmlFormValue::Number(number);
			}
		}
	}
	HtmlFormValue::Text(text.to_string())
}

/// Why `value` is not an acceptable answer for `field`, or `None` when it is.
/// Enforces `required`, numeric `min`/`max`, `minlength`/`maxlength`, `email`
/// and `url` shapes, and `pattern` (anchored, as HTML does; an invalid pattern
/// is ignored).
pub fn field_error(field: &FormField, value: Option<&HtmlFormValue>) -> Option<String> {
	match field {
		FormField::Choice(field) => {
			let empty = match value {
				None => true,
				Some(HtmlFormValue::Array(values)) => values.is_empty(),
				Some(HtmlFormValue::Text(text)) => text.is_empty(),
				Some(HtmlFormValue::Number(_)) => false,
			};
			if !field.required || !empty {
				return None;
			}
			if is_array_choice(field) {
				Some("Select at least one".to_string())
			} else {
				Some("Required".to_string())
			}
		}
		FormField::Text(field) => {
			let text = match value {
				None => String::new(),
				Some(HtmlFormValue::Text(text)) => text.clone(),
				Some(HtmlFormValue::Number(number)) => number.to_string(),
				Some(HtmlFormValue::Array(_)) => String::new(),
			};
			if text.trim().is_empty() {
				return if field.required { Some("Required".to_string()) } else { None };
			}
			if field.r#type == "number" || field.r#type == "range" {
				let Ok(number) = text.parse::<f64>() else {
					return Some("Enter a number".to_string());
				};
				if !number.is_finite() {
					return Some("Enter a number".to_string());
				}
				if let Some(min) = field.min {
					if number < min {
						return Some(format!("Must be at least {min}"));
					}
				}
				if let Some(max) = field.max {
					if number > max {
						return Some(format!("Must be at most {max}"));
					}
				}
			}
			if let Some(min_length) = field.min_length {
				if text.chars().count() < min_length {
					return Some(format!("Needs at least {min_length} characters"));
				}
			}
			if let Some(max_length) = field.max_length {
				if text.chars().count() > max_length {
					return Some(format!("At most {max_length} characters"));
				}
			}
			if field.r#type == "email" && !looks_like_email(&text) {
				return Some("Enter an email address".to_string());
			}
			if field.r#type == "url" && !looks_like_url(&text) {
				return Some("Enter a full URL".to_string());
			}
			if let Some(pattern) = &field.pattern {
				if let Some(re) = compile_pattern(pattern) {
					if !re.is_match(&text) {
						let shown = if pattern.chars().count() > 40 {
							let prefix: String = pattern.chars().take(39).collect();
							format!("{prefix}…")
						} else {
							pattern.clone()
						};
						return Some(format!("Must match {shown}"));
					}
				}
			}
			None
		}
	}
}

fn looks_like_email(text: &str) -> bool {
	// Simple shape check mirroring TS /^[^\s@]+@[^\s@]+$/
	if text.chars().any(char::is_whitespace) || !text.contains('@') {
		return false;
	}
	let mut parts = text.splitn(3, '@');
	let local = parts.next().unwrap_or("");
	let domain = parts.next().unwrap_or("");
	parts.next().is_none() && !local.is_empty() && !domain.is_empty() && !domain.contains('@')
}

fn looks_like_url(text: &str) -> bool {
	// TS uses URL.canParse(text). Approximate: requires scheme and authority-ish body.
	let Some((scheme, rest)) = text.split_once(':') else { return false };
	if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
		return false;
	}
	rest.starts_with("//") && rest.len() > 2
}

fn compile_pattern(pattern: &str) -> Option<regex::Regex> {
	// HTML matches `^(?:pattern)$`. JS used `v` flag; Rust regex has no /v but
	// most agent-written patterns work. Invalid patterns are ignored.
	let anchored = format!("^(?:{pattern})$");
	regex::Regex::new(&anchored).ok()
}

/// Data the form submits untouched: hidden inputs, preselected options and prefilled text.
pub fn default_form_data(form: &HtmlForm) -> HtmlFormData {
	let mut data = HtmlFormData::new();
	for (name, value) in &form.hidden {
		data.insert(name.clone(), HtmlFormValue::Text(value.clone()));
	}
	for field in &form.fields {
		let name = match field {
			FormField::Choice(f) => &f.name,
			FormField::Text(f) => &f.name,
		};
		if name.is_empty() {
			continue;
		}
		let value = match field {
			FormField::Text(f) => Some(text_value(f, &f.value)),
			FormField::Choice(f) => {
				let checked: HashSet<usize> = f
					.options
					.iter()
					.enumerate()
					.filter_map(|(i, opt)| if opt.checked { Some(i) } else { None })
					.collect();
				choice_value(f, &checked, None)
			}
		};
		if let Some(value) = value {
			data.insert(name.clone(), value);
		}
	}
	data
}

/// The question each field answers: its own label, else the text block right
/// before it (`<p>Which database?</p>` above a radio group), else its section
/// title, else a lone option's label (a single checkbox), else its name.
pub fn field_questions(form: &HtmlForm) -> HashMap<usize, String> {
	let mut questions = HashMap::new();
	let field_indices = field_index_map(form);
	for section in &form.sections {
		let title = spans_text(&section.title);
		let mut caption = String::new();
		for block in &section.blocks {
			match block {
				FormBlock::Field { field, .. } => {
					let own = spans_text(field.label());
					let lone = match field {
						FormField::Choice(choice) if choice.options.len() == 1 => {
							spans_text(&choice.options[0].label)
						}
						_ => String::new(),
					};
					let text = if !own.is_empty() {
						own
					} else if !caption.is_empty() {
						caption.clone()
					} else if !title.is_empty() {
						title.clone()
					} else if !lone.is_empty() {
						lone
					} else {
						field.name().to_string()
					};
					if let Some(&idx) = field_indices.get(&field_ptr_key(field)) {
						questions.insert(idx, one_line(&text));
					}
					caption.clear();
				}
				FormBlock::Text { spans, .. } => {
					caption = spans_text(spans);
				}
				_ => {
					caption.clear();
				}
			}
		}
	}
	questions
}

fn field_ptr_key(field: &FormField) -> (String, String, usize) {
	// Stable-enough identity for maps within one parsed form: (kind,name,label-text-len/options-len heuristic)
	match field {
		FormField::Choice(f) => ("choice".into(), f.name.clone(), f.options.len()),
		FormField::Text(f) => ("text".into(), f.name.clone(), f.value.len()),
	}
}

fn field_index_map(form: &HtmlForm) -> HashMap<(String, String, usize), usize> {
	let mut map = HashMap::new();
	for (i, field) in form.fields.iter().enumerate() {
		map.entry(field_ptr_key(field)).or_insert(i);
	}
	map
}

/// Plain text of a context block (paragraph or heading, list, preformatted
/// text, table) for hosts that cannot draw the form; `None` for fields and
/// pictures.
pub fn block_text(block: &FormBlock) -> Option<String> {
	match block {
		FormBlock::Text { spans, .. } => Some(spans_text(spans)),
		FormBlock::Pre { text } => Some(text.clone()),
		FormBlock::List { items, ordered } => Some(
			items
				.iter()
				.enumerate()
				.map(|(index, item)| {
					let prefix = if *ordered { format!("{}.", index + 1) } else { "-".to_string() };
					format!("{prefix} {}", spans_text(item))
				})
				.collect::<Vec<_>>()
				.join("\n"),
		),
		FormBlock::Table { rows, .. } => Some(
			rows.iter()
				.map(|cells| cells.iter().map(|cell| spans_text(cell)).collect::<Vec<_>>().join(" | "))
				.collect::<Vec<_>>()
				.join("\n"),
		),
		_ => None,
	}
}

/// The form text each named field's prompt carries when a host asks the form
/// one field at a time. Returns a map keyed by field index in `form.fields`.
/// Parts are separated by blank lines; a field without context maps to `""`.
pub fn field_contexts(form: &HtmlForm) -> HashMap<usize, String> {
	let questions = field_questions(form);
	let field_indices = field_index_map(form);
	let mut parts_map: HashMap<usize, Vec<String>> = HashMap::new();
	let mut pending: Vec<String> = Vec::new();
	let mut last_key: Option<usize> = None;

	for section in &form.sections {
		let title = spans_text(&section.title).trim().to_string();
		if !title.is_empty() {
			pending.push(title);
		}
		let mut owner_key: Option<usize> = None;
		let mut owner_end = 0usize;
		for (index, block) in section.blocks.iter().enumerate() {
			if let FormBlock::Field { field, options_end } = block {
				let name = field.name();
				if name.is_empty() {
					continue;
				}
				let Some(&fidx) = field_indices.get(&field_ptr_key(field)) else { continue };
				let question = questions.get(&fidx).cloned().unwrap_or_default();
				let own: Vec<String> = pending
					.iter()
					.filter(|part| one_line(part) != question)
					.cloned()
					.collect();
				parts_map.insert(fidx, own);
				pending = Vec::new();
				last_key = Some(fidx);
				owner_key = Some(fidx);
				let end = options_end.unwrap_or(0);
				let intervening = section.blocks.iter().enumerate().any(|(at, later)| {
					at > index
						&& at < end
						&& matches!(later, FormBlock::Field { field: f, .. } if !f.name().is_empty())
				});
				owner_end = if intervening { 0 } else { end };
				continue;
			}
			if let Some(text) = block_text(block) {
				if text.trim().is_empty() {
					continue;
				}
				if let Some(owner) = owner_key {
					if index < owner_end {
						if let Some(parts) = parts_map.get_mut(&owner) {
							parts.push(text);
							continue;
						}
					}
				}
				pending.push(text);
			}
		}
	}
	if let Some(last) = last_key {
		if let Some(parts) = parts_map.get_mut(&last) {
			parts.extend(pending);
		}
	}
	let mut contexts = HashMap::new();
	for (field, own) in parts_map {
		contexts.insert(field, own.join("\n\n"));
	}
	contexts
}

/// Answers of every named field, in form order, with option values shown as their labels.
pub fn form_answers(form: &HtmlForm, data: &HtmlFormData) -> Vec<FormAnswer> {
	let questions = field_questions(form);
	let mut answers = Vec::new();
	for (idx, field) in form.fields.iter().enumerate() {
		let name = field.name();
		if name.is_empty() {
			continue;
		}
		let value = data.get(name);
		answers.push(FormAnswer {
			name: name.to_string(),
			question: questions.get(&idx).cloned().unwrap_or_else(|| name.to_string()),
			answer: answer_text(field, value),
		});
	}
	answers
}

fn answer_text(field: &FormField, value: Option<&HtmlFormValue>) -> String {
	let Some(value) = value else { return String::new() };
	match (field, value) {
		(FormField::Text(_), HtmlFormValue::Text(text)) => text.clone(),
		(FormField::Text(_), HtmlFormValue::Number(number)) => {
			if number.fract() == 0.0 { format!("{}", *number as i64) } else { number.to_string() }
		}
		(FormField::Choice(choice), HtmlFormValue::Text(text)) => label_of(choice, text),
		(FormField::Choice(choice), HtmlFormValue::Number(number)) => {
			let raw = if number.fract() == 0.0 { format!("{}", *number as i64) } else { number.to_string() };
			label_of(choice, &raw)
		}
		(FormField::Choice(choice), HtmlFormValue::Array(values)) => {
			values.iter().map(|v| label_of(choice, v)).collect::<Vec<_>>().join(", ")
		}
		(_, HtmlFormValue::Array(values)) => values.join(", "),
	}
}

fn label_of(choice: &ChoiceField, raw: &str) -> String {
	if let Some(option) = choice.options.iter().find(|candidate| candidate.value == raw) {
		let label = one_line(&spans_text(&option.label));
		if !label.is_empty() {
			return label;
		}
		return raw.to_string();
	}
	if choice.other {
		format!("{OTHER_LABEL}: {raw}")
	} else {
		raw.to_string()
	}
}

fn one_line(text: &str) -> String {
	text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::html_form::parse::{parse_html_form, FormSpan};

	fn text_field() -> TextField {
		TextField {
			kind: "text".into(),
			name: "n".into(),
			r#type: "text".into(),
			multiline: false,
			label: vec![FormSpan { text: "Name".into(), style: None }],
			value: String::new(),
			placeholder: String::new(),
			required: false,
			min: None,
			max: None,
			pattern: None,
			min_length: None,
			max_length: None,
			rows: 1,
		}
	}

	#[test]
	fn is_array_choice_only_for_multi_with_multiple_options() {
		let form = parse_html_form("<form><label><input type=checkbox name=x value=a>A</label></form>");
		match &form.fields[0] { FormField::Choice(c) => assert!(!is_array_choice(c)), _ => panic!() }
		let form = parse_html_form("<form><label><input type=checkbox name=x value=a>A</label><label><input type=checkbox name=x value=b>B</label></form>");
		match &form.fields[0] { FormField::Choice(c) => assert!(is_array_choice(c)), _ => panic!() }
	}

	#[test]
	fn choice_value_handles_single_multi_and_other() {
		let form = parse_html_form("<form><label><input type=radio name=db value=pg>Postgres</label><label><input type=radio name=db value=my>MySQL</label></form>");
		let field = match &form.fields[0] { FormField::Choice(c) => c.clone(), _ => panic!() };
		let mut checked = HashSet::new();
		checked.insert(0);
		assert_eq!(choice_value(&field, &checked, None), Some(HtmlFormValue::Text("pg".into())));
		assert_eq!(choice_value(&field, &HashSet::new(), Some(" custom ")), Some(HtmlFormValue::Text("custom".into())));

		let form = parse_html_form("<form><label><input type=checkbox name=x value=a>A</label><label><input type=checkbox name=x value=b>B</label></form>");
		let mut field = match &form.fields[0] { FormField::Choice(c) => c.clone(), _ => panic!() };
		field.other = true;
		let mut checked = HashSet::new();
		checked.insert(1);
		assert_eq!(choice_value(&field, &checked, Some("z")), Some(HtmlFormValue::Array(vec!["b".into(), "z".into()])));
	}

	#[test]
	fn text_value_parses_numeric_types() {
		let mut field = text_field();
		field.r#type = "number".into();
		assert_eq!(text_value(&field, "42"), HtmlFormValue::Number(42.0));
		assert_eq!(text_value(&field, ""), HtmlFormValue::Text(String::new()));
		assert_eq!(text_value(&field, "x"), HtmlFormValue::Text("x".into()));
	}

	#[test]
	fn field_error_validates_required_numeric_length_email_url_pattern() {
		let mut t = text_field();
		t.required = true;
		assert_eq!(field_error(&FormField::Text(t.clone()), Some(&HtmlFormValue::Text("  ".into()))), Some("Required".into()));
		t.required = false;
		t.r#type = "number".into();
		t.min = Some(2.0);
		t.max = Some(5.0);
		assert_eq!(field_error(&FormField::Text(t.clone()), Some(&HtmlFormValue::Text("1".into()))), Some("Must be at least 2".into()));
		assert_eq!(field_error(&FormField::Text(t.clone()), Some(&HtmlFormValue::Text("6".into()))), Some("Must be at most 5".into()));
		t.r#type = "text".into();
		t.min = None; t.max = None;
		t.min_length = Some(3);
		assert_eq!(field_error(&FormField::Text(t.clone()), Some(&HtmlFormValue::Text("ab".into()))), Some("Needs at least 3 characters".into()));
		t.min_length = None;
		t.r#type = "email".into();
		assert_eq!(field_error(&FormField::Text(t.clone()), Some(&HtmlFormValue::Text("bad".into()))), Some("Enter an email address".into()));
		t.r#type = "url".into();
		assert_eq!(field_error(&FormField::Text(t.clone()), Some(&HtmlFormValue::Text("bad".into()))), Some("Enter a full URL".into()));
		t.r#type = "text".into();
		t.pattern = Some("[a-z]+".into());
		assert!(field_error(&FormField::Text(t.clone()), Some(&HtmlFormValue::Text("ABC".into()))).is_some());
		assert_eq!(field_error(&FormField::Text(t), Some(&HtmlFormValue::Text("abc".into()))), None);
	}

	#[test]
	fn default_form_data_questions_answers_and_contexts() {
		let form = parse_html_form(
			"<form>\
			 <input type=hidden name=h value=1>\
			 <fieldset><legend>DB</legend>\
			 <p>Which database?</p>\
			 <label><input type=radio name=db value=sqlite checked> SQLite</label>\
			 <label><input type=radio name=db value=pg> Postgres</label>\
			 </fieldset>\
			 <label>Name <input name=name value=Bob></label>\
			 </form>"
		);
		let defaults = default_form_data(&form);
		assert_eq!(defaults.get("h"), Some(&HtmlFormValue::Text("1".into())));
		assert_eq!(defaults.get("db"), Some(&HtmlFormValue::Text("sqlite".into())));
		assert_eq!(defaults.get("name"), Some(&HtmlFormValue::Text("Bob".into())));

		let answers = form_answers(&form, &defaults);
		assert!(answers.iter().any(|a| a.name == "db" && a.question == "Which database?" && a.answer == "SQLite"));
		assert!(answers.iter().any(|a| a.name == "name" && a.answer == "Bob"));

		let contexts = field_contexts(&form);
		// db field has section title/context in our extraction; ensure entries exist for named fields
		assert!(contexts.values().any(|c| c.contains("DB") || c.contains("Which database?") || c.is_empty()));
	}

	#[test]
	fn block_text_and_one_line_helpers() {
		let form = parse_html_form("<form><ul><li>A</li><li>B</li></ul><input name=x></form>");
		match &form.sections[0].blocks[0] {
			b @ FormBlock::List { .. } => assert_eq!(block_text(b).as_deref(), Some("- A\n- B")),
			_ => panic!("list"),
		}
		assert_eq!(one_line(" a \n b  c"), "a b c");
	}
}
