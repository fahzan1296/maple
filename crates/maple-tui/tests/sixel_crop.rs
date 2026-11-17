//! Port of `l/test/sixel-crop.test.ts`.
//!
//! Tests `maple_tui::terminal_capabilities::{crop_sixel_top, crop_sixel_top_max}`.

use maple_tui::terminal_capabilities::{crop_sixel_top, crop_sixel_top_max};

#[derive(Debug)]
struct Decoded {
	width: usize,
	height: usize,
	first_band_width: usize,
	pixels: Vec<Vec<Option<String>>>,
}

fn decode(sequence: &str) -> Decoded {
	let bytes = sequence.as_bytes();
	assert!(bytes.starts_with(b"\x1bP"), "Invalid SIXEL fixture");
	let header_end = sequence.find('q').expect("Invalid SIXEL fixture") + 1;
	let raster_start = sequence[header_end..].find('"').expect("Invalid SIXEL fixture") + header_end + 1;
	let mut nums = Vec::new();
	let mut i = raster_start;
	while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b';') {
		let start = i;
		while i < bytes.len() && bytes[i].is_ascii_digit() {
			i += 1;
		}
		nums.push(sequence[start..i].parse::<usize>().unwrap());
		if i < bytes.len() && bytes[i] == b';' {
			i += 1;
		} else {
			break;
		}
	}
	assert!(nums.len() >= 4, "Invalid SIXEL fixture");
	assert!(sequence.ends_with("\x1b\\"), "Invalid SIXEL fixture");
	let width = nums[2];
	let height = nums[3];
	let mut pixels: Vec<Vec<Option<String>>> = vec![vec![None; width]; height];
	let mut palette: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
	let mut color: i64 = 0;
	let mut x: usize = 0;
	let mut y: usize = 0;
	let mut first_band_width: usize = 0;
	let mut offset = header_end;

	while offset < sequence.len().saturating_sub(2) {
		let ch = bytes[offset];
		match ch {
			b'"' => {
				offset += 1;
				while offset < sequence.len().saturating_sub(2)
					&& (bytes[offset].is_ascii_digit() || bytes[offset] == b';')
				{
					offset += 1;
				}
			}
			b'#' => {
				offset += 1;
				let mut vals: Vec<i64> = Vec::new();
				while offset < sequence.len().saturating_sub(2)
					&& (bytes[offset].is_ascii_digit() || bytes[offset] == b';')
				{
					let start = offset;
					while offset < bytes.len() && bytes[offset].is_ascii_digit() {
						offset += 1;
					}
					if start < offset {
						vals.push(sequence[start..offset].parse().unwrap());
					}
					if offset < bytes.len() && bytes[offset] == b';' {
						offset += 1;
					} else {
						break;
					}
				}
				if let Some(&id) = vals.first() {
					color = id;
					if vals.len() == 5 {
						palette.insert(
							id,
							format!("{},{},{}", vals[2], vals[3], vals[4]),
						);
					}
				}
			}
			b'$' => {
				x = 0;
				offset += 1;
			}
			b'-' => {
				x = 0;
				y += 6;
				offset += 1;
			}
			b'!' => {
				offset += 1;
				let start = offset;
				while offset < bytes.len() && bytes[offset].is_ascii_digit() {
					offset += 1;
				}
				let count: usize = sequence[start..offset].parse().unwrap();
				let character = sequence[offset..].chars().next().unwrap();
				offset += character.len_utf8();
				paint(&mut pixels, &palette, color, x, y, width, height, character, count, &mut first_band_width);
				x += count;
			}
			b if (b'?'..=b'~').contains(&b) => {
				let character = sequence[offset..].chars().next().unwrap();
				offset += character.len_utf8();
				paint(&mut pixels, &palette, color, x, y, width, height, character, 1, &mut first_band_width);
				x += 1;
			}
			_ => {
				offset += 1;
			}
		}
	}

	Decoded {
		width,
		height,
		first_band_width,
		pixels,
	}
}

