//! Port of `l/test/timeline-picker-native.test.ts` (portable subset).
//!
//! The reference renders a timeline/turn picker sheet and inspects its native
//! rows. The crate ports the data model (`copy_selector`): turn summaries,
//! timeline items, block extraction and a state-driven selector component, so
//! these tests assert those contracts plus the component's copy/cancel keys and
//! its `rows` native description.

use std::cell::RefCell;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NativeNode};
use maple_tui::overlays::copy_selector::{
	block_caption, collect_blocks, entry_time_ms, extract_blocks, extract_links, first_line,
	target_copy, timeline_item, tool_call_label, turn_summary, CopyBlock, CopyBlockKind,
	CopySelectorCallbacks, CopySelectorComponent, OutlineTarget, TIMELINE_COLUMNS, TimelineItem,
	TimelineItems, TranscriptEntry, TurnSummary,
};

fn entry(id: &str, role: &str, content: &str) -> TranscriptEntry {
	TranscriptEntry {
		id: id.to_string(),
		role: role.to_string(),
		content: content.to_string(),
		..TranscriptEntry::default()
	}
}

fn user_target(id: &str, content: &str) -> OutlineTarget {
	OutlineTarget {
		turn_id: id.to_string(),
		entries: vec![TranscriptEntry {
			is_user_turn: true,
			..entry(id, "user", content)
		}],
		is_user_turn: true,
	}
}

fn assistant_target(id: &str, content: &str) -> OutlineTarget {
	OutlineTarget {
		turn_id: id.to_string(),
		entries: vec![entry(id, "assistant", content)],
		is_user_turn: false,
	}
}

fn node_text(node: &NativeNode) -> String {
	let mut out = String::new();
	if let Some(text) = node.props.text() {
		out.push_str(text);
	}
	if let Some(spans) = node.props.spans() {
		for span in spans {
			out.push_str(&span.t);
		}
	}
	for child in &node.children {
		out.push_str(&node_text(child));
	}
	out
}

#[test]
fn timeline_columns_are_dim_and_item_nodes_carry_turn_kind_and_time() {
	assert_eq!(TIMELINE_COLUMNS.len(), 1);
	assert_eq!(TIMELINE_COLUMNS[0].id, "at");
	assert_eq!(TIMELINE_COLUMNS[0].format, "dim");
	assert_eq!(TIMELINE_COLUMNS[0].priority, 1);

	assert_eq!(entry_time_ms(Some(0)), "00:00");
	assert_eq!(entry_time_ms(Some(3_661_000)), "01:01");
	assert_eq!(entry_time_ms(Some(86_399_000)), "23:59");
	assert_eq!(entry_time_ms(None), "");
	assert_eq!(entry_time_ms(Some(-1)), "");

	let targets = vec![
		user_target("u1", "hello"),
		assistant_target("a1", "thinking out loud"),
		OutlineTarget {
			turn_id: "t1".to_string(),
			entries: vec![TranscriptEntry {
				tool_name: Some("read".to_string()),
				..entry("t1", "toolResult", "file body")
			}],
			is_user_turn: false,
		},
	];
	let mut cache = TimelineItems::default();
	let items: &[TimelineItem] = cache.of(&targets);
	assert_eq!(items.len(), 3);
	assert_eq!(items[0].node, "user");
	assert_eq!(items[0].at.as_deref(), None);
	assert_eq!(items[1].node, "assistant");
	assert_eq!(items[2].node, "tool");
	assert_eq!(items[2].role.as_deref(), Some("omp.tool.read"));
	assert_eq!(items[0].id, "u1");

	let stamped = OutlineTarget {
		turn_id: "u2".to_string(),
		entries: vec![TranscriptEntry {
			timestamp: Some(3_661_000),
			is_user_turn: true,
			..entry("u2", "user", "later")
		}],
		is_user_turn: true,
	};
	let item = timeline_item(&stamped);
	assert_eq!(item.at.as_deref(), Some("01:01"));
	assert_eq!(item.label, "later");
}

