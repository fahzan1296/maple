//! Reference: `l/test/prompt-action-autocomplete.test.ts`.
//!
//! Ports the provider-level `^` mention / `#action` / slash-argument / internal
//! URL / GitHub-ref cases. The reference's `getArgumentCompletions` command
//! hook and the editor-level popup cases are not ported: [`SlashCommand`] has
//! no per-command argument-completion callback, and `Editor` exposes no
//! `is_showing_autocomplete`/`on_autocomplete_update`/`insert_text` seam.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use maple_tui::autocomplete::{AutocompleteContext, AutocompleteProvider, SlashCommand, Suggestions};
use maple_tui::keybindings::KeybindingsManager;
use maple_tui::prompt::internal_url_autocomplete::{
	set_internal_url_completion_host, InternalUrlCandidate, InternalUrlCompletionHost,
	InternalUrlCallerContext,
};
use maple_tui::prompt::model_mention_autocomplete::{
	ModelMentionCandidate, ModelMentionCandidateSource, ModelMentionModel,
};
use maple_tui::prompt::prompt_action_autocomplete::{
	create_prompt_action_autocomplete_provider, PromptActionAutocompleteOptions,
};

const FIVE_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const BASE_PATH: &str = "/nonexistent-maple-prompt-action-test";

fn ctx(lines: &[&str], cursor_col: usize) -> AutocompleteContext {
	AutocompleteContext::new(lines.iter().map(|line| line.to_string()).collect(), 0, cursor_col)
}

fn mention(provider: &str, id: &str, name: &str, efforts: &[&str]) -> ModelMentionCandidate {
	let selector = format!("{provider}/{id}");
	ModelMentionCandidate {
		selector: selector.clone(),
		model: ModelMentionModel {
			id: id.to_string(),
			name: name.to_string(),
			selector,
			efforts: efforts.iter().map(|effort| effort.to_string()).collect(),
		},
	}
}

struct MentionSource {
	candidates: Box<dyn Fn(&str) -> Vec<ModelMentionCandidate>>,
}

impl ModelMentionCandidateSource for MentionSource {
	fn candidates(&self, query: &str) -> Vec<ModelMentionCandidate> {
		(self.candidates)(query)
	}
}

fn mention_source<F>(candidates: F) -> Option<Box<dyn ModelMentionCandidateSource>>
where
	F: Fn(&str) -> Vec<ModelMentionCandidate> + 'static,
{
	Some(Box::new(MentionSource {
		candidates: Box::new(candidates),
	}))
}

/// A source that mirrors the reference's `modelMentions(query)` filter.
fn matching(candidate: ModelMentionCandidate) -> impl Fn(&str) -> Vec<ModelMentionCandidate> {
	move |query| {
		candidate
			.selector
			.contains(query)
			.then(|| vec![candidate.clone()])
	}
}

struct StubUrlHost;

impl InternalUrlCompletionHost for StubUrlHost {
	fn completion_schemes(&self) -> Vec<String> {
		vec!["omp".to_string()]
	}

	fn resolve_completions(
		&self,
		scheme: &str,
		_query: &str,
		_context: &InternalUrlCallerContext,
	) -> Option<Vec<InternalUrlCandidate>> {
		if scheme == "omp" {
			Some(vec![InternalUrlCandidate {
				value: "docs".to_string(),
				label: Some("docs".to_string()),
				description: None,
			}])
		} else {
			None
		}
	}
}

fn install_url_host() {
	set_internal_url_completion_host(Some(Arc::new(StubUrlHost)));
}

fn options(
	commands: Vec<SlashCommand>,
	model_mentions: Option<Box<dyn ModelMentionCandidateSource>>,
) -> PromptActionAutocompleteOptions {
	PromptActionAutocompleteOptions {
		commands,
		base_path: PathBuf::from(BASE_PATH),
		command_usage: None,
		internal_url_caller: None,
		model_mentions,
		keybindings: KeybindingsManager::new(),
		editor_keybindings: KeybindingsManager::new(),
		format_keys: Some(Box::new(|keys: &[String]| keys.join("/"))),
		copy_current_line: Box::new(|| {}),
		copy_prompt: Box::new(|| {}),
		undo: Box::new(|_prefix: &str| {}),
		move_cursor_to_message_end: Box::new(|| {}),
		move_cursor_to_message_start: Box::new(|| {}),
		move_cursor_to_line_start: Box::new(|| {}),
		move_cursor_to_line_end: Box::new(|| {}),
	}
}

