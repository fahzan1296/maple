//! Debug viewer frame chrome/viewport adapter.
//! Reference: `l/src/apps/debug/viewer-frame.ts`.

use crate::chrome::overlay_box::{OverlayPanel, OverlayTheme, PanelDivider, PanelRows};
use crate::component::Component;
use crate::components::scroll_view::{ScrollRangeAnchor, ScrollRangeOversized, ScrollView};

/// One complete debug-viewer frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugViewerFrameContent {
	pub header: Vec<String>,
	pub body_lines: Vec<String>,
	pub body_anchor: Option<ScrollRangeAnchor>,
	pub footer: Vec<String>,
}

/// Dimensions supplied while a debug viewer prepares its domain rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugViewerFrameContext {
	/// Panel content width with one column reserved for an overflowing viewport's scrollbar.
	pub content_width: usize,
	pub body_height: usize,
}

pub struct DebugViewerFrameOptions {
	pub title: String,
	pub get_height: Box<dyn Fn() -> usize>,
	pub header_rows: usize,
	pub footer_rows: usize,
	pub frame: Box<dyn FnMut(DebugViewerFrameContext) -> DebugViewerFrameContent>,
	pub minimum_body_rows: usize,
	pub follow_tail: bool,
}

/// Debug-local adapter for the fullscreen viewers' common rounded panel and viewport.
/// Domain controllers retain filtering, selection, formatting, copy, and live data;
/// this component owns chrome sizing, persistent scroll state, selection reveal, and
/// frame-row to logical-body translation.
pub struct DebugViewerFrame {
	options: DebugViewerFrameOptions,
	header: PanelRows,
	viewport: ScrollView,
	footer: PanelRows,
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

impl DebugViewerFrame {
	pub fn new(options: DebugViewerFrameOptions) -> Self {
		let header_rows = options.header_rows;
		let footer_rows = options.footer_rows;
		let minimum_body_rows = options.minimum_body_rows;
		let supports_follow_tail = options.follow_tail;
		let follow_tail = supports_follow_tail;
		let mut header = PanelRows::new();
		header.set_height(Some(header_rows));
		let mut footer = PanelRows::new();
		footer.set_height(Some(footer_rows));
		let initial_body = minimum_body_rows.max(1);
		let mut viewport = ScrollView::new(Vec::new(), initial_body);
		viewport.set_follow_tail(follow_tail);
		let mut panel = OverlayPanel::default();
		panel.set_title(&options.title);
		panel.set_theme(OverlayTheme::default());
		panel.add_child(PanelRows::new());
		// replace children properly
		let mut frame = DebugViewerFrame {
			header_rows,
			footer_rows,
			minimum_body_rows,
			supports_follow_tail,
			follow_tail,
			body_lines: Vec::new(),
			rendered_body_height: 0,
			disposed: false,
			panel: OverlayPanel::default(),
			header,
			viewport,
			footer,
			options,
		};
		frame.rebuild_panel();
		frame
	}

	fn rebuild_panel(&mut self) {
		let mut panel = OverlayPanel::default();
		panel.set_title(&self.options.title);
		panel.set_theme(OverlayTheme::default());
		// header
		let mut h = PanelRows::new();
		h.set_height(Some(self.header_rows));
		h.set_lines(self.header.lines());
		panel.add_child(h);
		panel.add_child(PanelDivider);
		// viewport: include as child by rendering placeholder rows through PanelRows is not ideal;
		// keep ScrollView as a field and compose in render.
		panel.add_child(PanelRows::new());
		panel.add_child(PanelDivider);
		let mut f = PanelRows::new();
		f.set_height(Some(self.footer_rows));
		f.set_lines(self.footer.lines());
		panel.add_child(f);
		self.panel = panel;
	}

	pub fn get_body_height(&self) -> usize {
		let h = (self.options.get_height)();
		h.saturating_sub(self.header_rows)
			.saturating_sub(self.footer_rows)
			.saturating_sub(4)
			.max(self.minimum_body_rows)
	}

	pub fn is_following_tail(&self) -> bool {
		!self.disposed && self.follow_tail
	}

	pub fn set_follow_tail(&mut self, follow: bool) {
		if self.disposed || !self.supports_follow_tail {
			return;
		}
		self.follow_tail = follow;
		self.viewport.set_follow_tail(follow);
		if follow {
			self.viewport.scroll_to_bottom();
		}
	}

	/// Manual viewport movement suspends append-follow, including at the current tail.
	pub fn scroll(&mut self, delta: isize) {
		if self.disposed {
			return;
		}
		self.set_follow_tail(false);
		let cur = self.viewport.scroll_offset() as isize;
		let next = (cur + delta).max(0) as usize;
		self.viewport.scroll_to(next);
	}

	pub fn set_scroll_offset(&mut self, offset: usize) {
		if self.disposed {
			return;
		}
		self.set_follow_tail(false);
		self.viewport.scroll_to(offset);
	}

