//! Terminal multiplexer/host detection (port of the multiplexer modules and
//! `terminal-capabilities` environment probing).
//! Reference: `l/src/multiplexers/{zellij,screen,tmux,types,cmux,notify,wmux,herdr,orca}.ts`
//! and `l/src/terminal-multiplexer.ts`, `l/src/terminal-capabilities.ts`.

use std::env;
use std::process::{Command, Stdio};

use crate::terminal_capabilities::{wrap_tmux_passthrough, TerminalNotification};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Multiplexer {
    Herdr,
    Tmux,
    Screen,
    Zellij,
    Cmux,
    Wmux,
    Orca,
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
            Multiplexer::Herdr => "herdr",
            Multiplexer::Tmux => "tmux",
            Multiplexer::Screen => "screen",
            Multiplexer::Zellij => "zellij",
            Multiplexer::Cmux => "cmux",
            Multiplexer::Wmux => "wmux",
            Multiplexer::Orca => "orca",
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
        matches!(
            self,
            Multiplexer::Herdr
                | Multiplexer::Tmux
                | Multiplexer::Screen
                | Multiplexer::Zellij
                | Multiplexer::Wmux
        )
    }

    /// Honors DEC 2026 synchronized output passthrough.
    pub fn honors_synchronized_output(self) -> bool {
        !matches!(self, Multiplexer::Zellij)
    }
}

/// Detect the enclosing host from environment variables.
pub fn detect() -> Option<Multiplexer> {
    detect_with(&|key| {
        env::var_os(key).map(|value| value.to_string_lossy().into_owned())
    })
}

/// Classification shared by [`detect`] and the tests.
///
/// Mirrors `classifyTerminalMultiplexerModule` from
/// `l/src/terminal-multiplexer.ts`: `session`-precedence multiplexers in
/// registry order, then the terminal-host markers, then the `outerApp` tier.
/// `get` yields the raw value of one environment variable.
fn detect_with(get: &dyn Fn(&str) -> Option<String>) -> Option<Multiplexer> {
    let marked = |key: &str| get(key).is_some();
    let filled = |key: &str| get(key).is_some_and(|value| !value.trim().is_empty());
    // Session tier, in registry order.
    if get("HERDR_ENV").as_deref() == Some("1")
        || marked("HERDR_PANE_ID")
        || marked("HERDR_TAB_ID")
        || marked("HERDR_WORKSPACE_ID")
    {
        return Some(Multiplexer::Herdr);
    }
    if marked("TMUX") {
        return Some(Multiplexer::Tmux);
    }
    if marked("STY") {
        return Some(Multiplexer::Screen);
    }
    if marked("ZELLIJ") || marked("ZELLIJ_PANE_ID") {
        return Some(Multiplexer::Zellij);
    }
    if marked("CMUX_SURFACE_ID") {
        return Some(Multiplexer::Cmux);
    }
    if get("WMUX").as_deref() == Some("1") || marked("WMUX_SURFACE_ID") {
        return Some(Multiplexer::Wmux);
    }
    // Terminal hosts without a multiplexer registry entry.
    if marked("WEZTERM_PANE") {
        return Some(Multiplexer::WezTerm);
    }
    if marked("KITTY_WINDOW_ID") {
        return Some(Multiplexer::Kitty);
    }
    if marked("ITERM_SESSION_ID") {
        return Some(Multiplexer::ITerm2);
    }
    if marked("TERM_SESSION_ID") {
        return Some(Multiplexer::AppleTerminal);
    }
    if marked("WT_SESSION") {
        return Some(Multiplexer::WindowsTerminal);
    }
    if marked("WARP_HIDE_STARTUP_TEXT") {
        return Some(Multiplexer::Warp);
    }
    if marked("GHOSTTY_RESOURCES_DIR") || get("TERM_PROGRAM").as_deref() == Some("ghostty") {
        return Some(Multiplexer::Ghostty);
    }
    // `outerApp` tier: recognized for launches, ranked below nested sessions.
    if filled("ORCA_PANE_KEY") && filled("ORCA_WORKTREE_ID") {
        return Some(Multiplexer::Orca);
    }
    None
}

/// Out-of-band multiplexer notification delivery.
///
/// Ported from `l/src/multiplexers/notify.ts`. The crate keeps the
/// multiplexer modules in this flat file rather than a `multiplexers/`
/// directory, so the notifier helpers live here alongside detection.

/// Title for out-of-band multiplexer notifications that carry none of their own.
pub const DEFAULT_NOTIFICATION_TITLE: &str = "omp";

