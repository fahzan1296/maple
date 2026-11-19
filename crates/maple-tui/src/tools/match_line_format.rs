//! Match-line formatting for grep/ast-grep style results.
//!
//! Reference: `l/src/tools/match-line-format.ts`.

/// Format a single line of match output for grep/ast-grep style results.
///
/// Matched lines are prefixed with `*`; context lines are prefixed with a space
/// so line numbers align. In hashline mode the line uses the editable
/// `LINE:content` shape; otherwise the legacy `LINE|content` display shape.
/// Line numbers are never padded.
pub fn format_match_line(line_number: usize, line: &str, is_match: bool, use_hash_lines: bool) -> String {
	let marker = if is_match { '*' } else { ' ' };
	if use_hash_lines {
		format!("{marker}{line_number}:{line}")
	} else {
		format!("{marker}{line_number}|{line}")
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn hashline_match_and_context() {
		assert_eq!(format_match_line(12, "let x = 1;", true, true), "*12:let x = 1;");
		assert_eq!(format_match_line(13, "let y = 2;", false, true), " 13:let y = 2;");
	}

	#[test]
	fn plain_match_and_context() {
		assert_eq!(format_match_line(7, "fn main() {}", true, false), "*7|fn main() {}");
		assert_eq!(format_match_line(8, "}", false, false), " 8|}");
	}

	#[test]
	fn line_numbers_not_padded() {
		assert_eq!(format_match_line(1, "a", false, true), " 1:a");
		assert_eq!(format_match_line(1000, "a", true, false), "*1000|a");
	}
}
