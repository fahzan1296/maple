//! Port of `l/test/overlay-scroll.test.ts`.
//!
//! Drives the real `Tui` over `VirtualTerminal` and asserts scrollback growth,
//! overlay placement/clamping and hardware-cursor updates across resizes.

use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;
use std::time::Duration;

use maple_tui::component::Component;
use maple_tui::native::describe::CURSOR_MARKER;
use maple_tui::terminal::virt::VirtualTerminal;
use maple_tui::terminal::{Terminal, TtyEvent};
use maple_tui::tui::{OverlayAnchor, OverlayMargin, OverlayOptions, Tui};

#[derive(Clone)]
struct Shared(Rc<RefCell<VirtualTerminal>>);

impl Shared {
    fn new(width: u16, height: u16) -> Self {
        Self(Rc::new(RefCell::new(VirtualTerminal::new(width, height))))
    }

    fn resize(&self, width: u16, height: u16) {
        self.0.borrow_mut().resize(width, height);
    }

    fn viewport(&self) -> Vec<String> {
        self.0.borrow().viewport_rows()
    }

    fn scrollback_len(&self) -> usize {
        self.0.borrow().scrollback().len()
    }

    fn scrollback_rows(&self) -> Vec<String> {
        self.0.borrow().scrollback().to_vec()
    }

    fn hardware_cursor(&self) -> Option<(u16, u16)> {
        self.0.borrow().hardware_cursor()
    }
}

impl Terminal for Shared {
    fn start(&mut self) -> io::Result<()> {
        self.0.borrow_mut().start()
    }
    fn stop(&mut self) -> io::Result<()> {
        self.0.borrow_mut().stop()
    }
    fn write(&mut self, data: &str) {
        self.0.borrow_mut().write(data);
    }
    fn event(&mut self, timeout: Duration) -> Option<TtyEvent> {
        self.0.borrow_mut().event(timeout)
    }
    fn columns(&self) -> u16 {
        self.0.borrow().columns()
    }
    fn rows(&self) -> u16 {
        self.0.borrow().rows()
    }
    fn move_by(&mut self, lines: i32) {
        self.0.borrow_mut().move_by(lines);
    }
    fn hide_cursor(&mut self, force: bool) {
        self.0.borrow_mut().hide_cursor(force);
    }
    fn show_cursor(&mut self, force: bool) {
        self.0.borrow_mut().show_cursor(force);
    }
    fn clear_line(&mut self) {
        self.0.borrow_mut().clear_line();
    }
    fn clear_from_cursor(&mut self) {
        self.0.borrow_mut().clear_from_cursor();
    }
    fn clear_screen(&mut self) {
        self.0.borrow_mut().clear_screen();
    }
    fn append_scrollback(&mut self, rows: &[String]) {
        self.0.borrow_mut().append_scrollback(rows);
    }
    fn set_viewport(&mut self, rows: &[String]) {
        self.0.borrow_mut().set_viewport(rows);
    }
    fn set_hardware_cursor(&mut self, position: Option<(u16, u16)>) {
        self.0.borrow_mut().set_hardware_cursor(position);
    }
    fn clear_scrollback(&mut self) {
        self.0.borrow_mut().clear_scrollback();
    }
}

fn owned_lines(rows: Vec<String>) -> (Lines, Rc<RefCell<Vec<String>>>) {
    let state = Rc::new(RefCell::new(rows));
    (Lines { lines: Rc::clone(&state) }, state)
}

struct Lines {
    lines: Rc<RefCell<Vec<String>>>,
}

impl Component for Lines {
    fn render(&mut self, _width: u16) -> Vec<String> {
        self.lines.borrow().clone()
    }
}

fn lines(rows: &[&str]) -> (Lines, Rc<RefCell<Vec<String>>>) {
    let state = Rc::new(RefCell::new(rows.iter().map(|row| row.to_string()).collect()));
    (Lines { lines: Rc::clone(&state) }, state)
}

fn build_rows(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("row-{index}")).collect()
}

struct CursorText {
    col: Rc<Cell<usize>>,
}

impl Component for CursorText {
    fn render(&mut self, _width: u16) -> Vec<String> {
        let mut row = "cursor-anchor".to_string();
        row.insert_str(self.col.get(), CURSOR_MARKER);
        vec![row]
    }
}

struct OverlayText {
    text: Rc<RefCell<String>>,
    cursor: bool,
}

impl Component for OverlayText {
    fn render(&mut self, _width: u16) -> Vec<String> {
        let mut row = self.text.borrow().clone();
        if self.cursor {
            row.push_str(CURSOR_MARKER);
        }
        vec![row]
    }
}

fn viewport_row_numbers(shared: &Shared) -> Vec<usize> {
    shared
        .viewport()
        .iter()
        .filter_map(|line| line.trim().strip_prefix("row-"))
        .filter_map(|index| index.parse().ok())
        .collect()
}

