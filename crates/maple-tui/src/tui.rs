//! The UI engine: owns components, dispatches input, drives the renderer.
//! Reference: `l/src/tui.ts`.
//!
//! Deviations from the reference:
//! - Input is decoded (`Key`) rather than raw terminal bytes, so an input
//!   listener can consume or replace a key but cannot rewrite escape text.
//! - Overlays support floating anchor/margin/max-height placement and a
//!   fullscreen modal mode. The process terminal already holds the alternate
//!   screen, so no nested 1049 toggle is emitted.
//! - The frame provider supplies the viewport and one pending history batch per
//!   frame; native scrollback accounting stays with the provider.

use crate::component::Component;
use crate::keys::Key;
use crate::mouse_tracking::{MouseTracking, MouseTrackingState};
use crate::screen::Screen;
use crate::terminal::{Terminal, TtyEvent};
use crate::text_shape::{truncate_to_width, Ellipsis};
use crate::trigger::Trigger;
use std::cell::Cell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

const ERASE_SAVED_LINES: &str = "\x1b[3J";

/// A component identity the engine can focus without borrowing the component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// Top-level child, by position in [`Tui::children`].
    Child(usize),
    /// Overlay, by the id returned from [`OverlayHandle::id`].
    Overlay(u64),
}

/// Where a floating overlay sits on the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// Rows/columns reserved around a floating overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayMargin {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}

impl OverlayMargin {
    pub const fn all(rows: usize) -> Self {
        Self { top: rows, right: rows, bottom: rows, left: rows }
    }
}

/// Options for [`Tui::show_overlay`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayOptions {
    /// Paint only this overlay, as a modal frame, for its lifetime.
    pub fullscreen: bool,
    /// Keep mouse reporting active while this fullscreen overlay is up.
    pub mouse_tracking: bool,
    /// Placement of the floating overlay.
    pub anchor: OverlayAnchor,
    /// Reserved rows/columns the overlay must not paint into.
    pub margin: OverlayMargin,
    /// Explicit row cap; defaults to the rows left after margins.
    pub max_height: Option<usize>,
    /// Explicit width in columns; defaults to the terminal width.
    pub width: Option<usize>,
    /// Fixed row, overriding the vertical anchor.
    pub row: Option<u16>,
    /// Fixed column, overriding the horizontal anchor.
    pub col: Option<u16>,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            fullscreen: false,
            mouse_tracking: true,
            anchor: OverlayAnchor::default(),
            margin: OverlayMargin::default(),
            max_height: None,
            width: None,
            row: None,
            col: None,
        }
    }
}

/// Painted inline window observed by [`Tui::get_mutable_viewport`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MutableViewport {
    /// First painted row; always 0 in this crate.
    pub top: u16,
    /// Painted rows; 0 while a fullscreen overlay covers the window.
    pub length: usize,
}

/// Handle controlling one overlay.
///
/// `hide` is applied by the engine on the next input or render pass, because
/// restoring focus needs exclusive access to the [`Tui`].
pub struct OverlayHandle {
    id: u64,
    hidden: Rc<Cell<bool>>,
    closed: Rc<Cell<bool>>,
    trigger: Trigger,
}

impl OverlayHandle {
    /// Engine-assigned overlay id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Permanently remove the overlay.
    pub fn hide(self) {
        self.closed.set(true);
        self.trigger.request();
    }

    /// Temporarily hide or show the overlay without removing it.
    pub fn set_hidden(&self, hidden: bool) {
        if self.hidden.get() != hidden {
            self.hidden.set(hidden);
            self.trigger.request();
        }
    }

    /// Whether the overlay is temporarily hidden.
    pub fn is_hidden(&self) -> bool {
        self.hidden.get()
    }
}

/// Outcome of one input listener.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputListenerResult {
    /// Stop delivery: neither later listeners nor the focused component see the key.
    pub consume: bool,
    /// Replacement key for later listeners and the focused component.
    pub data: Option<Key>,
}

impl InputListenerResult {
    /// Pass the key through unchanged.
    pub fn pass() -> Self {
        Self::default()
    }

    /// Consume the key.
    pub fn consume() -> Self {
        Self {
            consume: true,
            data: None,
        }
    }

