//! Debug viewer frame: rounded panel, fixed header/footer rows, and a scroll
//! viewport with persistent scroll state and selection reveal.
//!
//! Reference: `l/src/apps/debug/viewer-frame.ts`. Domain controllers keep
//! filtering, selection, formatting and live data; this component owns chrome
//! sizing, persistent scroll state, selection reveal, and frame-row to
//! logical-body translation.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::chrome::{OverlayPanel, PanelRows};
use maple_tui::component::Component;
use maple_tui::components::scroll_view::{ScrollRangeAnchor, ScrollRangeOversized, ScrollView};
use maple_tui::components::scroll_viewport::ScrollViewTheme;
use maple_tui::style::{Color, Style};

/// Body region of one debug-viewer frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugViewerBody {
	/// Logical body rows.
	pub lines: Vec<String>,
	/// Selection revealed after the body rows are installed.
	pub anchor: Option<ScrollRangeAnchor>,
}

/// One complete debug-viewer frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugViewerFrameContent {
	/// Rows above the first divider.
	pub header: Vec<String>,
	/// Scrollable body region.
	pub body: DebugViewerBody,
	/// Rows below the second divider.
	pub footer: Vec<String>,
}

/// Dimensions supplied while a debug viewer prepares its domain rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugViewerFrameContext {
	/// Panel content width with one column reserved for an overflowing viewport's scrollbar.
	pub content_width: usize,
	/// Viewport height in rows.
	pub body_height: usize,
}

/// Frame construction options.
pub struct DebugViewerFrameOptions {
	/// Panel title.
	pub title: String,
	/// Current terminal height in rows.
	pub get_height: Box<dyn Fn() -> f64>,
	/// Pinned header height in rows.
	pub header_rows: usize,
	/// Pinned footer height in rows.
	pub footer_rows: usize,
	/// Builds the frame rows for the given content dimensions.
	pub frame: Box<dyn Fn(DebugViewerFrameContext) -> DebugViewerFrameContent>,
	/// Floor for the body height. Defaults to 3.
	pub minimum_body_rows: Option<usize>,
	/// Whether append-follow is available at all. Defaults to false.
	pub follow_tail: Option<bool>,
}

/// Shared widget handle so the panel can own the child while the frame keeps
/// driving it.
#[derive(Clone)]
struct Shared<T>(Rc<RefCell<T>>);

impl<T: Component + 'static> Component for Shared<T> {
	fn render(&mut self, width: u16) -> Vec<String> {
		self.0.borrow_mut().render(width)
	}

	fn invalidate(&mut self) {
		self.0.borrow_mut().invalidate();
	}

	fn dispose(&mut self) {
		self.0.borrow_mut().dispose();
	}
}

/// Debug-local adapter for the fullscreen viewers' common rounded panel and viewport.
pub struct DebugViewerFrame {
	options: DebugViewerFrameOptions,
	header: Rc<RefCell<PanelRows>>,
	viewport: Rc<RefCell<ScrollView>>,
	footer: Rc<RefCell<PanelRows>>,
	panel: OverlayPanel,
	header_rows: usize,
	footer_rows: usize,
	minimum_body_rows: usize,
	supports_follow_tail: bool,
	follow_tail: bool,
	body_lines: Vec<String>,
	rendered_body_height: usize,
	disposed: bool,
}

fn trunc_row(value: f64) -> Option<usize> {
	if value.is_finite() && value >= 0.0 {
		Some(value.trunc() as usize)
	} else {
		None
	}
}

impl DebugViewerFrame {
	/// Build the frame and its panel chrome.
	pub fn new(options: DebugViewerFrameOptions) -> Self {
		let header_rows = options.header_rows;
		let footer_rows = options.footer_rows;
		let minimum_body_rows = options.minimum_body_rows.unwrap_or(3);
		let supports_follow_tail = options.follow_tail.unwrap_or(false);
		let follow_tail = supports_follow_tail;

		let mut header = PanelRows::new();
		header.set_height(Some(header_rows));
		let mut footer = PanelRows::new();
		footer.set_height(Some(footer_rows));
		let mut viewport = ScrollView::new(Vec::new(), 0);
		viewport.set_scrollbar_style(ScrollViewTheme {
			track: Style::fg(Color::DarkGray),
			thumb: Style::fg(Color::Cyan),
		});
		viewport.set_follow_tail(follow_tail);

		let header = Rc::new(RefCell::new(header));
		let viewport = Rc::new(RefCell::new(viewport));
		let footer = Rc::new(RefCell::new(footer));

		let mut panel = OverlayPanel::new(options.title.clone(), "omp.overlay");
		panel.add_child(Shared(header.clone()));
		panel.add_divider();
		panel.add_child(Shared(viewport.clone()));
		panel.add_divider();
		panel.add_child(Shared(footer.clone()));

		DebugViewerFrame {
			options,
			header,
			viewport,
			footer,
			panel,
			header_rows,
			footer_rows,
			minimum_body_rows,
			supports_follow_tail,
			follow_tail,
			body_lines: Vec::new(),
			rendered_body_height: 0,
			disposed: false,
		}
	}

