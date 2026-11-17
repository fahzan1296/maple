//! Transcript row for a user-invoked skill.
//!
//! Reference: `l/src/chat/skill-message.ts`.
//!
//! Two layouts, chosen by where the `/skill:<name>` token sat in the submitted
//! draft:
//!
//! - **Callout** (token first): a user bubble with a skill-colored left rail,
//!   the skill chip and prompt size as its header, then the rest of the draft as
//!   full Markdown.
//! - **Inline** (token mid-prompt): a plain user bubble with the token drawn as
//!   the same soft-pill chip the composer showed.
//!
//! In both, the chip is an OSC 8 link to the SKILL.md. Expanding (tool-output
//! toggle) appends the rendered skill prompt.
//!
//! Deviations: the theme is constructor-injected; this crate's [`Disclosure`]
//! takes a title string rather than a summary component, so the inline layout
//! uses the chip header as its title; the expanded prompt is rebuilt on toggle
//! instead of staying lazy; and the callout's Markdown body uses the crate's
//! [`Markdown`] with the default markdown theme (the user-bubble color callback
//! lives in the user-message component).

use std::sync::Arc;

use crate::chat::messages::{CustomMessage, CustomMessageContent, MessageBlock};
use crate::component::Component;
use crate::components::r#box::{Box as BoxComponent, BoxStyler};
use crate::components::container::Container;
use crate::components::disclosure::Disclosure;
use crate::components::markdown::{Markdown, MarkdownTheme};
use crate::components::spacer::Spacer;
use crate::components::text::Text;
use crate::native::describe::{card, md, node, span};
use crate::native::node::{NativeNode, NodeKind, Props, Text as NodeText};
use crate::prompt::composer_attachments::{
    collapse_skill_tokens, skill_chip_label, skill_chip_style, skill_token,
};
use crate::render::hyperlink::{file_hyperlink, file_link_span, FilePosition};
use crate::theme::theme_class::Theme;

/// `details` of a skill prompt message, as this renderer reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SkillDetails {
    name: String,
    path: Option<String>,
    args: Option<String>,
    prompt: Option<String>,
    line_count: Option<i64>,
}

impl SkillDetails {
    fn from_message(message: &CustomMessage) -> SkillDetails {
        let Some(details) = message.details.as_ref().and_then(|details| details.as_object()) else {
            return SkillDetails::default();
        };
        let string = |key: &str| {
            details
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        SkillDetails {
            name: string("name").unwrap_or_default(),
            path: string("path").filter(|path| !path.is_empty()),
            args: string("args"),
            prompt: string("prompt"),
            line_count: details.get("lineCount").and_then(|value| value.as_i64()),
        }
    }
}

/// The transcript row for a user-invoked skill.
pub struct SkillMessageComponent {
    message: CustomMessage,
    image_links: Option<Vec<Option<String>>>,
    expanded: bool,
    transcript_block_finalized: bool,
    theme: Theme,
    container: Container,
    native: Option<NativeNode>,
}

impl SkillMessageComponent {
    /// Build the row for a `/skill:` message.
    pub fn new(
        message: CustomMessage,
        image_links: Option<Vec<Option<String>>>,
        theme: &Theme,
    ) -> Self {
        let mut component = SkillMessageComponent {
            message,
            image_links,
            expanded: false,
            transcript_block_finalized: true,
            theme: theme.clone(),
            container: Container::new(),
            native: None,
        };
        component.rebuild();
        component
    }

    /// Expand or collapse the skill prompt.
    pub fn set_expanded(&mut self, expanded: bool) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.native = None;
        self.rebuild();
    }

    /// Whether the skill prompt is expanded.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Attached-image links carried from the composer.
    pub fn image_links(&self) -> Option<&[Option<String>]> {
        self.image_links.as_deref()
    }

    /// Apply a native `toggle` event for the `prompt` slot.
    pub fn handle_native_toggle(&mut self, key: &str, collapsed: bool) {
        if key == "prompt" {
            self.set_expanded(!collapsed);
        }
    }

    /// Whether the block already retired to scrollback.
    pub fn is_transcript_block_finalized(&self) -> bool {
        self.transcript_block_finalized
    }

