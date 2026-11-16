//! Row-move planning for paints a terminal may present half-applied.
//!
//! Reference: `l/src/render/row-moves.ts`.
//!
//! Without synchronized output (DEC 2026) a terminal can show any prefix of a
//! frame's bytes. Repainting a viewport whose content shifted row by row,
//! top-down, then exposes rows already rewritten above rows that have not moved
//! yet, and the seam shows the shifted rows twice. Moving the surviving rows
//! with delete-line/insert-line first keeps every intermediate screen a
//! coherent, shifted copy of the previous frame, and leaves only the rows that
//! really changed to rewrite.
//!
//! Rows are integer keys: [`BLANK_ROW`] for an empty row, [`UNKNOWN_ROW`] for
//! committed history the planner does not model (never matched, never moved),
//! and any other value for one distinct line.
//!
//! Deviation: the reference indexes its row arrays with JS numbers, so an
//! upward band can compute a negative index (`start + shift` when `start <
//! distance`) or one past the end, and reads `NaN` (`hits[-1]!`, `blanks[7]!`).
//! `NaN` arithmetic makes the gain `NaN`, and `NaN > bestGain` is false, so
//! such a band is never selected. `usize` cannot represent either index, so the
//! port keeps the index in `isize` and models the reference read exactly: a
//! negative or out-of-range index yields `None`, which poisons the gain and
//! suppresses the candidate. `base` is clamped to `screen.len()` because the
//! reference's `height - base` is negative when `base` exceeds the height and
//! its scan loop then never runs.

/// Key of an empty row.
pub const BLANK_ROW: i32 = 0;
/// Key of a row whose content is not modeled; it matches nothing.
pub const UNKNOWN_ROW: i32 = -1;

/// `count` rows starting at screen row `from` move to start at `to`; the rows
/// they vacate turn blank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowMove {
	/// First screen row of the band that moves.
	pub from: usize,
	/// Screen row the band lands on.
	pub to: usize,
	/// Number of rows in the band.
	pub count: usize,
}

/// The band move that turns the most rows of `screen` into `target`, or `None`
/// when no move saves a row rewrite. Only rows at or below `base` are moved or
/// vacated. An upward move never lands on row 0: it deletes lines at its
/// destination, and Terminal.app archives lines deleted from the top row into
/// scrollback.
pub fn find_row_move(screen: &[i32], target: &[i32], base: usize) -> Option<RowMove> {
	let height = screen.len();
	let base = base.min(height);
	// hits[r]: rows above r already matching in place; blanks[r]: rows above r
	// whose target is blank (a vacated row then matches).
	let mut hits = vec![0_i32; height + 1];
	let mut blanks = vec![0_i32; height + 1];
	for r in 0..height {
		let hit = target[r] != UNKNOWN_ROW && screen[r] == target[r];
		blanks[r + 1] = blanks[r] + i32::from(target[r] == BLANK_ROW);
		hits[r + 1] = hits[r] + i32::from(hit);
	}
	// The reference reads its prefix arrays with JS number indices: an index
	// outside `0..=height` yields `NaN`, the gain becomes `NaN`, and
	// `NaN > bestGain` drops the band. The port keeps the window bounds signed so
	// those reads fail the same way.
	let prefix = |table: &[i32], index: isize| -> Option<i32> {
		(0..=height as isize).contains(&index).then(|| table[index as usize])
	};
	let mut best: Option<RowMove> = None;
	let mut best_gain = 0_i32;
	// Nearer shifts first, so a tie keeps the smaller move.
	let mut distance = 1;
	while distance < height - base {
		for sign in [1_isize, -1] {
			let shift = sign * distance as isize;
			let first = if shift > 0 { base.max(1) } else { base + distance };
			let last = if shift > 0 { height - distance } else { height };
			let mut start: Option<usize> = None;
			let mut content = false;
			let mut r = first;
			while r <= last {
				let source = r as isize + shift;
				let matched = r < last
					&& source >= 0
					&& source < height as isize
					&& target[r] != UNKNOWN_ROW
					&& screen[source as usize] == target[r];
				if matched {
					if start.is_none() {
						start = Some(r);
					}
					if target[r] != BLANK_ROW {
						content = true;
					}
					r += 1;
					continue;
				}
				if let (Some(start), true) = (start, content) {
					let count = r - start;
					// Rows the move rewrites: its destination plus the band it
					// vacates. The reference's `start + shift` is negative only for
					// an upward band starting within `distance` of the top; the
					// port's signed window reads out of bounds there too, so the
					// same band is dropped.
					let vacated_start = if shift > 0 { r as isize } else { start as isize + shift };
					let vacated_end = if shift > 0 { r as isize + shift } else { start as isize };
					let gain = [
                        prefix(&hits, r as isize),
                        prefix(&hits, start as isize),
						prefix(&blanks, vacated_end),
						prefix(&blanks, vacated_start),
						prefix(&hits, vacated_end),
						prefix(&hits, vacated_start),
					]
					.into_iter()
					.collect::<Option<Vec<_>>>()
					.map(|v| count as i32 - (v[0] - v[1]) + (v[2] - v[3]) - (v[4] - v[5]));
					let Some(gain) = gain else {
						continue;
					};
					// `from` is `start + shift`, which is `start - distance` for an
					// upward move; it is only reached when the band matched, so it
					// is non-negative and inside the screen.
					let from = (start as isize + shift) as usize;
					if gain > best_gain {
						best = Some(RowMove { from, to: start, count });
						best_gain = gain;
					}
				}
				start = None;
				content = false;
				r += 1;
			}
		}
		distance += 1;
	}
	best
}

