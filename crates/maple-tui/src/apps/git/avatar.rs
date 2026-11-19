//! Avatar loading and deterministic identicon fallback for the Git sidebar.
//!
//! Reference: `l/src/apps/git/avatar.ts`.
//!
//! Avatar providers are consulted in priority order and the first hit wins;
//! when none matches, [`identicon_lines`] derives a stable 5x5 mirrored
//! half-block portrait from the MD5 of the commit author's email.

/// Source of avatar images: URL, data URI, or local file path.
pub trait AvatarSource {
	/// Resolve an avatar for `email`, relative to `cwd`; `None` means no match.
	fn get(&self, email: &str, cwd: &str) -> Option<String>;
}

/// Render the 3-line identicon for `email`.
///
/// `colorize` receives the derived HSL hex and the rendered line, matching the
/// reference's `(hex, text)` callback.
pub fn identicon_lines<F>(email: &str, colorize: F) -> Vec<String>
where
	F: Fn(&str, &str) -> String,
{
	let normalized = email.trim().to_ascii_lowercase();
	let digest = md5_hex(normalized.as_bytes());
	let hue = (((digest.as_bytes()[0] as u32) << 8) | digest.as_bytes()[1] as u32) % 360;
	let hex = hsl_to_hex(hue as f64, 0.55, 0.58);
	let mut lines = Vec::with_capacity(3);
	for row in 0..3 {
		let mut line = String::with_capacity(10);
		for x in 0..5 {
			let column = if x < 3 { x } else { 4 - x };
			let top = digest_bit(&digest, column, row * 2);
			let bottom = row * 2 + 1 < 5 && digest_bit(&digest, column, row * 2 + 1);
			let cell = match (top, bottom) {
				(true, true) => "█",
				(true, false) => "▀",
				(false, true) => "▄",
				(false, false) => " ",
			};
			line.push_str(cell);
			line.push_str(cell);
		}
		lines.push(colorize(&hex, &line));
	}
	lines
}

/// Bit `(column, y)` of the mirrored identicon grid.
fn digest_bit(digest: &str, column: usize, y: usize) -> bool {
	let byte = digest.as_bytes()[3 + column * 5 + y];
	let digit = (byte as char).to_digit(16).unwrap_or(0);
	digit % 2 == 0
}

