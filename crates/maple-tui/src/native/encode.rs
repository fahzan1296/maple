//! Tern Surface Protocol framing: `ESC _ tsp ; <verb> [;k=v]* ; <body> ESC \`.
//!
//! Bodies are UTF-8 JSON (base64 for blobs). A body larger than the APC limit
//! is chunked: the first chunk carries the parameters, every chunk carries
//! `c=<id>`, and all but the last carry `m=1`. Cuts never split a UTF-8 code
//! point and never leave the next chunk starting with a `key=value;` segment a
//! receiver would read as one more parameter.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value};

/// Protocol version this build speaks.
pub const TSP_VERSION: u64 = 1;
/// APC identifier: every TSP message body starts with `tsp;`.
pub const TSP_APC_ID: &str = "tsp";
/// Largest APC body a sender emits before chunking unless the hello reply says
/// otherwise.
pub const TSP_DEFAULT_APC_LIMIT: usize = 65_536;
/// Unacknowledged frames a sender may have in flight unless the hello reply
/// says otherwise.
pub const TSP_DEFAULT_CREDITS: u32 = 2;
/// APC introducer.
pub const APC: &str = "\x1b_";
/// ST terminator.
pub const ST: &str = "\x1b\\";
/// Every TSP message starts with this.
pub const TSP_PREFIX: &str = "\x1b_tsp;";
/// TSP input framing through ConPTY, whose input parser discards APC.
pub const TSP_OSC_PREFIX: &str = "\x1b]877;tsp;";

/// One-letter message verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verb {
    /// Query (`q`).
    Query,
    /// Open surface (`o`).
    Open,
    /// Frame (`f`).
    Frame,
    /// Blob (`b`).
    Blob,
    /// Close surface (`x`).
    Close,
    /// Reply (`r`).
    Reply,
    /// Event (`e`).
    Event,
}

impl Verb {
    /// The wire character.
    pub fn as_char(self) -> char {
        match self {
            Verb::Query => 'q',
            Verb::Open => 'o',
            Verb::Frame => 'f',
            Verb::Blob => 'b',
            Verb::Close => 'x',
            Verb::Reply => 'r',
            Verb::Event => 'e',
        }
    }

    /// Parse a wire character.
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'q' => Some(Verb::Query),
            'o' => Some(Verb::Open),
            'f' => Some(Verb::Frame),
            'b' => Some(Verb::Blob),
            'x' => Some(Verb::Close),
            'r' => Some(Verb::Reply),
            'e' => Some(Verb::Event),
            _ => None,
        }
    }
}

/// Message parameters (`k=v`, ASCII, never containing `;`).
pub type TspParams = Vec<(String, String)>;

/// One decoded APC message: verb, parameters and raw body.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMessage {
    /// Message verb.
    pub verb: String,
    /// Parameters in wire order.
    pub params: TspParams,
    /// Raw body.
    pub body: String,
}

impl RawMessage {
    /// The `c` (chunk id) parameter, when present.
    pub fn chunk_id(&self) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == "c")
            .map(|(_, value)| value.as_str())
    }

    /// Whether the `m=1` (more) parameter is present.
    pub fn more(&self) -> bool {
        self.params
            .iter()
            .any(|(key, value)| key == "m" && value == "1")
    }
}

/// The terminal's hello reply.
#[derive(Clone, Debug, PartialEq)]
pub struct Hello {
    /// Protocol version.
    pub v: i64,
    /// Terminal name.
    pub term: String,
    /// Supported kinds.
    pub kinds: Vec<String>,
    /// Advertised protocol features.
    pub features: Vec<String>,
    /// APC limit override.
    pub apc: Option<u32>,
    /// Credit override.
    pub credits: Option<u32>,
    /// Column count.
    pub cols: Option<u16>,
    /// Dark appearance.
    pub dark: Option<bool>,
    /// Reduced motion.
    pub reduce_motion: Option<bool>,
    /// 12-hour clock preference.
    pub hour12: Option<bool>,
    /// Terminal version.
    pub ver: Option<String>,
    /// Unknown fields, preserved.
    pub extra: Map<String, Value>,
}

/// A decoded terminal reply.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// Hello reply.
    Hello(Hello),
    /// Blob cache reply.
    Blobs {
        /// Ids the terminal already holds.
        have: Vec<String>,
        /// Unknown fields, preserved.
        extra: Map<String, Value>,
    },
}