fn longest_blank_run(lines: &[String]) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for line in lines {
        if line.trim().is_empty() {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

fn resize(shared: &Shared, tui: &mut Tui, width: u16, height: u16) {
    shared.resize(width, height);
    tui.handle_event(TtyEvent::Resize(width, height));
}

#[test]
fn does_not_scroll_the_terminal_when_an_overlay_is_shown_with_a_large_historical_working_area() {
    let terminal = Shared::new(80, 24);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&["base-0", "base-1", "base-2", "base-3", "base-4"]).0);
    tui.render_now();
    tui.show_overlay(lines(&["overlay-0", "overlay-1", "overlay-2"]).0, OverlayOptions::default());
    tui.render_now();
    assert!(terminal.scrollback_len() < 200);
    assert!(terminal.viewport().iter().any(|row| row.trim() == "overlay-2"));
}

#[test]
fn keeps_the_native_viewport_anchored_when_an_overlay_repaint_follows_a_focused_cursor_below_the_frame_tail() {
    let terminal = Shared::new(24, 6);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let (base, base_rows) = lines(&[]);
    *base_rows.borrow_mut() = build_rows(8);
    tui.add_child(base);
    let cursor_text = Rc::new(RefCell::new("overlay-cursor".to_string()));
    tui.show_overlay(
        OverlayText { text: Rc::clone(&cursor_text), cursor: true },
        OverlayOptions { row: Some(5), col: Some(0), width: Some(16), ..Default::default() },
    );
    let status_text = Rc::new(RefCell::new("status-before".to_string()));
    tui.show_overlay(
        OverlayText { text: Rc::clone(&status_text), cursor: false },
        OverlayOptions { row: Some(0), col: Some(0), width: Some(16), ..Default::default() },
    );
    tui.render_now();
    *base_rows.borrow_mut() = vec!["base-0".into(), "base-1".into()];
    tui.render_now();
    assert_eq!(terminal.hardware_cursor().map(|(row, _)| row), Some(5));
    let scrollback_before = terminal.scrollback_len();

    *status_text.borrow_mut() = "status-after".to_string();
    tui.request_render();
    tui.render_now();
    assert_eq!(terminal.scrollback_len(), scrollback_before);
    let expected: Vec<String> =
        ["status-after", "base-1", "", "", "", "overlay-cursor"].iter().map(|s| s.to_string()).collect();
    assert_eq!(terminal.viewport(), expected);
    assert_eq!(terminal.hardware_cursor().map(|(row, _)| row), Some(5));
}

#[test]
fn clamps_tall_overlays_without_an_explicit_max_height_to_the_available_rows() {
    let terminal = Shared::new(80, 24);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&["base-0", "base-1", "base-2"]).0);
    tui.render_now();
    tui.show_overlay(
        owned_lines((0..40).map(|i| format!("ov-{i}")).collect()).0,
        OverlayOptions {
            anchor: OverlayAnchor::Top,
            margin: OverlayMargin { bottom: 6, ..Default::default() },
            ..Default::default()
        },
    );
    tui.render_now();
    let max_overlay = |rows: Vec<String>| {
        rows.iter()
            .filter_map(|row| row.trim().strip_prefix("ov-"))
            .filter_map(|index| index.parse::<usize>().ok())
            .max()
    };
    let max = max_overlay(terminal.viewport()).expect("overlay row");
    assert!(max < 24 - 6);
    resize(&terminal, &mut tui, 80, 10);
    tui.render_now();
    let max = max_overlay(terminal.viewport()).expect("overlay row");
    assert!(max < 10 - 6);
}

#[test]
fn preserves_bottom_anchored_overlay_actions_when_clamped() {
    let terminal = Shared::new(80, 5);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&["base-0"]).0);
    tui.render_now();
    tui.show_overlay(
        owned_lines((0..10).map(|i| format!("ov-{i}")).collect()).0,
        OverlayOptions {
            anchor: OverlayAnchor::Bottom,
            width: Some(80),
            max_height: Some(5),
            ..Default::default()
        },
    );
    tui.render_now();
    let viewport = terminal.viewport().join("\n");
    assert!(viewport.contains("ov-5"));
    assert!(viewport.contains("ov-9"));
    assert!(!viewport.contains("ov-0"));
}

#[test]
fn clears_stale_viewport_content_on_launch() {
    let terminal = Shared::new(40, 4);
    terminal.0.borrow_mut().write("shell-0\r\nshell-1\r\nshell-2\r\nshell-3\r\nshell-4\r\n");
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&["ui-0", "ui-1"]).0);
    tui.render_now();
    assert_eq!(terminal.viewport(), vec!["ui-0".to_string(), "ui-1".to_string()]);
}