	/// Whether a 0-based rendered frame row lies inside the last rendered body region.
	pub fn is_body_frame_row(&self, frame_row: usize) -> bool {
		let local = frame_row as isize - (self.header_rows as isize + 2);
		local >= 0 && (local as usize) < self.rendered_body_height
	}

	/// Map a 0-based rendered frame row into the complete logical body buffer.
	pub fn to_body_row(&self, frame_row: usize) -> Option<usize> {
		if !self.is_body_frame_row(frame_row) {
			return None;
		}
		let local = frame_row.saturating_sub(self.header_rows + 2);
		self.viewport.logical_row_at(local)
	}

	/// Map a 0-based rendered frame row into the fixed header region.
	pub fn to_header_row(&self, frame_row: usize) -> Option<usize> {
		if self.disposed {
			return None;
		}
		let local = frame_row as isize - 1;
		if local >= 0 && (local as usize) < self.header_rows {
			Some(local as usize)
		} else {
			None
		}
	}

	fn set_body_lines(&mut self, lines: &[String]) {
		if lines.len() == self.body_lines.len() && lines.iter().zip(&self.body_lines).all(|(a, b)| a == b) {
			return;
		}
		self.body_lines = lines.to_vec();
		self.viewport.set_lines(self.body_lines.clone());
	}
}

impl Component for DebugViewerFrame {
	fn render(&mut self, width: u16) -> Vec<String> {
		if self.disposed {
			return Vec::new();
		}
		let frame_width = width;
		let body_height = self.get_body_height();
		let context = DebugViewerFrameContext {
			content_width: (frame_width as usize).saturating_sub(5).max(1),
			body_height,
		};
		let mut frame = (self.options.frame)(context);
		self.header.set_height(Some(self.header_rows));
		self.header.set_lines(&frame.header);
		self.footer.set_height(Some(self.footer_rows));
		self.footer.set_lines(&frame.footer);
		self.viewport.set_height(body_height.max(1));
		self.set_body_lines(&frame.body_lines);
		self.rendered_body_height = body_height;
		if let Some(mut anchor) = frame.body_anchor.take() {
			anchor.oversized = Some(ScrollRangeOversized::Start);
			if self.viewport.reveal_range(Some(anchor), context.content_width as u16) {
				self.set_follow_tail(false);
			}
		}

		let theme = OverlayTheme::default();
		let mut out = Vec::new();
		out.push(crate::chrome::overlay_box::top_border(frame_width, &self.options.title, &theme));
		for line in self.header.render((frame_width as usize).saturating_sub(4) as u16) {
			out.push(crate::chrome::overlay_box::row(&line, frame_width, &theme));
		}
		out.push(crate::chrome::overlay_box::divider(frame_width, &theme));
		for line in self.viewport.render((frame_width as usize).saturating_sub(4) as u16) {
			out.push(crate::chrome::overlay_box::row(&line, frame_width, &theme));
		}
		out.push(crate::chrome::overlay_box::divider(frame_width, &theme));
		for line in self.footer.render((frame_width as usize).saturating_sub(4) as u16) {
			out.push(crate::chrome::overlay_box::row(&line, frame_width, &theme));
		}
		out.push(crate::chrome::overlay_box::bottom_border(frame_width, &theme));
		out
	}

	fn invalidate(&mut self) {
		if self.disposed {
			return;
		}
		self.header.invalidate();
		self.viewport.invalidate();
		self.footer.invalidate();
		self.panel.invalidate();
	}

	fn dispose(&mut self) {
		if self.disposed {
			return;
		}
		self.disposed = true;
		self.body_lines.clear();
		self.rendered_body_height = 0;
		self.viewport.dispose();
		self.panel.dispose();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn frame_content() -> DebugViewerFrameContent {
		DebugViewerFrameContent {
			header: vec!["h".into()],
			body_lines: vec!["a".into(), "b".into(), "c".into()],
			body_anchor: None,
			footer: vec!["f".into()],
		}
	}

	#[test]
	fn body_height_and_row_mapping() {
		let mut vf = DebugViewerFrame::new(DebugViewerFrameOptions {
			title: "T".into(),
			get_height: Box::new(|| 20),
			header_rows: 1,
			footer_rows: 1,
			minimum_body_rows: 3,
			follow_tail: false,
			frame: Box::new(|_| frame_content()),
		});
		assert!(vf.get_body_height() >= 3);
		let _ = vf.render(40);
		assert!(vf.to_header_row(1).is_some());
	}

	#[test]
	fn follow_tail_toggle() {
		let mut vf = DebugViewerFrame::new(DebugViewerFrameOptions {
			title: "T".into(),
			get_height: Box::new(|| 12),
			header_rows: 1,
			footer_rows: 1,
			minimum_body_rows: 1,
			follow_tail: true,
			frame: Box::new(|_| frame_content()),
		});
		assert!(vf.is_following_tail());
		vf.scroll(1);
		assert!(!vf.is_following_tail());
	}
}
