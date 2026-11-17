//! Port of `l/test/width-aware-text.test.ts`.
//!
//! Divergence: exercises `WidthAwareText` via Rust `Component::render` and
//! `NativeDescribe::describe` with `DescribeContext`; extracts inner `text`
//! spans to observe formatted content (TS used a `describedText` helper).

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::native::node::{DescribeContext, NativeChild};
use maple_tui::native::NativeDescribe;
use maple_tui::render::width_aware_text::WidthAwareText;

fn cx(cols: u16) -> DescribeContext {
	DescribeContext {
		cols,
		reduce_motion: false,
		dark: true,
		supports: |_| true,
		feature: |_| false,
	}
}

fn described_text(component: &WidthAwareText, context: &DescribeContext) -> String {
	let described = component.describe(context);
	let child = described.children.first().cloned();
	let inner = match child {
		Some(NativeChild::Node(n)) => n,
		Some(NativeChild::Component(c)) => c.describe(context),
		None => described,
	};
	if inner.kind != "text" {
		return String::new();
	}
	inner
		.props
		.get("spans")
		.and_then(|v| v.as_array())
		.map(|arr| {
			arr.iter()
				.filter_map(|s| s.get("t").and_then(|t| t.as_str()))
				.collect::<String>()
		})
		.unwrap_or_default()
}

#[test]
fn describe_and_render_budgets_differ_by_padding_x_times_two() {
	let widths: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
	let seen = widths.clone();
	let mut component = WidthAwareText::new(
		move |width| {
			seen.borrow_mut().push(width);
			"x".to_string()
		},
		2,
		1,
	);
	component.describe(&cx(80));
	component.render(80);
	assert_eq!(&*widths.borrow(), &[80, 76]);
}

#[test]
fn re_describes_at_cx_cols_after_render_instead_of_reusing_narrow_text() {
	let mut component = WidthAwareText::new(|width| format!("w{width}"), 1, 1);
	assert_eq!(described_text(&component, &cx(80)), "w80");
	component.render(80);
	assert_eq!(described_text(&component, &cx(80)), "w80");
}