#[test]
fn does_not_duplicate_transcript_into_scrollback_on_repeated_forced_redraws() {
    let terminal = Shared::new(40, 4);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&build_rows(60).iter().map(String::as_str).collect::<Vec<_>>()).0);
    tui.render_now();
    let baseline = terminal.scrollback_len();
    for _ in 0..5 {
        tui.request_render_with(true, Default::default());
        tui.render_now();
    }
    assert!(terminal.scrollback_len() <= baseline + 4);
}

#[test]
fn fully_redraws_on_height_increase_to_avoid_stale_viewport_rows() {
    let terminal = Shared::new(40, 4);
    terminal.0.borrow_mut().write("shell-0\r\nshell-1\r\nshell-2\r\nshell-3\r\nshell-4\r\n");
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&["ui-0", "ui-1", "ui-2", "ui-3"]).0);
    tui.render_now();
    resize(&terminal, &mut tui, 40, 8);
    tui.render_now();
    assert!(!terminal.viewport().join("\n").contains("shell-"));
}

#[test]
fn keeps_single_viewport_copy_under_simultaneous_height_and_content_changes() {
    let terminal = Shared::new(60, 8);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let (component, rows) = lines(&[]);
    tui.add_child(component);
    for index in 0..12 {
        *rows.borrow_mut() = build_rows(4 + index);
        resize(&terminal, &mut tui, 60, if index % 2 == 0 { 7 } else { 9 });
        tui.render_now();
        let numbers = viewport_row_numbers(&terminal);
        let mut unique = numbers.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), numbers.len());
    }
    assert_eq!(terminal.viewport().last().unwrap().trim(), "row-14");
}

#[test]
fn keeps_scrollback_bounded_on_resize_when_content_size_is_stable() {
    let terminal = Shared::new(60, 8);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&build_rows(140).iter().map(String::as_str).collect::<Vec<_>>()).0);
    tui.render_now();
    let before = terminal.scrollback_len();
    for index in 0..8 {
        resize(
            &terminal,
            &mut tui,
            if index % 2 == 0 { 59 } else { 60 },
            if index % 2 == 0 { 9 } else { 8 },
        );
        tui.render_now();
    }
    assert!(terminal.scrollback_len() - before < 120);
}

#[test]
fn renders_a_fresh_viewport_on_resize_when_content_grows_before_resize() {
    let terminal = Shared::new(60, 8);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let (component, rows) = lines(&[]);
    *rows.borrow_mut() = build_rows(8);
    tui.add_child(component);
    tui.render_now();
    *rows.borrow_mut() = build_rows(140);
    resize(&terminal, &mut tui, 59, 9);
    tui.render_now();
    assert!(terminal.viewport().last().unwrap().contains("row-139"));
}

#[test]
fn stays_anchored_across_shrink_grow_cycles_while_overflowing_viewport() {
    let terminal = Shared::new(30, 6);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let (component, rows) = lines(&[]);
    *rows.borrow_mut() = build_rows(64);
    tui.add_child(component);
    tui.render_now();
    for cycle in 0..3 {
        *rows.borrow_mut() = build_rows(64 - cycle * 8);
        tui.request_render();
        tui.render_now();
        *rows.borrow_mut() = build_rows(64 - cycle * 8 + 4);
        tui.request_render();
        tui.render_now();
    }
    let viewport = terminal.viewport();
    assert!(viewport.iter().all(|line| line.trim().starts_with("row-")));
    let numbers: Vec<usize> = viewport
        .iter()
        .map(|line| line.trim().strip_prefix("row-").unwrap().parse().unwrap())
        .collect();
    assert_eq!(numbers.last(), Some(&51));
    assert!(*numbers.first().unwrap() >= 40);
}

#[test]
fn updates_hardware_cursor_without_redrawing_content() {
    let terminal = Shared::new(40, 6);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let col = Rc::new(Cell::new(0));
    tui.add_child(CursorText { col: Rc::clone(&col) });
    tui.render_now();
    let before = terminal.scrollback_len();
    for next in 1..=10 {
        col.set(next);
        tui.request_render();
        tui.render_now();
    }
    assert_eq!(terminal.viewport()[0].trim(), "cursor-anchor");
    assert_eq!(terminal.hardware_cursor(), Some((0, 10)));
    assert!(terminal.scrollback_len() - before < 2);
}

