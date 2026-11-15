//! The UI engine: owns components, dispatches input, drives the renderer.

use crate::component::Component;
use crate::keys::Key;
use crate::screen::Screen;
use crate::terminal::{TtyEvent, Terminal};
use crate::trigger::Trigger;
use std::io;
use std::time::Duration;

/// Root container that renders children top-to-bottom and forwards input to
/// the focused (last added) component.
pub struct Tui {
    terminal: Box<dyn Terminal>,
    children: Vec<Box<dyn Component>>,
    screen: Screen,
    dirty: bool,
    stopped: bool,
    trigger: Trigger,
    on_debug: Option<Box<dyn FnMut()>>,
}

impl Tui {
    pub fn new(terminal: Box<dyn Terminal>) -> Self {
        Tui {
            terminal,
            children: Vec::new(),
            screen: Screen::new(),
            dirty: true,
            stopped: false,
            trigger: Trigger::new(),
            on_debug: None,
        }
    }

    /// Shared wake-up handle components use to request animation frames.
    pub fn render_trigger(&self) -> Trigger {
        self.trigger.clone()
    }

    pub fn add_child<C: Component + 'static>(&mut self, child: C) -> &mut Self {
        self.children.push(Box::new(child));
        self.request_render();
        self
    }

    pub fn insert_child<C: Component + 'static>(&mut self, index: usize, child: C) -> &mut Self {
        let index = index.min(self.children.len());
        self.children.insert(index, Box::new(child));
        self.request_render();
        self
    }

    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index >= self.children.len() {
            return None;
        }
        let removed = self.children.remove(index);
        self.request_render();
        Some(removed)
    }

    pub fn children(&self) -> &[Box<dyn Component>] {
        &self.children
    }

    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    pub fn request_render(&mut self) {
        self.dirty = true;
        self.trigger.request();
    }

    pub fn set_on_debug(&mut self, handler: Box<dyn FnMut()>) {
        self.on_debug = Some(handler);
    }

    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// Claim the terminal. Does not enter the event loop; see `run`.
    pub fn start(&mut self) -> io::Result<()> {
        self.terminal.start()
    }

    /// Leave the alternate screen and restore terminal state.
    pub fn stop(&mut self) -> io::Result<()> {
        self.stopped = true;
        self.trigger.request();
        self.terminal.stop()
    }

    /// Block until `stop` is called, pumping events and rendering.
    pub fn run(&mut self) -> io::Result<()> {
        self.start()?;
        while !self.stopped {
            if let Some(event) = self.terminal.event(Duration::from_millis(16)) {
                self.handle_event(event);
            }
            if self.trigger.take() {
                self.dirty = true;
            }
            if self.dirty {
                self.render_now();
            }
        }
        self.terminal.stop()
    }

    /// Render one frame immediately (useful for headless tests).
    pub fn render_now(&mut self) {
        let width = self.terminal.columns().max(1);
        let rows_available = self.terminal.rows().max(1) as usize;
        let mut lines: Vec<String> = Vec::new();
        for child in &mut self.children {
            lines.extend(child.render(width));
        }
        if lines.len() > rows_available {
            let excess = lines.len() - rows_available;
            lines.drain(0..excess);
        }
        self.screen.flush(&mut *self.terminal, &lines, width);
        self.dirty = false;
    }

    fn handle_event(&mut self, event: TtyEvent) {
        match event {
            TtyEvent::Resize(_, _) => {
                self.dirty = true;
            }
            TtyEvent::Key(key) => {
                if let Key::Ctrl('D') = key {
                    if let Some(handler) = self.on_debug.as_mut() {
                        handler();
                    }
                } else if let Some(child) = self.children.last_mut() {
                    child.handle_input(&key);
                }
                self.request_render();
            }
            TtyEvent::Mouse(mouse) => {
                if let Some(child) = self.children.last_mut() {
                    child.handle_mouse(&mouse);
                }
                self.request_render();
            }
            TtyEvent::Paste(text) => {
                if let Some(child) = self.children.last_mut() {
                    child.handle_paste(&text);
                }
                self.request_render();
            }
            TtyEvent::Disconnect => {
                self.stopped = true;
            }
        }
    }
}
