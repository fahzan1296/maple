//! Multi-field form with text, select, and toggle controls.
//! Reference: `l/src/components/form.ts`.
//!
//! Ports `l/src/components/form.ts`. A [`Form`] owns focus traversal across
//! [`FormField`]s; each field owns its value, validation error, and control
//! rendering. Enter validates and submits the whole form; Tab/Shift+Tab move
//! focus; Esc cancels.
//!
//! Divergences from the TypeScript original:
//! - The TypeScript module composes already-built `Input`/`SelectList`
//!   components; this port renders the controls inline so a form is
//!   self-contained and deterministic.
//! - Text fields have no caret movement; input appends at the end.
//! - `SelectFormField`'s picker sheet, async preview, and promise-returning
//!   callbacks are out of scope; submit and change callbacks are synchronous.
//! - Toggle fields are an addition (the TypeScript form ships text and select
//!   fields only).

use crate::component::Component;
use crate::keys::Key;
use crate::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{pad_to_width, truncate_to_width, Ellipsis};

/// Truncate `row` to `width` columns, yielding an empty string at zero width.
fn fit_row(row: &str, width: usize) -> String {
    if width == 0 {
        String::new()
    } else {
        truncate_to_width(row, width, Ellipsis::Omit)
    }
}

/// Policy for a single-line field submitted while empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmptyInput {
    /// Submit the empty value.
    #[default]
    Submit,
    /// Treat the empty submit as a cancel.
    Cancel,
    /// Refuse the empty submit and show an error.
    Reject,
}

/// One choice of a select field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    /// Stable value reported to the form.
    pub value: String,
    /// Display title.
    pub title: String,
    /// Optional secondary line.
    pub description: Option<String>,
    /// Disabled choices stay visible but cannot be picked.
    pub disabled: bool,
}

impl SelectOption {
    /// Build an enabled option.
    pub fn new(value: impl Into<String>, title: impl Into<String>) -> Self {
        SelectOption {
            value: value.into(),
            title: title.into(),
            description: None,
            disabled: false,
        }
    }

    /// Attach a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Mark the option disabled.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// Control held by a [`FormField`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormFieldKind {
    /// Single-line text input.
    Text {
        /// Current value.
        value: String,
        /// Mask the value when rendered.
        secret: bool,
        /// Prompt drawn before the value.
        prompt: Option<String>,
    },
    /// Single-choice list.
    Select {
        /// Choices.
        items: Vec<SelectOption>,
        /// Currently picked value.
        selected: Option<String>,
        /// Filter text.
        query: String,
        /// Visible rows.
        max_visible: usize,
        /// Whether typing filters the choices.
        searchable: bool,
    },
    /// Boolean toggle.
    Toggle {
        /// Current state.
        value: bool,
    },
}

impl FormFieldKind {
    /// Text control with an empty value.
    pub fn text(value: impl Into<String>) -> Self {
        FormFieldKind::Text {
            value: value.into(),
            secret: false,
            prompt: None,
        }
    }

    /// Secret text control.
    pub fn secret(value: impl Into<String>) -> Self {
        FormFieldKind::Text {
            value: value.into(),
            secret: true,
            prompt: None,
        }
    }

    /// Select control over `items`.
    pub fn select(items: Vec<SelectOption>, selected: Option<String>) -> Self {
        FormFieldKind::Select {
            items,
            selected,
            query: String::new(),
            max_visible: 5,
            searchable: true,
        }
    }

    /// Toggle control.
    pub fn toggle(value: bool) -> Self {
        FormFieldKind::Toggle { value }
    }

    fn is_text(&self) -> bool {
        matches!(self, FormFieldKind::Text { .. })
    }
}

