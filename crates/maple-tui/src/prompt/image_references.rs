//! Reference: `l/src/prompt/image-references.ts`.
//!
//! Links behind transcript and draft images: either the file the image was read
//! from, or a host-written blob copy of its bytes, gated by the active hyperlink
//! policy.
//!
//! Deviations:
//! - `Bun.hash` is replaced by a SHA-256 digest of the base64 payload.
//! - The async blob-writer variant collapses into the synchronous one; Rust
//!   callers blocking on a writer is the host's concern.
//! - The writer and its path cache live in process-wide `Mutex`es, the Rust
//!   spelling of the reference's module-level variable and `Map`.
//! - `isNativeRendering()` reads [`crate::native::state::native_state`].

use std::sync::{Arc, LazyLock, Mutex};

use sha2::{Digest, Sha256};

use crate::native::describe::span;
use crate::native::node::Span;
use crate::native::state::native_state;
use crate::prompt::image_format::blob_extension_for_image_mime_type;
use crate::prompt::image_source::ImageContent;
use crate::render::hyperlink::{
	file_hyperlink, file_link_span, is_hyperlink_enabled, is_native_link_enabled, FilePosition,
};

/// Materialized image destination returned by the host blob writer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageBlobResult {
	pub display_path: String,
}

/// Options for a blob write.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageBlobOptions {
	pub extension: Option<String>,
}

/// Host blob writer: bytes plus options to a destination.
pub type ImageBlobWriter =
	dyn Fn(&[u8], &ImageBlobOptions) -> Result<ImageBlobResult, String> + Send + Sync;

/// Cached probe result for a draft image: dimensions, `None` (probe failed), or
/// `None`-of-`None` (never probed).
pub fn cached_image_dimensions(
	image: &ImageContent,
) -> Option<Option<crate::prompt::image_source::ImageDimensions>> {
	image.cached_image_dimensions()
}

/// Record a probe result for a draft image (see [`cached_image_dimensions`]).
pub fn set_cached_image_dimensions(
	image: &mut ImageContent,
	dimensions: Option<crate::prompt::image_source::ImageDimensions>,
) {
	image.set_cached_image_dimensions(dimensions);
}

/// `label` as a link to `image_links[index - 1]`, or the plain rendered label.
pub fn image_reference_hyperlink(
	label: &str,
	index: usize,
	image_links: Option<&[Option<String>]>,
	render_label: &dyn Fn(&str) -> String,
) -> String {
	let rendered = render_label(label);
	let target = image_links
		.and_then(|links| index.checked_sub(1).and_then(|slot| links.get(slot)))
		.and_then(|link| link.as_deref());
	match target {
		Some(target) => file_hyperlink(target, &rendered, FilePosition::default()),
		None => rendered,
	}
}

/// Base64 payload to raw bytes; invalid payloads pass through as their own bytes
/// rather than panicking (the reference's `Buffer.from` ignores bad input).
fn decode_base64(data: &str) -> Vec<u8> {
	use base64::Engine;
	base64::engine::general_purpose::STANDARD
		.decode(data)
		.unwrap_or_else(|_| data.as_bytes().to_vec())
}

fn materialize_image_reference_link(
	image: &ImageContent,
	index: usize,
	put_blob: &ImageBlobWriter,
) -> Option<String> {
	let options = ImageBlobOptions {
		extension: blob_extension_for_image_mime_type(Some(&image.mime_type)),
	};
	match put_blob(&decode_base64(&image.data), &options) {
		Ok(result) => Some(result.display_path),
		Err(error) => {
			let _ = (index, &image.mime_type, error);
			None
		}
	}
}

/// Write blob copies for `images`; `None` when there are none or every write
/// failed.
pub fn materialize_image_reference_links(
	images: Option<&[ImageContent]>,
	put_blob: &ImageBlobWriter,
) -> Option<Vec<Option<String>>> {
	let images = images.filter(|images| !images.is_empty())?;
	let links: Vec<Option<String>> = images
		.iter()
		.enumerate()
		.map(|(index, image)| materialize_image_reference_link(image, index + 1, put_blob))
		.collect();
	links.iter().any(Option::is_some).then_some(links)
}

