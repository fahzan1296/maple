//! Image input limits and decode diagnostics.
//!
//! Reference: `l/src/chat/image-loading.ts`.
//!
//! Constants, the WebP-averse model test, the two ingress errors, the JPEG
//! completeness walk, and the PNG-conversion cache that keying renderers use to
//! reuse one conversion per distinct payload.
//!
//! Deviations: this crate has no image codec and no async runtime, so
//! [`image_decode_failure_reason`] validates what can be validated without one
//! (base64, container sniffing, JPEG end-of-image walk) and the PNG converters
//! take the encoder as a callback; the in-flight promise map collapses into the
//! cache plus a caller-supplied encoder, since a synchronous caller cannot join a
//! conversion that has not finished.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use base64::Engine;

use crate::chat::messages::ImageContent;
use crate::tools::output_meta::format_bytes;

/// Largest image accepted as model input.
pub const MAX_IMAGE_INPUT_BYTES: usize = 20 * 1024 * 1024;

/// Image MIME types every vision backend accepts as-is.
pub const SUPPORTED_INPUT_IMAGE_MIME_TYPES: [&str; 4] =
    ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Whether `mime_type` is an image type every backend accepts as-is.
pub fn is_supported_input_image_mime_type(mime_type: &str) -> bool {
    SUPPORTED_INPUT_IMAGE_MIME_TYPES
        .iter()
        .any(|supported| supported.eq_ignore_ascii_case(mime_type))
}

/// The model fields the WebP check reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelImageTraits<'a> {
    /// Configured provider id.
    pub provider: &'a str,
    /// Provider API dialect.
    pub api: &'a str,
    /// Image input decoder reported by the catalog (`"stb"` for llama.cpp).
    pub image_input_decoder: Option<&'a str>,
}

/// Ollama and its local-backend family decode image input through llama.cpp /
/// `stb_image`, which is compiled without WebP support, so a WebP upload fails
/// with an opaque HTTP 400. Detect those models so the resize pipeline encodes
/// to PNG/JPEG instead — the automatic equivalent of `MAPLE_NO_WEBP=1`.
pub fn model_lacks_webp_support(model: Option<&ModelImageTraits<'_>>) -> bool {
    let Some(model) = model else {
        return false;
    };
    model.image_input_decoder == Some("stb")
        || matches!(
            model.provider,
            "ollama" | "ollama-cloud" | "llama.cpp" | "lm-studio" | "local-server"
        )
        || model.api == "ollama-chat"
}

/// `Some(true)` when `model` cannot decode WebP, otherwise `None` so the
/// `MAPLE_NO_WEBP` env fallback in the resize pipeline still applies.
pub fn webp_exclusion_for_model(model: Option<&ModelImageTraits<'_>>) -> Option<bool> {
    model_lacks_webp_support(model).then_some(true)
}

/// An image file over the ingress byte ceiling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageInputTooLargeError {
    /// The rejected size.
    pub bytes: usize,
    /// The ceiling.
    pub max_bytes: usize,
}

impl ImageInputTooLargeError {
    /// The error message.
    pub fn message(&self) -> String {
        format!(
            "Image file too large: {} exceeds {} limit.",
            format_bytes(self.bytes),
            format_bytes(self.max_bytes)
        )
    }
}

impl std::fmt::Display for ImageInputTooLargeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ImageInputTooLargeError {}

/// Image bytes cannot be decoded — a truncated stream, a payload with a hole in
/// the middle, or bytes that are not the container they claim.
///
/// Failing at ingress keeps them out of the transcript, where they would
/// otherwise be persisted and rejected by the provider on every later request,
/// with no way to resume the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidImageDataError {
    /// Why the payload does not decode.
    pub reason: String,
    message: String,
}

