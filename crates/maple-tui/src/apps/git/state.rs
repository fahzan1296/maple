//! Git repository model shared by the sidebar, diff pane and git TUI.
//!
//! Reference: `l/src/apps/git/state.ts`.
//!
//! Deviations:
//! - `DiffStreamProgress`/`DiffStreamResult`/`DiffRun`/`PatchHunk` come from
//!   `@oh-my-pi/pi-natives`, which is not ported. They are declared here from
//!   the package's `index.d.ts` shapes and will move with the natives port.
//! - The async `GitTuiModel` operations become trait methods returning
//!   [`BoxFuture`]; rejections map to `Result<_, String>`. The DOM
//!   `AbortSignal` becomes `Option<&dyn Fn() -> bool>` (an `aborted` probe).
//! - TypeScript `kind` string discriminants become Rust enum variants.

use std::future::Future;
use std::pin::Pin;

/// Context lines retained around each exact streamed hunk.
pub const DIFF_CONTEXT_LINES: u32 = 3;

/// Boxed future used by the async host operations in [`GitTuiModel`].
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Change classification shown beside a file path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChangeKind {
	#[default]
	Modified,
	Added,
	Deleted,
	Renamed,
	Untracked,
	Conflicted,
}

impl ChangeKind {
	/// Reference string discriminant (`"modified"`, `"added"`, ...).
	pub fn as_str(self) -> &'static str {
		match self {
			ChangeKind::Modified => "modified",
			ChangeKind::Added => "added",
			ChangeKind::Deleted => "deleted",
			ChangeKind::Renamed => "renamed",
			ChangeKind::Untracked => "untracked",
			ChangeKind::Conflicted => "conflicted",
		}
	}
}

impl From<&str> for ChangeKind {
	fn from(value: &str) -> Self {
		match value {
			"added" => ChangeKind::Added,
			"deleted" => ChangeKind::Deleted,
			"renamed" => ChangeKind::Renamed,
			"untracked" => ChangeKind::Untracked,
			"conflicted" => ChangeKind::Conflicted,
			_ => ChangeKind::Modified,
		}
	}
}

/// Repository area represented by a file list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChangeArea {
	#[default]
	Unstaged,
	Staged,
	Commit,
}

impl ChangeArea {
	/// Reference string discriminant (`"unstaged"`, `"staged"`, `"commit"`).
	pub fn as_str(self) -> &'static str {
		match self {
			ChangeArea::Unstaged => "unstaged",
			ChangeArea::Staged => "staged",
			ChangeArea::Commit => "commit",
		}
	}
}

impl From<&str> for ChangeArea {
	fn from(value: &str) -> Self {
		match value {
			"staged" => ChangeArea::Staged,
			"commit" => ChangeArea::Commit,
			_ => ChangeArea::Unstaged,
		}
	}
}

/// One changed path shown in the sidebar file lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangedFile {
	pub path: String,
	/// Pre-rename path for renames/copies.
	pub orig_path: Option<String>,
	pub kind: ChangeKind,
	pub area: ChangeArea,
	pub additions: Option<u32>,
	pub deletions: Option<u32>,
}

/// HEAD commit metadata for the clean-tree sidebar view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadCommit {
	pub sha: String,
	pub short_sha: String,
	pub subject: String,
	pub body: String,
	pub author_name: String,
	pub author_email: String,
	/// Already formatted by the host.
	pub author_date: String,
	pub parents: Vec<String>,
	/// Changed paths once their numstats have loaded.
	pub files: Vec<ChangedFile>,
	/// Whether `files` contains the complete commit file list.
	pub files_loaded: bool,
}

/// A terminal-ready image decoded from one Git file side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewImage {
	pub data: String,
	/// Always `"image/png"` in the reference.
	pub mime_type: String,
	pub source_mime_type: String,
	pub width_px: u32,
	pub height_px: u32,
	pub byte_length: u32,
	/// Stable content identity for terminal graphics placement.
	pub key: String,
	/// Git LFS object id when the preview came from local LFS storage.
	pub lfs_oid: Option<String>,
}