	/// Components to mount in place of this frame.
	pub fn debug_children(&self) -> Vec<&dyn Component> {
		if self.disposed {
			Vec::new()
		} else {
			vec![&self.panel]
		}
	}

	/// Viewport height after reserving chrome and borders.
	pub fn get_body_height(&self) -> usize {
		let height_value = (self.options.get_height)();
		let height = if height_value.is_finite() {
			height_value.trunc().max(0.0) as usize
		} else {
			0
		};
		height
			.saturating_sub(self.header_rows + self.footer_rows + 4)
			.max(self.minimum_body_rows)
	}

	/// Whether append-follow is currently active.
	pub fn is_following_tail(&self) -> bool {
		!self.disposed && self.follow_tail
	}

	/// Enable or disable append-follow; enabling jumps to the tail.
	pub fn set_follow_tail(&mut self, follow: bool) {
		if self.disposed || !self.supports_follow_tail {
			return;
		}
		self.follow_tail = follow;
		self.viewport.borrow_mut().set_follow_tail(follow);
		if follow {
			self.viewport.borrow_mut().scroll_to_bottom();
		}
	}

	/// Manual viewport movement suspends append-follow, including at the tail.
	pub fn scroll(&mut self, delta: f64) {
		if self.disposed {
			return;
		}
		self.set_follow_tail(false);
		let step = if delta.is_finite() { delta.trunc() as i64 } else { 0 };
		self.viewport.borrow_mut().scroll_by(step);
	}

	/// Jump to an absolute scroll offset, suspending append-follow.
	pub fn set_scroll_offset(&mut self, offset: f64) {
		if self.disposed {
			return;
		}
		self.set_follow_tail(false);
		if let Some(offset) = trunc_row(offset) {
			self.viewport.borrow_mut().scroll_to(offset);
		}
	}

	/// Whether a 0-based rendered frame row lies inside the last rendered body region.
	pub fn is_body_frame_row(&self, frame_row: f64) -> bool {
		let Some(local_row) = trunc_row(frame_row).and_then(|row| row.checked_sub(self.header_rows + 2)) else {
			return false;
		};
		local_row < self.rendered_body_height
	}

	/// Map a 0-based rendered frame row into the complete logical body buffer.
	pub fn to_body_row(&self, frame_row: f64) -> Option<usize> {
		if !self.is_body_frame_row(frame_row) {
			return None;
		}
		let local_row = trunc_row(frame_row)?.checked_sub(self.header_rows + 2)?;
		self.viewport.borrow().logical_row_at(local_row)
	}

	/// Map a 0-based rendered frame row into the fixed header region.
	pub fn to_header_row(&self, frame_row: f64) -> Option<usize> {
		if self.disposed {
			return None;
		}
		let local_row = trunc_row(frame_row)?.checked_sub(1)?;
		(local_row < self.header_rows).then_some(local_row)
	}

	/// Drop the panel's derived render cache.
	pub fn invalidate(&mut self) {
		if self.disposed {
			return;
		}
		self.panel.invalidate();
	}

	/// Release the frame and its panel.
	pub fn dispose(&mut self) {
		if self.disposed {
			return;
		}
		self.disposed = true;
		self.body_lines = Vec::new();
		self.rendered_body_height = 0;
		self.panel.dispose();
	}