/// One labeled control plus its validation state.
pub struct FormField {
    /// Stable identifier reported to change callbacks.
    pub id: String,
    /// Label row above the control.
    pub label: String,
    /// Description row between the label and the control.
    pub description: Option<String>,
    /// Hint row below the control.
    pub hint: Option<String>,
    /// Control.
    pub kind: FormFieldKind,
    /// Empty-submit policy for text fields.
    pub empty: EmptyInput,
    /// Error shown when an empty reject submission fails.
    pub empty_error: Option<String>,
    /// Whether the value must be non-empty.
    pub required: bool,
    /// Validation returning the error message for a value.
    pub validate: Option<Box<dyn Fn(&str) -> Option<String>>>,
    /// Current validation error.
    pub error: Option<String>,
}

impl FormField {
    /// Build a field with `id` and a control.
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: FormFieldKind) -> Self {
        FormField {
            id: id.into(),
            label: label.into(),
            description: None,
            hint: None,
            kind,
            empty: EmptyInput::default(),
            empty_error: None,
            required: false,
            validate: None,
            error: None,
        }
    }

    /// Attach a description row.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Attach a hint row.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Set the empty-submit policy.
    pub fn with_empty(mut self, empty: EmptyInput) -> Self {
        self.empty = empty;
        self
    }

    /// Require a non-empty value.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Attach a synchronous validator.
    pub fn with_validate(mut self, validate: impl Fn(&str) -> Option<String> + 'static) -> Self {
        self.validate = Some(Box::new(validate));
        self
    }

    /// Current value as text.
    pub fn value(&self) -> String {
        match &self.kind {
            FormFieldKind::Text { value, .. } => value.clone(),
            FormFieldKind::Select { selected, .. } => selected.clone().unwrap_or_default(),
            FormFieldKind::Toggle { value } => value.to_string(),
        }
    }

    /// Replace the value, clearing the error.
    pub fn set_value(&mut self, value: &str) {
        match &mut self.kind {
            FormFieldKind::Text { value: current, .. } => *current = value.to_string(),
            FormFieldKind::Select { selected, .. } => {
                *selected = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                };
            }
            FormFieldKind::Toggle { value: current } => *current = value == "true",
        }
        self.error = None;
    }

    /// Show a validation error.
    pub fn set_error(&mut self, message: Option<String>) {
        self.error = message;
    }

    /// Validation message for `value`, if any.
    pub fn validate_value(&self, value: &str) -> Option<String> {
        let empty = value.trim().is_empty();
        if empty && self.kind.is_text() && self.empty == EmptyInput::Reject {
            return Some(
                self.empty_error
                    .clone()
                    .unwrap_or_else(|| "A value is required.".to_string()),
            );
        }
        if self.required && empty {
            return Some(
                self.empty_error
                    .clone()
                    .unwrap_or_else(|| "A value is required.".to_string()),
            );
        }
        match &self.validate {
            Some(validate) => validate(value),
            None => None,
        }
    }

    /// Handle a key; returns the new value when the control changed it.
    pub fn handle_key(&mut self, key: &Key) -> Option<String> {
        match &mut self.kind {
            FormFieldKind::Text { value, .. } => {
                if let Some(character) = key.as_char() {
                    value.push(character);
                    return Some(value.clone());
                }
                if crate::keys::is_backspace(key) {
                    value.pop();
                    return Some(value.clone());
                }
                None
            }
            FormFieldKind::Toggle { value } => {
                if matches!(key, Key::Char(' ')) {
                    *value = !*value;
                    return Some(value.to_string());
                }
                None
            }
            FormFieldKind::Select {
                items,
                selected,
                query,
                max_visible,
                searchable,
            } => {
                let mut visible = visible_options(items, query);
                if visible.is_empty() {
                    return None;
                }
                if crate::keys::is_arrow_up(key) || matches!(key, Key::Ctrl('p')) {
                    let index = selected_index(&visible, selected);
                    let next = if index == 0 {
                        visible.len() - 1
                    } else {
                        index - 1
                    };
                    *selected = Some(visible[next].value.clone());
                    return selected.clone();
                }
                if crate::keys::is_arrow_down(key) || matches!(key, Key::Ctrl('n')) {
                    let index = selected_index(&visible, selected);
                    let next = (index + 1) % visible.len();
                    *selected = Some(visible[next].value.clone());
                    return selected.clone();
                }
                if crate::keys::is_enter(key) {
                    return selected.clone();
                }
                if crate::keys::is_backspace(key) {
                    query.pop();
                    visible = visible_options(items, query);
                    if !visible.is_empty() {
                        let still = visible
                            .iter()
                            .any(|option| selected.as_deref() == Some(option.value.as_str()));
                        if !still {
                            *selected = Some(visible[0].value.clone());
                        }
                    }
                    return None;
                }
                if *searchable {
                    if let Some(character) = key.as_char() {
                        query.push(character);
                        visible = visible_options(items, query);
                        if !visible.is_empty() {
                            let still = visible
                                .iter()
                                .any(|option| selected.as_deref() == Some(option.value.as_str()));
                            if !still {
                                *selected = Some(visible[0].value.clone());
                                return selected.clone();
                            }
                        }
                    }
                }
                let _ = max_visible;
                None
            }
        }
    }
}

