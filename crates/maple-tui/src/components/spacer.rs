//! Vertical whitespace.
//!
//! Reference: `l/src/components/spacer.ts`.

use crate::component::Component;

/// Terminal spacing step for a count of blank rows.
///
/// Reference: `l/src/components/spacer.ts::spaceForRows`.
pub fn space_for_rows(rows: usize) -> &'static str {
	if rows == 0 {
		return "none";
	}
	if rows <= 1 {
		return "sm";
	}
	if rows <= 2 {
		return "md";
	}
	"lg"
}

/// Spacer component that renders empty lines.
pub struct Spacer {
	lines: usize,
	cached: Option<Vec<String>>,
}

impl Spacer {
	pub fn new(lines: usize) -> Self {
		Self { lines: if lines == 0 { 1 } else { lines }, cached: None }
	}

	pub fn set_lines(&mut self, lines: usize) {
		let lines = if lines == 0 { 1 } else { lines };
		if lines == self.lines {
			return;
		}
		self.lines = lines;
		self.cached = None;
	}

	pub fn lines(&self) -> usize {
		self.lines
	}

	/// Return the spacer height for debug inspection.
	pub fn debug_state(&self) -> std::collections::HashMap<String, String> {
		let mut state = std::collections::HashMap::new();
		state.insert("height".to_string(), self.lines.to_string());
		state
	}

	pub fn invalidate(&mut self) {
		// No cached render state beyond the blank-line buffer; kept for API parity.
	}
}

impl Component for Spacer {
	fn render(&mut self, _width: u16) -> Vec<String> {
		if let Some(cached) = &self.cached {
			return cached.clone();
		}
		let cached = vec![String::new(); self.lines];
		self.cached = Some(cached.clone());
		cached
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::component::Component;

	#[test]
	fn renders_requested_blank_lines_and_caches() {
		let mut spacer = Spacer::new(3);
		let a = spacer.render(80);
		let b = spacer.render(80);
		assert_eq!(a, vec!["", "", ""]);
		assert_eq!(b, a);
		spacer.set_lines(1);
		assert_eq!(spacer.render(80), vec![""]);
	}

	#[test]
	fn space_for_rows_buckets_match_reference() {
		assert_eq!(space_for_rows(0), "none");
		assert_eq!(space_for_rows(1), "sm");
		assert_eq!(space_for_rows(2), "md");
		assert_eq!(space_for_rows(3), "lg");
		assert_eq!(space_for_rows(10), "lg");
	}

	#[test]
	fn debug_state_reports_height() {
		let spacer = Spacer::new(2);
		assert_eq!(spacer.debug_state().get("height").map(String::as_str), Some("2"));
	}

	#[test]
	fn set_lines_unchanged_keeps_cache_identity_semantics() {
		let mut spacer = Spacer::new(2);
		let _ = spacer.render(80);
		spacer.set_lines(2);
		assert_eq!(spacer.render(80), vec!["", ""]);
	}
}
