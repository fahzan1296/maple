//! Self-contained QR Code generator (byte mode, versions 1-40, EC levels
//! L/M/Q/H) with a half-block ANSI terminal renderer.
//!
//! Reference: `l/src/chrome/qrcode.ts`.
//!
//! Deviations:
//! - `encodeText`/`encodeBytes` throw on overflow in the reference; Rust
//!   returns `Result`.
//! - `encodeText` takes a Rust string (already UTF-8) instead of re-encoding.
//! - `QrRenderOptions` is taken by value and defaults to the spec margin of 4;
//!   `None` means "default options".

/// Error-correction level of a QR symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QrEcLevel {
	/// Level L (~7% correction).
	L,
	/// Level M (~15% correction).
	M,
	/// Level Q (~25% correction).
	Q,
	/// Level H (~30% correction).
	H,
}

impl QrEcLevel {
	/// The reference's level string.
	pub fn as_str(self) -> &'static str {
		match self {
			QrEcLevel::L => "L",
			QrEcLevel::M => "M",
			QrEcLevel::Q => "Q",
			QrEcLevel::H => "H",
		}
	}

	/// Parse a level string.
	pub fn from_str(value: &str) -> Option<Self> {
		match value {
			"L" => Some(QrEcLevel::L),
			"M" => Some(QrEcLevel::M),
			"Q" => Some(QrEcLevel::Q),
			"H" => Some(QrEcLevel::H),
			_ => None,
		}
	}

	/// Index into the spec tables.
	fn table(self) -> usize {
		match self {
			QrEcLevel::L => 0,
			QrEcLevel::M => 1,
			QrEcLevel::Q => 2,
			QrEcLevel::H => 3,
		}
	}

	/// 2-bit format code.
	fn format_bits(self) -> u32 {
		match self {
			QrEcLevel::L => 1,
			QrEcLevel::M => 0,
			QrEcLevel::Q => 3,
			QrEcLevel::H => 2,
		}
	}
}

const MIN_VERSION: usize = 1;
const MAX_VERSION: usize = 40;

