//! Multi-step wizard controller with per-step validation.
//!
//! Ports `l/src/components/wizard-step.ts` (slot layout, gap, height budget,
//! optional-preview collapse) and adds the step-controller surface the Rust
//! crate needs: `set_step`/`set_steps`, next/previous navigation, validation,
//! and submit/cancel callbacks.
//!
//! Divergences from the TypeScript original:
//! - The TypeScript `WizardStep` renders one step's component slots; this port
//!   keeps the slot layout (title, intro, preview, content, status, footer)
//!   but stores slot text directly instead of composing `Component` children,
//!   so the controller is self-contained and deterministic.
//! - `fitContent` is replaced by slicing the content rows to the budget.
//! - Async step kinds carry no promise; `validate` is synchronous.

use crate::component::Component;
use crate::keys::Key;
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{pad_to_width, truncate_to_width, Ellipsis};

/// Semantic role of a wizard step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WizardStepKind {
    /// Free-form text entry.
    #[default]
    Input,
    /// Single or multiple choice.
    Choice,
    /// Confirmation prompt.
    Confirm,
    /// Asynchronous work.
    Async,
    /// Caller-defined content.
    Custom,
}

impl WizardStepKind {
    /// Lower-case name as in the TypeScript union.
    pub fn as_str(self) -> &'static str {
        match self {
            WizardStepKind::Input => "input",
            WizardStepKind::Choice => "choice",
            WizardStepKind::Confirm => "confirm",
            WizardStepKind::Async => "async",
            WizardStepKind::Custom => "custom",
        }
    }
}

/// One wizard page.
pub struct WizardStepPage {
    /// Step title rendered as the heading.
    pub title: String,
    /// Optional introduction under the title.
    pub intro: Option<String>,
    /// Optional preview slot; dropped when it does not fit the height budget.
    pub preview: Option<String>,
    /// Whether the preview may be dropped to preserve content height.
    pub preview_optional: bool,
    /// Body text; newlines split into rows.
    pub content: String,
    /// Optional status line under the content.
    pub status: Option<String>,
    /// Optional footer line.
    pub footer: Option<String>,
    /// Semantic role.
    pub kind: WizardStepKind,
    /// Validation returning the error message when the step is invalid.
    pub validate: Option<Box<dyn Fn() -> Option<String>>>,
}

impl WizardStepPage {
    /// Build a page with a title and body.
    pub fn new(title: impl Into<String>, content: impl Into<String>) -> Self {
        WizardStepPage {
            title: title.into(),
            intro: None,
            preview: None,
            preview_optional: false,
            content: content.into(),
            status: None,
            footer: None,
            kind: WizardStepKind::default(),
            validate: None,
        }
    }

    /// Attach an introduction.
    pub fn with_intro(mut self, intro: impl Into<String>) -> Self {
        self.intro = Some(intro.into());
        self
    }

    /// Attach an optional preview slot.
    pub fn with_preview(mut self, preview: impl Into<String>) -> Self {
        self.preview = Some(preview.into());
        self.preview_optional = true;
        self
    }

    /// Attach a status line.
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    /// Attach a footer line.
    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// Set the semantic role.
    pub fn with_kind(mut self, kind: WizardStepKind) -> Self {
        self.kind = kind;
        self
    }

    /// Attach a synchronous validator.
    pub fn with_validate(mut self, validate: impl Fn() -> Option<String> + 'static) -> Self {
        self.validate = Some(Box::new(validate));
        self
    }
}

/// Theme for [`WizardStep`].
#[derive(Clone)]
pub struct WizardStepTheme {
    /// Step title.
    pub title: Style,
    /// Introduction text.
    pub intro: Style,
    /// Body text.
    pub body: Style,
    /// Preview slot.
    pub preview: Style,
    /// Status line.
    pub status: Style,
    /// Validation error.
    pub error: Style,
    /// Footer text.
    pub footer: Style,
    /// Navigation hint.
    pub hint: Style,
    /// Glyph set; retained for hosts that draw their own chrome.
    pub symbols: SymbolTheme,
}

