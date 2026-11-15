use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "maple", version, about = "RAM-efficient multi-session TUI coding agent")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Resume a session by id or file stem.
    #[arg(long, global = true)]
    pub resume: Option<String>,

    /// Session title (stored in the header).
    #[arg(long, global = true)]
    pub title: Option<String>,

    /// Model name shown in the statusline.
    #[arg(long, global = true, default_value = "unknown")]
    pub model: String,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run without the TUI and print a JSON session summary.
    Headless,
    /// List sessions for the current directory.
    List,
}
