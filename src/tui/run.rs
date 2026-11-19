use anyhow::Result;
use maple_tui::screen::Screen;
use maple_tui::terminal::{TtyEvent, Terminal};
use maple_tui::{Key, ProcessTerminal};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::resource_guard::{over_limit, sample_rss_kb};
use crate::session::store::{new_entry_id, now_rfc3339, SessionFile, Store};
use crate::session::types::SessionEntry;

use super::app::App;
use super::render;

/// SOP-RESLIM.2 default per-process RSS budget (MB); abort at 80% (SOP-RESLIM.4).
const RSS_LIMIT_MB: u64 = 256;
/// Monitoring starts after this uptime (SOP-RESLIM.4: runs > 60 s).
const MONITOR_DELAY: Duration = Duration::from_secs(60);
/// Sampling interval (SOP-RESLIM.4: at least every 5 s).
const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

pub fn run(store: Store, file: SessionFile, model: String) -> Result<()> {
    let mut terminal = ProcessTerminal::new();
    terminal.start()?;
    let monitor = spawn_monitor(Store::with_home(crate::session::path::agent_home(), &file.header.cwd), file.path.clone());
    let mut app = App::new(store, file, model);
    let mut screen = Screen::new();
    let result = event_loop(&mut terminal, &mut screen, &mut app, &monitor);
    monitor.store(true, Ordering::SeqCst);
    let _ = terminal.stop();
    result
}

/// Background RSS sampler. Sleeps between samples (no busy loop); sets `quit`
/// once RSS reaches 80% of the limit and records the abort in the session.
fn spawn_monitor(store: Store, path: std::path::PathBuf) -> Arc<AtomicBool> {
    let quit = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&quit);
    std::thread::spawn(move || {
        std::thread::sleep(MONITOR_DELAY);
        while !flag.load(Ordering::SeqCst) {
            if let Some(rss_kb) = sample_rss_kb(std::process::id() as i32) {
                if over_limit(rss_kb, RSS_LIMIT_MB) {
                    record_abort(&store, &path, rss_kb);
                    flag.store(true, Ordering::SeqCst);
                    return;
                }
            }
            std::thread::sleep(SAMPLE_INTERVAL);
        }
    });
    quit
}

fn record_abort(store: &Store, path: &std::path::Path, rss_kb: u64) {
    // Best-effort: the guard must not fail the session write path.
    let entry = SessionEntry::new(
        "abort",
        new_entry_id(),
        None,
        now_rfc3339(),
        serde_json::json!({ "reason": "rss", "rssKb": rss_kb, "limitMb": RSS_LIMIT_MB }),
    );
    let _ = store.append(path, &entry);
}

fn event_loop(
    terminal: &mut ProcessTerminal,
    screen: &mut Screen,
    app: &mut App,
    monitor: &AtomicBool,
) -> Result<()> {
    while !app.should_quit {
        if monitor.load(Ordering::SeqCst) {
            app.should_quit = true;
            app.status = "resource guard: RSS over limit, aborting".to_string();
            break;
        }
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
