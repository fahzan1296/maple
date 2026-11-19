//! Port of `l/test/qrcode.test.ts`.

use maple_tui::chrome::qrcode::{
	render_qr_half_blocks, QrCode, QrEcLevel, QrEncodeOptions, QrRenderOptions,
};
use maple_tui::strip_ansi;
use sha2::{Digest, Sha256};

fn matrix_fingerprint(qr: &QrCode) -> String {
	let mut bits = String::new();
	for y in 0..qr.size {
		for x in 0..qr.size {
			bits.push(if qr.module(x, y) { '1' } else { '0' });
		}
	}
	let digest = Sha256::digest(bits.as_bytes());
	let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
	hex[..16].to_string()
}

#[test]
fn produces_stable_symbols() {
	let vectors: &[(&str, QrEcLevel, usize, usize, usize, &str)] = &[
		("HELLO WORLD", QrEcLevel::M, 0, 1, 21, "a28227450c6dd5ab"),
		(
			"https://my.omp.sh/#mgAYTZwEnpRQtca0CTgn-Q.gdJU",
			QrEcLevel::M,
			4,
			4,
			33,
			"4af2f66e1b06a5b1",
		),
		(
			"https://web.example/collab/#relay.example.com:8443/r/AbCdEfGhIjKlMnOp.0123456789abcdef",
			QrEcLevel::Q,
			6,
			7,
			45,
			"2ca4a51e3cba1adf",
		),
		("x", QrEcLevel::L, 1, 1, 21, "d1330f755ac63f88"),
	];
	for (text, ecl, mask, version, size, hash) in vectors {
		let qr = QrCode::encode_bytes(
			text.as_bytes(),
			*ecl,
			Some(QrEncodeOptions {
				mask: Some(*mask),
				..Default::default()
			}),
		)
		.expect("encodes");
		assert_eq!(qr.version, *version, "{text}");
		assert_eq!(qr.size, *size, "{text}");
		assert_eq!(qr.mask, *mask, "{text}");
		assert_eq!(&matrix_fingerprint(&qr), hash, "{text}");
	}
}

#[test]
fn auto_selects_the_smallest_version_that_fits() {
	assert_eq!(QrCode::encode_text(&"a".repeat(14), QrEcLevel::M).unwrap().version, 1);
	assert_eq!(QrCode::encode_text(&"a".repeat(15), QrEcLevel::M).unwrap().version, 2);
	assert_eq!(QrCode::encode_text(&"a".repeat(26), QrEcLevel::M).unwrap().version, 2);
	assert_eq!(QrCode::encode_text(&"a".repeat(27), QrEcLevel::M).unwrap().version, 3);
}

#[test]
fn sizes_by_utf8_byte_length_not_string_length() {
	assert_eq!(QrCode::encode_text(&"日".repeat(5), QrEcLevel::M).unwrap().version, 2);
}

#[test]
fn deterministically_selects_a_penalty_minimizing_mask() {
	let qr = QrCode::encode_text("https://my.omp.sh/#demo", QrEcLevel::M).unwrap();
	assert_eq!(qr.mask, 1);
	assert_eq!(matrix_fingerprint(&qr), "ee820c588fe36d99");
}

#[test]
fn errors_when_payload_exceeds_version_40() {
	assert!(QrCode::encode_text(&"a".repeat(1274), QrEcLevel::H).is_err());
}

#[test]
fn places_three_finder_patterns_at_corners() {
	let qr = QrCode::encode_text("finder", QrEcLevel::M).unwrap();
	let finder_corner = |ox: usize, oy: usize| {
		let dx = if ox == 0 { 1 } else { -1 };
		let dy = if oy == 0 { 1 } else { -1 };
		qr.module(ox, oy) && !qr.module(ox.wrapping_add(dx), oy.wrapping_add(dy))
	};
	assert!(finder_corner(0, 0));
	assert!(finder_corner(qr.size - 1, 0));
	assert!(finder_corner(0, qr.size - 1));
}

#[test]
fn frames_the_symbol_in_a_light_quiet_zone() {
	let qr = QrCode::encode_text("https://omp.sh/#demo", QrEcLevel::M).unwrap();
	let margin = 3;
	let lines = render_qr_half_blocks(&qr, Some(QrRenderOptions { margin }));
	let stripped: Vec<String> = lines.iter().map(|line| strip_ansi(line)).collect();
	assert_eq!(stripped[0].chars().count(), qr.size + margin * 2);
	let quiet_rows = margin / 2;
	for line in &stripped[..quiet_rows] {
		assert_eq!(line, &" ".repeat(qr.size + margin * 2));
	}
	let body = &lines[quiet_rows + 2];
	assert!(body.contains("\x1b[47m"), "{body:?}");
	assert!(body.contains(['▀', '▄', '█']), "{body:?}");
}

#[test]
fn emits_half_rows_for_size_plus_margins() {
	let qr = QrCode::encode_text("rows", QrEcLevel::M).unwrap();
	let margin = 4;
	let lines = render_qr_half_blocks(&qr, Some(QrRenderOptions { margin }));
	assert_eq!(lines.len(), (qr.size + margin * 2).div_ceil(2));
}