impl Default for WizardStepTheme {
    fn default() -> Self {
        WizardStepTheme {
            title: Style::fg(Color::White).bold(),
            intro: Style::fg(Color::DarkGray),
            body: Style::default(),
            preview: Style::fg(Color::DarkGray),
            status: Style::fg(Color::Cyan),
            error: Style::fg(Color::LightRed),
            footer: Style::default(),
            hint: Style::fg(Color::DarkGray).dim(),
            symbols: SymbolTheme::unicode(),
        }
    }
}

/// A wizard over one or more pages.
pub struct WizardStep {
    steps: Vec<WizardStepPage>,
    index: usize,
    gap: usize,
    max_height: Option<usize>,
    min_content_lines: usize,
    error: Option<String>,
    theme: WizardStepTheme,
    on_step_change: Option<Box<dyn FnMut(usize)>>,
    on_next: Option<Box<dyn FnMut(usize)>>,
    on_previous: Option<Box<dyn FnMut(usize)>>,
    on_cancel: Option<Box<dyn FnMut()>>,
    on_submit: Option<Box<dyn FnMut(usize)>>,
}

impl Default for WizardStep {
    fn default() -> Self {
        WizardStep::new(Vec::new(), WizardStepTheme::default())
    }
}

impl WizardStep {
    /// Build a wizard over `steps`.
    pub fn new(steps: Vec<WizardStepPage>, theme: WizardStepTheme) -> Self {
        WizardStep {
            steps,
            index: 0,
            gap: 1,
            max_height: None,
            min_content_lines: 1,
            error: None,
            theme,
            on_step_change: None,
            on_next: None,
            on_previous: None,
            on_cancel: None,
            on_submit: None,
        }
    }

    /// Replace every page and reset to the first one.
    pub fn set_steps(&mut self, steps: Vec<WizardStepPage>) {
        self.steps = steps;
        self.index = 0;
        self.error = None;
    }

    /// Move to `index` (clamped), firing `on_step_change`.
    pub fn set_step(&mut self, index: usize) {
        if self.steps.is_empty() {
            self.index = 0;
            self.error = None;
            return;
        }
        let target = index.min(self.steps.len() - 1);
        if target == self.index {
            return;
        }
        self.index = target;
        self.error = None;
        if let Some(callback) = self.on_step_change.as_mut() {
            callback(self.index);
        }
    }

    /// Current page index.
    pub fn step(&self) -> usize {
        self.index
    }

    /// Number of pages.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Current page.
    pub fn current_step(&self) -> Option<&WizardStepPage> {
        self.steps.get(self.index)
    }

    /// True when the first page is showing.
    pub fn is_first_step(&self) -> bool {
        self.index == 0
    }

    /// True when the last page is showing.
    pub fn is_last_step(&self) -> bool {
        self.index + 1 >= self.steps.len()
    }

    /// Blank rows between non-empty slots.
    pub fn gap(&self) -> usize {
        self.gap
    }

    /// Set the gap between slots.
    pub fn set_gap(&mut self, gap: usize) {
        self.gap = gap;
    }

    /// Bound the whole step; `None` removes the bound.
    pub fn set_max_height(&mut self, max_height: Option<usize>) {
        self.max_height = max_height;
    }

    /// Height bound.
    pub fn max_height(&self) -> Option<usize> {
        self.max_height
    }

    /// Minimum useful content rows used by the preview-collapse decision.
    pub fn min_content_lines(&self) -> usize {
        self.min_content_lines
    }

    /// Set the minimum content height.
    pub fn set_min_content_lines(&mut self, lines: usize) {
        self.min_content_lines = lines;
    }