impl Reply {
    /// The reply tag (`hello` or `blobs`).
    pub fn tag(&self) -> &'static str {
        match self {
            Reply::Hello(_) => "hello",
            Reply::Blobs { .. } => "blobs",
        }
    }

    /// The hello reply, when this is one.
    pub fn hello(&self) -> Option<&Hello> {
        match self {
            Reply::Hello(hello) => Some(hello),
            Reply::Blobs { .. } => None,
        }
    }

    /// Serialize to the wire reply object.
    pub fn to_value(&self) -> Value {
        match self {
            Reply::Hello(hello) => {
                let mut out = hello.extra.clone();
                out.insert("r".to_string(), Value::String("hello".to_string()));
                out.insert("v".to_string(), Value::Number(hello.v.into()));
                out.insert("term".to_string(), Value::String(hello.term.clone()));
                out.insert(
                    "kinds".to_string(),
                    Value::Array(hello.kinds.iter().cloned().map(Value::String).collect()),
                );
                if !hello.features.is_empty() {
                    out.insert(
                        "features".to_string(),
                        Value::Array(
                            hello.features.iter().cloned().map(Value::String).collect(),
                        ),
                    );
                }
                let optional: [(&str, Option<Value>); 7] = [
                    (
                        "apc",
                        hello.apc.map(|value| Value::Number(u64::from(value).into())),
                    ),
                    (
                        "credits",
                        hello.credits.map(|value| Value::Number(u64::from(value).into())),
                    ),
                    (
                        "cols",
                        hello.cols.map(|value| Value::Number(u64::from(value).into())),
                    ),
                    ("dark", hello.dark.map(Value::Bool)),
                    ("reduceMotion", hello.reduce_motion.map(Value::Bool)),
                    ("hour12", hello.hour12.map(Value::Bool)),
                    ("ver", hello.ver.clone().map(Value::String)),
                ];
                for (key, value) in optional {
                    if let Some(value) = value {
                        out.insert(key.to_string(), value);
                    }
                }
                Value::Object(out)
            }
            Reply::Blobs { have, extra } => {
                let mut out = extra.clone();
                out.insert("r".to_string(), Value::String("blobs".to_string()));
                out.insert(
                    "have".to_string(),
                    Value::Array(have.iter().cloned().map(Value::String).collect()),
                );
                Value::Object(out)
            }
        }
    }
}

/// A decoded terminal event. Unknown fields are preserved verbatim.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    value: Map<String, Value>,
}

impl Event {
    /// The event name (`ack`, `toggle`, `edit`, …).
    pub fn ev(&self) -> &str {
        self.value.get("ev").and_then(Value::as_str).unwrap_or("")
    }

    /// Borrow a field.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.value.get(key)
    }

    /// Borrow a string field.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.value.get(key).and_then(Value::as_str)
    }

    /// Borrow a bool field.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.value.get(key).and_then(Value::as_bool)
    }

    /// Borrow a numeric field.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.value.get(key).and_then(Value::as_i64)
    }

    /// Serialize to the wire event object.
    pub fn to_value(&self) -> Value {
        Value::Object(self.value.clone())
    }
}

/// A decoded terminal → program message.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// A reply.
    Reply {
        /// Verb (`r`).
        verb: Verb,
        /// The reply.
        reply: Reply,
    },
    /// An event.
    Event {
        /// Verb (`e`).
        verb: Verb,
        /// The event.
        event: Event,
    },
}

impl Incoming {
    /// The verb this message arrived with.
    pub fn verb(&self) -> Verb {
        match self {
            Incoming::Reply { verb, .. } => *verb,
            Incoming::Event { verb, .. } => *verb,
        }
    }

    /// The event, when this is one.
    pub fn event(&self) -> Option<&Event> {
        match self {
            Incoming::Event { event, .. } => Some(event),
            Incoming::Reply { .. } => None,
        }
    }

    /// The reply, when this is one.
    pub fn reply(&self) -> Option<&Reply> {
        match self {
            Incoming::Reply { reply, .. } => Some(reply),
            Incoming::Event { .. } => None,
        }
    }
}