fn visible_options(items: &[SelectOption], query: &str) -> Vec<SelectOption> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|option| {
            option.title.to_lowercase().contains(&query)
                || option
                    .description
                    .as_deref()
                    .is_some_and(|description| description.to_lowercase().contains(&query))
        })
        .cloned()
        .collect()
}

fn selected_index(visible: &[SelectOption], selected: &Option<String>) -> usize {
    visible
        .iter()
        .position(|option| selected.as_deref() == Some(option.value.as_str()))
        .unwrap_or(0)
}

/// Theme for [`Form`].
#[derive(Clone)]
pub struct FormTheme {
    /// Field label.
    pub label: Style,
    /// Field description.
    pub description: Style,
    /// Validation error text.
    pub error: Style,
    /// Hint text.
    pub hint: Style,
    /// Selection cursor and text caret.
    pub cursor: Style,
    /// Selected select row.
    pub selected: Style,
    /// Unselected select row.
    pub option: Style,
    /// Toggle marker.
    pub toggle: Style,
    /// Glyph set; retained for hosts that draw their own chrome.
    pub symbols: SymbolTheme,
}

impl Default for FormTheme {
    fn default() -> Self {
        FormTheme {
            label: Style::fg(Color::White).bold(),
            description: Style::fg(Color::DarkGray),
            error: Style::fg(Color::LightRed),
            hint: Style::fg(Color::DarkGray).dim(),
            cursor: Style::fg(Color::White).bold(),
            selected: Style::fg(Color::White).bold(),
            option: Style::default(),
            toggle: Style::fg(Color::LightGreen),
            symbols: SymbolTheme::unicode(),
        }
    }
}

/// A form over one or more fields.
pub struct Form {
    fields: Vec<FormField>,
    theme: FormTheme,
    active_index: usize,
    bounds: Vec<(usize, usize)>,
    on_change: Option<Box<dyn FnMut(&str, &str)>>,
    on_submit: Option<Box<dyn FnMut(&[(String, String)])>>,
    on_cancel: Option<Box<dyn FnMut()>>,
}

impl Default for Form {
    fn default() -> Self {
        Form::new(Vec::new(), FormTheme::default())
    }
}

impl Form {
    /// Build a form over `fields`.
    pub fn new(fields: Vec<FormField>, theme: FormTheme) -> Self {
        Form {
            fields,
            theme,
            active_index: 0,
            bounds: Vec::new(),
            on_change: None,
            on_submit: None,
            on_cancel: None,
        }
    }

    /// Replace the fields and reset focus to the first one.
    pub fn set_fields(&mut self, fields: Vec<FormField>) {
        self.fields = fields;
        self.active_index = 0;
    }

    /// The fields in render order.
    pub fn fields(&self) -> &[FormField] {
        &self.fields
    }

    /// Mutable access to a field by id.
    pub fn field_mut(&mut self, id: &str) -> Option<&mut FormField> {
        self.fields.iter_mut().find(|field| field.id == id)
    }

    /// Read a field by id.
    pub fn field(&self, id: &str) -> Option<&FormField> {
        self.fields.iter().find(|field| field.id == id)
    }

