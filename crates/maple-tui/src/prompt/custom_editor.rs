//! Reference: `l/src/prompt/custom-editor.ts`.
//!
//! The composer editor: staged draft state (pasted images, videos and large
//! text pastes) on top of the crate's [`Editor`], the composer chip
//! descriptors the attachment band renders, the chip→content atom table
//! consulted on submit, and the paste-path recognition that turns a clipboard
//! or bracketed paste into attachments.
//!
//! Deviations:
//! - No TypeScript class inheritance: [`CustomEditor`] owns an [`Editor`] and
//!   delegates text access. Input dispatch, keybinding decoding and repaint
//!   requests stay with the host.
//! - The native (TSP) composer card is not ported; [`ComposerNativeState`] is
//!   plain data for hosts that render their own controls.
//! - Spelling/autocorrect and word completion are injected by the host
//!   (`set_text_assist_provider`); the reference constructs them itself.
//! - `#composerChipsCache` keeps the reference's revision-drop contract but is
//!   a plain field, not a private one.
//! - Staged-paste expansion wraps content in a fenced block; the reference's
//!   exact wrapper lives in its unconverted atom helper.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::bracketed_paste::{PASTE_END, PASTE_START};
use crate::components::editor::Editor;
use crate::prompt::composer_attachments::{ChipKind, chip_label};
use crate::prompt::image_source::ImageContent;
use crate::prompt::video::is_video_path;
use crate::theme::theme_class::Theme;

/// Live composer state a native layout shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComposerNativeState {
	/// The draft runs as a shell command (`!`, `!!`) or Python (`$`, `$$`);
	/// `excluded` keeps it from the model.
	pub shell: Option<ShellState>,
	/// Thinking effort word (`high`, `off`, `auto`); `None` when the model has
	/// no thinking.
	pub thinking: Option<String>,
	/// The thinking level draws as the model chip's icon instead of its own
	/// chip.
	pub thinking_in_model: bool,
	/// Generation tok/s after the thinking level.
	pub rate: Option<f64>,
	/// A turn is running: the send keycap becomes a Stop button.
	pub running: bool,
	/// Native dictation control; `None` hides it.
	pub dictation: Option<DictationState>,
	/// The viewed subagent's lineage, outermost first and the viewed agent
	/// last; `None` on the main session.
	pub viewing: Option<Vec<String>>,
	/// The session's title, quoted as the native placeholder.
	pub title: Option<String>,
}

/// Shell mode of a draft.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellState {
	pub kind: ShellKind,
	/// The draft does not reach the model.
	pub excluded: bool,
}

/// Which interpreter a shell-mode draft runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
	Bash,
	Python,
}

/// Dictation phase of the native composer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictationState {
	Idle,
	Starting,
	Recording,
	Transcribing,
}

impl DictationState {
	/// Recording and transcription still write into the draft, so sending it
	/// then would split the dictation.
	pub fn holds_draft(self) -> bool {
		matches!(self, DictationState::Recording | DictationState::Transcribing)
	}
}

/// Independently switchable prose-assistance features of the composer.
#[derive(Clone, Debug, PartialEq)]
pub struct SpellingFeatures {
	/// Typo detection (spell marks).
	pub typo_detection: bool,
	/// Autocorrect on word boundaries.
	pub autocorrect: bool,
	/// Word-completion engine; `off` disables ghost text.
	pub autocomplete: WordCompletionMethod,
}

impl Default for SpellingFeatures {
	fn default() -> Self {
		SpellingFeatures {
			typo_detection: true,
			autocorrect: false,
			autocomplete: WordCompletionMethod::Off,
		}
	}
}

/// Word-completion engine selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordCompletionMethod {
	Off,
	/// Completion from words already in the draft.
	Draft,
	/// Completion from a corpus/model source wired by the host.
	Model,
}

/// A large text paste staged as a composer chip. `content` feeds the band
/// card's snippet and captions; the submit-time expansion lives in the
/// editor's atom table under `label`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextAttachment {
	pub n: i64,
	pub label: String,
	pub content: String,
	pub line_count: usize,
	pub char_count: usize,
}

