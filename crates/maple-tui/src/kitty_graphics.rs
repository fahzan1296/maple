//! Kitty graphics: Unicode placeholder placement (`U=1` + U+10EEEE), with
//! runtime feature state and env overrides.
//!
//! Unicode placeholders let a transmitted image be displayed by writing ordinary
//! text cells — the placeholder char U+10EEEE plus row/column combining
//! diacritics — instead of a cursor-positioned `a=p` direct placement. The image
//! then participates in the normal text grid, so it survives horizontal slicing,
//! reflow and overlapping draws (each cell names its own row+column, so a sliced
//! row still maps to the correct sub-region). See kitty
//! `docs/graphics-protocol.rst` "Unicode placeholders for relative placements".
//!
//! Port of `l/src/kitty-graphics.ts`. Protocol gating (`imageProtocol == Kitty`)
//! lives in the caller, so this module never inspects [`crate::terminal_capabilities`]
//! protocol state. The multiplexer-session helpers it needs are reused from
//! [`crate::terminal_capabilities`], which is also where the detected terminal id
//! seeds [`get_kitty_graphics`].

use std::sync::{LazyLock, Mutex};

use crate::terminal_capabilities::{
    is_inside_terminal_multiplexer, terminal_multiplexer_sessions, wrap_tmux_passthrough_if_needed,
    EnvReader, TerminalId,
};

/// Kitty Unicode placeholder base character (U+10EEEE, Plane 16 PUA).
pub const KITTY_PLACEHOLDER: char = '\u{10eeee}';

/**
 * Row/column diacritics (Unicode combining class 230, no decomposition) used to
 * name a placeholder cell's row and column. Index `i` → codepoint. Derived from
 * kitty `gen/rowcolumn-diacritics.txt` (Unicode 6.0.0 NSM set). 297 entries, so
 * a single image can address up to 297 rows/columns without ID-high-byte tricks.
 */