// ISO/IEC 18004 Table 9 — error-correction codewords per block, indexed
// [ecTable][version]. Index 0 of each row pads the 1-based version axis.
#[rustfmt::skip]
const ECC_CODEWORDS_PER_BLOCK: [[i32; 41]; 4] = [
	[-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
	[-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
	[-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
	[-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];

// ISO/IEC 18004 Table 9 — number of error-correction blocks, indexed
// [ecTable][version].
#[rustfmt::skip]
const NUM_EC_BLOCKS: [[i32; 41]; 4] = [
	[-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
	[-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
	[-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
	[-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];

const BYTE_MODE_INDICATOR: u32 = 0x4;
const PAD_BYTES: [u32; 2] = [0xec, 0x11];

const PENALTY_N1: i64 = 3;
const PENALTY_N2: i64 = 3;
const PENALTY_N3: i64 = 40;
const PENALTY_N4: i64 = 10;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_QR_ROW_PREFIX: &str = "\x1b[47m\x1b[30m";

fn get_bit(value: u32, index: u32) -> bool {
	((value >> index) & 1) != 0
}

/// Whether mask `m` flips the module at `(x, y)`; the 8 data-mask conditions.
fn mask_bit(m: usize, x: usize, y: usize) -> bool {
	match m {
		0 => (x + y) % 2 == 0,
		1 => y % 2 == 0,
		2 => x % 3 == 0,
		3 => (x + y) % 3 == 0,
		4 => (x / 3 + y / 2) % 2 == 0,
		5 => ((x * y) % 2) + ((x * y) % 3) == 0,
		6 => (((x * y) % 2) + ((x * y) % 3)) % 2 == 0,
		_ => (((x + y) % 2) + ((x * y) % 3)) % 2 == 0,
	}
}

/// GF(256) multiply under the QR primitive polynomial `0x11D`.
fn gf_multiply(x: u32, y: u32) -> u32 {
	let mut z = 0u32;
	for i in (0..8).rev() {
		z = (z << 1) ^ ((z >> 7) * 0x11d);
		z ^= ((y >> i) & 1) * x;
	}
	z & 0xff
}

/// Reed-Solomon generator polynomial coefficients for `degree` EC codewords.
fn rs_divisor(degree: usize) -> Vec<u8> {
	let mut result = vec![0u8; degree];
	if degree == 0 {
		return result;
	}
	result[degree - 1] = 1;
	let mut root = 1u32;
	for _ in 0..degree {
		for j in 0..result.len() {
			result[j] = gf_multiply(result[j] as u32, root) as u8;
			if j + 1 < result.len() {
				result[j] ^= result[j + 1];
			}
		}
		root = gf_multiply(root, 0x02);
	}
	result
}

/// Reed-Solomon remainder (the EC codewords) of `data` divided by `divisor`.
fn rs_remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
	let mut result = vec![0u8; divisor.len()];
	for &byte in data {
		let factor = byte ^ result[0];
		result.copy_within(1.., 0);
		let last = result.len() - 1;
		result[last] = 0;
		for (index, &coefficient) in divisor.iter().enumerate() {
			result[index] ^= gf_multiply(coefficient as u32, factor as u32) as u8;
		}
	}
	result
}

/// Total data modules (bits available before EC) for a version.
fn raw_data_modules(version: usize) -> usize {
	let mut result = (16 * version + 128) * version + 64;
	if version >= 2 {
		let num_align = version / 7 + 2;
		result -= (25 * num_align - 10) * num_align - 55;
		if version >= 7 {
			result -= 36;
		}
	}
	result
}

/// Number of usable data codewords (8-bit) at a version + EC level.
fn data_codewords(version: usize, ec_table: usize) -> usize {
	raw_data_modules(version) / 8
		- ECC_CODEWORDS_PER_BLOCK[ec_table][version] as usize * NUM_EC_BLOCKS[ec_table][version] as usize
}

/// Byte-mode character-count indicator width in bits for a version.
fn char_count_bits(version: usize) -> usize {
	if version <= 9 {
		8
	} else {
		16
	}
}

/// Append-only MSB-first bit buffer.
struct BitBuffer {
	bits: Vec<u8>,
}

impl BitBuffer {
	fn new() -> Self {
		BitBuffer { bits: Vec::new() }
	}

	fn len(&self) -> usize {
		self.bits.len()
	}

	fn append(&mut self, value: u32, count: usize) {
		for index in (0..count).rev() {
			self.bits.push(((value >> index) & 1) as u8);
		}
	}

	fn to_bytes(&self) -> Vec<u8> {
		let mut out = vec![0u8; self.bits.len() >> 3];
		for (index, &bit) in self.bits.iter().enumerate() {
			out[index >> 3] = (out[index >> 3] << 1) | bit;
		}
		out
	}
}

/// Encoding constraints; `None` fields take the reference defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QrEncodeOptions {
	/// Lowest version to consider (default 1).
	pub min_version: Option<usize>,
	/// Highest version to consider (default 40).
	pub max_version: Option<usize>,
	/// Force a mask 0-7; `None` auto-selects the lowest-penalty mask.
	pub mask: Option<usize>,
}

/// Rendering constraints; the margin defaults to the spec's 4 modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QrRenderOptions {
	/// Quiet-zone width in modules on every side.
	pub margin: usize,
}

impl Default for QrRenderOptions {
	fn default() -> Self {
		QrRenderOptions { margin: 4 }
	}
}

/// A finished QR symbol: a square grid of dark/light modules plus the chosen
/// version, EC level, and mask.
pub struct QrCode {
	/// Module count per side.
	pub size: usize,
	/// Selected mask pattern (0-7).
	pub mask: usize,
	/// QR version (1-40).
	pub version: usize,
	/// Error-correction level.
	pub ec_level: QrEcLevel,
	modules: Vec<Vec<bool>>,
	is_function: Vec<Vec<bool>>,
}

impl QrCode {
	fn new(
		version: usize,
		ec_level: QrEcLevel,
		data_codewords_interleaved: &[u8],
		mask: Option<usize>,
	) -> Self {
		let size = version * 4 + 17;
		let mut qr = QrCode {
			size,
			mask: 0,
			version,
			ec_level,
			modules: vec![vec![false; size]; size],
			is_function: vec![vec![false; size]; size],
		};
		qr.draw_function_patterns();
		qr.draw_codewords(data_codewords_interleaved);
		qr.mask = qr.select_mask(mask);
		qr
	}

	/// Whether the module at `(x, y)` is dark.
	pub fn module(&self, x: usize, y: usize) -> bool {
		self.modules[y][x]
	}

	/// Encode a string in byte mode (UTF-8).
	pub fn encode_text(text: &str, ec_level: QrEcLevel) -> Result<QrCode, String> {
		QrCode::encode_bytes(text.as_bytes(), ec_level, None)
	}

	/// Encode raw bytes in byte mode.
	pub fn encode_bytes(
		data: &[u8],
		ec_level: QrEcLevel,
		options: Option<QrEncodeOptions>,
	) -> Result<QrCode, String> {
		let options = options.unwrap_or_default();
		let table = ec_level.table();
		let min_version = MIN_VERSION.max(options.min_version.unwrap_or(MIN_VERSION));
		let max_version = MAX_VERSION.min(options.max_version.unwrap_or(MAX_VERSION));

		let mut version = min_version;
		loop {
			let capacity_bits = data_codewords(version, table) * 8;
			let used_bits = 4 + char_count_bits(version) + data.len() * 8;
			if used_bits <= capacity_bits {
				break;
			}
			if version >= max_version {
				return Err(format!(
					"data too long for a QR code ({} bytes, EC {})",
					data.len(),
					ec_level.as_str()
				));
			}
			version += 1;
		}

		let mut bits = BitBuffer::new();
		bits.append(BYTE_MODE_INDICATOR, 4);
		bits.append(data.len() as u32, char_count_bits(version));
		for &byte in data {
			bits.append(byte as u32, 8);
		}

		let capacity_bits = data_codewords(version, table) * 8;
		bits.append(0, (capacity_bits - bits.len()).min(4));
		bits.append(0, (8 - (bits.len() % 8)) % 8);
		let mut pad = 0usize;
		while bits.len() < capacity_bits {
			bits.append(PAD_BYTES[pad], 8);
			pad ^= 1;
		}

		let codewords = Self::interleave(&bits.to_bytes(), version, table);
		let mask = match options.mask {
			Some(mask) if mask <= 7 => Some(mask),
			Some(mask) => return Err(format!("invalid mask {mask}")),
			None => None,
		};
		Ok(QrCode::new(version, ec_level, &codewords, mask))
	}

	/// Split into blocks, append Reed-Solomon EC, and interleave per the spec.
	fn interleave(data: &[u8], version: usize, ec_table: usize) -> Vec<u8> {
		let num_blocks = NUM_EC_BLOCKS[ec_table][version] as usize;
		let ecc_len = ECC_CODEWORDS_PER_BLOCK[ec_table][version] as usize;
		let raw_codewords = raw_data_modules(version) / 8;
		let num_short = num_blocks - (raw_codewords % num_blocks);
		let short_len = raw_codewords / num_blocks;
		let divisor = rs_divisor(ecc_len);

		let block_len = short_len + 1;
		let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(num_blocks);
		let mut offset = 0usize;
		for index in 0..num_blocks {
			let dat_len = short_len - ecc_len + if index < num_short { 0 } else { 1 };
			let dat = &data[offset..offset + dat_len];
			offset += dat_len;
			let mut block = vec![0u8; block_len];
			block[..dat_len].copy_from_slice(dat);
			let remainder = rs_remainder(dat, &divisor);
			block[block_len - ecc_len..].copy_from_slice(&remainder);
			blocks.push(block);
		}

		let mut result = vec![0u8; raw_codewords];
		let mut write = 0usize;
		for index in 0..block_len {
			for block_index in 0..num_blocks {
				if index == short_len - ecc_len && block_index < num_short {
					continue;
				}
				result[write] = blocks[block_index][index];
				write += 1;
			}
		}
		result
	}

	fn set_function(&mut self, x: usize, y: usize, dark: bool) {
		self.modules[y][x] = dark;
		self.is_function[y][x] = true;
	}

	fn draw_function_patterns(&mut self) {
		for index in 0..self.size {
			self.set_function(6, index, index % 2 == 0);
			self.set_function(index, 6, index % 2 == 0);
		}
		self.draw_finder(3, 3);
		self.draw_finder(self.size - 4, 3);
		self.draw_finder(3, self.size - 4);

		let align = self.alignment_positions();
		let last = align.len().saturating_sub(1);
		for i in 0..align.len() {
			for j in 0..align.len() {
				if (i == 0 && j == 0) || (i == 0 && j == last) || (i == last && j == 0) {
					continue;
				}
				self.draw_alignment(align[i], align[j]);
			}
		}

		self.draw_format_bits(0);
		self.draw_version();
	}

	fn draw_finder(&mut self, cx: usize, cy: usize) {
		for dy in -4isize..=4 {
			for dx in -4isize..=4 {
				let dist = dx.abs().max(dy.abs());
				let x = cx as isize + dx;
				let y = cy as isize + dy;
				if x >= 0 && x < self.size as isize && y >= 0 && y < self.size as isize {
					self.set_function(x as usize, y as usize, dist != 2 && dist != 4);
				}
			}
		}
	}

	fn draw_alignment(&mut self, cx: usize, cy: usize) {
		for dy in -2isize..=2 {
			for dx in -2isize..=2 {
				let dist = dx.abs().max(dy.abs());
				self.set_function((cx as isize + dx) as usize, (cy as isize + dy) as usize, dist != 1);
			}
		}
	}

	fn alignment_positions(&self) -> Vec<usize> {
		if self.version == 1 {
			return Vec::new();
		}
		let num_align = self.version / 7 + 2;
		let step = if self.version == 32 {
			26
		} else {
			(((self.size - 13) as f64 / (num_align * 2 - 2) as f64).ceil() as usize) * 2
		};
		let mut result = vec![6];
		let mut pos = self.size - 7;
		while result.len() < num_align {
			result.insert(1, pos);
			pos = pos.saturating_sub(step);
		}
		result
	}

	fn draw_format_bits(&mut self, mask: usize) {
		let data = (self.ec_level.format_bits() << 3) | mask as u32;
		let mut rem = data;
		for _ in 0..10 {
			rem = (rem << 1) ^ ((rem >> 9) * 0x537);
		}
		let bits = ((data << 10) | rem) ^ 0x5412;

		for index in 0..=5 {
			self.set_function(8, index, get_bit(bits, index as u32));
		}
		self.set_function(8, 7, get_bit(bits, 6));
		self.set_function(8, 8, get_bit(bits, 7));
		self.set_function(7, 8, get_bit(bits, 8));
		for index in 9..15 {
			self.set_function(14 - index, 8, get_bit(bits, index as u32));
		}
		for index in 0..8 {
			self.set_function(self.size - 1 - index, 8, get_bit(bits, index as u32));
		}
		for index in 8..15 {
			self.set_function(8, self.size - 15 + index, get_bit(bits, index as u32));
		}
		self.set_function(8, self.size - 8, true);
	}

	fn draw_version(&mut self) {
		if self.version < 7 {
			return;
		}
		let mut rem = self.version as u32;
		for _ in 0..12 {
			rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
		}
		let bits = ((self.version as u32) << 12) | rem;
		for index in 0..18 {
			let bit = get_bit(bits, index as u32);
			let a = self.size - 11 + (index % 3);
			let b = index / 3;
			self.set_function(a, b, bit);
			self.set_function(b, a, bit);
		}
	}

	fn draw_codewords(&mut self, data: &[u8]) {
		let mut index = 0usize;
		let total_bits = data.len() * 8;
		let mut right = self.size as isize - 1;
		while right >= 1 {
			if right == 6 {
				right = 5;
			}
			for vert in 0..self.size {
				for j in 0..2isize {
					let x = (right - j) as usize;
					let upward = ((right + 1) & 2) == 0;
					let y = if upward { self.size - 1 - vert } else { vert };
					if !self.is_function[y][x] && index < total_bits {
						self.modules[y][x] = get_bit(data[index / 8] as u32, (7 - (index & 7)) as u32);
						index += 1;
					}
				}
			}
			right -= 2;
		}
	}

	fn apply_mask(&mut self, mask: usize) {
		for y in 0..self.size {
			for x in 0..self.size {
				if !self.is_function[y][x] && mask_bit(mask, x, y) {
					self.modules[y][x] = !self.modules[y][x];
				}
			}
		}
	}

	fn select_mask(&mut self, forced: Option<usize>) -> usize {
		let mut mask = forced;
		if mask.is_none() {
			let mut min_penalty = i64::MAX;
			for candidate in 0..8 {
				self.apply_mask(candidate);
				self.draw_format_bits(candidate);
				let penalty = self.penalty_score();
				if penalty < min_penalty {
					mask = Some(candidate);
					min_penalty = penalty;
				}
				self.apply_mask(candidate);
			}
		}
		let mask = mask.expect("a mask was selected");
		self.apply_mask(mask);
		self.draw_format_bits(mask);
		mask
	}

	fn penalty_score(&self) -> i64 {
		let mut result: i64 = 0;
		let size = self.size;

		for y in 0..size {
			let mut run_color = false;
			let mut run_len = 0u32;
			let mut history = [0u32; 7];
			for x in 0..size {
				if self.modules[y][x] == run_color {
					run_len += 1;
					if run_len == 5 {
						result += PENALTY_N1;
					} else if run_len > 5 {
						result += 1;
					}
				} else {
					Self::finder_add_history(run_len, &mut history, size);
					if !run_color {
						result += Self::finder_count_patterns(&history) as i64 * PENALTY_N3;
					}
					run_color = self.modules[y][x];
					run_len = 1;
				}
			}
			result += Self::finder_terminate(run_color, run_len, &mut history, size) as i64 * PENALTY_N3;
		}

		for x in 0..size {
			let mut run_color = false;
			let mut run_len = 0u32;
			let mut history = [0u32; 7];
			for y in 0..size {
				if self.modules[y][x] == run_color {
					run_len += 1;
					if run_len == 5 {
						result += PENALTY_N1;
					} else if run_len > 5 {
						result += 1;
					}
				} else {
					Self::finder_add_history(run_len, &mut history, size);
					if !run_color {
						result += Self::finder_count_patterns(&history) as i64 * PENALTY_N3;
					}
					run_color = self.modules[y][x];
					run_len = 1;
				}
			}
			result += Self::finder_terminate(run_color, run_len, &mut history, size) as i64 * PENALTY_N3;
		}

		for y in 0..size.saturating_sub(1) {
			for x in 0..size.saturating_sub(1) {
				let color = self.modules[y][x];
				if color == self.modules[y][x + 1]
					&& color == self.modules[y + 1][x]
					&& color == self.modules[y + 1][x + 1]
				{
					result += PENALTY_N2;
				}
			}
		}

		let mut dark = 0i64;
		for y in 0..size {
			for x in 0..size {
				if self.modules[y][x] {
					dark += 1;
				}
			}
		}
		let total = (size * size) as i64;
		let k = ((dark * 20 - total * 10).abs() + total - 1) / total - 1;
		result += k * PENALTY_N4;
		result
	}

	fn finder_count_patterns(history: &[u32; 7]) -> u32 {
		let n = history[1];
		let core = n > 0 && history[2] == n && history[3] == n * 3 && history[4] == n && history[5] == n;
		(if core && history[0] >= n * 4 && history[6] >= n { 1 } else { 0 })
			+ (if core && history[6] >= n * 4 && history[0] >= n { 1 } else { 0 })
	}

	fn finder_add_history(run_len: u32, history: &mut [u32; 7], size: usize) {
		let mut run_len = run_len;
		if history[0] == 0 {
			run_len += size as u32;
		}
		history.copy_within(0..6, 1);
		history[0] = run_len;
	}

	fn finder_terminate(run_color: bool, run_len: u32, history: &mut [u32; 7], size: usize) -> u32 {
		let mut run_len = run_len;
		if run_color {
			Self::finder_add_history(run_len, history, size);
			run_len = 0;
		}
		run_len += size as u32;
		Self::finder_add_history(run_len, history, size);
		Self::finder_count_patterns(history)
	}
}

/// Render a QR symbol as plain half-block rows (no escapes) for a terminal that
/// draws text in its own foreground color. `ink_light` draws the *light*
/// modules (quiet zone included) as ink.
pub fn render_qr_text(qr: &QrCode, ink_light: bool, options: Option<QrRenderOptions>) -> Vec<String> {
	let margin = options.map(|options| options.margin).unwrap_or(4);
	let dim = qr.size + margin * 2;
	let ink = |gx: usize, gy: usize| -> bool {
		let x = gx as isize - margin as isize;
		let y = gy as isize - margin as isize;
		let dark = x >= 0
			&& (x as usize) < qr.size
			&& y >= 0
			&& (y as usize) < qr.size
			&& qr.module(x as usize, y as usize);
		gy < dim && dark != ink_light
	};
	let mut lines = Vec::new();
	let mut gy = 0;
	while gy < dim {
		let mut row = String::new();
		for gx in 0..dim {
			let top = ink(gx, gy);
			let bottom = ink(gx, gy + 1);
			row.push(if top {
				if bottom {
					'█'
				} else {
					'▀'
				}
			} else if bottom {
				'▄'
			} else {
				' '
			});
		}
		lines.push(row);
		gy += 2;
	}
	lines
}

/// Render a QR symbol as ANSI half-block rows: each text row packs two module
/// rows via `▀`/`▄`/`█`, drawn black-on-white so a phone camera reads dark
/// modules as data and the quiet zone as the light margin.
pub fn render_qr_half_blocks(qr: &QrCode, options: Option<QrRenderOptions>) -> Vec<String> {
	let margin = options.map(|options| options.margin).unwrap_or(4);
	let dim = qr.size + margin * 2;
	let dark = |gx: usize, gy: usize| -> bool {
		let x = gx as isize - margin as isize;
		let y = gy as isize - margin as isize;
		x >= 0
			&& (x as usize) < qr.size
			&& y >= 0
			&& (y as usize) < qr.size
			&& qr.module(x as usize, y as usize)
	};
	let mut lines = Vec::new();
	let mut gy = 0;
	while gy < dim {
		let mut row = String::from(ANSI_QR_ROW_PREFIX);
		for gx in 0..dim {
			let top = dark(gx, gy);
			let bottom = gy + 1 < dim && dark(gx, gy + 1);
			row.push(if top {
				if bottom {
					'█'
				} else {
					'▀'
				}
			} else if bottom {
				'▄'
			} else {
				' '
			});
		}
		row.push_str(ANSI_RESET);
		lines.push(row);
		gy += 2;
	}
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encodes_a_short_url_at_version_one() {
		let qr = QrCode::encode_text("https://example.com/collab", QrEcLevel::M).expect("encodes");
		assert_eq!(qr.version, 1);
		assert_eq!(qr.size, 21);
		assert!((0..8).contains(&qr.mask));
	}

	#[test]
	fn encoding_is_deterministic() {
		let first = QrCode::encode_text("https://example.com/collab", QrEcLevel::M).expect("encodes");
		let second = QrCode::encode_text("https://example.com/collab", QrEcLevel::M).expect("encodes");
		assert_eq!(first.mask, second.mask);
		for y in 0..first.size {
			for x in 0..first.size {
				assert_eq!(first.module(x, y), second.module(x, y));
			}
		}
	}

	#[test]
	fn forced_mask_is_honoured_and_invalid_masks_error() {
		let qr = QrCode::encode_bytes(
			b"hi",
			QrEcLevel::M,
			Some(QrEncodeOptions { mask: Some(5), ..Default::default() }),
		)
		.expect("encodes");
		assert_eq!(qr.mask, 5);
		assert!(QrCode::encode_bytes(
			b"hi",
			QrEcLevel::M,
			Some(QrEncodeOptions { mask: Some(8), ..Default::default() }),
		)
		.is_err());
	}

	#[test]
	fn oversized_data_is_rejected() {
		let data = vec![0u8; 3000];
		assert!(QrCode::encode_bytes(&data, QrEcLevel::M, None).is_err());
	}

	#[test]
	fn half_blocks_paint_the_quiet_zone_as_a_white_margin() {
		let qr = QrCode::encode_text("https://example.com/collab", QrEcLevel::M).expect("encodes");
		let lines = render_qr_half_blocks(&qr, None);
		assert_eq!(lines.len(), (qr.size + 9) / 2);
		assert!(lines[0].starts_with(ANSI_QR_ROW_PREFIX));
		assert!(lines[0].ends_with(ANSI_RESET));
		let body = &lines[0][ANSI_QR_ROW_PREFIX.len()..lines[0].len() - ANSI_RESET.len()];
		assert!(body.chars().all(|ch| ch == ' '));
	}

	#[test]
	fn plain_text_render_inverts_with_ink_light() {
		let qr = QrCode::encode_text("https://example.com/collab", QrEcLevel::M).expect("encodes");
		let dark = render_qr_text(&qr, false, None);
		let light = render_qr_text(&qr, true, None);
		assert_eq!(dark.len(), light.len());
		let dark_ink = dark.join("").matches(['█', '▀', '▄']).count();
		let light_ink = light.join("").matches(['█', '▀', '▄']).count();
		assert!(dark_ink > 0);
		assert!(light_ink > 0);
	}

	#[test]
	fn margin_option_widens_the_symbol() {
		let qr = QrCode::encode_text("https://example.com/collab", QrEcLevel::M).expect("encodes");
		let tight = render_qr_half_blocks(&qr, Some(QrRenderOptions { margin: 0 }));
		let wide = render_qr_half_blocks(&qr, None);
		assert!(tight[0].len() < wide[0].len());
	}

	#[test]
	fn ec_level_round_trips_through_its_string() {
		for level in [QrEcLevel::L, QrEcLevel::M, QrEcLevel::Q, QrEcLevel::H] {
			assert_eq!(QrEcLevel::from_str(level.as_str()), Some(level));
		}
		assert_eq!(QrEcLevel::from_str("X"), None);
	}
}