#[test]
fn turn_summary_and_tool_call_label_cover_every_role() {
	let summary = turn_summary(&entry("1", "user", "hello\nworld"));
	assert_eq!(summary, TurnSummary { label: "hello".to_string(), role: "omp.user".to_string() });

	let summary = turn_summary(&entry("2", "assistant", "\n\n"));
	assert_eq!(summary.label, "thinking");
	assert_eq!(summary.role, "omp.assistant");

	let summary = turn_summary(&TranscriptEntry {
		tool_name: Some("grep".to_string()),
		..entry("3", "toolResult", "hits")
	});
	assert_eq!(summary.label, "grep result");
	assert_eq!(summary.role, "omp.tool.grep");

	let summary = turn_summary(&TranscriptEntry {
		command: Some("ls -la".to_string()),
		..entry("4", "bashExecution", "")
	});
	assert_eq!(summary.label, "$ ls -la");
	assert_eq!(summary.role, "omp.tool.bash");

	let summary = turn_summary(&entry("5", "compactionSummary", ""));
	assert_eq!(summary, TurnSummary { label: "Compaction summary".to_string(), role: "omp.summary".to_string() });

	let mut args = std::collections::BTreeMap::new();
	args.insert("file_path".to_string(), "src/main.rs".to_string());
	assert_eq!(tool_call_label("read", Some(&args)), "read src/main.rs");
	args.insert("command".to_string(), "   ".to_string());
	assert_eq!(tool_call_label("bash", Some(&args)), "read src/main.rs");
	assert_eq!(tool_call_label("read", None), "read");
	assert_eq!(first_line("  \n\tfirst  second \nthird"), "first second");
}

#[test]
fn collect_blocks_groups_code_quotes_links_commands_and_outputs() {
	let assistant = entry(
		"a1",
		"assistant",
		"```rs\nfn a() {}\n```\n> quoted\nsee https://example.com/docs",
	);
	let blocks = collect_blocks(&[assistant]);
	assert_eq!(blocks.len(), 3);
	assert!(blocks.iter().any(|block| {
		block.kind == Some(CopyBlockKind::Code)
			&& block.language.as_deref() == Some("rs")
			&& block.content == "fn a() {}"
	}));
	assert!(blocks.iter().any(|block| block.kind == Some(CopyBlockKind::Quote) && block.content == "quoted"));
	assert!(blocks.iter().any(|block| {
		block.kind == Some(CopyBlockKind::Link) && block.href.as_deref() == Some("https://example.com/docs")
	}));

	let bash = TranscriptEntry {
		command: Some("ls -la".to_string()),
		..entry("b1", "bashExecution", "total 0")
	};
	let blocks = collect_blocks(&[bash]);
	assert_eq!(blocks.len(), 2);
	assert_eq!(blocks[0].kind, Some(CopyBlockKind::Command));
	assert_eq!(blocks[0].language.as_deref(), Some("bash"));
	assert_eq!(blocks[1].kind, Some(CopyBlockKind::Text));

	let python = TranscriptEntry {
		code: Some("print(1)".to_string()),
		..entry("p1", "pythonExecution", "1")
	};
	let blocks = collect_blocks(&[python]);
	assert_eq!(blocks[0].kind, Some(CopyBlockKind::Command));
	assert_eq!(blocks[0].language.as_deref(), Some("python"));

	let empty = entry("t1", "toolResult", "   ");
	assert!(collect_blocks(&[empty]).is_empty());

	let links = extract_links("a https://x.y, b http://z.tv)");
	assert_eq!(links.len(), 2);
	assert_eq!(links[0].1, "https://x.y");
}

#[test]
fn block_caption_names_language_and_line_count() {
	let code = CopyBlock {
		label: "code".to_string(),
		content: "a\nb".to_string(),
		kind: Some(CopyBlockKind::Code),
		language: Some("rust".to_string()),
		..CopyBlock::default()
	};
	assert_eq!(block_caption(&code), "rust · 2 lines");
	let quote = CopyBlock {
		label: "quote".to_string(),
		content: "one".to_string(),
		kind: Some(CopyBlockKind::Quote),
		..CopyBlock::default()
	};
	assert_eq!(block_caption(&quote), "quote");
}