/// A notification's title and body after defaulting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationTitleAndBody {
    /// Display title; defaults to [`DEFAULT_NOTIFICATION_TITLE`].
    pub title: String,
    /// Display body; empty when the source carried none.
    pub body: String,
}

/// Input accepted by [`notification_title_and_body`]: raw text or a structured
/// [`TerminalNotification`], mirroring the reference's `string | TerminalNotification`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NotificationMessage<'a> {
    /// Plain message text.
    Text(&'a str),
    /// Structured notification.
    Notification(&'a TerminalNotification),
}

impl<'a> From<&'a str> for NotificationMessage<'a> {
    fn from(value: &'a str) -> Self {
        NotificationMessage::Text(value)
    }
}

impl<'a> From<&'a String> for NotificationMessage<'a> {
    fn from(value: &'a String) -> Self {
        NotificationMessage::Text(value.as_str())
    }
}

impl<'a> From<&'a TerminalNotification> for NotificationMessage<'a> {
    fn from(value: &'a TerminalNotification) -> Self {
        NotificationMessage::Notification(value)
    }
}

/// Resolve a notification's title and body, applying the default title.
pub fn notification_title_and_body(message: NotificationMessage<'_>) -> NotificationTitleAndBody {
    match message {
        NotificationMessage::Text(text) => NotificationTitleAndBody {
            title: DEFAULT_NOTIFICATION_TITLE.to_string(),
            body: text.to_string(),
        },
        NotificationMessage::Notification(notification) => NotificationTitleAndBody {
            title: notification
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .unwrap_or(DEFAULT_NOTIFICATION_TITLE)
                .to_string(),
            body: notification.body.clone().unwrap_or_default(),
        },
    }
}

/// Start a notifier CLI without waiting for or pinning it.
///
/// Returns `false` when the binary cannot be spawned, which leaves delivery to
/// the terminal fallback. The child is detached (`stdin`/`stdout`/`stderr`
/// discarded); dropping the handle does not terminate it.
pub fn spawn_notifier(cmd: &[String]) -> bool {
    if cmd.is_empty() {
        return false;
    }
    Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// A query wrapped so it reaches the outer terminal and refreshes the
/// multiplexer's cached background-color reply.
#[derive(Clone, Copy, Debug)]
pub struct BackgroundColorCache {
    /// Wraps one OSC 11 query for the outer terminal.
    pub passthrough: fn(&str) -> String,
    /// How long the multiplexer needs to store the outer reply.
    pub settle_ms: u32,
}



impl Multiplexer {
    /// Tracks DEC 2004 per pane, so bracketed paste is reasserted without a
    /// DECRPM confirmation.
    pub fn gates_bracketed_paste(self) -> bool {
        matches!(self, Multiplexer::Tmux)
    }

    /// Expands REP into styled cells, so replays REP-encode their padding.
    pub fn expands_rep_padding(self) -> bool {
        matches!(self, Multiplexer::Tmux)
    }

    /// Expires a synchronized update after a deadline.
    pub fn expires_synchronized_output(self) -> bool {
        matches!(self, Multiplexer::Tmux)
    }

    /// Restoring the alternate buffer ends a synchronized update early.
    pub fn alt_restore_ends_synchronized_output(self) -> bool {
        matches!(self, Multiplexer::Tmux)
    }

    /// May grow its grid before SIGWINCH reaches the application.
    pub fn grows_before_sigwinch(self) -> bool {
        matches!(self, Multiplexer::Tmux)
    }

    /// OSC 11 replies come from a cache an explicit refresh must update first.
    pub fn background_color_cache(self) -> Option<BackgroundColorCache> {
        match self {
            Multiplexer::Tmux => Some(BackgroundColorCache {
                passthrough: wrap_tmux_passthrough,
                settle_ms: 100,
            }),
            _ => None,
        }
    }

    /// Delivery tier tried for this host, innermost first.
    ///
    /// `pane` addresses the exact pane through a CLI, `surface` the containing
    /// app surface, and `inBand` rewrites the terminal notification sequence.
    pub fn notification_tier(self) -> Option<NotificationDeliveryTier> {
        match self {
            Multiplexer::Herdr => Some(NotificationDeliveryTier::Pane),
            Multiplexer::Cmux => Some(NotificationDeliveryTier::Surface),
            Multiplexer::Tmux | Multiplexer::Zellij => {
                Some(NotificationDeliveryTier::InBand)
            }
            _ => None,
        }
    }

    /// Deliver through this host's notifier. `false` leaves the notification
    /// to the terminal fallback.
    pub fn send_notification(self, request: &mut NotificationRequest<'_>) -> bool {
        match self {
            Multiplexer::Tmux => send_in_band(request, true),
            Multiplexer::Zellij => send_in_band(request, false),
            Multiplexer::Cmux => send_cmux(request),
            Multiplexer::Herdr => send_herdr(request),
            _ => false,
        }
    }
}

/// Notification delivery tier, tried innermost first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationDeliveryTier {
    /// Exact pane through the multiplexer's CLI.
    Pane,
    /// Containing application surface.
    Surface,
    /// Rewritten terminal notification sequence.
    InBand,
}