    /// Forward a different key.
    pub fn replace(key: Key) -> Self {
        Self {
            consume: false,
            data: Some(key),
        }
    }
}

/// Identifier returned by [`Tui::add_input_listener`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputListenerId(u64);

type InputListener = Box<dyn FnMut(&Key) -> InputListenerResult>;

/// Options for a rich render request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderRequestOptions {
    /// Ask the terminal to drop its saved scrollback before the next paint.
    pub clear_scrollback: bool,
}

/// Physical terminal dimensions supplied to a frame provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportSize {
    pub columns: u16,
    pub rows: u16,
}

/// How a [`HistoryBatch`] extends the terminal's native history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryKind {
    /// Finalized rows appended to existing history.
    #[default]
    Append,
    /// The complete history ledger, replacing what the terminal shows.
    Replay,
}

/// One history append or replay offered with a terminal frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryBatch {
    pub id: u64,
    pub rows: Vec<String>,
    pub kind: HistoryKind,
}

/// One provider-composed terminal frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalFramePlan {
    pub history: Option<HistoryBatch>,
    pub viewport: Vec<String>,
}

/// Produces bounded terminal frames and retires acknowledged history batches.
pub trait TerminalFrameProvider {
    fn render_frame(&mut self, viewport: ViewportSize) -> TerminalFramePlan;

    fn acknowledge_history(&mut self, _id: u64) {}

    /// Full semantic viewport for the transient resize buffer.
    fn render_resize_frame(&mut self, _viewport: ViewportSize) -> Option<Vec<String>> {
        None
    }

    /// Re-offer finalized history after a display reset or resize replay.
    fn begin_history_replay(&mut self) {}

    /// Force every eligible finalized prefix to retire before stop.
    fn begin_history_flush(&mut self) {}
}

/// Rows painted by one frame, observed through [`Tui::set_on_paint`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TuiPaint {
    /// Rows committed above the viewport by this paint.
    pub history: Vec<String>,
    /// Complete live viewport: one prepared row per painted line.
    pub viewport: Vec<String>,
    /// The paint erased prior content and repainted from the top.
    pub reset: bool,
    /// The viewport is a fullscreen overlay rather than the transcript.
    pub alt: bool,
    pub columns: u16,
    pub rows: u16,
}

struct OverlayEntry {
    id: u64,
    component: Box<dyn Component>,
    options: OverlayOptions,
    hidden: Rc<Cell<bool>>,
    closed: Rc<Cell<bool>>,
    pre_focus: Option<FocusTarget>,
}

/// Root container that renders children top-to-bottom and forwards input to
/// the focused component (falling back to the last child), then overlays.
pub struct Tui {
    terminal: Box<dyn Terminal>,
    children: Vec<Box<dyn Component>>,
    overlays: Vec<OverlayEntry>,
    focus: Option<FocusTarget>,
    input_listeners: Vec<(InputListenerId, InputListener)>,
    next_input_listener: u64,
    next_overlay: u64,
    screen: Screen,
    dirty: bool,
    force_full: bool,
    clear_scrollback: bool,
    stopped: bool,
    trigger: Trigger,
    on_debug: Option<Box<dyn FnMut()>>,
    on_paint: Option<Box<dyn FnMut(&TuiPaint)>>,
    last_paint: Option<TuiPaint>,
    frame_provider: Option<Box<dyn TerminalFrameProvider>>,
    inline_mouse_provider: Option<Box<dyn Fn() -> bool>>,
    mouse_tracking: MouseTracking,
    modal_active: bool,
    last_overflow: Vec<String>,
}

impl Tui {
    pub fn new(terminal: Box<dyn Terminal>) -> Self {
        Tui {
            terminal,
            children: Vec::new(),
            overlays: Vec::new(),
            focus: None,
            input_listeners: Vec::new(),
            next_input_listener: 0,
            next_overlay: 0,
            screen: Screen::new(),
            dirty: true,
            force_full: false,
            clear_scrollback: false,
            stopped: false,
            trigger: Trigger::new(),
            on_debug: None,
            on_paint: None,
            last_paint: None,
            frame_provider: None,
            inline_mouse_provider: None,
            mouse_tracking: MouseTracking::new(),
            modal_active: false,
            last_overflow: Vec::new(),
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
        if let Some(FocusTarget::Child(focused)) = self.focus {
            if focused >= index {
                self.focus = Some(FocusTarget::Child(focused + 1));
            }
        }
        self.request_render();
        self
    }

    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index >= self.children.len() {
            return None;
        }
        let removed = self.children.remove(index);
        self.focus = match self.focus {
            Some(FocusTarget::Child(focused)) if focused == index => None,
            Some(FocusTarget::Child(focused)) if focused > index => {
                Some(FocusTarget::Child(focused - 1))
            }
            focus => focus,
        };
        self.request_render();
        Some(removed)
    }