fn labels(suggestions: &Suggestions) -> Vec<&str> {
	suggestions.items.iter().map(|item| item.label.as_str()).collect()
}

#[test]
fn shows_prompt_actions_with_configured_shortcut_hints() {
	let mut keybindings = KeybindingsManager::new();
	keybindings.set_user_binding("app.clipboard.copyLine", Some(vec!["ctrl+shift+l".to_string()]));
	keybindings.set_user_binding(
		"app.clipboard.copyPrompt",
		Some(vec!["alt+shift+c".to_string(), "ctrl+shift+c".to_string()]),
	);
	let mut opts = options(Vec::new(), None);
	opts.keybindings = keybindings;
	let provider = create_prompt_action_autocomplete_provider(opts);

	let suggestions = provider
		.get_suggestions(&ctx(&["#"], 1))
		.expect("prompt actions");
	assert_eq!(suggestions.prefix, "#");
	assert_eq!(
		labels(&suggestions),
		vec![
			"Copy current line",
			"Copy whole prompt",
			"Undo",
			"Move cursor to message end",
			"Move cursor to message start",
			"Move cursor to line start",
			"Move cursor to line end",
		]
	);
	assert_eq!(suggestions.items[0].description.as_deref(), Some("ctrl+shift+l"));
	assert_eq!(
		suggestions.items[1].description.as_deref(),
		Some("alt+shift+c/ctrl+shift+c")
	);
}

#[test]
fn passes_the_typed_trigger_to_undo_and_leaves_text_removal_to_the_editor() {
	let undo_calls = Rc::new(Cell::new(0));
	let undo_prefix = Rc::new(RefCell::new(String::new()));
	let mut opts = options(Vec::new(), None);
	opts.undo = {
		let undo_calls = Rc::clone(&undo_calls);
		let undo_prefix = Rc::clone(&undo_prefix);
		Box::new(move |prefix: &str| {
			undo_calls.set(undo_calls.get() + 1);
			*undo_prefix.borrow_mut() = prefix.to_string();
		})
	};
	let provider = create_prompt_action_autocomplete_provider(opts);

	let suggestions = provider
		.get_suggestions(&ctx(&["hello #undo"], 11))
		.expect("undo row");
	let item = suggestions
		.items
		.iter()
		.find(|item| item.label == "Undo")
		.expect("undo row");
	let applied = provider.apply_completion(&ctx(&["hello #undo"], 11), item, &suggestions.prefix);
	assert_eq!(applied.lines, vec!["hello #undo".to_string()]);
	assert_eq!(applied.cursor_line, 0);
	assert_eq!(applied.cursor_col, 11);
	assert_eq!(undo_calls.get(), 1);
	assert_eq!(*undo_prefix.borrow(), "#undo");
}

#[test]
fn falls_back_to_normal_typing_for_literal_hashtags_with_no_matching_action() {
	let provider = create_prompt_action_autocomplete_provider(options(Vec::new(), None));
	assert!(provider.get_suggestions(&ctx(&["release #v1"], 11)).is_none());
}

#[test]
fn treats_hash_prompt_action_tokens_as_literal_text_inside_slash_command_arguments() {
	let commands = vec![SlashCommand::new("rename")
		.with_description("Rename current session")
		.with_allow_args(true)];
	let provider = create_prompt_action_autocomplete_provider(options(commands, None));
	assert!(provider
		.get_suggestions(&ctx(&["/rename repro #copy"], 19))
		.is_none());
}

#[test]
fn returns_hash_prompt_action_completions_for_matched_slash_commands_that_reject_arguments() {
	let commands = vec![SlashCommand::new("settings")
		.with_description("Open settings")
		.with_allow_args(false)];
	let provider = create_prompt_action_autocomplete_provider(options(commands, None));
	let suggestions = provider
		.get_suggestions(&ctx(&["/settings #copy"], 15))
		.expect("prompt actions");
	assert_eq!(suggestions.prefix, "#copy");
	assert_eq!(labels(&suggestions), vec!["Copy current line", "Copy whole prompt"]);
}

#[test]
fn falls_through_to_internal_url_completion_for_allow_args_commands() {
	install_url_host();
	let commands = vec![SlashCommand::new("btw")
		.with_description("By the way")
		.with_allow_args(true)];
	let provider = create_prompt_action_autocomplete_provider(options(commands, None));
	let suggestions = provider
		.get_suggestions(&ctx(&["/btw omp://"], 11))
		.expect("internal url");
	assert_eq!(suggestions.prefix, "omp://");
	assert!(!suggestions.items.is_empty());
}