impl InvalidImageDataError {
    /// Build the error for `label`.
    pub fn new(label: &str, mime_type: &str, reason: &str) -> Self {
        InvalidImageDataError {
            reason: reason.to_string(),
            message: format!("{label} is not a decodable {mime_type} image: {reason}"),
        }
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for InvalidImageDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InvalidImageDataError {}

/// Whether a JPEG stream reaches an EOI marker.
fn jpeg_stream_is_complete(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return false;
    }
    let mut offset = 2usize;
    while offset < bytes.len() {
        if bytes[offset] != 0xff {
            return false;
        }
        let mut mark_offset = offset + 1;
        while mark_offset < bytes.len() && bytes[mark_offset] == 0xff {
            mark_offset += 1;
        }
        if mark_offset >= bytes.len() {
            return false;
        }
        let marker = bytes[mark_offset];
        if marker == 0xd9 {
            return true;
        }
        offset = mark_offset + 1;
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if marker == 0xd8 {
            return false;
        }
        if offset + 1 >= bytes.len() {
            return false;
        }
        let segment_length = ((bytes[offset] as usize) << 8) | bytes[offset + 1] as usize;
        if segment_length < 2 {
            return false;
        }
        offset += segment_length;
        if offset > bytes.len() {
            return false;
        }
        if marker == 0xda {
            // Entropy-coded data: 0xFF is stuffed as FF00, restart markers are
            // standalone, and every other following byte opens the next segment.
            loop {
                let Some(next) = bytes[offset..].iter().position(|&byte| byte == 0xff) else {
                    return false;
                };
                let next = offset + next;
                let mut scan = next + 1;
                while scan < bytes.len() && bytes[scan] == 0xff {
                    scan += 1;
                }
                if scan >= bytes.len() {
                    return false;
                }
                let code = bytes[scan];
                if code == 0x00 || (0xd0..=0xd7).contains(&code) {
                    offset = scan + 1;
                    continue;
                }
                offset = next;
                break;
            }
        }
    }
    false
}

/// Sniff the raster container a payload actually holds.
fn detected_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// Why an image cannot be decoded, or `None` when it decodes.
///
/// A full decode is the only check that matches what vision backends accept, so
/// this validates exactly: base64 well-formedness, the declared-vs-detected
/// container, and — for JPEG — the end-of-image marker that a middle-elided
/// stream loses. Callers on hot paths must cache the verdict.
pub fn image_decode_failure_reason(image: &ImageContent) -> Option<String> {
    let bytes = match base64::engine::general_purpose::STANDARD.decode(image.data.as_bytes()) {
        Ok(bytes) => bytes,
        Err(error) => return Some(format!("invalid base64: {error}")),
    };
    let declared = image.mime_type.to_lowercase();
    if let Some(detected) = detected_mime(&bytes) {
        if detected != declared {
            return Some(format!("declared {} but contains {detected}", image.mime_type));
        }
    }
    if declared == "image/jpeg" && !jpeg_stream_is_complete(&bytes) {
        return Some("truncated JPEG data: no EOI marker".to_string());
    }
    None
}

/// Encodes raster bytes to PNG, or `None` when the runtime cannot encode them.
pub type PngEncoder<'a> = &'a dyn Fn(&[u8]) -> Option<Vec<u8>>;

/// Converts an image to PNG, or `None` when its bytes cannot be decoded or the
/// encoder declines.
pub fn convert_image_to_png(image: &ImageContent, encoder: PngEncoder<'_>) -> Option<ImageContent> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image.data.as_bytes())
        .ok()?;
    let data = encoder(&bytes)?;
    Some(ImageContent {
        data: base64::engine::general_purpose::STANDARD.encode(data),
        mime_type: "image/png".to_string(),
        ..image.clone()
    })
}

/// Byte ceiling for [`convert_image_to_png_shared`]'s resident conversions.
/// Converted PNGs are larger than the webp/jpeg they came from, so the cache is
/// bounded and evicts least-recently-used entries rather than growing with the
/// session's image history.
const PNG_CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;

/// Least-recently-used first; a hit re-inserts under a fresh sequence number.
struct PngCache {
    entries: BTreeMap<u64, (String, ImageContent)>,
    by_key: BTreeMap<String, u64>,
    bytes: usize,
    next: u64,
}

impl PngCache {
    fn new() -> Self {
        PngCache {
            entries: BTreeMap::new(),
            by_key: BTreeMap::new(),
            bytes: 0,
            next: 0,
        }
    }

