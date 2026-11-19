use anyhow::Result;
use clap::Parser;
use maple::cli::{Cli, Command};
use maple::resource_guard::{collect_snapshot, preflight};
use maple::session::{SessionFile, SessionSummary, Store};

/// SOP-RESLIM.2 default per-process RSS budget (MB); build/test use 512.
const EXPECTED_PEAK_MB: u64 = 256;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?.to_string_lossy().into_owned();
    // SOP-RESLIM.3 pre-flight. `list` is a bounded scan and may skip it.
    if !matches!(cli.command, Some(Command::List)) {
        let snapshot = collect_snapshot();
        // The TUI/headless run is interactive and wrapped in the caller's
        // timeout contract; no per-command timeout exists here, so pass `true`
        // to avoid a self-abort and keep the remaining abort conditions live.
        preflight(&snapshot, EXPECTED_PEAK_MB, true)
            .map_err(|reason| anyhow::anyhow!(reason))?;
    }
    let store = Store::new(&cwd);

    match cli.command {
        Some(Command::List) => {
            let sessions = store.list()?;
            println!("{}", serde_json::to_string_pretty(&sessions)?);
        }
        Some(Command::Headless) => headless(&store, &cli)?,
        None => {
            let file = resolve(&store, &cli)?;
            maple::tui::run(store, file, cli.model)?;
        }
    }
    Ok(())
}

fn resolve(store: &Store, cli: &Cli) -> Result<SessionFile> {
    match cli.resume.as_deref() {
        Some(arg) => store.open(arg),
        None => store.create(cli.title.as_deref()),
    }
}

fn headless(store: &Store, cli: &Cli) -> Result<()> {
    let file = resolve(store, cli)?;
    let (header, entries) = store.load(&file.path)?;
    let summary = SessionSummary {
        session_id: header.id,
        file: file.path.to_string_lossy().into_owned(),
        cwd: header.cwd,
        created_at: header.timestamp,
        title: header.title,
        parent_session: header.parent_session,
        entries: entries.len(),
    };
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
