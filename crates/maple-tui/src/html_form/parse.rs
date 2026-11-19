//! Reduces an agent-written HTML `<form>` to what a terminal form draws.
//!
//! The model is deliberately small: titled sections (one per top-level
//! `<fieldset>`, plus untitled runs of loose content between them), context
//! blocks (paragraphs, headings, lists, preformatted text, tables) and fields.
//! Radios and checkboxes join one [`ChoiceField`] per `type` + `name`
//! wherever they appear, so a group split across table cells or list items is
//! still one question. Layout-only markup (`<br>` between labels, wrapper
//! `<div>`s, `style`, buttons) is dropped: the renderer owns layout, and the
//! UI owns Submit/Cancel.
//!
//! Reference: `l/src/html-form/parse.ts`.
//!
//! Deviation: the TypeScript implementation parses with `@oh-my-pi/pi-utils/dom`
//! (`parseHTML`, DOM nodes/elements). This clean-room port uses a small,
//! dependency-free tolerant HTML parser sufficient for agent-written forms and
//! walks an internal tree with equivalent behavior for the supported subset.

use std::collections::{HashMap, HashSet};

/// Inline emphasis a span keeps from `<code>`, `<strong>`/`<b>` and `<em>`/`<i>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormSpanStyle {
	Code,
	Strong,
	Em,
}

/// A run of inline text; `"\n"` inside `text` is a hard line break (`<br>`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormSpan {
	pub text: String,
	pub style: Option<FormSpanStyle>,
}

impl FormSpan {
	fn plain(text: impl Into<String>) -> Self {
		Self {
			text: text.into(),
			style: None,
		}
	}

	fn styled(text: impl Into<String>, style: FormSpanStyle) -> Self {
		Self {
			text: text.into(),
			style: Some(style),
		}
	}
}

/// One answer of a [`ChoiceField`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceOption {
	pub value: String,
	pub label: Vec<FormSpan>,
	pub checked: bool,
	pub disabled: bool,
}

/// A radio group (`multi: false`), checkbox group (`multi: true`) or `<select>` (`select: true`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceField {
	pub kind: String,
	pub name: String,
	pub multi: bool,
	/// From a `<select>`: a single select draws as a one-row dropdown instead of option rows.
	pub select: bool,
	/// The UI adds a free-text "Other" answer: set for radio and checkbox groups of
	/// two or more options that don't already offer one of their own.
	pub other: bool,
	/// The control's own label; empty for radio/checkbox groups, whose question is the surrounding text.
	pub label: Vec<FormSpan>,
	pub options: Vec<ChoiceOption>,
	pub required: bool,
}

/// A single-line `<input>` (`multiline: false`) or a `<textarea>`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextField {
	pub kind: String,
	pub name: String,
	/// The `<input type>` (`text`, `number`, `url`, …); `textarea` for textareas.
	pub r#type: String,
	pub multiline: bool,
	pub label: Vec<FormSpan>,
	pub value: String,
	pub placeholder: String,
	pub required: bool,
	pub min: Option<f64>,
	pub max: Option<f64>,
	pub pattern: Option<String>,
	pub min_length: Option<usize>,
	pub max_length: Option<usize>,
	/// Visible rows a textarea asked for (`rows`), 1 for inputs.
	pub rows: usize,
}

/// Any control the user answers.
#[derive(Clone, Debug, PartialEq)]
pub enum FormField {
	Choice(ChoiceField),
	Text(TextField),
}

impl FormField {
	pub fn kind(&self) -> &str {
		match self {
			FormField::Choice(_) => "choice",
			FormField::Text(_) => "text",
		}
	}

	pub fn name(&self) -> &str {
		match self {
			FormField::Choice(field) => &field.name,
			FormField::Text(field) => &field.name,
		}
	}

	pub fn label(&self) -> &[FormSpan] {
		match self {
			FormField::Choice(field) => &field.label,
			FormField::Text(field) => &field.label,
		}
	}

	pub fn required(&self) -> bool {
		match self {
			FormField::Choice(field) => field.required,
			FormField::Text(field) => field.required,
		}
	}
}

/// A renderable unit of a section, in document order.
#[derive(Clone, Debug, PartialEq)]
pub enum FormBlock {
	Text {
		spans: Vec<FormSpan>,
		heading: bool,
	},
	List {
		items: Vec<Vec<FormSpan>>,
		ordered: bool,
	},
	Pre {
		text: String,
	},
	Table {
		rows: Vec<Vec<Vec<FormSpan>>>,
		header_rows: usize,
	},
	/// An `<img>`: `src` is an http(s) or `data:` URL, a `file:` URL or a local path (see `media.rs`).
	Image {
		src: String,
		alt: String,
	},
	/// An inline `<svg>`, serialized whole.
	Svg {
		markup: String,
		alt: String,
	},
	/// A field. A radio/checkbox group whose later options come after other blocks of its section
	/// (text between options) has `options_end`: the index just past the last of those blocks.
	Field {
		field: FormField,
		options_end: Option<usize>,
	},
}

/// A `<fieldset>` (titled by its `<legend>`) or loose content between fieldsets (untitled).
#[derive(Clone, Debug, PartialEq)]
pub struct FormSection {
	pub title: Vec<FormSpan>,
	pub blocks: Vec<FormBlock>,
	/// A top-level `<fieldset>`, legend or not; `false` for loose content between fieldsets.
	pub fieldset: bool,
}

/// A parsed form: what to draw, the fields in document order, and `<input type="hidden">` pairs.
#[derive(Clone, Debug, PartialEq)]
pub struct HtmlForm {
	pub sections: Vec<FormSection>,
	pub fields: Vec<FormField>,
	pub hidden: Vec<(String, String)>,
}

fn skipped(tag: &str) -> bool {
	matches!(
		tag,
		"button"
			| "head"
			| "iframe"
			| "link"
			| "meta"
			| "noscript"
			| "script"
			| "style"
			| "template"
			| "title"
	)
}