/// One notification offered to a host notifier.
pub struct NotificationRequest<'a> {
    /// Message or structured notification.
    pub message: NotificationMessage<'a>,
    /// Environment lookup, normally `std::env::var`.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// Formatted OSC notification, or `None` when the terminal uses a bare BEL.
    pub sequence: Option<&'a str>,
    /// Write a sequence through the active terminal.
    pub write: &'a mut dyn FnMut(&str),
}

impl<'a> NotificationRequest<'a> {
    pub fn new(
        message: NotificationMessage<'a>,
        env: &'a dyn Fn(&str) -> Option<String>,
        sequence: Option<&'a str>,
        write: &'a mut dyn FnMut(&str),
    ) -> Self {
        Self {
            message,
            env,
            sequence,
            write,
        }
    }
}

/// tmux swallows bare OSCs and Zellij has no DCS passthrough; both accept the
/// notification sequence followed by a BEL, with tmux wrapped.
fn send_in_band(request: &mut NotificationRequest<'_>, passthrough: bool) -> bool {
    let Some(sequence) = request.sequence else {
        return false;
    };
    let sequence = if passthrough {
        wrap_tmux_passthrough(sequence)
    } else {
        sequence.to_string()
    };
    (request.write)(&format!("{sequence}\x07"));
    true
}