    /// Hold this card live as an unreconciled optimistic `/skill:` row.
    pub fn mark_transcript_block_pending(&mut self) {
        self.transcript_block_finalized = false;
    }

    /// Finalize an optimistic row adopted in place.
    pub fn mark_transcript_block_finalized(&mut self) {
        self.transcript_block_finalized = true;
    }

    /// A user-toned card (role `omp.skill`): the skill chip and prompt size in
    /// the head, the user's draft always visible, and only the loaded skill
    /// prompt behind a disclosure.
    pub fn describe(&mut self) -> NativeNode {
        if let Some(native) = &self.native {
            return native.clone();
        }
        let details = SkillDetails::from_message(&self.message);
        let (_, label, display) = self.chip_parts(&details);
        let leading = leading_draft(&display, &label);
        let draft = if leading {
            display[label.len()..].trim().to_string()
        } else {
            display.clone()
        };

        let mut head = vec![match &details.path {
            Some(path) => file_link_span(path, &label, "customMessageLabel strong"),
            None => span(&label, Some("customMessageLabel strong")),
        }];
        if let Some(line_count) = details.line_count {
            head.push(span(
                format!("  {line_count} {}", line_unit(line_count)),
                Some("muted"),
            ));
        }
        let mut children = Vec::new();
        if !draft.is_empty() {
            children.push(md(draft, Props::new()));
        }
        let prompt_text = if self.expanded {
            message_text(&self.message)
        } else {
            String::new()
        };
        let mut section_props = Props::new();
        section_props.set_head(NodeText::Spans(vec![span("prompt", Some("muted"))]));
        section_props.set_str("role", "omp.skill.prompt");
        section_props.set_bool("collapsible", true);
        section_props.set_bool("collapsed", !self.expanded);
        let section_children = if prompt_text.is_empty() {
            Vec::new()
        } else {
            vec![md(prompt_text, Props::new())]
        };
        children.push(node(
            NodeKind::Section,
            section_props,
            section_children,
            Some("prompt"),
        ));
        let mut props = Props::new();
        props.set_str("role", "omp.skill");
        props.set_str("tone", "user");
        props.set_head(NodeText::Spans(head));
        let described = card(props, children);
        self.native = Some(described.clone());
        described
    }

    /// Drop the memoized description and rebuild the rows.
    pub fn invalidate(&mut self) {
        self.native = None;
        self.rebuild();
    }

    fn chip_parts(&self, details: &SkillDetails) -> (String, String, String) {
        let name = if details.name.trim().is_empty() {
            "unknown".to_string()
        } else {
            details.name.trim().to_string()
        };
        let token = skill_token(&name);
        let prompt = details
            .prompt
            .clone()
            .unwrap_or_else(|| match &details.args {
                Some(args) => format!("{token} {args}"),
                None => token,
            });
        // Display-only collapse: only the invoked skill becomes a chip; a second
        // `/skill:` token the dispatcher ignored stays literal so the transcript
        // never claims a skill that never loaded.
        let display = collapse_skill_tokens(&prompt, &|candidate| candidate == name, &mut |_, _| {});
        let label = skill_chip_label(&name, Some(&self.theme));
        (name, label, display)
    }

    fn rebuild(&mut self) {
        self.container = Container::new();
        let details = SkillDetails::from_message(&self.message);
        let (_, label, display) = self.chip_parts(&details);
        let leading = leading_draft(&display, &label);

        if !leading {
            // Inline: the chip header is the summary row; the prompt rides in
            // the disclosure body as plain rows.
            let mut disclosure = Disclosure::new(self.header(&label, &details));
            if self.expanded {
                let prompt = message_text(&self.message);
                if !prompt.is_empty() {
                    disclosure.set_lines(prompt.lines().map(str::to_string).collect());
                }
            }
            disclosure.set_open(self.expanded);
            self.container.add_child(disclosure);
            return;
        }

        let body = display[label.len()..].trim().to_string();
        let mut children: Vec<std::boxed::Box<dyn Component>> =
            vec![std::boxed::Box::new(Text::new(self.header(&label, &details)))];
        if !body.is_empty() {
            children.push(std::boxed::Box::new(Spacer::new(1)));
            children.push(std::boxed::Box::new(self.markdown(&body)));
        }
        if self.expanded {
            let prompt = message_text(&self.message);
            if !prompt.is_empty() {
                children.push(std::boxed::Box::new(Spacer::new(1)));
                children.push(std::boxed::Box::new(Text::new(
                    self.theme
                        .fg("muted", "prompt")
                        .unwrap_or_else(|_| "prompt".to_string()),
                )));
                children.push(std::boxed::Box::new(Spacer::new(1)));
                children.push(std::boxed::Box::new(self.markdown(&prompt)));
            }
        }
        self.container.add_child(SkillCallout::new(children, &self.theme));
    }

