//! Tern Surface Protocol (TSP) native rendering.
//!
//! The surface is described as [`node::NativeNode`] trees, framed by
//! [`encode`], diffed into frame ops by [`state`], and backed by content-
//! addressed images in [`blobs`]. Components describe themselves through
//! [`describe`].
//!
//! The module is a clean-room port of `l/src/native`; the wire format and
//! operation semantics follow `docs/recon/chrome.md` §6.

/// Content-addressed image blobs and their store.
pub mod blobs;
/// Node-tree builders and the `NativeDescribe` component contract.
pub mod describe;
/// TSP framing: encode, chunk, decode.
pub mod encode;
/// The `NativeNode` model, kinds, props and describe context.
pub mod node;
/// ANSI-styled text → escape-free spans and plain text.
pub mod spans;
/// Native state, settle memo, bounded reconciler and document applier.
pub mod state;

pub use blobs::{
    base64_image_node, blob_id, image_dimensions, image_node, media_node, openable_image_props,
    read_media_path, shown_media, BlobStore, NativeBlob, ShownMedia,
};
pub use describe::{
    ansi, card, code, col, compact, diff, elapsed, empty_node, item, kbd, keyed, key_names, kv,
    list, md, node, prop_node, row, shimmer, span, stable_key, table, text, tree, with_hidden,
    NativeDescribe, ShimmerPalette, CURSOR_MARKER,
};
pub use encode::{
    decode_body, encode_hello_query, encode_json, encode_json_with_limit, encode_message,
    encode_message_with_limit, parse_message, split_message, try_decode_body, try_split_message,
    FrameError, Hello, Incoming, RawMessage, Reply, Event, TspEncoder, TspParams, TspReader,
    Verb, APC, ST, TSP_APC_ID, TSP_DEFAULT_APC_LIMIT, TSP_DEFAULT_CREDITS, TSP_OSC_PREFIX,
    TSP_PREFIX, TSP_VERSION,
};
pub use node::{
    DescribeContext, NativeNode, NativeScreen, NativeSurface, NodeKind, Props, Reveal, RevealAt,
    Scroll, ScrollBy, Span, TableColumn, TableRow, Text, Tone, TreeNode,
};
pub use spans::{
    compact_text, plain_line, plain_text, rows_text, spans_line, style_spans, styled_background,
    styled_spans,
};
pub use state::{
    escape_segment, native_state, same_items, same_props, unescape_segment, ApplyError, Frame,
    FrameOp, Memo, NativeCounters, NativeReconciler, NativeSettle, NativeState, SettleKey,
    TextMode, TspDocument, WireNode,
};
