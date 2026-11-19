# maple-tui implementation contract

Clean-room Rust port of `maple-tui` v18.20.2 (`l/`). All new modules live
under `crates/maple-tui/src/`. Do **not** edit `Cargo.toml`, `src/lib.rs`,
`src/components/mod.rs`, `src/terminal/**`, or another agent's files: list your
new modules in the task output and the integrator wires them.

## Hard rules

- `use crate::...` paths only; no external crates beyond those already in
  `Cargo.toml` (ratatui, crossterm, anyhow, clap, libc, serde, serde_json, sha2,
  time, uuid). If a dependency is missing, say so in the output instead.
- `#[allow(dead_code)]` is forbidden at module level. Prefix unused private
  items with `_` or delete them.
- Every public struct/method gets a `///` doc comment.
- Deterministic, side-effect-free rendering: `render` never touches I/O.
- Tests are inline `#[cfg(test)] mod tests`; no network, no filesystem.
- Every file compiles standalone under `edition = "2021"`.
- Follow existing style: 4-space indent, `impl Default` when a zero value makes
  sense, no `unwrap` on user input paths.

## Core types (already implemented — read these files before using)

```rust
// component.rs
pub trait Component {
    fn render(&mut self, width: u16) -> Vec<String>;
    fn handle_input(&mut self, _key: &Key) {}
    fn handle_mouse(&mut self, _event: &MouseEvent) {}
    fn handle_paste(&mut self, _text: &str) {}
    fn invalidate(&mut self) {}
    fn dispose(&mut self) {}
}

// keys.rs
pub enum Key {
    Char(char), Ctrl(char), Alt(char), Shift(char),
    Enter, Esc, Tab, BackTab, Backspace, Delete, Insert,
    Home, End, PageUp, PageDown, Paste(String),
    CtrlUp, CtrlDown, CtrlLeft, CtrlRight,
    Up, Down, Left, Right, F(u8), Unknown,
}
// predicates: is_enter/is_escape/is_tab/is_shift_tab/is_backspace/is_delete,
// is_arrow_up/down/left/right/is_home/is_end/is_page_up/is_page_down,
// is_ctrl_a/c/d/e/k/o/p/u/w/is_ctrl_left/right/is_alt_left/right,
// is_shift_enter/is_alt_enter/is_f(n)

// style.rs
pub enum Color { Reset, Black..White, Rgb(u8,u8,u8), Indexed(u8) }
pub struct Style { fg: Option<Color>, bg: Option<Color>, bold, dim, italic,
                   underline, strikethrough, reverse, hidden }
// Style::fg/bg/bold/dim/italic/underline/strikethrough/reverse/hidden,
// merge(base, over), paint(&str) -> String, is_default()

// text_shape.rs
pub enum Ellipsis { Omit, Unicode }
pub fn visible_width(&str) -> usize;   // ANSI-aware
pub fn plain_width(&str) -> usize;
pub fn truncate_to_width(&str, usize, Ellipsis) -> String;
pub fn wrap_text_with_ansi(&str, usize) -> Vec<String>;
pub fn pad_to_width(&str, usize) -> String;

// symbols.rs
pub struct LineSymbols { top_left, top_right, bottom_left, bottom_right,
                         horizontal, vertical: String }
pub struct TableSymbols { ... + tee_down/tee_up/tee_left/tee_right/cross }
pub struct SymbolTheme { box_round, box_sharp, table, quote_border: String,
                         hr_char: char, spinner_frames: Vec<String> }
SymbolTheme::unicode()/ascii()/set_spinner_frames(Vec<String>)

// mouse.rs
pub struct MouseEvent { kind, column: u16, row: u16, shift, ctrl, alt }
pub enum MouseKind { Down(Button), Up, Drag, Moved, ScrollUp, ScrollDown }

// trigger.rs
pub struct Trigger { request(), take(), wait_for_request(Duration) -> bool }

// terminal/mod.rs
pub enum TtyEvent { Key(Key), Mouse(MouseEvent), Paste(String),
                    Resize(u16,u16), Disconnect }
pub trait Terminal { start, stop, write, poll_event, event, columns, rows,
                      move_by, hide_cursor, show_cursor, clear_line,
                      clear_from_cursor, clear_screen }
```

## Component authoring rules

1. Structs own plain state (`Vec<String>` rows, `usize` indices, `String`
   buffers). `render(&mut self, width)` recomputes rows when state changed;
   keep a `dirty: bool` or recompute cheaply, then return `Vec<String>`.
2. Rows MUST be `<= width` visible cells. Truncate with
   `truncate_to_width(row, width as usize, Ellipsis::Omit)`; never panic.
3. Themes are plain structs of `Style` fields implementing `Default`
   (semantic defaults, e.g. muted label `Color::DarkGray`). Keep a
   `#[derive(Clone)]` where callbacks might clone.
4. Callbacks: `on_submit: Option<Box<dyn FnMut(&str) + 'static>>` or store
   with `Box<dyn FnMut(...) + 'static>`; expose `set_*` builder methods that
   return `&mut Self` or `Self`.
5. Focus: components that render a cursor must include
   `crate::component::CURSOR_MARKER` before the cursor cell (define/import the
   constant if absent — check `component.rs`; if missing, report it).
6. `handle_input` consumes only keys the component handles; return `()` and
   leave the key unconsumed when it does not apply. There is no return value;
   the TUI forwards unhandled keys to no one, so only handle what you own.
7. Scrolling/selection state changes on `handle_input`; `render` reflects it.
8. No `Arc<Mutex<...>>` shared with the event loop unless the component must
   mutate from another thread — then document why.

## Verification expected per slice

- Unit tests for: state mutation per key, render shape (exact row strings for a
  fixed width), boundary handling (empty input, zero width, narrow width),
  and callback firing.
- Do not run `cargo build`/`cargo test` (the integrator compiles everything
  once). Write code as if the whole crate must compile together.
