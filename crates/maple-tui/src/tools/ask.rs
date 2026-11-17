//! Ask (HTML form) tool render-model types and helpers.
//!
//! Reference: `l/src/tools/ask.ts`.
//!
//! Deviations:
//! - TS renders interactive TUI components/native views with `Theme`,
//!   `framedToolCard`, status lines, tree/wrapped text rendering, and
//!   `OwnerMemo`. Rust ports argument/result data shapes, answer extraction/
//!   sanitization/masking helpers, question/summary derivation, and plain/
//!   native describe helpers.
//! - Uses crate `html_form::{parse::{parse_html_form, HtmlForm, spans_text, FormField}, data::{FormAnswer, OptionNote, field_questions}}`
//!   instead of reimplementing parsing/data logic.
//! - Interactive form rendering/dialog behavior and full theme glyphs are host
//!   concerns for renderer-completion pass.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::html_form::data::{field_questions, text_value, FormAnswer, OptionNote};
use crate::html_form::parse::{parse_html_form, spans_text, FormBlock, FormField, FormSection, HtmlForm};
use crate::native::describe::{compact, node, row, span as nspan, text as ntext};
use crate::native::node::{NodeKind, NodeText, Props, Span, Tone};
use crate::tools::native_view::{note_text, tool_head, NativeToolHead, NativeToolView, ToolHeadDetail};

/// Answers and selected-option notes displayed for an ask tool result.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskToolDetails {
	/// Each named field's question and answer text, in form order.
	#[serde(default)]
	pub answers: Vec<AskAnswer>,
	/// Notes attached to selected options.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub notes: Option<Vec<AskOptionNote>>,
	/// The deadline submitted the form's current values instead of the user.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub timed_out: Option<bool>,
}

/// Serializable answer record (compatible with `FormAnswer`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskAnswer {
	pub name: String,
	pub question: String,
	pub answer: String,
}

impl From<FormAnswer> for AskAnswer {
	fn from(value: FormAnswer) -> Self {
		Self { name: value.name, question: value.question, answer: value.answer }
	}
}

impl From<AskAnswer> for FormAnswer {
	fn from(value: AskAnswer) -> Self {
		Self { name: value.name, question: value.question, answer: value.answer }
	}
}

/// Serializable option note (compatible with `OptionNote`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskOptionNote {
	pub name: String,
	pub question: String,
	pub option: String,
	pub value: String,
	pub text: String,
}

impl From<OptionNote> for AskOptionNote {
	fn from(value: OptionNote) -> Self {
		Self { name: value.name, question: value.question, option: value.option, value: value.value, text: value.text }
	}
}

impl From<AskOptionNote> for OptionNote {
	fn from(value: AskOptionNote) -> Self {
		Self { name: value.name, question: value.question, option: value.option, value: value.value, text: value.text }
	}
}

/// Render args for ask tool call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskRenderArgs {
	/// Form HTML; may be partially streamed/malformed.
	pub html: Option<String>,
}

pub const TIMED_OUT_NOTE: &str = "auto-selected after timeout — not a user choice";
pub const NO_ANSWER: &str = "(no answer)";

/// Option marker token (plain).
pub fn option_marker_token(multi: bool, selected: bool) -> &'static str {
	match (multi, selected) {
		(true, true) => "[x]",
		(true, false) => "[ ]",
		(false, true) => "(*)",
		(false, false) => "( )",
	}
}

/// Sanitize model-authored text for a terminal row (tabs expanded, control chars stripped).
pub fn clean(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for ch in value.chars() {
		match ch {
			'\t' => out.push_str("   "),
			'\n' | '\r' => out.push(' '),
			c if c.is_control() => {}
			c => out.push(c),
		}
	}
	out
}

/// Fold cleaned text onto one line.
pub fn one_line(value: &str) -> String {
	clean(value).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse form from untrusted/streamed args; `None` until valid markup arrives.
pub fn args_form(args: Option<&AskRenderArgs>) -> Option<HtmlForm> {
	let html = args.and_then(|a| a.html.as_deref())?;
	if html.trim().is_empty() {
		return None;
	}
	Some(parse_html_form(html))
}

/// Names of password fields whose values never reach the transcript.
pub fn password_names(form: Option<&HtmlForm>) -> HashSet<String> {
	let mut names = HashSet::new();
	let Some(form) = form else { return names; };
	for field in &form.fields {
		if let FormField::Text(t) = field {
			if t.r#type == "password" {
				if !t.name.is_empty() {
					names.insert(t.name.clone());
				}
			}
		}
	}
	names
}

/// Extract well-formed answers from details value.
pub fn details_answers(details: Option<&AskToolDetails>) -> Option<Vec<FormAnswer>> {
	let details = details?;
	if details.answers.is_empty() && details.notes.is_none() && details.timed_out.is_none() {
		// allow empty-but-present? keep behavior simple
	}
	Some(details.answers.iter().cloned().map(FormAnswer::from).collect())
}

