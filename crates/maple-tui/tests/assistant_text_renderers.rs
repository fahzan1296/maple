//! Port of `l/test/assistant-text-renderers.test.ts`.
//!
//! Deviations: the reference drives native TUI actions, `TspHarness`, and
//! `logger.warn`. The crate has no TUI harness or logger here, so the port
//! covers the renderer pipeline through the terminal render and the native
//! describe text: markdown drawing, per-block streaming flags, sealed-block
//! caching, content offsets, panic fallback, live-viewport publication
//! suppression, and timestamp-scoped redraws.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use maple_tui::chat::assistant_message::AssistantMessageComponent;
use maple_tui::chat::extension_types::{AssistantTextRenderContext, AssistantTextRenderer};
use maple_tui::chat::messages::{AssistantContent, AssistantMessage, TextContent};
use maple_tui::component::Component;
use maple_tui::theme::default_theme;
use maple_tui::strip_ansi;

fn text(value: &str) -> AssistantContent {
	AssistantContent::Text(TextContent {
		text: value.to_string(),
		..TextContent::default()
	})
}

fn message(content: Vec<AssistantContent>, timestamp: i64) -> AssistantMessage {
	AssistantMessage {
		content,
		timestamp,
		..AssistantMessage::default()
	}
}

fn renderer(
	f: impl Fn(&AssistantTextRenderContext<'_>) -> Option<String> + 'static,
) -> AssistantTextRenderer {
	Box::new(f)
}

fn drawn(target: &mut AssistantMessageComponent) -> String {
	strip_ansi(&target.render(80).join("\n"))
}

/// `{{short|long}}` draws `short`; an unclosed `{{…` is withheld while streaming.
fn stretch_value(input: &str) -> String {
	let mut output = String::new();
	let bytes = input.as_bytes();
	let mut index = 0;
	while index < bytes.len() {
		if input[index..].starts_with("{{") {
			let Some(close) = input[index + 2..].find("}}") else {
				break;
			};
			let inner = &input[index + 2..index + 2 + close];
			let short = inner.split('|').next().unwrap_or(inner);
			output.push_str(short);
			index = index + 2 + close + 2;
		} else {
			let ch = input[index..].chars().next().expect("non-empty slice");
			output.push(ch);
			index += ch.len_utf8();
		}
	}
	output
}

fn stretch(context: &AssistantTextRenderContext<'_>) -> Option<String> {
	Some(stretch_value(context.text))
}

#[test]
fn draws_renderer_markdown_while_message_keeps_text_as_written() {
	let mut target = AssistantMessageComponent::new(default_theme());
	target.set_text_renderers(vec![renderer(stretch)]);
	let written = "Cache keys {{drift|drift when package.json changes}}.";
	target.update_content(message(vec![text(written)], 1), false);

	let rows = drawn(&mut target);
	assert!(rows.contains("Cache keys drift."), "rows: {rows:?}");
	assert!(!rows.contains("{{"), "rows: {rows:?}");
	assert_eq!(target.message().and_then(|m| m.content.first()), Some(&text(written)));
}

#[test]
fn reports_earlier_block_final_once_a_later_block_streams() {
	let seen = Rc::new(RefCell::new(Vec::new()));
	let record = {
		let seen = seen.clone();
		renderer(move |context| {
			seen.borrow_mut().push((context.content_index, context.streaming));
			Some(context.text.to_string())
		})
	};
	let mut target = AssistantMessageComponent::new(default_theme());
	target.set_text_renderers(vec![record]);
	target.update_content(
		message(vec![text("Closed paragraph."), text("Still streaming")], 1),
		true,
	);
	drawn(&mut target);
	assert_eq!(seen.borrow().clone(), vec![(0, false), (1, true)]);
}

#[test]
fn called_with_text_so_far_on_every_update_and_once_final() {
	let calls: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
	let record = {
		let calls = calls.clone();
		renderer(move |context| {
			calls.borrow_mut().push((context.text.to_string(), context.streaming));
			let text = context.text.replace("{{sh", "(more)").replace("{{short|lo", "(more)");
			Some(stretch_value(&text))
		})
	};
	let mut target = AssistantMessageComponent::new(default_theme());
	target.set_text_renderers(vec![record]);
	let steps = ["A {{sh", "A {{short|lo", "A {{short|long}} tail"];
	let mut shown = Vec::new();
	for step in steps {
		target.update_content(message(vec![text(step)], 1), true);
		shown.push(drawn(&mut target).trim().to_string());
	}
	target.update_content(message(vec![text(steps[2])], 1), false);
	shown.push(drawn(&mut target).trim().to_string());

	assert_eq!(
		calls.borrow().clone(),
		vec![
			(steps[0].to_string(), true),
			(steps[1].to_string(), true),
			(steps[2].to_string(), true),
			(steps[2].to_string(), false),
		]
	);
	assert_eq!(shown, ["A (more)", "A (more)", "A short tail", "A short tail"]);
}

#[test]
fn sealed_block_is_final_and_reused_until_a_redraw_asks_again() {
	let calls = Rc::new(Cell::new(0));
	let count = {
		let calls = calls.clone();
		renderer(move |context| {
			calls.set(calls.get() + 1);
			Some(context.text.to_string())
		})
	};
	let mut target = AssistantMessageComponent::new(default_theme());
	target.set_text_renderers(vec![count]);
	let sealed = message(vec![text("Reading the file first.")], 7);
	target.update_content(sealed.clone(), true);
	drawn(&mut target);
	target.mark_transcript_block_finalized();
	drawn(&mut target);
	assert_eq!(calls.get(), 2, "transient draft then sealed final");

	for _ in 0..5 {
		target.update_content(sealed.clone(), true);
		drawn(&mut target);
	}
	assert_eq!(calls.get(), 2, "sealed output is reused across later updates");

	assert!(target.rerender_message(7));
	drawn(&mut target);
	assert_eq!(calls.get(), 3, "an explicit redraw reruns the renderer");
	target.set_text_renderers(vec![renderer(move |context| {
		calls.set(calls.get() + 1);
		Some(context.text.to_string())
	})]);
	drawn(&mut target);
	assert_eq!(calls.get(), 4, "swapping renderers reruns them");
}

#[test]
fn throwing_renderer_falls_back_to_its_input_and_later_renderers_still_run() {
	let mut target = AssistantMessageComponent::new(default_theme());
	target.set_text_renderers(vec![
		renderer(|_| panic!("boom")),
		renderer(|context| Some(format!("{} (checked)", context.text))),
	]);
	target.update_content(message(vec![text("Plain reply")], 1), false);
	let rows = drawn(&mut target);
	assert!(rows.contains("Plain reply (checked)"), "rows: {rows:?}");
	assert_eq!(target.renderer_failure_count(), 1);
}

#[test]
fn content_offset_indexes_blocks_in_the_full_reply() {
	let indexes = Rc::new(RefCell::new(Vec::new()));
	let record = {
		let indexes = indexes.clone();
		renderer(move |context| {
			indexes.borrow_mut().push(context.content_index);
			stretch(context)
		})
	};
	let mut target = AssistantMessageComponent::new(default_theme());
	target.set_text_renderer_offset(2);
	target.set_text_renderers(vec![record]);
	target.update_content(message(vec![text("A {{short|long}} tail")], 1), true);
	let rows = drawn(&mut target);
	assert!(rows.contains("A short tail"), "rows: {rows:?}");
	assert_eq!(indexes.borrow().clone(), vec![2]);

	target.set_text_renderers(vec![]);
	let raw = drawn(&mut target);
	assert!(raw.contains("{{short|long}}"), "no renderers draw the text as written: {raw:?}");
}

#[test]
fn renderers_keep_the_reply_live_instead_of_publishing_rewritable_rows() {
	let mut plain = AssistantMessageComponent::new(default_theme());
	plain.update_content(message(vec![text("Paragraph one has settled.")], 1), false);
	let rows = plain.publish_stable_rows(80);
	assert!(!rows.is_empty());
	assert!(!plain.transcript_stable_rows().is_empty());

	let mut rendered = AssistantMessageComponent::new(default_theme());
	rendered.set_text_renderers(vec![renderer(|context| Some(context.text.to_string()))]);
	rendered.update_content(message(vec![text("Paragraph one has settled.")], 1), false);
	assert!(rendered.publish_stable_rows(80).is_empty());
	assert!(rendered.transcript_stable_rows().is_empty());

	plain.set_text_renderers(vec![renderer(|context| Some(context.text.to_string()))]);
	assert!(plain.transcript_stable_rows().is_empty(), "late renderers drop published rows");
}

#[test]
fn rerender_only_redraws_the_matching_timestamp() {
	let open = Rc::new(Cell::new(false));
	let toggle = {
		let open = open.clone();
		renderer(move |context| {
			let long = context.text.contains("this whole paragraph");
			Some(if open.get() && long {
				context.text.replace("{{this|this whole paragraph}}", "this whole paragraph")
			} else {
				stretch_value(context.text)
			})
		})
	};
	let mut target = AssistantMessageComponent::new(default_theme());
	target.set_text_renderers(vec![toggle]);
	target.update_content(message(vec![text("Read {{this|this whole paragraph}}.")], 42), false);
	target.mark_transcript_block_finalized();
	let rows = drawn(&mut target);
	assert!(rows.contains("Read this."), "rows: {rows:?}");

	open.set(true);
	assert!(!target.rerender_message(41));
	assert!(drawn(&mut target).contains("Read this."), "other timestamps are untouched");
	assert!(target.rerender_message(42));
	let rows = drawn(&mut target);
	assert!(rows.contains("Read this whole paragraph."), "rows: {rows:?}");
}
