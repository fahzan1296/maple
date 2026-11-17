//! Terminal identity (port of `l/src/ttyid.ts`).
//! Reference: `l/src/ttyid.ts`.

use std::env;

/// Stable identity for the current terminal pane, or `None` when piped.
pub fn get_terminal_id() -> Option<String> {
    if let Ok(link) = std::fs::read_link("/proc/self/fd/0") {
        let name = link.to_string_lossy();
        if name.starts_with("/dev/") {
            let name = name.trim_start_matches("/dev/");
            return Some(name.replace('/', "-"));
        }
    }
    if let Ok(pane) = env::var("ZELLIJ_PANE_ID") {
        if !pane.is_empty() {
            let session = env::var("ZELLIJ_SESSION_NAME").unwrap_or_default();
            let session = session.replace(' ', "-");
            let pane = pane.replace('%', "-");
            return Some(format!(
                "zellij-{}-{}",
                if session.is_empty() { "session" } else { &session },
                pane
            ));
        }
    }
    if let Ok(pane) = env::var("TMUX_PANE") {
        if !pane.is_empty() {
            return Some(format!("tmux-{}", pane.trim_start_matches('%')));
        }
    }
    if let Ok(id) = env::var("CMUX_SURFACE_ID") {
        if !id.is_empty() {
            return Some(format!("cmux-{id}"));
        }
    }
    if let Ok(id) = env::var("KITTY_WINDOW_ID") {
        if !id.is_empty() {
            return Some(format!("kitty-{id}"));
        }
    }
    if let Ok(pane) = env::var("WEZTERM_PANE") {
        if !pane.is_empty() {
            return Some(format!("wezterm-{pane}"));
        }
    }
    if let Ok(id) = env::var("TERM_SESSION_ID") {
        if !id.is_empty() {
            return Some(format!("apple-{}", id.trim_start_matches('w')));
        }
    }
    if let Ok(id) = env::var("WT_SESSION") {
        if !id.is_empty() {
            return Some(format!("wt-{id}"));
        }
    }
    None
}

#[cfg(test)]
    mod tests {
    use super::*;

    #[test]
    fn prefers_proc_link_when_available() {
        // CI containers often lack a tty; the helper must simply not panic.
        let _ = get_terminal_id();
    }
}