#[test]
fn delegates_try_sync_slash_completion() {
	let commands = vec![SlashCommand::new("model").with_description("Switch AI model")];
	let provider = create_prompt_action_autocomplete_provider(options(commands, None));
	let suggestions = provider.try_sync_slash_completion("/mo").expect("slash completion");
	assert!(suggestions.items.iter().any(|item| item.value == "model"));
}

#[test]
fn returns_null_from_try_sync_slash_completion_for_non_slash_text() {
	let commands = vec![SlashCommand::new("model").with_description("Switch AI model")];
	let provider = create_prompt_action_autocomplete_provider(options(commands, None));
	assert!(provider.try_sync_slash_completion("hello").is_none());
}

#[test]
fn suggests_model_mentions_and_reanchors_completion_to_the_live_token() {
	let candidate = mention("a", "x", "X One", &[]);
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		mention_source(move |query| {
			(query.is_empty() || candidate.selector.contains(query)).then(|| vec![candidate.clone()])
		}),
	));

	let suggestions = provider
		.get_suggestions(&ctx(&["see ^a"], 6))
		.expect("mention");
	assert_eq!(suggestions.prefix, "^a");
	assert_eq!(suggestions.items.len(), 1);
	assert_eq!(suggestions.items[0].value, "a/x");
	assert_eq!(suggestions.items[0].label, "a/x");
	assert_eq!(suggestions.items[0].description.as_deref(), Some("X One"));
	assert_eq!(suggestions.items[0].icon_name.as_deref(), Some("model"));

	let live_line = "see ^a-extra";
	let applied = provider.apply_completion(
		&ctx(&[live_line], live_line.chars().count()),
		&suggestions.items[0],
		&suggestions.prefix,
	);
	assert_eq!(applied.lines, vec!["see ^a/x ".to_string()]);
	assert_eq!(applied.cursor_col, 9);
}

#[test]
fn limits_model_mention_suggestions_to_the_first_twenty_ranked_candidates() {
	let candidates: Vec<ModelMentionCandidate> = (1..=21)
		.map(|index| mention("demo", &format!("model-{index}"), &format!("Model {index}"), &[]))
		.collect();
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		mention_source(move |query| {
			candidates
				.iter()
				.filter(|candidate| candidate.selector.contains(query))
				.cloned()
				.collect()
		}),
	));
	let suggestions = provider
		.get_suggestions(&ctx(&["^"], 1))
		.expect("mentions");
	assert_eq!(suggestions.items.len(), 20);
	assert_eq!(suggestions.items[0].value, "demo/model-1");
	assert_eq!(suggestions.items[19].value, "demo/model-20");
}

#[test]
fn re_lists_model_mentions_once_a_discovery_retry_recovers_models() {
	let candidates = Rc::new(RefCell::new(vec![mention("a", "x", "X One", &[])]));
	let retried = Rc::new(Cell::new(false));
	struct RetrySource {
		candidates: Rc<RefCell<Vec<ModelMentionCandidate>>>,
		retried: Rc<Cell<bool>>,
	}
	impl ModelMentionCandidateSource for RetrySource {
		fn candidates(&self, query: &str) -> Vec<ModelMentionCandidate> {
			self.candidates
				.borrow()
				.iter()
				.filter(|candidate| candidate.selector.contains(query))
				.cloned()
				.collect()
		}
		fn retry_discovery(&self) -> bool {
			self.retried.replace(true)
		}
	}
	let source = RetrySource {
		candidates: Rc::clone(&candidates),
		retried: Rc::clone(&retried),
	};
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		Some(Box::new(source)),
	));

	let first = provider
		.get_suggestions(&ctx(&["^x"], 2))
		.expect("first mention");
	assert_eq!(
		first.items.iter().map(|item| item.value.as_str()).collect::<Vec<_>>(),
		vec!["a/x"]
	);
	assert!(retried.get());

	candidates
		.borrow_mut()
		.insert(0, mention("b", "xx", "Recovered", &[]));
	let second = provider
		.get_suggestions(&ctx(&["^x"], 2))
		.expect("recovered mention");
	assert_eq!(
		second.items.iter().map(|item| item.value.as_str()).collect::<Vec<_>>(),
		vec!["b/xx", "a/x"]
	);
}

