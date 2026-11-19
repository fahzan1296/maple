//! TTSR (Time Traveling Stream Rules) notification: a rule violation rewound
//! the stream.
//!
//! Reference: `l/src/chat/ttsr-notification.ts`.
//!
//! One block can carry several rules: a single event may match multiple rules,
//! and consecutive notifications merge into the previous block via
//! [`TtsrNotification::add_rules`] while it is still the live transcript tail.
//!
//! Deviations: the reference's `MessageNoticeComponent` closures and its
//! process-global `theme` become explicit `&Theme` arguments, and the notice
//! presentation is the plain [`NoticePresentation`] struct (icon, header, body
//! rows) that [`crate::chrome::MessageNotice`] consumes.

use crate::chrome::{MessageNotice, NoticeTheme};
use crate::component::Component;
use crate::native::describe::{node, span, text, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text};
use crate::render::render_utils::expand_key_hint;
use crate::style::{Color, Style};
use crate::theme::Theme;

/// Rule fields shown in rewind notifications.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationRule {
    /// Rule name, the dedup key.
    pub name: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// Rule body, used when the description is absent.
    pub content: Option<String>,
}

impl NotificationRule {
    /// `description` when non-blank, else `content`.
    pub fn detail(&self) -> Option<&str> {
        self.description
            .as_deref()
            .or(self.content.as_deref())
            .map(str::trim)
            .filter(|detail| !detail.is_empty())
    }
}

/// Collapsed view shows at most this many rules before eliding the rest.
pub const MAX_COLLAPSED_RULES: usize = 4;

/// The notice's presentation: icon name, header line, body rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticePresentation {
    /// Icon glyph drawn before the header.
    pub icon: String,
    /// The header line.
    pub header: String,
    /// Body rows, styled by the caller's theme where relevant.
    pub body: Vec<String>,
}

/// The native form of the notice: head spans and body nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct NoticeNativePresentation {
    /// Head spans.
    pub head: Vec<crate::native::node::Span>,
    /// Body nodes.
    pub body: Vec<NativeNode>,
}

/// A TTSR notification block.
pub struct TtsrNotification {
    rules: Vec<NotificationRule>,
    notice: MessageNotice,
    expanded: bool,
    hidden: bool,
    theme_style: NoticeTheme,
}

impl TtsrNotification {
    /// A notification for `rules`.
    pub fn new(rules: Vec<NotificationRule>, theme: &Theme) -> Self {
        let mut notification = TtsrNotification {
            rules,
            notice: MessageNotice::new(String::new()),
            expanded: false,
            hidden: false,
            theme_style: NoticeTheme {
                header: Style::fg(Color::Yellow),
                body: Style::default(),
                background: None,
            },
        };
        notification.notice.set_theme(notification.theme_style.clone());
        notification.refresh(theme);
        notification
    }

    /// Whether tool activity is visible; hiding it hides this block too.
    pub fn set_tool_activity_visible(&mut self, visible: bool) {
        self.hidden = !visible;
        self.notice.set_leading_space(visible);
    }

    /// A snapshot: [`Self::add_rules`] grows the block's own list in place.
    pub fn rules(&self) -> Vec<NotificationRule> {
        self.rules.clone()
    }

    /// Merge additional rules into this block (deduped by rule name).
    pub fn add_rules(&mut self, rules: &[NotificationRule], theme: &Theme) {
        let mut changed = false;
        for rule in rules {
            if self.rules.iter().any(|existing| existing.name == rule.name) {
                continue;
            }
            self.rules.push(rule.clone());
            changed = true;
        }
        if changed {
            self.refresh(theme);
        }
    }