    /// Set a field's value without firing `on_change`.
    pub fn set_field_value(&mut self, id: &str, value: &str) -> bool {
        match self.field_mut(id) {
            Some(field) => {
                field.set_value(value);
                true
            }
            None => false,
        }
    }

    /// Current `(id, value)` pairs.
    pub fn values(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .map(|field| (field.id.clone(), field.value()))
            .collect()
    }

    /// Index of the focused field.
    pub fn focused_field(&self) -> usize {
        self.active_index
    }

    /// Focus the field at `index` (clamped).
    pub fn set_focused_field(&mut self, index: usize) {
        if self.fields.is_empty() {
            self.active_index = 0;
        } else {
            self.active_index = index.min(self.fields.len() - 1);
        }
    }

    /// Focus a field by id.
    pub fn focus_field(&mut self, id: &str) -> bool {
        match self.fields.iter().position(|field| field.id == id) {
            Some(index) => {
                self.active_index = index;
                true
            }
            None => false,
        }
    }

    /// Move focus one field forward (wrapping).
    pub fn focus_next(&mut self) {
        if !self.fields.is_empty() {
            self.active_index = (self.active_index + 1) % self.fields.len();
        }
    }

    /// Move focus one field back (wrapping).
    pub fn focus_previous(&mut self) {
        if !self.fields.is_empty() {
            self.active_index = (self.active_index + self.fields.len() - 1) % self.fields.len();
        }
    }

    /// Current theme.
    pub fn theme(&self) -> &FormTheme {
        &self.theme
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: FormTheme) {
        self.theme = theme;
    }