const ROWCOLUMN_DIACRITICS: [u32; 297] = [
	0x305, 0x30d, 0x30e, 0x310, 0x312, 0x33d, 0x33e, 0x33f, 0x346, 0x34a, 0x34b, 0x34c, 0x350, 0x351, 0x352, 0x357,
	0x35b, 0x363, 0x364, 0x365, 0x366, 0x367, 0x368, 0x369, 0x36a, 0x36b, 0x36c, 0x36d, 0x36e, 0x36f, 0x483, 0x484,
	0x485, 0x486, 0x487, 0x592, 0x593, 0x594, 0x595, 0x597, 0x598, 0x599, 0x59c, 0x59d, 0x59e, 0x59f, 0x5a0, 0x5a1,
	0x5a8, 0x5a9, 0x5ab, 0x5ac, 0x5af, 0x5c4, 0x610, 0x611, 0x612, 0x613, 0x614, 0x615, 0x616, 0x617, 0x657, 0x658,
	0x659, 0x65a, 0x65b, 0x65d, 0x65e, 0x6d6, 0x6d7, 0x6d8, 0x6d9, 0x6da, 0x6db, 0x6dc, 0x6df, 0x6e0, 0x6e1, 0x6e2,
	0x6e4, 0x6e7, 0x6e8, 0x6eb, 0x6ec, 0x730, 0x732, 0x733, 0x735, 0x736, 0x73a, 0x73d, 0x73f, 0x740, 0x741, 0x743,
	0x745, 0x747, 0x749, 0x74a, 0x7eb, 0x7ec, 0x7ed, 0x7ee, 0x7ef, 0x7f0, 0x7f1, 0x7f3, 0x816, 0x817, 0x818, 0x819,
	0x81b, 0x81c, 0x81d, 0x81e, 0x81f, 0x820, 0x821, 0x822, 0x823, 0x825, 0x826, 0x827, 0x829, 0x82a, 0x82b, 0x82c,
	0x82d, 0x951, 0x953, 0x954, 0xf82, 0xf83, 0xf86, 0xf87, 0x135d, 0x135e, 0x135f, 0x17dd, 0x193a, 0x1a17, 0x1a75, 0x1a76,
	0x1a77, 0x1a78, 0x1a79, 0x1a7a, 0x1a7b, 0x1a7c, 0x1b6b, 0x1b6d, 0x1b6e, 0x1b6f, 0x1b70, 0x1b71, 0x1b72, 0x1b73, 0x1cd0, 0x1cd1,
	0x1cd2, 0x1cda, 0x1cdb, 0x1ce0, 0x1dc0, 0x1dc1, 0x1dc3, 0x1dc4, 0x1dc5, 0x1dc6, 0x1dc7, 0x1dc8, 0x1dc9, 0x1dcb, 0x1dcc, 0x1dd1,
	0x1dd2, 0x1dd3, 0x1dd4, 0x1dd5, 0x1dd6, 0x1dd7, 0x1dd8, 0x1dd9, 0x1dda, 0x1ddb, 0x1ddc, 0x1ddd, 0x1dde, 0x1ddf, 0x1de0, 0x1de1,
	0x1de2, 0x1de3, 0x1de4, 0x1de5, 0x1de6, 0x1dfe, 0x20d0, 0x20d1, 0x20d4, 0x20d5, 0x20d6, 0x20d7, 0x20db, 0x20dc, 0x20e1, 0x20e7,
	0x20e9, 0x20f0, 0x2cef, 0x2cf0, 0x2cf1, 0x2de0, 0x2de1, 0x2de2, 0x2de3, 0x2de4, 0x2de5, 0x2de6, 0x2de7, 0x2de8, 0x2de9, 0x2dea,
	0x2deb, 0x2dec, 0x2ded, 0x2dee, 0x2def, 0x2df0, 0x2df1, 0x2df2, 0x2df3, 0x2df4, 0x2df5, 0x2df6, 0x2df7, 0x2df8, 0x2df9, 0x2dfa,
	0x2dfb, 0x2dfc, 0x2dfd, 0x2dfe, 0x2dff, 0xa66f, 0xa67c, 0xa67d, 0xa6f0, 0xa6f1, 0xa8e0, 0xa8e1, 0xa8e2, 0xa8e3, 0xa8e4, 0xa8e5,
	0xa8e6, 0xa8e7, 0xa8e8, 0xa8e9, 0xa8ea, 0xa8eb, 0xa8ec, 0xa8ed, 0xa8ee, 0xa8ef, 0xa8f0, 0xa8f1, 0xaab0, 0xaab2, 0xaab3, 0xaab7,
	0xaab8, 0xaabe, 0xaabf, 0xaac1, 0xfe20, 0xfe21, 0xfe22, 0xfe23, 0xfe24, 0xfe25, 0xfe26, 0x10a0f, 0x10a38, 0x1d185, 0x1d186, 0x1d187,
	0x1d188, 0x1d189, 0x1d1aa, 0x1d1ab, 0x1d1ac, 0x1d1ad, 0x1d242, 0x1d243, 0x1d244,
];

/// Largest row/column index expressible with the diacritic table (one cell each).
pub const KITTY_PLACEHOLDER_MAX_CELLS: usize = ROWCOLUMN_DIACRITICS.len();

/// Runtime Kitty graphics feature state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KittyGraphicsFeatures {
	/// Display images via Unicode placeholders instead of direct `a=p` placement.
	pub unicode_placeholders: bool,
}

impl Default for KittyGraphicsFeatures {
	fn default() -> Self {
		// Off until `terminal_capabilities` seeds it from the detected terminal
		// id — the default-on path corrupts wezterm and tmux-passthrough sessions.
		Self { unicode_placeholders: false }
	}
}

static KITTY_GRAPHICS: LazyLock<Mutex<KittyGraphicsFeatures>> =
	LazyLock::new(|| Mutex::new(KittyGraphicsFeatures::default()));