fn block_tag(tag: &str) -> bool {
	matches!(
		tag,
		"address"
			| "article"
			| "aside"
			| "blockquote"
			| "dd"
			| "details"
			| "div"
			| "dl"
			| "dt"
			| "figure"
			| "figcaption"
			| "footer"
			| "header"
			| "li"
			| "main"
			| "nav"
			| "p"
			| "section"
	)
}

fn heading_tag(tag: &str) -> bool {
	matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "summary")
}

fn style_of(tag: &str) -> Option<FormSpanStyle> {
	match tag {
		"b" | "strong" => Some(FormSpanStyle::Strong),
		"code" | "kbd" | "samp" | "tt" => Some(FormSpanStyle::Code),
		"em" | "i" => Some(FormSpanStyle::Em),
		_ => None,
	}
}

fn ignored_input(input_type: &str) -> bool {
	matches!(input_type, "button" | "file" | "image" | "reset" | "submit")
}

/// Parse agent-written form HTML; content outside the first `<form>` is ignored when one exists.
pub fn parse_html_form(html: &str) -> HtmlForm {
	let root = parse_html(html);
	let form = find_first_element(&root, "form").cloned();
	let builder_root = form.map(DomNode::Element).unwrap_or(root);
	FormBuilder::new(builder_root).build()
}

/// Append `text` to `spans`, merging into the previous span when the style matches.
fn append_span(spans: &mut Vec<FormSpan>, text: &str, style: Option<FormSpanStyle>) {
	if text.is_empty() {
		return;
	}
	if let Some(last) = spans.last_mut() {
		if last.style == style {
			last.text.push_str(text);
			return;
		}
	}
	spans.push(match style {
		Some(style) => FormSpan::styled(text, style),
		None => FormSpan::plain(text),
	});
}

/// Collapse whitespace like HTML layout does: runs become one space, spaces
/// next to a hard break vanish, and the ends are trimmed. Empty spans drop.
fn normalize_spans(spans: Vec<FormSpan>) -> Vec<FormSpan> {
	let mut out = Vec::<FormSpan>::new();
	for span in spans {
		let text = collapse_layout_whitespace(&span.text);
		append_span(&mut out, &text, span.style);
	}
	if let Some(first) = out.first_mut() {
		first.text = first.text.trim_start_matches(char::is_whitespace).to_string();
	}
	if let Some(last) = out.last_mut() {
		last.text = last.text.trim_end_matches(char::is_whitespace).to_string();
	}
	out.retain(|span| !span.text.is_empty());
	out
}

fn collapse_layout_whitespace(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut pending_space = false;
	let chars: Vec<char> = text.chars().collect();
	let mut i = 0;
	while i < chars.len() {
		let ch = chars[i];
		if ch == '\n' {
			while out.ends_with(' ') {
				out.pop();
			}
			out.push('\n');
			pending_space = false;
			i += 1;
			while i < chars.len() && chars[i].is_whitespace() && chars[i] != '\n' {
				i += 1;
			}
			continue;
		}
		if ch.is_whitespace() && ch != '\n' {
			pending_space = true;
			i += 1;
			continue;
		}
		if pending_space && !out.is_empty() && !out.ends_with('\n') {
			out.push(' ');
		}
		out.push(ch);
		pending_space = false;
		i += 1;
	}
	if pending_space && !out.is_empty() && !out.ends_with('\n') {
		out.push(' ');
	}
	out
}

/// Plain text of spans, for labels that fall back to a single string.
pub fn spans_text(spans: &[FormSpan]) -> String {
	spans.iter().map(|span| span.text.as_str()).collect()
}

fn number_attr(element: &Element, name: &str) -> Option<f64> {
	let raw = element.get_attribute(name)?;
	if raw.trim().is_empty() {
		return None;
	}
	let value = raw.trim().parse::<f64>().ok()?;
	if value.is_finite() { Some(value) } else { None }
}

