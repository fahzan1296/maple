//! Content-addressed binary blobs (images) for `image` nodes.
//!
//! `describe` registers the bytes and puts the returned id in `image.p.blob`;
//! the native backend delivers each referenced blob once per terminal
//! connection, else uploads it with verb `b` before the frame that first
//! references it. The store keeps registered blobs for the process lifetime
//! and owned blobs only while a node's props hold a reference, so a blob is
//! released as soon as nothing describes it.


use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, Props};

/// A content-addressed blob.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeBlob {
    /// Hex SHA-256 of the bytes.
    pub id: String,
    /// MIME type.
    pub mime: String,
    /// The bytes.
    pub bytes: Vec<u8>,
}

impl NativeBlob {
    /// The blob of `bytes`, hashed content-addressed.
    pub fn new(bytes: Vec<u8>, mime: impl Into<String>) -> Self {
        NativeBlob {
            id: blob_id(&bytes),
            mime: mime.into(),
            bytes,
        }
    }
}

/// Hex SHA-256 of `bytes`; the blob id.
pub fn blob_id(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut id = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// Decode standard base64 (padding optional, whitespace ignored).
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if matches!(byte, b'=' | b'\n' | b'\r') {
            continue;
        }
        buffer = (buffer << 6) | u32::from(digit(byte)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// The blob store: registration, ownership reference counts and cleanup.
#[derive(Default)]
pub struct BlobStore {
    blobs: std::collections::HashMap<String, NativeBlob>,
    registered: std::collections::BTreeSet<String>,
    refs: std::collections::HashMap<String, usize>,
    order: std::collections::VecDeque<String>,
}

impl BlobStore {
    /// An empty store.
    pub fn new() -> Self {
        BlobStore {
            blobs: std::collections::HashMap::new(),
            registered: std::collections::BTreeSet::new(),
            refs: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
        }
    }

    /// Register `bytes` for process-lifetime, id-only use.
    pub fn register(&mut self, bytes: &[u8], mime: &str) -> String {
        let id = self.insert(bytes, mime);
        self.registered.insert(id.clone());
        id
    }

    /// The id of `bytes` without registering it.
    pub fn id_of(&self, bytes: &[u8]) -> String {
        blob_id(bytes)
    }

    fn insert(&mut self, bytes: &[u8], mime: &str) -> String {
        let id = blob_id(bytes);
        if let std::collections::hash_map::Entry::Vacant(slot) = self.blobs.entry(id.clone()) {
            slot.insert(NativeBlob {
                id: id.clone(),
                mime: mime.to_string(),
                bytes: bytes.to_vec(),
            });
            self.order.push_back(id.clone());
            self.refs.insert(id.clone(), 0);
        }
        id
    }

    /// A registered blob by id.
    pub fn get(&self, id: &str) -> Option<NativeBlob> {
        self.blobs.get(id).cloned()
    }

    /// Whether `id` is registered for the process lifetime.
    pub fn is_registered(&self, id: &str) -> bool {
        self.registered.contains(id)
    }

    /// Take one ownership reference to `id`; `false` when unknown.
    pub fn retain(&mut self, id: &str) -> bool {
        match self.refs.get_mut(id) {
            Some(count) => {
                *count = count.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// Drop one ownership reference; an owned blob with no references left is
    /// released. Returns whether the id is still live.
    pub fn release(&mut self, id: &str) -> bool {
        let remove = match self.refs.get_mut(id) {
            Some(count) => {
                *count = count.saturating_sub(1);
                *count == 0 && !self.registered.contains(id)
            }
            None => false,
        };
        if remove {
            self.remove(id);
        }
        !remove
    }

    /// Current ownership reference count.
    pub fn retain_count(&self, id: &str) -> usize {
        self.refs.get(id).copied().unwrap_or(0)
    }

    fn remove(&mut self, id: &str) {
        self.blobs.remove(id);
        self.refs.remove(id);
        self.order.retain(|entry| entry.as_str() != id);
    }

    /// Release every owned blob with no references; returns how many went.
    pub fn cleanup(&mut self) -> usize {
        let stale: Vec<String> = self
            .order
            .iter()
            .filter(|id| {
                self.refs.get(*id).copied().unwrap_or(0) == 0 && !self.registered.contains(*id)
            })
            .cloned()
            .collect();
        let count = stale.len();
        for id in stale {
            self.remove(&id);
        }
        count
    }

    /// Number of live blobs, registered and owned.
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// Whether the store holds no blobs.
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }
}

/// Props that name a blob, as an `image` node's `blob` prop.
pub fn blob_props(blob: &NativeBlob) -> Props {
    let mut props = Props::new();
    props.set_str("blob", blob.id.as_str());
    props
}

/// An image whose props own their bytes, including while hidden, unmounted or
/// awaiting upload.
pub fn image_node(
    bytes: &[u8],
    mime: &str,
    props: Props,
    key: Option<&str>,
) -> NativeNode {
    let blob = NativeBlob::new(bytes.to_vec(), mime);
    let mut props = props;
    props.set_str("blob", blob.id.as_str());
    node(NodeKind::Image, props, Vec::new(), key)
}

/// An `image` node for a base64 payload; the pixel size is probed from the
/// header. Decoding and hashing cost real time, so callers cache the node per
/// payload.
pub fn base64_image_node(
    data: &str,
    mime_type: &str,
    props: Props,
    key: Option<&str>,
) -> Option<NativeNode> {
    let bytes = base64_decode(data)?;
    let mut props = props;
    if let Some((width, height)) = image_dimensions(&bytes, mime_type) {
        props.set_i64("w", i64::from(width));
        props.set_i64("h", i64::from(height));
    }
    Some(image_node(&bytes, mime_type, props, key))
}

/// Pixel size from the container header, when the format is recognized.
pub fn image_dimensions(bytes: &[u8], mime: &str) -> Option<(u32, u32)> {
    if mime.starts_with("image/png") {
        return png_dimensions(bytes);
    }
    if mime.starts_with("image/gif") {
        return gif_dimensions(bytes);
    }
    if mime.starts_with("image/jpeg") || mime.starts_with("image/jpg") {
        return jpeg_dimensions(bytes);
    }
    None
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8; 8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != *SIGNATURE {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

fn gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 || !bytes.starts_with(b"GIF8") {
        return None;
    }
    let width = u16::from_le_bytes(bytes[6..8].try_into().ok()?);
    let height = u16::from_le_bytes(bytes[8..10].try_into().ok()?);
    Some((u32::from(width), u32::from(height)))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 9 < bytes.len() {
        if bytes[at] != 0xff {
            at += 1;
            continue;
        }
        let marker = bytes[at + 1];
        if marker == 0xd8 || (0xd0..=0xd9).contains(&marker) {
            at += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if length < 2 {
            return None;
        }
        if matches!(marker, 0xc0 | 0xc1 | 0xc2) {
            let height = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]);
            let width = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]);
            return Some((u32::from(width), u32::from(height)));
        }
        at += 2 + length;
    }
    None
}

/// Percent-encode a path for a `file://` URL.
fn file_url(path: &str) -> String {
    let mut out = String::from("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'/'
            | b':'
            | b'@'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'=' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Props that make an `image` node open the file at `file_path` (always a local
/// `file://` URL, because the open action runs locally).
pub fn openable_image_props(file_path: Option<&str>) -> Props {
    let mut props = Props::new();
    if let Some(path) = file_path {
        if !path.is_empty() {
            props.set_str("href", file_url(path));
            let mut actions = serde_json::Map::new();
            actions.insert(
                "menu".to_string(),
                Value::Array(vec![Value::String("open".to_string())]),
            );
            props.insert("actions", Value::Object(actions));
        }
    }
    props
}

/// The disk file a tool result's image was read from (`read` details
/// `mediaPath`), when it has one.
pub fn read_media_path(details: &Value) -> Option<String> {
    let media_path = details.get("mediaPath")?;
    let path = media_path.as_str()?;
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// A `media` node the host loads by path, capped at `max_rows` lines.
pub fn media_node(path: &str, max_rows: Option<u16>, key: Option<&str>) -> NativeNode {
    let mut props = Props::new();
    props.set_str("path", path);
    if let Some(max_rows) = max_rows {
        if max_rows > 0 {
            props.set_str("max", format!("{max_rows}lines"));
        }
    }
    node(NodeKind::Media, props, Vec::new(), key)
}

/// Blob ids of the `image` nodes and paths of the `media` nodes under
/// `children`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShownMedia {
    /// Referenced blob ids.
    pub blobs: std::collections::BTreeSet<String>,
    /// Referenced file paths.
    pub paths: std::collections::BTreeSet<String>,
}

impl ShownMedia {
    /// Whether nothing is referenced.
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty() && self.paths.is_empty()
    }
}

/// Collect the media a described tree references.
pub fn shown_media(children: &[NativeNode]) -> ShownMedia {
    let mut shown = ShownMedia::default();
    visit_media(children, &mut shown);
    shown
}

fn visit_media(children: &[NativeNode], shown: &mut ShownMedia) {
    for child in children {
        match child.kind {
            NodeKind::Image => {
                if let Some(blob) = child.prop("blob").and_then(Value::as_str) {
                    shown.blobs.insert(blob.to_string());
                }
            }
            NodeKind::Media => {
                if let Some(path) = child.prop("path").and_then(Value::as_str) {
                    shown.paths.insert(path.to_string());
                }
            }
            _ => {}
        }
        visit_media(&child.children, shown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&[0, 0, 0, 0, b'I', b'H', b'D', b'R']);
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn blob_id_is_sha256_hex() {
        assert_eq!(blob_id(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(blob_id(b"a"), blob_id(b"a"));
        assert_ne!(blob_id(b"a"), blob_id(b"b"));
        assert_eq!(blob_id(b"abc").len(), 64);
    }

    #[test]
    fn registration_refcounting_and_cleanup() {
        let mut store = BlobStore::new();
        let registered = store.register(b"registered", "image/png");
        assert!(store.is_registered(&registered));
        assert_eq!(store.retain_count(&registered), 0);

        let id = store.insert(b"owned", "image/png");
        assert!(!store.is_registered(&id));
        assert!(store.retain(&id));
        assert!(store.retain(&id));
        assert_eq!(store.retain_count(&id), 2);
        assert!(store.release(&id));
        assert_eq!(store.len(), 2);
        assert!(!store.release(&id));
        assert_eq!(store.len(), 1);
        assert!(store.get(&id).is_none());

        assert_eq!(store.cleanup(), 0);
        assert_eq!(store.len(), 1);
        assert!(store.get(&registered).is_some());
    }

    #[test]
    fn cleanup_drops_unreferenced_owned_blobs() {
        let mut store = BlobStore::new();
        let a = store.register(b"a", "image/png");
        let b = store.insert(b"b", "image/png");
        assert_eq!(store.cleanup(), 1);
        assert!(store.get(&b).is_none());
        assert!(store.get(&a).is_some());
    }

    #[test]
    fn image_node_carries_blob_id() {
        let node = image_node(b"bytes", "image/png", Props::new(), Some("shot"));
        assert_eq!(node.kind, NodeKind::Image);
        assert_eq!(node.key.as_deref(), Some("shot"));
        assert_eq!(node.prop("blob").and_then(Value::as_str), Some(blob_id(b"bytes").as_str()));
    }
    /// Standard base64 with padding, for building fixtures.
    fn base64_encode(data: &[u8]) -> String {
        const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let n = (chunk[0] as u32) << 16
                | (*chunk.get(1).unwrap_or(&0) as u32) << 8
                | (*chunk.get(2).unwrap_or(&0) as u32);
            out.push(DIGITS[((n >> 18) & 63) as usize] as char);
            out.push(DIGITS[((n >> 12) & 63) as usize] as char);
            if chunk.len() > 1 {
                out.push(DIGITS[((n >> 6) & 63) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(DIGITS[(n & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }


    #[test]
    fn base64_image_node_probes_png_size() {
        let payload = base64_encode(&png(320, 240));
        let node = base64_image_node(&payload, "image/png", Props::new(), None).unwrap();
        assert_eq!(node.prop("w"), Some(&Value::Number(320.into())));
        assert_eq!(node.prop("h"), Some(&Value::Number(240.into())));
        assert!(base64_image_node("!!!", "image/png", Props::new(), None).is_none());
    }

    #[test]
    fn openable_props_and_media_paths() {
        let props = openable_image_props(Some("/tmp/a b.png"));
        assert_eq!(props.get_str("href"), Some("file:///tmp/a%20b.png"));
        assert_eq!(props.get("actions").unwrap()["menu"][0], "open");
        assert!(openable_image_props(None).is_empty());
        assert!(openable_image_props(Some("")).is_empty());

        let details = serde_json::json!({"mediaPath": "/tmp/x.png"});
        assert_eq!(read_media_path(&details).as_deref(), Some("/tmp/x.png"));
        assert_eq!(read_media_path(&serde_json::json!({"mediaPath": ""})), None);
        assert_eq!(read_media_path(&serde_json::json!({})), None);
    }

    #[test]
    fn shown_media_collects_blobs_and_paths() {
        let image = image_node(b"img", "image/png", Props::new(), None);
        let media = media_node("/tmp/a.png", Some(4), None);
        let tree = crate::native::describe::col(vec![image, media], Props::new());
        let shown = shown_media(&[tree]);
        assert_eq!(shown.blobs.len(), 1);
        assert_eq!(shown.paths.len(), 1);
        assert!(shown.paths.contains("/tmp/a.png"));
        assert!(ShownMedia::default().is_empty());
    }
}