    fn touch(&mut self, key: &str) -> Option<ImageContent> {
        let sequence = *self.by_key.get(key)?;
        let Some((_, image)) = self.entries.remove(&sequence) else {
            return None;
        };
        let sequence = self.next;
        self.next += 1;
        self.by_key.insert(key.to_string(), sequence);
        self.entries.insert(sequence, (key.to_string(), image.clone()));
        Some(image)
    }

    fn insert(&mut self, key: String, image: ImageContent) {
        let sequence = self.next;
        self.next += 1;
        self.bytes += image.data.len();
        self.by_key.insert(key.clone(), sequence);
        self.entries.insert(sequence, (key, image));
        while self.bytes > PNG_CACHE_MAX_BYTES && self.entries.len() > 1 {
            let Some((&oldest, _)) = self.entries.iter().next() else {
                break;
            };
            if let Some((key, entry)) = self.entries.remove(&oldest) {
                self.by_key.remove(&key);
                self.bytes -= entry.data.len();
            }
        }
    }
}

static PNG_CACHE: LazyLock<Mutex<PngCache>> = LazyLock::new(|| Mutex::new(PngCache::new()));

/// Content-addressed identity of an image payload, stable across components and
/// transcript rebuilds. Callers key their own per-image state by this instead of
/// positional ids like `${toolCallId}:${index}`, which go stale when the images
/// behind a position are replaced.
pub fn image_payload_key(image: &ImageContent) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(image.data.as_bytes());
    let hash = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{}:{}:{hash}", image.mime_type, image.data.len())
}

/// The PNG conversion of `image` when one is still resident, else `None`.
/// Synchronous so renderers can use an already-converted image on the spot
/// instead of scheduling another async re-render.
pub fn cached_png_conversion(image: &ImageContent) -> Option<ImageContent> {
    let key = image_payload_key(image);
    let mut cache = PNG_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.touch(&key)
}

/// Converts `image` to PNG at most once per distinct payload: later callers hit
/// the shared cache. Kitty-graphics renderers use this because a decode plus
/// re-encode is expensive, and the same image is delivered repeatedly
/// (read-result replay) and rebuilt from scratch on resume, rewind, and `/tree`
/// navigation.
///
/// Only the payload is shared; caller-specific fields (`url`, `detail`)
/// describe the source image, not this PNG. Failures are not cached.
pub fn convert_image_to_png_shared(
    image: &ImageContent,
    encoder: PngEncoder<'_>,
) -> Option<ImageContent> {
    let key = image_payload_key(image);
    if let Some(cached) = cached_png_conversion(image) {
        return Some(cached);
    }
    let converted = convert_image_to_png(image, encoder)?;
    let shared = ImageContent {
        data: converted.data,
        mime_type: converted.mime_type,
        ..ImageContent::default()
    };
    let mut cache = PNG_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.insert(key, shared.clone());
    Some(shared)
}

