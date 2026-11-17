//! Reference: `l/test/overlay-focus.test.ts`.
//!
//! Deviations forced by this crate's public model:
//! - Components are addressed by [`FocusTarget`] (top-level child index or
//!   overlay id), not by reference, and nested focus (`Container` slots,
//!   `OverlayFocusOwner`) is not ported. Focus therefore follows the child slot
//!   index, not a live component identity.
//! - `Component` has no `capturesInput` preflight; input listeners are the only
//!   pre-component hook.
//! - A focused overlay with no `handle_input` does not fall back to the editor
//!   it displaced: input stays with the focused target.
//! - `OverlayHandle::hide` is observed by the next event/render pass, so the
//!   tests render (or input) once before asserting the restored focus.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use maple_tui::tui::{FocusTarget, InputListenerResult, OverlayOptions, Tui};
use maple_tui::{Component, Key, Terminal, TtyEvent};

struct TestTerminal;

impl Terminal for TestTerminal {
    fn start(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn stop(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write(&mut self, _data: &str) {}
    fn event(&mut self, _timeout: Duration) -> Option<TtyEvent> {
        None
    }
    fn columns(&self) -> u16 {
        80
    }
    fn rows(&self) -> u16 {
        24
    }
    fn move_by(&mut self, _lines: i32) {}
    fn hide_cursor(&mut self, _force: bool) {}
    fn show_cursor(&mut self, _force: bool) {}
    fn clear_line(&mut self) {}
    fn clear_from_cursor(&mut self) {}
    fn clear_screen(&mut self) {}
}

struct Recorder {
    label: String,
    inputs: Rc<RefCell<Vec<Key>>>,
}

impl Recorder {
    fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl Component for Recorder {
    fn render(&mut self, _width: u16) -> Vec<String> {
        vec![self.label.clone()]
    }

    fn handle_input(&mut self, key: &Key) {
        self.inputs.borrow_mut().push(key.clone());
    }
}

fn key() -> TtyEvent {
    TtyEvent::Key(Key::Char('x'))
}

#[test]
fn focused_overlay_keeps_keys_instead_of_the_displaced_editor() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let editor = Recorder::new("editor");
    let editor_inputs = Rc::clone(&editor.inputs);
    tui.add_child(editor);
    tui.set_focus(Some(FocusTarget::Child(0)));

    let sidebar = Recorder::new("sidebar");
    let sidebar_inputs = Rc::clone(&sidebar.inputs);
    let sidebar_id = tui.show_overlay(sidebar, OverlayOptions::default()).id();
    tui.handle_event(key());

    assert_eq!(tui.get_focused(), Some(FocusTarget::Overlay(sidebar_id)));
    assert_eq!(*sidebar_inputs.borrow(), [Key::Char('x')]);
    assert!(editor_inputs.borrow().is_empty());
}

#[test]
fn hide_restores_the_displaced_child_focus_and_input() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let editor = Recorder::new("editor");
    let editor_inputs = Rc::clone(&editor.inputs);
    tui.add_child(editor);
    tui.set_focus(Some(FocusTarget::Child(0)));

    let sidebar = Recorder::new("sidebar");
    let sidebar_inputs = Rc::clone(&sidebar.inputs);
    let handle = tui.show_overlay(sidebar, OverlayOptions::default());
    tui.handle_event(key());
    assert_eq!(*sidebar_inputs.borrow(), [Key::Char('x')]);

    handle.hide();
    tui.render_now();
    assert_eq!(tui.get_focused(), Some(FocusTarget::Child(0)));
    tui.handle_event(TtyEvent::Key(Key::Char('y')));
    assert_eq!(*editor_inputs.borrow(), [Key::Char('y')]);
}

#[test]
fn input_listener_consumes_before_the_focused_component() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let editor = Recorder::new("editor");
    let editor_inputs = Rc::clone(&editor.inputs);
    tui.add_child(editor);
    tui.set_focus(Some(FocusTarget::Child(0)));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let listener_seen = Rc::clone(&seen);
    tui.add_input_listener(move |key| {
        listener_seen.borrow_mut().push(key.clone());
        InputListenerResult::consume()
    });

    tui.handle_event(key());

    assert_eq!(*seen.borrow(), [Key::Char('x')]);
    assert!(editor_inputs.borrow().is_empty());
}