fn usize_attr(element: &Element, name: &str) -> Option<usize> {
	number_attr(element, name).and_then(|value| {
		if value.is_finite() && value >= 0.0 {
			Some(value.trunc() as usize)
		} else {
			None
		}
	})
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DomNode {
	Element(Element),
	Text(String),
	Comment(String),
}

impl DomNode {
	fn children(&self) -> &[DomNode] {
		match self {
			DomNode::Element(element) => &element.children,
			_ => &[],
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Element {
	tag: String,
	attrs: HashMap<String, String>,
	children: Vec<DomNode>,
}

impl Element {
	fn local_name(&self) -> &str { &self.tag }
	fn id(&self) -> Option<&str> { self.attrs.get("id").map(String::as_str) }
	fn get_attribute(&self, name: &str) -> Option<&str> {
		self.attrs.get(&name.to_ascii_lowercase()).map(String::as_str)
	}
	fn has_attribute(&self, name: &str) -> bool {
		self.attrs.contains_key(&name.to_ascii_lowercase())
	}
	fn outer_html(&self) -> String { serialize_outer_html(self) }
	fn text_content(&self) -> String {
		let mut out = String::new();
		collect_text(self, &mut out);
		out
	}
	fn query_selector_all(&self, controls: bool) -> Vec<Element> {
		let mut out = Vec::new();
		collect_query(self, controls, &mut out);
		out
	}
	fn direct_children_elements(&self) -> impl Iterator<Item = &Element> {
		self.children.iter().filter_map(|child| match child {
			DomNode::Element(element) => Some(element),
			_ => None,
		})
	}
}

fn collect_text(element: &Element, out: &mut String) {
	for child in &element.children {
		match child {
			DomNode::Text(text) => out.push_str(text),
			DomNode::Element(child) => collect_text(child, out),
			DomNode::Comment(_) => {}
		}
	}
}

fn collect_query(element: &Element, controls: bool, out: &mut Vec<Element>) {
	for child in &element.children {
		if let DomNode::Element(child) = child {
			let tag = child.local_name();
			let matched = if controls {
				matches!(tag, "input" | "select" | "textarea")
			} else {
				tag == "label"
			};
			if matched {
				out.push(child.clone());
			}
			collect_query(child, controls, out);
		}
	}
}

fn find_first_element<'a>(node: &'a DomNode, tag: &str) -> Option<&'a Element> {
	match node {
		DomNode::Element(element) => {
			if element.local_name() == tag {
				return Some(element);
			}
			for child in &element.children {
				if let Some(found) = find_first_element(child, tag) {
					return Some(found);
				}
			}
			None
		}
		_ => None,
	}
}

fn serialize_outer_html(element: &Element) -> String {
	let mut out = String::new();
	serialize_element(&DomNode::Element(element.clone()), &mut out);
	out
}

fn serialize_element(node: &DomNode, out: &mut String) {
	match node {
		DomNode::Text(text) => out.push_str(text),
		DomNode::Comment(text) => {
			out.push_str("<!--");
			out.push_str(text);
			out.push_str("-->");
		}
		DomNode::Element(element) => {
			out.push('<');
			out.push_str(&element.tag);
			let mut keys: Vec<_> = element.attrs.keys().cloned().collect();
			keys.sort();
			for key in keys {
				let value = &element.attrs[&key];
				out.push(' ');
				out.push_str(&key);
				out.push_str("=\"");
				out.push_str(&escape_attr(value));
				out.push('"');
			}
			out.push('>');
			if matches!(element.tag.as_str(), "br" | "hr" | "img" | "input" | "meta" | "link") {
				return;
			}
			for child in &element.children {
				serialize_element(child, out);
			}
			out.push_str("</");
			out.push_str(&element.tag);
			out.push('>');
		}
	}
}

fn escape_attr(value: &str) -> String {
	value
		.replace('&', "&amp;")
		.replace('"', "&quot;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
}

fn svg_title(svg: &Element) -> String {
	if let Some(label) = svg.get_attribute("aria-label") {
		return label.trim().to_string();
	}
	for child in svg.direct_children_elements() {
		if child.local_name() == "title" {
			return child.text_content().split_whitespace().collect::<Vec<_>>().join(" ");
		}
	}
	String::new()
}

#[derive(Clone)]
struct GroupEntry {
	field_index: usize,
	section_index: usize,
	block_index: usize,
}

struct FormBuilder {
	root: DomNode,
	sections: Vec<FormSection>,
	fields: Vec<FormField>,
	hidden: Vec<(String, String)>,
	groups: HashMap<String, GroupEntry>,
	label_for: HashMap<String, Element>,
	section_index: usize,
	inline: Vec<FormSpan>,
	fieldset_depth: usize,
}

impl FormBuilder {
	fn new(root: DomNode) -> Self {
		Self {
			root,
			sections: vec![FormSection {
				title: Vec::new(),
				blocks: Vec::new(),
				fieldset: false,
			}],
			fields: Vec::new(),
			hidden: Vec::new(),
			groups: HashMap::new(),
			label_for: HashMap::new(),
			section_index: 0,
			inline: Vec::new(),
			fieldset_depth: 0,
		}
	}

	fn build(mut self) -> HtmlForm {
		let controls = root_query_controls(&self.root);
		let ids: HashSet<String> = controls.iter().filter_map(|c| c.id().map(str::to_string)).collect();
		for label in root_query_labels(&self.root) {
			if let Some(target) = label.get_attribute("for") {
				if ids.contains(target) && !self.label_for.contains_key(target) {
					self.label_for.insert(target.to_string(), label);
				}
			}
		}
		self.children_node(&self.root.clone(), None);
		self.flush(false);
		for field in &mut self.fields {
			if let FormField::Choice(choice) = field {
				if !choice.select && choice.options.len() >= 2 {
					choice.other = !choice.options.iter().any(|option| {
						spans_text(&option.label)
							.trim_start()
							.to_ascii_lowercase()
							.starts_with("other")
					});
				}
			}
		}
		HtmlForm {
			sections: self
				.sections
				.into_iter()
				.filter(|section| !section.blocks.is_empty() || !section.title.is_empty())
				.collect(),
			fields: self.fields,
			hidden: self.hidden,
		}
	}

	fn children_node(&mut self, node: &DomNode, style: Option<FormSpanStyle>) {
		for child in node.children() {
			self.walk(child, style);
		}
	}

	fn children_element(&mut self, element: &Element, style: Option<FormSpanStyle>) {
		for child in &element.children {
			self.walk(child, style);
		}
	}

	fn walk(&mut self, node: &DomNode, style: Option<FormSpanStyle>) {
		match node {
			DomNode::Comment(_) => {}
			DomNode::Text(text) => append_span(&mut self.inline, text, style),
			DomNode::Element(element) => {
				let tag = element.local_name();
				if skipped(tag) {
					return;
				}
				match tag {
					"br" => append_span(&mut self.inline, "\n", None),
					"hr" => self.flush(false),
					"input" | "select" | "textarea" => self.control(element, None),
					"label" => self.label(element, style),
					"fieldset" => self.fieldset(element),
					"legend" => {
						self.flush(false);
						self.children_element(element, style);
						self.flush(true);
					}
					"ul" | "ol" => self.list(element, tag == "ol"),
					"pre" => {
						self.flush(false);
						let text = element.text_content();
						let text = text.strip_prefix('\n').unwrap_or(&text).trim_end().to_string();
						self.sections[self.section_index]
							.blocks
							.push(FormBlock::Pre { text });
					}
					"table" => self.table(element),
					"img" => {
						let Some(src) = element
							.get_attribute("src")
							.map(str::trim)
							.filter(|s| !s.is_empty())
						else {
							return;
						};
						self.flush(false);
						self.sections[self.section_index].blocks.push(FormBlock::Image {
							src: src.to_string(),
							alt: element.get_attribute("alt").unwrap_or("").trim().to_string(),
						});
					}
					"svg" => {
						self.flush(false);
						self.sections[self.section_index].blocks.push(FormBlock::Svg {
							markup: element.outer_html(),
							alt: svg_title(element),
						});
					}
					_ if heading_tag(tag) => {
						self.flush(false);
						self.children_element(element, style);
						self.flush(true);
					}
					_ if block_tag(tag) => {
						self.flush(false);
						self.children_element(element, style);
						self.flush(false);
					}
					_ => self.children_element(element, style_of(tag).or(style)),
				}
			}
		}
	}

	fn flush(&mut self, heading: bool) {
		let spans = normalize_spans(std::mem::take(&mut self.inline));
		if !spans.is_empty() {
			self.sections[self.section_index]
				.blocks
				.push(FormBlock::Text { spans, heading });
		}
	}

	fn spans_of(&self, element: &Element) -> Vec<FormSpan> {
		let mut spans = Vec::new();
		fn visit(spans: &mut Vec<FormSpan>, node: &DomNode, style: Option<FormSpanStyle>) {
			match node {
				DomNode::Comment(_) => {}
				DomNode::Text(text) => append_span(spans, text, style),
				DomNode::Element(element) => {
					let tag = element.local_name();
					if skipped(tag) || matches!(tag, "input" | "select" | "textarea" | "svg") {
						return;
					}
					if tag == "br" {
						append_span(spans, "\n", None);
						return;
					}
					if tag == "img" {
						append_span(spans, element.get_attribute("alt").unwrap_or(""), style);
						return;
					}
					if block_tag(tag) || heading_tag(tag) {
						append_span(spans, " ", None);
					}
					for child in &element.children {
						visit(spans, child, style_of(tag).or(style));
					}
				}
			}
		}
		for child in &element.children {
			visit(&mut spans, child, None);
		}
		normalize_spans(spans)
	}

	fn label(&mut self, label: &Element, style: Option<FormSpanStyle>) {
		if let Some(target) = label.get_attribute("for") {
			if self.label_for.get(target).is_some_and(|stored| stored == label) {
				return;
			}
		}
		let controls = label.query_selector_all(true);
		if controls.is_empty() {
			self.children_element(label, style);
			return;
		}
		self.flush(false);
		let spans = self.spans_of(label);
		for control in controls {
			self.control(&control, Some(spans.clone()));
		}
	}

	fn fieldset(&mut self, fieldset: &Element) {
		self.flush(false);
		let legend = fieldset
			.direct_children_elements()
			.find(|child| child.local_name() == "legend")
			.cloned();
		if self.fieldset_depth > 0 {
			if let Some(legend) = &legend {
				self.walk(&DomNode::Element(legend.clone()), None);
			}
		} else {
			let title = legend.as_ref().map(|l| self.spans_of(l)).unwrap_or_default();
			self.start_section(title, true);
		}
		self.fieldset_depth += 1;
		for child in &fieldset.children {
			if let (Some(legend), DomNode::Element(element)) = (&legend, child) {
				if element == legend {
					continue;
				}
			}
			self.walk(child, None);
		}
		self.flush(false);
		self.fieldset_depth -= 1;
		if self.fieldset_depth == 0 {
			self.start_section(Vec::new(), false);
		}
	}

	fn start_section(&mut self, title: Vec<FormSpan>, fieldset: bool) {
		if self.sections[self.section_index].blocks.is_empty()
			&& self.sections[self.section_index].title.is_empty()
		{
			self.sections[self.section_index].title = title;
			self.sections[self.section_index].fieldset = fieldset;
			return;
		}
		self.sections.push(FormSection {
			title,
			blocks: Vec::new(),
			fieldset,
		});
		self.section_index = self.sections.len() - 1;
	}

	fn list(&mut self, list: &Element, ordered: bool) {
		self.flush(false);
		if !list.query_selector_all(true).is_empty() {
			self.children_element(list, None);
			self.flush(false);
			return;
		}
		let items: Vec<Vec<FormSpan>> = list
			.direct_children_elements()
			.filter(|child| child.local_name() == "li")
			.map(|item| self.spans_of(item))
			.filter(|spans| !spans.is_empty())
			.collect();
		if !items.is_empty() {
			self.sections[self.section_index]
				.blocks
				.push(FormBlock::List { items, ordered });
		}
	}

	fn table(&mut self, table: &Element) {
		self.flush(false);
		let rows = collect_table_rows(table);
		let cells_of = |row: &Element| -> Vec<Element> {
			row.direct_children_elements()
				.filter(|cell| matches!(cell.local_name(), "td" | "th"))
				.cloned()
				.collect()
		};
		if table.query_selector_all(true).is_empty() {
			let cells: Vec<Vec<Vec<FormSpan>>> = rows
				.iter()
				.map(|row| cells_of(row).iter().map(|cell| self.spans_of(cell)).collect())
				.collect();
			let mut header_rows = 0;
			while header_rows < rows.len() {
				let row = &rows[header_rows];
				let in_thead = has_ancestor_tag(&DomNode::Element(table.clone()), row, "thead");
				let all_th = {
					let cells = cells_of(row);
					!cells.is_empty() && cells.iter().all(|cell| cell.local_name() == "th")
				};
				if in_thead || all_th {
					header_rows += 1;
				} else {
					break;
				}
			}
			if cells.iter().any(|row| !row.is_empty()) {
				self.sections[self.section_index].blocks.push(FormBlock::Table {
					rows: cells,
					header_rows,
				});
			}
			return;
		}
		let mut header: Vec<Vec<FormSpan>> = Vec::new();
		for row in &rows {
			let cells = cells_of(row);
			let all_th_no_controls = cells.iter().all(|cell| cell.local_name() == "th")
				&& row.query_selector_all(true).is_empty();
			if all_th_no_controls {
				header = cells.iter().map(|cell| self.spans_of(cell)).collect();
				continue;
			}
			let mut text: Vec<FormSpan> = Vec::new();
			for cell in &cells {
				if !cell.query_selector_all(true).is_empty() {
					continue;
				}
				let spans = self.spans_of(cell);
				if spans.is_empty() {
					continue;
				}
				if !text.is_empty() {
					append_span(&mut text, " · ", None);
				}
				for span in spans {
					append_span(&mut text, &span.text, span.style);
				}
			}
			if !text.is_empty() {
				self.sections[self.section_index]
					.blocks
					.push(FormBlock::Text { spans: text, heading: false });
			}
			for (column, cell) in cells.iter().enumerate() {
				for control in cell.query_selector_all(true) {
					let own = find_wrapping_label(cell, &control);
					let spans = own
						.as_ref()
						.map(|label| self.spans_of(label))
						.filter(|spans| !spans.is_empty())
						.or_else(|| {
							let cell_spans = self.spans_of(cell);
							if cell_spans.is_empty() { None } else { Some(cell_spans) }
						})
						.or_else(|| header.get(column).cloned())
						.unwrap_or_default();
					self.control(&control, Some(spans));
				}
			}
		}
	}

	fn detached_label(&self, control: &Element) -> Vec<FormSpan> {
		if let Some(id) = control.id() {
			if let Some(label) = self.label_for.get(id) {
				return self.spans_of(label);
			}
		}
		let text = control
			.get_attribute("aria-label")
			.or_else(|| control.get_attribute("title"));
		match text {
			Some(text) => normalize_spans(vec![FormSpan::plain(text)]),
			None => Vec::new(),
		}
	}

	fn control(&mut self, control: &Element, wrapping: Option<Vec<FormSpan>>) {
		self.flush(false);
		let label = match wrapping {
			Some(spans) if !spans.is_empty() => spans,
			_ => self.detached_label(control),
		};
		let name = control.get_attribute("name").unwrap_or("").to_string();
		let required = control.has_attribute("required");
		let tag = control.local_name();
		if tag == "select" {
			let multi = control.has_attribute("multiple");
			let mut options: Vec<ChoiceOption> = collect_options(control)
				.into_iter()
				.map(|option| {
					let text = option.text_content().split_whitespace().collect::<Vec<_>>().join(" ");
					ChoiceOption {
						value: option.get_attribute("value").unwrap_or(&text).to_string(),
						label: vec![FormSpan::plain(text)],
						checked: option.has_attribute("selected"),
						disabled: option.has_attribute("disabled"),
					}
				})
				.collect();
			if !multi && !options.iter().any(|option| option.checked) {
				if let Some(first) = options.iter_mut().find(|option| !option.disabled) {
					first.checked = true;
				}
			}
			self.add_field(FormField::Choice(ChoiceField {
				kind: "choice".to_string(),
				name,
				multi,
				select: true,
				other: false,
				label,
				options,
				required,
			}));
			return;
		}
		if tag == "textarea" {
			let value = control.text_content();
			let value = value
				.strip_prefix("\r\n")
				.or_else(|| value.strip_prefix('\n'))
				.unwrap_or(&value)
				.to_string();
			self.add_field(FormField::Text(TextField {
				kind: "text".to_string(),
				name: name.clone(),
				r#type: "textarea".to_string(),
				multiline: true,
				label: if label.is_empty() { self.name_label(&name) } else { label },
				value,
				placeholder: control.get_attribute("placeholder").unwrap_or("").to_string(),
				required,
				min: None,
				max: None,
				pattern: None,
				min_length: usize_attr(control, "minlength"),
				max_length: usize_attr(control, "maxlength"),
				rows: usize_attr(control, "rows").unwrap_or(4),
			}));
			return;
		}
		let input_type = control.get_attribute("type").unwrap_or("text").to_ascii_lowercase();
		if ignored_input(&input_type) {
			return;
		}
		let value = control.get_attribute("value").unwrap_or("").to_string();
		if input_type == "hidden" {
			if !name.is_empty() {
				self.hidden.push((name, value));
			}
			return;
		}
		if input_type == "radio" || input_type == "checkbox" {
			let option = ChoiceOption {
				value: if value.is_empty() { "on".to_string() } else { value },
				label: if label.is_empty() {
					let text = control.get_attribute("value").unwrap_or(&name);
					vec![FormSpan::plain(text)]
				} else {
					label
				},
				checked: control.has_attribute("checked"),
				disabled: control.has_attribute("disabled"),
			};
			let key = format!("{input_type}\0{name}");
			if let Some(entry) = self.groups.get(&key).cloned() {
				if let Some(FormField::Choice(field)) = self.fields.get_mut(entry.field_index) {
					field.options.push(option);
					field.required = field.required || required;
				}
				if entry.section_index == self.section_index {
					let block_count = self.sections[self.section_index].blocks.len();
					let last_index = block_count.saturating_sub(1);
					if last_index != entry.block_index {
						if let Some(FormBlock::Field { options_end, .. }) =
							self.sections[self.section_index].blocks.get_mut(entry.block_index)
						{
							*options_end = Some(block_count);
						}
					}
				}
				return;
			}
			let field = FormField::Choice(ChoiceField {
				kind: "choice".to_string(),
				name: name.clone(),
				multi: input_type == "checkbox",
				select: false,
				other: false,
				label: Vec::new(),
				options: vec![option],
				required,
			});
			let block_index = self.add_field(field);
			self.groups.insert(
				key,
				GroupEntry {
					field_index: self.fields.len() - 1,
					section_index: self.section_index,
					block_index,
				},
			);
			return;
		}
		self.add_field(FormField::Text(TextField {
			kind: "text".to_string(),
			name: name.clone(),
			r#type: input_type,
			multiline: false,
			label: if label.is_empty() { self.name_label(&name) } else { label },
			value,
			placeholder: control.get_attribute("placeholder").unwrap_or("").to_string(),
			required,
			min: number_attr(control, "min"),
			max: number_attr(control, "max"),
			pattern: control.get_attribute("pattern").map(str::to_string),
			min_length: usize_attr(control, "minlength"),
			max_length: usize_attr(control, "maxlength"),
			rows: 1,
		}));
	}

	fn name_label(&self, name: &str) -> Vec<FormSpan> {
		if name.is_empty() {
			Vec::new()
		} else {
			vec![FormSpan::plain(name.replace(['_', '-'], " "))]
		}
	}

	fn add_field(&mut self, field: FormField) -> usize {
		self.fields.push(field.clone());
		let index = self.sections[self.section_index].blocks.len();
		self.sections[self.section_index]
			.blocks
			.push(FormBlock::Field { field, options_end: None });
		index
	}
}

fn root_query_controls(root: &DomNode) -> Vec<Element> {
	match root {
		DomNode::Element(element) => element.query_selector_all(true),
		_ => Vec::new(),
	}
}

fn root_query_labels(root: &DomNode) -> Vec<Element> {
	match root {
		DomNode::Element(element) => element.query_selector_all(false),
		_ => Vec::new(),
	}
}

fn collect_table_rows(table: &Element) -> Vec<Element> {
	let mut out = Vec::new();
	fn collect(node: &DomNode, out: &mut Vec<Element>) {
		if let DomNode::Element(element) = node {
			for child in &element.children {
				if let DomNode::Element(child_el) = child {
					if child_el.local_name() == "tr" {
						out.push(child_el.clone());
						collect(child, out);
					} else if child_el.local_name() == "table" {
						// nested table's trs are not ours
					} else {
						collect(child, out);
					}
				}
			}
		}
	}
	collect(&DomNode::Element(table.clone()), &mut out);
	out
}

fn has_ancestor_tag(root: &DomNode, target: &Element, tag: &str) -> bool {
	fn search(node: &DomNode, target: &Element, tag: &str, in_tag: bool) -> bool {
		if let DomNode::Element(element) = node {
			let now_in = in_tag || element.local_name() == tag;
			if element == target {
				return now_in;
			}
			for child in &element.children {
				if search(child, target, tag, now_in) {
					return true;
				}
			}
		}
		false
	}
	search(root, target, tag, false)
}

fn collect_options(select: &Element) -> Vec<Element> {
	fn visit(node: &DomNode, out: &mut Vec<Element>) {
		if let DomNode::Element(element) = node {
			if element.local_name() == "option" {
				out.push(element.clone());
			}
			for child in &element.children {
				visit(child, out);
			}
		}
	}
	let mut out = Vec::new();
	visit(&DomNode::Element(select.clone()), &mut out);
	out
}

fn find_wrapping_label(cell: &Element, control: &Element) -> Option<Element> {
	fn visit(node: &DomNode, control: &Element) -> Option<Element> {
		if let DomNode::Element(element) = node {
			if element.local_name() == "label" && contains_element(element, control) {
				return Some(element.clone());
			}
			for child in &element.children {
				if let Some(found) = visit(child, control) {
					return Some(found);
				}
			}
		}
		None
	}
	visit(&DomNode::Element(cell.clone()), control)
}

fn contains_element(haystack: &Element, needle: &Element) -> bool {
	if haystack == needle {
		return true;
	}
	for child in &haystack.children {
		if let DomNode::Element(child) = child {
			if contains_element(child, needle) {
				return true;
			}
		}
	}
	false
}

// --- Minimal tolerant HTML parser ---

fn parse_html(html: &str) -> DomNode {
	let mut parser = HtmlParser::new(html);
	let children = parser.parse_nodes(None);
	DomNode::Element(Element {
		tag: "#document".to_string(),
		attrs: HashMap::new(),
		children,
	})
}

struct HtmlParser<'a> {
	src: &'a [u8],
	i: usize,
}

impl<'a> HtmlParser<'a> {
	fn new(src: &'a str) -> Self { Self { src: src.as_bytes(), i: 0 } }
	fn eof(&self) -> bool { self.i >= self.src.len() }
	fn peek(&self) -> Option<u8> { self.src.get(self.i).copied() }
	fn starts_with(&self, s: &str) -> bool {
		self.src.get(self.i..).is_some_and(|rest| rest.starts_with(s.as_bytes()))
	}
	fn starts_with_ci(&self, s: &str) -> bool {
		let bytes = s.as_bytes();
		let Some(rest) = self.src.get(self.i..self.i + bytes.len()) else { return false };
		rest.iter().zip(bytes).all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
	}
	fn bump(&mut self) -> Option<u8> {
		let b = self.peek()?;
		self.i += 1;
		Some(b)
	}

	fn parse_nodes(&mut self, stop_tag: Option<&str>) -> Vec<DomNode> {
		let mut nodes = Vec::new();
		while !self.eof() {
			if let Some(stop) = stop_tag {
				if self.starts_with("</") {
					let save = self.i;
					self.i += 2;
					let name = self.parse_name();
					self.skip_ws();
					if self.peek() == Some(b'>') { self.bump(); } else { self.i = save + 2; }
					if name.eq_ignore_ascii_case(stop) {
						return nodes;
					}
					self.i = save;
				}
			}
			if self.starts_with("<!--") {
				nodes.push(self.parse_comment());
				continue;
			}
			if self.peek() == Some(b'<') {
				if self.starts_with("</") {
					if stop_tag.is_some() {
						return nodes;
					}
					self.i += 2;
					let _ = self.parse_name();
					self.skip_ws();
					if self.peek() == Some(b'>') { self.bump(); }
					continue;
				}
				if let Some(el) = self.parse_element() {
					nodes.push(el);
				} else {
					nodes.push(DomNode::Text("<".to_string()));
					self.bump();
				}
				continue;
			}
			nodes.push(self.parse_text());
		}
		nodes
	}

	fn parse_comment(&mut self) -> DomNode {
		self.i += 4;
		let start = self.i;
		while !self.eof() && !self.starts_with("-->") { self.i += 1; }
		let text = String::from_utf8_lossy(&self.src[start..self.i]).to_string();
		if self.starts_with("-->") { self.i += 3; }
		DomNode::Comment(text)
	}

	fn parse_text(&mut self) -> DomNode {
		let start = self.i;
		while !self.eof() && self.peek() != Some(b'<') { self.i += 1; }
		let raw = String::from_utf8_lossy(&self.src[start..self.i]).to_string();
		DomNode::Text(decode_entities(&raw))
	}

	fn parse_element(&mut self) -> Option<DomNode> {
		let save = self.i;
		self.i += 1;
		if self.peek().is_some_and(|b| b == b'!' || b == b'?' || b.is_ascii_digit()) {
			while !self.eof() && self.peek() != Some(b'>') { self.i += 1; }
			if self.peek() == Some(b'>') { self.bump(); }
			return None;
		}
		let tag = self.parse_name();
		if tag.is_empty() {
			self.i = save;
			return None;
		}
		let tag_lc = tag.to_ascii_lowercase();
		let attrs = self.parse_attrs();
		self.skip_ws();
		let self_close = if self.peek() == Some(b'/') { self.i += 1; true } else { false };
		if self.peek() == Some(b'>') { self.bump(); }
		if self_close || is_void(&tag_lc) {
			return Some(DomNode::Element(Element { tag: tag_lc, attrs, children: Vec::new() }));
		}
		let children = if tag_lc == "script" || tag_lc == "style" || tag_lc == "textarea" {
			self.parse_raw_text(&tag_lc)
		} else {
			self.parse_nodes(Some(&tag_lc))
		};
		Some(DomNode::Element(Element { tag: tag_lc, attrs, children }))
	}

	fn parse_raw_text(&mut self, tag: &str) -> Vec<DomNode> {
		let start = self.i;
		let close = format!("</{tag}");
		while !self.eof() {
			if self.starts_with_ci(&close) {
				let text = String::from_utf8_lossy(&self.src[start..self.i]).to_string();
				self.i += 2 + tag.len();
				self.skip_ws();
				if self.peek() == Some(b'>') { self.bump(); }
				return if text.is_empty() { Vec::new() } else { vec![DomNode::Text(text)] };
			}
			self.i += 1;
		}
		let text = String::from_utf8_lossy(&self.src[start..self.i]).to_string();
		if text.is_empty() { Vec::new() } else { vec![DomNode::Text(text)] }
	}

	fn parse_name(&mut self) -> String {
		let start = self.i;
		while let Some(b) = self.peek() {
			if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' { self.i += 1; } else { break; }
		}
		String::from_utf8_lossy(&self.src[start..self.i]).to_string()
	}

	fn parse_attrs(&mut self) -> HashMap<String, String> {
		let mut attrs = HashMap::new();
		loop {
			self.skip_ws();
			match self.peek() {
				None | Some(b'>') | Some(b'/') => break,
				_ => {}
			}
			let name = self.parse_attr_name();
			if name.is_empty() { self.bump(); continue; }
			self.skip_ws();
			let value = if self.peek() == Some(b'=') {
				self.bump();
				self.skip_ws();
				self.parse_attr_value()
			} else {
				String::new()
			};
			attrs.entry(name.to_ascii_lowercase()).or_insert(value);
		}
		attrs
	}

	fn parse_attr_name(&mut self) -> String {
		let start = self.i;
		while let Some(b) = self.peek() {
			if b.is_ascii_whitespace() || b == b'=' || b == b'>' || b == b'/' { break; }
			self.i += 1;
		}
		String::from_utf8_lossy(&self.src[start..self.i]).to_string()
	}

	fn parse_attr_value(&mut self) -> String {
		match self.peek() {
			Some(b'"') => {
				self.bump();
				let start = self.i;
				while !self.eof() && self.peek() != Some(b'"') { self.i += 1; }
				let v = String::from_utf8_lossy(&self.src[start..self.i]).to_string();
				if self.peek() == Some(b'"') { self.bump(); }
				decode_entities(&v)
			}
			Some(b'\'') => {
				self.bump();
				let start = self.i;
				while !self.eof() && self.peek() != Some(b'\'') { self.i += 1; }
				let v = String::from_utf8_lossy(&self.src[start..self.i]).to_string();
				if self.peek() == Some(b'\'') { self.bump(); }
				decode_entities(&v)
			}
			_ => {
				let start = self.i;
				while let Some(b) = self.peek() {
					if b.is_ascii_whitespace() || b == b'>' { break; }
					self.i += 1;
				}
				let v = String::from_utf8_lossy(&self.src[start..self.i]).to_string();
				decode_entities(&v)
			}
		}
	}

	fn skip_ws(&mut self) {
		while self.peek().is_some_and(|b| b.is_ascii_whitespace()) { self.i += 1; }
	}
}

fn is_void(tag: &str) -> bool {
	matches!(tag, "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "param" | "source" | "track" | "wbr")
}

fn decode_entities(text: &str) -> String {
	text.replace("&amp;", "&")
		.replace("&lt;", "<")
		.replace("&gt;", ">")
		.replace("&quot;", "\"")
		.replace("&#39;", "'")
		.replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn plain(text: &str) -> FormSpan { FormSpan::plain(text) }

	#[test]
	fn parses_radio_group_and_preselected() {
		let form = parse_html_form(
			"<form><fieldset><legend>Database</legend>\
			 <label><input type=\"radio\" name=\"db\" value=\"sqlite\" checked> SQLite</label>\
			 <label><input type=\"radio\" name=\"db\" value=\"pg\"> Postgres</label>\
			 </fieldset></form>",
		);
		assert_eq!(form.sections.len(), 1);
		assert_eq!(spans_text(&form.sections[0].title), "Database");
		assert!(form.sections[0].fieldset);
		assert_eq!(form.fields.len(), 1);
		match &form.fields[0] {
			FormField::Choice(field) => {
				assert_eq!(field.kind, "choice");
				assert_eq!(field.name, "db");
				assert!(!field.multi);
				assert!(!field.select);
				assert!(field.other);
				assert_eq!(field.options.len(), 2);
				assert_eq!(field.options[0].value, "sqlite");
				assert!(field.options[0].checked);
				assert_eq!(spans_text(&field.options[0].label), "SQLite");
				assert_eq!(field.options[1].value, "pg");
				assert!(!field.options[1].checked);
			}
			other => panic!("expected choice field, got {other:?}"),
		}
	}

	#[test]
	fn joins_split_checkbox_group_and_marks_options_end() {
		let html = "<form><fieldset><legend>F</legend>\
			<label><input type=\"checkbox\" name=\"x\" value=\"a\"> A</label>\
			<p>between</p>\
			<label><input type=\"checkbox\" name=\"x\" value=\"b\"> B</label>\
			</fieldset></form>";
		let form = parse_html_form(html);
		assert_eq!(form.fields.len(), 1);
		let block = form.sections[0]
			.blocks
			.iter()
			.find_map(|b| match b { FormBlock::Field { field, options_end } => Some((field, *options_end)), _ => None })
			.unwrap();
		match block.0 {
			FormField::Choice(f) => {
				assert!(f.multi);
				assert_eq!(f.options.len(), 2);
			}
			_ => panic!("choice"),
		}
		assert_eq!(block.1, Some(form.sections[0].blocks.len()));
	}

	#[test]
	fn parses_text_inputs_textarea_select_and_hidden() {
		let html = "<form>\
			<input type=\"hidden\" name=\"h\" value=\"v\">\
			<label for=\"n\">Name</label><input id=\"n\" name=\"name\" type=\"text\" value=\"A\" required minlength=\"2\">\
			<textarea name=\"bio\" rows=\"5\">hi</textarea>\
			<select name=\"c\"><option value=\"1\">One</option><option value=\"2\" selected>Two</option></select>\
			</form>";
		let form = parse_html_form(html);
		assert_eq!(form.hidden, vec![("h".to_string(), "v".to_string())]);
		assert_eq!(form.fields.len(), 3);
		match &form.fields[0] {
			FormField::Text(f) => {
				assert_eq!(f.name, "name");
				assert_eq!(f.r#type, "text");
				assert!(!f.multiline);
				assert_eq!(spans_text(&f.label), "Name");
				assert_eq!(f.value, "A");
				assert!(f.required);
				assert_eq!(f.min_length, Some(2));
				assert_eq!(f.rows, 1);
			}
			_ => panic!("text"),
		}
		match &form.fields[1] {
			FormField::Text(f) => {
				assert_eq!(f.name, "bio");
				assert_eq!(f.r#type, "textarea");
				assert!(f.multiline);
				assert_eq!(f.value, "hi");
				assert_eq!(f.rows, 5);
			}
			_ => panic!("textarea"),
		}
		match &form.fields[2] {
			FormField::Choice(f) => {
				assert!(f.select);
				assert!(!f.multi);
				assert_eq!(f.options.len(), 2);
				assert!(!f.options[0].checked);
				assert!(f.options[1].checked);
			}
			_ => panic!("select"),
		}
	}

	#[test]
	fn parses_blocks_table_list_pre_image_svg_and_spans() {
		let html = "<form><h2>Title</h2><p>Hello <strong>world</strong><br>and <code>x</code></p>\
			<ul><li>One</li><li>Two</li></ul>\
			<pre>  code\n</pre>\
			<table><tr><th>A</th></tr><tr><td>B</td></tr></table>\
			<img src=\"x.png\" alt=\"pic\">\
			<svg aria-label=\"chart\"><title>t</title></svg>\
			<input name=\"n\"></form>";
		let form = parse_html_form(html);
		let blocks = &form.sections[0].blocks;
		assert!(matches!(blocks[0], FormBlock::Text { heading: true, .. }));
		match &blocks[1] {
			FormBlock::Text { spans, heading } => {
				assert!(!heading);
				assert_eq!(spans_text(spans), "Hello world\nand x");
				assert!(spans.iter().any(|s| s.style == Some(FormSpanStyle::Strong) && s.text == "world"));
				assert!(spans.iter().any(|s| s.style == Some(FormSpanStyle::Code) && s.text == "x"));
			}
			_ => panic!("text"),
		}
		match &blocks[2] {
			FormBlock::List { items, ordered } => {
				assert!(!ordered);
				assert_eq!(items.len(), 2);
				assert_eq!(spans_text(&items[0]), "One");
			}
			_ => panic!("list"),
		}
		match &blocks[3] {
			FormBlock::Pre { text } => assert_eq!(text, "  code"),
			_ => panic!("pre"),
		}
		match &blocks[4] {
			FormBlock::Table { rows, header_rows } => {
				assert_eq!(*header_rows, 1);
				assert_eq!(rows.len(), 2);
				assert_eq!(spans_text(&rows[0][0]), "A");
				assert_eq!(spans_text(&rows[1][0]), "B");
			}
			_ => panic!("table"),
		}
		match &blocks[5] {
			FormBlock::Image { src, alt } => {
				assert_eq!(src, "x.png");
				assert_eq!(alt, "pic");
			}
			_ => panic!("img"),
		}
		match &blocks[6] {
			FormBlock::Svg { markup, alt } => {
				assert!(markup.contains("<svg"));
				assert_eq!(alt, "chart");
			}
			_ => panic!("svg"),
		}
	}

	#[test]
	fn ignores_content_outside_first_form() {
		let form = parse_html_form("<div>out</div><form><p>in</p><input name=\"x\"></form><p>after</p>");
		match &form.sections[0].blocks[0] {
			FormBlock::Text { spans, .. } => assert_eq!(spans_text(spans), "in"),
			other => panic!("not text: {other:?}"),
		}
		assert_eq!(form.fields.len(), 1);
	}

	#[test]
	fn normalizes_whitespace_and_drops_empty_spans() {
		let spans = normalize_spans(vec![plain("  hello \n  world  "), plain("")]);
		assert_eq!(spans, vec![plain("hello\nworld")]);
	}
}
