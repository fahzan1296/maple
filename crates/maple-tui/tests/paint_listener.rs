//! Reference: `l/test/paint-listener.test.ts`.
//!
//! Deviations:
//! - The crate has no render scheduler; each case renders explicitly with
//!   [`Tui::render_now`], which runs the same paint path.
//! - `request_render_with(true, ...)` does not replay native history by itself;
//!   the frame provider reports a history batch on the next frame.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use maple_tui::tui::{
    RenderRequestOptions, TerminalFramePlan, TerminalFrameProvider, Tui, TuiPaint, ViewportSize,
};
use maple_tui::{Terminal, TtyEvent};

struct TestTerminal {
    columns: u16,
    rows: u16,
}

impl TestTerminal {
    fn new(columns: u16, rows: u16) -> Self {
        Self { columns, rows }
    }
}

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

struct Provider {
    plan: Rc<RefCell<TerminalFramePlan>>,
    viewports: Rc<RefCell<Vec<ViewportSize>>>,
}

impl TerminalFrameProvider for Provider {
    fn render_frame(&mut self, viewport: ViewportSize) -> TerminalFramePlan {
        self.viewports.borrow_mut().push(viewport);
        self.plan.borrow().clone()
    }

    fn acknowledge_history(&mut self, id: u64) {
        let mut plan = self.plan.borrow_mut();
        if plan.history.as_ref().is_some_and(|batch| batch.id == id) {
            plan.history = None;
        }
    }
}

fn rows(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn reports_complete_diff_history_append_and_destructive_reset_paints() {
    let mut tui = Tui::new(Box::new(TestTerminal::new(20, 4)));
    let plan = Rc::new(RefCell::new(TerminalFramePlan {
        history: None,
        viewport: rows(&["initial", "status"]),
    }));
    let viewports = Rc::new(RefCell::new(Vec::new()));
    tui.set_frame_provider(Some(Box::new(Provider {
        plan: Rc::clone(&plan),
        viewports: Rc::clone(&viewports),
    })));
    let paints: Rc<RefCell<Vec<TuiPaint>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&paints);
    tui.set_on_paint(Box::new(move |paint| sink.borrow_mut().push(paint.clone())));
    tui.render_now();
    paints.borrow_mut().clear();
    viewports.borrow_mut().clear();

    plan.borrow_mut().viewport = rows(&["diff", "status changed"]);
    tui.request_render();
    tui.render_now();
    {
        let paints = paints.borrow();
        assert_eq!(paints.len(), 1);
        assert!(paints[0].history.is_empty());
        assert_eq!(paints[0].viewport, rows(&["diff", "status changed"]));
        assert!(!paints[0].reset);
        assert!(!paints[0].alt);
        assert_eq!((paints[0].columns, paints[0].rows), (20, 4));
    }
    assert_eq!(
        *viewports.borrow(),
        [ViewportSize {
            columns: 20,
            rows: 4
        }]
    );

    paints.borrow_mut().clear();
    *plan.borrow_mut() = TerminalFramePlan {
        history: Some(maple_tui::tui::HistoryBatch {
            id: 1,
            rows: rows(&["finished block"]),
            kind: maple_tui::tui::HistoryKind::Append,
        }),
        viewport: rows(&["next", "status"]),
    };
    tui.request_render_with(true, RenderRequestOptions::default());
    tui.render_now();
    {
        let paints = paints.borrow();
        assert_eq!(paints.len(), 1);
        assert_eq!(paints[0].history, rows(&["finished block"]));
        assert_eq!(paints[0].viewport, rows(&["next", "status"]));
        assert!(!paints[0].reset);
        assert!(!paints[0].alt);
        assert_eq!((paints[0].columns, paints[0].rows), (20, 4));
    }
    assert!(plan.borrow().history.is_none(), "history batch acknowledged");

    paints.borrow_mut().clear();
    plan.borrow_mut().viewport = rows(&["replacement", "status"]);
    tui.request_render_with(
        true,
        RenderRequestOptions {
            clear_scrollback: true,
        },
    );
    tui.render_now();
    let paints = paints.borrow();
    assert_eq!(paints.len(), 1);
    assert!(paints[0].history.is_empty());
    assert_eq!(paints[0].viewport, rows(&["replacement", "status"]));
    assert!(paints[0].reset);
    assert!(!paints[0].alt);
    assert_eq!((paints[0].columns, paints[0].rows), (20, 4));
}