impl TextAttachment {
	pub fn new(n: i64, label: impl Into<String>, content: impl Into<String>) -> Self {
		let content = content.into();
		let line_count = content.lines().count();
		let char_count = content.chars().count();
		TextAttachment {
			n,
			label: label.into(),
			content,
			line_count,
			char_count,
		}
	}
}

/// One visible composer attachment, in band order (vision attachments first,
/// then text pastes).
#[derive(Clone, Debug, PartialEq)]
pub enum ComposerChipDescriptor {
	Image {
		n: i64,
		image: ImageContent,
		link: Option<String>,
	},
	Video {
		n: i64,
		image: ImageContent,
		link: Option<String>,
	},
	Paste { n: i64, text: TextAttachment },
}

impl ComposerChipDescriptor {
	/// Chip kind, matching [`ChipKind`].
	pub fn kind(&self) -> ChipKind {
		match self {
			ComposerChipDescriptor::Image { .. } => ChipKind::Image,
			ComposerChipDescriptor::Video { .. } => ChipKind::Video,
			ComposerChipDescriptor::Paste { .. } => ChipKind::Paste,
		}
	}

	/// Chip number, as shown in the label.
	pub fn n(&self) -> i64 {
		match self {
			ComposerChipDescriptor::Image { n, .. }
			| ComposerChipDescriptor::Video { n, .. }
			| ComposerChipDescriptor::Paste { n, .. } => *n,
		}
	}

	/// Chip label (`[Image #1]`, `[Paste #2]`, …).
	pub fn label(&self, theme: Option<&Theme>) -> String {
		chip_label(self.kind(), self.n(), theme)
	}

	/// The file this chip stands for, when it has one.
	pub fn link(&self) -> Option<&str> {
		match self {
			ComposerChipDescriptor::Image { link, .. } | ComposerChipDescriptor::Video { link, .. } => {
				link.as_deref()
			}
			ComposerChipDescriptor::Paste { .. } => None,
		}
	}
}

/// Chip descriptors recomputed only when the draft changed.
struct ComposerChipsCache {
	text_revision: u64,
	images_revision: u64,
	image_count: usize,
	image_link_count: usize,
	text_count: usize,
	chips: Vec<ComposerChipDescriptor>,
}

/// Host-wired prose assistance, so the editor stays free of OS integration.
pub trait TextAssistProvider {
	/// Ghost-text completion at the cursor.
	fn get_word_completion(
		&self,
		lines: &[String],
		cursor_line: usize,
		cursor_col: usize,
	) -> Option<String>;
	/// Feedback about an accepted or dismissed completion.
	fn word_completion_feedback(
		&mut self,
		lines: &[String],
		cursor_line: usize,
		cursor_col: usize,
		suggestion: &str,
		accepted: bool,
	);
	/// Autocorrect the word before the cursor.
	fn try_autocorrect(&mut self, lines: &[String], cursor_line: usize, cursor_col: usize) -> Option<String>;
	/// Replacement candidates for the word at the cursor.
	fn get_word_replacements(&self, lines: &[String], cursor_line: usize, cursor_col: usize) -> Vec<String>;
}

/// The composer editor: the crate [`Editor`] plus staged draft attachments.
pub struct CustomEditor {
	editor: Editor,
	spelling_features: SpellingFeatures,
	word_completion_method: WordCompletionMethod,
	/// Draft images pasted into the composer, consumed on submit.
	pub pending_images: Vec<ImageContent>,
	/// Per-image source links parallel to [`CustomEditor::pending_images`].
	pub pending_image_links: Vec<Option<String>>,
	/// Large text pastes staged as compact chip tokens.
	pub pending_texts: Vec<TextAttachment>,
	/// Host-provided clickable chip targets for restored drafts.
	pub image_links: Option<Vec<Option<String>>>,
	/// Chip label → submit-time expansion.
	atoms: HashMap<String, String>,
	text_attachment_counter: i64,
	text_revision: u64,
	images_revision: u64,
	composer_chips_cache: Option<ComposerChipsCache>,
	composer_state: ComposerNativeState,
	text_assist: Option<Box<dyn TextAssistProvider>>,
}