    /// Expand or collapse the rule descriptions.
    pub fn set_expanded(&mut self, expanded: bool, theme: &Theme) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.refresh(theme);
    }

    /// Whether the descriptions are expanded.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Whether the block is hidden with tool activity.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// The current presentation.
    pub fn presentation(&self, theme: &Theme) -> NoticePresentation {
        if self.rules.len() == 1 {
            self.presentation_single(&self.rules[0], theme)
        } else {
            self.presentation_multi(theme)
        }
    }

    /// An inline warning notice, "Rule applied: no-unwrap", with every rule's
    /// full description as the disclosure below it.
    pub fn native_presentation(&self) -> NoticeNativePresentation {
        let single = if self.rules.len() == 1 {
            Some(&self.rules[0])
        } else {
            None
        };
        let head = match single {
            Some(rule) => vec![
                span("Rule applied: ", Some("warning")),
                span(&rule.name, Some("mono strong")),
            ],
            None => {
                let names = self
                    .rules
                    .iter()
                    .map(|rule| rule.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                vec![
                    span(
                        format!("{} rules applied: ", self.rules.len()),
                        Some("warning"),
                    ),
                    span(names, Some("mono strong")),
                ]
            }
        };
        let mut body = Vec::new();
        for rule in &self.rules {
            let desc = rule.detail();
            let mut body_props = Props::new();
            body_props.set_str("wrap", "word");
            body_props.set_str("key", &rule.name);
            if single.is_some() {
                if let Some(desc) = desc {
                    body.push(text(
                        Text::spans(vec![span(desc, Some("em"))]),
                        body_props,
                    ));
                }
                continue;
            }
            let content = match desc {
                Some(desc) => Text::spans(vec![
                    span(&rule.name, Some("strong")),
                    span(": ", None),
                    span(desc, Some("em")),
                ]),
                None => Text::spans(vec![span(&rule.name, Some("strong"))]),
            };
            body.push(text(content, body_props));
        }
        NoticeNativePresentation { head, body }
    }

    fn refresh(&mut self, theme: &Theme) {
        let presentation = self.presentation(theme);
        self.notice = MessageNotice::new(presentation.header);
        self.notice.set_theme(self.theme_style.clone());
        self.notice.set_icon(Some(presentation.icon));
        self.notice.set_leading_space(!self.hidden);
        self.notice.set_body(&presentation.body);
    }

    fn presentation_single(&self, rule: &NotificationRule, theme: &Theme) -> NoticePresentation {
        let header = format!(
            "Injecting rule: {}  {}",
            theme.bold(&rule.name),
            theme.icon().rewind
        );
        let icon = theme.icon().warning.to_string();
        let Some(desc) = rule.detail() else {
            return NoticePresentation {
                icon,
                header,
                body: Vec::new(),
            };
        };
        let mut display_text = desc.to_string();
        let mut truncated = false;
        if !self.expanded {
            let lines: Vec<&str> = desc.split('\n').collect();
            if lines.len() > 2 {
                display_text = format!("{}…", lines[..2].join("\n"));
                truncated = true;
            }
        }
        let mut body = vec![theme.italic(&display_text)];
        if truncated {
            body.push(theme.italic(&format!(" ({} to expand)", expand_key_hint())));
        }
        NoticePresentation {
            icon,
            header,
            body,
        }
    }

    fn presentation_multi(&self, theme: &Theme) -> NoticePresentation {
        let header = format!(
            "Injecting {} rules:  {}",
            self.rules.len(),
            theme.icon().rewind
        );
        let visible: &[NotificationRule] = if self.expanded {
            &self.rules
        } else {
            &self.rules[..self.rules.len().min(MAX_COLLAPSED_RULES)]
        };
        let mut body = Vec::new();
        let mut elided_detail = false;
        for rule in visible {
            let mut line = theme.bold(&rule.name);
            if let Some(desc) = rule.detail() {
                let mut display_text = desc.to_string();
                if !self.expanded {
                    if let Some(newline) = desc.find('\n') {
                        display_text = format!("{}…", desc[..newline].trim_end());
                        elided_detail = true;
                    }
                }
                line.push_str(": ");
                line.push_str(&theme.italic(&display_text));
            }
            body.push(line);
        }
        let hidden = self.rules.len() - visible.len();
        if hidden > 0 {
            body.push(
                theme.italic(&format!("… +{hidden} more ({} to expand)", expand_key_hint())),
            );
        } else if elided_detail {
            body.push(theme.italic(&format!(" ({} to expand)", expand_key_hint())));
        }
        NoticePresentation {
            icon: theme.icon().warning.to_string(),
            header,
            body,
        }
    }
}

impl Component for TtsrNotification {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 || self.hidden {
            return Vec::new();
        }
        self.notice.render(width)
    }

    fn invalidate(&mut self) {
        self.notice.invalidate();
    }
}

