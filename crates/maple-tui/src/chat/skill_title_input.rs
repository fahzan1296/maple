//! Compact title text for a user-invoked `/skill:<name>` prompt.
//!
//! Reference: `l/src/chat/skill-title-input.ts`.
//!
//! The title model's input is the composer chip text, else the prompt, else the
//! `/skill:name args` selector reconstruction — never the expanded SKILL.md
//! body. The same string is what a rewind puts back in the editor.
//!
//! Deviations: the reference's partial object literals become explicit
//! [`SkillPromptTitleInput`] / [`SkillPromptTitleMessage`] structs.

use serde_json::Value;

/// The skill prompt's parts as the composer knows them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkillPromptTitleInput<'a> {
    /// Skill name.
    pub name: Option<&'a str>,
    /// Arguments after the name.
    pub args: Option<&'a str>,
    /// The expanded prompt text.
    pub prompt: Option<&'a str>,
    /// The queued message's chip text.
    pub queue_chip_text: Option<&'a str>,
}

/// The persisted custom message a title is derived from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkillPromptTitleMessage<'a> {
    /// Message role (`custom` for skill prompts).
    pub role: &'a str,
    /// Custom type tag.
    pub custom_type: Option<&'a str>,
    /// Who initiated the message.
    pub attribution: Option<&'a str>,
    /// Type-specific details.
    pub details: Option<&'a Value>,
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// The title-model input: chip text, then prompt, then `/skill:name args`.
pub fn skill_prompt_title_input(input: &SkillPromptTitleInput<'_>) -> String {
    if let Some(chip) = trimmed(input.queue_chip_text) {
        return chip.to_string();
    }
    if let Some(prompt) = trimmed(input.prompt) {
        return prompt.to_string();
    }
    match (trimmed(input.name), trimmed(input.args)) {
        (Some(name), Some(args)) => format!("/skill:{name} {args}"),
        (Some(name), None) => format!("/skill:{name}"),
        (None, args) => args.unwrap_or_default().to_string(),
    }
}

/// Title text for a persisted skill-prompt custom message, or `None` for any
/// other message.
pub fn title_text_from_skill_prompt(message: &SkillPromptTitleMessage<'_>) -> Option<String> {
    if message.role != "custom" || message.custom_type != Some("skill-prompt") {
        return None;
    }
    if message.attribution != Some("user") {
        return None;
    }
    let field = |key: &str| message.details.and_then(|details| details.get(key)).and_then(Value::as_str);
    let title = skill_prompt_title_input(&SkillPromptTitleInput {
        name: field("name"),
        args: field("args"),
        prompt: field("prompt"),
        queue_chip_text: field("__queueChipText"),
    });
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inputs_prefer_chip_then_prompt_then_selector() {
        let cases = [
            (
                SkillPromptTitleInput {
                    name: Some("review"),
                    args: Some("HEAD"),
                    prompt: Some("expanded"),
                    queue_chip_text: Some("  chip text  "),
                },
                "chip text",
            ),
            (
                SkillPromptTitleInput {
                    name: Some("review"),
                    args: Some("HEAD"),
                    prompt: Some(" expanded "),
                    queue_chip_text: None,
                },
                "expanded",
            ),
            (
                SkillPromptTitleInput {
                    name: Some("review"),
                    args: Some(" HEAD "),
                    prompt: None,
                    queue_chip_text: None,
                },
                "/skill:review HEAD",
            ),
            (
                SkillPromptTitleInput {
                    name: Some("review"),
                    args: None,
                    prompt: None,
                    queue_chip_text: None,
                },
                "/skill:review",
            ),
            (
                SkillPromptTitleInput {
                    name: None,
                    args: Some("just args"),
                    prompt: None,
                    queue_chip_text: None,
                },
                "just args",
            ),
            (SkillPromptTitleInput::default(), ""),
        ];
        for (input, expected) in cases {
            assert_eq!(skill_prompt_title_input(&input), expected);
        }
    }

    #[test]
    fn titles_come_only_from_user_skill_prompts() {
        let details = json!({"name": "review", "args": "HEAD", "prompt": "expanded"});
        let message = SkillPromptTitleMessage {
            role: "custom",
            custom_type: Some("skill-prompt"),
            attribution: Some("user"),
            details: Some(&details),
        };
        assert_eq!(
            title_text_from_skill_prompt(&message).as_deref(),
            Some("expanded")
        );

        let chip_details = json!({"name": "review", "__queueChipText": "/skill:review HEAD"});
        let queued = SkillPromptTitleMessage {
            details: Some(&chip_details),
            ..message
        };
        assert_eq!(
            title_text_from_skill_prompt(&queued).as_deref(),
            Some("/skill:review HEAD")
        );

        let others = [
            SkillPromptTitleMessage {
                role: "user",
                ..message
            },
            SkillPromptTitleMessage {
                custom_type: Some("collab-prompt"),
                ..message
            },
            SkillPromptTitleMessage {
                attribution: Some("agent"),
                ..message
            },
            SkillPromptTitleMessage {
                details: None,
                ..message
            },
        ];
        for other in others {
            assert_eq!(title_text_from_skill_prompt(&other), None);
        }
    }
}
