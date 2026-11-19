//! Late LSP diagnostics (arrived after edit/write returned) in the transcript.
//!
//! Reference: `l/src/chat/late-diagnostics-message.ts`.
//!
//! Native terminals append late diagnostics into the edit/write frame they
//! belong to ([`route_late_diagnostics`]); whatever no frame took draws in the
//! standalone [`LateDiagnosticsMessageComponent`], a severity-toned card that
//! follows the global tool-output expand toggle.
//!
//! Deviations: the crate has no `formatDiagnostics` port yet, so the card body
//! is one row per diagnostic message (collapsed to the first
//! [`COLLAPSED_DIAGNOSTICS`]) instead of the shared tool tree renderer;
//! `isNativeRendering()` is a parameter because native state lives in the
//! render host, and the theme is passed in rather than read globally.

use crate::components::disclosure::Disclosure;
use crate::component::Component;
use crate::native::describe::{card, span, text, with_hidden};
use crate::native::node::{NativeNode, Props, Text as NodeText};
use crate::theme::theme_class::Theme;

/// Diagnostics shown by the collapsed native card, matching the ANSI tree's
/// collapsed count.
pub const COLLAPSED_DIAGNOSTICS: usize = 5;

/// One file's worth of late LSP diagnostics, as carried on the transcript
/// message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LateDiagnosticsFile {
    /// File path.
    pub path: Option<String>,
    /// Short summary line.
    pub summary: Option<String>,
    /// Whether the file has errors (not only warnings).
    pub errored: bool,
    /// Pre-formatted diagnostic rows.
    pub messages: Vec<String>,
}

/// A diagnostic file with every field resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LateDiagnosticsEntry {
    /// File path.
    pub path: String,
    /// Short summary line.
    pub summary: String,
    /// Whether the file has errors.
    pub errored: bool,
    /// Pre-formatted diagnostic rows.
    pub messages: Vec<String>,
}

/// A tool frame (edit/write) that takes late diagnostics for its own paths.
pub trait LateDiagnosticsTarget {
    /// Attach `files`; returns whether any were taken.
    fn attach_late_diagnostics(&mut self, files: &[LateDiagnosticsEntry]) -> bool;
}

/// Native terminals append late diagnostics into the edit/write frame they
/// belong to: each file goes to the most recent target in `targets` that
/// accepts it. Returns the files no frame took (all of them when
/// `native_rendering` is false), for the standalone
/// [`LateDiagnosticsMessageComponent`].
pub fn route_late_diagnostics(
    targets: &mut [&mut dyn LateDiagnosticsTarget],
    files: &[LateDiagnosticsFile],
    native_rendering: bool,
) -> Vec<LateDiagnosticsFile> {
    if !native_rendering {
        return files.to_vec();
    }
    let mut unrouted = Vec::new();
    for file in files {
        let entry = file.path.as_ref().map(|path| LateDiagnosticsEntry {
            path: path.clone(),
            summary: file.summary.clone().unwrap_or_default(),
            errored: file.errored,
            messages: file.messages.clone(),
        });
        let routed = entry.is_some_and(|entry| {
            targets
                .iter_mut()
                .rev()
                .any(|target| target.attach_late_diagnostics(std::slice::from_ref(&entry)))
        });
        if !routed {
            unrouted.push(file.clone());
        }
    }
    unrouted
}

/// Aggregate payload of a file list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DiagnosticInput {
    errored: bool,
    summary: String,
    messages: Vec<String>,
}

/// The standalone late-diagnostics card.
pub struct LateDiagnosticsMessage {
    files: Vec<LateDiagnosticsFile>,
    expanded: bool,
    tool_activity_visible: bool,
    disclosure: Disclosure,
}

impl LateDiagnosticsMessage {
    /// Build the card for `files`.
    pub fn new(files: Vec<LateDiagnosticsFile>) -> Self {
        let mut component = LateDiagnosticsMessage {
            files,
            expanded: false,
            tool_activity_visible: true,
            disclosure: Disclosure::new("Late diagnostics"),
        };
        component.rebuild();
        component
    }

    /// The diagnostic files.
    pub fn files(&self) -> &[LateDiagnosticsFile] {
        &self.files
    }

    /// Whether the card is expanded.
    pub fn expanded(&self) -> bool {
        self.expanded
    }