/// `image` itself when its type needs no conversion, else its PNG conversion.
pub fn ensure_supported_image_input(
    image: &ImageContent,
    encoder: PngEncoder<'_>,
) -> Option<ImageContent> {
    if is_supported_input_image_mime_type(&image.mime_type) {
        return Some(image.clone());
    }
    convert_image_to_png(image, encoder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(mime: &str, data: &str) -> ImageContent {
        ImageContent {
            data: data.to_string(),
            mime_type: mime.to_string(),
            ..ImageContent::default()
        }
    }

    const PNG_1PX: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";

    #[test]
    fn webp_averse_models_are_detected() {
        let traits = |provider: &'static str, api: &'static str, decoder: Option<&'static str>| {
            ModelImageTraits {
                provider,
                api,
                image_input_decoder: decoder,
            }
        };
        assert!(!model_lacks_webp_support(None));
        assert!(model_lacks_webp_support(Some(&traits("ollama", "", None))));
        assert!(model_lacks_webp_support(Some(&traits("lm-studio", "", None))));
        assert!(model_lacks_webp_support(Some(&traits("x", "ollama-chat", None))));
        assert!(model_lacks_webp_support(Some(&traits("x", "", Some("stb")))));
        assert!(!model_lacks_webp_support(Some(&traits("anthropic", "messages", None))));
        assert_eq!(webp_exclusion_for_model(None), None);
        assert_eq!(
            webp_exclusion_for_model(Some(&traits("ollama", "", None))),
            Some(true)
        );
    }

    #[test]
    fn supported_mime_types_match_case_insensitively() {
        assert!(is_supported_input_image_mime_type("image/png"));
        assert!(is_supported_input_image_mime_type("IMAGE/WEBP"));
        assert!(!is_supported_input_image_mime_type("image/svg+xml"));
    }

    #[test]
    fn oversized_images_describe_both_limits() {
        let error = ImageInputTooLargeError {
            bytes: 30 * 1024 * 1024,
            max_bytes: MAX_IMAGE_INPUT_BYTES,
        };
        assert!(error.to_string().contains("exceeds"));
        assert!(error.message().contains("MB"));
    }

    #[test]
    fn invalid_images_name_their_reason() {
        let error = InvalidImageDataError::new("diagram", "image/png", "truncated stream");
        assert_eq!(
            error.message(),
            "diagram is not a decodable image/png image: truncated stream"
        );
        assert_eq!(error.reason, "truncated stream");
    }

    #[test]
    fn png_payloads_decode_and_mismatches_are_reported() {
        assert_eq!(image_decode_failure_reason(&image("image/png", PNG_1PX)), None);
        // Declared JPEG but holds PNG magic.
        let mismatch = image_decode_failure_reason(&image("image/jpeg", PNG_1PX)).unwrap();
        assert!(mismatch.contains("declared image/jpeg but contains image/png"));
        // Not base64 at all.
        assert!(image_decode_failure_reason(&image("image/png", "!!!")).is_some());
    }

    #[test]
    fn truncated_jpegs_are_rejected() {
        // SOI, then APP0 claiming 16 bytes but the stream ends.
        let truncated = base64::engine::general_purpose::STANDARD.encode([0xff, 0xd8, 0xff, 0xe0, 0x00]);
        let reason = image_decode_failure_reason(&image("image/jpeg", &truncated)).unwrap();
        assert!(reason.contains("no EOI marker"));
    }

    #[test]
    fn jpeg_walk_accepts_a_minimal_complete_stream() {
        // SOI, EOI.
        assert!(jpeg_stream_is_complete(&[0xff, 0xd8, 0xff, 0xd9]));
        // SOI then nothing.
        assert!(!jpeg_stream_is_complete(&[0xff, 0xd8, 0xff]));
    }

    #[test]
    fn payload_keys_ignore_position_and_track_content() {
        let first = image("image/png", PNG_1PX);
        let second = image("image/png", PNG_1PX);
        assert_eq!(image_payload_key(&first), image_payload_key(&second));
        assert_ne!(image_payload_key(&first), image_payload_key(&image("image/gif", PNG_1PX)));
    }

    #[test]
    fn conversions_are_shared_once_per_payload() {
        let source = image("image/webp", PNG_1PX);
        let encodes = std::cell::Cell::new(0);
        let encoder = |bytes: &[u8]| {
            encodes.set(encodes.get() + 1);
            Some(bytes.to_vec())
        };
        let first = convert_image_to_png_shared(&source, &encoder).expect("converted");
        assert_eq!(first.mime_type, "image/png");
        let second = convert_image_to_png_shared(&source, &encoder).expect("cached");
        assert_eq!(second.data, first.data);
        assert_eq!(encodes.get(), 1);
        assert!(cached_png_conversion(&source).is_some());
    }

    #[test]
    fn supported_inputs_pass_through_untouched() {
        let png = image("image/png", PNG_1PX);
        let encoder = |_bytes: &[u8]| -> Option<Vec<u8>> { panic!("must not encode") };
        assert_eq!(ensure_supported_image_input(&png, &encoder), Some(png));
    }
}
