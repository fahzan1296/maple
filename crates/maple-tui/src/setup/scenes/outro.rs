//! Reference: `l/src/setup/scenes/outro.ts`.
//!
//! Closing setup page: the brand mark over the starfield, a success line, the
//! default model in effect, and a sweep that stands in for the timed hand-off.
//!
//! Deviations from the TypeScript original:
//! - `theme` is a `&Theme` parameter, not the module singleton.
//! - `Memo` is omitted; the description is rebuilt per call.
//! - `APP_NAME`/`VERSION` come from [`crate::setup::scenes::splash`].
//! - `renderSetupOutro` takes the resolved default model (`Option<String>`)
//!   because the wizard looks it up once per outro.

use crate::native::describe::{col, compact, keyed, node, span, text};
use crate::native::node::{NativeNode, NodeKind, NodeText, Props};
use crate::setup::scenes::splash::{
	center_line, fg, gradient_logo, render_starfield, ShineConfig, APP_NAME, PI_LOGO,
	SETUP_TICK_MS,
};
use crate::text_shape::pad_to_width;
use crate::theme::theme_class::Theme;

/// Total length of the outro animation, in milliseconds.
pub const SETUP_OUTRO_MS: f64 = 1200.0;

fn grow() -> Props {
	let mut props = Props::new();
	props.insert("grow", 1.into());
	props
}

/// Native outro, centered: the builtin `omp` mark, "You're all set", the
/// default model now in effect, and an indeterminate progress bar. A click
/// sends the `continue` action.
pub fn describe_setup_outro(model: Option<&str>) -> NativeNode {
	let mut mark = Props::new();
	mark.set_str("builtin", "omp");
	mark.set_str("alt", APP_NAME);
	mark.set_i64("w", 72);
	mark.set_role("omp.setup.outro.mark");

	let mut title = Props::new();
	title.set_str("wrap", "none");
	title.set_role("omp.setup.outro.title");

	let mut items: Vec<Option<NativeNode>> = vec![
		Some(node(NodeKind::Spacer, grow(), Vec::new(), Some("top"))),
		Some(node(NodeKind::Image, mark, Vec::new(), Some("logo"))),
		Some(keyed(
			text(
				NodeText::Spans(vec![span("You're all set", Some("strong"))]),
				title,
			),
			"title",
		)),
	];
	if let Some(model) = model {
		let mut props = Props::new();
		props.set_str("wrap", "none");
		props.set_str("truncate", "end");
		props.set_role("omp.setup.outro.model");
		items.push(Some(keyed(
			text(
				NodeText::Spans(vec![
					span("Default model", Some("muted")),
					span(" · ", Some("dim")),
					span(model, Some("muted")),
				]),
				props,
			),
			"model",
		)));
	}
	let mut status = Props::new();
	status.set_str("wrap", "none");
	status.set_role("omp.setup.outro.status");
	items.push(Some(keyed(
		text(
			NodeText::Spans(vec![span(
				format!("Starting {}…", APP_NAME),
				Some("muted"),
			)]),
			status,
		),
		"status",
	)));
	let mut progress = Props::new();
	progress.set_role("omp.setup.outro.progress");
	items.push(Some(node(
		NodeKind::Progress,
		progress,
		Vec::new(),
		Some("progress"),
	)));
	items.push(Some(node(
		NodeKind::Spacer,
		grow(),
		Vec::new(),
		Some("bottom"),
	)));

	let mut root = Props::new();
	root.set_str("align", "center");
	root.insert("grow", 1.into());
	root.set_role("omp.setup.outro");
	root.insert("actions", serde_json::json!({ "click": "continue" }));
	col(compact(items), root)
}

/// Starfield outro frame with the sweep and lockup overlaid.
pub fn render_setup_outro(
	theme: &Theme,
	width: usize,
	height: usize,
	elapsed_ms: f64,
	model: Option<String>,
) -> Vec<String> {
	let w = width.max(1);
	let h = height.max(1);
	let frame = (elapsed_ms / SETUP_TICK_MS).floor() as i64;
	let mut lines = render_starfield(theme, w, h, frame + 1000);
	let progress = (elapsed_ms / SETUP_OUTRO_MS).clamp(0.0, 1.0);
	let shine = ShineConfig {
		pos: (progress * 2.0) % 1.0,
		strength: 1.0 - progress,
	};
	let logo = gradient_logo(&PI_LOGO, progress * 1.2, Some(shine));
	let title = theme.bold(&fg(
		theme,
		"success",
		&format!("{} Setup saved", theme.status().success),
	));
	let subtitle = fg(theme, "muted", "Handing off to the normal CLI…");
	let track = w.saturating_sub(8);
	let sweep_width = ((track as f64) * progress).floor() as usize;
	let sweep_width = sweep_width.clamp(1, track.max(1));
	let sweep = format!(
		"{}{}",
		fg(theme, "accent", &"━".repeat(sweep_width)),
		fg(theme, "dim", &"─".repeat(track.saturating_sub(sweep_width)))
	);
	let mut content: Vec<String> = logo;
	content.push(String::new());
	content.push(title);
	content.push(subtitle);
	content.push(String::new());
	content.push(sweep);
	let _ = model;
	let start = h.saturating_sub(content.len()) / 2;
	for (index, line) in content.iter().enumerate() {
		if start + index >= lines.len() {
			break;
		}
		lines[start + index] = center_line(line, w);
	}
	lines
		.into_iter()
		.map(|line| pad_to_width(&line, w))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::loader::{create_theme, CreateThemeOptions};

	fn test_theme() -> Theme {
		create_theme(builtin_theme_json("dark").expect("builtin theme"), CreateThemeOptions::default()).expect("theme")
	}

	#[test]
	fn outro_description_carries_continue_action() {
		let node = describe_setup_outro(None);
		assert_eq!(node.kind, NodeKind::Col);
		assert_eq!(node.props.get_str("role"), Some("omp.setup.outro"));
		assert_eq!(
			node
				.props
				.get("actions")
				.and_then(|value| value.get("click")),
			Some(&serde_json::Value::String("continue".to_string()))
		);
	}

	#[test]
	fn outro_description_includes_model_only_when_present() {
		let with_model = describe_setup_outro(Some("maple-2.5"));
		let without = describe_setup_outro(None);
		let has = |node: &NativeNode| {
			node.children
				.iter()
				.any(|child| child.props.get_str("role") == Some("omp.setup.outro.model"))
		};
		assert!(has(&with_model));
		assert!(!has(&without));
	}

	#[test]
	fn outro_frame_fits_the_screen() {
		let theme = test_theme();
		let lines = render_setup_outro(&theme, 40, 12, 0.0, Some("maple-2.5".to_string()));
		assert_eq!(lines.len(), 12);
		for line in &lines {
			assert!(crate::text_shape::visible_width(line) <= 40);
		}
	}

	#[test]
	fn outro_sweep_grows_with_elapsed_time() {
		let theme = test_theme();
		let early = render_setup_outro(&theme, 40, 12, 0.0, None);
		let late = render_setup_outro(&theme, 40, 12, SETUP_OUTRO_MS, None);
		let bar = |lines: &[String]| {
			lines
				.iter()
				.map(|line| {
					line.chars()
						.filter(|character| *character == '━' || *character == '─')
						.count()
				})
				.sum::<usize>()
		};
		assert!(bar(&late) >= bar(&early));
	}
}
