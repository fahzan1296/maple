//! Port of `l/test/editor-autocomplete-actions.test.ts`.
//!
//! The reference drives `Editor`'s asynchronous autocomplete scheduling (a
//! provider returning promises, `AbortSignal`, partial suggestions, popup
//! lifecycle). The crate's providers are synchronous and the base `Editor` only
//! accepts a `Tab` candidate, so the provider-level contracts — slash matching
//! and aliases, `#` actions, `^` mentions, `@` file completion, popup selection
//! — are asserted against `CombinedAutocompleteProvider`,
//! `PromptActionAutocompleteProvider`, `FileAutocompleteProvider`,
//! `SlashCommandProvider`, and `AutocompletePopup`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use maple_tui::autocomplete::{
	apply_model_mention_completion, AutocompleteContext, AutocompleteItem, AutocompletePopup,
	AutocompleteProvider, CombinedAutocompleteProvider, FileAutocompleteProvider, SlashCommand,
	SlashCommandProvider, Suggestions,
};
use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::prompt::model_mention_autocomplete::{
	ModelMentionCandidate, ModelMentionCandidateSource, ModelMentionCompletion, ModelMentionModel,
	get_model_mention_suggestions,
};
use maple_tui::prompt::prompt_action_autocomplete::{
	PromptActionAutocompleteProvider, PromptActionDefinition,
};

fn ctx(text: &str) -> AutocompleteContext {
	AutocompleteContext::new(vec![text.to_string()], 0, text.chars().count())
}

fn first_label(suggestions: &Suggestions) -> String {
	suggestions
		.items
		.first()
		.expect("at least one suggestion")
		.label
		.clone()
}

fn apply_first(provider: &dyn AutocompleteProvider, text: &str) -> maple_tui::autocomplete::AppliedText {
	let context = ctx(text);
	let suggestions = provider
		.get_suggestions(&context)
		.expect("suggestions for the context");
	let item = suggestions.items[0].clone();
	provider.apply_completion(&context, &item, &suggestions.prefix)
}

fn command(name: &str) -> SlashCommand {
	SlashCommand::new(name)
}

static DIRS: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(label: &str) -> PathBuf {
	let mut dir = std::env::temp_dir();
	dir.push(format!(
		"maple-autocomplete-{}-{}-{}",
		label,
		std::process::id(),
		DIRS.fetch_add(1, Ordering::SeqCst)
	));
	std::fs::create_dir_all(&dir).expect("temp dir");
	dir
}

fn cleanup(dir: &Path) {
	let _ = std::fs::remove_dir_all(dir);
}

// ── Slash commands ───────────────────────────────────────────────────────────

#[test]
fn slash_completion_replaces_characters_typed_after_the_popup_opened() {
	let provider = CombinedAutocompleteProvider::new(
		vec![command("skills:fix-bug").with_description("Fix a bug")],
		"/tmp",
		None,
	);
	let applied = apply_first(&provider, "/ski");
	assert_eq!(applied.lines[0], "/skills:fix-bug ");
	assert_eq!(applied.cursor_col, "/skills:fix-bug ".chars().count());
}

#[test]
fn slash_completion_matches_an_alias_and_a_description_subsequence() {
	let provider = CombinedAutocompleteProvider::new(
		vec![
			command("x").with_alias("abcd").with_description("ace"),
			command("loop").with_description(
				"Toggle loop mode, not a re-run: gate a re-check on command status",
			),
		],
		"/tmp",
		None,
	);

	let alias = provider
		.get_suggestions(&ctx("/ac"))
		.expect("alias suggestions");
	assert_eq!(first_label(&alias), "abcd");
	assert_eq!(apply_first(&provider, "/ac").lines[0], "/abcd ");

	let described = provider
		.get_suggestions(&ctx("/mde"))
		.expect("description suggestions");
	assert_eq!(first_label(&described), "loop");
	assert!(described.items[0].description_match);
}
#[test]
fn sync_slash_completion_matches_the_typed_command() {
	let provider = CombinedAutocompleteProvider::new(
		vec![command("login"), command("model")],
		"/tmp",
		None,
	);
	let suggestions = provider
		.try_sync_slash_completion("/mod")
		.expect("sync suggestions");
	assert_eq!(first_label(&suggestions), "model");
	assert!(provider.try_sync_slash_completion("/xyz").is_none());
}