    pub fn children(&self) -> &[Box<dyn Component>] {
        &self.children
    }

    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Show an overlay above the children and focus it.
    pub fn show_overlay<C: Component + 'static>(
        &mut self,
        component: C,
        options: OverlayOptions,
    ) -> OverlayHandle {
        let id = self.next_overlay;
        self.next_overlay += 1;
        let hidden = Rc::new(Cell::new(false));
        let closed = Rc::new(Cell::new(false));
        self.overlays.push(OverlayEntry {
            id,
            component: Box::new(component),
            options,
            hidden: Rc::clone(&hidden),
            closed: Rc::clone(&closed),
            pre_focus: self.focus,
        });
        self.focus = Some(FocusTarget::Overlay(id));
        self.request_render();
        OverlayHandle {
            id,
            hidden,
            closed,
            trigger: self.trigger.clone(),
        }
    }

    /// Permanently remove the topmost overlay and restore the focus it displaced.
    pub fn hide_overlay(&mut self) {
        let Some(index) = self.overlays.len().checked_sub(1) else {
            return;
        };
        let entry = self.overlays.remove(index);
        if self.focus == Some(FocusTarget::Overlay(entry.id)) {
            self.focus = self
                .valid_focus(entry.pre_focus)
                .or_else(|| self.topmost_overlay().map(FocusTarget::Overlay));
        }
        self.request_render();
    }

    /// Whether any overlay is visible.
    pub fn has_overlay(&self) -> bool {
        self.overlays
            .iter()
            .any(|entry| !entry.closed.get() && !entry.hidden.get())
    }

    /// Focus a component; an invalid target clears the explicit focus.
    ///
    /// A visible overlay holds the keys: while one is up, focus requests for
    /// anything but the topmost overlay are ignored, as in the reference.
    pub fn set_focus(&mut self, target: Option<FocusTarget>) {
        if let Some(topmost) = self.topmost_overlay() {
            if target != Some(FocusTarget::Overlay(topmost)) {
                return;
            }
        }
        self.focus = self.valid_focus(target);
        self.request_render();
    }

    /// Explicitly focused component, if any.
    pub fn get_focused(&self) -> Option<FocusTarget> {
        self.focus
    }

    /// Register a key listener that sees input before the focused component.
    ///
    /// The returned id removes it via [`Tui::remove_input_listener`].
    pub fn add_input_listener<F>(&mut self, listener: F) -> InputListenerId
    where
        F: FnMut(&Key) -> InputListenerResult + 'static,
    {
        let id = InputListenerId(self.next_input_listener);
        self.next_input_listener += 1;
        self.input_listeners.push((id, Box::new(listener)));
        id
    }

    /// Remove a listener registered by [`Tui::add_input_listener`].
    pub fn remove_input_listener(&mut self, id: InputListenerId) -> bool {
        let before = self.input_listeners.len();
        self.input_listeners.retain(|(listener_id, _)| *listener_id != id);
        before != self.input_listeners.len()
    }

    /// Observe every completed paint. One observer; replaces any previous one.
    pub fn set_on_paint(&mut self, handler: Box<dyn FnMut(&TuiPaint)>) {
        self.on_paint = Some(handler);
    }

    /// Most recent paint, recorded even without an `on_paint` observer.
    pub fn last_paint(&self) -> Option<&TuiPaint> {
        self.last_paint.as_ref()
    }

    /// Install the frame provider; `None` returns to plain child rows.
    pub fn set_frame_provider(&mut self, provider: Option<Box<dyn TerminalFrameProvider>>) {
        self.frame_provider = provider;
        self.request_render();
    }

    /// Install the inline mouse-reporting provider; `None` disables it.
    ///
    /// Reporting is on only while the provider returns true and no overlay
    /// covers the window. Fullscreen overlays keep reporting when
    /// [`OverlayOptions::mouse_tracking`] is true.
    pub fn set_inline_mouse_tracking_provider(
        &mut self,
        provider: Option<Box<dyn Fn() -> bool>>,
    ) {
        self.inline_mouse_provider = provider;
        self.request_render();
    }

    /// The inline painted window: `top` is always 0 and `length` is the last
    /// paint's row count, or 0 while a fullscreen overlay is active.
    pub fn get_mutable_viewport(&self) -> MutableViewport {
        match &self.last_paint {
            Some(paint) if !paint.alt => MutableViewport {
                top: 0,
                length: paint.viewport.len(),
            },
            _ => MutableViewport::default(),
        }
    }

    /// Mark the engine dirty; the next render diffs against the live screen.
    pub fn request_render(&mut self) {
        self.dirty = true;
        self.trigger.request();
    }

    /// Request a frame; `clear_scrollback` repaints from the top and records a
    /// destructive reset paint.
    ///
    /// `history_append` has no separate effect here: the engine owns no native
    /// history, and a frame provider reports history on every frame.
    pub fn request_render_with(
        &mut self,
        _history_append: bool,
        options: RenderRequestOptions,
    ) {
        if options.clear_scrollback {
            self.clear_scrollback = true;
            self.force_full = true;
        }
        self.request_render();
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
        let mouse_bytes = self.mouse_tracking.shutdown();
        if !mouse_bytes.is_empty() {
            self.terminal.write(mouse_bytes);
        }
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
        self.stop()
    }

    /// Render one frame immediately (useful for headless tests).
    pub fn render_now(&mut self) {
        self.sync_overlays();
        let width = self.terminal.columns().max(1);
        let rows_available = self.terminal.rows().max(1) as usize;
        let fullscreen = self
            .topmost_overlay()
            .is_some_and(|id| self.overlay_is_fullscreen(id));
        let mut reset = self.force_full;
        self.force_full = false;
        if fullscreen != self.modal_active {
            self.screen = Screen::new();
            self.modal_active = fullscreen;
            reset = true;
        }
        let want_mouse = match self.topmost_visible_overlay_options() {
            None => {
                if self.inline_mouse_provider.as_ref().is_some_and(|provider| provider()) {
                    MouseTrackingState::Inline
                } else {
                    MouseTrackingState::Off
                }
            }
            Some(options) if options.fullscreen && options.mouse_tracking => {
                MouseTrackingState::Full
            }
            Some(_) => MouseTrackingState::Off,
        };
        let mouse_bytes = self.mouse_tracking.reconcile(want_mouse);
        if !mouse_bytes.is_empty() {
            self.terminal.write(mouse_bytes);
        }
        if self.clear_scrollback {
            self.terminal.write(ERASE_SAVED_LINES);
            self.terminal.clear_scrollback();
            self.last_overflow.clear();
            self.clear_scrollback = false;
            reset = true;
        }
        if reset {
            self.screen = Screen::new();
        }

        let mut history = Vec::new();
        let mut acknowledged = None;
        let mut lines = if let Some(provider) = self.frame_provider.as_mut() {
            let plan = provider.render_frame(ViewportSize {
                columns: width,
                rows: rows_available as u16,
            });
            if let Some(batch) = plan.history {
                history = batch.rows;
                acknowledged = Some(batch.id);
            }
            plan.viewport
        } else if fullscreen {
            let id = self.topmost_overlay().unwrap_or_default();
            self.overlay_component_mut(id)
                .map(|component| component.render(width))
                .unwrap_or_default()
        } else {
            let mut base = Vec::new();
            for child in &mut self.children {
                base.extend(child.render(width));
            }
            let overflow_start = base.len().saturating_sub(rows_available);
            let overflow = base[..overflow_start].to_vec();
            if overflow != self.last_overflow {
                let common = overflow
                    .iter()
                    .zip(&self.last_overflow)
                    .take_while(|(next, previous)| next == previous)
                    .count();
                self.terminal.append_scrollback(&overflow[common..]);
                self.last_overflow = overflow;
            }
            base
        };

        let has_overlay = self
            .overlays
            .iter()
            .any(|entry| !entry.closed.get() && !entry.hidden.get());
        if has_overlay && !fullscreen {
            lines = self.compose_overlays(&lines, width, rows_available);
        }
        if lines.len() > rows_available {
            lines.drain(0..lines.len() - rows_available);
        }
        let cursor = strip_cursor_markers(&mut lines);
        self.terminal.set_hardware_cursor(cursor);
        self.screen.flush(&mut *self.terminal, &lines, width);
        let viewport = self.screen.rows().to_vec();
        self.terminal.set_viewport(&viewport);
        if let (Some(id), Some(provider)) = (acknowledged, self.frame_provider.as_mut()) {
            provider.acknowledge_history(id);
        }

        let paint = TuiPaint {
            history,
            viewport,
            reset,
            alt: fullscreen,
            columns: width,
            rows: self.terminal.rows(),
        };
        self.last_paint = Some(paint.clone());
        if let Some(handler) = self.on_paint.as_mut() {
            handler(&paint);
        }
        self.dirty = false;
    }

    /// Feed a terminal event through the normal input and focus pipeline.
    ///
    /// `run` uses this; tests and debug hooks may call it directly.
    pub fn handle_event(&mut self, event: TtyEvent) {
        self.sync_overlays();
        match event {
            TtyEvent::Resize(_, _) => {
                self.force_full = true;
                self.dirty = true;
            }
            TtyEvent::Key(key) => {
                if let Key::Ctrl('D') = key {
                    if let Some(handler) = self.on_debug.as_mut() {
                        handler();
                    }
                } else if self.dispatch_key(key) {
                    self.request_render();
                }
            }
            TtyEvent::Mouse(mouse) => {
                if let Some(component) = self.focused_component_mut() {
                    component.handle_mouse(&mouse);
                }
                self.request_render();
            }
            TtyEvent::Paste(text) => {
                if let Some(component) = self.focused_component_mut() {
                    component.handle_paste(&text);
                }
                self.request_render();
            }
            TtyEvent::Disconnect => {
                self.stopped = true;
            }
        }
    }

    /// Deliver one key to the listeners, then the focused component. Returns
    /// whether a component consumed it and the frame needs a repaint.
    fn dispatch_key(&mut self, mut key: Key) -> bool {
        for (_, listener) in &mut self.input_listeners {
            let result = listener(&key);
            if let Some(replacement) = result.data {
                key = replacement;
            }
            if result.consume {
                return false;
            }
        }
        let Some(component) = self.focused_component_mut() else {
            return false;
        };
        component.handle_input(&key);
        true
    }

    /// Mutably borrow the focused component, falling back to the last child.
    fn focused_component_mut(&mut self) -> Option<&mut (dyn Component + 'static)> {
        match self.focus {
            Some(FocusTarget::Child(index)) => {
                self.children.get_mut(index).map(|child| child.as_mut())
            }
            Some(FocusTarget::Overlay(id)) => self.overlay_component_mut(id),
            None => self.children.last_mut().map(|child| child.as_mut()),
        }
    }

    fn overlay_component_mut(&mut self, id: u64) -> Option<&mut (dyn Component + 'static)> {
        self.overlays
            .iter_mut()
            .find(|entry| entry.id == id && !entry.closed.get() && !entry.hidden.get())
            .map(|entry| entry.component.as_mut())
    }

    fn overlay_is_fullscreen(&self, id: u64) -> bool {
        self.overlays
            .iter()
            .find(|entry| entry.id == id)
            .is_some_and(|entry| entry.options.fullscreen)
    }

    fn topmost_overlay(&self) -> Option<u64> {
        self.overlays
            .iter()
            .rev()
            .find(|entry| !entry.closed.get() && !entry.hidden.get())
            .map(|entry| entry.id)
    }

    fn topmost_visible_overlay_options(&self) -> Option<OverlayOptions> {
        self.overlays
            .iter()
            .rev()
            .find(|entry| !entry.closed.get() && !entry.hidden.get())
            .map(|entry| entry.options)
    }

    fn valid_focus(&self, target: Option<FocusTarget>) -> Option<FocusTarget> {
        let target = target?;
        match target {
            FocusTarget::Child(index) if index < self.children.len() => Some(target),
            FocusTarget::Overlay(id)
                if self
                    .overlays
                    .iter()
                    .any(|entry| entry.id == id && !entry.closed.get() && !entry.hidden.get()) =>
            {
                Some(target)
            }
            _ => None,
        }
    }

    /// Paint floating overlays onto a viewport-sized canvas over the base rows.
    fn compose_overlays(&mut self, base: &[String], width: u16, rows: usize) -> Vec<String> {
        let mut canvas = vec![String::new(); rows];
        let start = base.len().saturating_sub(rows);
        for (index, line) in base.iter().skip(start).enumerate() {
            canvas[index] = line.clone();
        }
        for entry in &mut self.overlays {
            if entry.closed.get() || entry.hidden.get() {
                continue;
            }
            let options = entry.options;
            let available_height = rows.saturating_sub(options.margin.top + options.margin.bottom);
            if available_height == 0 {
                continue;
            }
            let available_width =
                (width as usize).saturating_sub(options.margin.left + options.margin.right).max(1);
            let overlay_width = options.width.unwrap_or(width as usize).min(available_width).max(1);
            let mut rendered = entry.component.render(overlay_width as u16);
            let height = options.max_height.unwrap_or(available_height).min(available_height);
            if rendered.len() > height {
                if matches!(
                    options.anchor,
                    OverlayAnchor::BottomLeft | OverlayAnchor::Bottom | OverlayAnchor::BottomRight
                ) {
                    rendered.drain(0..rendered.len() - height);
                } else {
                    rendered.truncate(height);
                }
            }
            let y = options.row.map(usize::from).unwrap_or_else(|| match options.anchor {
                OverlayAnchor::BottomLeft | OverlayAnchor::Bottom | OverlayAnchor::BottomRight => {
                    rows.saturating_sub(options.margin.bottom + rendered.len())
                }
                OverlayAnchor::TopLeft | OverlayAnchor::Top | OverlayAnchor::TopRight => {
                    options.margin.top
                }
                _ => options.margin.top + available_height.saturating_sub(rendered.len()) / 2,
            });
            let x = options.col.map(usize::from).unwrap_or_else(|| match options.anchor {
                OverlayAnchor::TopLeft | OverlayAnchor::Left | OverlayAnchor::BottomLeft => {
                    options.margin.left
                }
                OverlayAnchor::TopRight | OverlayAnchor::Right | OverlayAnchor::BottomRight => {
                    (width as usize)
                        .saturating_sub(options.margin.right + overlay_width)
                }
                _ => (width as usize).saturating_sub(overlay_width) / 2,
            });
            for (offset, line) in rendered.into_iter().enumerate() {
                if let Some(slot) = canvas.get_mut(y + offset) {
                    paint_overlay_row(slot, x, overlay_width, &line);
                }
            }
        }
        canvas
    }

    fn sync_overlays(&mut self) {
        let pre_focus = self
            .overlays
            .iter()
            .find(|entry| Some(FocusTarget::Overlay(entry.id)) == self.focus)
            .map(|entry| entry.pre_focus)
            .unwrap_or(None);
        let removed: Vec<(u64, Option<FocusTarget>)> = self
            .overlays
            .iter()
            .filter(|entry| entry.closed.get())
            .map(|entry| (entry.id, entry.pre_focus))
            .collect();
        self.overlays.retain(|entry| !entry.closed.get());
        // An overlay opened above a removed one inherits its predecessor.
        for entry in &mut self.overlays {
            let mut target = entry.pre_focus;
            while let Some(FocusTarget::Overlay(id)) = target {
                let Some((_, predecessor)) = removed.iter().find(|(removed, _)| *removed == id) else {
                    break;
                };
                target = *predecessor;
            }
            entry.pre_focus = target;
        }
        if self.valid_focus(self.focus).is_none() {
            let mut target = pre_focus;
            while let Some(FocusTarget::Overlay(id)) = target {
                let Some((_, predecessor)) = removed.iter().find(|(removed, _)| *removed == id) else {
                    break;
                };
                target = *predecessor;
            }
            self.focus = self
                .valid_focus(target)
                .or_else(|| self.topmost_overlay().map(FocusTarget::Overlay));
        }
        if let Some(FocusTarget::Child(index)) = self.focus {
            if index >= self.children.len() {
                self.focus = None;
            }
        }
    }
}