fn is_cmux_surface_id(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    if groups.len() != 5
        || groups
            .iter()
            .map(|group| group.len())
            .ne([8, 4, 4, 4, 12].into_iter())
    {
        return false;
    }
    groups
        .iter()
        .all(|group| group.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn send_cmux(request: &NotificationRequest<'_>) -> bool {
    let surface_id = (request.env)("CMUX_SURFACE_ID")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if !is_cmux_surface_id(&surface_id) {
        return false;
    }
    let parsed = notification_title_and_body(request.message);
    spawn_notifier(&[
        "cmux".to_string(),
        "notify".to_string(),
        "--surface".to_string(),
        surface_id,
        "--title".to_string(),
        parsed.title,
        "--body".to_string(),
        parsed.body,
    ])
}

fn is_herdr_pane_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-'))
}

fn herdr_sound(message: NotificationMessage<'_>) -> &'static str {
    let NotificationMessage::Notification(notification) = message else {
        return "none";
    };
    let Some(kinds) = notification.notification_type.as_deref() else {
        return "none";
    };
    if kinds.iter().any(|kind| kind == "ask" || kind == "error") {
        "request"
    } else if kinds.iter().any(|kind| kind == "completion") {
        "done"
    } else {
        "none"
    }
}

fn send_herdr(request: &NotificationRequest<'_>) -> bool {
    let pane_id = (request.env)("HERDR_PANE_ID")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if !is_herdr_pane_id(&pane_id) {
        return false;
    }
    let parsed = notification_title_and_body(request.message);
    // Herdr reads exactly these positionals as a help request, with no `--`.
    let title = if matches!(parsed.title.as_str(), "help" | "--help" | "-h") {
        DEFAULT_NOTIFICATION_TITLE.to_string()
    } else {
        parsed.title
    };
    spawn_notifier(&[
        "herdr".to_string(),
        "notification".to_string(),
        "show".to_string(),
        title,
        "--body".to_string(),
        parsed.body,
        "--sound".to_string(),
        herdr_sound(request.message).to_string(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(values: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn tmux_owns_grid_and_zellij_rejects_sync() {
        assert!(Multiplexer::Tmux.host_owns_grid_on_resize());
        assert!(Multiplexer::Zellij.host_owns_grid_on_resize());
        assert!(!Multiplexer::Zellij.honors_synchronized_output());
        assert!(Multiplexer::Kitty.honors_synchronized_output());
    }

    #[test]
    fn new_multiplexer_names() {
        let cases = [
            (Multiplexer::Herdr, "herdr"),
            (Multiplexer::Wmux, "wmux"),
            (Multiplexer::Orca, "orca"),
        ];
        for (multiplexer, name) in cases {
            assert_eq!(multiplexer.name(), name);
        }
    }

    #[test]
    fn session_tier_precedence_and_markers() {
        let cases: &[(&[(&str, &str)], Option<Multiplexer>)] = &[
            (&[("HERDR_ENV", "1")], Some(Multiplexer::Herdr)),
            (&[("HERDR_PANE_ID", "3")], Some(Multiplexer::Herdr)),
            (&[("HERDR_TAB_ID", "t1")], Some(Multiplexer::Herdr)),
            (&[("HERDR_WORKSPACE_ID", "w1")], Some(Multiplexer::Herdr)),
            (&[("HERDR_ENV", "0"), ("TMUX", "/tmp/tmux-0")], Some(Multiplexer::Tmux)),
            (&[("TMUX", "/tmp/tmux-0")], Some(Multiplexer::Tmux)),
            (&[("STY", "1234.pts-0")], Some(Multiplexer::Screen)),
            (&[("ZELLIJ", "1")], Some(Multiplexer::Zellij)),
            (&[("CMUX_SURFACE_ID", "uuid")], Some(Multiplexer::Cmux)),
            (&[("WMUX", "1")], Some(Multiplexer::Wmux)),
            (&[("WMUX_SURFACE_ID", "s")], Some(Multiplexer::Wmux)),
            (&[("WMUX", "0"), ("WEZTERM_PANE", "2")], Some(Multiplexer::WezTerm)),
            (&[("TMUX", "x"), ("ZELLIJ", "1")], Some(Multiplexer::Tmux)),
        ];
        for (values, expected) in cases {
            assert_eq!(detect_with(&lookup(values)), *expected);
        }
    }

    #[test]
    fn orca_requires_both_paired_ids() {
        let both = lookup(&[("ORCA_PANE_KEY", "p"), ("ORCA_WORKTREE_ID", "w")]);
        assert_eq!(detect_with(&both), Some(Multiplexer::Orca));
        let blank = lookup(&[("ORCA_PANE_KEY", "  "), ("ORCA_WORKTREE_ID", "w")]);
        assert_eq!(detect_with(&blank), None);
        let one = lookup(&[("ORCA_PANE_KEY", "p")]);
        assert_eq!(detect_with(&one), None);
        // Orca ranks below a nested session marker.
        let nested = lookup(&[("TMUX", "x"), ("ORCA_PANE_KEY", "p"), ("ORCA_WORKTREE_ID", "w")]);
        assert_eq!(detect_with(&nested), Some(Multiplexer::Tmux));
    }

    #[test]
    fn owns_grid_matches_reference_modules() {
        assert!(Multiplexer::Herdr.host_owns_grid_on_resize());
        assert!(Multiplexer::Wmux.host_owns_grid_on_resize());
        assert!(!Multiplexer::Orca.host_owns_grid_on_resize());
        assert!(Multiplexer::Herdr.honors_synchronized_output());
    }

    #[test]
    fn notification_title_defaults_for_plain_text() {
        assert_eq!(
            notification_title_and_body(NotificationMessage::Text("done")),
            NotificationTitleAndBody { title: "omp".to_string(), body: "done".to_string() }
        );
    }

    #[test]
    fn notification_title_trims_and_falls_back() {
        let notification = TerminalNotification {
            title: Some("  ".to_string()),
            body: None,
            ..Default::default()
        };
        assert_eq!(
            notification_title_and_body(NotificationMessage::from(&notification)),
            NotificationTitleAndBody { title: "omp".to_string(), body: String::new() }
        );
        let notification = TerminalNotification {
            title: Some(" Build finished ".to_string()),
            body: Some("ok".to_string()),
            ..Default::default()
        };
        assert_eq!(
            notification_title_and_body((&notification).into()),
            NotificationTitleAndBody { title: "Build finished".to_string(), body: "ok".to_string() }
        );
    }

    #[test]
    fn spawn_notifier_reports_spawn_success_and_failure() {
        assert!(spawn_notifier(&["/bin/true".to_string()]));
        assert!(!spawn_notifier(&[]));
        assert!(!spawn_notifier(&["/definitely/not/a/real/binary".to_string()]));
    }
}

#[cfg(test)]
mod host_capability_tests {
    use super::*;

    fn no_env(_key: &str) -> Option<String> {
        None
    }

    #[test]
    fn tmux_is_the_only_host_with_grid_flags() {
        assert!(Multiplexer::Tmux.gates_bracketed_paste());
        assert!(Multiplexer::Tmux.expands_rep_padding());
        assert!(Multiplexer::Tmux.expires_synchronized_output());
        assert!(Multiplexer::Tmux.alt_restore_ends_synchronized_output());
        assert!(Multiplexer::Tmux.grows_before_sigwinch());
        assert!(!Multiplexer::Zellij.gates_bracketed_paste());
        assert!(!Multiplexer::Herdr.expands_rep_padding());
        assert!(!Multiplexer::Cmux.expires_synchronized_output());
        assert!(!Multiplexer::Screen.alt_restore_ends_synchronized_output());
        assert!(!Multiplexer::Wmux.grows_before_sigwinch());

        let cache = Multiplexer::Tmux.background_color_cache().unwrap();
        assert_eq!(cache.settle_ms, 100);
        assert_eq!((cache.passthrough)("q"), "\x1bPtmux;q\x1b\\");
        assert!(Multiplexer::Zellij.background_color_cache().is_none());
    }

    #[test]
    fn notification_tiers_match_host_notifiers() {
        assert_eq!(
            Multiplexer::Herdr.notification_tier(),
            Some(NotificationDeliveryTier::Pane)
        );
        assert_eq!(
            Multiplexer::Cmux.notification_tier(),
            Some(NotificationDeliveryTier::Surface)
        );
        assert_eq!(
            Multiplexer::Tmux.notification_tier(),
            Some(NotificationDeliveryTier::InBand)
        );
        assert_eq!(
            Multiplexer::Zellij.notification_tier(),
            Some(NotificationDeliveryTier::InBand)
        );
        assert_eq!(Multiplexer::Screen.notification_tier(), None);
        assert_eq!(Multiplexer::Wmux.notification_tier(), None);
        assert_eq!(Multiplexer::Orca.notification_tier(), None);
    }

    #[test]
    fn tmux_and_zellij_deliver_in_band_sequences_with_bel() {
        let sequence = "\x1b]9;done\x07";
        let mut written = String::new();
        {
            let mut write = |data: &str| written.push_str(data);
            let mut request = NotificationRequest::new(
                NotificationMessage::Text("done"),
                &no_env,
                Some(sequence),
                &mut write,
            );
            assert!(Multiplexer::Tmux.send_notification(&mut request));
        }
        assert_eq!(
            written,
            format!("{}\x07", wrap_tmux_passthrough(sequence))
        );

        let mut written = String::new();
        {
            let mut write = |data: &str| written.push_str(data);
            let mut request = NotificationRequest::new(
                NotificationMessage::Text("done"),
                &no_env,
                Some(sequence),
                &mut write,
            );
            assert!(Multiplexer::Zellij.send_notification(&mut request));
        }
        assert_eq!(written, format!("{sequence}\x07"));
    }

    #[test]
    fn invalid_pane_or_surface_ids_fall_through() {
        let env = |key: &str| {
            (key == "CMUX_SURFACE_ID")
                .then(|| "not-a-surface".to_string())
                .or_else(|| (key == "HERDR_PANE_ID").then(|| "bad pane!".to_string()))
        };
        let mut written = String::new();
        let mut write = |data: &str| written.push_str(data);
        let mut request = NotificationRequest::new(
            NotificationMessage::Text("done"),
            &env,
            Some("osc"),
            &mut write,
        );
        assert!(!Multiplexer::Cmux.send_notification(&mut request));
        assert!(!Multiplexer::Herdr.send_notification(&mut request));
        assert!(!Multiplexer::Screen.send_notification(&mut request));
        assert!(written.is_empty());
    }

    #[test]
    fn cmux_and_herdr_ids_follow_the_reference_shapes() {
        assert!(is_cmux_surface_id("123e4567-e89b-12d3-a456-426614174000"));
        assert!(is_cmux_surface_id("123E4567-E89B-12D3-A456-426614174000"));
        assert!(!is_cmux_surface_id("123e4567e89b12d3a456426614174000"));
        assert!(!is_cmux_surface_id("123e4567-e89b-12d3-a456-42661417400g"));

        assert!(is_herdr_pane_id("pane:1_A-2"));
        assert!(!is_herdr_pane_id(""));
        assert!(!is_herdr_pane_id("pane with space"));
        assert!(!is_herdr_pane_id(&"p".repeat(65)));
    }
}