#[allow(clippy::too_many_arguments)]
fn paint(
	pixels: &mut [Vec<Option<String>>],
	palette: &std::collections::HashMap<i64, String>,
	color: i64,
	x: usize,
	y: usize,
	width: usize,
	height: usize,
	character: char,
	count: usize,
	first_band_width: &mut usize,
) {
	let bits = (character as u32).saturating_sub(63) & 0x3f;
	if y == 0 {
		*first_band_width = (*first_band_width).max(x + count);
	}
	for i in 0..count {
		let px = x + i;
		if px >= width {
			continue;
		}
		for row in 0..6 {
			if (bits & (1 << row)) == 0 {
				continue;
			}
			let py = y + row;
			if py >= height {
				continue;
			}
			if let Some(val) = palette.get(&color) {
				pixels[py][px] = Some(val.clone());
			}
		}
	}
}

const RED: &str = "100,0,0";
// Each band uses multiple color passes, transparent columns, and repeated pixels.
const IMAGE: &str = "\x1bP0;1;0q\"1;1;5;18#1;2;100;0;0#2;2;0;100;0#3;2;0;0;100#1!2V?$#2??!2i-#3!3j$#1???@-#2!2~$#3??!2T\x1b\\";

#[test]
fn leaves_zero_pixel_crop_byte_for_byte_unchanged() {
	assert_eq!(crop_sixel_top(IMAGE, 0), IMAGE);
}

#[test]
fn keeps_transparent_leading_bands_visible_without_painting_pixels() {
	let cropped = crop_sixel_top(IMAGE, 6);
	assert!(cropped.starts_with("\x1bP0;1;0q\"1;1;5;12!5?$-"));
	let out = decode(&cropped);
	assert_eq!(out.width, 5);
	assert_eq!(out.height, 12);
	assert_eq!(out.first_band_width, 5);
	assert!(out.pixels.iter().all(|row| row.iter().all(|p| p.is_none())));
}

#[test]
fn retains_image_width_when_cropping_exposes_transparent_first_band() {
	let cropped = crop_sixel_top(IMAGE, 12);
	assert!(cropped.starts_with("\x1bP0;1;0q\"1;1;5;6!5?$-"));
	let out = decode(&cropped);
	assert_eq!(out.width, 5);
	assert_eq!(out.height, 6);
	assert_eq!(out.first_band_width, 5);
}

#[test]
fn survives_every_six_pixel_band_for_arbitrary_offsets() {
	for pixels in [2usize, 6, 7, 17] {
		let out = decode(&crop_sixel_top(IMAGE, pixels));
		let expected = decode(IMAGE);
		for y in 0..expected.height.saturating_sub(pixels) {
			for x in 0..expected.width {
				assert_eq!(out.pixels[y][x], expected.pixels[y + pixels][x], "pixels={pixels} x={x} y={y}");
			}
		}
	}
}

#[test]
fn removes_image_when_crop_consumes_whole_raster() {
	for pixels in [18usize, 19] {
		assert_eq!(crop_sixel_top(IMAGE, pixels), "");
	}
}

#[test]
fn retains_palette_definitions_from_removed_bands_and_transparent_runs() {
	let cropped = crop_sixel_top(IMAGE, 12);
	assert!(cropped.contains("#1;2;100;0;0"));
	assert!(cropped.contains("#2;2;0;100;0"));
	assert!(cropped.contains("#3;2;0;0;100"));
}

#[test]
fn crops_declared_partial_final_band_without_exposing_padding_pixels() {
	let partial = "\x1bPq\"1;1;2;8#1;2;100;0;0#1~-#1N\x1b\\";
	let out = decode(&crop_sixel_top(partial, 5));
	assert_eq!(out.height, 3);
	assert_eq!(out.pixels[0], vec![Some(RED.to_string()), Some(RED.to_string())]);
	assert_eq!(out.pixels[1], vec![None, None]);
	assert_eq!(out.pixels[2], vec![None, None]);
}

#[test]
fn caps_remainder_to_whole_bands_inside_terminal_rows() {
	let capped = crop_sixel_top_max(IMAGE, 2, 40);
	let out = decode(&capped);
	// 18-2=16, capped by floor(40/6)*6=36 -> 16
	assert_eq!(out.height, 16);
	// tighter cap: floor(20/6)*6 = 18, but remainder is only 16
	let capped2 = crop_sixel_top_max(IMAGE, 2, 20);
	let out2 = decode(&capped2);
	assert_eq!(out2.height, 16);
}