impl Default for CustomEditor {
	fn default() -> Self {
		Self::new()
	}
}

impl CustomEditor {
	pub fn new() -> Self {
		CustomEditor {
			editor: Editor::new(),
			spelling_features: SpellingFeatures::default(),
			word_completion_method: WordCompletionMethod::Off,
			pending_images: Vec::new(),
			pending_image_links: Vec::new(),
			pending_texts: Vec::new(),
			image_links: None,
			atoms: HashMap::new(),
			text_attachment_counter: 0,
			text_revision: 0,
			images_revision: 0,
			composer_chips_cache: None,
			composer_state: ComposerNativeState::default(),
			text_assist: None,
		}
	}

	/// The wrapped base editor.
	pub fn editor(&self) -> &Editor {
		&self.editor
	}

	/// The wrapped base editor, for rendering and input dispatch.
	pub fn editor_mut(&mut self) -> &mut Editor {
		&mut self.editor
	}

	pub fn set_text(&mut self, text: impl Into<String>) {
		self.editor.set_text(text);
		self.text_revision += 1;
	}

	pub fn get_text(&self) -> String {
		self.editor.get_text()
	}

	/// Wire the host's spelling/completion engine.
	pub fn set_text_assist_provider(&mut self, provider: Box<dyn TextAssistProvider>) {
		self.text_assist = Some(provider);
	}

	pub fn text_assist(&self) -> Option<&dyn TextAssistProvider> {
		self.text_assist.as_deref()
	}

	pub fn text_assist_mut(&mut self) -> Option<&mut (dyn TextAssistProvider + 'static)> {
		match self.text_assist.as_deref_mut() {
			Some(provider) => Some(provider),
			None => None,
		}
	}

	/// Independently configure typo detection, the word-completion engine and
	/// autocorrect.
	pub fn set_spelling_features(&mut self, features: SpellingFeatures) {
		self.spelling_features = SpellingFeatures {
			typo_detection: features.typo_detection,
			autocorrect: features.autocorrect,
			autocomplete: features.autocomplete,
		};
		self.word_completion_method = features.autocomplete;
	}


	/// Clear the composer draft: reset the text and all pending draft state.
	/// The host records `history_text`, if any, in its history.
	pub fn clear_draft(&mut self) {
		self.editor.set_text("");
		self.text_revision += 1;
		self.clear_paste_state();
		self.image_links = None;
	}

	/// Drop every staged attachment and its atom.
	pub fn clear_paste_state(&mut self) {
		self.pending_images.clear();
		self.pending_image_links.clear();
		self.pending_texts.clear();
		self.atoms.clear();
		self.images_revision += 1;
		self.composer_chips_cache = None;
	}

	/// Stage a pasted image (or video preview).
	pub fn add_pending_image(&mut self, image: ImageContent, link: Option<String>) {
		self.pending_images.push(image);
		self.pending_image_links.push(link);
		self.images_revision += 1;
		self.composer_chips_cache = None;
	}

	/// Stage a large text paste: returns the chip number the token carries.
	pub fn add_pending_text(&mut self, content: impl Into<String>, label: Option<String>) -> i64 {
		self.text_attachment_counter += 1;
		let n = self.text_attachment_counter;
		let label = label.unwrap_or_else(|| chip_label(ChipKind::Paste, n, None));
		let attachment = TextAttachment::new(n, label.clone(), content);
		self.atoms
			.insert(label, format_paste_expansion(&attachment.content));
		self.pending_texts.push(attachment);
		self.images_revision += 1;
		self.composer_chips_cache = None;
		n
	}

