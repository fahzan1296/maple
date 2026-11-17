//! Reference: `l/src/prompt/prompt-action-autocomplete.ts`.
//!
//! The prompt autocomplete provider: `^` model mentions, slash-command arguments,
//! `#<number>` GitHub refs, `#action` prompt actions, internal URLs, emoji
//! shortcodes, and the combined slash/`@`-file provider as fallback.
//!
//! Deviations:
//! - The async suggestion contract (`AbortSignal`, `onPartial`, awaiting a
//!   discovery retry) collapses to the synchronous
//!   [`AutocompleteProvider`] trait; a pending model-mention retry is started and
//!   the rows are recomputed on the next call instead of being awaited.
//! - Action items are plain tabled rows: `apply_completion` matches the action by
//!   `item.value` (the action label) because the reference tags items with
//!   `actionId`. The reference's `onApplied` callback is not part of the trait, so
//!   the action runs as part of `apply_completion`.
//! - `formatKeyHints` needs a theme and [`Theme`](crate::theme::Theme) is not
//!   `Sync`; key-hint formatting is an injected closure.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::autocomplete::{
	find_leading_slash_command_start, subsequence_match, subsequence_score, AppliedText,
	AutocompleteContext, AutocompleteItem, AutocompleteProvider, CombinedAutocompleteProvider,
	SlashCommand, Suggestions,
};
use crate::keybindings::KeybindingsManager;
use crate::prompt::emoji_autocomplete::{
	apply_emoji_completion, get_emoji_suggestions, is_emoji_prefix, try_emoji_inline_replace,
};
use crate::prompt::github_ref_autocomplete::{
	get_github_ref_context, get_github_ref_suggestions, GithubRefQualifier,
};
use crate::prompt::internal_url_autocomplete::{
	apply_internal_url_completion, get_internal_url_suggestions, is_internal_url_prefix,
	InternalUrlCallerContext,
};
use crate::prompt::model_mention_autocomplete::{
	apply_model_mention_completion, get_model_mention_suggestions, is_model_mention_prefix,
	ModelMentionCandidateSource, ModelMentionCompletion,
};

static EMOJI_AUTOCOMPLETE_ENABLED: AtomicBool = AtomicBool::new(true);