#[test]
fn limits_scrollback_growth_during_resize_oscillation_with_overflowing_content() {
    let terminal = Shared::new(60, 10);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let (component, rows) = lines(&[]);
    *rows.borrow_mut() = build_rows(160);
    tui.add_child(component);
    tui.render_now();
    let before = terminal.scrollback_len();
    for index in 0..18 {
        *rows.borrow_mut() = build_rows(140 + (index % 6) * 8);
        resize(
            &terminal,
            &mut tui,
            if index % 2 == 0 { 59 } else { 60 },
            if index % 3 == 0 { 11 } else { 10 },
        );
        tui.render_now();
        assert!(!viewport_row_numbers(&terminal).is_empty());
    }
    let scrollback = terminal.scrollback_rows();
    assert!(scrollback.len() - before < 220);
    assert!(longest_blank_run(&scrollback) < 30);
}

#[test]
fn limits_scrollback_while_toggling_overlays_over_overflowing_content() {
    let terminal = Shared::new(60, 10);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let (component, rows) = lines(&[]);
    *rows.borrow_mut() = build_rows(150);
    tui.add_child(component);
    tui.render_now();
    let before = terminal.scrollback_len();
    for index in 0..12 {
        let overlay_rows =
            vec![format!("overlay-{index}-0"), format!("overlay-{index}-1"), format!("overlay-{index}-2")];
        let overlay = tui.show_overlay(
            owned_lines(overlay_rows).0,
            OverlayOptions { anchor: OverlayAnchor::Center, ..Default::default() },
        );
        tui.render_now();
        overlay.hide();
        tui.render_now();
        if index % 4 == 0 {
            *rows.borrow_mut() = build_rows(140 + (index % 4) * 10);
            tui.request_render();
            tui.render_now();
        }
        assert!(!viewport_row_numbers(&terminal).is_empty());
    }
    let scrollback = terminal.scrollback_rows();
    assert!(scrollback.len() - before < 320);
    assert!(longest_blank_run(&scrollback) < 50);
}

#[test]
fn keeps_scrollback_bounded_under_rapid_micro_resize_oscillation() {
    let terminal = Shared::new(80, 12);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&build_rows(180).iter().map(String::as_str).collect::<Vec<_>>()).0);
    tui.render_now();
    let before = terminal.scrollback_len();
    for index in 0..24 {
        resize(
            &terminal,
            &mut tui,
            if index % 2 == 0 { 79 } else { 80 },
            if index % 3 == 0 { 11 } else { 12 },
        );
        tui.render_now();
        assert!(!viewport_row_numbers(&terminal).is_empty());
    }
    let scrollback = terminal.scrollback_rows();
    assert!(scrollback.len() - before < 320);
    assert!(longest_blank_run(&scrollback) < 60);
}

#[test]
fn avoids_scrollback_growth_on_repeated_no_op_renders_with_overflowing_content() {
    let terminal = Shared::new(70, 10);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    tui.add_child(lines(&build_rows(130).iter().map(String::as_str).collect::<Vec<_>>()).0);
    tui.render_now();
    let before = terminal.scrollback_len();
    for _ in 0..16 {
        tui.render_now();
    }
    assert!(terminal.scrollback_len() - before < 30);
}

#[test]
fn stays_stable_with_direct_row_delta_movement() {
    let terminal = Shared::new(50, 10);
    let mut tui = Tui::new(Box::new(terminal.clone()));
    let (component, rows) = lines(&[]);
    *rows.borrow_mut() = build_rows(150);
    tui.add_child(component);
    tui.render_now();
    let before = terminal.scrollback_len();
    for index in 0..18 {
        *rows.borrow_mut() = build_rows(120 + (index % 8) * 6);
        resize(
            &terminal,
            &mut tui,
            if index % 2 == 0 { 50 } else { 49 },
            if index % 3 == 0 { 11 } else { 10 },
        );
        tui.request_render();
        tui.render_now();
        assert!(!viewport_row_numbers(&terminal).is_empty());
    }
    let scrollback = terminal.scrollback_rows();
    assert!(scrollback.len() - before < 260);
    assert!(longest_blank_run(&scrollback) < 40);
}

#[test]
fn scrolls_the_virtual_viewport_through_retained_scrollback() {
    let mut terminal = VirtualTerminal::new(20, 3);
    terminal.append_scrollback(&(0..10).map(|index| format!("history-{index}")).collect::<Vec<_>>());
    terminal.set_viewport(&["live-0".into(), "live-1".into(), "live-2".into()]);

    let rows = |items: [&str; 3]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
    assert_eq!(terminal.viewport_rows(), rows(["live-0", "live-1", "live-2"]));
    terminal.scroll_by(2);
    assert_eq!(terminal.viewport_offset(), 2);
    assert_eq!(terminal.viewport_rows(), rows(["history-8", "history-9", "live-0"]));
    terminal.scroll_to(0);
    assert_eq!(terminal.viewport_rows(), rows(["live-0", "live-1", "live-2"]));
    terminal.clear_scrollback();
    assert!(terminal.scrollback().is_empty());
    assert_eq!(terminal.viewport_offset(), 0);
}
