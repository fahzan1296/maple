//! OSC 7501 program-status negotiation.
//!
//! Reference: `l/src/terminal.ts` (`#queryProgramStatusSupport`,
//! `#resolveProgramStatusSupport`, `#publishProgramStatusSupport`).
//!
//! The probe is a pure state machine: [`ProgramStatusProbe::begin`] returns the
//! bytes to write, [`ProgramStatusProbe::feed`] consumes complete stdin
//! sequences and [`ProgramStatusProbe::stop`] invalidates support. Subscribers
//! are notified in place; the raw-stdin seam lives on `ProcessTerminal`.

use crate::terminal_capabilities::{is_inside_terminal_multiplexer, EnvReader};

/// Query written when a probe starts: OSC 7501 feature query plus a DA1
/// sentinel, so a terminal that answers only DA1 resolves as unsupported.
pub const PROGRAM_STATUS_QUERY: &str = "\x1b]7501;?\x1b\\\x1b[c";

/// Bytes the terminal should receive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramStatusAction {
	/// Write these bytes to the terminal.
	Write(String),
}

/// Parse an OSC 7501 feature reply into a support verdict.
///
/// `None` when `sequence` is not a complete `OSC 7501 ; ? …` reply. A reply
/// that omits `states=` declares the protocol's implicit state set and is
/// supported; an explicit set must contain both `working` and `blocked`.
/// Whitespace around keys and values is stripped.
pub fn parse_program_status_reply(sequence: &str) -> Option<bool> {
	let rest = sequence.strip_prefix("\x1b]7501;?")?;
	let body = if let Some(body) = rest.strip_suffix("\x1b\\") {
		body
	} else if let Some(body) = rest.strip_suffix('\x07') {
		body
	} else {
		return None;
	};
	if !body.is_empty() && !body.starts_with(':') {
		return None;
	}
	if body.contains('\x07') || body.contains('\x1b') {
		return None;
	}
	let mut states: Option<Vec<&str>> = None;
	if let Some(pairs) = body.strip_prefix(':') {
		for pair in pairs.split(':') {
			let Some(equals) = pair.find('=') else {
				continue;
			};
			if pair[..equals].trim() != "states" {
				continue;
			}
			states = Some(pair[equals + 1..].split(',').map(str::trim).collect());
		}
	}
	Some(states.is_none_or(|states| {
		states.iter().any(|state| *state == "working")
			&& states.iter().any(|state| *state == "blocked")
	}))
}

/// Whether `sequence` is a DA1 (`CSI ? … c`) reply.
pub fn is_da1_response(sequence: &str) -> bool {
	let Some(rest) = sequence.as_bytes().strip_prefix(b"\x1b[?") else {
		return false;
	};
	let Some((last, middle)) = rest.split_last() else {
		return false;
	};
	*last == b'c' && middle.iter().all(|byte| byte.is_ascii_digit() || *byte == b';')
}

/// Whether `sequence` is a program-status reply or a DA1 sentinel.
pub fn is_program_status_sequence(sequence: &str) -> bool {
	sequence.starts_with("\x1b]7501;") || is_da1_response(sequence)
}

/// OSC 7501 support state shared across terminal restarts.
#[derive(Default)]
pub struct ProgramStatusProbe {
	pending: bool,
	result: Option<bool>,
	reply_buffer: String,
	callbacks: Vec<(usize, Box<dyn Fn(bool)>)>,
	next_callback_id: usize,
}

impl ProgramStatusProbe {
	/// Subscribe; the callback fires immediately with the current verdict.
	pub fn subscribe(&mut self, callback: Box<dyn Fn(bool)>) -> usize {
		let id = self.next_callback_id;
		self.next_callback_id += 1;
		self.callbacks.push((id, callback));
		if let Some(result) = self.result {
			if let Some((_, registered)) = self.callbacks.last() {
				registered(result);
			}
		}
		id
	}

	/// Drop a subscription.
	pub fn unsubscribe(&mut self, id: usize) {
		self.callbacks.retain(|(callback_id, _)| *callback_id != id);
	}

	/// The last published verdict, if any.
	pub fn result(&self) -> Option<bool> {
		self.result
	}

	/// Whether a probe is awaiting its reply.
	pub fn pending(&self) -> bool {
		self.pending
	}

	/// Start a probe. Returns the query to write, or nothing when a
	/// multiplexer owns the grid and the reply cannot be routed back.
	pub fn begin(&mut self, env: &impl EnvReader) -> Vec<ProgramStatusAction> {
		self.pending = false;
		self.reply_buffer.clear();
		self.result = None;
		if is_inside_terminal_multiplexer(env) {
			return Vec::new();
		}
		self.pending = true;
		vec![ProgramStatusAction::Write(PROGRAM_STATUS_QUERY.to_string())]
	}

	/// Feed one complete stdin sequence. Returns the verdicts published.
	pub fn feed(&mut self, sequence: &str) -> Vec<bool> {
		if is_da1_response(sequence) {
			return self.resolve(false);
		}
		if self.reply_buffer.is_empty() && !sequence.starts_with("\x1b]7501;") {
			return Vec::new();
		}
		if !self.reply_buffer.is_empty() && sequence.starts_with('\x1b') && sequence != "\x1b\\" {
			self.reply_buffer.clear();
			return Vec::new();
		}
		self.reply_buffer.push_str(sequence);
		if self.reply_buffer.len() > 4096 {
			self.reply_buffer.clear();
			return Vec::new();
		}
		if !self.reply_buffer.ends_with('\x07') && !self.reply_buffer.ends_with("\x1b\\") {
			return Vec::new();
		}

		let reply = std::mem::take(&mut self.reply_buffer);
		match parse_program_status_reply(&reply) {
			Some(supported) => self.resolve(supported),
			None => Vec::new(),
		}
	}

	/// Whether `feed` would consume `sequence` rather than forward it.
	pub fn consumes_sequence(&self, sequence: &str) -> bool {
		if !self.reply_buffer.is_empty() {
			return true;
		}
		is_program_status_sequence(sequence)
	}

	/// Release support until the next probe answers.
	pub fn stop(&mut self) -> Vec<bool> {
		self.pending = false;
		self.reply_buffer.clear();
		if self.result == Some(true) {
			self.publish(false)
		} else {
			Vec::new()
		}
	}

	fn resolve(&mut self, supported: bool) -> Vec<bool> {
		if !self.pending {
			return Vec::new();
		}
		self.pending = false;
		self.reply_buffer.clear();
		self.publish(supported)
	}

	fn publish(&mut self, supported: bool) -> Vec<bool> {
		if self.result == Some(supported) {
			return Vec::new();
		}
		self.result = Some(supported);
		for (_, callback) in &self.callbacks {
			callback(supported);
		}
		vec![supported]
	}
}