/// Apply `move` to a screen model, as [`row_move_sequence`] applies it to the
/// terminal.
pub fn apply_row_move(screen: &mut [i32], move_: RowMove) {
	let RowMove { from, to, count } = move_;
	let band = screen[from..from + count].to_vec();
	if from < to {
		screen[from..to].fill(BLANK_ROW);
	} else {
		screen[to + count..from + count].fill(BLANK_ROW);
	}
	for (i, row) in band.into_iter().enumerate() {
		screen[to + i] = row;
	}
}

/// Terminal bytes performing `move` on a `height`-row screen with default
/// margins: rows outside the band and its vacated rows keep their place. Lines
/// inserted or deleted take the current background, so callers reset SGR first.
pub fn row_move_sequence(move_: RowMove, height: usize) -> String {
	let RowMove { from, to, count } = move_;
	if to < from {
		let shift = from - to;
		let up = format!("\x1b[{};1H\x1b[{}M", to + 1, shift);
		return if from + count < height {
			format!("{up}\x1b[{};1H\x1b[{}L", to + count + 1, shift)
		} else {
			up
		};
	}
	let shift = to - from;
	let down = format!("\x1b[{};1H\x1b[{}L", from + 1, shift);
	if to + count < height {
		format!("\x1b[{};1H\x1b[{}M{down}", from + count + 1, shift)
	} else {
		down
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn no_move_when_rows_already_match() {
		assert_eq!(find_row_move(&[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5], 0), None);
	}

	#[test]
	fn downward_shift_of_a_band() {
		// Content enters at the top and pushes the band one row down; the rows the
		// band vacates are blank in the target.
		assert_eq!(
			find_row_move(&[1, 2, 0, 9, 9], &[0, 1, 2, 0, 9], 0),
			Some(RowMove { from: 0, to: 1, count: 4 })
		);
	}

	#[test]
	fn upward_shift_of_a_band() {
		// The band slides two rows up and vacates its bottom rows.
		assert_eq!(
			find_row_move(&[0, 0, 1, 2, 3], &[1, 2, 3, 0, 0], 0),
			Some(RowMove { from: 3, to: 1, count: 2 })
		);
	}

	#[test]
	fn band_that_vacates_rows() {
		// A one-row downward shift: the destination rows are blank in the target
		// and the band's vacated tail is blank too.
		assert_eq!(
			find_row_move(&[9, 1, 2, 0, 9], &[9, 0, 1, 2, 0], 0),
			Some(RowMove { from: 1, to: 2, count: 3 })
		);
	}

	#[test]
	fn unknown_rows_never_match() {
		assert_eq!(
			find_row_move(&[1, 2, 3, 0, 0], &[1, 2, 3, UNKNOWN_ROW, 0], 0),
			None
		);
	}

	#[test]
	fn ties_keep_the_nearer_move() {
		// A distance-1 shift of rows 3..5 and a distance-2 shift of rows 2..4 both
		// score 2. Distance 1 is scanned first and strict `>` keeps it, so the
		// smaller move wins.
		assert_eq!(
			find_row_move(&[1, 3, 1, 4, 2, 1], &[2, 2, 2, 1, 4, 3], 2),
			Some(RowMove { from: 2, to: 3, count: 2 })
		);
	}

	#[test]
	fn base_above_height_selects_nothing() {
		assert_eq!(find_row_move(&[1, 2, 3], &[3, 2, 1], 5), None);
	}

	#[test]
	fn apply_row_move_down_vacates_the_band() {
		let mut screen = vec![1, 2, 3, 0, 0];
		apply_row_move(&mut screen, RowMove { from: 0, to: 2, count: 3 });
		assert_eq!(screen, vec![0, 0, 1, 2, 3]);
	}

	#[test]
	fn apply_row_move_up_vacates_the_tail() {
		let mut screen = vec![0, 0, 1, 2, 3];
		apply_row_move(&mut screen, RowMove { from: 2, to: 0, count: 3 });
		assert_eq!(screen, vec![1, 2, 3, 0, 0]);
	}

	#[test]
	fn sequence_up_without_insert() {
		assert_eq!(
			row_move_sequence(RowMove { from: 2, to: 0, count: 3 }, 5),
			"\x1b[1;1H\x1b[2M"
		);
	}

	#[test]
	fn sequence_up_with_insert() {
		assert_eq!(
			row_move_sequence(RowMove { from: 2, to: 0, count: 2 }, 5),
			"\x1b[1;1H\x1b[2M\x1b[3;1H\x1b[2L"
		);
	}

	#[test]
	fn sequence_down_without_delete() {
		assert_eq!(
			row_move_sequence(RowMove { from: 0, to: 2, count: 3 }, 5),
			"\x1b[1;1H\x1b[2L"
		);
	}

	#[test]
	fn sequence_down_with_delete() {
		assert_eq!(
			row_move_sequence(RowMove { from: 0, to: 2, count: 2 }, 5),
			"\x1b[3;1H\x1b[2M\x1b[1;1H\x1b[2L"
		);
	}

	#[test]
	fn find_then_apply_reaches_the_target() {
		let screen = vec![1, 2, 0, 9, 9];
		let target = [0, 1, 2, 0, 9];
		let move_ = find_row_move(&screen, &target, 0).expect("a saving move exists");
		assert_eq!(row_move_sequence(move_, screen.len()), "\x1b[1;1H\x1b[1L");
		let mut applied = screen.clone();
		apply_row_move(&mut applied, move_);
		assert_eq!(applied, vec![0, 1, 2, 0, 9]);
	}
}
