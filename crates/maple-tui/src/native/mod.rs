//! Tern Surface Protocol (TSP) native rendering.
//! Reference: `l/src/native/index.ts`.
//!
//! The surface is described as [`node::NativeNode`] trees, framed by
//! [`encode`], diffed into frame ops by [`state`], and backed by content-
//! addressed images in [`blobs`]. Components describe themselves through
//! [`describe`].
//!
//! The module is a clean-room port of `l/src/native`; the wire format and
//! operation semantics follow `docs/recon/chrome.md` §6.

/// Terminal-side surface backend that applies described frames.
pub mod backend;

/// Content-addressed image blobs and their store.
pub mod blobs;
/// Node-tree builders and the `NativeDescribe` component contract.
pub mod describe;
/// TSP framing: encode, chunk, decode.
pub mod encode;
/// The `NativeNode` model, kinds, props and describe context.
pub mod node;
/// Owner-keyed memoization for describe output.
pub mod memo;
/// Overlay sheets: cards, action bars, buttons, key hints.
pub mod overlay;
/// Selection pickers and their sheet wrapper.
pub mod picker;
/// Quiet-mode reduction of terminal-clocked animations.
pub mod quiet;
/// Theme colour tokens ↔ TSP tones.
pub mod tone;
/// PUA icon glyph runs normalized into `icon` spans.
pub mod icons;
/// Row estimation for described subtrees.
pub mod measure;
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
pub use quiet::{quiet_motion, QuietSwitches};
pub use tone::{
    background_chrome, color_tone, color_tone_token, sample_background, sample_foreground,
    tone_color, BackgroundChrome,
};
pub use icons::{normalize_icon_props, normalize_icon_spans, normalize_icon_text};
pub use measure::{estimate_rows, DescribedOf, MeasureChild};
pub use memo::OwnerMemo;
pub use state::{
    escape_segment, native_state, same_items, same_props, unescape_segment, ApplyError, Frame,
    FrameOp, Memo, NativeCounters, NativeReconciler, NativeSettle, NativeState, SettleKey,
    TextMode, TspDocument, WireNode,
};
