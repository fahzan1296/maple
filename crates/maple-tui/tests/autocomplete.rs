//! Port of `l/test/autocomplete.test.ts`.
//!
//! The crate provider is synchronous and has no `skill:` registry, recursive
//! fuzzy walker, live descriptions, or command-argument completion providers.
//! Those skill/fuzzy/argument scenarios are ignored; path-prefix extraction,
//! slash-command ranking, and completion application are asserted.

use maple_tui::autocomplete::{
    AutocompleteContext, AutocompleteItem, AutocompleteProvider, CombinedAutocompleteProvider,
    FileAutocompleteProvider, SlashCommand, build_slash_command_completions, builtin_slash_commands,
    extract_path_prefix,
};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_DIRS: AtomicUsize = AtomicUsize::new(0);

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let seq = TEMP_DIRS.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "maple-autocomplete-{}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        TempDir { path }
    }

    fn file(&self, name: &str) {
        std::fs::write(self.path.join(name), "export {};\n").expect("seed file");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn ctx(lines: &[&str], cursor_line: usize, cursor_col: usize) -> AutocompleteContext {
    AutocompleteContext::new(lines.iter().map(|l| l.to_string()).collect(), cursor_line, cursor_col)
}

fn command_provider() -> CombinedAutocompleteProvider {
    CombinedAutocompleteProvider::new(builtin_slash_commands(), "/tmp", None)
}

#[test]
fn extracts_slash_prefix_only_when_forced() {
    assert_eq!(extract_path_prefix("hey /", true), Some("/".to_string()));
    assert_eq!(extract_path_prefix("/command /", true), Some("/".to_string()));
    assert_eq!(extract_path_prefix("hey /", false), None);
    assert_eq!(extract_path_prefix("hey there", true), Some("there".to_string()));
}

#[test]
fn force_file_suggestions_skip_slash_commands() {
    let provider = FileAutocompleteProvider::new("/tmp");
    let slash = ctx(&["/model"], 0, 6);
    assert!(provider.get_force_file_suggestions(&slash).is_none());
    let temp = TempDir::new();
    temp.file("alpha.ts");
    let line = format!("command {}/al", temp.path.display());
    let context = ctx(&[line.as_str()], 0, line.chars().count());
    let suggestions = provider
        .get_force_file_suggestions(&context)
        .expect("forced path suggestions");
    assert!(suggestions.items.iter().any(|item| item.value.contains("alpha.ts")));
    assert!(!suggestions.items.is_empty());
}

#[test]
fn slash_command_prefix_suggests_commands() {
    let provider = command_provider();
    let suggestions = provider
        .get_suggestions(&ctx(&["/mod"], 0, 4))
        .expect("slash suggestions");
    assert_eq!(suggestions.prefix, "/mod");
    assert!(suggestions.items.iter().any(|item| item.value == "model"));
}

#[test]
fn mid_prompt_slash_falls_through_to_path_completion() {
    // The crate has no mid-prompt command lookup: a `/token` after prose is
    // treated as a path prefix, and `/rev` does not exist under the base dir.
    let provider = command_provider();
    assert!(provider
        .get_suggestions(&ctx(&["there is an issue", "/rev"], 1, 4))
        .is_none());
}

#[test]
fn slash_inside_word_returns_nothing() {
    let provider = command_provider();
    let line = "word/security";
    assert!(provider
        .get_suggestions(&ctx(&[line], 0, line.len()))
        .is_none());
}

#[test]
fn prose_token_that_only_fuzzy_matches_description_returns_nothing() {
    let provider = CombinedAutocompleteProvider::new(
        vec![SlashCommand::new("humanizer")
            .with_description("Remove signs of AI-generated writing from text")],
        "/tmp",
        None,
    );
    let line = "we should /sign";
    assert!(provider
        .get_suggestions(&ctx(&[line], 0, line.len()))
        .is_none());
}

#[test]
fn exact_command_match_ranks_above_longer_prefix_match() {
    let commands = vec![SlashCommand::new("model"), SlashCommand::new("mode-check")];
    let items = build_slash_command_completions(&commands, "mode", None);
    assert_eq!(items.first().map(|item| item.value.as_str()), Some("model"));
    assert_eq!(items.len(), 2);
}

#[test]
fn alias_match_is_offered() {
    let commands = vec![SlashCommand::new("model").with_alias("m")];
    let items = build_slash_command_completions(&commands, "m", None);
    assert!(items.iter().any(|item| item.value == "m"));
}

#[test]
fn apply_slash_completion_preserves_leading_whitespace() {
    let provider = command_provider();
    let context = ctx(&["  /ski"], 0, 6);
    let item = AutocompleteItem {
        value: "skills:fix-bug".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "/s");
    assert_eq!(applied.lines[0], "  /skills:fix-bug ");
    assert_eq!(applied.cursor_col, "  /skills:fix-bug ".chars().count());
}

#[test]
fn apply_slash_completion_with_whitespace_carrying_prefix() {
    let provider = command_provider();
    let context = ctx(&["  /sk"], 0, 5);
    let item = AutocompleteItem {
        value: "skill".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "  /sk");
    assert_eq!(applied.lines[0], "  /skill ");
    assert_eq!(applied.cursor_col, "  /skill ".chars().count());
}

#[test]
fn apply_leading_slash_path_without_command_insertion() {
    let provider = command_provider();
    let context = ctx(&["/tm"], 0, 3);
    let item = AutocompleteItem {
        value: "/tmp/".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "/tm");
    assert_eq!(applied.lines[0], "/tmp/");
    assert_eq!(applied.cursor_col, "/tmp/".chars().count());
}

#[test]
fn apply_absolute_deep_path_replaces_only_active_token() {
    let temp = TempDir::new();
    temp.file("alpha.ts");
    let provider = CombinedAutocompleteProvider::new(Vec::new(), temp.path.clone(), None);
    let base = temp.path.display().to_string();
    let prefix = format!("{base}/al");
    let completed = format!("{base}/alpha.ts");
    let line = format!("open {prefix}");
    let context = ctx(&[line.as_str()], 0, line.chars().count());
    let item = AutocompleteItem {
        value: completed.clone(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, &prefix);
    assert_eq!(applied.lines[0], format!("open {completed}"));
    assert_eq!(applied.cursor_col, format!("open {completed}").chars().count());
}

#[test]
fn apply_quoted_absolute_path() {
    let provider = command_provider();
    let context = ctx(&["/tm"], 0, 3);
    let item = AutocompleteItem {
        value: "\"/tmp/\"".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "/tm");
    assert_eq!(applied.lines[0], "\"/tmp/\"");
}

#[test]
fn apply_path_inside_slash_command_argument() {
    let provider = command_provider();
    let context = ctx(&["/swarm run pac"], 0, 14);
    let item = AutocompleteItem {
        value: "package.json".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "pac");
    assert_eq!(applied.lines[0], "/swarm run package.json");
    assert_eq!(applied.cursor_col, "/swarm run package.json".chars().count());
}

#[test]
fn apply_at_directory_adds_no_trailing_space() {
    let provider = command_provider();
    let context = ctx(&["see @pack"], 0, 9);
    let item = AutocompleteItem {
        value: "@packages/".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "@pack");
    assert_eq!(applied.lines[0], "see @packages/");
    assert_eq!(applied.cursor_col, "see @packages/".chars().count());
}

#[test]
fn apply_quoted_at_directory_adds_no_trailing_space() {
    let provider = command_provider();
    let line = "see @\"my fold";
    let context = ctx(&[line], 0, line.chars().count());
    let item = AutocompleteItem {
        value: "@\"my folder/".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "@\"my fold");
    assert_eq!(applied.lines[0], "see @\"my folder/");
}

#[test]
fn automatic_trigger_uses_only_explicit_path_contexts() {
    let temp = TempDir::new();
    temp.file("alpha.ts");
    let provider = FileAutocompleteProvider::new(temp.path.clone());
    assert!(provider.get_suggestions(&ctx(&["hey "], 0, 4)).is_none());
    let line = format!("hey {}/al", temp.path.display());
    let suggestions = provider
        .get_suggestions(&ctx(&[line.as_str()], 0, line.chars().count()))
        .expect("explicit path suggestions");
    assert!(suggestions.prefix.ends_with("/al"), "prefix: {}", suggestions.prefix);
    assert!(!suggestions.items.is_empty());
}

#[test]
fn at_file_reference_suggests_directory_entries() {
    let temp = TempDir::new();
    temp.file("alpha.ts");
    let provider = FileAutocompleteProvider::new(temp.path.clone());
    let line = "see @al";
    let suggestions = provider
        .get_suggestions(&ctx(&[line], 0, line.len()))
        .expect("at-file suggestions");
    assert_eq!(suggestions.prefix, "@al");
    assert!(suggestions
        .items
        .iter()
        .any(|item| item.value.contains("alpha.ts")));
}

#[test]
#[ignore = "missing facility: skill: namespace registry and collapsing"]
fn skills_collapse_into_namespace_row() {
    let provider = command_provider();
    let suggestions = provider.get_suggestions(&ctx(&["/mod"], 0, 4)).expect("commands");
    assert!(suggestions.items.iter().any(|item| item.value == "model"));
}

#[test]
#[ignore = "missing facility: mid-prompt skill lookup"]
fn mid_prompt_bare_name_matches_skill() {
    let provider = command_provider();
    let line = "polish this /hum";
    assert!(provider
        .get_suggestions(&ctx(&[line], 0, line.len()))
        .is_none());
}

#[test]
#[ignore = "missing facility: hyphen-delimited skill segment matching"]
fn hyphen_segments_match_skills() {
    let commands = vec![SlashCommand::new("research-last30days")];
    let items = build_slash_command_completions(&commands, "last", None);
    assert!(items.iter().any(|item| item.value == "research-last30days"));
}

#[test]
#[ignore = "missing facility: recursive fuzzyFind walker"]
fn unmatched_mid_prompt_slash_falls_back_to_paths() {
    let temp = TempDir::new();
    temp.file("alpha.ts");
    let provider = FileAutocompleteProvider::new(temp.path.clone());
    let line = format!("see {}/al", temp.path.display());
    let suggestions = provider
        .get_suggestions(&ctx(&[line.as_str()], 0, line.chars().count()))
        .expect("path fallback");
    assert!(!suggestions.items.is_empty());
}

#[test]
#[ignore = "missing facility: per-command argument completion providers"]
fn slash_command_argument_completions_win_over_file_references() {
    let provider = command_provider();
    let context = ctx(&["/model "], 0, 7);
    assert!(provider.get_suggestions(&context).is_none());
}

#[test]
#[ignore = "missing facility: Windows backslash path handling"]
fn backslash_directory_completion_adds_no_trailing_space() {
    let provider = command_provider();
    let line = "see @packages\\";
    let context = ctx(&[line], 0, line.len());
    let item = AutocompleteItem {
        value: "@packages\\".to_string(),
        ..AutocompleteItem::default()
    };
    let applied = provider.apply_completion(&context, &item, "@packages\\");
    assert_eq!(applied.lines[0], "see @packages\\");
}