    /// Set the expanded state.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
        self.disclosure.set_open(expanded);
    }

    /// Show or hide the card with the rest of the tool activity.
    pub fn set_tool_activity_visible(&mut self, visible: bool) {
        self.tool_activity_visible = visible;
    }

    /// Whether tool activity is visible.
    pub fn tool_activity_visible(&self) -> bool {
        self.tool_activity_visible
    }

    /// A severity-toned card (role `omp.diagnostics.late`) with one wrapped
    /// line per diagnostic, clamped by the terminal while collapsed.
    pub fn describe(&self, theme: &Theme) -> NativeNode {
        let input = self.diagnostic_input();
        let errored = input.as_ref().is_some_and(|input| input.errored);
        let icon = if errored {
            theme.symbol("status.error").to_string()
        } else {
            theme.symbol("status.warning").to_string()
        };
        let mut head = vec![
            span(format!("{icon} "), Some(if errored { "error" } else { "warning" })),
            span("Late diagnostics", Some("toolTitle")),
        ];
        if let Some(summary) = input.as_ref().map(|input| input.summary.as_str()).filter(|s| !s.is_empty()) {
            head.push(span(format!(" ({summary})"), Some("dim")));
        }
        let mut props = Props::new();
        props.set_str("role", "omp.diagnostics.late");
        props.set_str("tone", if errored { "error" } else { "warning" });
        props.set_head(NodeText::Spans(head));
        props.set_bool("collapsible", true);
        props.set_bool("collapsed", !self.expanded);
        props.set_i64("preview", COLLAPSED_DIAGNOSTICS as i64);
        let children: Vec<NativeNode> = input
            .map(|input| input.messages)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, message)| {
                let mut text_props = Props::new();
                text_props.set_str("wrap", "word");
                text_props.set_str("key", format!("d{index}"));
                text(
                    NodeText::Spans(vec![span(message, Some("mono"))]),
                    text_props,
                )
            })
            .collect();
        let diagnostics = card(props, children);
        with_hidden(diagnostics, !self.tool_activity_visible)
    }

    /// Preserve controlled state and rematerialize both diagnostic ranges.
    pub fn invalidate(&mut self) {
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let input = self
            .diagnostic_input()
            .unwrap_or_else(|| DiagnosticInput {
                errored: false,
                summary: String::new(),
                messages: Vec::new(),
            });
        self.disclosure.set_open(self.expanded);
        self.disclosure.set_lines(self.format(&input));
    }

    /// Aggregate file payloads; `None` when there is nothing to render.
    fn diagnostic_input(&self) -> Option<DiagnosticInput> {
        let mut messages = Vec::new();
        let mut summaries = Vec::new();
        let mut errored = false;
        for file in &self.files {
            messages.extend(file.messages.iter().cloned());
            if let Some(summary) = &file.summary {
                summaries.push(summary.clone());
            }
            if file.errored {
                errored = true;
            }
        }
        if messages.is_empty() {
            return None;
        }
        Some(DiagnosticInput {
            errored,
            summary: summaries.join(", "),
            messages,
        })
    }

    /// The rows for one branch: the first [`COLLAPSED_DIAGNOSTICS`] messages
    /// while collapsed, every message when expanded.
    fn format(&self, input: &DiagnosticInput) -> Vec<String> {
        let message_rows: Vec<String> = if self.expanded {
            input.messages.clone()
        } else {
            input.messages.iter().take(COLLAPSED_DIAGNOSTICS).cloned().collect()
        };
        let mut rows = vec![format!("Late diagnostics ({})", input.messages.len())];
        rows.extend(message_rows);
        if !self.expanded && input.messages.len() > COLLAPSED_DIAGNOSTICS {
            rows.push(format!(
                "+{} more",
                input.messages.len() - COLLAPSED_DIAGNOSTICS
            ));
        }
        rows
    }
}