/// One side of a media or binary Git change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileAssetSide {
	Empty,
	Text {
		byte_length: u32,
		lfs_oid: Option<String>,
	},
	Image {
		image: ReviewImage,
	},
	Binary {
		byte_length: Option<u32>,
		lfs_oid: Option<String>,
	},
	TooLarge {
		byte_length: Option<u32>,
		lfs_oid: Option<String>,
	},
	LfsMissing {
		oid: String,
		byte_length: u32,
	},
}

impl FileAssetSide {
	/// Reference `kind` discriminant.
	pub fn kind(&self) -> &'static str {
		match self {
			FileAssetSide::Empty => "empty",
			FileAssetSide::Text { .. } => "text",
			FileAssetSide::Image { .. } => "image",
			FileAssetSide::Binary { .. } => "binary",
			FileAssetSide::TooLarge { .. } => "tooLarge",
			FileAssetSide::LfsMissing { .. } => "lfsMissing",
		}
	}
}

/// A line-diff run without its token text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRun {
	pub count: u32,
	pub added: bool,
	pub removed: bool,
}

/// One unified diff hunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchHunk {
	/// 1-based first line of the hunk in the old text.
	pub old_start: u32,
	pub old_lines: u32,
	/// 1-based first line of the hunk in the new text.
	pub new_start: u32,
	pub new_lines: u32,
	/// `+`/`-`/` `-prefixed lines without trailing newlines, plus
	/// `\ No newline at end of file` markers where applicable.
	pub lines: Vec<String>,
}

/// Exact line-diff output produced when a stream finishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffStreamResult {
	pub runs: Vec<DiffRun>,
	pub hunks: Vec<PatchHunk>,
	pub old_ends_newline: bool,
	pub new_ends_newline: bool,
}

/// Observable ingestion state for a streamed line diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffStreamProgress {
	/// Complete old-side lines available for rendering.
	pub old_lines: u32,
	/// Complete new-side lines available for rendering.
	pub new_lines: u32,
	/// Leading complete lines proven equal on both sides.
	pub stable_common_lines: u32,
	pub old_done: bool,
	pub new_done: bool,
	/// Whether either side contains a NUL byte/code unit.
	pub binary: bool,
	/// Whether either native file exceeded its caller-provided size limit.
	pub too_large: bool,
}

/// Diffable UTF-8 content for both sides of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFileContents {
	pub old_text: String,
	pub new_text: String,
	pub stream_result: DiffStreamResult,
}

/// Where one side of a change lives, so a host viewer can load it by path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaRef {
	/// Absolute path of the file (repository root joined with its repository path).
	pub path: String,
	/// Git revision holding this side (`HEAD`, a sha, `:0` for the index);
	/// absent for the worktree file.
	pub rev: Option<String>,
}

/// Non-text sides rendered as previews or explicit placeholders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetFileContents {
	pub old: FileAssetSide,
	pub new: FileAssetSide,
	/// Source of `old`; absent when that side is empty.
	pub old_ref: Option<MediaRef>,
	/// Source of `new`; absent when that side is empty.
	pub new_ref: Option<MediaRef>,
}

/// Loaded file content selected for text diffing or asset preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileContents {
	Text(TextFileContents),
	Asset(AssetFileContents),
}

impl FileContents {
	/// Reference `kind` discriminant.
	pub fn kind(&self) -> &'static str {
		match self {
			FileContents::Text(_) => "text",
			FileContents::Asset(_) => "asset",
		}
	}
}

/// Newly completed lines and state emitted while a file pair streams.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStreamUpdate {
	pub old_line_offset: u32,
	pub old_lines: Vec<String>,
	pub new_line_offset: u32,
	pub new_lines: Vec<String>,
	pub progress: DiffStreamProgress,
}

/// Repository values read by the sidebar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitViewState {
	pub cwd: String,
	pub branch: Option<String>,
	pub clean: bool,
	pub unstaged: Vec<ChangedFile>,
	pub staged: Vec<ChangedFile>,
	pub head_commit: Option<HeadCommit>,
}

/// Commit options accepted by [`GitTuiModel::commit`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommitOptions {
	pub amend: bool,
}

/// Patch options accepted by [`GitTuiModel::apply_patch`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyPatchOptions {
	pub cached: bool,
	pub reverse: bool,
}

/// Repository operations supplied by the command host.
///
/// The reference interface extends [`GitViewState`]; a Rust trait cannot carry
/// fields, so implementors expose the data through [`GitTuiModel::view_state`].
pub trait GitTuiModel {
	/// Current repository values.
	fn view_state(&self) -> &GitViewState;
	/// Mutable access to repository values.
	fn view_state_mut(&mut self) -> &mut GitViewState;