#[test]
fn multiple_paint_only_overlays_do_not_pass_through_to_the_child() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let editor = Recorder::new("editor");
    let editor_inputs = Rc::clone(&editor.inputs);
    tui.add_child(editor);
    tui.set_focus(Some(FocusTarget::Child(0)));

    let sidebar = Recorder::new("sidebar");
    let sidebar_inputs = Rc::clone(&sidebar.inputs);
    let sidebar_id = tui.show_overlay(sidebar, OverlayOptions::default()).id();
    let decoration = Recorder::new("decoration");
    let decoration_inputs = Rc::clone(&decoration.inputs);
    let decoration_id = tui.show_overlay(decoration, OverlayOptions::default()).id();

    tui.handle_event(key());
    assert_eq!(tui.get_focused(), Some(FocusTarget::Overlay(decoration_id)));
    assert_eq!(*decoration_inputs.borrow(), [Key::Char('x')]);
    assert!(sidebar_inputs.borrow().is_empty());
    assert!(editor_inputs.borrow().is_empty());

    tui.hide_overlay();
    tui.render_now();
    assert_eq!(tui.get_focused(), Some(FocusTarget::Overlay(sidebar_id)));
    tui.handle_event(TtyEvent::Key(Key::Char('y')));
    assert_eq!(*sidebar_inputs.borrow(), [Key::Char('y')]);
    assert!(editor_inputs.borrow().is_empty());
}

#[test]
fn focus_requests_cannot_leave_a_visible_fullscreen_overlay() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let editor = Recorder::new("editor");
    let editor_inputs = Rc::clone(&editor.inputs);
    tui.add_child(editor);
    tui.set_focus(Some(FocusTarget::Child(0)));

    let settings = Recorder::new("settings");
    let settings_inputs = Rc::clone(&settings.inputs);
    let settings_id = tui
        .show_overlay(settings, OverlayOptions { fullscreen: true, ..Default::default() })
        .id();
    tui.set_focus(Some(FocusTarget::Child(0)));
    tui.handle_event(key());

    assert_eq!(tui.get_focused(), Some(FocusTarget::Overlay(settings_id)));
    assert_eq!(*settings_inputs.borrow(), [Key::Char('x')]);
    assert!(editor_inputs.borrow().is_empty());
}

#[test]
fn an_overlay_cannot_delegate_focus_to_a_child() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let prompt = Recorder::new("prompt");
    let prompt_inputs = Rc::clone(&prompt.inputs);
    tui.add_child(prompt);
    tui.set_focus(Some(FocusTarget::Child(0)));

    let wizard = Recorder::new("wizard");
    let wizard_inputs = Rc::clone(&wizard.inputs);
    let wizard_id = tui.show_overlay(wizard, OverlayOptions::default()).id();
    tui.set_focus(Some(FocusTarget::Child(0)));
    tui.handle_event(key());

    assert_eq!(tui.get_focused(), Some(FocusTarget::Overlay(wizard_id)));
    assert_eq!(*wizard_inputs.borrow(), [Key::Char('x')]);
    assert!(prompt_inputs.borrow().is_empty());
}

#[test]
fn closing_a_fullscreen_overlay_can_hand_focus_to_the_live_slot_owner() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let stale_editor = Recorder::new("editor");
    let stale_inputs = Rc::clone(&stale_editor.inputs);
    tui.add_child(stale_editor);
    tui.set_focus(Some(FocusTarget::Child(0)));

    let handle = tui.show_overlay(
        Recorder::new("settings"),
        OverlayOptions { fullscreen: true, ..Default::default() },
    );
    tui.remove_child(0);
    let live = Recorder::new("approval");
    let live_inputs = Rc::clone(&live.inputs);
    tui.add_child(live);

    handle.hide();
    tui.render_now();
    tui.set_focus(Some(FocusTarget::Child(0)));
    tui.handle_event(TtyEvent::Key(Key::Char('a')));

    assert_eq!(tui.get_focused(), Some(FocusTarget::Child(0)));
    assert_eq!(*live_inputs.borrow(), [Key::Char('a')]);
    assert!(stale_inputs.borrow().is_empty());
}

#[test]
fn hide_alone_restores_the_slot_index_not_the_stale_component() {
    let mut tui = Tui::new(Box::new(TestTerminal));
    let stale_editor = Recorder::new("editor");
    let stale_inputs = Rc::clone(&stale_editor.inputs);
    tui.add_child(stale_editor);
    tui.set_focus(Some(FocusTarget::Child(0)));

    let handle = tui.show_overlay(
        Recorder::new("settings"),
        OverlayOptions { fullscreen: true, ..Default::default() },
    );
    tui.remove_child(0);
    let live = Recorder::new("approval");
    let live_inputs = Rc::clone(&live.inputs);
    tui.add_child(live);

    handle.hide();
    tui.handle_event(TtyEvent::Key(Key::Char('a')));

    assert_eq!(tui.get_focused(), Some(FocusTarget::Child(0)));
    assert_eq!(*live_inputs.borrow(), [Key::Char('a')]);
    assert!(stale_inputs.borrow().is_empty());
}