impl Component for LateDiagnosticsMessage {
    fn render(&mut self, width: u16) -> Vec<String> {
        if !self.tool_activity_visible {
            return Vec::new();
        }
        self.disclosure.set_open(self.expanded);
        self.disclosure.render(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin::builtin_theme_json;
    use crate::theme::loader::{create_theme, CreateThemeOptions};
    use crate::theme::theme_class::ColorMode;

    fn theme() -> Theme {
        create_theme(
            builtin_theme_json("dark").unwrap(),
            CreateThemeOptions {
                mode: Some(ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn file(path: &str, errored: bool, messages: &[&str]) -> LateDiagnosticsFile {
        LateDiagnosticsFile {
            path: Some(path.into()),
            summary: Some(format!("{path}: 1 problem")),
            errored,
            messages: messages.iter().map(|message| message.to_string()).collect(),
        }
    }

    #[test]
    fn aggregates_messages_summaries_and_severity() {
        let component = LateDiagnosticsMessage::new(vec![
            file("a.rs", false, &["w1"]),
            file("b.rs", true, &["e1", "e2"]),
        ]);
        let input = component.diagnostic_input().unwrap();
        assert!(input.errored);
        assert_eq!(input.messages, vec!["w1".to_string(), "e1".into(), "e2".into()]);
        assert_eq!(input.summary, "a.rs: 1 problem, b.rs: 1 problem");
    }

    #[test]
    fn no_messages_means_no_input() {
        let component = LateDiagnosticsMessage::new(vec![LateDiagnosticsFile {
            path: Some("a.rs".into()),
            ..LateDiagnosticsFile::default()
        }]);
        assert!(component.diagnostic_input().is_none());
    }

    #[test]
    fn collapsed_rows_are_clamped_and_expanded_show_all() {
        let messages: Vec<String> = (0..(COLLAPSED_DIAGNOSTICS + 2))
            .map(|i| format!("m{i}"))
            .collect();
        let mut component = LateDiagnosticsMessage::new(vec![LateDiagnosticsFile {
            path: Some("a.rs".into()),
            summary: None,
            errored: false,
            messages: messages.clone(),
        }]);
        let input = component.diagnostic_input().unwrap();
        let collapsed = component.format(&input);
        assert!(collapsed.iter().any(|row| row.contains("+2 more")));
        assert!(!collapsed.iter().any(|row| row.contains("m6")));
        component.set_expanded(true);
        let expanded = component.format(&input);
        assert!(expanded.iter().any(|row| row.contains("m6")));
        assert!(!expanded.iter().any(|row| row.contains("more")));
    }


    #[test]
    fn hidden_tool_activity_renders_nothing() {
        let mut component = LateDiagnosticsMessage::new(vec![file("a.rs", false, &["w1"])]);
        component.set_tool_activity_visible(false);
        assert!(component.render(80).is_empty());
        let described = component.describe(&theme());
        assert_eq!(described.props.hidden(), Some(true));
        assert_eq!(described.props.get_str("role"), Some("omp.diagnostics.late"));
    }

    struct Frame {
        accepts: bool,
        seen: Vec<String>,
    }

    impl LateDiagnosticsTarget for Frame {
        fn attach_late_diagnostics(&mut self, files: &[LateDiagnosticsEntry]) -> bool {
            if !self.accepts {
                return false;
            }
            self.seen
                .extend(files.iter().map(|file| file.path.clone()));
            true
        }
    }

    #[test]
    fn routing_prefers_the_most_recent_frame() {
        let mut older = Frame {
            accepts: true,
            seen: Vec::new(),
        };
        let mut newer = Frame {
            accepts: true,
            seen: Vec::new(),
        };
        {
            let mut targets: Vec<&mut dyn LateDiagnosticsTarget> = vec![&mut older, &mut newer];
            let unrouted = route_late_diagnostics(
                &mut targets,
                &[file("a.rs", false, &["w1"])],
                true,
            );
            assert!(unrouted.is_empty());
        }
        assert!(older.seen.is_empty());
        assert_eq!(newer.seen, vec!["a.rs".to_string()]);
    }

    #[test]
    fn routing_without_native_rendering_returns_everything() {
        let mut frame = Frame {
            accepts: true,
            seen: Vec::new(),
        };
        let mut targets: Vec<&mut dyn LateDiagnosticsTarget> = vec![&mut frame];
        let files = [file("a.rs", false, &["w1"])];
        let unrouted = route_late_diagnostics(&mut targets, &files, false);
        assert_eq!(unrouted, files);
        assert!(frame.seen.is_empty());
    }

    #[test]
    fn pathless_files_are_unrouted() {
        let mut frame = Frame {
            accepts: true,
            seen: Vec::new(),
        };
        let mut targets: Vec<&mut dyn LateDiagnosticsTarget> = vec![&mut frame];
        let file = LateDiagnosticsFile {
            path: None,
            ..LateDiagnosticsFile::default()
        };
        let unrouted = route_late_diagnostics(&mut targets, &[file.clone()], true);
        assert_eq!(unrouted, vec![file]);
    }
}