fn paint_overlay_row(row: &mut String, x: usize, width: usize, content: &str) {
    let content = truncate_to_width(content, width, Ellipsis::Omit);
    let content: Vec<char> = content.chars().collect();
    let mut chars: Vec<char> = row.chars().collect();
    if chars.len() < x + width.max(content.len()) {
        chars.resize(x + width.max(content.len()), ' ');
    }
    for (offset, ch) in content.iter().enumerate() {
        chars[x + offset] = *ch;
    }
    let fill = width.saturating_sub(content.len());
    for slot in chars.iter_mut().skip(x + content.len()).take(fill) {
        *slot = ' ';
    }
    *row = chars.into_iter().collect();
}

fn strip_cursor_markers(lines: &mut [String]) -> Option<(u16, u16)> {
    const MARKER: &str = crate::native::describe::CURSOR_MARKER;
    let mut position = None;
    for (row, line) in lines.iter_mut().enumerate() {
        if let Some(index) = line.find(MARKER) {
            if position.is_none() {
                position = Some((row as u16, line[..index].chars().count() as u16));
            }
            *line = line.replace(MARKER, "");
        }
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct MockTerminal {
        writes: Vec<String>,
        columns: u16,
        rows: u16,
    }

    impl MockTerminal {
        fn new() -> Self {
            Self {
                writes: Vec::new(),
                columns: 80,
                rows: 24,
            }
        }
    }

    impl Terminal for MockTerminal {
        fn start(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn stop(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write(&mut self, data: &str) {
            self.writes.push(data.to_string());
        }
        fn event(&mut self, _timeout: Duration) -> Option<TtyEvent> {
            None
        }
        fn columns(&self) -> u16 {
            self.columns
        }
        fn rows(&self) -> u16 {
            self.rows
        }
        fn move_by(&mut self, _lines: i32) {}
        fn hide_cursor(&mut self, _force: bool) {}
        fn show_cursor(&mut self, _force: bool) {}
        fn clear_line(&mut self) {}
        fn clear_from_cursor(&mut self) {}
        fn clear_screen(&mut self) {}
    }

    struct Recorder {
        keys: Rc<RefCell<Vec<Key>>>,
        rows: Vec<String>,
    }

    impl Recorder {
        fn new(rows: &[&str]) -> Self {
            Self {
                keys: Rc::new(RefCell::new(Vec::new())),
                rows: rows.iter().map(|row| row.to_string()).collect(),
            }
        }
    }

    impl Component for Recorder {
        fn render(&mut self, _width: u16) -> Vec<String> {
            self.rows.clone()
        }
        fn handle_input(&mut self, key: &Key) {
            self.keys.borrow_mut().push(key.clone());
        }
    }

    use std::cell::Cell;

    struct Provider {
        frames: Rc<Cell<usize>>,
        acknowledged: Rc<Cell<Option<u64>>>,
    }

    impl TerminalFrameProvider for Provider {
        fn render_frame(&mut self, viewport: ViewportSize) -> TerminalFramePlan {
            self.frames.set(self.frames.get() + 1);
            TerminalFramePlan {
                history: Some(HistoryBatch {
                    id: 7,
                    rows: vec![format!("history {}x{}", viewport.columns, viewport.rows)],
                    kind: HistoryKind::Append,
                }),
                viewport: vec!["provider".to_string()],
            }
        }

        fn acknowledge_history(&mut self, id: u64) {
            self.acknowledged.set(Some(id));
        }
    }

    #[test]
    fn frame_provider_supplies_history_and_viewport() {
        let mut tui = Tui::new(Box::new(MockTerminal::new()));
        let frames = Rc::new(Cell::new(0));
        let acknowledged = Rc::new(Cell::new(None));
        tui.set_frame_provider(Some(Box::new(Provider {
            frames: Rc::clone(&frames),
            acknowledged: Rc::clone(&acknowledged),
        })));
        let observed = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&observed);
        tui.set_on_paint(Box::new(move |paint| *sink.borrow_mut() = Some(paint.clone())));

        tui.render_now();
        let paint = tui.last_paint().unwrap();
        assert_eq!(paint.history, vec!["history 80x24".to_string()]);
        assert_eq!(paint.viewport, vec!["provider".to_string()]);
        assert_eq!(frames.get(), 1);
        assert_eq!(acknowledged.get(), Some(7));
        assert_eq!(observed.borrow().as_ref(), Some(paint));
    }

    #[test]
    fn hidden_overlay_keeps_its_stack_entry() {
        let mut tui = Tui::new(Box::new(MockTerminal::new()));
        tui.add_child(Recorder::new(&["child"]));
        let overlay = tui.show_overlay(Recorder::new(&["overlay"]), OverlayOptions::default());
        overlay.set_hidden(true);
        tui.render_now();
        assert_eq!(tui.last_paint().unwrap().viewport, vec!["child".to_string()]);
        assert!(!tui.has_overlay());

        overlay.set_hidden(false);
        tui.render_now();
        assert!(tui.has_overlay());
        assert_eq!(
            tui.last_paint().unwrap().viewport,
            vec!["child".to_string(), "overlay".to_string()]
        );
    }
    #[test]
    fn input_routes_to_explicit_focus_not_last_child() {
        let mut tui = Tui::new(Box::new(MockTerminal::new()));
        let first = Recorder::new(&["first"]);
        let first_keys = Rc::clone(&first.keys);
        let second = Recorder::new(&["second"]);
        let second_keys = Rc::clone(&second.keys);
        tui.add_child(first);
        tui.add_child(second);
        tui.set_focus(Some(FocusTarget::Child(0)));
        tui.handle_event(TtyEvent::Key(Key::Char('a')));
        assert_eq!(*first_keys.borrow(), [Key::Char('a')]);
        assert!(second_keys.borrow().is_empty());
    }
    #[test]
    fn overlay_hide_restores_focus_and_stops_rendering() {
        let mut tui = Tui::new(Box::new(MockTerminal::new()));
        tui.add_child(Recorder::new(&["child"]));
        tui.set_focus(Some(FocusTarget::Child(0)));
        let overlay = tui.show_overlay(Recorder::new(&["overlay"]), OverlayOptions::default());
        assert_eq!(tui.get_focused(), Some(FocusTarget::Overlay(overlay.id())));
        tui.render_now();
        assert_eq!(
            tui.last_paint().unwrap().viewport,
            vec!["child".to_string(), "overlay".to_string()]
        );

        overlay.hide();
        tui.render_now();
        assert_eq!(tui.get_focused(), Some(FocusTarget::Child(0)));
        assert_eq!(
            tui.last_paint().unwrap().viewport,
            vec!["child".to_string()]
        );
        assert!(!tui.has_overlay());
    }

    #[test]
    fn input_listener_consumes_before_focused_component() {
        let mut tui = Tui::new(Box::new(MockTerminal::new()));
        let child = Recorder::new(&["child"]);
        let keys = Rc::clone(&child.keys);
        tui.add_child(child);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let listener_seen = Rc::clone(&seen);
        tui.add_input_listener(move |key| {
            listener_seen.borrow_mut().push(key.clone());
            InputListenerResult::consume()
        });
        tui.handle_event(TtyEvent::Key(Key::Char('x')));
        assert_eq!(*seen.borrow(), [Key::Char('x')]);
        assert!(keys.borrow().is_empty());
    }

    #[test]
    fn paint_records_geometry_and_fullscreen_alt_frame() {
        let mut tui = Tui::new(Box::new(MockTerminal::new()));
        tui.add_child(Recorder::new(&["child"]));
        tui.render_now();
        let paint = tui.last_paint().unwrap();
        assert_eq!((paint.columns, paint.rows), (80, 24));
        assert!(!paint.alt);

        let overlay = tui.show_overlay(
            Recorder::new(&["modal"]),
            OverlayOptions { fullscreen: true, ..Default::default() },
        );
        tui.render_now();
        let paint = tui.last_paint().unwrap();
        assert!(paint.alt);
        assert_eq!(paint.viewport, vec!["modal".to_string()]);

        overlay.hide();
        tui.render_now();
        assert!(!tui.last_paint().unwrap().alt);
    }
}