/// Current Kitty graphics feature state.
pub fn get_kitty_graphics() -> KittyGraphicsFeatures {
	*KITTY_GRAPHICS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the Kitty graphics feature state (the TypeScript original merges a
/// partial; with one field a full replace is equivalent).
pub fn set_kitty_graphics(features: KittyGraphicsFeatures) {
	*KITTY_GRAPHICS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = features;
}

/// Whether the detected terminal renders Kitty Unicode placeholders (`U=1` +
/// U+10EEEE with row/column diacritics).
///
/// Kitty, Ghostty, Otty, Monstar, Rio and Tern advertise placeholder support
/// directly. A multiplexer cannot use cursor-positioned placements because the
/// outer terminal does not know pane scroll/reflow state. An explicit
/// `MAPLE_FORCE_IMAGE_PROTOCOL=kitty` opts into placeholders under any multiplexer —
/// matching `timg -pk`. Automatic fallback stays off inside multiplexers whose
/// pane marker does not prove that a Kitty renderer is enabled (Herdr's is
/// experimental).
///
/// `MAPLE_NO_KITTY_PLACEHOLDERS=1` and `MAPLE_KITTY_PLACEHOLDERS=0` remain hard
/// opt-outs; `MAPLE_KITTY_PLACEHOLDERS=1` explicitly opts in anywhere else.
pub fn detect_kitty_unicode_placeholders_support(terminal_id: TerminalId, env: &impl EnvReader) -> bool {
	let normalized = |value: Option<String>| value.map(|value| value.trim().to_ascii_lowercase()).unwrap_or_default();
	let off_raw = normalized(env.get("MAPLE_NO_KITTY_PLACEHOLDERS"));
	if matches!(off_raw.as_str(), "1" | "true" | "on" | "yes" | "y") {
		return false;
	}
	let force = normalized(env.get("MAPLE_KITTY_PLACEHOLDERS"));
	if matches!(force.as_str(), "1" | "true" | "on" | "yes" | "y") {
		return true;
	}
	if matches!(force.as_str(), "0" | "false" | "off" | "no" | "n") {
		return false;
	}
	if is_inside_terminal_multiplexer(env)
		&& env
			.get("MAPLE_FORCE_IMAGE_PROTOCOL")
			.map(|value| value.trim().eq_ignore_ascii_case("kitty"))
			.unwrap_or(false)
	{
		return true;
	}
	if terminal_multiplexer_sessions(env)
		.iter()
		.any(|multiplexer| multiplexer.images_require_override())
	{
		return false;
	}
	matches!(
		terminal_id,
		TerminalId::Kitty
			| TerminalId::Ghostty
			| TerminalId::Monstar
			| TerminalId::Otty
			| TerminalId::Rio
			| TerminalId::Tern
	)
}

/// Whether a `columns`×`rows` placeholder grid fits within the diacritic table.
pub fn kitty_placeholders_fit(columns: usize, rows: usize) -> bool {
	columns >= 1 && rows >= 1 && columns <= KITTY_PLACEHOLDER_MAX_CELLS && rows <= KITTY_PLACEHOLDER_MAX_CELLS
}

/// Diacritic naming table index `index`, or `None` past the end of the table.
fn diacritic(index: usize) -> Option<char> {
	ROWCOLUMN_DIACRITICS.get(index).and_then(|cp| char::from_u32(*cp))
}

/**
 * Virtual placement APC (`a=p,U=1`): tells the terminal that placeholder cells
 * carrying image id `i` should display the transmitted image, scaled to fit the
 * `c`×`r` cell box. Re-emitting with a stable `placementId` replaces in place.
 */
pub fn encode_kitty_virtual_placement(
	image_id: u32,
	placement_id: Option<u32>,
	columns: usize,
	rows: usize,
	env: &impl EnvReader,
) -> String {
	let mut params = vec![
		"a=p".to_string(),
		"U=1".to_string(),
		"q=2".to_string(),
		format!("i={image_id}"),
	];
	if let Some(placement_id) = placement_id {
		params.push(format!("p={placement_id}"));
	}
	params.push(format!("c={columns}"));
	params.push(format!("r={rows}"));
	let payload = format!("\x1b_G{}\x1b\\", params.join(","));
	wrap_tmux_passthrough_if_needed(&payload, env)
}

/**
 * Build the placeholder cell grid as one string per row. The image id is carried
 * in each row's foreground color and the placement id (if any) in its underline
 * color; every cell names its explicit row+column diacritic (robust to slicing,
 * unlike left-inheritance). Returns exactly `rows` strings.
 */
pub fn encode_kitty_placeholder_grid(
	image_id: u32,
	placement_id: Option<u32>,
	columns: usize,
	rows: usize,
) -> Vec<String> {
	let fg = format!(
		"\x1b[38;2;{};{};{}m",
		(image_id >> 16) & 0xff,
		(image_id >> 8) & 0xff,
		image_id & 0xff
	);
	let underline = placement_id.map_or_else(String::new, |placement_id| {
		format!(
			"\x1b[58:2::{}:{}:{}m",
			(placement_id >> 16) & 0xff,
			(placement_id >> 8) & 0xff,
			placement_id & 0xff
		)
	});
	let reset = "\x1b[39;59m";
	let lead = format!("{fg}{underline}");
	let mut out = Vec::with_capacity(rows);
	for r in 0..rows {
		let row_diacritic = diacritic(r).map_or_else(String::new, |c| c.to_string());
		let mut row = lead.clone();
		for c in 0..columns {
			row.push(KITTY_PLACEHOLDER);
			row.push_str(&row_diacritic);
			if let Some(column_diacritic) = diacritic(c) {
				row.push(column_diacritic);
			}
		}
		out.push(format!("{row}{reset}"));
	}
	out
}

/**
 * Full placeholder render: the virtual-placement APC prefixes line 0, and every
 * line carries placeholder cells. Returns exactly `rows` lines (no cursor moves).
 */
pub fn render_kitty_placeholder_lines(
	image_id: u32,
	placement_id: Option<u32>,
	columns: usize,
	rows: usize,
	env: &impl EnvReader,
) -> Vec<String> {
	let mut grid = encode_kitty_placeholder_grid(image_id, placement_id, columns, rows);
	if let Some(first) = grid.first_mut() {
		first.insert_str(0, &encode_kitty_virtual_placement(image_id, placement_id, columns, rows, env));
	}
	grid
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::terminal_capabilities::{MultiplexerSession, Platform, TestEnv};

	fn env(vars: &[(&str, &str)]) -> TestEnv {
		let mut env = TestEnv::new();
		for (key, value) in vars {
			env.insert(key, value);
		}
		env
	}

	#[test]
	fn exposes_placeholder_constant_and_table_size() {
		assert_eq!(KITTY_PLACEHOLDER, '\u{10eeee}');
		assert_eq!(KITTY_PLACEHOLDER_MAX_CELLS, 297);
		assert_eq!(diacritic(0), Some('\u{305}'));
		assert_eq!(diacritic(296), Some('\u{1d244}'));
		assert_eq!(diacritic(297), None);
	}

	#[test]
	fn placeholder_grid_fits_only_inside_table() {
		assert!(kitty_placeholders_fit(1, 1));
		assert!(kitty_placeholders_fit(297, 297));
		assert!(!kitty_placeholders_fit(0, 1));
		assert!(!kitty_placeholders_fit(1, 0));
		assert!(!kitty_placeholders_fit(298, 1));
		assert!(!kitty_placeholders_fit(1, 298));
	}

	#[test]
	fn grid_has_one_row_per_request_and_columns_per_row() {
		let grid = encode_kitty_placeholder_grid(0x112233, Some(0x445566), 3, 2);
		assert_eq!(grid.len(), 2);
		for row in &grid {
			assert!(row.starts_with("\x1b[38;2;17;34;51m\x1b[58:2::68:85:102m"));
			assert!(row.ends_with("\x1b[39;59m"));
			assert_eq!(row.matches(KITTY_PLACEHOLDER).count(), 3);
		}
		// Row diacritics differ, column diacritics repeat across the row.
		assert_ne!(grid[0], grid[1]);
	}

	#[test]
	fn grid_without_placement_id_omits_underline_color() {
		let grid = encode_kitty_placeholder_grid(1, None, 1, 1);
		assert!(!grid[0].contains("\x1b[58:"));
	}

	#[test]
	fn virtual_placement_carries_ids_and_box() {
		let env = env(&[]);
		assert_eq!(
			encode_kitty_virtual_placement(7, Some(9), 4, 2, &env),
			"\x1b_Ga=p,U=1,q=2,i=7,p=9,c=4,r=2\x1b\\"
		);
		assert_eq!(
			encode_kitty_virtual_placement(7, None, 4, 2, &env),
			"\x1b_Ga=p,U=1,q=2,i=7,c=4,r=2\x1b\\"
		);
	}

	#[test]
	fn virtual_placement_wraps_inside_tmux() {
		let env = env(&[("TMUX", "/tmp/tmux-1000/default,12,0")]);
		assert!(encode_kitty_virtual_placement(7, None, 4, 2, &env).starts_with("\x1bPtmux;"));
	}

	#[test]
	fn render_prefixes_first_line_with_placement_apc() {
		let env = env(&[]);
		let lines = render_kitty_placeholder_lines(7, Some(9), 2, 3, &env);
		assert_eq!(lines.len(), 3);
		assert!(lines[0].starts_with("\x1b_Ga=p,U=1,q=2,i=7,p=9,c=2,r=3\x1b\\"));
		assert!(!lines[1].starts_with("\x1b_Ga=p"));
		assert_eq!(lines[0].matches(KITTY_PLACEHOLDER).count(), 2);
	}

	#[test]
	fn placeholder_support_honors_opt_out_and_opt_in() {
		let off = env(&[("MAPLE_NO_KITTY_PLACEHOLDERS", "1")]);
		assert!(!detect_kitty_unicode_placeholders_support(TerminalId::Kitty, &off));

		let both = env(&[("MAPLE_KITTY_PLACEHOLDERS", "0"), ("MAPLE_NO_KITTY_PLACEHOLDERS", "1")]);
		assert!(!detect_kitty_unicode_placeholders_support(TerminalId::Kitty, &both));

		let force = env(&[("MAPLE_KITTY_PLACEHOLDERS", "yes")]);
		assert!(detect_kitty_unicode_placeholders_support(TerminalId::Warp, &force));
	}

	#[test]
	fn placeholder_support_allows_only_known_terminal_ids() {
		let env = env(&[]);
		for id in [
			TerminalId::Kitty,
			TerminalId::Ghostty,
			TerminalId::Monstar,
			TerminalId::Otty,
			TerminalId::Rio,
			TerminalId::Tern,
		] {
			assert!(detect_kitty_unicode_placeholders_support(id, &env), "{id:?}");
		}
		for id in [
			TerminalId::Wezterm,
			TerminalId::Warp,
			TerminalId::ITerm2,
			TerminalId::VSCode,
			TerminalId::Alacritty,
			TerminalId::Base,
		] {
			assert!(!detect_kitty_unicode_placeholders_support(id, &env), "{id:?}");
		}
	}

	#[test]
	fn placeholder_support_inside_multiplexer_follows_override_rules() {
		// tmux does not set `imagesRequireOverride`, so a leaked kitty id still
		// allows placeholders there; Herdr's pane marker does block them.
		let tmux = env(&[("TMUX", "/tmp/tmux-1000/default,12,0")]);
		assert!(detect_kitty_unicode_placeholders_support(TerminalId::Kitty, &tmux));
		let herdr = env(&[("HERDR_ENV", "1")]);
		assert!(!detect_kitty_unicode_placeholders_support(TerminalId::Kitty, &herdr));
		let forced = env(&[("HERDR_ENV", "1"), ("MAPLE_FORCE_IMAGE_PROTOCOL", "kitty")]);
		assert!(detect_kitty_unicode_placeholders_support(TerminalId::Kitty, &forced));
	}

	#[test]
	fn multiplexer_sessions_recognize_session_markers() {
		let tmux = env(&[("TMUX", "x")]);
		assert_eq!(terminal_multiplexer_sessions(&tmux), vec![MultiplexerSession::Tmux]);
		let term_only = env(&[("TERM", "tmux-256color")]);
		assert_eq!(terminal_multiplexer_sessions(&term_only), Vec::new());
		assert!(is_inside_terminal_multiplexer(&term_only));
	}

	#[test]
	fn feature_state_round_trips() {
		let saved = get_kitty_graphics();
		set_kitty_graphics(KittyGraphicsFeatures { unicode_placeholders: true });
		assert!(get_kitty_graphics().unicode_placeholders);
		set_kitty_graphics(saved);
		assert_eq!(get_kitty_graphics(), saved);
	}

}