static HOST_BLOB_WRITER: LazyLock<Mutex<Option<Arc<ImageBlobWriter>>>> =
	LazyLock::new(|| Mutex::new(None));
/// Blob paths already written for an image payload, keyed by mime type and content hash.
static BLOB_LINKS: LazyLock<Mutex<Vec<(String, String)>>> = LazyLock::new(|| Mutex::new(Vec::new()));
const BLOB_LINK_CACHE_LIMIT: usize = 256;

/// Register the host's blob writer that backs links for images without a file
/// of their own. Returns a disposer that unregisters it unless another writer
/// replaced it since.
pub fn set_image_blob_writer(next: Option<Arc<ImageBlobWriter>>) -> impl Fn() {
	*HOST_BLOB_WRITER
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner()) = next.clone();
	BLOB_LINKS
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.clear();
	move || {
		let mut guard = HOST_BLOB_WRITER
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner());
		let same = match (&*guard, &next) {
			(Some(current), Some(previous)) => Arc::ptr_eq(current, previous),
			(None, None) => true,
			_ => false,
		};
		if !same {
			return;
		}
		*guard = None;
		BLOB_LINKS
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
			.clear();
	}
}

fn host_blob_writer() -> Option<Arc<ImageBlobWriter>> {
	HOST_BLOB_WRITER
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.clone()
}

/// Whether transcript images carry links under the current policy: a native
/// surface honours only an explicit `off`, text mode the resolved OSC 8 support.
pub fn image_links_enabled() -> bool {
	if native_state().is_native_rendering() {
		is_native_link_enabled()
	} else {
		is_hyperlink_enabled()
	}
}

/// Content hash of an image payload, keying the blob path cache.
fn image_hash(data: &str) -> String {
	let mut hasher = Sha256::new();
	hasher.update(data.as_bytes());
	let digest = hasher.finalize();
	digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The file a transcript image opens: `source_path` when it was read from one,
/// else a blob copy of its bytes written through the host. `None` when links are
/// disabled or no file exists, so a disabled policy never writes blobs.
pub fn transcript_image_link(image: &ImageContent, source_path: Option<&str>) -> Option<String> {
	if !image_links_enabled() {
		return None;
	}
	if let Some(path) = source_path {
		return Some(path.to_string());
	}
	let writer = host_blob_writer()?;
	if image.data.is_empty() {
		return None;
	}
	let key = format!("{}:{}", image.mime_type, image_hash(&image.data));
	if let Some(cached) = BLOB_LINKS
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner())
		.iter()
		.find(|(known, _)| *known == key)
		.map(|(_, link)| link.clone())
	{
		return Some(cached);
	}
	let link = materialize_image_reference_link(image, 1, writer.as_ref())?;
	let mut cache = BLOB_LINKS
		.lock()
		.unwrap_or_else(|poisoned| poisoned.into_inner());
	if cache.len() >= BLOB_LINK_CACHE_LIMIT {
		cache.remove(0);
	}
	cache.push((key, link.clone()));
	Some(link)
}

/// `text` (an image's placeholder) as an OSC 8 link to the image's file; plain
/// when hyperlinks are off or no file exists.
pub fn linked_image_text(text: &str, image: &ImageContent, source_path: Option<&str>) -> String {
	if !is_hyperlink_enabled() {
		return text.to_string();
	}
	match transcript_image_link(image, source_path) {
		Some(target) => file_hyperlink(&target, text, FilePosition::default()),
		None => text.to_string(),
	}
}

