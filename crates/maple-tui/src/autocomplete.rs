//! Autocomplete providers and the suggestion popup.
//!
//! Ports `l/src/autocomplete.ts`. A provider turns the editor buffer plus the
//! cursor position into ranked [`AutocompleteItem`]s and applies the selected
//! item back into the buffer. Two built-in providers are provided:
//!
//! - [`FileAutocompleteProvider`] lists paths under a base directory (the
//!   project cwd by default), for `@` file tokens and explicit path prefixes.
//! - [`SlashCommandProvider`] completes slash-command names from a static
//!   registry ([`builtin_slash_commands`]).
//! - [`CombinedAutocompleteProvider`] dispatches between them.
//!
//! Divergences from the TypeScript original, kept deliberately small:
//! - Providers are synchronous. The TypeScript provider is async because it
//!   shells out to a native fuzzy walker and the filesystem; the Rust port uses
//!   `std::fs` directly, so a host should call it off the render thread.
//! - Recursive fuzzy file discovery (`fuzzyFind`) is not ported; `@` tokens use
//!   the deterministic immediate-directory prefix listing. This is the
//!   documented gap.
//! - The `skill:` namespace collapsing and mid-prompt skill lookup are not
//!   ported (there is no skill registry here); a mid-prompt `/token` falls
//!   through to path completion, as it does for a non-skill token.
//! - `getAutocompleteDescription` live descriptions are not ported; static
//!   descriptions are used.
//! - No module-level fuzzy-search index cache (that lives in [`crate::fuzzy`]).

use crate::component::Component;
use crate::keys::{
    is_arrow_down, is_arrow_up, is_end, is_enter, is_escape, is_home, is_page_down, is_page_up,
    is_shift_tab, is_tab, Key,
};
use crate::style::{Color, Style};
use crate::text_shape::{pad_to_width, truncate_to_width, Ellipsis};
use std::path::PathBuf;

/// Characters that end the token before a path or `@` file reference.
const PATH_DELIMITERS: &[char] = &[' ', '\t', '"', '\'', '='];
/// Default number of suggestion rows visible without scrolling.
const DEFAULT_MAX_VISIBLE: usize = 8;
/// Glyph drawn before the selected row.
const DEFAULT_CURSOR_GLYPH: &str = ">";
/// Gap between a label and its inline description or state.
const LABEL_DESCRIPTION_GAP: usize = 2;

/// One completion candidate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutocompleteItem {
    /// Text inserted into the buffer when accepted.
    pub value: String,
    /// Primary text rendered in the popup.
    pub label: String,
    /// Secondary text rendered after the label.
    pub description: Option<String>,
    /// Type-indicator glyph rendered before the label.
    pub icon: Option<String>,
    /// Named icon for terminals that resolve glyphs by name (`folder`, `file`).
    pub icon_name: Option<String>,
    /// Native detail when it differs from `description`.
    pub native_detail: Option<String>,
    /// Live state drawn after the description.
    pub state: Option<String>,
    /// Dim hint text shown while this item is selected.
    pub hint: Option<String>,
    /// Matched only through the description, not the name or an alias.
    pub description_match: bool,
}

impl AutocompleteItem {
    /// Build an item with a value and label.
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        AutocompleteItem {
            value: value.into(),
            label: label.into(),
            ..AutocompleteItem::default()
        }
    }

    /// Set the inline description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the type-indicator glyph.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set the named icon.
    pub fn with_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = Some(icon_name.into());
        self
    }

    /// Set the right-aligned live state.
    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }
}

/// A slash command the provider can complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashCommand {
    /// Command name without the leading slash.
    pub name: String,
    /// Alternate names that accept the command.
    pub aliases: Vec<String>,
    /// Static description shown in the popup.
    pub description: Option<String>,
    /// Type-indicator glyph shown before the command name.
    pub icon: Option<String>,
    /// Named icon drawn instead of `icon`.
    pub icon_name: Option<String>,
    /// Hint shown after an accepted command that takes arguments.
    pub argument_hint: Option<String>,
    /// Whether the command consumes argument text after its name.
    pub allow_args: bool,
}

impl SlashCommand {
    /// Build a command that accepts arguments.
    pub fn new(name: impl Into<String>) -> Self {
        SlashCommand {
            name: name.into(),
            aliases: Vec::new(),
            description: None,
            icon: None,
            icon_name: None,
            argument_hint: None,
            allow_args: true,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add an alias.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Set the argument hint.
    pub fn with_argument_hint(mut self, hint: impl Into<String>) -> Self {
        self.argument_hint = Some(hint.into());
        self
    }

    /// Set whether the command consumes arguments.
    pub fn with_allow_args(mut self, allow_args: bool) -> Self {
        self.allow_args = allow_args;
        self
    }
}

impl Default for SlashCommand {
    fn default() -> Self {
        SlashCommand::new("")
    }
}

/// The built-in slash-command registry.
pub fn builtin_slash_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand::new("help").with_description("Show available commands"),
        SlashCommand::new("clear").with_description("Clear the conversation"),
        SlashCommand::new("compact").with_description("Compact the conversation"),
        SlashCommand::new("config").with_description("Open settings"),
        SlashCommand::new("cost").with_description("Show session cost"),
        SlashCommand::new("diff").with_description("Show the pending diff"),
        SlashCommand::new("doctor").with_description("Check the installation"),
        SlashCommand::new("export").with_description("Export the conversation"),
        SlashCommand::new("init").with_description("Create an AGENTS file"),
        SlashCommand::new("login").with_description("Sign in"),
        SlashCommand::new("logout").with_description("Sign out"),
        SlashCommand::new("mcp").with_description("Manage MCP servers"),
        SlashCommand::new("memory").with_description("Edit memory files"),
        SlashCommand::new("model").with_description("Switch the model"),
        SlashCommand::new("permissions").with_description("Edit tool permissions"),
        SlashCommand::new("quit").with_description("Exit the session"),
        SlashCommand::new("resume").with_description("Resume a session"),
        SlashCommand::new("review").with_description("Review the last change"),
        SlashCommand::new("status").with_description("Show session status"),
        SlashCommand::new("theme").with_description("Switch the theme"),
        SlashCommand::new("tools").with_description("Manage tools"),
    ]
}

/// Editor buffer state a provider sees.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutocompleteContext {
    /// Full buffer lines.
    pub lines: Vec<String>,
    /// Line holding the cursor.
    pub cursor_line: usize,
    /// Cursor column as a char index into the current line.
    pub cursor_col: usize,
}

impl AutocompleteContext {
    /// Build a context.
    pub fn new(lines: Vec<String>, cursor_line: usize, cursor_col: usize) -> Self {
        AutocompleteContext {
            lines,
            cursor_line,
            cursor_col,
        }
    }

    /// The line holding the cursor, or an empty string.
    pub fn current_line(&self) -> &str {
        self.lines.get(self.cursor_line).map_or("", |line| line.as_str())
    }

    /// Text from the start of the current line to the cursor.
    pub fn text_before_cursor(&self) -> String {
        self.current_line()
            .chars()
            .take(self.cursor_col)
            .collect()
    }

    /// Text from the cursor to the end of the current line.
    pub fn text_after_cursor(&self) -> String {
        self.current_line()
            .chars()
            .skip(self.cursor_col)
            .collect()
    }
}

/// A ranked suggestion list plus the prefix it was matched against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Suggestions {
    /// Ranked candidates, best first.
    pub items: Vec<AutocompleteItem>,
    /// Text the editor replaces on acceptance.
    pub prefix: String,
}