	/// Snippet the band card shows for a staged paste.
	pub fn text_snippet(&self, label: &str) -> Option<&str> {
		self.pending_texts
			.iter()
			.find(|text| text.label == label)
			.map(|text| text.content.as_str())
	}

	/// The staged expansion for a chip label.
	pub fn atom(&self, label: &str) -> Option<&str> {
		self.atoms.get(label).map(String::as_str)
	}

	/// Replace staged paste chip tokens with their content, leaving unknown
	/// tokens alone.
	pub fn expand_pending_texts(&self, text: &str) -> String {
		if self.pending_texts.is_empty() {
			return text.to_string();
		}
		let mut out = text.to_string();
		for pending in &self.pending_texts {
			if let Some(expansion) = self.atoms.get(&pending.label) {
				out = out.replace(&pending.label, expansion);
			}
		}
		out
	}

	/// Remove the staged paste with chip number `n`; its number is never
	/// recycled.
	pub fn remove_pending_text(&mut self, n: i64) {
		if let Some(index) = self.pending_texts.iter().position(|text| text.n == n) {
			let removed = self.pending_texts.remove(index);
			self.atoms.remove(&removed.label);
			self.composer_chips_cache = None;
		}
	}

	/// Visible composer attachments in band order: vision attachments first,
	/// then text pastes.
	pub fn composer_chips(&mut self) -> Vec<ComposerChipDescriptor> {
		if let Some(cache) = &self.composer_chips_cache {
			if cache.text_revision == self.text_revision
				&& cache.images_revision == self.images_revision
				&& cache.image_count == self.pending_images.len()
				&& cache.image_link_count == self.pending_image_links.len()
				&& cache.text_count == self.pending_texts.len()
			{
				return cache.chips.clone();
			}
		}
		let mut chips = Vec::with_capacity(self.pending_images.len() + self.pending_texts.len());
		for (index, image) in self.pending_images.iter().enumerate() {
			let n = index as i64 + 1;
			let link = self.pending_image_links.get(index).cloned().flatten();
			let descriptor = match link.as_deref() {
				Some(path) if is_video_path(path) => ComposerChipDescriptor::Video {
					n,
					image: image.clone(),
					link,
				},
				_ => ComposerChipDescriptor::Image {
					n,
					image: image.clone(),
					link,
				},
			};
			chips.push(descriptor);
		}
		for text in &self.pending_texts {
			chips.push(ComposerChipDescriptor::Paste {
				n: text.n,
				text: text.clone(),
			});
		}
		self.composer_chips_cache = Some(ComposerChipsCache {
			text_revision: self.text_revision,
			images_revision: self.images_revision,
			image_count: self.pending_images.len(),
			image_link_count: self.pending_image_links.len(),
			text_count: self.pending_texts.len(),
			chips: chips.clone(),
		});
		chips
	}

	/// The composer state a native layout shows.
	pub fn composer_state(&self) -> &ComposerNativeState {
		&self.composer_state
	}

	pub fn set_composer_state(&mut self, state: ComposerNativeState) {
		self.composer_state = state;
	}
}

fn format_paste_expansion(content: &str) -> String {
	format!("````\n{content}\n````")
}

/// Bash, Python and other shell-mode drafts lead with a sigil.
static SHELL_SIGIL: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^\s*(?:!!?|\$\$?)[ \t]?").expect("shell sigil"));

/// Strip a shell-mode sigil from a draft.
pub fn strip_shell_sigil(text: &str) -> &str {
	SHELL_SIGIL.find(text).map(|m| &text[m.end()..]).unwrap_or(text)
}

static BRACKETED_IMAGE_PATH: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"(?i)\.(?:png|jpe?g|gif|webp)$").expect("image suffix"));

static SHELL_ESCAPED_PATH_CHAR: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r#"\\([\\\s'"()\[\]{}&;<>|?*!$`])"#).expect("shell escape"));

static URI_SCHEME: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9+.-]*:").expect("uri scheme"));

static FILE_URI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^file://").expect("file uri"));