    /// Fire [`Self::on_change`] when a field value changes.
    pub fn on_change(&mut self, callback: impl FnMut(&str, &str) + 'static) -> &mut Self {
        self.on_change = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_submit`] with every `(id, value)` after a valid submit.
    pub fn on_submit(&mut self, callback: impl FnMut(&[(String, String)]) + 'static) -> &mut Self {
        self.on_submit = Some(Box::new(callback));
        self
    }

    /// Fire [`Self::on_cancel`] on cancel.
    pub fn on_cancel(&mut self, callback: impl FnMut() + 'static) -> &mut Self {
        self.on_cancel = Some(Box::new(callback));
        self
    }

    /// Validate every field, storing and returning whether all passed.
    pub fn validate(&mut self) -> bool {
        let mut valid = true;
        for field in self.fields.iter_mut() {
            let value = field.value();
            match field.validate_value(&value) {
                Some(error) => {
                    field.error = Some(error);
                    valid = false;
                }
                None => field.error = None,
            }
        }
        valid
    }

    /// Validate and, when every field passes, fire `on_submit`.
    ///
    /// An empty text field configured with [`EmptyInput::Cancel`] cancels the
    /// form instead of submitting.
    pub fn submit(&mut self) -> bool {
        for field in &self.fields {
            if field.kind.is_text()
                && field.value().trim().is_empty()
                && field.empty == EmptyInput::Cancel
            {
                if let Some(callback) = self.on_cancel.as_mut() {
                    callback();
                }
                return false;
            }
        }
        if !self.validate() {
            return false;
        }
        let values: Vec<(String, String)> = self
            .fields
            .iter()
            .map(|field| (field.id.clone(), field.value()))
            .collect();
        if let Some(callback) = self.on_submit.as_mut() {
            callback(&values);
        }
        true
    }

    fn render_field(&self, field: &FormField, width: usize, focused: bool) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        if !field.label.is_empty() {
            lines.push(pad_to_width(&fit_row(&self.theme.label.paint(&field.label), width), width));
        }
        if let Some(description) = &field.description {
            lines.push(pad_to_width(
                &fit_row(&self.theme.description.paint(description), width),
                width,
            ));
        }
        lines.extend(self.render_control(field, width, focused));
        if let Some(error) = &field.error {
            lines.push(pad_to_width(&fit_row(&self.theme.error.paint(error), width), width));
        }
        if let Some(hint) = &field.hint {
            lines.push(pad_to_width(&fit_row(&self.theme.hint.paint(hint), width), width));
        }
        lines
    }

    fn render_control(&self, field: &FormField, width: usize, focused: bool) -> Vec<String> {
        match &field.kind {
            FormFieldKind::Text {
                value,
                secret,
                prompt,
            } => {
                let prompt = prompt.clone().unwrap_or_default();
                let display = if *secret {
                    "\u{2022}".repeat(value.chars().count())
                } else {
                    value.clone()
                };
                let caret = if focused { "\u{2588}" } else { "" };
                let text = format!("{}{}{}", prompt, display, caret);
                vec![pad_to_width(&fit_row(&text, width), width)]
            }
            FormFieldKind::Toggle { value } => {
                let marker = if *value { "[x]" } else { "[ ]" };
                let text = if focused {
                    format!("{} {}", self.theme.cursor.paint(marker), field.label)
                } else {
                    format!("{} {}", self.theme.toggle.paint(marker), field.label)
                };
                vec![pad_to_width(&fit_row(&text, width), width)]
            }
            FormFieldKind::Select {
                items,
                selected,
                query,
                max_visible,
                searchable,
            } => {
                let visible = visible_options(items, query);
                let viewport = (*max_visible).max(1).min(visible.len().max(1));
                let selected_index = selected_index(&visible, selected);
                let start = selected_index
                    .saturating_sub(viewport / 2)
                    .min(visible.len().saturating_sub(viewport));
                let mut lines = Vec::new();
                for row in 0..viewport.min(visible.len()) {
                    let option = &visible[start + row];
                    let is_selected = start + row == selected_index;
                    let prefix = if is_selected { "> " } else { "  " };
                    let title = if option.disabled {
                        self.theme.hint.paint(&option.title)
                    } else if is_selected {
                        self.theme.selected.paint(&option.title)
                    } else {
                        self.theme.option.paint(&option.title)
                    };
                    let mut text = format!("{}{}", prefix, title);
                    if is_selected {
                        if let Some(description) = &option.description {
                            text.push(' ');
                            text.push_str(&self.theme.description.paint(description));
                        }
                    }
                    lines.push(pad_to_width(&fit_row(&text, width), width));
                }
                if *searchable {
                    let status = if query.trim().is_empty() {
                        "  Type to search".to_string()
                    } else {
                        format!("  Search: {}", query)
                    };
                    lines.push(pad_to_width(
                        &fit_row(&self.theme.hint.paint(&status), width),
                        width,
                    ));
                }
                lines
            }
        }
    }
}

impl Component for Form {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 {
            return vec![String::new()];
        }
        let width = width as usize;
        let mut lines: Vec<String> = Vec::new();
        let mut bounds: Vec<(usize, usize)> = Vec::new();
        for (index, field) in self.fields.iter().enumerate() {
            if index > 0 {
                lines.push(pad_to_width("", width));
            }
            let start = lines.len();
            lines.extend(self.render_field(field, width, index == self.active_index));
            bounds.push((start, lines.len()));
        }
        self.bounds = bounds;
        lines
    }

    fn handle_input(&mut self, key: &Key) {
        if crate::keys::is_escape(key) {
            if let Some(callback) = self.on_cancel.as_mut() {
                callback();
            }
            return;
        }
        if self.fields.len() > 1 && (crate::keys::is_tab(key) || crate::keys::is_shift_tab(key)) {
            if crate::keys::is_shift_tab(key) {
                self.focus_previous();
            } else {
                self.focus_next();
            }
            return;
        }
        if crate::keys::is_enter(key) {
            let is_select = matches!(
                self.fields.get(self.active_index).map(|field| &field.kind),
                Some(FormFieldKind::Select { .. })
            );
            if is_select {
                self.forward(key);
            } else {
                self.submit();
            }
            return;
        }
        self.forward(key);
    }

    fn handle_paste(&mut self, text: &str) {
        if let Some(field) = self.fields.get_mut(self.active_index) {
            if let FormFieldKind::Text { value, .. } = &mut field.kind {
                value.push_str(text);
            }
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if !matches!(event.kind, MouseKind::Down(MouseButtonKind::Left)) {
            return;
        }
        let row = event.row as usize;
        let index = self
            .bounds
            .iter()
            .position(|(start, end)| row >= *start && row < *end);
        if let Some(index) = index {
            self.active_index = index;
        }
    }
}

impl Form {
    fn forward(&mut self, key: &Key) {
        let Some(field) = self.fields.get_mut(self.active_index) else {
            return;
        };
        let Some(value) = field.handle_key(key) else {
            return;
        };
        let id = field.id.clone();
        if let Some(callback) = self.on_change.as_mut() {
            callback(&id, &value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_theme() -> FormTheme {
        FormTheme {
            label: Style::default(),
            description: Style::default(),
            error: Style::default(),
            hint: Style::default(),
            cursor: Style::default(),
            selected: Style::default(),
            option: Style::default(),
            toggle: Style::default(),
            symbols: SymbolTheme::unicode(),
        }
    }

    fn fields() -> Vec<FormField> {
        vec![
            FormField::new("name", "Name", FormFieldKind::text("ada")).required(),
            FormField::new(
                "mode",
                "Mode",
                FormFieldKind::select(
                    vec![
                        SelectOption::new("fast", "Fast"),
                        SelectOption::new("slow", "Slow"),
                    ],
                    Some("fast".to_string()),
                ),
            ),
            FormField::new("verbose", "Verbose", FormFieldKind::toggle(false)),
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
        let mut form = Form::new(fields(), plain_theme());
        let lines = trimmed(form.render(40));
        assert_eq!(&lines[0], "Name");
        assert_eq!(&lines[1], "ada\u{2588}");
        assert_eq!(lines[2], "");
        assert_eq!(&lines[3], "Mode");
        assert_eq!(&lines[4], "> Fast");
        assert_eq!(&lines[5], "  Slow");
        assert_eq!(&lines[6], "  Type to search");
        assert_eq!(lines[7], "");
        assert_eq!(&lines[8], "Verbose");
        assert_eq!(&lines[9], "[ ] Verbose");
    }

    #[test]
    fn tab_moves_focus_and_enter_submits() {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&submitted);
        let mut form = Form::new(fields(), plain_theme());
        form.on_submit(move |values| {
            sink.lock().unwrap().push(values.to_vec());
        });
        assert_eq!(form.focused_field(), 0);
        form.handle_input(&Key::Tab);
        assert_eq!(form.focused_field(), 1);
        form.handle_input(&Key::BackTab);
        assert_eq!(form.focused_field(), 0);
        form.handle_input(&Key::Enter);
        let captured = submitted.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0],
            vec![
                ("name".to_string(), "ada".to_string()),
                ("mode".to_string(), "fast".to_string()),
                ("verbose".to_string(), "false".to_string()),
            ]
        );
    }

    #[test]
    fn typing_edits_focused_text_field() {
        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let mut form = Form::new(fields(), plain_theme());
        form.on_change(move |id, value| sink.lock().unwrap().push((id.to_string(), value.to_string())));
        form.handle_input(&Key::Char('!'));
        assert_eq!(form.field("name").unwrap().value(), "ada!");
        assert_eq!(
            changes.lock().unwrap().to_vec(),
            [("name".to_string(), "ada!".to_string())]
        );
        form.handle_input(&Key::Backspace);
        assert_eq!(form.field("name").unwrap().value(), "ada");
    }

    #[test]
    fn required_field_blocks_submit_and_shows_error() {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(0));
        let sink = std::sync::Arc::clone(&submitted);
        let mut form = Form::new(fields(), plain_theme());
        form.on_submit(move |_| *sink.lock().unwrap() += 1);
        form.set_field_value("name", "");
        assert!(!form.submit());
        assert_eq!(*submitted.lock().unwrap(), 0);
        assert_eq!(
            form.field("name").unwrap().error.as_deref(),
            Some("A value is required.")
        );
        form.set_field_value("name", "grace");
        assert!(form.submit());
        assert_eq!(*submitted.lock().unwrap(), 1);
        assert!(form.field("name").unwrap().error.is_none());
    }

    #[test]
    fn custom_validator_reports_message() {
        let mut form = Form::new(
            vec![FormField::new("port", "Port", FormFieldKind::text("abc"))
                .with_validate(|value| {
                    if value.parse::<u16>().is_err() {
                        Some("Port must be a number.".to_string())
                    } else {
                        None
                    }
                })],
            plain_theme(),
        );
        assert!(!form.submit());
        assert_eq!(
            form.field("port").unwrap().error.as_deref(),
            Some("Port must be a number.")
        );
    }

    #[test]
    fn select_arrow_and_enter() {
        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let mut form = Form::new(fields(), plain_theme());
        form.focus_field("mode");
        form.on_change(move |id, value| sink.lock().unwrap().push((id.to_string(), value.to_string())));
        form.handle_input(&Key::Down);
        assert_eq!(form.field("mode").unwrap().value(), "slow");
        form.handle_input(&Key::Enter);
        assert_eq!(
            changes.lock().unwrap().to_vec(),
            [
                ("mode".to_string(), "slow".to_string()),
                ("mode".to_string(), "slow".to_string()),
            ]
        );
    }

    #[test]
    fn select_typing_filters() {
        let mut form = Form::new(fields(), plain_theme());
        form.focus_field("mode");
        form.handle_input(&Key::Char('l'));
        let lines = trimmed(form.render(30));
        assert_eq!(&lines[4], "> Slow");
        assert_eq!(&lines[5], "  Search: l");
    }

    #[test]
    fn toggle_space_flips() {
        let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&changes);
        let mut form = Form::new(fields(), plain_theme());
        form.focus_field("verbose");
        form.on_change(move |id, value| sink.lock().unwrap().push((id.to_string(), value.to_string())));
        form.handle_input(&Key::Char(' '));
        assert_eq!(form.field("verbose").unwrap().value(), "true");
        assert_eq!(
            changes.lock().unwrap().to_vec(),
            [("verbose".to_string(), "true".to_string())]
        );
    }

    #[test]
    fn escape_cancels() {
        let cancels = std::sync::Arc::new(std::sync::Mutex::new(0));
        let sink = std::sync::Arc::clone(&cancels);
        let mut form = Form::new(fields(), plain_theme());
        form.on_cancel(move || *sink.lock().unwrap() += 1);
        form.handle_input(&Key::Esc);
        assert_eq!(*cancels.lock().unwrap(), 1);
    }

    #[test]
    fn set_focused_field_clamps() {
        let mut form = Form::new(fields(), plain_theme());
        form.set_focused_field(99);
        assert_eq!(form.focused_field(), 2);
        form.set_focused_field(1);
        assert_eq!(form.focused_field(), 1);
    }

    #[test]
    fn empty_form_and_zero_width() {
        let mut form = Form::new(Vec::new(), plain_theme());
        assert!(form.render(30).is_empty());
        assert_eq!(form.render(0), vec![""]);
        assert!(form.submit());
    }

    #[test]
    fn narrow_widths_never_exceed() {
        let mut form = Form::new(fields(), plain_theme());
        for width in [1u16, 2, 3, 8, 20] {
            for line in form.render(width) {
                assert!(crate::text_shape::visible_width(&line) <= width as usize);
            }
        }
    }

    #[test]
    fn mouse_click_focuses_field() {
        let mut form = Form::new(fields(), plain_theme());
        form.render(40);
        form.handle_mouse(&MouseEvent {
            kind: MouseKind::Down(MouseButtonKind::Left),
            column: 0,
            row: 8,
            shift: false,
            ctrl: false,
            alt: false,
        });
        assert_eq!(form.focused_field(), 2);
    }

    #[test]
    fn secret_masks_value() {
        let mut form = Form::new(
            vec![FormField::new("token", "Token", FormFieldKind::secret("abc"))],
            plain_theme(),
        );
        let lines = trimmed(form.render(30));
        assert_eq!(&lines[1], "\u{2022}\u{2022}\u{2022}\u{2588}");
    }
}