/// Extract well-formed selected-option notes from details.
pub fn details_notes(details: Option<&AskToolDetails>) -> Vec<OptionNote> {
	details
		.and_then(|d| d.notes.as_ref())
		.map(|ns| ns.iter().cloned().map(OptionNote::from).collect())
		.unwrap_or_default()
}

/// Notes for one answer.
pub fn notes_for_answer(answer: &FormAnswer, notes: &[OptionNote]) -> Vec<OptionNote> {
	notes
		.iter()
		.filter(|n| n.name == answer.name && n.question == answer.question)
		.cloned()
		.collect()
}

/// Shown text of an answer: masked for password fields, sanitized otherwise.
pub fn answer_shown(answer: &FormAnswer, passwords: &HashSet<String>) -> String {
	if passwords.contains(&answer.name) {
		let len = answer.answer.chars().count().min(12);
		"•".repeat(len)
	} else {
		clean(&answer.answer)
	}
}

/// Named field questions in form order.
pub fn named_questions(form: Option<&HtmlForm>) -> Vec<String> {
	let Some(form) = form else { return Vec::new(); };
	let qmap = field_questions(form);
	form.fields
		.iter()
		.enumerate()
		.filter_map(|(i, f)| {
			let has_name = match f {
				FormField::Choice(c) => !c.name.is_empty(),
				FormField::Text(t) => !t.name.is_empty(),
			};
			if !has_name { return None; }
			Some(qmap.get(&i).cloned().unwrap_or_else(|| match f {
				FormField::Choice(c) => c.name.clone(),
				FormField::Text(t) => t.name.clone(),
			}))
		})
		.collect()
}

/// Head for ask tool.
pub fn ask_head(question: Option<&str>, meta: Option<&str>) -> NativeToolHead {
	NativeToolHead {
		title: Some(NodeText::Plain("Ask".to_string())),
		target: question.map(|q| NodeText::Plain(one_line(q))).filter(|t| match t { NodeText::Plain(s) => !s.is_empty(), _ => true }),
		target_kind: Some("text".to_string()),
		meta: meta.map(|m| vec![NodeText::Plain(m.to_string())]),
		..NativeToolHead::default()
	}
}

/// Native describe for ask call.
pub fn describe_ask_call(args: Option<&AskRenderArgs>) -> NativeToolView {
	let form = args_form(args);
	let questions = named_questions(form.as_ref());
	let meta = if questions.len() > 1 { Some(format!("{} fields", questions.len())) } else { None };
	NativeToolView {
		tool: Some(ask_head(questions.first().map(|s| s.as_str()), meta.as_deref())),
		inline: Some(true),
		body: Some(vec![ntext(NodeText::Spans(vec![Span::styled("Waiting for your answer…".to_string(), "muted")]), Props::new())]),
		..NativeToolView::default()
	}
}

fn describe_answer_node(answer: &FormAnswer, shown: &str, key: &str, notes: &[OptionNote]) -> crate::native::node::NativeNode {
	let mut children = Vec::new();
	let mut qprops = Props::new();
	qprops.set_str("wrap", "word");
	children.push(ntext(NodeText::Spans(vec![Span::new(one_line(&answer.question))]), qprops));
	if shown.is_empty() {
		let mut p = Props::new();
		p.set_str("role", "omp.tool.answer.off");
		children.push(ntext(NodeText::Spans(vec![Span::styled(NO_ANSWER.to_string(), "dim")]), p));
	} else {
		let icon = {
			let mut ip = Props::new();
			ip.set_str("name", "check");
			ip.set_str("tone", "success");
			node(NodeKind::Icon, ip, Vec::new(), None)
		};
		let mut tp = Props::new();
		tp.set_str("wrap", "word");
		let txt = ntext(NodeText::Spans(vec![Span::styled(shown.to_string(), "toolOutput")]), tp);
		let mut rp = Props::new();
		rp.set_str("gap", "sm");
		rp.set_str("align", "baseline");
		rp.set_str("role", "omp.tool.answer");
		children.push(row(vec![icon, txt], rp));
	}
	for note in notes_for_answer(answer, notes) {
		let mut p = Props::new();
		p.set_str("wrap", "word");
		p.set_str("role", "omp.tool.answer.note");
		children.push(ntext(NodeText::Spans(vec![
			Span::styled("✎ ".to_string(), "dim"),
			Span::styled(format!("{}: {}", one_line(&note.option), clean(&note.text)), "em"),
		]), p));
	}
	let mut sp = Props::new();
	sp.set_str("id", key);
	node(NodeKind::Section, sp, children, Some(key))
}