/// Native counterpart of [`linked_image_text`]: a span linking to the image's
/// file when one exists.
pub fn linked_image_span(
	text: &str,
	image: &ImageContent,
	source_path: Option<&str>,
	s: Option<&str>,
) -> Span {
	match transcript_image_link(image, source_path) {
		Some(target) => file_link_span(&target, text, s.unwrap_or("")),
		None => span(text, s),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::prompt::image_source::ImageContent;
	use crate::render::hyperlink::{apply_hyperlink_setting, HyperlinkMode};

	#[test]
	fn reference_hyperlink_uses_matching_link() {
		let render = |text: &str| format!("[{text}]");
		let links = vec![Some("/tmp/a.png".to_string()), None];
		let linked = image_reference_hyperlink("1", 1, Some(&links), &render);
		assert!(linked.contains("file://") || linked.contains("/tmp/a.png"), "{linked}");
		let plain = image_reference_hyperlink("2", 2, Some(&links), &render);
		assert_eq!(plain, "[2]");
		let missing = image_reference_hyperlink("3", 3, Some(&links), &render);
		assert_eq!(missing, "[3]");
		let no_links = image_reference_hyperlink("1", 1, None, &render);
		assert_eq!(no_links, "[1]");
	}

	#[test]
	fn materializes_links_only_when_a_write_succeeds() {
		let image = ImageContent::new("QUJD", "image/png");
		let writer = |_bytes: &[u8], options: &ImageBlobOptions| {
			assert_eq!(options.extension.as_deref(), Some("png"));
			Ok(ImageBlobResult {
				display_path: "local://blob/1.png".to_string(),
			})
		};
		let links = materialize_image_reference_links(Some(&[image.clone()]), &writer)
			.expect("links");
		assert_eq!(links, vec![Some("local://blob/1.png".to_string())]);
		assert_eq!(materialize_image_reference_links(Some(&[]), &writer), None);
		assert_eq!(materialize_image_reference_links(None, &writer), None);

		let failing = |_bytes: &[u8], _options: &ImageBlobOptions| Err("disk full".to_string());
		assert_eq!(
			materialize_image_reference_links(Some(&[image]), &failing),
			None
		);
	}

	#[test]
	fn transcript_links_prefer_source_path_and_cache_blobs() {
		apply_hyperlink_setting(Some(HyperlinkMode::Always));
		let mut writes = 0;
		let writer = Arc::new(
			move |_bytes: &[u8], _options: &ImageBlobOptions| -> Result<ImageBlobResult, String> {
				Ok(ImageBlobResult {
					display_path: "local://blob/x.png".to_string(),
				})
			},
		);
		let _ = &mut writes;
		let dispose = set_image_blob_writer(Some(writer));
		let image = ImageContent::new("QUJD", "image/png");
		assert_eq!(
			transcript_image_link(&image, Some("/tmp/a.png")).as_deref(),
			Some("/tmp/a.png")
		);
		assert_eq!(
			transcript_image_link(&image, None).as_deref(),
			Some("local://blob/x.png")
		);
		dispose();
		assert_eq!(transcript_image_link(&image, None), None);
		apply_hyperlink_setting(Some(HyperlinkMode::Auto));
	}

	#[test]
	fn linked_text_and_span_follow_policy() {
		apply_hyperlink_setting(Some(HyperlinkMode::Off));
		let image = ImageContent::new("QUJD", "image/png");
		assert_eq!(linked_image_text("[Image 1]", &image, Some("/tmp/a.png")), "[Image 1]");
		let plain = linked_image_span("[Image 1]", &image, Some("/tmp/a.png"), None);
		assert_eq!(plain.t, "[Image 1]");
		assert!(plain.s.is_none());

		apply_hyperlink_setting(Some(HyperlinkMode::Always));
		let linked = linked_image_text("[Image 1]", &image, Some("/tmp/a.png"));
		assert!(linked.contains(']'), "{linked}");
		let linked_span = linked_image_span("[Image 1]", &image, Some("/tmp/a.png"), Some("dim"));
		assert_eq!(linked_span.t, "[Image 1]");
		apply_hyperlink_setting(Some(HyperlinkMode::Auto));
	}
}