/// A framing or decoding failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The sequence is not a TSP APC message.
    NotTsp,
    /// The sequence does not end with ST.
    MissingTerminator,
    /// The verb is empty.
    EmptyVerb,
    /// A parameter segment is malformed.
    BadParam,
    /// The body is not valid JSON.
    BadJson,
    /// A required field is missing or has the wrong type.
    MissingField(&'static str),
    /// The verb is not a TSP verb.
    UnexpectedVerb(char),
    /// The event name is not in the vocabulary.
    UnknownEvent(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::NotTsp => write!(f, "not a TSP message"),
            FrameError::MissingTerminator => write!(f, "TSP message is missing ST"),
            FrameError::EmptyVerb => write!(f, "TSP message has an empty verb"),
            FrameError::BadParam => write!(f, "TSP parameter is malformed"),
            FrameError::BadJson => write!(f, "TSP body is not valid JSON"),
            FrameError::MissingField(field) => write!(f, "TSP body is missing {field}"),
            FrameError::UnexpectedVerb(verb) => write!(f, "unknown TSP verb {verb:?}"),
            FrameError::UnknownEvent(event) => write!(f, "unknown TSP event {event:?}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Whether `segment` is a `key=value` parameter (no `;` in the value).
fn is_param(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let mut eq = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'=' {
            eq = Some(index);
            break;
        }
        if !byte.is_ascii_alphanumeric() && *byte != b'_' && *byte != b'-' {
            return false;
        }
    }
    let eq = match eq {
        Some(eq) => eq,
        None => return false,
    };
    if eq == 0 {
        return false;
    }
    bytes[eq + 1..]
        .iter()
        .all(|byte| (0x21..=0x3a).contains(byte) || (0x3c..=0x7e).contains(byte))
}

/// Split a complete `ESC _ tsp;… ESC \` string; `None` when it isn't one.
pub fn split_message(sequence: &str) -> Option<RawMessage> {
    try_split_message(sequence).ok()
}

/// Split a complete `ESC _ tsp;… ESC \` string, reporting framing failures.
pub fn try_split_message(sequence: &str) -> Result<RawMessage, FrameError> {
    if !sequence.starts_with(TSP_PREFIX) {
        return Err(FrameError::NotTsp);
    }
    if !sequence.ends_with(ST) {
        return Err(FrameError::MissingTerminator);
    }
    let inner = &sequence[TSP_PREFIX.len()..sequence.len() - ST.len()];
    let semi = match inner.find(';') {
        Some(semi) => semi,
        None => {
            if inner.is_empty() {
                return Err(FrameError::EmptyVerb);
            }
            return Ok(RawMessage {
                verb: inner.to_string(),
                params: Vec::new(),
                body: String::new(),
            });
        }
    };
    if semi == 0 {
        return Err(FrameError::EmptyVerb);
    }
    let verb = inner[..semi].to_string();
    let mut params = Vec::new();
    let mut pos = semi + 1;
    loop {
        let next = match inner[pos..].find(';') {
            Some(next) => pos + next,
            None => break,
        };
        let segment = &inner[pos..next];
        if !is_param(segment) {
            break;
        }
        let eq = segment.find('=').unwrap_or(segment.len());
        params.push((segment[..eq].to_string(), segment[eq + 1..].to_string()));
        pos = next + 1;
    }
    Ok(RawMessage {
        verb,
        params,
        body: inner[pos..].to_string(),
    })
}

/// Decode an unchunked reply/event body; `None` for malformed input.
pub fn decode_body(verb: &str, body: &str) -> Option<Incoming> {
    try_decode_body(verb, body).ok()
}

/// Decode an unchunked reply/event body, reporting failures.
pub fn try_decode_body(verb: &str, body: &str) -> Result<Incoming, FrameError> {
    let wire = verb.chars().next().ok_or(FrameError::EmptyVerb)?;
    let parsed = Verb::from_char(wire).ok_or(FrameError::UnexpectedVerb(wire))?;
    let value: Value = serde_json::from_str(body).map_err(|_| FrameError::BadJson)?;
    let object = value.as_object().ok_or(FrameError::MissingField("object"))?;
    match parsed {
        Verb::Reply => {
            let reply = decode_reply(object).ok_or(FrameError::MissingField("reply"))?;
            Ok(Incoming::Reply {
                verb: Verb::Reply,
                reply,
            })
        }
        Verb::Event => {
            let event = decode_event(object).ok_or_else(|| {
                FrameError::UnknownEvent(
                    object
                        .get("ev")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )
            })?;
            Ok(Incoming::Event {
                verb: Verb::Event,
                event,
            })
        }
        _ => Err(FrameError::UnexpectedVerb(wire)),
    }
}

/// Decode one complete, unchunked terminal → program message.
pub fn parse_message(sequence: &str) -> Option<Incoming> {
    let raw = split_message(sequence)?;
    if raw.more() {
        return None;
    }
    decode_body(&raw.verb, &raw.body)
}

fn is_string_array(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().all(Value::is_string))
}

fn decode_reply(value: &Map<String, Value>) -> Option<Reply> {
    match value.get("r")?.as_str()? {
        "hello" => {
            let v = value.get("v")?.as_i64()?;
            let term = value.get("term")?.as_str()?.to_string();
            if !is_string_array(value.get("kinds")) {
                return None;
            }
            let kinds = value
                .get("kinds")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            let features = value
                .get("features")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let known = [
                "r",
                "v",
                "term",
                "kinds",
                "features",
                "apc",
                "credits",
                "cols",
                "dark",
                "reduceMotion",
                "hour12",
                "ver",
            ];
            let mut extra = Map::new();
            for (key, item) in value {
                if !known.contains(&key.as_str()) {
                    extra.insert(key.clone(), item.clone());
                }
            }
            Some(Reply::Hello(Hello {
                v,
                term,
                kinds,
                features,
                apc: value.get("apc").and_then(Value::as_u64).map(|v| v as u32),
                credits: value
                    .get("credits")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32),
                cols: value.get("cols").and_then(Value::as_u64).map(|v| v as u16),
                dark: value.get("dark").and_then(Value::as_bool),
                reduce_motion: value.get("reduceMotion").and_then(Value::as_bool),
                hour12: value.get("hour12").and_then(Value::as_bool),
                ver: value
                    .get("ver")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                extra,
            }))
        }
        "blobs" => {
            if !is_string_array(value.get("have")) {
                return None;
            }
            let have = value
                .get("have")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            let mut extra = Map::new();
            for (key, item) in value {
                if key != "r" && key != "have" {
                    extra.insert(key.clone(), item.clone());
                }
            }
            Some(Reply::Blobs { have, extra })
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum FieldKind {
    Str,
    Num,
    Bool,
    StrArray,
}

fn required_fields(ev: &str) -> Option<&'static [(&'static str, FieldKind)]> {
    Some(match ev {
        "ack" => &[("sf", FieldKind::Str), ("s", FieldKind::Num)],
        "resize" => &[("cols", FieldKind::Num)],
        "theme" => &[("dark", FieldKind::Bool)],
        "motion" => &[("reduce", FieldKind::Bool)],
        "visible" => &[("visible", FieldKind::Bool)],
        "toggle" => &[
            ("sf", FieldKind::Str),
            ("id", FieldKind::Str),
            ("collapsed", FieldKind::Bool),
        ],
        "select" => &[("sf", FieldKind::Str), ("id", FieldKind::Str), ("item", FieldKind::Str)],
        "activate" => &[("sf", FieldKind::Str), ("id", FieldKind::Str), ("item", FieldKind::Str)],
        "action" => &[("sf", FieldKind::Str), ("id", FieldKind::Str), ("act", FieldKind::Str)],
        "change" => &[("sf", FieldKind::Str), ("id", FieldKind::Str), ("item", FieldKind::Str)],
        "edit" => &[
            ("sf", FieldKind::Str),
            ("id", FieldKind::Str),
            ("from", FieldKind::Num),
            ("to", FieldKind::Num),
            ("text", FieldKind::Str),
            ("cursor", FieldKind::Num),
            ("len", FieldKind::Num),
        ],
        "undo" => &[("sf", FieldKind::Str), ("id", FieldKind::Str)],
        "focus" => &[("sf", FieldKind::Str), ("id", FieldKind::Str)],
        "error" => &[("msg", FieldKind::Str)],
        "gone" => &[("ids", FieldKind::StrArray)],
        _ => return None,
    })
}

fn field_matches(value: Option<&Value>, kind: FieldKind) -> bool {
    match kind {
        FieldKind::Str => value.is_some_and(Value::is_string),
        FieldKind::Num => value.is_some_and(Value::is_number),
        FieldKind::Bool => value.is_some_and(Value::is_boolean),
        FieldKind::StrArray => is_string_array(value),
    }
}

fn decode_event(value: &Map<String, Value>) -> Option<Event> {
    let ev = value.get("ev")?.as_str()?.to_string();
    if ev == "send" {
        let sf = value.get("sf").and_then(Value::as_str).unwrap_or("");
        let id = value.get("id").and_then(Value::as_str).unwrap_or("");
        let text = value.get("text")?.as_str()?;
        if sf.is_empty() || id.is_empty() {
            return None;
        }
        let _ = text;
        return Some(Event {
            value: value.clone(),
        });
    }
    let fields = required_fields(&ev)?;
    if fields
        .iter()
        .all(|(key, kind)| field_matches(value.get(*key), *kind))
    {
        Some(Event {
            value: value.clone(),
        })
    } else {
        None
    }
}

/// Reassembles chunked terminal → program messages and decodes replies and
/// events. Unknown verbs, unknown events and unknown fields are tolerated;
/// malformed input yields `None`.
#[derive(Default)]
pub struct TspReader {
    chunks: HashMap<String, String>,
}

impl TspReader {
    /// An empty reader.
    pub fn new() -> Self {
        TspReader {
            chunks: HashMap::new(),
        }
    }

    /// Feed one complete APC string; the decoded message once complete.
    pub fn feed(&mut self, sequence: &str) -> Option<Incoming> {
        let raw = split_message(sequence)?;
        let chunk_id = raw.chunk_id().map(str::to_string);
        let more = raw.more();
        let verb = raw.verb;
        let mut body = raw.body;
        if let Some(chunk_id) = chunk_id {
            let joined = self.chunks.get(&chunk_id).cloned().unwrap_or_default() + body.as_str();
            if more {
                self.chunks.insert(chunk_id.to_string(), joined);
                return None;
            }
            self.chunks.remove(&chunk_id);
            body = joined;
        }
        decode_body(&verb, &body)
    }

    /// Forget any pending chunks (a new connection).
    pub fn reset(&mut self) {
        self.chunks.clear();
    }

    /// Number of chunks waiting for their final piece.
    pub fn pending_chunks(&self) -> usize {
        self.chunks.len()
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Whether `bytes` from `at` leads with a `key=value;` segment.
fn leads_with_parameter(bytes: &[u8], at: usize) -> bool {
    let mut i = at;
    while i < bytes.len() {
        let byte = bytes[i];
        if !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
            break;
        }
        i += 1;
    }
    if i == at || i >= bytes.len() || bytes[i] != b'=' {
        return false;
    }
    i += 1;
    while i < bytes.len() && (0x21..=0x7e).contains(&bytes[i]) && bytes[i] != b';' {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b';'
}

/// The safe cut nearest below `start + limit`, else the nearest above.
fn chunk_end(bytes: &[u8], start: usize, limit: usize) -> usize {
    let target = start + limit;
    if target >= bytes.len() {
        return bytes.len();
    }
    let safe = |at: usize| -> bool { (bytes[at] & 0xc0) != 0x80 && !leads_with_parameter(bytes, at) };
    let mut at = target;
    while at > start {
        if safe(at) {
            return at;
        }
        at -= 1;
    }
    let mut at = target + 1;
    while at < bytes.len() {
        if safe(at) {
            return at;
        }
        at += 1;
    }
    bytes.len()
}

/// Chunk bodies for `body`, cut the way Tern's `tsp_chunks` cuts.
fn chunk_bodies(body: &str, limit: usize) -> Vec<String> {
    let max = limit.max(1);
    let mut pieces = Vec::new();
    // Pure ASCII without `;` (every base64 blob) has no parameter-shaped
    // segment and no multibyte character, so it is cut every `limit` bytes.
    if body.is_ascii() && !body.contains(';') {
        let mut at = 0;
        while at < body.len() {
            let end = (at + max).min(body.len());
            pieces.push(body[at..end].to_string());
            at = end;
        }
        return pieces;
    }
    let bytes = body.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let end = chunk_end(bytes, start, max);
        let piece = String::from_utf8_lossy(&bytes[start..end]).into_owned();
        pieces.push(piece);
        start = end;
    }
    pieces
}

fn encode_params(params: &TspParams) -> String {
    let mut out = String::new();
    for (key, value) in params {
        out.push(';');
        out.push_str(key);
        out.push('=');
        out.push_str(value);
    }
    out
}

fn frame(verb: Verb, params: &str, body: &str) -> String {
    let mut out = String::with_capacity(TSP_PREFIX.len() + body.len() + ST.len() + 8);
    out.push_str(TSP_PREFIX);
    out.push(verb.as_char());
    out.push_str(params);
    out.push(';');
    out.push_str(body);
    out.push_str(ST);
    out
}

/// Format `value` in base 36 (lowercase, no sign), as the wire does.
fn base36(mut value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

static NEXT_CHUNK_ID: AtomicU64 = AtomicU64::new(1);

fn next_chunk_id() -> u64 {
    NEXT_CHUNK_ID.fetch_add(1, Ordering::Relaxed)
}

/// Encode one logical message, chunked when `body` exceeds the APC limit.
pub fn encode_message(verb: Verb, body: &str, params: &TspParams) -> String {
    encode_message_with_limit(verb, body, params, TSP_DEFAULT_APC_LIMIT)
}

/// Encode one logical message with an explicit APC limit.
pub fn encode_message_with_limit(verb: Verb, body: &str, params: &TspParams, limit: usize) -> String {
    let base = encode_params(params);
    if utf16_len(body).saturating_mul(3) <= limit || body.len() <= limit {
        return frame(verb, &base, body);
    }
    let chunk_id = base36(next_chunk_id());
    let pieces = chunk_bodies(body, limit);
    let mut out = String::new();
    let last = pieces.len().saturating_sub(1);
    for (index, piece) in pieces.iter().enumerate() {
        let more = if index < last { ";m=1" } else { "" };
        let head = if index == 0 { base.as_str() } else { "" };
        let params = format!("{head};c={chunk_id}{more}");
        out.push_str(&frame(verb, &params, piece));
    }
    out
}

/// Encode a JSON message body.
pub fn encode_json(verb: Verb, value: &Value, params: &TspParams) -> String {
    encode_json_with_limit(verb, value, params, TSP_DEFAULT_APC_LIMIT)
}

/// Encode a JSON message body with an explicit APC limit.
pub fn encode_json_with_limit(verb: Verb, value: &Value, params: &TspParams, limit: usize) -> String {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    encode_message_with_limit(verb, &body, params, limit)
}

/// The `hello` query; callers follow it with a DA1 sentinel.
pub fn encode_hello_query(version: Option<&str>) -> String {
    let mut value = Map::new();
    value.insert("q".to_string(), Value::String("hello".to_string()));
    value.insert(
        "v".to_string(),
        Value::Array(vec![Value::Number(TSP_VERSION.into())]),
    );
    value.insert("app".to_string(), Value::String("omp".to_string()));
    value.insert(
        "features".to_string(),
        Value::Array(
            ["edit", "undo", "send"]
                .iter()
                .map(|feature| Value::String((*feature).to_string()))
                .collect(),
        ),
    );
    if let Some(version) = version {
        value.insert("ver".to_string(), Value::String(version.to_string()));
    }
    encode_json(Verb::Query, &Value::Object(value), &TspParams::new())
}

/// A stateful encoder: chunk ids increase across messages on one connection.
#[derive(Clone, Debug)]
pub struct TspEncoder {
    next_chunk_id: u64,
}

impl Default for TspEncoder {
    fn default() -> Self {
        TspEncoder::new()
    }
}

impl TspEncoder {
    /// An encoder whose first chunk id is 1.
    pub fn new() -> Self {
        TspEncoder { next_chunk_id: 1 }
    }

    /// An encoder starting at an explicit chunk id.
    pub fn with_start_id(start: u64) -> Self {
        TspEncoder {
            next_chunk_id: start,
        }
    }

    /// The next chunk id this encoder will hand out.
    pub fn next_id(&self) -> u64 {
        self.next_chunk_id
    }

    /// Encode one logical message under the default APC limit.
    pub fn encode_message(&mut self, verb: Verb, body: &str, params: &TspParams) -> String {
        self.encode_message_with_limit(verb, body, params, TSP_DEFAULT_APC_LIMIT)
    }

    /// Encode one logical message with an explicit APC limit.
    pub fn encode_message_with_limit(
        &mut self,
        verb: Verb,
        body: &str,
        params: &TspParams,
        limit: usize,
    ) -> String {
        let base = encode_params(params);
        if utf16_len(body).saturating_mul(3) <= limit || body.len() <= limit {
            return frame(verb, &base, body);
        }
        let chunk_id = base36(self.next_chunk_id);
        self.next_chunk_id = self.next_chunk_id.wrapping_add(1);
        let pieces = chunk_bodies(body, limit);
        let mut out = String::new();
        let last = pieces.len().saturating_sub(1);
        for (index, piece) in pieces.iter().enumerate() {
            let more = if index < last { ";m=1" } else { "" };
            let head = if index == 0 { base.as_str() } else { "" };
            let params = format!("{head};c={chunk_id}{more}");
            out.push_str(&frame(verb, &params, piece));
        }
        out
    }

    /// Encode a JSON message body.
    pub fn encode_json(&mut self, verb: Verb, value: &Value, params: &TspParams) -> String {
        let body = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
        self.encode_message(verb, &body, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(stream: &str) -> Vec<String> {
        stream
            .split(ST)
            .filter(|part| !part.is_empty())
            .map(|part| format!("{part}{ST}"))
            .collect()
    }

    fn split_all(stream: &str) -> Vec<RawMessage> {
        messages(stream)
            .iter()
            .map(|message| split_message(message).unwrap())
            .collect()
    }

    #[test]
    fn hello_query_matches_the_wire_shape() {
        let raw = split_message(&encode_hello_query(Some("test"))).unwrap();
        assert_eq!(raw.verb, "q");
        let body: Value = serde_json::from_str(&raw.body).unwrap();
        assert_eq!(body["q"], "hello");
        assert_eq!(body["v"], serde_json::json!([1]));
        assert_eq!(body["app"], "omp");
        assert_eq!(
            body["features"],
            serde_json::json!(["edit", "undo", "send"])
        );
        assert_eq!(body["ver"], "test");
    }

    #[test]
    fn body_at_the_limit_is_one_frame() {
        let body = "x".repeat(40);
        assert_eq!(
            encode_message_with_limit(Verb::Frame, &body, &TspParams::new(), 40),
            format!("{TSP_PREFIX}f;{body}{ST}")
        );
    }

    #[test]
    fn conformance_chunking_cuts_safely_and_reassembles() {
        let body = "{\"sf\":\"s1\",\"s\":9,\"ops\":[[\"text\",\"log\",\"append\",\"0123456789abcdefghij\"]]}";
        let stream = encode_message_with_limit(Verb::Frame, body, &TspParams::new(), 24);
        let chunks = split_all(&stream);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.verb == "f"));
        let id = chunks[0].chunk_id().unwrap().to_string();
        assert!(chunks.iter().all(|chunk| chunk.chunk_id() == Some(id.as_str())));
        assert!(chunks[..chunks.len() - 1].iter().all(|chunk| chunk.more()));
        assert!(!chunks[chunks.len() - 1].more());
        let joined: String = chunks.iter().map(|chunk| chunk.body.as_str()).collect();
        assert_eq!(joined, body);
        for chunk in &chunks {
            assert!(chunk.body.len() <= 24);
            assert!(!chunk.body.contains('\u{1b}'));
        }
    }

    #[test]
    fn multibyte_body_never_splits_a_code_point() {
        let body = format!("{{\"text\":\"{}\"}}", "ab€漢字🙂🙃 é\"quote\" ".repeat(9));
        let stream = encode_message_with_limit(Verb::Frame, &body, &TspParams::new(), 23);
        let chunks = split_all(&stream);
        assert!(chunks.len() > 1);
        let ids: std::collections::HashSet<_> =
            chunks.iter().filter_map(|chunk| chunk.chunk_id()).collect();
        assert_eq!(ids.len(), 1);
        let joined: String = chunks.iter().map(|chunk| chunk.body.as_str()).collect();
        assert_eq!(joined, body);
        for chunk in &chunks {
            assert!(chunk.body.len() <= 23);
            assert!(!leads_with_parameter(chunk.body.as_bytes(), 0));
        }
    }

    #[test]
    fn parameter_shaped_cut_is_avoided() {
        let event = serde_json::json!({"ev": "error", "msg": "aaaaaaaaaax=1;tail"});
        let body = event.to_string();
        let limit = body.find("x=1;").unwrap();
        let stream = encode_message_with_limit(Verb::Event, &body, &TspParams::new(), limit);
        let chunks = split_all(&stream);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(!chunk.body.starts_with("x=1;"));
        }
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.body.as_str())
                .collect::<String>(),
            body
        );
    }

    #[test]
    fn ascii_base64_cuts_exactly_at_the_limit() {
        let body = "a".repeat(100);
        let mut params = TspParams::new();
        params.push(("mime".to_string(), "image/png".to_string()));
        let chunks = split_all(&encode_message_with_limit(Verb::Blob, &body, &params, 20));
        assert_eq!(chunks.len(), 5);
        for chunk in &chunks[..4] {
            assert_eq!(chunk.body.len(), 20);
        }
        assert_eq!(
            chunks[0].params[0],
            ("mime".to_string(), "image/png".to_string())
        );
        assert_eq!(chunks[0].chunk_id(), chunks[4].chunk_id());
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.body.as_str())
                .collect::<String>(),
            body
        );
    }

    #[test]
    fn split_tolerates_and_rejects_shapes() {
        let raw = split_message("\x1b_tsp;f;a=1;b=2;body\x1b\\").unwrap();
        assert_eq!(raw.verb, "f");
        assert_eq!(
            raw.params,
            vec![("a".to_string(), "1".to_string()), ("b".to_string(), "2".to_string())]
        );
        assert_eq!(raw.body, "body");
        assert!(split_message("\x1b_25a1;e;{}\x1b\\").is_none());
        assert!(split_message("\x1b_tsp;f;body").is_none());
        assert!(split_message("\x1b_tsp;;body\x1b\\").is_none());
        assert_eq!(
            try_split_message("\x1b_25a1;e;{}\x1b\\").unwrap_err(),
            FrameError::NotTsp
        );
        assert_eq!(
            try_split_message("\x1b_tsp;f;body").unwrap_err(),
            FrameError::MissingTerminator
        );
    }

    #[test]
    fn reader_reassembles_chunks_and_tolerates_unknown_fields() {
        let mut reader = TspReader::new();
        let event = serde_json::json!({
            "ev": "toggle",
            "sf": "s:1",
            "id": "a.b",
            "collapsed": false,
            "future": {"x": 1}
        });
        let stream = encode_message_with_limit(Verb::Event, &event.to_string(), &TspParams::new(), 8);
        let mut decoded = None;
        for message in messages(&stream) {
            if let Some(message) = reader.feed(&message) {
                decoded = Some(message);
            }
        }
        let decoded = decoded.unwrap();
        assert_eq!(decoded.verb(), Verb::Event);
        let event = decoded.event().unwrap();
        assert_eq!(event.ev(), "toggle");
        assert_eq!(event.get_str("id"), Some("a.b"));
        assert_eq!(event.get("future").unwrap()["x"], 1);
        assert_eq!(reader.pending_chunks(), 0);
    }

    #[test]
    fn malformed_bodies_are_rejected() {
        assert!(
            parse_message("\x1b_tsp;r;{\"r\":\"hello\",\"v\":1,\"term\":\"tern\"}\x1b\\").is_none()
        );
        assert!(parse_message("\x1b_tsp;e;{\"ev\":\"ack\"}\x1b\\").is_none());
        assert!(parse_message("\x1b_tsp;e;{not json\x1b\\").is_none());
        assert!(
            parse_message("\x1b_tsp;f;{\"sf\":\"s:1\",\"s\":1,\"ops\":[]}\x1b\\").is_none()
        );
        let tolerant = parse_message(
            "\x1b_tsp;r;{\"r\":\"hello\",\"v\":1,\"term\":\"tern\",\"kinds\":[\"col\"],\"x\":2}\x1b\\",
        )
        .unwrap();
        let hello = tolerant.reply().unwrap().hello().unwrap();
        assert_eq!(hello.kinds, vec!["col".to_string()]);
        assert_eq!(hello.extra.get("x"), Some(&Value::Number(2.into())));
    }

    #[test]
    fn send_events_require_surface_target_and_text() {
        let valid = serde_json::json!({"ev": "send", "sf": "s:1", "id": "a.line/input", "text": "prompt"});
        assert!(parse_message(&format!("\x1b_tsp;e;{}{ST}", valid)).is_some());
        for bad in [
            serde_json::json!({"ev": "send", "id": "a", "text": "p"}),
            serde_json::json!({"ev": "send", "sf": "", "id": "a", "text": "p"}),
            serde_json::json!({"ev": "send", "sf": "s:1", "id": "", "text": "p"}),
            serde_json::json!({"ev": "send", "sf": "s:1", "id": "a"}),
            serde_json::json!({"ev": "send", "sf": "s:1", "id": "a", "text": ["p"]}),
        ] {
            assert!(
                parse_message(&format!("\x1b_tsp;e;{}{ST}", bad)).is_none(),
                "expected rejection: {bad}"
            );
        }
        let blank = serde_json::json!({"ev": "send", "sf": "s:1", "id": "a", "text": ""});
        assert!(parse_message(&format!("\x1b_tsp;e;{}{ST}", blank)).is_some());
    }

    #[test]
    fn change_events_accept_every_value_shape() {
        for value in [
            Value::Bool(true),
            Value::Number(50.into()),
            Value::String("branch".into()),
            Value::Array(vec![Value::String("c".into()), Value::String("a".into())]),
            Value::Null,
        ] {
            let event = serde_json::json!({"ev": "change", "sf": "s:1", "id": "pf", "item": "task.isolation.merge", "value": value});
            let decoded = parse_message(&format!("\x1b_tsp;e;{}{ST}", event)).unwrap();
            assert_eq!(decoded.event().unwrap().get("value"), Some(&value));
        }
        assert!(
            parse_message("\x1b_tsp;e;{\"ev\":\"change\",\"sf\":\"s:1\",\"id\":\"pf\"}\x1b\\")
                .is_none()
        );
    }

    #[test]
    fn stateful_encoder_increments_chunk_ids() {
        let mut encoder = TspEncoder::with_start_id(1);
        let body = "y".repeat(50);
        let first = encoder.encode_message_with_limit(Verb::Frame, &body, &TspParams::new(), 20);
        let second = encoder.encode_message_with_limit(Verb::Frame, &body, &TspParams::new(), 20);
        assert!(first.contains(";c=1;"));
        assert!(second.contains(";c=2;"));
        assert_eq!(encoder.next_id(), 3);
    }
}