	/// Rebuild the frame rows and render the panel.
	pub fn render(&mut self, width: u16) -> Vec<String> {
		if self.disposed {
			return Vec::new();
		}
		let frame_width = width as usize;
		let body_height = self.get_body_height();
		let context = DebugViewerFrameContext {
			content_width: frame_width.saturating_sub(5).max(1),
			body_height,
		};
		let frame = (self.options.frame)(context);
		self.header.borrow_mut().set_lines(&frame.header);
		self.footer.borrow_mut().set_lines(&frame.footer);
		self.viewport.borrow_mut().set_height(body_height);
		self.set_body_lines(&frame.body.lines);
		self.rendered_body_height = body_height;
		let anchor = frame.body.anchor.map(|anchor| ScrollRangeAnchor {
			oversized: Some(ScrollRangeOversized::Start),
			..anchor
		});
		if self.viewport.borrow_mut().reveal_range(anchor, width) {
			self.set_follow_tail(false);
		}
		self.panel.render(width)
	}

	fn set_body_lines(&mut self, lines: &[String]) {
		if self.body_lines.len() == lines.len() && self.body_lines.iter().zip(lines).all(|(a, b)| a == b) {
			return;
		}
		self.body_lines = lines.to_vec();
		self.viewport.borrow_mut().set_lines(self.body_lines.clone());
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn frame_with(header_rows: usize, footer_rows: usize, follow_tail: bool, line_count: usize) -> DebugViewerFrame {
		DebugViewerFrame::new(DebugViewerFrameOptions {
			title: "debug".to_string(),
			get_height: Box::new(|| 30.0),
			header_rows,
			footer_rows,
			frame: Box::new(move |context| DebugViewerFrameContent {
                header: (0..context.content_width.min(2)).map(|index| format!("h{index}")).collect(),
				body: DebugViewerBody {
					lines: (0..line_count).map(|index| format!("row{index}")).collect(),
					anchor: None,
				},
				footer: vec!["foot".to_string()],
			}),
			minimum_body_rows: None,
			follow_tail: Some(follow_tail),
		})
	}

	#[test]
	fn body_height_reserves_chrome_and_borders() {
		let frame = frame_with(2, 1, false, 10);
		assert_eq!(frame.get_body_height(), 23);
	}

	#[test]
	fn frame_rows_map_to_header_and_body() {
		let mut frame = frame_with(2, 1, false, 10);
		let rows = frame.render(40);
		assert_eq!(rows.len(), 30);
		assert!(frame.is_body_frame_row(4.0));
		assert!(!frame.is_body_frame_row(3.0));
		assert_eq!(frame.to_body_row(4.0), Some(0));
		assert_eq!(frame.to_body_row(3.0), None);
		assert_eq!(frame.to_header_row(1.0), Some(0));
		assert_eq!(frame.to_header_row(4.0), None);
	}

	#[test]
	fn reveal_suspends_follow_tail() {
		let content = DebugViewerFrameContent {
			header: vec!["h0".to_string(), "h1".to_string()],
			body: DebugViewerBody {
				lines: (0..30).map(|index| format!("row{index}")).collect(),
				anchor: Some(ScrollRangeAnchor {
					id: "sel".to_string(),
					start: 5,
					end: 6,
					margin: None,
					alignment: None,
					mode: None,
					oversized: None,
				}),
			},
			footer: vec!["foot".to_string()],
		};
		let mut frame = DebugViewerFrame::new(DebugViewerFrameOptions {
			title: "debug".to_string(),
			get_height: Box::new(|| 30.0),
			header_rows: 2,
			footer_rows: 1,
			frame: Box::new(move |_| content.clone()),
			minimum_body_rows: None,
			follow_tail: Some(true),
		});
		assert!(frame.is_following_tail());
		assert_eq!(frame.render(40).len(), 30);
		assert!(!frame.is_following_tail());
	}

	#[test]
	fn manual_scroll_suspends_follow_tail() {
		let mut frame = frame_with(2, 1, true, 30);
		assert!(frame.is_following_tail());
		frame.scroll(2.0);
		assert!(!frame.is_following_tail());
	}

	#[test]
	fn unsupported_follow_tail_stays_off() {
		let mut frame = frame_with(2, 1, false, 10);
		frame.set_follow_tail(true);
		assert!(!frame.is_following_tail());
	}

	#[test]
	fn dispose_clears_children() {
		let mut frame = frame_with(2, 1, false, 10);
		assert_eq!(frame.debug_children().len(), 1);
		frame.dispose();
		assert!(frame.debug_children().is_empty());
		assert!(frame.render(40).is_empty());
	}
}
