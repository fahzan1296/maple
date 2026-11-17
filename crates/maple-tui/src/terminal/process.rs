//! Real terminal backend over the process standard streams.
//! Reference: `l/src/terminal.ts`.

use super::program_status::{ProgramStatusAction, ProgramStatusProbe};
use super::{CursorVisibilityTracker, TtyEvent, Terminal};
use crate::stdin_buffer::{StdinBuffer, StdinBufferEvent};
use crate::terminal_capabilities::ProcessEnv;
use crate::keys::from_crossterm;
use crate::mouse::from_crossterm as mouse_from_crossterm;
use crossterm::cursor::{Hide, MoveDown, MoveTo, MoveUp, Show};
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
    started: bool,
    program_status: ProgramStatusProbe,
    stdin_buffer: StdinBuffer,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    cursor_tracker: CursorVisibilityTracker,
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
            program_status: ProgramStatusProbe::default(),
            stdin_buffer: StdinBuffer::default(),
            cursor_tracker: CursorVisibilityTracker::new(),
        }
    }

    /// Subscribe to OSC 7501 program-status verdicts across terminal restarts.
    pub fn on_program_status_support(&mut self, callback: Box<dyn Fn(bool)>) -> usize {
        self.program_status.subscribe(callback)
    }

    /// Drop a program-status subscription.
    pub fn unsubscribe_program_status_support(&mut self, id: usize) {
        self.program_status.unsubscribe(id)
    }

    /// The last program-status verdict, if any.
    pub fn program_status_support(&self) -> Option<bool> {
        self.program_status.result()
    }

    /// Drive the program-status probe directly: the raw-stdin seam that needs
    /// no PTY. `begin` returns the query action, `feed_raw_stdin` consumes
    /// bytes, `stop` invalidates support.
    pub fn program_status_probe_mut(&mut self) -> &mut ProgramStatusProbe {
        &mut self.program_status
    }

    /// Feed one raw stdin read through the stdin buffer and the program-status
    /// probe; returns the completed input events.
    pub fn feed_raw_stdin(&mut self, data: &str) -> Vec<StdinBufferEvent> {
        let events = self.stdin_buffer.process(data);
        let mut forwarded = Vec::with_capacity(events.len());
        for event in events {
            if let StdinBufferEvent::Data(sequence) = &event {
                if self.program_status.consumes_sequence(sequence) {
                    self.program_status.feed(sequence);
                    continue;
                }
            }
            forwarded.push(event);
        }
        forwarded
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
        for action in self.program_status.begin(&ProcessEnv) {
            match action {
                ProgramStatusAction::Write(data) => self.write(&data),
            }
        }
        self.spawn_worker();
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        if !self.started {
            return Ok(());
        }
        self.started = false;
        let _ = self.program_status.stop();
        self.stdin_buffer.clear();
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
        self.cursor_tracker.track_write(data);
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

    fn set_hardware_cursor(&mut self, position: Option<(u16, u16)>) {
        let mut out = io::stdout();
        match position {
            Some((row, column)) => {
                let _ = out.execute(MoveTo(column, row));
                let _ = out.execute(Show);
            }
            None => {
                let _ = out.execute(Hide);
            }
        }
        let _ = out.flush();
    }

    fn hide_cursor(&mut self, force: bool) {
        if !self.cursor_tracker.should_hide(force) {
            return;
        }
        self.cursor_tracker.record_hide();
        let mut out = io::stdout();
        let _ = out.execute(Hide);
        let _ = out.flush();
    }

    fn show_cursor(&mut self, force: bool) {
        if !self.cursor_tracker.should_show(force) {
            return;
        }
        self.cursor_tracker.record_show();
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