static ABSOLUTE_PATH_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
	Regex::new(r"^(?:/|~/|file://|\\\\|[A-Za-z]:[\\/])").expect("absolute prefix")
});

static RELATIVE_PATH_PREFIX: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"^\.\.?[\\/]").expect("relative prefix"));

/// True when an unescaped whitespace in `text` is followed by a second path
/// anchor — the signature of a multi-path payload. The reference uses a
/// lookbehind for the escape check; the regex crate has none, so the walk is
/// done here.
fn has_interior_path_anchor(text: &str) -> bool {
	let chars: Vec<char> = text.chars().collect();
	for (index, ch) in chars.iter().enumerate() {
		if !ch.is_whitespace() {
			continue;
		}
		let escaped = index > 0 && chars[index - 1] == '\\';
		if escaped {
			continue;
		}
		let rest: String = chars[index + 1..].iter().collect();
		if ABSOLUTE_PATH_PREFIX.is_match(&rest) || RELATIVE_PATH_PREFIX.is_match(&rest) {
			return true;
		}
	}
	false
}

fn percent_decode(input: &str) -> String {
	let bytes = input.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		if bytes[index] == b'%' {
			if let Some(hex) = input.get(index + 1..index + 3) {
				if let Ok(value) = u8::from_str_radix(hex, 16) {
					out.push(value);
					index += 3;
					continue;
				}
			}
		}
		out.push(bytes[index]);
		index += 1;
	}
	String::from_utf8_lossy(&out).into_owned()
}

/// `file://` URL → local filesystem path, decoding percent escapes.
fn file_uri_to_path(url: &str) -> Option<String> {
	let rest = FILE_URI.find(url).map(|m| &url[m.end()..])?;
	// Drop an authority component ("localhost", or a UNC host that is kept as
	// a leading double slash).
	let path = match rest.find('/') {
		Some(index) => {
			let authority = &rest[..index];
			let tail = &rest[index..];
			if !authority.is_empty() && authority.eq_ignore_ascii_case("localhost") {
				tail.to_string()
			} else if !authority.is_empty() {
				format!("//{authority}{tail}")
			} else {
				tail.to_string()
			}
		}
		None => rest.to_string(),
	};
	if path.is_empty() {
		return None;
	}
	Some(percent_decode(&path))
}

fn is_pasted_path_separator(ch: char) -> bool {
	matches!(ch, ' ' | '\t' | '\r' | '\n')
}

/// Trim quotes and shell escapes; unwrap `file://` URLs.
pub fn normalize_pasted_path(path: &str) -> String {
	let trimmed = path.trim();
	let first = trimmed.chars().next();
	let last = trimmed.chars().last();
	let unquoted = if trimmed.chars().count() > 1
		&& matches!(first, Some('"') | Some('\''))
		&& last == first
	{
		let mut chars = trimmed.chars();
		chars.next();
		chars.next_back();
		chars.as_str().to_string()
	} else {
		trimmed.to_string()
	};
	if FILE_URI.is_match(&unquoted) {
		if let Some(decoded) = file_uri_to_path(&unquoted) {
			return decoded;
		}
	}
	let without_home_escape = match unquoted.strip_prefix("\\~/") {
		Some(rest) => format!("~/{rest}"),
		None => unquoted,
	};
	SHELL_ESCAPED_PATH_CHAR.replace_all(&without_home_escape, "$1").into_owned()
}

/// A path is explicit when it is absolute, dot-relative, or uses Windows
/// separators instead of a non-`file` URI scheme.
pub fn is_explicit_pasted_path(path: &str) -> bool {
	if ABSOLUTE_PATH_PREFIX.is_match(path) || RELATIVE_PATH_PREFIX.is_match(path) {
		return true;
	}
	if URI_SCHEME.is_match(path) {
		return false;
	}
	path.contains('\\')
}

/// A pasted local image or video can become a vision-ready attachment.
pub fn is_previewable_attachment_path(path: &str) -> bool {
	BRACKETED_IMAGE_PATH.is_match(path) || is_video_path(path)
}