/// Set the process-wide emoji completion preference.
pub fn set_emoji_autocomplete_enabled(enabled: bool) {
	EMOJI_AUTOCOMPLETE_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Current emoji completion preference.
pub fn emoji_autocomplete_enabled() -> bool {
	EMOJI_AUTOCOMPLETE_ENABLED.load(Ordering::Relaxed)
}

/// One `#action` row.
#[derive(Clone)]
pub struct PromptActionDefinition {
	pub id: String,
	pub label: String,
	pub description: String,
	pub keywords: Vec<String>,
	/// Runs with the raw `#…` prefix the user typed.
	pub execute: Arc<dyn Fn(&str)>,
}

/// Everything the provider needs from the app.
pub struct PromptActionAutocompleteOptions {
	pub commands: Vec<SlashCommand>,
	pub base_path: PathBuf,
	/// Usage count per command name for frequency-ranked slash completions.
	pub command_usage: Option<Box<dyn Fn(&str) -> usize>>,
	/// Read the receiving session at lookup time, following focus and session
	/// replacement.
	pub internal_url_caller: Option<Box<dyn Fn() -> InternalUrlCallerContext>>,
	/// Session-scoped models available for `^` mentions.
	pub model_mentions: Option<Box<dyn ModelMentionCandidateSource>>,
	pub keybindings: KeybindingsManager,
	/// Editor keybindings, used for the undo hint.
	pub editor_keybindings: KeybindingsManager,
	/// Key-hint formatter (`["ctrl+k"]` → `⌃K`); default joins with `/`.
	pub format_keys: Option<Box<dyn Fn(&[String]) -> String>>,
	pub copy_current_line: Box<dyn Fn()>,
	pub copy_prompt: Box<dyn Fn()>,
	pub undo: Box<dyn Fn(&str)>,
	pub move_cursor_to_message_end: Box<dyn Fn()>,
	pub move_cursor_to_message_start: Box<dyn Fn()>,
	pub move_cursor_to_line_start: Box<dyn Fn()>,
	pub move_cursor_to_line_end: Box<dyn Fn()>,
}

/// The `#` token at the end of the text before the cursor, including the `#`.
fn get_prompt_action_prefix(text_before_cursor: &str) -> Option<String> {
	let hash_index = text_before_cursor.rfind('#')?;
	let query = &text_before_cursor[hash_index + 1..];
	if query.chars().any(char::is_whitespace) {
		return None;
	}
	Some(text_before_cursor[hash_index..].to_string())
}

fn apply_github_ref_completion(
	lines: &[String],
	cursor_line: usize,
	cursor_col: usize,
	item: &AutocompleteItem,
	prefix: &str,
) -> Option<(Vec<String>, usize, usize)> {
	get_github_ref_context(prefix)?;
	let scheme = if item.value.starts_with("pr://") {
		Some("pr")
	} else if item.value.starts_with("issue://") {
		Some("issue")
	} else {
		None
	};
	let scheme = scheme?;
	let current_line = lines.get(cursor_line).cloned().unwrap_or_default();
	let before: String = current_line.chars().take(cursor_col).collect();
	let live_context = get_github_ref_context(&before);
	let live_context = match live_context {
		Some(context) => context,
		None => return Some((lines.to_vec(), cursor_line, cursor_col)),
	};
	let qualifier_matches = match (&live_context.qualifier, scheme) {
		(None, _) => true,
		(Some(GithubRefQualifier::Pr), "pr") => true,
		(Some(GithubRefQualifier::Issue), "issue") => true,
		_ => false,
	};
	if !qualifier_matches {
		return Some((lines.to_vec(), cursor_line, cursor_col));
	}
	let replacement = AutocompleteItem::new(
		format!("{scheme}://{}", live_context.number),
		item.label.clone(),
	);
	Some(apply_internal_url_completion(
		lines,
		cursor_line,
		cursor_col,
		&replacement,
		&live_context.prefix,
	))
}

/// Prompt autocomplete with prompt actions, `^` mentions, and inline replace.
pub struct PromptActionAutocompleteProvider {
	commands: Vec<SlashCommand>,
	base_provider: CombinedAutocompleteProvider,
	actions: Vec<PromptActionDefinition>,
	internal_url_caller: Box<dyn Fn() -> InternalUrlCallerContext>,
	model_mentions: Option<Box<dyn ModelMentionCandidateSource>>,
}

impl PromptActionAutocompleteProvider {
	/// Build the provider.
	pub fn new(
		commands: Vec<SlashCommand>,
		base_path: PathBuf,
		actions: Vec<PromptActionDefinition>,
		command_usage: Option<Box<dyn Fn(&str) -> usize>>,
		internal_url_caller: Option<Box<dyn Fn() -> InternalUrlCallerContext>>,
		model_mentions: Option<Box<dyn ModelMentionCandidateSource>>,
	) -> Self {
		let base_provider = CombinedAutocompleteProvider::new(
			commands.clone(),
			base_path.clone(),
			command_usage,
		);
		let default_caller = base_path.to_string_lossy().into_owned();
		PromptActionAutocompleteProvider {
			commands,
			base_provider,
			actions,
			internal_url_caller: internal_url_caller.unwrap_or_else(|| {
				Box::new(move || InternalUrlCallerContext {
					cwd: Some(default_caller.clone()),
					..InternalUrlCallerContext::default()
				})
			}),
			model_mentions,
		}
	}

	fn action_for(&self, item: &AutocompleteItem) -> Option<&PromptActionDefinition> {
		self.actions.iter().find(|action| action.label == item.value)
	}

	fn is_in_slash_command_arguments(&self, ctx: &AutocompleteContext) -> bool {
		let text_before_cursor = ctx.text_before_cursor();
		let Some(leading_slash_start) = find_leading_slash_command_start(&text_before_cursor) else {
			return false;
		};
		if ctx.lines.iter().take(ctx.cursor_line).any(|line| !line.trim().is_empty()) {
			return false;
		}
		let command_text: String = text_before_cursor.chars().skip(leading_slash_start).collect();
		let Some(space_index) = command_text.find(' ') else {
			return false;
		};
		let command_name: String = command_text
			.chars()
			.skip(1)
			.take(space_index - 1)
			.collect();
		let command = self.commands.iter().find(|command| {
			command.name == command_name || command.aliases.iter().any(|alias| *alias == command_name)
		});
		command.is_some_and(|command| command.allow_args)
	}

	fn outranks_model_mention(&self, ctx: &AutocompleteContext) -> bool {
		let text_before_cursor = ctx.text_before_cursor();
		self.is_in_slash_command_arguments(ctx)
			|| get_github_ref_context(&text_before_cursor).is_some()
			|| get_prompt_action_prefix(&text_before_cursor).is_some()
	}

	fn model_mention_suggestions(&self, text_before_cursor: &str) -> Option<Suggestions> {
		let (suggestions, pending) = match get_model_mention_suggestions(
			text_before_cursor,
			self.model_mentions.as_deref(),
		) {
			ModelMentionCompletion::Matched { suggestions, pending } => (suggestions, pending),
			_ => return None,
		};
		if pending {
			// The retry was just started; reading again picks up whatever it
			// already settled synchronously.
			if let ModelMentionCompletion::Matched { suggestions: retry, .. } =
				get_model_mention_suggestions(text_before_cursor, self.model_mentions.as_deref())
			{
				return Some(Suggestions {
					items: retry.items,
					prefix: retry.prefix,
				});
			}
		}
		Some(Suggestions {
			items: suggestions.items,
			prefix: suggestions.prefix,
		})
	}

	fn internal_url_suggestions(&self, text_before_cursor: &str) -> Option<Suggestions> {
		let get_caller: &dyn Fn() -> InternalUrlCallerContext = &|| (self.internal_url_caller)();
		get_internal_url_suggestions(text_before_cursor, None, Some(get_caller))
			.map(|(items, prefix)| Suggestions { items, prefix })
	}
	pub fn try_sync_inline_replace(&self, text_before_cursor: &str) -> Option<(usize, String)> {
		if !emoji_autocomplete_enabled() {
			return None;
		}
		try_emoji_inline_replace(text_before_cursor)
	}
}

impl AutocompleteProvider for PromptActionAutocompleteProvider {
	fn get_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions> {
		let text_before_cursor = ctx.text_before_cursor();
		if !self.outranks_model_mention(ctx) {
			if let Some(mention) = self.model_mention_suggestions(&text_before_cursor) {
				return Some(mention);
			}
		}
		if self.is_in_slash_command_arguments(ctx) {
			if let Some(argument_suggestions) = self.base_provider.get_suggestions(ctx) {
				return Some(argument_suggestions);
			}
			if let Some(mention) = self.model_mention_suggestions(&text_before_cursor) {
				return Some(mention);
			}
			// No slash-argument completion for this input: preserve numeric GitHub
			// references and internal URLs while keeping `#copy`-style prompts literal.
			if let Some((items, prefix)) = get_github_ref_suggestions(&text_before_cursor) {
				return Some(Suggestions { items, prefix });
			}
			return self.internal_url_suggestions(&text_before_cursor);
		}

		if let Some((items, prefix)) = get_github_ref_suggestions(&text_before_cursor) {
			return Some(Suggestions { items, prefix });
		}
		if let Some(prompt_action_prefix) = get_prompt_action_prefix(&text_before_cursor) {
			let query: String = prompt_action_prefix.chars().skip(1).collect();
			let query = query.to_lowercase();
			let mut scored: Vec<(AutocompleteItem, i64)> = Vec::new();
			for action in &self.actions {
				let mut parts = vec![action.label.clone(), action.description.clone()];
				parts.extend(action.keywords.iter().cloned());
				let searchable = parts.join(" ").to_lowercase();
				if !subsequence_match(&query, &searchable) {
					continue;
				}
				let item = AutocompleteItem::new(action.label.clone(), action.label.clone())
					.with_description(action.description.clone());
				scored.push((item, subsequence_score(&query, &searchable)));
			}
			if !scored.is_empty() {
				scored.sort_by(|a, b| b.1.cmp(&a.1));
				return Some(Suggestions {
					items: scored.into_iter().map(|(item, _)| item).collect(),
					prefix: prompt_action_prefix,
				});
			}
		}

		if let Some(url_suggestions) = self.internal_url_suggestions(&text_before_cursor) {
			return Some(url_suggestions);
		}

		if emoji_autocomplete_enabled() {
			if let Some((items, prefix)) = get_emoji_suggestions(&text_before_cursor) {
				return Some(Suggestions { items, prefix });
			}
		}

		self.base_provider.get_suggestions(ctx)
	}

	fn apply_completion(
		&self,
		ctx: &AutocompleteContext,
		item: &AutocompleteItem,
		prefix: &str,
	) -> AppliedText {
		if let Some((lines, cursor_line, cursor_col)) =
			apply_github_ref_completion(&ctx.lines, ctx.cursor_line, ctx.cursor_col, item, prefix)
		{
			return AppliedText {
				lines,
				cursor_line,
				cursor_col,
			};
		}

		if prefix.starts_with('#') {
			if let Some(action) = self.action_for(item) {
				if action.id == "undo" {
					(action.execute)(prefix);
					return AppliedText {
						lines: ctx.lines.clone(),
						cursor_line: ctx.cursor_line,
						cursor_col: ctx.cursor_col,
					};
				}
				let current_line = ctx.current_line().to_string();
				let chars: Vec<char> = current_line.chars().collect();
				let before_prefix: String = chars
					.iter()
					.take(ctx.cursor_col.saturating_sub(prefix.chars().count()))
					.collect();
				let after_cursor: String = chars.iter().skip(ctx.cursor_col).collect();
				let mut new_lines = ctx.lines.clone();
				if ctx.cursor_line < new_lines.len() {
					new_lines[ctx.cursor_line] = format!("{before_prefix}{after_cursor}");
				}
				let result = AppliedText {
					lines: new_lines,
					cursor_line: ctx.cursor_line,
					cursor_col: before_prefix.chars().count(),
				};
				(action.execute)(prefix);
				return result;
			}
		}

		if is_model_mention_prefix(prefix) {
			let (lines, cursor_line, cursor_col) = apply_model_mention_completion(
				&ctx.lines,
				ctx.cursor_line,
				ctx.cursor_col,
				item,
				prefix,
			);
			return AppliedText {
				lines,
				cursor_line,
				cursor_col,
			};
		}

		if is_internal_url_prefix(prefix) {
			let (lines, cursor_line, cursor_col) =
				apply_internal_url_completion(&ctx.lines, ctx.cursor_line, ctx.cursor_col, item, prefix);
			return AppliedText {
				lines,
				cursor_line,
				cursor_col,
			};
		}

		if is_emoji_prefix(prefix) {
			let (lines, cursor_line, cursor_col) =
				apply_emoji_completion(&ctx.lines, ctx.cursor_line, ctx.cursor_col, item, prefix);
			return AppliedText {
				lines,
				cursor_line,
				cursor_col,
			};
		}

		self.base_provider.apply_completion(ctx, item, prefix)
	}

	fn inline_hint(&self, ctx: &AutocompleteContext) -> Option<String> {
		self.base_provider.inline_hint(ctx)
	}

	fn try_sync_slash_completion(&self, text_before_cursor: &str) -> Option<Suggestions> {
		self.base_provider
			.try_sync_slash_completion(text_before_cursor)
	}

	fn get_force_file_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions> {
		self.base_provider.get_force_file_suggestions(ctx)
	}

	fn should_trigger_file_completion(&self, ctx: &AutocompleteContext) -> bool {
		self.base_provider.should_trigger_file_completion(ctx)
	}
}

/// Build the provider with the reference's standard `#` actions.
pub fn create_prompt_action_autocomplete_provider(
	options: PromptActionAutocompleteOptions,
) -> PromptActionAutocompleteProvider {
	let format_keys = options
		.format_keys
		.unwrap_or_else(|| Box::new(|keys: &[String]| keys.join("/")));
	let description_for = |keybindings: &KeybindingsManager, id: &str| {
		let keys = keybindings.get_keys(id);
		format_keys(&keys)
	};
	let PromptActionAutocompleteOptions {
		commands,
		base_path,
		command_usage,
		internal_url_caller,
		model_mentions,
		keybindings,
		editor_keybindings,
		format_keys: _,
		copy_current_line,
		copy_prompt,
		undo,
		move_cursor_to_message_end,
		move_cursor_to_message_start,
		move_cursor_to_line_start,
		move_cursor_to_line_end,
	} = options;

	let copy_current_line = Arc::new(copy_current_line);
	let copy_prompt = Arc::new(copy_prompt);
	let undo = Arc::new(undo);
	let move_cursor_to_message_end = Arc::new(move_cursor_to_message_end);
	let move_cursor_to_message_start = Arc::new(move_cursor_to_message_start);
	let move_cursor_to_line_start = Arc::new(move_cursor_to_line_start);
	let move_cursor_to_line_end = Arc::new(move_cursor_to_line_end);

	let actions = vec![
		PromptActionDefinition {
			id: "copy-line".to_string(),
			label: "Copy current line".to_string(),
			description: description_for(&keybindings, "app.clipboard.copyLine"),
			keywords: vec![
				"copy".to_string(),
				"line".to_string(),
				"clipboard".to_string(),
				"current".to_string(),
			],
			execute: {
				let copy_current_line = copy_current_line.clone();
				Arc::new(move |_prefix: &str| copy_current_line())
			},
		},
		PromptActionDefinition {
			id: "copy-prompt".to_string(),
			label: "Copy whole prompt".to_string(),
			description: description_for(&keybindings, "app.clipboard.copyPrompt"),
			keywords: vec![
				"copy".to_string(),
				"prompt".to_string(),
				"clipboard".to_string(),
				"message".to_string(),
			],
			execute: {
				let copy_prompt = copy_prompt.clone();
				Arc::new(move |_prefix: &str| copy_prompt())
			},
		},
		PromptActionDefinition {
			id: "undo".to_string(),
			label: "Undo".to_string(),
			description: description_for(&editor_keybindings, "tui.editor.undo"),
			keywords: vec![
				"undo".to_string(),
				"revert".to_string(),
				"edit".to_string(),
				"history".to_string(),
			],
			execute: {
				let undo = undo.clone();
				Arc::new(move |prefix: &str| undo(prefix))
			},
		},
		PromptActionDefinition {
			id: "cursor-message-end".to_string(),
			label: "Move cursor to message end".to_string(),
			description: "Current message".to_string(),
			keywords: ["move", "cursor", "message", "end", "prompt", "last", "bottom"]
				.iter()
				.map(|word| word.to_string())
				.collect(),
			execute: {
				let move_cursor_to_message_end = move_cursor_to_message_end.clone();
				Arc::new(move |_prefix: &str| move_cursor_to_message_end())
			},
		},
		PromptActionDefinition {
			id: "cursor-message-start".to_string(),
			label: "Move cursor to message start".to_string(),
			description: "Current message".to_string(),
			keywords: [
				"move", "cursor", "message", "start", "beginning", "prompt", "first", "top",
			]
			.iter()
			.map(|word| word.to_string())
			.collect(),
			execute: {
				let move_cursor_to_message_start = move_cursor_to_message_start.clone();
				Arc::new(move |_prefix: &str| move_cursor_to_message_start())
			},
		},
		PromptActionDefinition {
			id: "cursor-line-start".to_string(),
			label: "Move cursor to line start".to_string(),
			description: description_for(&editor_keybindings, "tui.editor.cursorLineStart"),
			keywords: ["move", "cursor", "line", "start", "beginning", "home"]
				.iter()
				.map(|word| word.to_string())
				.collect(),
			execute: {
				let move_cursor_to_line_start = move_cursor_to_line_start.clone();
				Arc::new(move |_prefix: &str| move_cursor_to_line_start())
			},
		},
		PromptActionDefinition {
			id: "cursor-line-end".to_string(),
			label: "Move cursor to line end".to_string(),
			description: description_for(&editor_keybindings, "tui.editor.cursorLineEnd"),
			keywords: ["move", "cursor", "line", "end"]
				.iter()
				.map(|word| word.to_string())
				.collect(),
			execute: {
				let move_cursor_to_line_end = move_cursor_to_line_end.clone();
				Arc::new(move |_prefix: &str| move_cursor_to_line_end())
			},
		},
	];

	PromptActionAutocompleteProvider::new(
		commands,
		base_path,
		actions,
		command_usage,
		internal_url_caller,
		model_mentions,
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::Cell;
	use std::rc::Rc;

	fn options(copied: Rc<Cell<usize>>, undone: Rc<Cell<usize>>) -> PromptActionAutocompleteOptions {
		PromptActionAutocompleteOptions {
			commands: crate::autocomplete::builtin_slash_commands(),
			base_path: PathBuf::from("."),
			command_usage: None,
			internal_url_caller: None,
			model_mentions: None,
			keybindings: KeybindingsManager::new(),
			editor_keybindings: KeybindingsManager::new(),
			format_keys: Some(Box::new(|keys: &[String]| keys.join("+"))),
			copy_current_line: {
				let copied = copied.clone();
				Box::new(move || copied.set(copied.get() + 1))
			},
			copy_prompt: Box::new(|| {}),
			undo: {
				let undone = undone.clone();
				Box::new(move |_prefix: &str| undone.set(undone.get() + 1))
			},
			move_cursor_to_message_end: Box::new(|| {}),
			move_cursor_to_message_start: Box::new(|| {}),
			move_cursor_to_line_start: Box::new(|| {}),
			move_cursor_to_line_end: Box::new(|| {}),
		}
	}

	fn ctx(lines: &[&str], line: usize, col: usize) -> AutocompleteContext {
		AutocompleteContext::new(lines.iter().map(|line| line.to_string()).collect(), line, col)
	}

	#[test]
	fn prompt_action_listing_and_prefix() {
		assert_eq!(
			get_prompt_action_prefix("please #cop"),
			Some("#cop".to_string())
		);
		assert_eq!(get_prompt_action_prefix("#copy now"), None);
		assert_eq!(get_prompt_action_prefix("none"), None);
	}

	#[test]
	fn suggests_matching_actions_ranked() {
		let copied = Rc::new(Cell::new(0));
		let undone = Rc::new(Cell::new(0));
		let provider = create_prompt_action_autocomplete_provider(options(copied, undone));
		let suggestions = provider.get_suggestions(&ctx(&["#copy"], 0, 5)).expect("items");
		assert_eq!(suggestions.prefix, "#copy");
		assert_eq!(suggestions.items[0].value, "Copy current line");
	}

	#[test]
	fn applies_action_by_removing_the_token() {
		let copied = Rc::new(Cell::new(0));
		let undone = Rc::new(Cell::new(0));
		let provider = create_prompt_action_autocomplete_provider(options(
			copied.clone(),
			undone.clone(),
		));
		let suggestions = provider.get_suggestions(&ctx(&["#copy"], 0, 5)).expect("items");
		let item = &suggestions.items[0];
		let applied = provider.apply_completion(&ctx(&["#copy"], 0, 5), item, "#copy");
		assert_eq!(applied.lines[0], "");
		assert_eq!(applied.cursor_col, 0);
		assert_eq!(copied.get(), 1);
	}

	#[test]
	fn undo_keeps_the_token() {
		let copied = Rc::new(Cell::new(0));
		let undone = Rc::new(Cell::new(0));
		let provider = create_prompt_action_autocomplete_provider(options(copied, undone.clone()));
		let suggestions = provider.get_suggestions(&ctx(&["#undo"], 0, 5)).expect("items");
		let item = suggestions
			.items
			.iter()
			.find(|item| item.value == "Undo")
			.expect("undo row");
		let applied = provider.apply_completion(&ctx(&["#undo"], 0, 5), item, "#undo");
		assert_eq!(applied.lines[0], "#undo");
		assert_eq!(undone.get(), 1);
	}

	#[test]
	fn github_ref_rows_apply_to_the_live_number() {
		let copied = Rc::new(Cell::new(0));
		let undone = Rc::new(Cell::new(0));
		let provider = create_prompt_action_autocomplete_provider(options(copied, undone));
		let suggestions = provider
			.get_suggestions(&ctx(&["fix #3164"], 0, 9))
			.expect("refs");
		assert_eq!(suggestions.items.len(), 2);
		let applied =
			provider.apply_completion(&ctx(&["fix #3164"], 0, 9), &suggestions.items[0], "#3164");
		assert_eq!(applied.lines[0], "fix pr://3164 ");
	}

	#[test]
	fn slash_argument_routing() {
		// A slash command without `allowArgs` is never argument context.
		let provider = PromptActionAutocompleteProvider::new(
			vec![SlashCommand::new("model").with_allow_args(false)],
			PathBuf::from("."),
			Vec::new(),
			None,
			None,
			None,
		);
		assert!(!provider.is_in_slash_command_arguments(&ctx(&["/model gpt"], 0, 10)));
		// A slash command with `allowArgs` and a typed space is argument context.
		let provider = PromptActionAutocompleteProvider::new(
			vec![SlashCommand::new("model").with_allow_args(true)],
			PathBuf::from("."),
			Vec::new(),
			None,
			None,
			None,
		);
		assert!(provider.is_in_slash_command_arguments(&ctx(&["/model gpt"], 0, 10)));
		assert!(!provider.is_in_slash_command_arguments(&ctx(&["/model"], 0, 6)));
		// Prompt text above the command line disables argument routing.
		assert!(!provider.is_in_slash_command_arguments(&ctx(&["prose", "/model gpt"], 1, 10)));
	}

	#[test]
	fn emoji_toggle_is_process_wide() {
		set_emoji_autocomplete_enabled(false);
		assert!(!emoji_autocomplete_enabled());
		set_emoji_autocomplete_enabled(true);
		assert!(emoji_autocomplete_enabled());
	}
}
