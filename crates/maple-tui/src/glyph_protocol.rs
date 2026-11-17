//! Glyph Protocol client (port of `l/src/glyph-protocol.ts`).
//! Reference: `l/src/glyph-protocol.ts`.
//!
//! Ships the TUI's private-use icons to the terminal as APC sequences and
//! decodes the terminal's replies. The bundled outlines live in
//! `assets/glyph-bundle.json`; parsing is deferred to first use so startup
//! never pays for it unless glyphs are registered.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;
const GLYPH_PROTOCOL_ID: &str = "25a1";


const APC: &str = "\x1b_";
const ST: &str = "\x1b\\";

/// Codepoint queried after registration to confirm the bundle was accepted.
pub const GLYPH_CONFIRMATION_CODEPOINT: u32 = 0xf0d57;

const BUNDLE_JSON: &str = include_str!("../assets/glyph-bundle.json");

/// One bundled outline: base64 simple-glyph record plus its scale policy.
#[derive(Clone, Debug, Deserialize)]
pub struct GlyphBundleEntry {
    /// Base64 OpenType `glyf` simple-glyph record (§8.2 subset).
    pub glyf: String,
    /// Fill the cell edge to edge (`size=stretch`, powerline); otherwise `size=contain`.
    #[serde(default)]
    pub stretch: bool,
}

/// Generated icon bundle: shared design metrics plus outlines keyed by
/// lowercase hex codepoint.
#[derive(Clone, Debug, Deserialize)]
pub struct GlyphBundle {
    /// Units per em.
    pub upm: u32,
    /// Authored advance width — identical for every glyph of a Mono face.
    pub aw: u32,
    /// Authored line height (ascender − descender).
    pub lh: u32,
    /// Outlines keyed by lowercase hex codepoint.
    pub glyphs: BTreeMap<String, GlyphBundleEntry>,
}


static BUNDLE: LazyLock<GlyphBundle> =
    LazyLock::new(|| serde_json::from_str(BUNDLE_JSON).expect("glyph bundle is valid JSON"));

/// The embedded generated bundle.
pub fn glyph_bundle() -> &'static GlyphBundle {
    &BUNDLE
}

/// Whether `cp` lies in one of the three Private Use Areas the protocol accepts.
pub fn is_private_use_codepoint(cp: u32) -> bool {
    (0xe000..=0xf8ff).contains(&cp) || (0xf0000..=0xffffd).contains(&cp) || (0x100000..=0x10fffd).contains(&cp)
}

/// `s` verb: doubles as the protocol-detection ping (any reply confirms support).
pub fn encode_glyph_support_query() -> String {
    format!("{APC}{GLYPH_PROTOCOL_ID};s{ST}")
}

/// `q` verb: ask which sources (system font, session glossary) render `cp`.
pub fn encode_glyph_coverage_query(cp: u32) -> String {
    format!("{APC}{GLYPH_PROTOCOL_ID};q;cp={cp:x}{ST}")
}

/// `r` verb for one bundled outline at `cp`.
///
/// Layout hints follow the bundle's design metrics; `reply=0` suppresses the
/// acknowledgement so bulk startup registration leaves nothing on stdin.
pub fn encode_glyph_registration(
    cp: u32,
    entry: &GlyphBundleEntry,
    metrics: &GlyphBundle,
) -> Result<String, String> {
    if !is_private_use_codepoint(cp) {
        return Err(format!("U+{:X} is not a private-use codepoint", cp));
    }
    let size = if entry.stretch { "stretch" } else { "contain" };
    Ok(format!(
        "{APC}{GLYPH_PROTOCOL_ID};r;cp={cp:x};reply=0;upm={};aw={};lh={};size={size};{}{ST}",
        metrics.upm, metrics.aw, metrics.lh, entry.glyf
    ))
}

/// Every bundled icon as one contiguous write.
pub fn encode_bundled_glyph_registrations() -> Result<String, String> {
    let metrics = glyph_bundle();
    let mut out = String::new();
    for (hex, entry) in &metrics.glyphs {
        let cp = u32::from_str_radix(hex, 16).map_err(|_| format!("bad codepoint key {hex}"))?;
        out.push_str(&encode_glyph_registration(cp, entry, metrics)?);
    }
    Ok(out)
}

/// A terminal reply to any Glyph Protocol verb, decoded from its APC body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlyphProtocolReply {
    /// `s`: supported formats.
    Support { formats: Vec<String> },
    /// `q`: codepoint coverage.
    Coverage { cp: u32, coverage: Vec<String> },
    /// `r`: registration result.
    Registration { cp: u32, status: u32, reason: Option<String> },
    /// `c`: confirmation result.
    Confirmation { status: u32, reason: Option<String> },
}

/// Whether `sequence` is a complete Glyph Protocol APC message.
pub fn is_glyph_protocol_sequence(sequence: &str) -> bool {
    sequence.starts_with(&format!("{APC}{GLYPH_PROTOCOL_ID};")) && sequence.ends_with(ST)
}

fn parse_hex(value: Option<&String>) -> Option<u32> {
    u32::from_str_radix(value?, 16).ok()
}

fn parse_decimal(value: Option<&String>) -> Option<u32> {
    value?.parse().ok()
}