/// Split a payload on unescaped, unquoted whitespace; `None` when a quote or
/// trailing escape is left open.
pub fn split_pasted_path_segments(payload: &str) -> Option<Vec<String>> {
	let mut segments = Vec::new();
	let mut segment = String::new();
	let mut quote: Option<char> = None;
	let mut escaped = false;
	for ch in payload.chars() {
		if escaped {
			segment.push(ch);
			escaped = false;
			continue;
		}
		if ch == '\\' {
			segment.push(ch);
			escaped = true;
			continue;
		}
		if let Some(open) = quote {
			segment.push(ch);
			if ch == open {
				quote = None;
			}
			continue;
		}
		if ch == '"' || ch == '\'' {
			segment.push(ch);
			quote = Some(ch);
			continue;
		}
		if is_pasted_path_separator(ch) {
			if !segment.is_empty() {
				segments.push(std::mem::take(&mut segment));
			}
			continue;
		}
		segment.push(ch);
	}
	if escaped || quote.is_some() {
		return None;
	}
	if !segment.is_empty() {
		segments.push(segment);
	}
	if segments.is_empty() {
		None
	} else {
		Some(segments)
	}
}

/// Every whitespace/quoted-separated segment as a normalized, explicit path;
/// `None` when any segment is not one.
pub fn extract_explicit_path_segments(payload: &str) -> Option<Vec<String>> {
	let pasted = payload.trim();
	if pasted.is_empty() {
		return None;
	}
	let segments = split_pasted_path_segments(pasted)?;
	let mut paths = Vec::with_capacity(segments.len());
	for segment in segments {
		let path = normalize_pasted_path(&segment);
		if path.is_empty() || !is_explicit_pasted_path(&path) {
			return None;
		}
		paths.push(path);
	}
	Some(paths)
}

/// Whole-text-as-path pass: one previewable image or video path that is
/// anchored, newline-free, and carries no second anchor.
fn extract_whole_text_attachment_path(text: &str) -> Option<String> {
	let trimmed = text.trim();
	if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
		return None;
	}
	if !ABSOLUTE_PATH_PREFIX.is_match(trimmed) || has_interior_path_anchor(trimmed) {
		return None;
	}
	let whole_path = normalize_pasted_path(trimmed);
	if whole_path.is_empty() || !is_explicit_pasted_path(&whole_path) || !is_previewable_attachment_path(&whole_path)
	{
		return None;
	}
	Some(whole_path)
}

/// Paths from plain (un-bracketed) clipboard text.
pub fn extract_paste_paths_from_text(text: &str) -> Option<Vec<String>> {
	extract_explicit_path_segments(text)
}

/// Image or video paths from plain clipboard text, falling back to a single
/// whole-text path when the splitter fails (unescaped spaces).
pub fn extract_image_paste_paths_from_text(text: &str) -> Option<Vec<String>> {
	if let Some(paths) = extract_paste_paths_from_text(text) {
		return if paths.iter().all(|path| is_previewable_attachment_path(path)) {
			Some(paths)
		} else {
			None
		};
	}
	extract_whole_text_attachment_path(text).map(|path| vec![path])
}

fn bracketed_paste_payload(data: &str) -> Option<&str> {
	let rest = data.strip_prefix(PASTE_START)?;
	let end = rest.find(PASTE_END)?;
	if end + PASTE_END.len() != rest.len() {
		return None;
	}
	Some(&rest[..end])
}

/// Paths from a complete bracketed-paste sequence.
pub fn extract_bracketed_paste_paths(data: &str) -> Option<Vec<String>> {
	extract_explicit_path_segments(bracketed_paste_payload(data)?)
}

/// Image or video paths from a complete bracketed-paste sequence.
pub fn extract_bracketed_image_paste_paths(data: &str) -> Option<Vec<String>> {
	extract_image_paste_paths_from_text(bracketed_paste_payload(data)?)
}