/// The buffer after a completion was applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedText {
    /// Full buffer lines.
    pub lines: Vec<String>,
    /// Line holding the cursor.
    pub cursor_line: usize,
    /// Cursor column as a char index.
    pub cursor_col: usize,
}

/// Turns editor state into suggestions and applies the chosen item.
pub trait AutocompleteProvider {
    /// Suggestions for the current cursor position, or `None` when nothing
    /// applies.
    fn get_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions>;

    /// Apply `item` to the buffer, returning the new text and cursor.
    fn apply_completion(
        &self,
        ctx: &AutocompleteContext,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> AppliedText;

    /// Dim ghost text to draw after the cursor, if any.
    fn inline_hint(&self, _ctx: &AutocompleteContext) -> Option<String> {
        None
    }

    /// Synchronous command-name completion for a leading slash token.
    fn try_sync_slash_completion(&self, _text_before_cursor: &str) -> Option<Suggestions> {
        None
    }

    /// File suggestions for an explicit Tab press.
    fn get_force_file_suggestions(&self, _ctx: &AutocompleteContext) -> Option<Suggestions> {
        None
    }

    /// Whether a Tab press should attempt file completion.
    fn should_trigger_file_completion(&self, _ctx: &AutocompleteContext) -> bool {
        false
    }
}

/// Locate the slash that opens a slash command on the line, allowing leading
/// whitespace. Returns the index of the `/`, or `None` when the line is not a
/// slash command.
pub fn find_leading_slash_command_start(text: &str) -> Option<usize> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('/') {
        return None;
    }
    Some(text.chars().count() - trimmed.chars().count())
}

/// Locate the trailing slash-command token (`prose … /tok`).
pub fn find_trailing_slash_command_start(text: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    for i in (0..chars.len()).rev() {
        if chars[i] != '/' {
            continue;
        }
        if i > 0 && !chars[i - 1].is_whitespace() {
            continue;
        }
        if chars[i + 1..]
            .iter()
            .any(|c| c.is_whitespace() || *c == '/')
        {
            continue;
        }
        return Some(i);
    }
    None
}

/// Whether any earlier line, or the text before `slash_start`, is non-blank.
fn has_prompt_text_before_slash(
    lines: &[String],
    cursor_line: usize,
    text_before_cursor: &str,
    slash_start: usize,
) -> bool {
    if lines
        .iter()
        .take(cursor_line)
        .any(|line| !line.trim().is_empty())
    {
        return true;
    }
    !text_before_cursor
        .chars()
        .take(slash_start)
        .collect::<String>()
        .trim()
        .is_empty()
}

/// Whether index `i` is the first char of a whitespace-delimited token.
fn is_token_start(chars: &[char], index: usize) -> bool {
    index == 0 || PATH_DELIMITERS.contains(&chars[index - 1])
}

/// Index of the last path delimiter, scanning backwards.
fn find_last_delimiter(chars: &[char]) -> Option<usize> {
    chars.iter().rposition(|c| PATH_DELIMITERS.contains(c))
}

/// Offset of the opening quote of an unclosed `"…` token, if any.
fn find_unclosed_quote_start(chars: &[char]) -> Option<usize> {
    let mut in_quotes = false;
    let mut quote_start = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' {
            in_quotes = !in_quotes;
            if in_quotes {
                quote_start = i;
            }
        }
    }
    in_quotes.then_some(quote_start)
}

/// Text from the unclosed quote (or an `@` immediately before it) to the end.
fn extract_quoted_prefix(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let quote_start = find_unclosed_quote_start(&chars)?;
    if quote_start > 0 && chars[quote_start - 1] == '@' {
        if !is_token_start(&chars, quote_start - 1) {
            return None;
        }
        return Some(chars[quote_start - 1..].iter().collect());
    }
    if !is_token_start(&chars, quote_start) {
        return None;
    }
    Some(chars[quote_start..].iter().collect())
}

/// Extract the live whitespace-delimited or double-quoted `@` file token ending
/// at the cursor.
pub fn extract_at_file_prefix(text: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_prefix(text) {
        if quoted.starts_with("@\"") {
            return Some(quoted);
        }
    }

    let chars: Vec<char> = text.chars().collect();
    let token_start = find_last_delimiter(&chars).map_or(0, |index| index + 1);
    if chars.get(token_start) == Some(&'@') {
        return Some(chars[token_start..].iter().collect());
    }
    None
}

/// A path token split into the text to complete and its decoration flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedPathPrefix {
    /// Text to complete, without `@` or quotes.
    pub raw_prefix: String,
    /// Whether the token started with `@`.
    pub is_at_prefix: bool,
    /// Whether the token started with `"`.
    pub is_quoted_prefix: bool,
}

/// Split a path token into its raw prefix and decoration flags.
pub fn parse_path_prefix(prefix: &str) -> ParsedPathPrefix {
    if let Some(rest) = prefix.strip_prefix("@\"") {
        ParsedPathPrefix {
            raw_prefix: rest.to_string(),
            is_at_prefix: true,
            is_quoted_prefix: true,
        }
    } else if let Some(rest) = prefix.strip_prefix('"') {
        ParsedPathPrefix {
            raw_prefix: rest.to_string(),
            is_at_prefix: false,
            is_quoted_prefix: true,
        }
    } else if let Some(rest) = prefix.strip_prefix('@') {
        ParsedPathPrefix {
            raw_prefix: rest.to_string(),
            is_at_prefix: true,
            is_quoted_prefix: false,
        }
    } else {
        ParsedPathPrefix {
            raw_prefix: prefix.to_string(),
            is_at_prefix: false,
            is_quoted_prefix: false,
        }
    }
}

/// Whether a completion value represents a directory: trailing slash or
/// backslash, optionally followed by a closing quote.
pub fn is_directory_completion_value(value: &str) -> bool {
    let trimmed = value
        .strip_suffix('"')
        .or_else(|| value.strip_suffix('\''))
        .unwrap_or(value);
    trimmed.ends_with('/') || trimmed.ends_with('\\')
}

/// Build the text inserted for a completed path.
pub fn build_completion_value(
    path: &str,
    is_directory: bool,
    is_at_prefix: bool,
    is_quoted_prefix: bool,
) -> String {
    let needs_quotes = is_quoted_prefix || path.contains(' ');
    let prefix = if is_at_prefix { "@" } else { "" };

    if !needs_quotes {
        return format!("{prefix}{path}");
    }

    let open_quote = format!("{prefix}\"");
    let close_quote = if is_directory { "" } else { "\"" };
    format!("{open_quote}{path}{close_quote}")
}

/// Case-sensitive subsequence test; callers pass lowercased text.
pub fn subsequence_match(query: &str, target: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let query_chars: Vec<char> = query.chars().collect();
    if query_chars.len() > target.chars().count() {
        return false;
    }
    let mut query_index = 0usize;
    for c in target.chars() {
        if query_index >= query_chars.len() {
            break;
        }
        if query_chars[query_index] == c {
            query_index += 1;
        }
    }
    query_index == query_chars.len()
}

/// Ranked-tier subsequence score (100/80/60/40−gaps·5); higher is better, 0 is
/// no match.
pub fn subsequence_score(query: &str, target: &str) -> i64 {
    if query.is_empty() {
        return 1;
    }
    if target == query {
        return 100;
    }
    if target.starts_with(query) {
        return 80;
    }
    if target.contains(query) {
        return 60;
    }

    let query_chars: Vec<char> = query.chars().collect();
    let mut query_index = 0usize;
    let mut gaps = 0i64;
    let mut last_match_index: Option<usize> = None;
    for (ti, c) in target.chars().enumerate() {
        if query_index >= query_chars.len() {
            break;
        }
        if query_chars[query_index] == c {
            if let Some(last) = last_match_index {
                if ti - last > 1 {
                    gaps += 1;
                }
            }
            last_match_index = Some(ti);
            query_index += 1;
        }
    }
    if query_index != query_chars.len() {
        return 0;
    }
    (40 - gaps * 5).max(1)
}

