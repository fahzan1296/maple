//! Port of `l/test/assistant-message-link-targets.test.ts` (pure
//! `assistantMessageLinkTargets` describe only; the component describe needs
//! `createAssistantMessageComponent`, which the crate does not provide).

use std::collections::HashMap;

use maple_tui::chat::messages::{AssistantContent, AssistantMessage, TextContent};
use maple_tui::prompt::interactive_context_helpers::assistant_message_link_targets;

fn assistant(text: &str) -> AssistantMessage {
	AssistantMessage {
		content: vec![AssistantContent::Text(TextContent {
			text: text.to_string(),
			..Default::default()
		})],
		timestamp: 0,
		..Default::default()
	}
}

#[test]
fn picks_up_a_link_that_completes_while_its_streaming_text_block_grows_in_place() {
	let mut message = assistant("See [the report](local://report");
	let targets = HashMap::from([(
		"local://report.md".to_string(),
		"file:///tmp/report.md".to_string(),
	)]);
	assert!(assistant_message_link_targets(&message, &targets).is_empty());

	if let AssistantContent::Text(text) = &mut message.content[0] {
		text.text.push_str(".md) for details.");
	}
	assert_eq!(
		assistant_message_link_targets(&message, &targets),
		HashMap::from([(
			"local://report.md".to_string(),
			"file:///tmp/report.md".to_string()
		)])
	);
}