    fn markdown(&self, text: &str) -> Markdown {
        let mut markdown = Markdown::new();
        markdown.set_text(text);
        markdown.set_theme(MarkdownTheme::default());
        markdown
    }

    /// Chip linked to its SKILL.md, then the muted prompt size.
    fn header(&self, label: &str, details: &SkillDetails) -> String {
        let chip = skill_chip_style(label, &bubble_reset(&self.theme), Some(&self.theme));
        let mut parts = vec![match &details.path {
            Some(path) => file_hyperlink(
                path,
                &chip,
                FilePosition {
                    line: Some(1),
                    col: None,
                },
            ),
            None => chip,
        }];
        if let Some(line_count) = details.line_count {
            parts.push(
                self.theme
                    .fg("muted", &format!("{line_count} {}", line_unit(line_count)))
                    .unwrap_or_default(),
            );
        }
        parts.join("  ")
    }
}

/// `1 line` vs `N lines`.
fn line_unit(line_count: i64) -> &'static str {
    if line_count == 1 {
        "line"
    } else {
        "lines"
    }
}

/// Whether the draft starts with the chip label.
fn leading_draft(display: &str, label: &str) -> bool {
    display.starts_with(label)
        && display[label.len()..]
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(true)
}

/// The submitted text: a bare string, else the joined text blocks.
fn message_text(message: &CustomMessage) -> String {
    match &message.content {
        CustomMessageContent::Text(text) => text.clone(),
        CustomMessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                MessageBlock::Text(text) => Some(text.text.as_str()),
                MessageBlock::Image(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Bubble foreground + background to re-arm after an inline chip.
fn bubble_reset(theme: &Theme) -> String {
    format!(
        "{}{}",
        theme
            .get_fg_on_bg_ansi("userMessageText", "userMessageBg")
            .unwrap_or_default(),
        theme.get_bg_ansi("userMessageBg").unwrap_or_default()
    )
}

impl Component for SkillMessageComponent {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.container.render(width)
    }
}

/// A user-bubble box with a skill-colored rail down its left edge.
pub struct SkillCallout {
    inner: BoxComponent,
    rail: String,
}

impl SkillCallout {
    /// Wrap `children` in the rail box.
    pub fn new(children: Vec<std::boxed::Box<dyn Component>>, theme: &Theme) -> Self {
        let background = theme.get_bg_ansi("userMessageBg").unwrap_or_default();
        let styler: BoxStyler = Arc::new({
            let background = background.clone();
            move |value: &str| format!("{background}{value}")
        });
        let mut inner = BoxComponent::styled(1, 1, Some(styler), None);
        inner.set_ignore_tight(true);
        for child in children {
            inner.add_child(ErasedChild(child));
        }
        let glyph = theme.symbol("skill.rail").to_string();
        let rail = theme
            .bg("userMessageBg", &theme.fg("customMessageLabel", &glyph).unwrap_or(glyph))
            .unwrap_or_default();
        SkillCallout { inner, rail }
    }

    /// The rail text prepended to every inner row.
    pub fn rail(&self) -> &str {
        &self.rail
    }
}

/// Lets an already-boxed child join a [`BoxComponent`].
struct ErasedChild(std::boxed::Box<dyn Component>);

impl Component for ErasedChild {
    fn render(&mut self, width: u16) -> Vec<String> {
        self.0.render(width)
    }
}

impl Component for SkillCallout {
    fn render(&mut self, width: u16) -> Vec<String> {
        let inner = self.inner.render(width.saturating_sub(1).max(1));
        let rail = self.rail.clone();
        inner
            .into_iter()
            .map(|line| format!("{rail}{line}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::messages::{MessageAttribution, SKILL_PROMPT_MESSAGE_TYPE};
    use crate::theme::builtin::builtin_theme_json;
    use crate::theme::loader::{create_theme, CreateThemeOptions};
    use crate::theme::theme_class::ColorMode;
    use serde_json::json;

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

    fn message(prompt: &str, name: &str, path: Option<&str>, content: &str) -> CustomMessage {
        CustomMessage {
            custom_type: SKILL_PROMPT_MESSAGE_TYPE.to_string(),
            content: CustomMessageContent::Text(content.into()),
            display: true,
            details: Some(json!({
                "name": name,
                "path": path,
                "prompt": prompt,
                "lineCount": 12,
            })),
            attribution: MessageAttribution::User,
            timestamp: 0,
        }
    }

    #[test]
    fn details_are_read_from_the_payload() {
        let details =
            SkillDetails::from_message(&message("/skill:rust hi", "rust", Some("/s/SKILL.md"), ""));
        assert_eq!(details.name, "rust");
        assert_eq!(details.path.as_deref(), Some("/s/SKILL.md"));
        assert_eq!(details.line_count, Some(12));
        let empty = SkillDetails::from_message(&CustomMessage {
            details: None,
            ..message("x", "x", None, "")
        });
        assert!(empty.name.is_empty());
    }

    #[test]
    fn callout_layout_draws_the_rail_and_expands_the_prompt() {
        let theme = theme();
        let mut component = SkillMessageComponent::new(
            message("/skill:rust do it", "rust", Some("/s/SKILL.md"), "SKILL BODY"),
            None,
            &theme,
        );
        let rows = component.render(40);
        assert!(!rows.is_empty());
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).contains("rust"));
        component.set_expanded(true);
        let expanded = component.render(40);
        assert!(expanded.iter().any(|row| row.contains("SKILL BODY")));
        assert!(component.is_expanded());
    }

    #[test]
    fn inline_layout_keeps_the_chip_header_visible() {
        let theme = theme();
        let mut component = SkillMessageComponent::new(
            message("please use /skill:rust", "rust", None, "BODY"),
            None,
            &theme,
        );
        let rows = component.render(40);
        assert!(crate::ansi_scan::strip_ansi(&rows[0]).contains("rust"));
        component.handle_native_toggle("prompt", false);
        assert!(component.is_expanded());
    }

    #[test]
    fn describe_marks_the_skill_role_and_prompt_section() {
        let theme = theme();
        let mut component =
            SkillMessageComponent::new(message("/skill:rust", "rust", None, "BODY"), None, &theme);
        let described = component.describe();
        assert_eq!(described.props.get_str("role"), Some("omp.skill"));
        assert_eq!(described.props.get_str("tone"), Some("user"));
        assert_eq!(described.children.len(), 2);
        assert_eq!(
            described.children[1].props.get_str("role"),
            Some("omp.skill.prompt")
        );
        assert_eq!(described.children[1].props.get_bool("collapsed"), Some(true));
        component.set_expanded(true);
        let expanded = component.describe();
        assert_eq!(
            expanded.children[1].props.get_bool("collapsed"),
            Some(false)
        );
    }

    #[test]
    fn finalization_flags_track_optimistic_rows() {
        let theme = theme();
        let mut component =
            SkillMessageComponent::new(message("/skill:rust", "rust", None, "x"), None, &theme);
        assert!(component.is_transcript_block_finalized());
        component.mark_transcript_block_pending();
        assert!(!component.is_transcript_block_finalized());
        component.mark_transcript_block_finalized();
        assert!(component.is_transcript_block_finalized());
    }

    #[test]
    fn leading_drafts_need_a_word_boundary_after_the_label() {
        assert!(leading_draft("chip rest", "chip"));
        assert!(leading_draft("chip", "chip"));
        assert!(!leading_draft("chipx", "chip"));
    }
}