/// Whether a file completion value still fits the live `@` token.
pub fn at_completion_matches(token: &str, value: &str) -> bool {
    let query = parse_path_prefix(token).raw_prefix.replace('\\', "/").to_lowercase();
    let target_raw = parse_path_prefix(value).raw_prefix;
    let target = target_raw
        .strip_suffix('"')
        .unwrap_or(target_raw.as_str())
        .to_lowercase();
    subsequence_match(&query, &target)
}

/// Exact/leading-prefix command score; higher is better, 0 is no match.
pub fn score_command_text_match(lower_prefix: &str, lower_target: &str) -> i64 {
    if lower_prefix.is_empty() {
        return 1;
    }
    if lower_prefix == lower_target {
        return 1000;
    }
    if lower_target.starts_with(lower_prefix) {
        return 900;
    }
    if subsequence_match(lower_prefix, lower_target) {
        subsequence_score(lower_prefix, lower_target)
    } else {
        0
    }
}

/// Whether `name` is the command's name or one of its aliases.
pub fn command_matches_name_or_alias(command: &SlashCommand, name: &str) -> bool {
    command.name == name || command.aliases.iter().any(|alias| alias == name)
}

/// Extract a path-like prefix from the text before the cursor. `force`
/// (Tab) returns the token even when it is not unambiguous path syntax.
pub fn extract_path_prefix(text: &str, force: bool) -> Option<String> {
    if let Some(quoted) = extract_quoted_prefix(text) {
        return Some(quoted);
    }

    let chars: Vec<char> = text.chars().collect();
    let prefix: String = match find_last_delimiter(&chars) {
        Some(index) => chars[index + 1..].iter().collect(),
        None => text.to_string(),
    };

    if force {
        return Some(prefix);
    }
    if prefix.starts_with('/')
        || prefix.starts_with("./")
        || prefix.starts_with("../")
        || prefix.starts_with("~/")
    {
        return Some(prefix);
    }
    let bytes = prefix.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\') {
        return Some(prefix);
    }
    None
}

/// Parent directory of a `/`-separated path, Node `path.posix.dirname` style.
fn posix_dirname(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(index) => path[..index].to_string(),
        None => ".".to_string(),
    }
}

fn is_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/') || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

/// A scored command candidate used while ranking.
struct ScoredCommand {
    item: AutocompleteItem,
    score: f64,
    usage: usize,
}

/// Rank commands against a lowercased prefix, best first. Equal scores fall
/// back to usage count, then to registry order (the sort is stable).
pub fn build_slash_command_completions(
    commands: &[SlashCommand],
    lower_prefix: &str,
    command_usage: Option<&dyn Fn(&str) -> usize>,
) -> Vec<AutocompleteItem> {
    let mut scored: Vec<ScoredCommand> = Vec::new();

    for command in commands {
        let name = &command.name;
        let usage = command_usage.map_or(0, |count| count(name));
        let hint = command.argument_hint.as_deref();
        let static_desc = command.description.as_deref().unwrap_or("");
        let full_desc = match (hint, static_desc) {
            (Some(hint), desc) if !desc.is_empty() => format!("{hint} - {desc}"),
            (Some(hint), _) => hint.to_string(),
            (None, desc) if !desc.is_empty() => desc.to_string(),
            (None, _) => String::new(),
        };
        let name_score = score_command_text_match(lower_prefix, &name.to_lowercase()) as f64;
        let lower_desc = static_desc.to_lowercase();
        let desc_score = if !lower_desc.is_empty() && subsequence_match(lower_prefix, &lower_desc) {
            subsequence_score(lower_prefix, &lower_desc) as f64 * 0.5
        } else {
            0.0
        };
        let primary_score = name_score.max(desc_score);

        let mut best: Option<ScoredCommand> = None;
        if primary_score > 0.0 {
            best = Some(ScoredCommand {
                item: AutocompleteItem {
                    value: name.clone(),
                    label: name.clone(),
                    description: if full_desc.is_empty() {
                        None
                    } else {
                        Some(full_desc.clone())
                    },
                    icon: command.icon.clone(),
                    icon_name: command.icon_name.clone(),
                    hint: hint.map(str::to_string),
                    description_match: name_score == 0.0,
                    ..AutocompleteItem::default()
                },
                score: primary_score,
                usage,
            });
        }

        if !lower_prefix.is_empty() {
            for alias in &command.aliases {
                if alias == name {
                    continue;
                }
                let alias_score =
                    score_command_text_match(lower_prefix, &alias.to_lowercase()) as f64;
                if alias_score == 0.0 {
                    continue;
                }
                if best
                    .as_ref()
                    .is_some_and(|current| alias_score <= current.score)
                {
                    continue;
                }
                best = Some(ScoredCommand {
                    item: AutocompleteItem {
                        value: alias.clone(),
                        label: alias.clone(),
                        description: if full_desc.is_empty() {
                            None
                        } else {
                            Some(full_desc.clone())
                        },
                        icon: command.icon.clone(),
                        icon_name: command.icon_name.clone(),
                        hint: hint.map(str::to_string),
                        description_match: false,
                        ..AutocompleteItem::default()
                    },
                    score: alias_score,
                    usage,
                });
            }
        }

        if let Some(best) = best {
            scored.push(best);
        }
    }

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.usage.cmp(&a.usage))
    });
    scored.into_iter().map(|entry| entry.item).collect()
}

/// Slash-command completion over a static registry.
pub struct SlashCommandProvider {
    commands: Vec<SlashCommand>,
    command_usage: Option<Box<dyn Fn(&str) -> usize>>,
}

impl SlashCommandProvider {
    /// Build a provider over `commands`.
    pub fn new(commands: Vec<SlashCommand>) -> Self {
        SlashCommandProvider {
            commands,
            command_usage: None,
        }
    }

    /// Build a provider over [`builtin_slash_commands`].
    pub fn builtin() -> Self {
        SlashCommandProvider::new(builtin_slash_commands())
    }

    /// Rank equal matches by per-command usage count.
    pub fn set_command_usage(&mut self, usage: Box<dyn Fn(&str) -> usize>) {
        self.command_usage = Some(usage);
    }

    /// The commands this provider completes.
    pub fn commands(&self) -> &[SlashCommand] {
        &self.commands
    }

    fn usage(&self) -> Option<&dyn Fn(&str) -> usize> {
        self.command_usage.as_deref()
    }
}

impl Default for SlashCommandProvider {
    fn default() -> Self {
        SlashCommandProvider::builtin()
    }
}

impl AutocompleteProvider for SlashCommandProvider {
    fn get_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions> {
        slash_command_suggestions(&self.commands, ctx, self.usage())
    }

