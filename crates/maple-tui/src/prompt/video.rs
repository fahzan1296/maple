//! Reference: `l/src/prompt/video.ts`.
//!
//! Video container detection plus the contact-sheet image a video is turned
//! into for the model, tagged with its original path.

use std::path::Path;

use crate::prompt::image_source::{
	image_attachment_source, tag_image_attachment_source, ImageAttachmentSourceKind, ImageContent,
	SourceTaggedImage,
};

/// Container extensions treated as video. Mirrors the video subset of the
/// local-protocol binary list.
const VIDEO_EXTENSIONS: [&str; 7] = [".mp4", ".mov", ".mkv", ".webm", ".m4v", ".avi", ".wmv"];

/// True when the path names a video container we handle through ffmpeg.
pub fn is_video_path(file_path: &str) -> bool {
	let extension = Path::new(file_path)
		.extension()
		.map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
		.unwrap_or_default();
	VIDEO_EXTENSIONS.contains(&extension.as_str())
}

/// Create a model-ready contact-sheet image tagged with its original video path.
pub fn create_video_preview_image(preview: ImageContent, source_path: &str) -> SourceTaggedImage {
	tag_image_attachment_source(preview, source_path, ImageAttachmentSourceKind::Video)
}

/// Return the original video path associated with a generated contact-sheet
/// image, via the shared attachment-source tag. `None` for untagged images and
/// for image-file (non-video) sources.
pub fn video_preview_source(preview: &ImageContent) -> Option<&str> {
	image_attachment_source(preview)
		.filter(|source| source.kind == ImageAttachmentSourceKind::Video)
		.map(|source| source.path.as_str())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::prompt::image_source::{ImageAttachmentSourceKind, ImageContent};

	#[test]
	fn detects_video_containers() {
		let cases: [(&str, bool); 10] = [
			("clip.mp4", true),
			("clip.MOV", true),
			("clip.mkv", true),
			("clip.webm", true),
			("clip.m4v", true),
			("clip.avi", true),
			("clip.wmv", true),
			("clip.png", false),
			("clip", false),
			("/tmp/a.b/clip.mov", true),
		];
		for (path, expected) in cases {
			assert_eq!(is_video_path(path), expected, "path {path}");
		}
	}

	#[test]
	fn tags_and_resolves_preview_source() {
		let preview = create_video_preview_image(ImageContent::new("QQ==", "image/png"), "/tmp/clip.mp4");
		assert_eq!(video_preview_source(&preview), Some("/tmp/clip.mp4"));

		let untagged = ImageContent::new("QQ==", "image/png");
		assert_eq!(video_preview_source(&untagged), None);

		let tagged_image = tag_image_attachment_source(
			ImageContent::new("QQ==", "image/png"),
			"/tmp/a.png",
			ImageAttachmentSourceKind::Image,
		);
		assert_eq!(video_preview_source(&tagged_image), None);
	}
}
