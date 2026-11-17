//! Eval tool execution renderer.
//!
//! Ported from `l/src/tools/eval.ts`. Display formatting lives in
//! [`crate::tools::eval_format`].

use crate::component::Component;
use crate::render::tool_card::{StatusLine, ToolCard, ToolCardPhase};
use crate::text_shape::visible_width;
use crate::theme::Theme;
use crate::tools::tool_execution::{ToolExecution, ToolExecutionState};

/// Re-exported display formatter.
pub use crate::tools::eval_format::format_eval_code_for_display;


/// Languages the eval formatter distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalLanguage {
    /// JavaScript / TypeScript.
    JavaScript,
    /// Python.
    Python,
}

impl EvalLanguage {
    /// Lower-case language label.
    pub fn label(self) -> &'static str {
        match self {
            EvalLanguage::JavaScript => "js",
            EvalLanguage::Python => "python",
        }
    }
}


/// An eval execution card: language-labelled code plus its output.
pub struct EvalExecution {
    language: EvalLanguage,
    source: String,
    output: String,
    execution: ToolExecution,
    theme: Theme,
}

impl EvalExecution {
    /// An eval card for `source` in `language`.
    pub fn new(source: impl Into<String>, language: EvalLanguage, theme: Theme) -> Self {
        EvalExecution {
            language,
            source: source.into(),
            output: String::new(),
            execution: ToolExecution::new("eval"),
            theme,
        }
    }

    /// Replace the source.
    pub fn set_source(&mut self, source: impl Into<String>) {
        self.source = source.into();
    }

    /// The source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Replace the language.
    pub fn set_language(&mut self, language: EvalLanguage) {
        self.language = language;
    }

    /// The language.
    pub fn language(&self) -> EvalLanguage {
        self.language
    }

    /// Replace the output.
    pub fn set_output(&mut self, output: impl Into<String>) {
        self.output = output.into();
    }

    /// The execution record.
    pub fn execution(&self) -> &ToolExecution {
        &self.execution
    }

    /// Replace the execution record.
    pub fn set_execution(&mut self, execution: ToolExecution) {
        self.execution = execution;
    }

    fn phase(&self) -> ToolCardPhase {
        match self.execution.state() {
            ToolExecutionState::Pending => ToolCardPhase::Pending,
            ToolExecutionState::Running => ToolCardPhase::Running,
            ToolExecutionState::Success => ToolCardPhase::Success,
            ToolExecutionState::Warning => ToolCardPhase::Warning,
            ToolExecutionState::Error => ToolCardPhase::Error,
            ToolExecutionState::Aborted => ToolCardPhase::Pending,
        }
    }

    fn card(&mut self) -> ToolCard {
        let formatted = format_eval_code_for_display(&self.source, self.language);
        let status = StatusLine::new(format!("eval · {}", self.language.label()));
        let mut card = ToolCard::new(self.theme.clone(), status);
        card.set_status(self.phase());
        card.set_body(formatted.split('\n').map(str::to_string).collect());
        if !self.output.is_empty() {
            card.set_output(&self.output);
        }
        card
    }
}

impl Component for EvalExecution {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return Vec::new();
        }
        self.card().render_at(width)
    }
}

/// One column of a table view.
#[derive(Clone, Debug, PartialEq)]
pub struct EvalTableColumn {
	/// Header text.
	pub head: String,
	/// Numeric columns render right-aligned.
	pub numeric: bool,
}

/// A pretty rendering of something a cell printed, shown in place of its text.
#[derive(Clone, Debug, PartialEq)]
pub enum EvalView {
	/// A data frame as a table.
	Table {
		columns: Vec<EvalTableColumn>,
		rows: Vec<Vec<String>>,
	},
	/// A structured value rendered as a tree.
	Json(serde_json::Value),
	/// An object's Markdown representation.
	Markdown(String),
}

/// A view and the exact printed text it stands for: renderers locate `text` in
/// the cell's output (entries in print order) and show the view there. An entry
/// whose text is no longer in the output (truncated) is skipped.
#[derive(Clone, Debug, PartialEq)]
pub struct EvalViewEntry {
	/// Exact printed text the view replaces.
	pub text: String,
	/// The view shown in its place.
	pub view: EvalView,
}

/// One ordered piece of a cell's output: printed text, or a view shown in place
/// of the text it matched.
#[derive(Clone, Debug, PartialEq)]
pub enum EvalOutputPart {
	/// Printed text.
	Text(String),
	/// A view located at this position.
	View(EvalView),
}

/// Split `output` around its views, in order: each view's `text` is searched
/// from the end of the previous match, unmatched entries are skipped, and blank
/// text between views is dropped (newlines bordering a view trimmed).
pub fn split_eval_output(output: &str, views: Option<&[EvalViewEntry]>) -> Vec<EvalOutputPart> {
	let mut parts = Vec::new();
	let mut cursor = 0usize;
	for entry in views.unwrap_or(&[]) {
		if entry.text.is_empty() {
			continue;
		}
		let Some(offset) = output[cursor..].find(&entry.text) else {
			continue;
		};
		let at = cursor + offset;
		push_eval_text(&mut parts, &output[cursor..at], cursor > 0, true);
		parts.push(EvalOutputPart::View(entry.view.clone()));
		cursor = at + entry.text.len();
	}
	push_eval_text(&mut parts, &output[cursor..], cursor > 0, cursor > 0);
	parts
}