fn list(value: Option<&String>) -> Vec<String> {
    match value {
        Some(v) if !v.is_empty() => v.split(',').map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

/// Decode a terminal reply.
///
/// Returns `None` for anything that is not a well-formed Glyph Protocol
/// message; unknown keys are ignored per §11 so future parameters never break
/// detection.
pub fn parse_glyph_protocol_reply(sequence: &str) -> Option<GlyphProtocolReply> {
    if !is_glyph_protocol_sequence(sequence) {
        return None;
    }
    let body = &sequence[APC.len() + GLYPH_PROTOCOL_ID.len() + 1..sequence.len() - ST.len()];
    let parts: Vec<&str> = body.split(';').collect();
    let verb = parts.first().copied().unwrap_or("");
    let mut params: BTreeMap<&str, &str> = BTreeMap::new();
    for part in parts.iter().skip(1) {
        if let Some(eq) = part.find('=') {
            if eq > 0 {
                params.insert(&part[..eq], &part[eq + 1..]);
            }
        }
    }
    let owned = |key: &str| params.get(key).map(|v| v.to_string());
    match verb {
        "s" => Some(GlyphProtocolReply::Support {
            formats: list(owned("fmt").as_ref()),
        }),
        "q" => {
            let cp = parse_hex(owned("cp").as_ref())?;
            Some(GlyphProtocolReply::Coverage {
                cp,
                coverage: list(owned("status").as_ref()),
            })
        }
        "r" => {
            let cp = parse_hex(owned("cp").as_ref())?;
            let status = parse_decimal(owned("status").as_ref())?;
            let reason = owned("reason").filter(|_| params.contains_key("reason"));
            Some(GlyphProtocolReply::Registration { cp, status, reason })
        }
        "c" => {
            let status = parse_decimal(owned("status").as_ref())?;
            let reason = owned("reason").filter(|_| params.contains_key("reason"));
            Some(GlyphProtocolReply::Confirmation { status, reason })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_support_and_coverage_verbs() {
        assert_eq!(encode_glyph_support_query(), "\x1b_25a1;s\x1b\\");
        assert_eq!(
            encode_glyph_coverage_query(0xf0d57),
            "\x1b_25a1;q;cp=f0d57\x1b\\"
        );
    }

    #[test]
    fn rejects_non_private_use_registration() {
        assert!(encode_glyph_registration(0x41, &GlyphBundleEntry { glyf: "AA".into(), stretch: false }, glyph_bundle()).is_err());
    }

    #[test]
    fn encodes_registration_with_metrics() {
        let bundle = GlyphBundle {
            upm: 1000,
            aw: 600,
            lh: 1320,
            glyphs: BTreeMap::new(),
        };
        let entry = GlyphBundleEntry { glyf: "AAQ".into(), stretch: true };
        assert_eq!(
            encode_glyph_registration(0xe000, &entry, &bundle).unwrap(),
            "\x1b_25a1;r;cp=e000;reply=0;upm=1000;aw=600;lh=1320;size=stretch;AAQ\x1b\\"
        );
    }

    #[test]
    fn bundled_registrations_cover_every_glyph() {
        let out = encode_bundled_glyph_registrations().unwrap();
        assert_eq!(
            out.matches("\x1b_25a1;r;").count(),
            glyph_bundle().glyphs.len()
        );
        assert!(out.ends_with(ST));
    }

    #[test]
    fn parses_all_reply_verbs() {
        assert_eq!(
            parse_glyph_protocol_reply("\x1b_25a1;s;fmt=kitty,sixel\x1b\\"),
            Some(GlyphProtocolReply::Support {
                formats: vec!["kitty".into(), "sixel".into()]
            })
        );
        assert_eq!(
            parse_glyph_protocol_reply("\x1b_25a1;q;cp=f0d57;status=font,glossary\x1b\\"),
            Some(GlyphProtocolReply::Coverage {
                cp: 0xf0d57,
                coverage: vec!["font".into(), "glossary".into()]
            })
        );
        assert_eq!(
            parse_glyph_protocol_reply("\x1b_25a1;r;cp=e000;status=0\x1b\\"),
            Some(GlyphProtocolReply::Registration { cp: 0xe000, status: 0, reason: None })
        );
        assert_eq!(
            parse_glyph_protocol_reply("\x1b_25a1;r;cp=e000;status=2;reason=busy\x1b\\"),
            Some(GlyphProtocolReply::Registration {
                cp: 0xe000,
                status: 2,
                reason: Some("busy".into())
            })
        );
        assert_eq!(
            parse_glyph_protocol_reply("\x1b_25a1;c;status=0\x1b\\"),
            Some(GlyphProtocolReply::Confirmation { status: 0, reason: None })
        );
    }

    #[test]
    fn rejects_malformed_replies() {
        // A bare `s` reply is well-formed: the terminal confirms support and
        // reports no formats (`fmt` absent means the empty list, per §11).
        assert_eq!(
            parse_glyph_protocol_reply("\x1b_25a1;s\x1b\\"),
            Some(GlyphProtocolReply::Support { formats: Vec::new() })
        );
        assert_eq!(parse_glyph_protocol_reply("25a1;s\x1b\\"), None);
        assert_eq!(parse_glyph_protocol_reply("\x1b_25a1;x;status=0\x1b\\"), None);
        assert_eq!(parse_glyph_protocol_reply("\x1b_25a1;r;cp=zz;status=0\x1b\\"), None);
    }
}