	/// Reload the repository state.
	fn refresh(&mut self) -> BoxFuture<'_, Result<bool, String>>;
	/// Load addition/deletion statistics for the changed files.
	fn load_change_stats(&mut self) -> BoxFuture<'_, Result<bool, String>>;
	/// Load the HEAD commit file list.
	fn load_head_files(&mut self) -> BoxFuture<'_, Result<bool, String>>;
	/// Stream both sides of `file`, reporting completed lines through
	/// `on_progress`. `signal` reports whether the caller cancelled.
	fn stream_contents<'a>(
		&'a mut self,
		file: &'a ChangedFile,
		on_progress: &'a mut dyn FnMut(FileStreamUpdate),
		signal: Option<&'a dyn Fn() -> bool>,
	) -> BoxFuture<'a, Result<FileContents, String>>;
	/// Stage `files`, or every change when `None`.
	fn stage(&mut self, files: Option<&[ChangedFile]>) -> BoxFuture<'_, Result<(), String>>;
	/// Unstage `files`, or every staged change when `None`.
	fn unstage(&mut self, files: Option<&[ChangedFile]>) -> BoxFuture<'_, Result<(), String>>;
	/// Discard the selected `files`.
	fn discard(&mut self, files: &[ChangedFile]) -> BoxFuture<'_, Result<(), String>>;
	/// Create a commit.
	fn commit(
		&mut self,
		message: &str,
		options: CommitOptions,
	) -> BoxFuture<'_, Result<(), String>>;
	/// Apply a unified patch to the index/worktree.
	fn apply_patch(
		&mut self,
		patch_text: &str,
		options: ApplyPatchOptions,
	) -> BoxFuture<'_, Result<(), String>>;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn change_kind_strings_match_reference() {
		assert_eq!(ChangeKind::Modified.as_str(), "modified");
		assert_eq!(ChangeKind::Added.as_str(), "added");
		assert_eq!(ChangeKind::Deleted.as_str(), "deleted");
		assert_eq!(ChangeKind::Renamed.as_str(), "renamed");
		assert_eq!(ChangeKind::Untracked.as_str(), "untracked");
		assert_eq!(ChangeKind::Conflicted.as_str(), "conflicted");
	}

	#[test]
	fn change_area_strings_match_reference() {
		assert_eq!(ChangeArea::Unstaged.as_str(), "unstaged");
		assert_eq!(ChangeArea::Staged.as_str(), "staged");
		assert_eq!(ChangeArea::Commit.as_str(), "commit");
	}

	#[test]
	fn file_asset_side_kind_discriminants() {
		assert_eq!(FileAssetSide::Empty.kind(), "empty");
		assert_eq!(
			FileAssetSide::Text {
				byte_length: 1,
				lfs_oid: None
			}
			.kind(),
			"text"
		);
		assert_eq!(
			FileAssetSide::Binary {
				byte_length: None,
				lfs_oid: None
			}
			.kind(),
			"binary"
		);
		assert_eq!(
			FileAssetSide::TooLarge {
				byte_length: Some(9),
				lfs_oid: None
			}
			.kind(),
			"tooLarge"
		);
		assert_eq!(
			FileAssetSide::LfsMissing {
				oid: "abc".to_string(),
				byte_length: 2
			}
			.kind(),
			"lfsMissing"
		);
	}

	#[test]
	fn file_contents_kind_discriminants() {
		let text = FileContents::Text(TextFileContents {
			old_text: "a".to_string(),
			new_text: "b".to_string(),
			stream_result: DiffStreamResult {
				runs: vec![],
				hunks: vec![],
				old_ends_newline: true,
				new_ends_newline: true,
			},
		});
		assert_eq!(text.kind(), "text");
		let asset = FileContents::Asset(AssetFileContents {
			old: FileAssetSide::Empty,
			new: FileAssetSide::Empty,
			old_ref: None,
			new_ref: None,
		});
		assert_eq!(asset.kind(), "asset");
	}

	#[test]
	fn diff_context_lines_matches_reference() {
		assert_eq!(DIFF_CONTEXT_LINES, 3);
	}
}
