//! Port of `l/test/assistant-message-narration.test.ts`.
//!
//! Deviations: the reference builds the component from the message plus a
//! hide-thinking flag; the crate takes `AssistantMessageComponent::new(theme)`
//! with `set_hide_thinking_block(true)` and `update_content`. Rows are
//! ANSI-stripped for the same containment assertions as the reference.

use maple_tui::chat::assistant_message::AssistantMessageComponent;
use maple_tui::chat::messages::{AssistantContent, AssistantMessage, ThinkingContent};
use maple_tui::component::Component;
use maple_tui::theme::default_theme;

const REASONING: &str = "Weighing whether the gradle run already finished.";
const NARRATION: &str = "The build is running; it should finish in about fifteen minutes.";

fn turn(narration: bool) -> AssistantMessage {
	AssistantMessage {
		content: vec![
			AssistantContent::Thinking(ThinkingContent {
				thinking: REASONING.to_string(),
				thinking_signature: Some("sig-reasoning".to_string()),
				..Default::default()
			}),
			AssistantContent::Thinking(ThinkingContent {
				thinking: NARRATION.to_string(),
				thinking_signature: Some("sig-narration".to_string()),
				narration: if narration { Some(true) } else { None },
				..Default::default()
			}),
		],
		..Default::default()
	}
}

fn rendered(message: AssistantMessage) -> String {
	let mut component = AssistantMessageComponent::new(default_theme());
	component.set_hide_thinking_block(true);
	component.update_content(message, false);
	maple_tui::strip_ansi(&component.render(100).join("\n"))
}

#[test]
fn renders_narration_as_answer_text_while_reasoning_stays_hidden() {
	let output = rendered(turn(true));
	assert!(output.contains(NARRATION), "narration must reach the transcript");
	assert!(!output.contains(REASONING), "reasoning must stay hidden");
}

#[test]
fn keeps_unmarked_thinking_hidden() {
	let output = rendered(turn(false));
	assert!(!output.contains(NARRATION), "unmarked thinking is not answer text");
	assert!(!output.contains(REASONING));
}