const MD5_S: [u32; 64] = [
	7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
	14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
	21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

const MD5_K: [u32; 64] = [
	0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
	0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
	0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
	0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
	0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
	0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
	0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
	0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// MD5 digest of `input` as a lowercase hex string (RFC 1321).
fn md5_hex(input: &[u8]) -> String {
	let mut message = input.to_vec();
	let bit_length = (input.len() as u64).wrapping_mul(8);
	message.push(0x80);
	while message.len() % 64 != 56 {
		message.push(0);
	}
	message.extend_from_slice(&bit_length.to_le_bytes());

	let mut a0 = 0x6745_2301_u32;
	let mut b0 = 0xefcd_ab89_u32;
	let mut c0 = 0x98ba_dcfe_u32;
	let mut d0 = 0x1032_5476_u32;

	for chunk in message.chunks_exact(64) {
		let mut m = [0_u32; 16];
		for (word, slot) in m.iter_mut().enumerate() {
			let offset = word * 4;
			*slot = u32::from_le_bytes([
				chunk[offset],
				chunk[offset + 1],
				chunk[offset + 2],
				chunk[offset + 3],
			]);
		}
		let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
		for round in 0..64 {
			let (f, g) = match round / 16 {
				0 => ((b & c) | (!b & d), round),
				1 => ((d & b) | (!d & c), (5 * round + 1) % 16),
				2 => (b ^ c ^ d, (3 * round + 5) % 16),
				_ => (c ^ (b | !d), (7 * round) % 16),
			};
			let sum = a
				.wrapping_add(f)
				.wrapping_add(MD5_K[round])
				.wrapping_add(m[g]);
			a = d;
			d = c;
			c = b;
			b = b.wrapping_add(sum.rotate_left(MD5_S[round]));
		}
		a0 = a0.wrapping_add(a);
		b0 = b0.wrapping_add(b);
		c0 = c0.wrapping_add(c);
		d0 = d0.wrapping_add(d);
	}

	format!("{a0:08x}{b0:08x}{c0:08x}{d0:08x}")
}

/// HSL (`h` in degrees, `s`/`l` in 0..1) to a lowercase `#rrggbb` string.
fn hsl_to_hex(h: f64, s: f64, l: f64) -> String {
	let h = h.rem_euclid(360.0);
	let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
	let segment = h / 60.0;
	let secondary = chroma * (1.0 - ((segment % 2.0) - 1.0).abs());
	let (r1, g1, b1) = match segment as u32 {
		0 => (chroma, secondary, 0.0),
		1 => (secondary, chroma, 0.0),
		2 => (0.0, chroma, secondary),
		3 => (0.0, secondary, chroma),
		4 => (secondary, 0.0, chroma),
		_ => (chroma, 0.0, secondary),
	};
	let m = l - chroma / 2.0;
	let channel = |value: f64| (value + m).mul_add(255.0, 0.5).clamp(0.0, 255.0) as u8;
	format!("#{:02x}{:02x}{:02x}", channel(r1), channel(g1), channel(b1))
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::RefCell;

	struct MapSource(Vec<(String, String)>);

	impl AvatarSource for MapSource {
		fn get(&self, email: &str, _cwd: &str) -> Option<String> {
			self.0.iter().find(|(key, _)| key == email).map(|(_, url)| url.clone())
		}
	}

	#[test]
	fn md5_matches_rfc1321_vectors() {
		assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
		assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
		assert_eq!(
			md5_hex(b"The quick brown fox jumps over the lazy dog"),
			"9e107d9d372bb6826bd81d3542a419d6"
		);
	}

	#[test]
	fn identicon_is_three_ten_column_lines_with_a_six_digit_hex() {
		let seen = RefCell::new((String::new(), Vec::new()));
		let lines = identicon_lines("Ada@example.com", |hex, text| {
			seen.borrow_mut().0 = hex.to_string();
			seen.borrow_mut().1.push(text.to_string());
			text.to_string()
		});
		assert_eq!(lines.len(), 3);
		for line in &lines {
			assert_eq!(line.chars().count(), 10, "line was {line:?}");
		}
		let (hex, raw) = seen.into_inner();
		assert_eq!(hex.len(), 7);
		assert!(hex.starts_with('#'));
		assert!(hex[1..].chars().all(|c| c.is_ascii_hexdigit()));
		assert_eq!(raw.len(), 3);
	}

	#[test]
	fn identicon_is_deterministic_and_case_insensitive() {
		let plain = |_: &str, text: &str| text.to_string();
		assert_eq!(
			identicon_lines("User@Example.com", plain),
			identicon_lines("  user@example.COM  ", plain)
		);
		assert_eq!(identicon_lines("a@b.c", plain), identicon_lines("a@b.c", plain));
	}

	#[test]
	fn identicon_uses_only_half_block_glyphs() {
		let lines = identicon_lines("user@example.com", |_, text| text.to_string());
		for line in lines {
			for ch in line.chars() {
				assert!(matches!(ch, '█' | '▀' | '▄' | ' '), "unexpected {ch:?}");
			}
		}
	}

	#[test]
	fn hsl_to_hex_returns_six_digit_hex() {
		assert_eq!(hsl_to_hex(0.0, 1.0, 0.5), "#ff0000");
		assert_eq!(hsl_to_hex(120.0, 1.0, 0.5), "#00ff00");
		assert_eq!(hsl_to_hex(240.0, 1.0, 0.5), "#0000ff");
	}

	#[test]
	fn avatar_source_returns_the_first_match() {
		let source = MapSource(vec![
			("one@example.com".to_string(), "https://example.com/1.png".to_string()),
			("two@example.com".to_string(), "data:image/png;base64,AA".to_string()),
		]);
		assert_eq!(
			source.get("two@example.com", "/repo").as_deref(),
			Some("data:image/png;base64,AA")
		);
		assert_eq!(source.get("missing@example.com", "/repo"), None);
	}
}
