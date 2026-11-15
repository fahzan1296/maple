use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;

use crate::session::store::{SessionFile, Store};

use super::app::App;
use super::render;

pub fn run(store: Store, file: SessionFile, model: String) -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new(store, file, model);
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|f| render::draw(f, app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                app.handle_key(key);
            }
        }
    }
    Ok(())
}