    /// Current validation error, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Current theme.
    pub fn theme(&self) -> &WizardStepTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: WizardStepTheme) {
        self.theme = theme;
    }

    /// Fire [`Self::on_step_change`] when the page index changes.
    pub fn on_step_change(&mut self, callback: impl FnMut(usize) + 'static) -> &mut Self {
        self.on_step_change = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_next`] after advancing.
    pub fn on_next(&mut self, callback: impl FnMut(usize) + 'static) -> &mut Self {
        self.on_next = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_previous`] after going back.
    pub fn on_previous(&mut self, callback: impl FnMut(usize) + 'static) -> &mut Self {
        self.on_previous = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_cancel`] on cancel.
    pub fn on_cancel(&mut self, callback: impl FnMut() + 'static) -> &mut Self {
        self.on_cancel = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_submit`] with the final page index after a valid submit.
    pub fn on_submit(&mut self, callback: impl FnMut(usize) + 'static) -> &mut Self {
        self.on_submit = Some(Box::new(callback));
        self
    }

    /// Validate the current page, storing and returning the outcome.
    pub fn validate(&mut self) -> bool {
        let message = match self.steps.get(self.index) {
            Some(page) => match &page.validate {
                Some(validate) => validate(),
                None => None,
            },
            None => None,
        };
        self.error = message;
        self.error.is_none()
    }

    /// Validate the current page, then advance.
    ///
    /// On the last page this submits instead of advancing.
    pub fn next(&mut self) -> bool {
        if self.steps.is_empty() {
            return false;
        }
        if !self.validate() {
            return false;
        }
        if self.is_last_step() {
            let index = self.index;
            if let Some(callback) = self.on_submit.as_mut() {
                callback(index);
            }
            return true;
        }
        let target = self.index + 1;
        self.index = target;
        self.error = None;
        if let Some(callback) = self.on_next.as_mut() {
            callback(target);
        }
        true
    }

    /// Go back one page when not on the first.
    pub fn previous(&mut self) -> bool {
        if self.is_first_step() {
            return false;
        }
        let target = self.index - 1;
        self.index = target;
        self.error = None;
        if let Some(callback) = self.on_previous.as_mut() {
            callback(target);
        }
        true
    }

    /// Validate the current page and fire `on_submit` when it passes.
    pub fn submit(&mut self) -> bool {
        if !self.validate() {
            return false;
        }
        let index = self.index;
        if let Some(callback) = self.on_submit.as_mut() {
            callback(index);
        }
        true
    }

    fn cancel(&mut self) {
        if let Some(callback) = self.on_cancel.as_mut() {
            callback();
        }
    }

    fn render_page(&self, width: usize) -> Vec<String> {
        let Some(page) = self.steps.get(self.index) else {
            return Vec::new();
        };
        let mut title = self.theme.title.paint(&page.title);
        if let Some(error) = &self.error {
            title = format!("{}{}", title, self.theme.error.paint(&format!("  ! {}", error)));
        }
        let mut slots: Vec<Vec<String>> = Vec::new();
        slots.push(vec![title]);
        if let Some(intro) = &page.intro {
            slots.push(vec![self.theme.intro.paint(intro)]);
        }
        let mut preview_index: Option<usize> = None;
        if let Some(preview) = &page.preview {
            preview_index = Some(slots.len());
            slots.push(vec![self.theme.preview.paint(preview)]);
        }
        let content_lines: Vec<String> = if page.content.is_empty() {
            Vec::new()
        } else {
            page.content
                .split('\n')
                .map(|line| self.theme.body.paint(line))
                .collect()
        };
        if !content_lines.is_empty() {
            slots.push(content_lines);
        }
        if let Some(status) = &page.status {
            slots.push(vec![self.theme.status.paint(status)]);
        }
        if let Some(footer) = &page.footer {
            slots.push(vec![self.theme.footer.paint(footer)]);
        }
        slots.push(vec![self.theme.hint.paint(&format!(
            "Step {} of {} · Enter {} · Esc cancel",
            self.index + 1,
            self.steps.len(),
            if self.is_last_step() { "submit" } else { "next" }
        ))]);

        // Optional preview collapse: drop it when the page cannot fit the bound.
        if page.preview_optional && self.max_height.is_some() {
            if let Some(index) = preview_index {
                let preview_rows = slots[index].len();
                let mut without_preview = slots.clone();
                without_preview.remove(index);
                let rows_without = joined_rows(&without_preview, self.gap);
                let bound = self.max_height.unwrap_or(usize::MAX);
                if rows_without + self.min_content_lines + self.gap + preview_rows > bound {
                    slots.remove(index);
                }
            }
        }

        let mut lines: Vec<String> = Vec::new();
        for (position, slot) in slots.iter().enumerate() {
            if position > 0 && self.gap > 0 {
                for _ in 0..self.gap {
                    lines.push(String::new());
                }
            }
            lines.extend(slot.iter().cloned());
        }
        if let Some(bound) = self.max_height {
            lines.truncate(bound);
        }
        lines
            .into_iter()
            .map(|line| pad_to_width(&fit_row(&line, width), width))
            .collect()
    }
}

