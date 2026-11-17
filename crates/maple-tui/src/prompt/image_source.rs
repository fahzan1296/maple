//! Reference: `l/src/prompt/image-source.ts`.
//!
//! File source carried on image attachments backed by a file on disk:
//! path-pasted/drag-and-dropped images, clipboard images the coding agent
//! commits to the session's `local://` root, and generated video contact-sheet
//! previews. The source travels with the draft object but stays out of
//! serialized/model-bound image data, until the session creates the hidden
//! companion message that tells the model the path.
//!
//! Deviation: the TypeScript hides the tag behind a `Symbol` key, which the
//! Rust port expresses as an `Option` field skipped by `serde`, so it is
//! likewise absent from the wire form. `SourceTaggedImage` is an alias: the
//! tag is optional metadata on [`ImageContent`], not a separate shape.

use serde::{Deserialize, Serialize};

/// How an image attachment's backing file entered the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageAttachmentSourceKind {
	Image,
	Video,
}

/// Local file backing an image attachment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAttachmentSource {
	/// Absolute filesystem path, or a session-relative `local://` URL for images
	/// committed to the session.
	pub path: String,
	pub kind: ImageAttachmentSourceKind,
}

/// Pixel dimensions of a draft image, as probed from its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageDimensions {
	pub width: u32,
	pub height: u32,
}

/// A model-ready image payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageContent {
	#[serde(rename = "type")]
	pub kind: String,
	pub data: String,
	#[serde(rename = "mimeType")]
	pub mime_type: String,
	/// Local file this payload was loaded from; never serialized (see module doc).
	#[serde(skip)]
	pub attachment_source: Option<ImageAttachmentSource>,
	/// Probe result riding on the draft: `Some(Some(dims))` succeeded,
	/// `Some(None)` failed (so the chips band never re-decodes a corrupt header),
	/// `None` never probed. Never serialized.
	#[serde(skip)]
	pub cached_dimensions: Option<Option<ImageDimensions>>,
}

impl ImageContent {
	/// A payload with no backing file.
	pub fn new(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
		Self {
			kind: "image".to_string(),
			data: data.into(),
			mime_type: mime_type.into(),
			attachment_source: None,
			cached_dimensions: None,
		}
	}

	/// The cached probe result for this draft.
	pub fn cached_image_dimensions(&self) -> Option<Option<ImageDimensions>> {
		self.cached_dimensions
	}

	/// Record a probe result for this draft.
	pub fn set_cached_image_dimensions(&mut self, dimensions: Option<ImageDimensions>) {
		self.cached_dimensions = Some(dimensions);
	}
}

/// An image attachment tagged with the original local file it came from.
pub type SourceTaggedImage = ImageContent;

/// Create a model-ready image tagged with the local file it was loaded from.
pub fn tag_image_attachment_source(
	image: ImageContent,
	path: impl Into<String>,
	kind: ImageAttachmentSourceKind,
) -> SourceTaggedImage {
	SourceTaggedImage {
		kind: "image".to_string(),
		data: image.data,
		mime_type: image.mime_type,
		attachment_source: Some(ImageAttachmentSource {
			path: path.into(),
			kind,
		}),
		cached_dimensions: image.cached_dimensions,
	}
}

/// Return the local file backing an image attachment, or `None` for payloads
/// with no file on disk.
pub fn image_attachment_source(image: &ImageContent) -> Option<&ImageAttachmentSource> {
	image.attachment_source.as_ref()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tags_and_reads_source() {
		let image = ImageContent::new("QUJD", "image/png");
		assert_eq!(image_attachment_source(&image), None);
		let tagged = tag_image_attachment_source(image, "/tmp/a.png", ImageAttachmentSourceKind::Image);
		assert_eq!(
			image_attachment_source(&tagged),
			Some(&ImageAttachmentSource {
				path: "/tmp/a.png".to_string(),
				kind: ImageAttachmentSourceKind::Image,
			})
		);
		assert_eq!(tagged.mime_type, "image/png");
	}

	#[test]
	fn source_is_not_serialized() {
		let cases = ["image", "video"];
		for value in cases {
			let kind = if value == "video" {
				ImageAttachmentSourceKind::Video
			} else {
				ImageAttachmentSourceKind::Image
			};
			let tagged =
				tag_image_attachment_source(ImageContent::new("QQ==", "image/png"), "local://x", kind);
			let json = serde_json::to_string(&tagged).expect("serialize");
			assert_eq!(json, r#"{"type":"image","data":"QQ==","mimeType":"image/png"}"#);
		}
	}
}
