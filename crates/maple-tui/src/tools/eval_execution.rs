//! Eval tool execution renderer and the bounded display formatter.
//!
//! Ported from `l/src/tools/eval.ts` and `l/src/tools/eval-format/index.ts`.
//! The TS formatters re-tokenize JavaScript/Python to reflow code; that tokenizer
//! is not ported in this slice, so [`format_eval_code_for_display`] keeps the
//! `%`-magic passthrough exact and otherwise applies a bounded, display-only
//! normalization (dedent, trailing-space trim, blank-line collapse, line cap).
//! The executed source is never modified.

use crate::component::Component;
use crate::render::tool_card::{StatusLine, ToolCard, ToolCardPhase};
use crate::text_shape::visible_width;
use crate::theme::Theme;
use crate::tools::tool_execution::{ToolExecution, ToolExecutionState};

/// Maximum source lines kept in a collapsed eval card.
pub const EVAL_DISPLAY_MAX_LINES: usize = 24;

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

fn count_indent(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ' || *ch == '\t').count()
}

/// Format an arbitrary eval-code prefix for display without changing the
/// executed source. `%`-prefixed magic passes through untouched.
pub fn format_eval_code_for_display(source: &str, language: EvalLanguage) -> String {
    let _ = language;
    if source.trim_start().starts_with('%') {
        return source.to_string();
    }
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| count_indent(line))
        .min()
        .unwrap_or(0);
    let mut out: Vec<String> = Vec::new();
    let mut blank_run = 0usize;
    for line in &lines {
        let dedented = if line.trim().is_empty() {
            ""
        } else if line.len() >= indent {
            &line[indent..]
        } else {
            line.trim_start()
        };
        if dedented.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 || out.is_empty() {
                continue;
            }
            out.push(String::new());
            continue;
        }
        blank_run = 0;
        out.push(dedented.trim_end().to_string());
    }
    while out.last().is_some_and(|line| line.is_empty()) {
        out.pop();
    }
    if out.len() > EVAL_DISPLAY_MAX_LINES {
        let hidden = out.len() - EVAL_DISPLAY_MAX_LINES;
        out.truncate(EVAL_DISPLAY_MAX_LINES);
        out.push(format!("… ({hidden} more lines)"));
    }
    out.join("\n")
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
    fn dedents_and_collapses_blank_runs() {
        let source = "\n    a\n\n\n\n    b\n";
        assert_eq!(
            format_eval_code_for_display(source, EvalLanguage::Python),
            "a\n\nb"
        );
    }

    #[test]
    fn trims_trailing_whitespace() {
        assert_eq!(
            format_eval_code_for_display("a   \nb\t\n", EvalLanguage::JavaScript),
            "a\nb"
        );
    }

    #[test]
    fn caps_the_displayed_line_count() {
        let source = (0..40).map(|index| format!("line {index}")).collect::<Vec<_>>().join("\n");
        let formatted = format_eval_code_for_display(&source, EvalLanguage::JavaScript);
        let lines: Vec<&str> = formatted.split('\n').collect();
        assert_eq!(lines.len(), EVAL_DISPLAY_MAX_LINES + 1);
        assert!(lines.last().unwrap().contains("more lines"));
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
