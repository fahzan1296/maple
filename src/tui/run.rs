use anyhow::Result;
use maple_tui::screen::Screen;
use maple_tui::terminal::{TtyEvent, Terminal};
use maple_tui::{Key, ProcessTerminal};
use std::time::Duration;

use crate::session::store::{SessionFile, Store};

use super::app::App;
use super::render;

pub fn run(store: Store, file: SessionFile, model: String) -> Result<()> {
    let mut terminal = ProcessTerminal::new();
    terminal.start()?;
    let mut app = App::new(store, file, model);
    let mut screen = Screen::new();
    let result = event_loop(&mut terminal, &mut screen, &mut app);
    let _ = terminal.stop();
    result
}

fn event_loop(
    terminal: &mut ProcessTerminal,
    screen: &mut Screen,
    app: &mut App,
) -> Result<()> {
    while !app.should_quit {
        let width = terminal.columns().max(1);
        let height = terminal.rows().max(1);
        let rows = render::render_rows(app, width, height);
        screen.flush(terminal, &rows, width);

        match terminal.event(Duration::from_millis(16)) {
            Some(TtyEvent::Key(key)) => handle_key(app, key),
            Some(TtyEvent::Paste(text)) => app.handle_paste(&text),
            Some(TtyEvent::Disconnect) => break,
            Some(TtyEvent::Resize(_, _)) | None => {}
            Some(TtyEvent::Mouse(_)) => {}
        }
    }
    Ok(())
}

/// Keep the UI engine's Ctrl-D debug hook out of the app; everything else is
/// forwarded to the view's key handler.
fn handle_key(app: &mut App, key: Key) {
    if let Key::Ctrl('d') = key {
        return;
    }
    app.handle_key(key);
}
