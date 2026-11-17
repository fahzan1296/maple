//! Mouse event model shared by the terminal backend and components.
//! Reference: `l/src/mouse.ts`.

use crossterm::event::{MouseButton, MouseEvent as CrosstermMouseEvent, MouseEventKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButtonKind {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Down(MouseButtonKind),
    Up(MouseButtonKind),
    Drag(MouseButtonKind),
    Moved,
    ScrollUp,
    ScrollDown,
}

/// A pointer event expressed in terminal cell coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseKind,
    pub column: u16,
    pub row: u16,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl MouseEvent {
    pub fn button(&self) -> Option<MouseButtonKind> {
        match self.kind {
            MouseKind::Down(b) | MouseKind::Up(b) | MouseKind::Drag(b) => Some(b),
            _ => None,
        }
    }

    pub fn is_scroll_up(&self) -> bool {
        self.kind == MouseKind::ScrollUp
    }

    pub fn is_scroll_down(&self) -> bool {
        self.kind == MouseKind::ScrollDown
    }
}

pub fn from_crossterm(event: CrosstermMouseEvent) -> MouseEvent {
    let kind = match event.kind {
        MouseEventKind::Down(button) => MouseKind::Down(map_button(button)),
        MouseEventKind::Up(button) => MouseKind::Up(map_button(button)),
        MouseEventKind::Drag(button) => MouseKind::Drag(map_button(button)),
        MouseEventKind::Moved => MouseKind::Moved,
        MouseEventKind::ScrollUp => MouseKind::ScrollUp,
        MouseEventKind::ScrollDown => MouseKind::ScrollDown,
        MouseEventKind::ScrollLeft => MouseKind::Moved,
        MouseEventKind::ScrollRight => MouseKind::Moved,
    };
    MouseEvent {
        kind,
        column: event.column,
        row: event.row,
        shift: event.modifiers.contains(crossterm::event::KeyModifiers::SHIFT),
        ctrl: event.modifiers.contains(crossterm::event::KeyModifiers::CONTROL),
        alt: event.modifiers.contains(crossterm::event::KeyModifiers::ALT),
    }
}

fn map_button(button: MouseButton) -> MouseButtonKind {
    match button {
        MouseButton::Left => MouseButtonKind::Left,
        MouseButton::Right => MouseButtonKind::Right,
        MouseButton::Middle => MouseButtonKind::Middle,
    }
}
