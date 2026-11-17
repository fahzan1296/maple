//! Reference: `l/test/prompt-start-write.test.ts`.
//!
//! The reference counts OSC 133;A writes from a terminal across scheduler
//! repaints. This port drives the same paint path synchronously: a counting
//! [`Terminal`] behind [`Tui`] with a frame provider that renders two user
//! bubbles, so the differential screen decides which rows are rewritten.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use maple_tui::chat::user_message::{
	prompt_zones_enabled, set_prompt_zones_enabled, UserBubbleOptions, UserMessageComponent,
};
use maple_tui::component::Component;
use maple_tui::terminal::{Terminal, TtyEvent};
use maple_tui::theme::default_theme;
use maple_tui::tui::{
	RenderRequestOptions, TerminalFramePlan, TerminalFrameProvider, Tui, ViewportSize,
};

const PROMPT_START: &str = "\x1b]133;A\x07";

struct CountingTerminal {
	dimensions: Rc<Cell<(u16, u16)>>,
	prompt_starts: Rc<Cell<usize>>,
}

impl CountingTerminal {
	fn new(dimensions: Rc<Cell<(u16, u16)>>, prompt_starts: Rc<Cell<usize>>) -> Self {
		Self {
			dimensions,
			prompt_starts,
		}
	}
}

impl Terminal for CountingTerminal {
	fn start(&mut self) -> std::io::Result<()> {
		Ok(())
	}

	fn stop(&mut self) -> std::io::Result<()> {
		Ok(())
	}

	fn write(&mut self, data: &str) {
		let seen = self.prompt_starts.get();
		self.prompt_starts
			.set(seen + data.matches(PROMPT_START).count());
	}

	fn event(&mut self, _timeout: Duration) -> Option<TtyEvent> {
		None
	}

	fn columns(&self) -> u16 {
		self.dimensions.get().0
	}

	fn rows(&self) -> u16 {
		self.dimensions.get().1
	}

	fn move_by(&mut self, _lines: i32) {}

	fn hide_cursor(&mut self, _force: bool) {}

	fn show_cursor(&mut self, _force: bool) {}

	fn clear_line(&mut self) {}

	fn clear_from_cursor(&mut self) {}

	fn clear_screen(&mut self) {}
}

struct BubbleProvider {
	messages: Vec<UserMessageComponent>,
}

impl TerminalFrameProvider for BubbleProvider {
	fn render_frame(&mut self, viewport: ViewportSize) -> TerminalFramePlan {
		let mut lines = Vec::new();
		for message in &mut self.messages {
			lines.extend(message.render(viewport.columns));
		}
		lines.push("status".to_string());
		TerminalFramePlan {
			history: None,
			viewport: lines,
		}
	}
}

#[test]
fn writes_no_prompt_start_on_redraws_while_zones_are_off_and_restores_the_zone() {
	set_prompt_zones_enabled(true);
	let theme = default_theme();
	let messages = vec![
		UserMessageComponent::new("Run sleep 15", UserBubbleOptions::default(), &theme),
		UserMessageComponent::new("and then?", UserBubbleOptions::default(), &theme),
	];
	let dimensions = Rc::new(Cell::new((80, 12)));
	let prompt_starts = Rc::new(Cell::new(0));
	let mut tui = Tui::new(Box::new(CountingTerminal::new(
		Rc::clone(&dimensions),
		Rc::clone(&prompt_starts),
	)));
	tui.set_frame_provider(Some(Box::new(BubbleProvider { messages })));
	tui.start().expect("start");
	tui.render_now();
	assert_eq!(prompt_starts.get(), 2);
	assert!(prompt_zones_enabled());

	set_prompt_zones_enabled(false);
	tui.request_render();
	tui.render_now();
	let before = prompt_starts.get();
	for _ in 0..3 {
		tui.request_render_with(
			true,
			RenderRequestOptions {
				clear_scrollback: true,
			},
		);
		tui.render_now();
	}
	dimensions.set((70, 12));
	tui.handle_event(TtyEvent::Resize(70, 12));
	tui.render_now();
	assert_eq!(prompt_starts.get(), before);

	set_prompt_zones_enabled(true);
	tui.request_render();
	tui.render_now();
	assert_eq!(prompt_starts.get(), before + 2);
	tui.request_render();
	tui.render_now();
	assert_eq!(prompt_starts.get(), before + 2);

	set_prompt_zones_enabled(true);
	tui.stop().expect("stop");
}