/// The single image or video path of a complete bracketed-paste sequence.
pub fn extract_bracketed_image_paste_path(data: &str) -> Option<String> {
	match extract_bracketed_image_paste_paths(data) {
		Some(paths) if paths.len() == 1 => paths.first().cloned(),
		_ => None,
	}
}

/// The single image or video path when `text` is exactly one.
pub fn extract_image_path_from_text(text: &str) -> Option<String> {
	if let Some(paths) = extract_paste_paths_from_text(text) {
		if paths.len() == 1 && is_previewable_attachment_path(&paths[0]) {
			return paths.first().cloned();
		}
		return None;
	}
	extract_whole_text_attachment_path(text)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn image() -> ImageContent {
		ImageContent::new("aGk=", "image/png")
	}

	#[test]
	fn splitter_honors_quotes_and_escapes() {
		assert_eq!(
			split_pasted_path_segments(r#"/tmp/a.png "/tmp/b c.png" /tmp/d\ e.png"#),
			Some(vec![
				"/tmp/a.png".to_string(),
				r#""/tmp/b c.png""#.to_string(),
				r#"/tmp/d\ e.png"#.to_string(),
			])
		);
		assert_eq!(split_pasted_path_segments("/tmp/a.png \"/tmp/b.png"), None);
		assert_eq!(split_pasted_path_segments("   "), None);
	}

	#[test]
	fn only_explicit_segments_survive() {
		assert_eq!(
			extract_paste_paths_from_text("/tmp/a.png /tmp/b.png"),
			Some(vec!["/tmp/a.png".to_string(), "/tmp/b.png".to_string()])
		);
		// A bare relative segment is not explicit.
		assert_eq!(extract_paste_paths_from_text("/tmp/a.png b.png"), None);
		assert_eq!(
			extract_paste_paths_from_text("./a.png ../b.png"),
			Some(vec!["./a.png".to_string(), "../b.png".to_string()])
		);
		assert_eq!(extract_paste_paths_from_text("see /tmp/x.png"), None);
	}

	#[test]
	fn quoted_and_shell_escaped_paths_normalize() {
		assert_eq!(normalize_pasted_path(r#""/tmp/a b.png""#), "/tmp/a b.png");
		assert_eq!(normalize_pasted_path(r"/tmp/a\ b.png"), "/tmp/a b.png");
		assert_eq!(normalize_pasted_path(r"\~/shot.png"), "~/shot.png");
		assert_eq!(
			normalize_pasted_path("file:///tmp/shot%20one.png"),
			"/tmp/shot one.png"
		);
		assert_eq!(
			normalize_pasted_path("file://localhost/tmp/x.png"),
			"/tmp/x.png"
		);
	}

	#[test]
	fn screenshot_with_spaces_falls_back_to_whole_path() {
		let path = "file:///Users/me/Desktop/Screenshot 2026-06-25 at 1.23.45 PM.png";
		// The splitter fails on the unescaped spaces; the whole-text pass
		// recovers the path because a `file://` prefix is anchored.
		assert_eq!(extract_paste_paths_from_text(path), None);
		assert_eq!(
			extract_image_path_from_text(path),
			Some("/Users/me/Desktop/Screenshot 2026-06-25 at 1.23.45 PM.png".to_string())
		);
	}

	#[test]
	fn two_paths_never_collapse_into_one() {
		let text = "/Users/me/Desktop/a.png /Users/me/Desktop/b shot.png";
		assert_eq!(extract_image_paste_paths_from_text(text), None);
		assert_eq!(extract_image_path_from_text(text), None);
	}

	#[test]
	fn non_image_pastes_stay_text() {
		let text = "/tmp/notes.txt /tmp/todo.txt";
		assert!(extract_paste_paths_from_text(text).is_some());
		assert_eq!(extract_image_paste_paths_from_text(text), None);
	}

	#[test]
	fn bracketed_sequences_need_both_markers() {
		let wrapped = format!("{PASTE_START}/tmp/a.png {PASTE_END}");
		assert_eq!(
			extract_bracketed_paste_paths(&wrapped),
			Some(vec!["/tmp/a.png".to_string()])
		);
		assert_eq!(
			extract_bracketed_image_paste_path(&wrapped),
			Some("/tmp/a.png".to_string())
		);
		assert_eq!(extract_bracketed_paste_paths("/tmp/a.png"), None);
		assert_eq!(
			extract_bracketed_paste_paths(&format!("{PASTE_START}/tmp/a.png")),
			None
		);
	}

	#[test]
	fn staged_chips_number_never_recycles() {
		let mut editor = CustomEditor::new();
		assert_eq!(editor.add_pending_text("one", None), 1);
		assert_eq!(editor.add_pending_text("two", None), 2);
		editor.remove_pending_text(2);
		assert_eq!(editor.add_pending_text("three", None), 3);
		let chips = editor.composer_chips();
		assert_eq!(chips.len(), 2);
		assert_eq!(chips[0].label(None), chip_label(ChipKind::Paste, 1, None));
		assert_eq!(chips[1].n(), 3);
	}

	#[test]
	fn chips_put_vision_first_and_cache_until_the_draft_changes() {
		let mut editor = CustomEditor::new();
		editor.add_pending_image(image(), Some("/tmp/clip.mp4".to_string()));
		editor.add_pending_image(image(), None);
		editor.add_pending_text("hello", None);
		let chips = editor.composer_chips();
		assert_eq!(chips[0].kind(), ChipKind::Video);
		assert_eq!(chips[1].kind(), ChipKind::Image);
		assert_eq!(chips[2].kind(), ChipKind::Paste);
		// A second read reuses the cache; staging drops it.
		assert_eq!(editor.composer_chips().len(), 3);
		editor.add_pending_image(image(), None);
		assert_eq!(editor.composer_chips().len(), 4);
	}

	#[test]
	fn paste_chips_expand_on_submit() {
		let mut editor = CustomEditor::new();
		let n = editor.add_pending_text("line one\nline two", None);
		let label = chip_label(ChipKind::Paste, n, None);
		let expanded = editor.expand_pending_texts(&format!("before {label} after"));
		assert!(expanded.starts_with("before ````"));
		assert!(expanded.contains("line one\nline two"));
		assert!(expanded.ends_with("```` after"));
		assert_eq!(editor.text_snippet(&label), Some("line one\nline two"));
	}

	#[test]
	fn clearing_the_draft_drops_chips_and_atoms() {
		let mut editor = CustomEditor::new();
		editor.add_pending_image(image(), None);
		editor.add_pending_text("hello", None);
		editor.set_text("draft");
		editor.clear_draft();
		assert_eq!(editor.get_text(), "");
		assert!(editor.pending_images.is_empty());
		assert!(editor.pending_texts.is_empty());
		assert!(editor.atom(&chip_label(ChipKind::Paste, 1, None)).is_none());
		assert!(editor.composer_chips().is_empty());
	}

	#[test]
	fn shell_sigil_is_stripped() {
		assert_eq!(strip_shell_sigil("!ls -la"), "ls -la");
		assert_eq!(strip_shell_sigil("  $$print(1)"), "print(1)");
		assert_eq!(strip_shell_sigil("plain"), "plain");
	}

	#[test]
	fn spelling_features_split_into_provider_settings() {
		let mut editor = CustomEditor::new();
		editor.set_spelling_features(SpellingFeatures {
			typo_detection: false,
			autocorrect: true,
			autocomplete: WordCompletionMethod::Draft,
		});
		assert!(!editor.spelling_features().typo_detection);
		assert!(editor.spelling_features().autocorrect);
		assert_eq!(editor.word_completion_method(), WordCompletionMethod::Draft);
	}

	#[test]
	fn dictation_phases_that_hold_the_draft() {
		assert!(!DictationState::Idle.holds_draft());
		assert!(!DictationState::Starting.holds_draft());
		assert!(DictationState::Recording.holds_draft());
		assert!(DictationState::Transcribing.holds_draft());
	}
}
