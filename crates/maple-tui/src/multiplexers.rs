//! Terminal multiplexer/host detection (port of the multiplexer modules and
//! `terminal-capabilities` environment probing).

use std::env;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Multiplexer {
    Zellij,
    Tmux,
    Screen,
    Cmux,
    WezTerm,
    Kitty,
    ITerm2,
    AppleTerminal,
    WindowsTerminal,
    Warp,
    Ghostty,
}

impl Multiplexer {
    pub fn name(self) -> &'static str {
        match self {
            Multiplexer::Zellij => "zellij",
            Multiplexer::Tmux => "tmux",
            Multiplexer::Screen => "screen",
            Multiplexer::Cmux => "cmux",
            Multiplexer::WezTerm => "wezterm",
            Multiplexer::Kitty => "kitty",
            Multiplexer::ITerm2 => "iterm2",
            Multiplexer::AppleTerminal => "apple-terminal",
            Multiplexer::WindowsTerminal => "windows-terminal",
            Multiplexer::Warp => "warp",
            Multiplexer::Ghostty => "ghostty",
        }
    }

    /// Host owns the scrollback/grid on resize; the TUI must take the in-place
    /// path rather than borrowing the alternate screen.
    pub fn host_owns_grid_on_resize(self) -> bool {
        matches!(self, Multiplexer::Tmux | Multiplexer::Zellij | Multiplexer::Screen)
    }

    /// Honors DEC 2026 synchronized output passthrough.
    pub fn honors_synchronized_output(self) -> bool {
        !matches!(self, Multiplexer::Zellij)
    }
}

/// Detect the enclosing host from environment variables.
pub fn detect() -> Option<Multiplexer> {
    if env::var_os("ZELLIJ").is_some() || env::var_os("ZELLIJ_PANE_ID").is_some() {
        return Some(Multiplexer::Zellij);
    }
    if env::var_os("TMUX").is_some() {
        return Some(Multiplexer::Tmux);
    }
    if env::var_os("STY").is_some() {
        return Some(Multiplexer::Screen);
    }
    if env::var_os("CMUX_SURFACE_ID").is_some() {
        return Some(Multiplexer::Cmux);
    }
    if env::var_os("WEZTERM_PANE").is_some() {
        return Some(Multiplexer::WezTerm);
    }
    if env::var_os("KITTY_WINDOW_ID").is_some() {
        return Some(Multiplexer::Kitty);
    }
    if env::var_os("ITERM_SESSION_ID").is_some() {
        return Some(Multiplexer::ITerm2);
    }
    if env::var_os("TERM_SESSION_ID").is_some() {
        return Some(Multiplexer::AppleTerminal);
    }
    if env::var_os("WT_SESSION").is_some() {
        return Some(Multiplexer::WindowsTerminal);
    }
    if env::var_os("WARP_HIDE_STARTUP_TEXT").is_some() {
        return Some(Multiplexer::Warp);
    }
    if env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
        || env::var("TERM_PROGRAM").as_deref() == Ok("ghostty")
    {
        return Some(Multiplexer::Ghostty);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_owns_grid_and_zellij_rejects_sync() {
        assert!(Multiplexer::Tmux.host_owns_grid_on_resize());
        assert!(Multiplexer::Zellij.host_owns_grid_on_resize());
        assert!(!Multiplexer::Zellij.honors_synchronized_output());
        assert!(Multiplexer::Kitty.honors_synchronized_output());
    }
}
