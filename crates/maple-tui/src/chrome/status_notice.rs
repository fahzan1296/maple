//! Transient status notices.
//!
//! Reference: `l/src/chrome/status-notice.ts`.
//!
//! Deviations:
//! - `Container`/`Text`/`Spacer` composition and typed `NativeUiEvent` dispatch
//!   have no direct crate equivalents; this port models ANSI rows as a blank
//!   line plus a styled [`Text`] and exposes native toast description via
//!   [`NativeDescribe`].
//! - Native action handling is exposed as [`StatusNotice::handle_action`] rather
//!   than `handleNativeEvent(NativeUiEvent)`.
//! - Toast lifetime (`TOAST_TTL`) is presentation metadata in the reference
//!   host; no host timer exists here, so it is exposed as
//!   [`StatusNotice::toast_ttl_ms`].



use crate::component::Component;
use crate::components::Text;
use crate::native::describe::{col, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeText, Props};
use crate::native::spans::styled_spans;
use crate::theme::builtin;
use crate::theme::loader::{create_theme, CreateThemeOptions};
use crate::theme::Theme;

/// How long a native status toast stays up (ms).
pub const TOAST_TTL_MS: u64 = 2400;

/// A button on a notice's native toast (e.g. Undo after a rewind).
pub struct StatusNoticeAction {
	/// The toast's action name.
	pub act: String,
	/// The button's text.
	pub label: String,
	/// Runs when the button is clicked.
	pub run: Box<dyn Fn() + Send + Sync>,
}

/// How a [`StatusNotice`] shows.
#[derive(Default)]
pub struct StatusNoticeOptions {
	/// Styles the ANSI line.
	pub style_fn: Option<Box<dyn Fn(&str) -> String + Send + Sync>>,
	/// Whether a native terminal toasts it. `false` keeps it ANSI-only.
	pub toast: Option<bool>,
	/// A button on the native toast.
	pub action: Option<StatusNoticeAction>,
	/// Theme the native toast reverse-maps colours against.
	pub theme: Option<Theme>,
}

/// A transient status notice.
pub struct StatusNotice {
	message: String,
	text: Text,
	toast: bool,
	action: Option<StatusNoticeAction>,
	style_fn: Option<Box<dyn Fn(&str) -> String + Send + Sync>>,
	theme: Theme,
}

impl StatusNotice {
	/// Create a notice for `message`.
	pub fn new(message: impl Into<String>, mut options: StatusNoticeOptions) -> Self {
		let theme = options.theme.take().unwrap_or_else(default_theme);
		let mut notice = StatusNotice {
			message: String::new(),
			text: Text::padded("", 0, 0),
			toast: options.toast.unwrap_or(true),
			action: options.action.take(),
			style_fn: options.style_fn.take(),
			theme,
		};
		notice.set_message(message.into(), options);
		notice
	}

	/// The text the notice currently shows.
	pub fn message(&self) -> &str {
		&self.message
	}

	/// Update the message and presentation options.
	pub fn set_message(&mut self, message: String, mut options: StatusNoticeOptions) {
		self.message = message.clone();
		if let Some(style_fn) = options.style_fn.take() {
			self.style_fn = Some(style_fn);
		}
		if let Some(toast) = options.toast {
			self.toast = toast;
		}
		if let Some(action) = options.action.take() {
			self.action = Some(action);
		}
		if let Some(theme) = options.theme.take() {
			self.theme = theme;
		}
		let rendered = match &self.style_fn {
			Some(style_fn) => style_fn(&self.message),
			None => self.message.clone(),
		};
		self.text.set_text(rendered);
	}

	/// Toast lifetime metadata (ms).
	pub fn toast_ttl_ms(&self) -> u64 {
		TOAST_TTL_MS
	}

	/// Whether this notice is toastable natively.
	pub fn toast_enabled(&self) -> bool {
		self.toast
	}

	/// Handle a native toast action by id.
	pub fn handle_action(&self, act: &str) -> bool {
		if let Some(action) = &self.action {
			if action.act == act {
				(action.run)();
				return true;
			}
		}
		false
	}
}

impl Component for StatusNotice {
	fn render(&mut self, width: u16) -> Vec<String> {
		let mut rows = vec![String::new()];
		rows.extend(self.text.render(width));
		rows
	}

	fn invalidate(&mut self) {
		self.text.invalidate();
	}
}

impl NativeDescribe for StatusNotice {
	fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
		if !self.toast {
			return col(Vec::new(), Props::new());
		}
		let rendered = match &self.style_fn {
			Some(style_fn) => style_fn(&self.message),
			None => self.message.clone(),
		};
		let spans = styled_spans(&rendered, &self.theme);
		let mut props = Props::new();
		props.set_str("role", "omp.status_notice.toast");
		if let Some(action) = &self.action {
			props.set_str("act", &action.act);
			props.set_str("label", &action.label);
		}
		text(NodeText::Spans(spans), props)
	}
}

/// The built-in dark theme used when a caller supplies none.
fn default_theme() -> Theme {
	let json = builtin::builtin_theme_json("dark").expect("built-in dark theme");
	create_theme(json, CreateThemeOptions::default()).expect("built-in dark theme loads")
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::{
		atomic::{AtomicUsize, Ordering},
		Arc,
	};
	#[test]
	fn renders_blank_row_before_message() {
		let mut notice = StatusNotice::new(
			"Copied",
			StatusNoticeOptions {
				style_fn: Some(Box::new(|s| format!("[{s}]"))),
				toast: Some(true),
				action: None,
				theme: None,
			},
		);
		let rows = notice.render(20);
		assert_eq!(rows.len(), 2);
		assert_eq!(rows[0], "");
		assert!(rows[1].contains("[Copied]"));
		assert_eq!(notice.message(), "Copied");
	}

	#[test]
	fn toast_disabled_describes_empty_column() {
		let notice = StatusNotice::new(
			"hidden",
			StatusNoticeOptions {
				toast: Some(false),
				..StatusNoticeOptions::default()
			},
		);
		let cx = DescribeContext::new(80);
		let node = notice.describe_native(&cx);
		assert!(node.children.is_empty());
	}

	#[test]
	fn action_handler_runs_matching_action() {
		let hits = Arc::new(AtomicUsize::new(0));
		let hits2 = Arc::clone(&hits);
		let notice = StatusNotice::new(
			"undo?",
			StatusNoticeOptions {
				action: Some(StatusNoticeAction {
					act: "undo".to_string(),
					label: "Undo".to_string(),
					run: Box::new(move || {
						hits2.fetch_add(1, Ordering::SeqCst);
					}),
				}),
				..StatusNoticeOptions::default()
			},
		);
		assert!(!notice.handle_action("nope"));
		assert!(notice.handle_action("undo"));
		assert_eq!(hits.load(Ordering::SeqCst), 1);
	}
}