/// Native describe for ask result.
pub fn describe_ask_result(
	details: Option<&AskToolDetails>,
	result_text_fallback: &str,
	args: Option<&AskRenderArgs>,
) -> NativeToolView {
	let answers = details_answers(details);
	let form = args_form(args);
	if answers.is_none() {
		let fallback = clean(result_text_fallback);
		return NativeToolView {
			tool: Some(ask_head(named_questions(form.as_ref()).first().map(|s| s.as_str()), None)),
			tone: Some(Tone::Warning),
			body: if fallback.is_empty() { None } else { Some(vec![note_text(&fallback, "muted", None)]) },
			..NativeToolView::default()
		};
	}
	let answers = answers.unwrap_or_default();
	let passwords = password_names(form.as_ref());
	let timed_out = details.and_then(|d| d.timed_out).unwrap_or(false);
	let notes = details_notes(details);
	let meta = if answers.len() > 1 { Some(format!("{} questions", answers.len())) } else { None };
	let mut body: Vec<crate::native::node::NativeNode> = answers
		.iter()
		.enumerate()
		.map(|(i, a)| {
			let shown = answer_shown(a, &passwords);
			describe_answer_node(a, &shown, &i.to_string(), &notes)
		})
		.collect();
	if timed_out {
		let mut p = Props::new();
		p.set_str("wrap", "word");
		p.set_str("role", "omp.tool.notice");
		body.push(ntext(NodeText::Spans(vec![Span::styled(TIMED_OUT_NOTE.to_string(), "muted")]), p));
	}
	NativeToolView {
		tool: Some(ask_head(answers.first().map(|a| a.question.as_str()), meta.as_deref())),
		tone: if timed_out { Some(Tone::Warning) } else { None },
		body: Some(body),
		..NativeToolView::default()
	}
}

/// Plain render-call summary.
pub fn render_ask_call_plain(args: Option<&AskRenderArgs>) -> String {
	let form = args_form(args);
	let qs = named_questions(form.as_ref());
	if qs.is_empty() {
		"Ask".to_string()
	} else if qs.len() == 1 {
		format!("Ask: {}", one_line(&qs[0]))
	} else {
		format!("Ask: {} · {} fields", one_line(&qs[0]), qs.len())
	}
}

/// Plain render-result lines.
pub fn render_ask_result_plain(details: Option<&AskToolDetails>, fallback: &str, args: Option<&AskRenderArgs>) -> Vec<String> {
	if let Some(answers) = details_answers(details) {
		let passwords = password_names(args_form(args).as_ref());
		let timed_out = details.and_then(|d| d.timed_out).unwrap_or(false);
		let notes = details_notes(details);
		let mut lines = Vec::new();
		for a in &answers {
			lines.push(one_line(&a.question));
			let shown = answer_shown(a, &passwords);
			if shown.is_empty() {
				lines.push(format!("  {NO_ANSWER}"));
			} else {
				for (i, l) in shown.lines().enumerate() {
					if i == 0 { lines.push(format!("  ✓ {l}")); } else { lines.push(format!("    {l}")); }
				}
			}
			for n in notes_for_answer(a, &notes) {
				lines.push(format!("  ✎ {}: {}", one_line(&n.option), clean(&n.text)));
			}
		}
		if timed_out {
			lines.push(TIMED_OUT_NOTE.to_string());
		}
		lines
	} else {
		let t = clean(fallback).trim().to_string();
		if t.is_empty() { vec!["Ask: no answer".into()] } else { vec![t] }
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn markers_and_cleaning() {
		assert_eq!(option_marker_token(false, true), "(*)");
		assert_eq!(option_marker_token(true, false), "[ ]");
		assert_eq!(clean("a\tb\nc"), "a   b c");
		assert_eq!(one_line("  a \n  b "), "a b");
	}

	#[test]
	fn form_parse_and_questions() {
		let html = r#"<form><p>Pick</p><label><input type="radio" name="c" value="x"> X</label></form>"#;
		let args = AskRenderArgs { html: Some(html.into()) };
		let form = args_form(Some(&args)).unwrap();
		let qs = named_questions(Some(&form));
		assert!(!qs.is_empty());
	}

	#[test]
	fn password_masking() {
		let mut pw = HashSet::new();
		pw.insert("secret".to_string());
		let a = FormAnswer { name: "secret".into(), question: "q".into(), answer: "abcd1234".into() };
		assert_eq!(answer_shown(&a, &pw), "••••••••");
	}

	#[test]
	fn describe_and_render_plain() {
		let call = describe_ask_call(Some(&AskRenderArgs { html: Some("<form><input name='n'></form>".into()) }));
		assert_eq!(call.inline, Some(true));
		let details = AskToolDetails {
			answers: vec![AskAnswer { name: "n".into(), question: "Name?".into(), answer: "Bob".into() }],
			notes: None,
			timed_out: Some(false),
		};
		let res = describe_ask_result(Some(&details), "", None);
		assert!(res.body.as_ref().map(|b| !b.is_empty()).unwrap_or(false));
		let lines = render_ask_result_plain(Some(&details), "", None);
		assert!(lines.iter().any(|l| l.contains("Bob")));
	}
}