    fn apply_completion(
        &self,
        ctx: &AutocompleteContext,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> AppliedText {
        apply_command_completion(&self.commands, ctx, item, prefix)
    }

    fn inline_hint(&self, _ctx: &AutocompleteContext) -> Option<String> {
        // Live per-command argument hints (`getInlineHint`) are not ported; the
        // static `argument_hint` is already shown in the suggestion row.
        None
    }

    fn try_sync_slash_completion(&self, text_before_cursor: &str) -> Option<Suggestions> {
        let slash_start = find_leading_slash_command_start(text_before_cursor)?;
        let command_text: String = text_before_cursor.chars().skip(slash_start).collect();
        if command_text.contains(' ') {
            return None;
        }
        let lower_prefix: String = command_text.chars().skip(1).collect();
        let lower_prefix = lower_prefix.to_lowercase();
        let items = build_slash_command_completions(
            &self.commands,
            &lower_prefix,
            self.usage(),
        );
        if items.is_empty() {
            return None;
        }
        Some(Suggestions {
            items,
            prefix: text_before_cursor.to_string(),
        })
    }
}

/// Slash-command suggestions for a buffer, shared by the provider and the
/// combined provider.
fn slash_command_suggestions(
    commands: &[SlashCommand],
    ctx: &AutocompleteContext,
    command_usage: Option<&dyn Fn(&str) -> usize>,
) -> Option<Suggestions> {
    let text_before = ctx.text_before_cursor();
    let leading_slash_start = find_leading_slash_command_start(&text_before);
    let trailing_slash_start = find_trailing_slash_command_start(&text_before);
    let has_prompt_text_before_trailing = trailing_slash_start.is_some_and(|start| {
        has_prompt_text_before_slash(&ctx.lines, ctx.cursor_line, &text_before, start)
    });
    let has_prompt_text_before_leading = leading_slash_start.is_some_and(|start| {
        has_prompt_text_before_slash(&ctx.lines, ctx.cursor_line, &text_before, start)
    });
    let slash_start = if has_prompt_text_before_trailing {
        trailing_slash_start
    } else if has_prompt_text_before_leading {
        None
    } else {
        leading_slash_start
    }?;

    let command_text: String = text_before.chars().skip(slash_start).collect();
    let is_mid_prompt = has_prompt_text_before_trailing;
    let space_index = command_text.find(' ');

    if space_index.is_none() && !is_mid_prompt {
        let lower_prefix: String = command_text.chars().skip(1).collect();
        let lower_prefix = lower_prefix.to_lowercase();
        let items = build_slash_command_completions(commands, &lower_prefix, command_usage);
        if !items.is_empty() {
            return Some(Suggestions {
                items,
                prefix: if is_mid_prompt {
                    command_text
                } else {
                    text_before
                },
            });
        }
        let after_slash: String = command_text.chars().skip(1).collect();
        if !is_mid_prompt && leading_slash_start == Some(slash_start) && !after_slash.contains('/') {
            return None;
        }
    } else if space_index.is_some() && !is_mid_prompt {
        let space_index = space_index.expect("checked above");
        let command_name: String = command_text.chars().skip(1).take(space_index - 1).collect();
        let argument_text: String = command_text.chars().skip(space_index + 1).collect();
        let command = commands
            .iter()
            .find(|command| command_matches_name_or_alias(command, &command_name));
        if let Some(command) = command {
            if !command.allow_args && argument_text.trim().is_empty() {
                return None;
            }
        }
    }

    None
}

/// Apply a slash-command item, inserting `/name ` at the leading slash.
fn apply_command_completion(
    commands: &[SlashCommand],
    ctx: &AutocompleteContext,
    item: &AutocompleteItem,
    prefix: &str,
) -> AppliedText {
    let line = ctx.current_line();
    let text_before = ctx.text_before_cursor();
    let after_cursor = ctx.text_after_cursor();

    let is_path_completion_item = item.value.starts_with('/') || item.value.starts_with('"');
    if find_leading_slash_command_start(prefix).is_some() {
        if let Some(line_slash) = find_leading_slash_command_start(&text_before) {
            if !is_path_completion_item {
                let slash_prefix: String = text_before.chars().skip(line_slash).collect();
                let is_known_command = commands
                    .iter()
                    .any(|command| command_matches_name_or_alias(command, &item.value));
                if !slash_prefix.contains(' ')
                    && (is_known_command || !slash_prefix.chars().skip(1).any(|c| c == '/'))
                {
                    let before_slash: String = line.chars().take(line_slash).collect();
                    let insert = format!("/{} ", item.value);
                    let new_line = format!("{before_slash}{insert}{after_cursor}");
                    let cursor_col = before_slash.chars().count() + insert.chars().count();
                    let mut lines = ctx.lines.clone();
                    lines[ctx.cursor_line] = new_line;
                    return AppliedText {
                        lines,
                        cursor_line: ctx.cursor_line,
                        cursor_col,
                    };
                }
            }
        }
    }

    let cut = ctx
        .cursor_col
        .saturating_sub(prefix.chars().count());
    let before_prefix: String = line.chars().take(cut).collect();
    let new_line = format!("{before_prefix}{}{after_cursor}", item.value);
    let mut lines = ctx.lines.clone();
    lines[ctx.cursor_line] = new_line;
    AppliedText {
        cursor_col: before_prefix.chars().count() + item.value.chars().count(),
        lines,
        cursor_line: ctx.cursor_line,
    }
}

/// File-path completion rooted at a base directory.
#[derive(Clone, Debug)]
pub struct FileAutocompleteProvider {
    base_path: PathBuf,
}

impl FileAutocompleteProvider {
    /// Build a provider rooted at `base_path`.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        FileAutocompleteProvider {
            base_path: base_path.into(),
        }
    }

    /// The directory relative paths resolve against.
    pub fn base_path(&self) -> &std::path::Path {
        &self.base_path
    }

    /// Expand a leading `~` to the home directory, preserving a trailing slash.
    fn expand_home(&self, file_path: &str) -> String {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let Some(home) = home else {
            return file_path.to_string();
        };
        let home = home.to_string_lossy().into_owned();
        if let Some(rest) = file_path.strip_prefix("~/") {
            let expanded = format!("{}/{rest}", home.trim_end_matches('/'));
            if file_path.ends_with('/') && !expanded.ends_with('/') {
                return format!("{expanded}/");
            }
            return expanded;
        }
        if file_path == "~" {
            return home;
        }
        file_path.to_string()
    }

    /// List directory entries matching `raw_prefix`, directories first.
    pub fn file_suggestions(
        &self,
        raw_prefix: &str,
        is_at_prefix: bool,
        is_quoted_prefix: bool,
    ) -> Vec<AutocompleteItem> {
        let mut expanded = raw_prefix.replace('\\', "/");
        let pre_expand = expanded.clone();
        if pre_expand.starts_with('~') {
            expanded = self.expand_home(&expanded);
        }

        let is_root_prefix = pre_expand.is_empty()
            || pre_expand == "./"
            || pre_expand == "../"
            || pre_expand == "~"
            || pre_expand == "~/"
            || pre_expand == "/"
            || (is_at_prefix && pre_expand.is_empty());

        let (search_dir, search_prefix): (PathBuf, String) = if is_root_prefix || expanded.ends_with('/') {
            let dir = if pre_expand.starts_with('~') || is_absolute_path(&expanded) {
                PathBuf::from(&expanded)
            } else {
                self.base_path.join(&expanded)
            };
            (dir, String::new())
        } else {
            let dir_part = match expanded.rfind('/') {
                Some(index) => expanded[..index].to_string(),
                None => String::new(),
            };
            let file = match expanded.rfind('/') {
                Some(index) => expanded[index + 1..].to_string(),
                None => expanded.clone(),
            };
            let dir = if pre_expand.starts_with('~') || is_absolute_path(&expanded) {
                if dir_part.is_empty() {
                    if expanded.starts_with('/') {
                        PathBuf::from("/")
                    } else {
                        PathBuf::from(&expanded)
                    }
                } else {
                    PathBuf::from(dir_part)
                }
            } else if dir_part.is_empty() {
                self.base_path.clone()
            } else {
                self.base_path.join(dir_part)
            };
            (dir, file)
        };

        let entries = match std::fs::read_dir(&search_dir) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        let lower_search_prefix = search_prefix.to_lowercase();
        let mut matched: Vec<(String, bool)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || !name.to_lowercase().starts_with(&lower_search_prefix) {
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            let is_directory = if file_type.is_symlink() {
                match entry.path().metadata() {
                    Ok(metadata) => metadata.is_dir(),
                    // A broken symlink, a file deleted between readdir and
                    // stat, or a permission error: drop the entry.
                    Err(_) => continue,
                }
            } else {
                file_type.is_dir()
            };
            matched.push((name, is_directory));
        }

        matched.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });

        let display_prefix = raw_prefix.replace('\\', "/");
        let mut suggestions = Vec::with_capacity(matched.len());
        for (name, is_directory) in matched {
            let relative_path = if display_prefix.ends_with('/') {
                format!("{display_prefix}{name}")
            } else if display_prefix.contains('/') {
                if let Some(home_relative) = display_prefix.strip_prefix("~/") {
                    let dir = posix_dirname(home_relative);
                    if dir == "." {
                        format!("~/{name}")
                    } else {
                        format!("~/{dir}/{name}")
                    }
                } else if is_absolute_path(&display_prefix) {
                    let dir = match display_prefix.rfind('/') {
                        Some(index) => &display_prefix[..index],
                        None => "",
                    };
                    if dir.is_empty() || dir == "/" {
                        format!("/{name}")
                    } else {
                        format!("{dir}/{name}")
                    }
                } else {
                    let dir = posix_dirname(&display_prefix);
                    let joined = if dir == "." {
                        name.clone()
                    } else {
                        format!("{dir}/{name}")
                    };
                    if display_prefix.starts_with("./") && !joined.starts_with("./") {
                        format!("./{joined}")
                    } else {
                        joined
                    }
                }
            } else if display_prefix.starts_with('~') {
                format!("~/{name}")
            } else {
                name.clone()
            };
            let relative_path = relative_path.replace('\\', "/");
            let parent_dir = posix_dirname(&relative_path);
            let path_value = if is_directory {
                format!("{relative_path}/")
            } else {
                relative_path
            };
            let value = build_completion_value(&path_value, is_directory, is_at_prefix, is_quoted_prefix);
            let label = if is_directory {
                format!("{name}/")
            } else {
                name
            };
            suggestions.push(AutocompleteItem {
                value,
                label,
                icon_name: Some(if is_directory { "folder" } else { "file" }.to_string()),
                native_detail: if parent_dir == "." {
                    None
                } else {
                    Some(parent_dir)
                },
                ..AutocompleteItem::default()
            });
        }
        suggestions
    }
}