#[test]
fn slash_command_usage_ranking_prefers_the_more_used_command() {
	let provider = CombinedAutocompleteProvider::new(
		vec![command("login"), command("model")],
		"/tmp",
		Some(Box::new(|name| if name == "model" { 5 } else { 0 })),
	);
	let suggestions = provider
		.try_sync_slash_completion("/")
		.expect("sync suggestions");
	assert_eq!(first_label(&suggestions), "model");
}

// ── `#` prompt actions ───────────────────────────────────────────────────────

fn action_provider(calls: &Arc<Mutex<usize>>) -> PromptActionAutocompleteProvider {
	let counter = Arc::clone(calls);
	PromptActionAutocompleteProvider::new(
		PathBuf::from("/tmp"),
		vec![PromptActionDefinition {
			id: "do".to_string(),
			label: "Do action".to_string(),
			description: "Do it".to_string(),
			keywords: vec!["action".to_string()],
			execute: Arc::new(move |_prefix: &str| *counter.lock().unwrap() += 1),
		}],
		None,
		None,
		None,
	)
}

#[test]
fn hash_action_selection_runs_the_action_and_clears_the_token() {
	let calls = Arc::new(Mutex::new(0usize));
	let provider = action_provider(&calls);
	let applied = apply_first(&provider, "#");
	assert_eq!(applied.lines[0], "");
	assert_eq!(applied.cursor_col, 0);
	assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn hash_query_ranks_actions_by_subsequence() {
	let calls = Arc::new(Mutex::new(0usize));
	let provider = action_provider(&calls);
	let suggestions = provider.get_suggestions(&ctx("#do")).expect("action suggestions");
	assert_eq!(first_label(&suggestions), "Do action");
	assert!(provider.get_suggestions(&ctx("#zzz")).is_none());
}

// ── `^` model mentions ───────────────────────────────────────────────────────

struct MentionSource;

impl ModelMentionCandidateSource for MentionSource {
	fn candidates(&self, query: &str) -> Vec<ModelMentionCandidate> {
		let selector = "a/x";
		if !selector.starts_with(query) {
			return Vec::new();
		}
		vec![ModelMentionCandidate {
			selector: selector.to_string(),
			model: ModelMentionModel {
				id: "a".to_string(),
				name: "A".to_string(),
				selector: selector.to_string(),
				efforts: vec!["low".to_string(), "high".to_string()],
			},
		}]
	}
}

#[test]
fn mention_triggers_at_line_space_and_tab_boundaries_but_not_inside_a_token() {
	for text in ["^", "ask ^", "ask\t^"] {
		match get_model_mention_suggestions(text, Some(&MentionSource)) {
			ModelMentionCompletion::Matched { suggestions, .. } => {
				assert_eq!(suggestions.items[0].value, "a/x");
				assert_eq!(suggestions.prefix, text.to_string());
			}
			other => panic!("{text}: no mention match: {other:?}"),
		}
	}
	assert!(matches!(
		get_model_mention_suggestions("a^b", Some(&MentionSource)),
		ModelMentionCompletion::Unhandled
	));
	assert!(matches!(
		get_model_mention_suggestions("^a", None),
		ModelMentionCompletion::Unhandled
	));
}

#[test]
fn mention_effort_levels_list_after_a_colon() {
	match get_model_mention_suggestions("^a:", Some(&MentionSource)) {
		ModelMentionCompletion::Matched { suggestions, .. } => {
			let values: Vec<&str> = suggestions.items.iter().map(|item| item.value.as_str()).collect();
			assert!(values.contains(&"a/x:low"), "values: {values:?}");
			assert!(values.contains(&"a/x:high"), "values: {values:?}");
		}
		other => panic!("no effort rows: {other:?}"),
	}
}

#[test]
fn mention_accept_replaces_the_token_with_the_selector_and_a_space() {
	let item = AutocompleteItem::new("a/x", "a/x");
	let (lines, _cursor_line, cursor_col) =
		apply_model_mention_completion(&["ask ^a".to_string()], 0, 6, &item, "^a");
	assert_eq!(lines[0], "ask ^a/x ");
	assert_eq!(cursor_col, "ask ^a/x ".chars().count());
}

#[test]
fn hash_actions_outrank_model_mentions() {
	let calls = Arc::new(Mutex::new(0usize));
	let provider = PromptActionAutocompleteProvider::new(
		Vec::new(),
		PathBuf::from("/tmp"),
		vec![PromptActionDefinition {
			id: "do".to_string(),
			label: "Do action".to_string(),
			description: "Do it".to_string(),
			keywords: vec![],
			execute: Arc::new(move |_prefix: &str| *calls.lock().unwrap() += 1),
		}],
		None,
		None,
		Some(Box::new(MentionSource)),
	);
	let suggestions = provider.get_suggestions(&ctx("#")).expect("action suggestions");
	assert_eq!(first_label(&suggestions), "Do action");
}

// ── `@` file completion ──────────────────────────────────────────────────────

#[test]
fn at_file_completion_lists_and_applies_a_file() {
	let dir = temp_dir("at-file");
	std::fs::write(dir.join("alpha.ts"), "export {};\n").expect("seed file");
	let provider = FileAutocompleteProvider::new(&dir);

	let suggestions = provider
		.get_suggestions(&ctx("@al"))
		.expect("file suggestions");
	assert_eq!(first_label(&suggestions), "alpha.ts");
	let applied = apply_first(&provider, "@al");
	assert_eq!(applied.lines[0], "@alpha.ts ");
	assert_eq!(applied.cursor_col, "@alpha.ts ".chars().count());
	cleanup(&dir);
}

#[test]
fn force_file_completion_applies_a_relative_path_after_leading_whitespace() {
	let dir = temp_dir("force");
	std::fs::write(dir.join("alpha.ts"), "export {};\n").expect("seed file");
	let provider = FileAutocompleteProvider::new(&dir);

	let context = ctx("  al");
	let suggestions = provider
		.get_force_file_suggestions(&context)
		.expect("forced file suggestions");
	assert_eq!(first_label(&suggestions), "alpha.ts");

	let item = suggestions.items[0].clone();
	let applied = provider.apply_completion(&context, &item, &suggestions.prefix);
	assert_eq!(applied.lines[0], "  alpha.ts ");
	cleanup(&dir);
}

#[test]
fn at_directory_accept_keeps_the_directory_open() {
	let dir = temp_dir("at-dir");
	std::fs::create_dir_all(dir.join("packages").join("tui")).expect("seed dirs");
	std::fs::create_dir_all(dir.join("packages").join("utils")).expect("seed dirs");
	let provider = FileAutocompleteProvider::new(&dir);

	let suggestions = provider
		.get_suggestions(&ctx("@pack"))
		.expect("directory suggestions");
	let item = suggestions.items[0].clone();
	assert!(item.value.ends_with('/'), "directory value: {}", item.value);
	let applied = provider.apply_completion(&ctx("@pack"), &item, &suggestions.prefix);
	assert_eq!(applied.lines[0], "@packages/");
	assert_eq!(applied.cursor_col, "@packages/".chars().count());
	cleanup(&dir);
}

#[test]
fn at_quoted_path_with_spaces_completes() {
	let dir = temp_dir("quoted");
	std::fs::create_dir_all(dir.join("0. before").join("obsidian calendar")).expect("seed dirs");
	let provider = FileAutocompleteProvider::new(&dir);

	let text = "@\"0. before/ob";
	let suggestions = provider.get_suggestions(&ctx(text)).expect("quoted suggestions");
	let item = suggestions.items[0].clone();
	let applied = provider.apply_completion(&ctx(text), &item, &suggestions.prefix);
	assert!(
		applied.lines[0].contains("obsidian calendar"),
		"applied: {}",
		applied.lines[0]
	);
	cleanup(&dir);
}

// ── Popup selection ──────────────────────────────────────────────────────────

#[test]
fn popup_updates_accepts_the_selection_and_wraps_navigation() {
	let provider = SlashCommandProvider::new(vec![
		command("alpha"),
		command("beta"),
		command("gamma"),
	]);
	let mut popup = AutocompletePopup::new();
	popup.set_provider(Box::new(provider));
	popup.set_context(ctx("/"));
	popup.update();
	assert_eq!(popup.suggestions().len(), 3);
	assert_eq!(popup.selected_index(), 0);

	popup.handle_input(&Key::Up);
	assert_eq!(popup.selected_index(), 2, "selection wraps");
	popup.handle_input(&Key::Down);
	assert_eq!(popup.selected_index(), 0);

	let accepted = Arc::new(Mutex::new(None));
	let sink = Arc::clone(&accepted);
	popup.set_on_accept(Box::new(move |_item, applied: &maple_tui::autocomplete::AppliedText| {
		*sink.lock().unwrap() = Some(applied.lines.clone());
	}));
	popup.handle_input(&Key::Tab);
	assert_eq!(
		accepted.lock().unwrap().as_deref(),
		Some(&["/alpha ".to_string()][..])
	);
}

#[test]
fn popup_escape_cancels_without_accepting() {
	let provider = SlashCommandProvider::new(vec![command("alpha")]);
	let mut popup = AutocompletePopup::new();
	popup.set_provider(Box::new(provider));
	popup.set_context(ctx("/"));
	popup.update();

	let cancelled = Arc::new(Mutex::new(false));
	let sink = Arc::clone(&cancelled);
	popup.set_on_cancel(Box::new(move || *sink.lock().unwrap() = true));
	let accepted = Arc::new(Mutex::new(false));
	let sink = Arc::clone(&accepted);
	popup.set_on_accept(Box::new(move |_item, _applied| *sink.lock().unwrap() = true));

	popup.handle_input(&Key::Esc);
	assert!(*cancelled.lock().unwrap());
	assert!(!*accepted.lock().unwrap());
}

// ── Not portable: the crate has no async Editor scheduling ──────────────────

#[test]
#[ignore = "missing facility: the crate's providers are synchronous; AbortSignal, partial suggestions, and popup lifecycle events have no counterpart"]
fn async_scheduling_and_popup_lifecycle() {
	let provider = SlashCommandProvider::new(vec![command("alpha")]);
	let suggestions = provider.get_suggestions(&ctx("/")).expect("sync suggestions");
	assert_eq!(first_label(&suggestions), "alpha");
}

#[test]
#[ignore = "missing facility: the crate's base Editor has no autocomplete popup, so cursor-leaves-token closure cannot be observed"]
fn popup_closes_when_the_cursor_leaves_the_trigger_token() {
	let provider = SlashCommandProvider::new(vec![command("alpha")]);
	assert!(provider.get_suggestions(&ctx("nice :D")).is_none());
}

#[test]
#[ignore = "missing facility: SlashCommand has no per-command getArgumentCompletions hook (only the static argument_hint)"]
fn slash_argument_completions() {
	let provider = CombinedAutocompleteProvider::new(vec![command("probe")], "/tmp", None);
	assert!(provider.get_suggestions(&ctx("/probe a")).is_none());
}

#[test]
#[ignore = "missing facility: the crate's base Editor does not route Enter through the autocomplete provider, so submit-vs-accept cannot be observed"]
fn enter_submits_or_accepts_through_the_editor() {
	let provider = SlashCommandProvider::new(vec![command("model")]);
	let suggestions = provider.try_sync_slash_completion("/mod").expect("sync suggestions");
	assert_eq!(first_label(&suggestions), "model");
}

#[test]
#[ignore = "missing facility: the crate's base Editor has no autocomplete invalidation pass keyed to destructive edits (issue #4295)"]
fn invalidation_on_destructive_edits() {
	let provider = SlashCommandProvider::new(vec![command("todo")]);
	assert!(provider.get_suggestions(&ctx("/todo sub")).is_none());
}

#[test]
#[ignore = "missing facility: the crate's providers do not chain a directory argument completion after an accepted slash command"]
fn slash_directory_argument_does_not_chain() {
	let provider = FileAutocompleteProvider::new("/tmp");
	assert!(provider.get_force_file_suggestions(&ctx("/tmp")).is_none());
}
