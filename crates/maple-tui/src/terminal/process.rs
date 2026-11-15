//! Real terminal backend over the process standard streams.

use super::{TtyEvent, Terminal};
use crate::keys::from_crossterm;
use crate::mouse::from_crossterm as mouse_from_crossterm;
use crossterm::cursor::{Hide, MoveDown, MoveUp, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    self, disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct ProcessTerminal {
    receiver: Receiver<TtyEvent>,
    sender: Sender<TtyEvent>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    started: bool,
}

impl Default for ProcessTerminal {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTerminal {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        ProcessTerminal {
            receiver,
            sender,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            started: false,
        }
    }

    fn spawn_worker(&mut self) {
        let sender = self.sender.clone();
        let stop = Arc::clone(&self.stop);
        self.worker = Some(thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match event::poll(Duration::from_millis(50)) {
                    Ok(true) => match event::read() {
                        Ok(Event::Key(key)) => {
                            let _ = sender.send(TtyEvent::Key(from_crossterm(key)));
                        }
                        Ok(Event::Mouse(mouse)) => {
                            let _ = sender.send(TtyEvent::Mouse(mouse_from_crossterm(mouse)));
                        }
                        Ok(Event::Paste(text)) => {
                            let _ = sender.send(TtyEvent::Paste(text));
                        }
                        Ok(Event::Resize(width, height)) => {
                            let _ = sender.send(TtyEvent::Resize(width, height));
                        }
                        Ok(_) => {}
                        Err(_) => {
                            let _ = sender.send(TtyEvent::Disconnect);
                            break;
                        }
                    },
                    Ok(false) => {}
                    Err(_) => {
                        let _ = sender.send(TtyEvent::Disconnect);
                        break;
                    }
                }
            }
        }));
    }
}

impl Terminal for ProcessTerminal {
    fn start(&mut self) -> io::Result<()> {
        if self.started {
            return Ok(());
        }
        enable_raw_mode()?;
        let mut out = io::stdout();
        out.execute(EnterAlternateScreen)?;
        out.execute(crossterm::event::EnableBracketedPaste)?;
        out.execute(EnableMouseCapture)?;
        let _ = out.execute(PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        ));
        out.flush()?;
        self.started = true;
        self.spawn_worker();
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        if !self.started {
            return Ok(());
        }
        self.started = false;
        self.stop.store(true, Ordering::Relaxed);
        let mut out = io::stdout();
        let _ = out.execute(PopKeyboardEnhancementFlags);
        let _ = out.execute(crossterm::event::DisableBracketedPaste);
        let _ = out.execute(DisableMouseCapture);
        let _ = out.execute(LeaveAlternateScreen);
        let _ = out.execute(Show);
        out.flush()?;
        disable_raw_mode()?;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Ok(())
    }

    fn write(&mut self, data: &str) {
        let mut out = io::stdout();
        let _ = out.write_all(data.as_bytes());
        let _ = out.flush();
    }

    fn event(&mut self, timeout: Duration) -> Option<TtyEvent> {
        self.receiver.recv_timeout(timeout).ok()
    }

    fn columns(&self) -> u16 {
        terminal::size().map(|(columns, _)| columns).unwrap_or(80)
    }

    fn rows(&self) -> u16 {
        terminal::size().map(|(_, rows)| rows).unwrap_or(24)
    }

    fn move_by(&mut self, lines: i32) {
        if lines == 0 {
            return;
        }
        let mut out = io::stdout();
        let result = if lines > 0 {
            out.execute(MoveDown(lines as u16))
        } else {
            out.execute(MoveUp((-lines) as u16))
        };
        let _ = result;
        let _ = out.flush();
    }

    fn hide_cursor(&mut self, _force: bool) {
        let mut out = io::stdout();
        let _ = out.execute(Hide);
        let _ = out.flush();
    }

    fn show_cursor(&mut self, _force: bool) {
        let mut out = io::stdout();
        let _ = out.execute(Show);
        let _ = out.flush();
    }

    fn clear_line(&mut self) {
        let mut out = io::stdout();
        let _ = out.execute(Clear(ClearType::CurrentLine));
        let _ = out.flush();
    }

    fn clear_from_cursor(&mut self) {
        let mut out = io::stdout();
        let _ = out.execute(Clear(ClearType::FromCursorDown));
        let _ = out.flush();
    }

    fn clear_screen(&mut self) {
        let mut out = io::stdout();
        let _ = out.execute(Clear(ClearType::All));
        let _ = out.flush();
    }
}