fn push_eval_text(parts: &mut Vec<EvalOutputPart>, chunk: &str, trim_start: bool, trim_end: bool) {
	let mut text = chunk;
	if trim_start {
		text = text.trim_start_matches('\n');
	}
	if trim_end {
		text = text.trim_end_matches('\n');
	}
	if !text.trim().is_empty() {
		parts.push(EvalOutputPart::Text(text.to_string()));
	}
}

/// Evictable status events a log keeps: a cell polling a helper in a loop emits
/// one event per call, and every live update and the persisted tool result
/// would otherwise carry them all.
pub const MAX_STATUS_EVENTS: usize = 200;

/// A status event emitted by eval prelude helpers for rendering.
#[derive(Clone, Debug, Default)]
pub struct EvalStatusEvent {
	/// Event op, e.g. `agent`, `todo`, `mcp_progress`.
	pub op: String,
	/// Validated thinking level supplied by the execution layer.
	pub resolved_thinking_level: Option<String>,
	/// Remaining payload fields.
	pub fields: serde_json::Map<String, serde_json::Value>,
}

impl PartialEq for EvalStatusEvent {
	fn eq(&self, other: &Self) -> bool {
		self.op == other.op
			&& self.resolved_thinking_level == other.resolved_thinking_level
			&& self.fields == other.fields
	}
}

impl EvalStatusEvent {
	/// Attach `key`/`value` to the event payload.
	pub fn with_field(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
		self.fields.insert(key.to_string(), value.into());
		self
	}
}

/// A status event list plus the count of evictable events dropped from its front.
#[derive(Clone, Debug, Default)]
pub struct StatusEventLog {
	/// Kept events, oldest first.
	pub status_events: Vec<EvalStatusEvent>,
	/// Evictable events dropped from the front.
	pub status_events_elided: usize,
}

/// Coalescing key of a progress-snapshot event: `agent`, `judge_batch`, and
/// `mcp_progress` events keyed by `id`, where only the newest snapshot matters.
/// Discrete actions (everything else) have no key.
pub fn status_event_key(event: &EvalStatusEvent) -> Option<String> {
	if !matches!(event.op.as_str(), "agent" | "judge_batch" | "mcp_progress") {
		return None;
	}
	let id = event
		.fields
		.get("id")
		.and_then(serde_json::Value::as_str)?;
	Some(format!("{}\u{0}{id}", event.op))
}

/// Append or replace a status event. Progress snapshots coalesce in place,
/// preserving first-seen order; every other op is a discrete action and simply
/// appends.
pub fn upsert_status_event(events: &mut Vec<EvalStatusEvent>, event: EvalStatusEvent) {
	if let Some(key) = status_event_key(&event) {
		if let Some(index) = events
			.iter()
			.position(|existing| status_event_key(existing).as_deref() == Some(key.as_str()))
		{
			events[index] = event;
			return;
		}
	}
	events.push(event);
}

/// [`upsert_status_event`] into a bounded log. Past [`MAX_STATUS_EVENTS`], the
/// oldest discrete event or MCP progress snapshot is dropped and counted. Agent
/// and judgment-batch snapshots stay, since agent cards read them, and so does
/// the newest committed `todo` result, which the session todo panel refreshes
/// from.
pub fn record_status_event(log: &mut StatusEventLog, event: EvalStatusEvent) {
	upsert_status_event(&mut log.status_events, event);
	if log.status_events.len() <= MAX_STATUS_EVENTS {
		return;
	}
	let keep = log
		.status_events
		.iter()
		.rposition(|existing| {
			existing.op == "todo"
				&& existing
					.fields
					.get("committed")
					.and_then(serde_json::Value::as_bool)
					.is_some_and(|committed| committed)
		});
	let oldest = log
		.status_events
		.iter()
		.enumerate()
		.position(|(index, existing)| {
			Some(index) != keep && (existing.op == "mcp_progress" || status_event_key(existing).is_none())
		});
	let Some(oldest) = oldest else {
		return;
	};
	log.status_events.remove(oldest);
	log.status_events_elided += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_lines_pass_through() {
        let source = "%matplotlib inline\nprint(1)";
        assert_eq!(
            format_eval_code_for_display(source, EvalLanguage::Python),
            source
        );
    }


    #[test]
    fn empty_source_is_safe() {
        assert_eq!(format_eval_code_for_display("", EvalLanguage::JavaScript), "");
    }

    #[test]
    fn card_renders_bounded_rows() {
        let theme = test_theme();
        let mut eval = EvalExecution::new("print('hi')", EvalLanguage::Python, theme);
        eval.set_output("hi\n");
        for width in [10u16, 20, 40] {
            for row in eval.render(width) {
                assert!(visible_width(&row) <= width as usize, "width {width}: {row:?}");
            }
        }
        assert!(eval.render(0).is_empty());
    }

    #[test]
    fn state_transitions_reach_the_card() {
        let theme = test_theme();
        let mut eval = EvalExecution::new("1 + 1", EvalLanguage::JavaScript, theme);
        assert_eq!(eval.phase(), ToolCardPhase::Pending);
        let mut execution = ToolExecution::new("eval");
        execution.start();
        eval.set_execution(execution);
        assert_eq!(eval.phase(), ToolCardPhase::Running);
        let mut execution = ToolExecution::new("eval");
        execution.start();
        execution.succeed();
        eval.set_execution(execution);
        assert_eq!(eval.phase(), ToolCardPhase::Success);
    }

    fn test_theme() -> Theme {
        crate::theme::create_theme(
            crate::theme::builtin::builtin_theme_json("dark").unwrap(),
            crate::theme::CreateThemeOptions::default(),
        )
        .unwrap()
    }
}