#[test]
fn target_copy_prefers_turn_content_then_the_first_block() {
	let target = user_target("u1", "hello");
	assert_eq!(target_copy(&target, &[]), ("hello".to_string(), "turn".to_string()));

	let empty_first = OutlineTarget {
		turn_id: "e1".to_string(),
		entries: vec![
			entry("e0", "toolResult", ""),
			TranscriptEntry {
				tool_name: Some("read".to_string()),
				..entry("e1", "toolResult", "body")
			},
		],
		is_user_turn: false,
	};
	let blocks = collect_blocks(&empty_first.entries);
	assert_eq!(target_copy(&empty_first, &blocks), ("body".to_string(), "read result".to_string()));

	let nothing = OutlineTarget::default();
	assert_eq!(target_copy(&nothing, &[]), (String::new(), "empty".to_string()));
}

#[test]
fn component_moves_copies_cancels_and_describes_rows() {
	let targets = vec![
		user_target("u1", "hello"),
		assistant_target("a1", "```rs\nfn a() {}\n```"),
	];
	let copies = std::rc::Rc::new(RefCell::new(Vec::new()));
	let cancelled = std::rc::Rc::new(RefCell::new(false));
	let copy_sink = std::rc::Rc::clone(&copies);
	let cancel_sink = std::rc::Rc::clone(&cancelled);
	let callbacks = Copies {
		copy_sink,
		cancel_sink,
	};
	let mut component = CopySelectorComponent::new(targets, callbacks);

	component.handle_input(&Key::Enter);
	assert_eq!(&*copies.borrow(), &["hello|turn".to_string()]);

	component.handle_input(&Key::Down);
	component.handle_input(&Key::Enter);
	assert_eq!(copies.borrow().len(), 2);

	component.handle_input(&Key::Down);
	component.handle_input(&Key::Enter);
	let last = copies.borrow().last().cloned().unwrap();
	assert_eq!(last, "fn a() {}|rs · 1 line");

	// The index clamps at the last row.
	component.handle_input(&Key::Down);
	component.handle_input(&Key::Down);
	component.handle_input(&Key::Enter);
	assert_eq!(copies.borrow().len(), 4);

	component.handle_input(&Key::Esc);
	assert!(*cancelled.borrow());

	let node = component.describe_native(&DescribeContext::new(60));
	assert_eq!(node.props.get_i64("index"), Some(2));
	assert_eq!(node.props.get_i64("cols"), Some(60));
	let lines = node.props.get("lines").and_then(|value| value.as_array()).expect("lines");
	assert!(!lines.is_empty());
	let text = node_text(&node);
	assert!(text.contains("Copy"));
	assert!(text.contains("hello"));
	assert!(text.contains("enter copy"));
}

struct Copies {
	copy_sink: std::rc::Rc<RefCell<Vec<String>>>,
	cancel_sink: std::rc::Rc<RefCell<bool>>,
}

impl CopySelectorCallbacks for Copies {
	fn on_copy(&self, content: String, label: String) {
		self.copy_sink.borrow_mut().push(format!("{content}|{label}"));
	}

	fn on_cancel(&self) {
		*self.cancel_sink.borrow_mut() = true;
	}

	fn request_render(&self) {}
}

#[test]
fn empty_selector_offers_one_empty_row() {
	let copies = std::rc::Rc::new(RefCell::new(Vec::new()));
	let copy_sink = std::rc::Rc::clone(&copies);
	let mut component = CopySelectorComponent::new(
		Vec::new(),
		Copies {
			copy_sink,
			cancel_sink: std::rc::Rc::new(RefCell::new(false)),
		},
	);
	component.handle_input(&Key::Enter);
	assert_eq!(&*copies.borrow(), &["|empty".to_string()]);
	let rendered = component.render(40);
	assert!(rendered.iter().any(|line| line.contains("empty")));
}

#[test]
fn extract_blocks_keeps_quotes_out_of_fences_and_closes_unterminated_fences() {
	let blocks = extract_blocks("```\n> not a quote\n```\n> real quote");
	assert_eq!(blocks.len(), 2);
	assert_eq!(blocks[0], (true, None, "\n> not a quote".to_string()));
	assert_eq!(blocks[1], (false, None, "real quote".to_string()));

	let unterminated = extract_blocks("```py\nprint(1)");
	assert_eq!(unterminated, vec![(true, Some("py".to_string()), "print(1)".to_string())]);
}
