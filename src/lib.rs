//! maple — RAM-efficient multi-session TUI coding agent.
//!
//! Clean-room implementation inspired by oh-my-pi (TUI/session/edit/memory)
//! and pi-messenger (flat cross-session mesh). See `PRD.md` and `docs/adr/`.

pub mod tui;
pub mod cli;
pub mod session;
pub mod resource_guard;
pub mod apps;
