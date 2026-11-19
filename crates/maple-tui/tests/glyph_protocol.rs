//! Port of the codec/bundle sections of `l/test/glyph-protocol.test.ts`.

use base64::Engine as _;
use maple_tui::glyph_protocol::{
	encode_bundled_glyph_registrations, encode_glyph_registration, glyph_bundle,
	is_private_use_codepoint, parse_glyph_protocol_reply, GlyphBundleEntry,
	GLYPH_CONFIRMATION_CODEPOINT,
};

#[test]
fn encodes_a_registration_with_bundle_metrics_and_fire_and_forget_gate() {
	let metrics = glyph_bundle();
	let entry = GlyphBundleEntry {
		glyf: "AAA=".to_string(),
		stretch: true,
	};
	assert_eq!(
		encode_glyph_registration(0xe0b0, &entry, metrics).unwrap(),
		"\x1b_25a1;r;cp=e0b0;reply=0;upm=1000;aw=600;lh=1320;size=stretch;AAA=\x1b\\"
	);
	let icon = GlyphBundleEntry {
		glyf: "AAA=".to_string(),
		stretch: false,
	};
	assert!(encode_glyph_registration(0xf00c, &icon, metrics)
		.unwrap()
		.contains(";size=contain;"));
}

#[test]
fn refuses_to_register_outside_the_private_use_areas() {
	assert!(encode_glyph_registration(
		0x61,
		&GlyphBundleEntry {
			glyf: "AAA=".to_string(),
			stretch: false
		},
		glyph_bundle()
	)
	.is_err());
	assert!(is_private_use_codepoint(0xe000));
	assert!(is_private_use_codepoint(0xf8ff));
	assert!(!is_private_use_codepoint(0xf900));
	assert!(is_private_use_codepoint(0xf0000));
	assert!(!is_private_use_codepoint(0xffffe));
	assert!(is_private_use_codepoint(0x10fffd));
}

#[test]
fn parses_every_reply_verb_and_treats_empty_lists_as_none() {
	assert_eq!(
		parse_glyph_protocol_reply("\x1b_25a1;s;fmt=glyf,colrv0,colrv1\x1b\\"),
		Some(maple_tui::glyph_protocol::GlyphProtocolReply::Support {
			formats: vec!["glyf".to_string(), "colrv0".to_string(), "colrv1".to_string()]
		})
	);
	assert_eq!(
		parse_glyph_protocol_reply("\x1b_25a1;s;fmt=\x1b\\"),
		Some(maple_tui::glyph_protocol::GlyphProtocolReply::Support { formats: vec![] })
	);
	assert_eq!(
		parse_glyph_protocol_reply("\x1b_25a1;q;cp=e0a0;status=system,glossary\x1b\\"),
		Some(maple_tui::glyph_protocol::GlyphProtocolReply::Coverage {
			cp: 0xe0a0,
			coverage: vec!["system".to_string(), "glossary".to_string()]
		})
	);
	assert_eq!(
		parse_glyph_protocol_reply("\x1b_25a1;q;cp=e0a0;status=\x1b\\"),
		Some(maple_tui::glyph_protocol::GlyphProtocolReply::Coverage {
			cp: 0xe0a0,
			coverage: vec![]
		})
	);
	assert_eq!(
		parse_glyph_protocol_reply("\x1b_25a1;r;cp=100000;status=3;reason=payload_too_large\x1b\\"),
		Some(maple_tui::glyph_protocol::GlyphProtocolReply::Registration {
			cp: 0x100000,
			status: 3,
			reason: Some("payload_too_large".to_string())
		})
	);
	assert_eq!(
		parse_glyph_protocol_reply("\x1b_25a1;c;status=0\x1b\\"),
		Some(maple_tui::glyph_protocol::GlyphProtocolReply::Confirmation {
			status: 0,
			reason: None
		})
	);
}

#[test]
fn rejects_foreign_apcs_unknown_verbs_and_malformed_numerics() {
	assert_eq!(parse_glyph_protocol_reply("\x1b_Ga=t,i=1;AAAA\x1b\\"), None);
	assert_eq!(parse_glyph_protocol_reply("\x1b_25a1;x;cp=e000\x1b\\"), None);
	assert_eq!(parse_glyph_protocol_reply("\x1b_25a1;r;status=0\x1b\\"), None);
	assert_eq!(parse_glyph_protocol_reply("\x1b_25a1;s;fmt=glyf"), None);
}

#[test]
fn ships_only_pua_codepoints_as_simple_glyph_records_under_the_wire_budget() {
	let metrics = glyph_bundle();
	let count = metrics.glyphs.len();
	assert!(count > 200);
	for (hex, entry) in &metrics.glyphs {
		let cp = u32::from_str_radix(hex, 16).expect("hex key");
		assert!(is_private_use_codepoint(cp), "{hex}");
		let record = base64::engine::general_purpose::STANDARD
			.decode(entry.glyf.as_bytes())
			.expect("base64 glyf");
		let contours = i16::from_be_bytes([record[0], record[1]]);
		assert!(contours > 0, "{hex}");
		assert!(record.len() <= 64 * 1024, "{hex}");
	}
	let registrations = encode_bundled_glyph_registrations().expect("registrations");
	assert_eq!(registrations.matches("\x1b_25a1;r;").count(), count);
	let confirm_hex = format!("{:x}", GLYPH_CONFIRMATION_CODEPOINT);
	assert!(metrics.glyphs.contains_key(&confirm_hex));
}