impl Default for FileAutocompleteProvider {
    fn default() -> Self {
        FileAutocompleteProvider::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

impl AutocompleteProvider for FileAutocompleteProvider {
    fn get_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions> {
        let text_before = ctx.text_before_cursor();

        if let Some(at_prefix) = extract_at_file_prefix(&text_before) {
            let parsed = parse_path_prefix(&at_prefix);
            let items = self.file_suggestions(
                &parsed.raw_prefix,
                parsed.is_at_prefix,
                parsed.is_quoted_prefix,
            );
            if items.is_empty() {
                return None;
            }
            return Some(Suggestions {
                items,
                prefix: at_prefix,
            });
        }

        let path_prefix = extract_path_prefix(&text_before, false)?;
        let parsed = parse_path_prefix(&path_prefix);
        let items = self.file_suggestions(
            &parsed.raw_prefix,
            parsed.is_at_prefix,
            parsed.is_quoted_prefix,
        );
        if items.is_empty() {
            return None;
        }
        Some(Suggestions {
            items,
            prefix: path_prefix,
        })
    }

    fn apply_completion(
        &self,
        ctx: &AutocompleteContext,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> AppliedText {
        apply_file_completion(ctx, item, prefix)
    }

    fn get_force_file_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions> {
        let text_before = ctx.text_before_cursor();
        let trimmed = text_before.trim();
        if trimmed.starts_with('/') && !trimmed.contains(' ') {
            return None;
        }
        let path_prefix = extract_path_prefix(&text_before, true)?;
        let parsed = parse_path_prefix(&path_prefix);
        let items = self.file_suggestions(
            &parsed.raw_prefix,
            parsed.is_at_prefix,
            parsed.is_quoted_prefix,
        );
        if items.is_empty() {
            return None;
        }
        Some(Suggestions {
            items,
            prefix: path_prefix,
        })
    }

    fn should_trigger_file_completion(&self, ctx: &AutocompleteContext) -> bool {
        let text_before = ctx.text_before_cursor();
        let trimmed = text_before.trim();
        !(trimmed.starts_with('/') && !trimmed.contains(' '))
    }
}

/// Apply a file-path item, preserving an `@` token and quoting.
fn apply_file_completion(
    ctx: &AutocompleteContext,
    item: &AutocompleteItem,
    prefix: &str,
) -> AppliedText {
    let line = ctx.current_line();
    let text_before = ctx.text_before_cursor();
    let after_cursor = ctx.text_after_cursor();

    let cut = if prefix.starts_with('@') {
        let live_at_prefix = extract_at_file_prefix(&text_before);
        let live_len = live_at_prefix
            .as_ref()
            .map_or(0, |prefix| prefix.chars().count());
        ctx.cursor_col.saturating_sub(live_len)
    } else {
        ctx.cursor_col.saturating_sub(prefix.chars().count())
    };

    let before_prefix: String = line.chars().take(cut).collect();
    let suffix = if is_directory_completion_value(&item.value) {
        ""
    } else {
        " "
    };
    let new_line = format!("{before_prefix}{}{suffix}{after_cursor}", item.value);
    let cursor_col = before_prefix.chars().count() + item.value.chars().count() + suffix.len();
    let mut lines = ctx.lines.clone();
    lines[ctx.cursor_line] = new_line;
    AppliedText {
        lines,
        cursor_line: ctx.cursor_line,
        cursor_col,
    }
}

/// Provider that dispatches between slash commands, `@` files, and paths.
pub struct CombinedAutocompleteProvider {
    commands: Vec<SlashCommand>,
    files: FileAutocompleteProvider,
    command_usage: Option<Box<dyn Fn(&str) -> usize>>,
}

impl CombinedAutocompleteProvider {
    /// Build a combined provider.
    pub fn new(
        commands: Vec<SlashCommand>,
        base_path: impl Into<PathBuf>,
        command_usage: Option<Box<dyn Fn(&str) -> usize>>,
    ) -> Self {
        CombinedAutocompleteProvider {
            commands,
            files: FileAutocompleteProvider::new(base_path),
            command_usage,
        }
    }

    /// Build with [`builtin_slash_commands`] rooted at the current directory.
    pub fn with_defaults() -> Self {
        CombinedAutocompleteProvider::new(
            builtin_slash_commands(),
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            None,
        )
    }

    /// The slash commands this provider completes.
    pub fn commands(&self) -> &[SlashCommand] {
        &self.commands
    }

    /// The file provider used for path completions.
    pub fn files(&self) -> &FileAutocompleteProvider {
        &self.files
    }

    /// The file provider used for path completions.
    pub fn files_mut(&mut self) -> &mut FileAutocompleteProvider {
        &mut self.files
    }
}

impl Default for CombinedAutocompleteProvider {
    fn default() -> Self {
        CombinedAutocompleteProvider::with_defaults()
    }
}

impl AutocompleteProvider for CombinedAutocompleteProvider {
    fn get_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions> {
        let text_before = ctx.text_before_cursor();

        let leading_slash_start = find_leading_slash_command_start(&text_before);
        let trailing_slash_start = find_trailing_slash_command_start(&text_before);
        let has_prompt_text_before_trailing = trailing_slash_start.is_some_and(|start| {
            has_prompt_text_before_slash(&ctx.lines, ctx.cursor_line, &text_before, start)
        });
        let has_prompt_text_before_leading = leading_slash_start.is_some_and(|start| {
            has_prompt_text_before_slash(&ctx.lines, ctx.cursor_line, &text_before, start)
        });
        let slash_start = if has_prompt_text_before_trailing {
            trailing_slash_start
        } else if has_prompt_text_before_leading {
            None
        } else {
            leading_slash_start
        };

        if let Some(slash_start) = slash_start {
            let command_text: String = text_before.chars().skip(slash_start).collect();
            let is_mid_prompt = has_prompt_text_before_trailing;
            let space_index = command_text.find(' ');

            if space_index.is_none() && !is_mid_prompt {
                let lower_prefix: String = command_text.chars().skip(1).collect();
                let lower_prefix = lower_prefix.to_lowercase();
                let items = build_slash_command_completions(
                    &self.commands,
                    &lower_prefix,
                    self.command_usage.as_deref(),
                );
                if !items.is_empty() {
                    return Some(Suggestions {
                        items,
                        prefix: text_before,
                    });
                }
                let after_slash: String = command_text.chars().skip(1).collect();
                if !is_mid_prompt && leading_slash_start == Some(slash_start) && !after_slash.contains('/') {
                    return None;
                }
            } else if space_index.is_some() && !is_mid_prompt {
                let space_index = space_index.expect("checked above");
                let command_name: String = command_text.chars().skip(1).take(space_index - 1).collect();
                let argument_text: String = command_text.chars().skip(space_index + 1).collect();
                let command = self
                    .commands
                    .iter()
                    .find(|command| command_matches_name_or_alias(command, &command_name));
                if let Some(command) = command {
                    if !command.allow_args && argument_text.trim().is_empty() {
                        return None;
                    }
                }
            }
        }

        if let Some(at_prefix) = extract_at_file_prefix(&text_before) {
            let parsed = parse_path_prefix(&at_prefix);
            let items = self.files.file_suggestions(
                &parsed.raw_prefix,
                parsed.is_at_prefix,
                parsed.is_quoted_prefix,
            );
            if items.is_empty() {
                return None;
            }
            return Some(Suggestions {
                items,
                prefix: at_prefix,
            });
        }

        let path_prefix = extract_path_prefix(&text_before, false)?;
        let parsed = parse_path_prefix(&path_prefix);
        let items = self.files.file_suggestions(
            &parsed.raw_prefix,
            parsed.is_at_prefix,
            parsed.is_quoted_prefix,
        );
        if items.is_empty() {
            return None;
        }
        Some(Suggestions {
            items,
            prefix: path_prefix,
        })
    }

    fn apply_completion(
        &self,
        ctx: &AutocompleteContext,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> AppliedText {
        if prefix.starts_with('@') {
            return apply_file_completion(ctx, item, prefix);
        }
        apply_command_completion(&self.commands, ctx, item, prefix)
    }

    fn inline_hint(&self, _ctx: &AutocompleteContext) -> Option<String> {
        None
    }

    fn try_sync_slash_completion(&self, text_before_cursor: &str) -> Option<Suggestions> {
        let provider = SlashCommandProvider::new(self.commands.clone());
        provider.try_sync_slash_completion(text_before_cursor)
    }

    fn get_force_file_suggestions(&self, ctx: &AutocompleteContext) -> Option<Suggestions> {
        self.files.get_force_file_suggestions(ctx)
    }

    fn should_trigger_file_completion(&self, ctx: &AutocompleteContext) -> bool {
        self.files.should_trigger_file_completion(ctx)
    }
}

/// Theme for [`AutocompletePopup`].
#[derive(Clone)]
pub struct AutocompletePopupTheme {
    /// Selection cursor glyph.
    pub cursor: Style,
    /// Unselected label.
    pub label: Style,
    /// Selected label.
    pub selected: Style,
    /// Band painted across the whole selected row.
    pub selected_row: Style,
    /// Inline description.
    pub description: Style,
    /// Right-aligned live state.
    pub state: Style,
    /// Dim hint text.
    pub hint: Style,
    /// Type-indicator glyph.
    pub icon: Style,
}

impl Default for AutocompletePopupTheme {
    fn default() -> Self {
        AutocompletePopupTheme {
            cursor: Style::fg(Color::White).bold(),
            label: Style::default(),
            selected: Style::fg(Color::White).bold(),
            selected_row: Style::default(),
            description: Style::fg(Color::DarkGray),
            state: Style::fg(Color::DarkGray),
            hint: Style::fg(Color::DarkGray).dim(),
            icon: Style::default(),
        }
    }
}

/// Popup that renders autocomplete suggestions and applies the selection.
pub struct AutocompletePopup {
    provider: Option<Box<dyn AutocompleteProvider>>,
    context: Option<AutocompleteContext>,
    items: Vec<AutocompleteItem>,
    prefix: String,
    selected: usize,
    max_visible: usize,
    wrap_navigation: bool,
    theme: AutocompletePopupTheme,
    on_accept: Option<Box<dyn FnMut(&AutocompleteItem, &AppliedText)>>,
    on_cancel: Option<Box<dyn FnMut()>>,
    on_selection_change: Option<Box<dyn FnMut(usize)>>,
}

impl Default for AutocompletePopup {
    fn default() -> Self {
        AutocompletePopup {
            provider: None,
            context: None,
            items: Vec::new(),
            prefix: String::new(),
            selected: 0,
            max_visible: DEFAULT_MAX_VISIBLE,
            wrap_navigation: true,
            theme: AutocompletePopupTheme::default(),
            on_accept: None,
            on_cancel: None,
            on_selection_change: None,
        }
    }
}

impl AutocompletePopup {
    /// Build an empty popup with the default theme.
    pub fn new() -> Self {
        AutocompletePopup::default()
    }

    /// Replace the provider consulted by [`AutocompletePopup::update`].
    pub fn set_provider(&mut self, provider: Box<dyn AutocompleteProvider>) {
        self.provider = Some(provider);
    }

    /// The current provider, if any.
    pub fn provider(&self) -> Option<&dyn AutocompleteProvider> {
        self.provider.as_deref()
    }

    /// Replace the suggestions and reset the selection to the first row.
    pub fn set_suggestions(&mut self, items: Vec<AutocompleteItem>, prefix: String) {
        self.items = items;
        self.prefix = prefix;
        self.selected = 0;
    }

    /// Current suggestions.
    pub fn suggestions(&self) -> &[AutocompleteItem] {
        &self.items
    }

    /// Prefix the current suggestions were matched against.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Store the buffer state the next update applies to.
    pub fn set_context(&mut self, context: AutocompleteContext) {
        self.context = Some(context);
    }

    /// Stored buffer state.
    pub fn context(&self) -> Option<&AutocompleteContext> {
        self.context.as_ref()
    }

    /// Query the provider with the stored context.
    pub fn update(&mut self) {
        let suggestions = {
            let (Some(provider), Some(context)) = (&self.provider, &self.context) else {
                return;
            };
            provider.get_suggestions(context)
        };
        if let Some(suggestions) = suggestions {
            self.set_suggestions(suggestions.items, suggestions.prefix);
        }
    }

    /// Force file completion at the cursor (Tab).
    pub fn force_file_completion(&mut self) {
        let suggestions = {
            let (Some(provider), Some(context)) = (&self.provider, &self.context) else {
                return;
            };
            provider.get_force_file_suggestions(context)
        };
        if let Some(suggestions) = suggestions {
            self.set_suggestions(suggestions.items, suggestions.prefix);
        }
    }

    /// Index of the selected row.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Number of visible rows without scrolling.
    pub fn max_visible(&self) -> usize {
        self.max_visible
    }

    /// Set the visible row budget.
    pub fn set_max_visible(&mut self, max_visible: usize) {
        self.max_visible = max_visible.max(1);
    }

    /// Whether selection wraps at the ends.
    pub fn wrap_navigation(&self) -> bool {
        self.wrap_navigation
    }

    /// Set whether selection wraps at the ends.
    pub fn set_wrap_navigation(&mut self, wrap: bool) {
        self.wrap_navigation = wrap;
    }

    /// The popup theme.
    pub fn theme(&self) -> &AutocompletePopupTheme {
        &self.theme
    }

    /// Replace the popup theme.
    pub fn set_theme(&mut self, theme: AutocompletePopupTheme) {
        self.theme = theme;
    }

    /// Fire `callback` when a suggestion is accepted.
    pub fn set_on_accept(&mut self, callback: Box<dyn FnMut(&AutocompleteItem, &AppliedText)>) {
        self.on_accept = Some(callback);
    }

    /// Fire `callback` when the popup is cancelled.
    pub fn set_on_cancel(&mut self, callback: Box<dyn FnMut()>) {
        self.on_cancel = Some(callback);
    }

    /// Fire `callback` when the selection moves.
    pub fn set_on_selection_change(&mut self, callback: Box<dyn FnMut(usize)>) {
        self.on_selection_change = Some(callback);
    }

    /// Move the selection by `delta` rows, wrapping or clamping.
    pub fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as isize;
        let mut next = self.selected as isize + delta;
        if self.wrap_navigation {
            next = next.rem_euclid(len);
        } else {
            next = next.clamp(0, len - 1);
        }
        self.select_index(next as usize);
    }

    /// Select an exact row, clamping to range.
    pub fn select_index(&mut self, index: usize) {
        if self.items.is_empty() {
            return;
        }
        let clamped = index.min(self.items.len() - 1);
        if clamped != self.selected {
            self.selected = clamped;
            if let Some(callback) = &mut self.on_selection_change {
                callback(self.selected);
            }
        }
    }

    fn accept(&mut self) {
        let Some(item) = self.items.get(self.selected).cloned() else {
            return;
        };
        let Some(provider) = &self.provider else {
            return;
        };
        let Some(context) = &self.context else {
            return;
        };
        let applied = provider.apply_completion(context, &item, &self.prefix);
        if let Some(callback) = &mut self.on_accept {
            callback(&item, &applied);
        }
    }

    fn cancel(&mut self) {
        if let Some(callback) = &mut self.on_cancel {
            callback();
        }
    }

    fn visible_range(&self) -> std::ops::Range<usize> {
        let len = self.items.len();
        if len == 0 {
            return 0..0;
        }
        let max = self.max_visible.max(1);
        if len <= max {
            return 0..len;
        }
        let half = max / 2;
        let mut start = self.selected.saturating_sub(half);
        if start + max > len {
            start = len - max;
        }
        start..start + max
    }

    fn render_row(&self, index: usize, width: usize) -> String {
        let item = &self.items[index];
        let selected = index == self.selected;

        let cursor = if selected { DEFAULT_CURSOR_GLYPH } else { " " };
        let mut content = self.theme.cursor.paint(cursor);
        content.push(' ');
        if let Some(icon) = &item.icon {
            content.push_str(&self.theme.icon.paint(icon));
            content.push(' ');
        }
        let label_style = if selected {
            self.theme.selected
        } else {
            self.theme.label
        };
        content.push_str(&label_style.paint(&item.label));
        if let Some(description) = &item.description {
            content.push_str(&" ".repeat(LABEL_DESCRIPTION_GAP));
            content.push_str(&self.theme.description.paint(description));
        }
        if let Some(state) = &item.state {
            content.push_str(&" ".repeat(LABEL_DESCRIPTION_GAP));
            content.push_str(&self.theme.state.paint(state));
        }

        let clipped = truncate_to_width(&content, width, Ellipsis::Omit);
        if selected && !self.theme.selected_row.is_default() {
            let padded = pad_to_width(&clipped, width);
            self.theme.selected_row.paint(&padded)
        } else {
            pad_to_width(&clipped, width)
        }
    }
}

impl Component for AutocompletePopup {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 || self.items.is_empty() {
            return Vec::new();
        }
        let width = width as usize;
        self.visible_range()
            .map(|index| self.render_row(index, width))
            .collect()
    }

    fn handle_input(&mut self, key: &Key) {
        if self.items.is_empty() {
            if is_escape(key) {
                self.cancel();
            }
            return;
        }
        if is_arrow_down(key) {
            self.move_selection(1);
        } else if is_arrow_up(key) || is_shift_tab(key) {
            self.move_selection(-1);
        } else if is_tab(key) || is_enter(key) {
            self.accept();
        } else if is_escape(key) {
            self.cancel();
        } else if is_home(key) {
            self.select_index(0);
        } else if is_end(key) {
            self.select_index(self.items.len() - 1);
        } else if is_page_down(key) {
            self.move_selection(self.max_visible as isize);
        } else if is_page_up(key) {
            self.move_selection(-(self.max_visible as isize));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi_scan::strip_ansi;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn ctx(lines: &[&str], cursor_line: usize, cursor_col: usize) -> AutocompleteContext {
        AutocompleteContext::new(
            lines.iter().map(|line| line.to_string()).collect(),
            cursor_line,
            cursor_col,
        )
    }

    #[test]
    fn subsequence_match_and_score_tiers() {
        assert!(subsequence_match("wig", "skill:wig"));
        assert!(!subsequence_match("wgi", "skill:wig"));
        assert_eq!(subsequence_score("", "anything"), 1);
        assert_eq!(subsequence_score("abc", "abc"), 100);
        assert_eq!(subsequence_score("ab", "abc"), 80);
        assert_eq!(subsequence_score("bc", "abc"), 60);
        assert_eq!(subsequence_score("ac", "abc"), 35);
        assert_eq!(subsequence_score("abc", "aXbXc"), 30);
    }

    #[test]
    fn path_token_parsing_and_values() {
        let at = parse_path_prefix("@src/foo");
        assert_eq!(at.raw_prefix, "src/foo");
        assert!(at.is_at_prefix);
        assert!(!at.is_quoted_prefix);

        let quoted = parse_path_prefix("@\"src/foo");
        assert_eq!(quoted.raw_prefix, "src/foo");
        assert!(quoted.is_quoted_prefix);

        assert_eq!(build_completion_value("src/foo", false, true, false), "@src/foo");
        assert_eq!(
            build_completion_value("src dir/foo", false, true, false),
            "@\"src dir/foo\""
        );
        assert_eq!(build_completion_value("src/foo/", true, false, false), "src/foo/");
        assert!(is_directory_completion_value("src/foo/"));
        assert!(is_directory_completion_value("src/foo/\""));
        assert!(!is_directory_completion_value("src/foo"));
    }

    #[test]
    fn at_prefix_extraction() {
        assert_eq!(extract_at_file_prefix("see @src/fo"), Some("@src/fo".to_string()));
        assert_eq!(extract_at_file_prefix("@src/fo"), Some("@src/fo".to_string()));
        assert_eq!(extract_at_file_prefix("@\"src/fo"), Some("@\"src/fo".to_string()));
        assert_eq!(extract_at_file_prefix("no token"), None);
        assert_eq!(extract_at_file_prefix("email a@b"), None);
        assert!(at_completion_matches("@sr", "@src/foo"));
        assert!(!at_completion_matches("@zz", "@src/foo"));
    }

    #[test]
    fn slash_token_detection() {
        assert_eq!(find_leading_slash_command_start("/he"), Some(0));
        assert_eq!(find_leading_slash_command_start("  /he"), Some(2));
        assert_eq!(find_leading_slash_command_start("hello"), None);
        assert_eq!(find_trailing_slash_command_start("prose /he"), Some(6));
        assert_eq!(find_trailing_slash_command_start("/he"), Some(0));
        assert_eq!(find_trailing_slash_command_start("and/or"), None);
    }

    #[test]
    fn command_scoring_tiers() {
        assert_eq!(score_command_text_match("", "help"), 1);
        assert_eq!(score_command_text_match("help", "help"), 1000);
        assert_eq!(score_command_text_match("hel", "help"), 900);
        assert_eq!(score_command_text_match("hlo", "help"), 0);
    }

    #[test]
    fn builtin_commands_complete_from_prefix() {
        let provider = SlashCommandProvider::builtin();
        let suggestions = provider
            .get_suggestions(&ctx(&["/he"], 0, 3))
            .expect("help should match");
        assert_eq!(suggestions.prefix, "/he");
        assert_eq!(suggestions.items[0].value, "help");

        let none = provider.get_suggestions(&ctx(&["/zzz"], 0, 4));
        assert!(none.is_none());
    }

    #[test]
    fn builtin_bare_slash_lists_commands() {
        let provider = SlashCommandProvider::builtin();
        let suggestions = provider
            .get_suggestions(&ctx(&["/"], 0, 1))
            .expect("bare slash lists commands");
        assert!(!suggestions.items.is_empty());
        assert_eq!(suggestions.items[0].value, "help");
    }

    #[test]
    fn command_apply_inserts_name_and_space() {
        let provider = SlashCommandProvider::builtin();
        let context = ctx(&["/he"], 0, 3);
        let item = AutocompleteItem::new("help", "help");
        let applied = provider.apply_completion(&context, &item, "/he");
        assert_eq!(applied.lines, vec!["/help ".to_string()]);
        assert_eq!(applied.cursor_col, 6);
    }

    #[test]
    fn mid_prompt_slash_falls_through_to_paths() {
        let provider = SlashCommandProvider::builtin();
        let context = ctx(&["prose /he"], 0, 9);
        assert!(provider.get_suggestions(&context).is_none());
    }

    #[test]
    fn force_completion_ignores_leading_command() {
        let provider = FileAutocompleteProvider::new("/definitely-not-a-real-maple-dir");
        let context = ctx(&["/he"], 0, 3);
        assert!(!provider.should_trigger_file_completion(&context));
        assert!(provider.get_force_file_suggestions(&context).is_none());
        assert_eq!(
            extract_path_prefix("/definitely-not-a-real-maple-dir", false),
            Some("/definitely-not-a-real-maple-dir".to_string())
        );
        assert_eq!(extract_path_prefix("bare", false), None);
    }

    #[test]
    fn combined_dispatch_prefers_commands() {
        let provider = CombinedAutocompleteProvider::new(
            builtin_slash_commands(),
            "/definitely-not-a-real-maple-dir",
            None,
        );
        let suggestions = provider
            .get_suggestions(&ctx(&["/he"], 0, 3))
            .expect("command match");
        assert_eq!(suggestions.items[0].value, "help");
        assert!(provider.get_suggestions(&ctx(&["/zzz"], 0, 4)).is_none());
    }

    fn popup_items() -> Vec<AutocompleteItem> {
        vec![
            AutocompleteItem::new("help", "help").with_description("Show help"),
            AutocompleteItem::new("clear", "clear").with_description("Clear all"),
        ]
    }

    #[test]
    fn popup_renders_selection_and_rows() {
        let mut popup = AutocompletePopup::new();
        popup.set_suggestions(popup_items(), "/".to_string());
        let rows = popup.render(24);
        assert_eq!(rows.len(), 2);
        assert_eq!(strip_ansi(&rows[0]).trim_end(), "> help  Show help");
        assert_eq!(strip_ansi(&rows[1]).trim_end(), "  clear  Clear all");
    }

    #[test]
    fn popup_rows_fit_width() {
        let mut popup = AutocompletePopup::new();
        popup.set_suggestions(popup_items(), "/".to_string());
        for width in [1u16, 8, 24, 100] {
            for row in popup.render(width) {
                assert!(crate::text_shape::visible_width(&row) <= width as usize);
            }
        }
        assert!(popup.render(0).is_empty());
        assert!(popup.render(24).len() == 2);
    }

    #[test]
    fn popup_arrow_navigation_moves_selection() {
        let mut popup = AutocompletePopup::new();
        popup.set_suggestions(popup_items(), "/".to_string());
        popup.handle_input(&Key::Down);
        assert_eq!(popup.selected_index(), 1);
        popup.handle_input(&Key::Up);
        assert_eq!(popup.selected_index(), 0);
        popup.handle_input(&Key::Up);
        assert_eq!(popup.selected_index(), 1);
    }

    #[test]
    fn popup_clamps_without_wrapping() {
        let mut popup = AutocompletePopup::new();
        popup.set_wrap_navigation(false);
        popup.set_suggestions(popup_items(), "/".to_string());
        popup.handle_input(&Key::Up);
        assert_eq!(popup.selected_index(), 0);
        popup.handle_input(&Key::Down);
        popup.handle_input(&Key::Down);
        assert_eq!(popup.selected_index(), 1);
    }

    #[test]
    fn popup_tab_accepts_through_provider() {
        let mut popup = AutocompletePopup::new();
        popup.set_provider(Box::new(SlashCommandProvider::builtin()));
        popup.set_context(ctx(&["/he"], 0, 3));
        popup.update();
        assert_eq!(popup.suggestions()[0].value, "help");

        let accepted = Rc::new(RefCell::new(None));
        let accepted_ref = Rc::clone(&accepted);
        popup.set_on_accept(Box::new(move |_item, applied| {
            *accepted_ref.borrow_mut() = Some(applied.clone());
        }));
        popup.handle_input(&Key::Tab);
        let applied = accepted.borrow().clone().expect("accept callback");
        assert_eq!(applied.lines, vec!["/help ".to_string()]);
        assert_eq!(applied.cursor_col, 6);
    }

    #[test]
    fn popup_enter_accepts_and_escape_cancels() {
        let mut popup = AutocompletePopup::new();
        popup.set_suggestions(popup_items(), "/".to_string());
        let cancels = Rc::new(RefCell::new(0));
        let cancels_ref = Rc::clone(&cancels);
        popup.set_on_cancel(Box::new(move || *cancels_ref.borrow_mut() += 1));
        popup.handle_input(&Key::Enter);
        assert_eq!(*cancels.borrow(), 0);
        popup.handle_input(&Key::Esc);
        assert_eq!(*cancels.borrow(), 1);
        popup.handle_input(&Key::Esc);
        assert_eq!(*cancels.borrow(), 2);
    }

    #[test]
    fn popup_selection_change_callback_fires() {
        let mut popup = AutocompletePopup::new();
        popup.set_suggestions(popup_items(), "/".to_string());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_ref = Rc::clone(&seen);
        popup.set_on_selection_change(Box::new(move |index| {
            seen_ref.borrow_mut().push(index);
        }));
        popup.handle_input(&Key::Down);
        popup.handle_input(&Key::Down);
        assert_eq!(*seen.borrow(), vec![1, 0]);
    }

    #[test]
    fn popup_hides_when_empty() {
        let mut popup = AutocompletePopup::new();
        assert!(popup.render(20).is_empty());
        popup.handle_input(&Key::Down);
        assert_eq!(popup.selected_index(), 0);
    }
}