#[test]
fn does_not_suggest_a_model_mention_without_a_left_token_boundary() {
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		mention_source(matching(mention("a", "x", "X One", &[]))),
	));
	assert!(provider.get_suggestions(&ctx(&["a^b"], 3)).is_none());
}

#[test]
fn suggests_model_mentions_inside_slash_command_arguments() {
	let commands = vec![SlashCommand::new("btw")
		.with_description("By the way")
		.with_allow_args(true)];
	let provider = create_prompt_action_autocomplete_provider(options(
		commands,
		mention_source(matching(mention("a", "x", "X One", &[]))),
	));
	let suggestions = provider
		.get_suggestions(&ctx(&["/btw ^a"], 7))
		.expect("mention");
	assert_eq!(suggestions.prefix, "^a");
	assert_eq!(
		suggestions.items.iter().map(|item| item.value.as_str()).collect::<Vec<_>>(),
		vec!["a/x"]
	);
}

#[test]
fn lists_the_models_supported_effort_levels_after_a_colon() {
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		mention_source(matching(mention("a", "x", "X One", &FIVE_LEVELS))),
	));

	let all = provider
		.get_suggestions(&ctx(&["ask ^x:"], 7))
		.expect("effort rows");
	let rows: Vec<(String, String)> = all
		.items
		.iter()
		.map(|item| (item.value.clone(), item.description.clone().unwrap_or_default()))
		.collect();
	assert_eq!(
		rows,
		vec![
			("a/x:low".to_string(), "X One · low".to_string()),
			("a/x:medium".to_string(), "X One · medium".to_string()),
			("a/x:high".to_string(), "X One · high".to_string()),
			("a/x:xhigh".to_string(), "X One · xhigh".to_string()),
			("a/x:max".to_string(), "X One · max".to_string()),
		]
	);

	let high = provider
		.get_suggestions(&ctx(&["ask ^x:hi"], 9))
		.expect("filtered effort rows");
	assert_eq!(
		high.items.iter().map(|item| item.value.as_str()).collect::<Vec<_>>(),
		vec!["a/x:high"]
	);
}

#[test]
fn lists_model_ids_that_contain_the_query_ahead_of_effort_rows() {
	let candidates = vec![
		mention("a", "glm", "GLM", &FIVE_LEVELS),
		mention("a", "glm:max", "GLM Max", &[]),
	];
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		mention_source(move |query| {
			candidates
				.iter()
				.filter(|candidate| candidate.selector.contains(query))
				.cloned()
				.collect()
		}),
	));
	let suggestions = provider
		.get_suggestions(&ctx(&["^glm:"], 5))
		.expect("rows");
	let rows: Vec<(String, String)> = suggestions
		.items
		.iter()
		.map(|item| (item.value.clone(), item.description.clone().unwrap_or_default()))
		.collect();
	assert_eq!(
		rows,
		vec![
			("a/glm:max".to_string(), "GLM Max".to_string()),
			("a/glm:low".to_string(), "GLM · low".to_string()),
			("a/glm:medium".to_string(), "GLM · medium".to_string()),
			("a/glm:high".to_string(), "GLM · high".to_string()),
			("a/glm:xhigh".to_string(), "GLM · xhigh".to_string()),
		]
	);
}

#[test]
fn does_not_offer_effort_rows_for_a_model_without_a_controllable_effort() {
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		mention_source(matching(mention("a", "x", "X One", &[]))),
	));
	assert!(provider.get_suggestions(&ctx(&["^x:hi"], 5)).is_none());
}

#[test]
fn leaves_hash_tokens_inside_a_mention_to_their_own_completions() {
	let provider = create_prompt_action_autocomplete_provider(options(
		Vec::new(),
		mention_source(matching(mention("a", "x", "X One", &FIVE_LEVELS))),
	));

	let action = provider
		.get_suggestions(&ctx(&["^x#copy"], 7))
		.expect("prompt action");
	assert_eq!(action.prefix, "#copy");
	assert_eq!(action.items[0].label, "Copy current line");

	let reference = provider
		.get_suggestions(&ctx(&["^x(#12"], 6))
		.expect("github refs");
	assert_eq!(reference.prefix, "#12");
	assert_eq!(
		reference.items.iter().map(|item| item.value.as_str()).collect::<Vec<_>>(),
		vec!["pr://12", "issue://12"]
	);
}