impl NativeDescribe for TtsrNotification {
    fn describe_native(&self, _cx: &DescribeContext) -> NativeNode {
        let presentation = self.native_presentation();
        let mut props = Props::new();
        props.set_str("gap", "sm");
        props.set_str("align", "start");
        props.set_str("role", "omp.notice.ttsr");
        props.set_str("tone", "warning");
        let head = text(Text::spans(presentation.head), Props::new());
        let mut children = vec![
            node(
                NodeKind::Icon,
                {
                    let mut icon = Props::new();
                    icon.set_str("name", "shield-alert");
                    icon
                },
                Vec::new(),
                Some("icon"),
            ),
            head,
        ];
        children.extend(presentation.body);
        node(NodeKind::Row, props, children, Some("ttsr"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        crate::theme::create_theme(
            crate::theme::builtin::builtin_theme_json("dark").expect("builtin theme"),
            crate::theme::CreateThemeOptions {
                mode: Some(crate::theme::color::ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .expect("theme resolves")
    }

    fn rule(name: &str, description: Option<&str>) -> NotificationRule {
        NotificationRule {
            name: name.to_string(),
            description: description.map(str::to_string),
            content: None,
        }
    }

    #[test]
    fn detail_prefers_the_description_over_the_content() {
        let described = NotificationRule {
            name: "r".into(),
            description: Some("  desc  ".into()),
            content: Some("body".into()),
        };
        assert_eq!(described.detail(), Some("desc"));
        let content_only = NotificationRule {
            name: "r".into(),
            description: Some("   ".into()),
            content: Some("body".into()),
        };
        assert_eq!(content_only.detail(), Some("body"));
        assert_eq!(rule("r", None).detail(), None);
    }

    #[test]
    fn a_single_rule_notice_names_the_rule_and_its_icon() {
        let theme = theme();
        let notification = TtsrNotification::new(vec![rule("no-unwrap", Some("Do not unwrap."))], &theme);
        let presentation = notification.presentation(&theme);
        assert_eq!(presentation.icon, theme.icon().warning);
        assert!(presentation.header.starts_with("Injecting rule: "));
        assert!(presentation.header.contains(&theme.bold("no-unwrap")));
        assert!(presentation.header.ends_with(theme.icon().rewind));
        assert_eq!(presentation.body, vec![theme.italic("Do not unwrap.")]);
    }

    #[test]
    fn a_long_single_description_folds_to_two_lines_when_collapsed() {
        let theme = theme();
        let notification = TtsrNotification::new(
            vec![rule("r", Some("one\ntwo\nthree"))],
            &theme,
        );
        let collapsed = notification.presentation(&theme);
        assert_eq!(collapsed.body[0], theme.italic("one\ntwo…"));
        assert!(collapsed.body[1].contains(&expand_key_hint()));

        let mut expanded = TtsrNotification::new(vec![rule("r", Some("one\ntwo\nthree"))], &theme);
        expanded.set_expanded(true, &theme);
        let presentation = expanded.presentation(&theme);
        assert_eq!(presentation.body, vec![theme.italic("one\ntwo\nthree")]);
    }

    #[test]
    fn multiple_rules_elide_past_the_collapsed_cap() {
        let theme = theme();
        let rules: Vec<NotificationRule> = (0..6)
            .map(|index| rule(&format!("r{index}"), Some("desc\nmore")))
            .collect();
        let notification = TtsrNotification::new(rules.clone(), &theme);
        let presentation = notification.presentation(&theme);
        assert_eq!(presentation.header, format!("Injecting 6 rules:  {}", theme.icon().rewind));
        assert_eq!(presentation.body.len(), MAX_COLLAPSED_RULES + 1);
        assert_eq!(
            presentation.body[MAX_COLLAPSED_RULES],
            theme.italic(&format!("… +2 more ({} to expand)", expand_key_hint()))
        );

        let mut expanded = TtsrNotification::new(rules, &theme);
        expanded.set_expanded(true, &theme);
        assert_eq!(expanded.presentation(&theme).body.len(), 6);
    }

    #[test]
    fn rules_merge_by_name_and_keep_order() {
        let theme = theme();
        let mut notification = TtsrNotification::new(vec![rule("a", Some("first"))], &theme);
        notification.add_rules(
            &[rule("a", Some("duplicate")), rule("b", Some("second"))],
            &theme,
        );
        let names: Vec<String> = notification.rules().into_iter().map(|rule| rule.name).collect();
        assert_eq!(names, ["a", "b"]);
        assert!(notification.presentation(&theme).header.starts_with("Injecting 2 rules:"));
    }

    #[test]
    fn hidden_notifications_render_no_rows() {
        let theme = theme();
        let mut notification = TtsrNotification::new(vec![rule("a", Some("x"))], &theme);
        assert!(!notification.render(60).is_empty());
        notification.set_tool_activity_visible(false);
        assert!(notification.is_hidden());
        assert!(notification.render(60).is_empty());
    }

    #[test]
    fn the_native_form_heads_every_rule_through_the_disclosure() {
        let theme = theme();
        let notification = TtsrNotification::new(vec![rule("no-unwrap", Some("Do not unwrap."))], &theme);
        let native = notification.native_presentation();
        assert_eq!(native.head.len(), 2);
        assert_eq!(native.head[0].t, "Rule applied: ");
        assert_eq!(native.head[1].t, "no-unwrap");
        assert_eq!(native.body.len(), 1);
        assert_eq!(native.body[0].plain_text(), "Do not unwrap.");
        assert_eq!(native.body[0].props.get_str("key"), Some("no-unwrap"));

        let described = notification.describe_native(&DescribeContext::new(80));
        assert_eq!(described.kind, NodeKind::Row);
        assert_eq!(described.props.role(), Some("omp.notice.ttsr"));
        assert_eq!(described.children[0].kind, NodeKind::Icon);
    }
}