fn joined_rows(slots: &[Vec<String>], gap: usize) -> usize {
    if slots.is_empty() {
        return 0;
    }
    slots.iter().map(|slot| slot.len()).sum::<usize>() + gap * (slots.len() - 1)
}

fn fit_row(row: &str, width: usize) -> String {
    if width == 0 {
        String::new()
    } else {
        truncate_to_width(row, width, Ellipsis::Omit)
    }
}

impl Component for WizardStep {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return vec![String::new()];
        }
        self.render_page(width as usize)
    }

    fn handle_input(&mut self, key: &Key) {
        if crate::keys::is_escape(key) {
            self.cancel();
            return;
        }
        let forward = crate::keys::is_enter(key)
            || crate::keys::is_tab(key)
            || crate::keys::is_arrow_right(key)
            || crate::keys::is_arrow_down(key)
            || crate::keys::is_ctrl_right(key)
            || matches!(key, Key::Ctrl('n'));
        if forward {
            self.next();
            return;
        }
        let backward = crate::keys::is_shift_tab(key)
            || crate::keys::is_arrow_left(key)
            || crate::keys::is_arrow_up(key)
            || crate::keys::is_ctrl_left(key)
            || matches!(key, Key::Ctrl('p'));
        if backward {
            self.previous();
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_theme() -> WizardStepTheme {
        WizardStepTheme {
            title: Style::default(),
            intro: Style::default(),
            body: Style::default(),
            preview: Style::default(),
            status: Style::default(),
            error: Style::default(),
            footer: Style::default(),
            hint: Style::default(),
            symbols: SymbolTheme::unicode(),
        }
    }

    fn steps() -> Vec<WizardStepPage> {
        vec![
            WizardStepPage::new("Account", "Enter your name")
                .with_intro("First, who are you?")
                .with_kind(WizardStepKind::Input),
            WizardStepPage::new("Plan", "Pick a plan").with_kind(WizardStepKind::Choice),
            WizardStepPage::new("Confirm", "Review and submit")
                .with_status("Ready")
                .with_kind(WizardStepKind::Confirm),
        ]
    }

    fn trimmed(lines: Vec<String>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect()
    }

    #[test]
    fn renders_exact_rows_at_fixed_width() {
        let mut wizard = WizardStep::new(steps(), plain_theme());
        let lines = trimmed(wizard.render(40));
        assert_eq!(&lines[0], "Account");
        assert_eq!(lines[1], "");
        assert_eq!(&lines[2], "First, who are you?");
        assert_eq!(lines[3], "");
        assert_eq!(&lines[4], "Enter your name");
        assert_eq!(lines[5], "");
        assert_eq!(&lines[6], "Step 1 of 3 · Enter next · Esc cancel");
    }

    #[test]
    fn next_and_previous_traverse_steps() {
        let nexts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let prevs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let next_sink = std::sync::Arc::clone(&nexts);
        let prev_sink = std::sync::Arc::clone(&prevs);
        let mut wizard = WizardStep::new(steps(), plain_theme());
        wizard.on_next(move |index| next_sink.lock().unwrap().push(index));
        wizard.on_previous(move |index| prev_sink.lock().unwrap().push(index));
        assert!(wizard.is_first_step());
        assert!(!wizard.is_last_step());
        assert!(wizard.next());
        assert_eq!(wizard.step(), 1);
        assert!(wizard.next());
        assert!(wizard.is_last_step());
        assert_eq!(
            nexts.lock().unwrap().to_vec(),
            [1usize, 2usize]
        );
        assert!(wizard.previous());
        assert_eq!(wizard.step(), 1);
        assert_eq!(prevs.lock().unwrap().to_vec(), [1usize]);
    }

    #[test]
    fn next_on_last_step_submits() {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&submitted);
        let mut wizard = WizardStep::new(steps(), plain_theme());
        wizard.on_submit(move |index| sink.lock().unwrap().push(index));
        wizard.set_step(2);
        assert!(wizard.next());
        assert_eq!(submitted.lock().unwrap().to_vec(), [2usize]);
    }

    #[test]
    fn invalid_step_blocks_next_and_shows_error() {
        let mut wizard = WizardStep::new(
            vec![
                WizardStepPage::new("Name", "Your name")
                    .with_validate(|| Some("Name is required.".to_string())),
                WizardStepPage::new("Done", "All set"),
            ],
            plain_theme(),
        );
        assert!(!wizard.next());
        assert_eq!(wizard.step(), 0);
        assert_eq!(wizard.error(), Some("Name is required."));
        let lines = trimmed(wizard.render(40));
        assert!(lines[0].contains("Name is required."));
    }

    #[test]
    fn submit_validates_current_step() {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&submitted);
        let mut wizard = WizardStep::new(steps(), plain_theme());
        wizard.on_submit(move |index| sink.lock().unwrap().push(index));
        assert!(wizard.submit());
        assert_eq!(submitted.lock().unwrap().to_vec(), [0usize]);
    }

    #[test]
    fn escape_cancels() {
        let cancels = std::sync::Arc::new(std::sync::Mutex::new(0));
        let sink = std::sync::Arc::clone(&cancels);
        let mut wizard = WizardStep::new(steps(), plain_theme());
        wizard.on_cancel(move || *sink.lock().unwrap() += 1);
        wizard.handle_input(&Key::Esc);
        assert_eq!(*cancels.lock().unwrap(), 1);
    }

    #[test]
    fn keyboard_forward_and_backward() {
        let mut wizard = WizardStep::new(steps(), plain_theme());
        wizard.handle_input(&Key::Right);
        assert_eq!(wizard.step(), 1);
        wizard.handle_input(&Key::Tab);
        assert_eq!(wizard.step(), 2);
        wizard.handle_input(&Key::BackTab);
        assert_eq!(wizard.step(), 1);
        wizard.handle_input(&Key::CtrlLeft);
        assert_eq!(wizard.step(), 0);
    }

    #[test]
    fn set_steps_resets_to_first() {
        let mut wizard = WizardStep::new(steps(), plain_theme());
        wizard.set_step(2);
        assert_eq!(wizard.step(), 2);
        wizard.set_steps(vec![WizardStepPage::new("Only", "One page")]);
        assert_eq!(wizard.step(), 0);
        assert_eq!(wizard.step_count(), 1);
        assert!(wizard.is_first_step());
        assert!(wizard.is_last_step());
    }

    #[test]
    fn set_step_clamps_and_fires_change() {
        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let mut wizard = WizardStep::new(steps(), plain_theme());
        wizard.on_step_change(move |index| sink.lock().unwrap().push(index));
        wizard.set_step(99);
        assert_eq!(wizard.step(), 2);
        assert_eq!(changes.lock().unwrap().to_vec(), [2usize]);
    }

    #[test]
    fn optional_preview_collapses_under_height_bound() {
        let mut wizard = WizardStep::new(
            vec![WizardStepPage::new("Preview", "body line")
                .with_preview("a very long preview")
                .with_footer("footer")],
            plain_theme(),
        );
        wizard.set_gap(0);
        wizard.set_max_height(Some(4));
        let lines = trimmed(wizard.render(40));
        assert!(!lines.iter().any(|line| line.contains("preview")));
        assert!(lines.iter().any(|line| line.contains("body line")));
        assert!(lines.iter().any(|line| line.contains("footer")));
    }

    #[test]
    fn empty_and_zero_width() {
        let mut wizard = WizardStep::new(Vec::new(), plain_theme());
        assert!(wizard.render(30).is_empty());
        assert_eq!(wizard.render(0), vec![""]);
        assert!(wizard.is_first_step());
        assert!(wizard.is_last_step());
        assert!(!wizard.next());
    }

    #[test]
    fn narrow_widths_never_exceed() {
        let mut wizard = WizardStep::new(steps(), plain_theme());
        for width in [1u16, 2, 5, 12] {
            for line in wizard.render(width) {
                assert!(crate::text_shape::visible_width(&line) <= width as usize);
            }
        }
    }
}
