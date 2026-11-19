//! Incomplete-todo reminder committed into the transcript.
//!
//! Reference: `l/src/chat/todo-reminder.ts`.
//!
//! The reference renders a `MessageNoticeComponent` and, on terminals that list
//! the `checklist` kind, a native checklist instead. This port keeps both paths:
//! [`TodoReminder::render`] draws the notice card through
//! [`crate::chrome::MessageNotice`], and [`NativeDescribe::describe_native`]
//! returns the checklist when the terminal supports it.
//!
//! `TodoItem`/`TodoStatus`/`TodoPhase` and [`todo_checklist_phases`] come from
//! `l/src/tools/todo.ts`, which has no crate module; they live here because the
//! reminder is their only transcript consumer.
//!
//! Deviations: the reference's `presentation`/`nativePresentation` closures and
//! its process-global `theme` become explicit `&Theme` reads; the notice's
//! hidden state is a flag on this component.

use crate::chrome::{MessageNotice, NoticeTheme};
use crate::component::Component;
use crate::native::describe::{node, span, text, with_hidden, NativeDescribe};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, Props, Text};
use crate::render::render_utils::replace_tabs;
use crate::style::{Color, Style};
use crate::theme::Theme;
use crate::tools::shell_viz::sanitize_text;

/// Lifecycle state of a todo item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodoStatus {
    /// Not started.
    Pending,
    /// Being worked on.
    InProgress,
    /// Finished.
    Completed,
    /// Dropped without finishing.
    Abandoned,
}

impl TodoStatus {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
            TodoStatus::Abandoned => "abandoned",
        }
    }

    /// Parse a wire status.
    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "pending" => Some(TodoStatus::Pending),
            "in_progress" => Some(TodoStatus::InProgress),
            "completed" => Some(TodoStatus::Completed),
            "abandoned" => Some(TodoStatus::Abandoned),
            _ => None,
        }
    }

    /// The checklist item status a native terminal draws (`§7.5`).
    pub fn checklist_status(self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "active",
            TodoStatus::Completed => "done",
            TodoStatus::Abandoned => "dropped",
        }
    }
}

/// A task displayed within a todo phase.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TodoItem {
    /// Task text.
    pub content: String,
    /// Lifecycle state.
    pub status: TodoStatus,
}

impl Default for TodoStatus {
    fn default() -> Self {
        TodoStatus::Pending
    }
}

/// A named group of todo tasks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TodoPhase {
    /// Phase name; empty for an unnamed phase.
    pub name: String,
    /// The phase's tasks.
    pub tasks: Vec<TodoItem>,
}

/// One checklist item, as the `checklist` node kind carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecklistItem {
    /// Stable item id (`p0.1`).
    pub id: String,
    /// Display text.
    pub text: String,
    /// `pending`, `active`, `done`, `dropped` or `blocked`.
    pub status: String,
}

/// One checklist phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecklistPhase {
    /// Stable phase id (`p0`).
    pub id: String,
    /// Display title.
    pub title: String,
    /// The phase's items.
    pub items: Vec<ChecklistItem>,
    /// A fully closed phase folds to one line.
    pub collapsed: bool,
}

/// Whether a todo counts as closed.
pub fn is_closed_todo(todo: &TodoItem) -> bool {
    matches!(todo.status, TodoStatus::Completed | TodoStatus::Abandoned)
}

/// Task text as drawn: ANSI/C0 stripped, tabs expanded.
pub fn for_display(text: &str) -> String {
    replace_tabs(&sanitize_text(text))
}

/// Checklist phases for native terminals: plain phase titles, completed phases
/// folded.
pub fn todo_checklist_phases(
    phases: &[TodoPhase],
    is_matched: &dyn Fn(&TodoItem) -> bool,
) -> Vec<ChecklistPhase> {
    phases
        .iter()
        .enumerate()
        .map(|(phase_index, phase)| ChecklistPhase {
            id: format!("p{phase_index}"),
            title: for_display(&phase.name),
            items: phase
                .tasks
                .iter()
                .enumerate()
                .map(|(task_index, task)| {
                    let matched = is_matched(task);
                    let status = if task.status == TodoStatus::Pending && matched {
                        "active"
                    } else {
                        task.status.checklist_status()
                    };
                    ChecklistItem {
                        id: format!("p{phase_index}.{task_index}"),
                        text: for_display(&task.content),
                        status: status.to_string(),
                    }
                })
                .collect(),
            collapsed: !phase.tasks.is_empty() && phase.tasks.iter().all(is_closed_todo),
        })
        .collect()
}

/// A todo completion reminder notification.
pub struct TodoReminder {
    notice: MessageNotice,
    todos: Vec<TodoItem>,
    note: String,
    header: String,
    icon: String,
    hidden: bool,
}

impl TodoReminder {
    /// A reminder for `todos`, `attempt` of `max_attempts`.
    pub fn new(todos: Vec<TodoItem>, attempt: u32, max_attempts: u32, theme: &Theme) -> Self {
        let count = todos.len();
        let plural = if count == 1 { "todo" } else { "todos" };
        let header = format!(
            "{count} incomplete {plural} - reminder {attempt}/{max_attempts}"
        );
        let icon = theme.icon().warning.to_string();
        let mut notice = MessageNotice::new(header.clone());
        notice.set_icon(Some(icon.clone()));
        notice.set_theme(NoticeTheme {
            header: Style::fg(Color::Yellow),
            body: Style::default(),
            background: None,
        });
        let body: Vec<String> = todos.iter().map(|_| String::new()).collect();
        notice.set_body(&body);
        TodoReminder {
            notice,
            todos,
            note: format!("reminder {attempt}/{max_attempts}"),
            header,
            icon,
            hidden: false,
        }
    }

    /// The header the reference builds from the count and attempt.
    pub fn header(&self) -> &str {
        &self.header
    }

    /// The reminder counter note (`reminder 1/3`).
    pub fn note(&self) -> &str {
        &self.note
    }

    /// The todos this reminder lists.
    pub fn todos(&self) -> &[TodoItem] {
        &self.todos
    }

    /// Whether tool activity is hidden, which hides this reminder too.
    pub fn set_tool_activity_visible(&mut self, visible: bool, theme: &Theme) {
        self.hidden = !visible;
        self.notice.set_leading_space(visible);
        self.notice.set_body(&self.body_rows(theme));
    }

    /// Whether the reminder is hidden with tool activity.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// The italic `☐ task` lines the notice draws.
    fn body_rows(&self, theme: &Theme) -> Vec<String> {
        let unchecked = theme.checkbox().unchecked;
        self.todos
            .iter()
            .map(|todo| theme.italic(&format!("  {unchecked} {}", todo.content)))
            .collect()
    }
}

impl Component for TodoReminder {
    fn render(&mut self, width: u16) -> Vec<String> {
        if width == 0 || self.hidden {
            return Vec::new();
        }
        self.notice.render(width)
    }

    fn invalidate(&mut self) {
        self.notice.invalidate();
    }
}

impl NativeDescribe for TodoReminder {
    fn describe_native(&self, cx: &DescribeContext) -> NativeNode {
        if !cx.supports(NodeKind::Checklist) {
            let mut head = Props::new();
            head.set_str("gap", "sm");
            head.set_str("role", "omp.notice.todo");
            return node(
                NodeKind::Row,
                head,
                vec![text(
                    Text::plain(format!("{} {}", self.icon, self.header)),
                    Props::new(),
                )],
                Some("todo"),
            );
        }
        let phases = todo_checklist_phases(
            &[TodoPhase {
                name: String::new(),
                tasks: self.todos.clone(),
            }],
            &|_| false,
        );
        with_hidden(
            checklist_node("reminder", &phases, Some(&self.note)),
            self.hidden,
        )
    }
}

/// A `checklist` node carrying `mode`, `phases` and an optional note.
pub(crate) fn checklist_node(
    mode: &str,
    phases: &[ChecklistPhase],
    note: Option<&str>,
) -> NativeNode {
    let mut props = Props::new();
    props.set_str("mode", mode);
    if let Some(note) = note {
        props.set_str("note", note);
    }
    props.insert(
        "phases".to_string(),
        serde_json::Value::Array(
            phases
                .iter()
                .map(|phase| {
                    let mut value = serde_json::Map::new();
                    value.insert("id".into(), serde_json::Value::String(phase.id.clone()));
                    value.insert("title".into(), serde_json::Value::String(phase.title.clone()));
                    value.insert("collapsed".into(), serde_json::Value::Bool(phase.collapsed));
                    value.insert(
                        "items".into(),
                        serde_json::Value::Array(
                            phase
                                .items
                                .iter()
                                .map(|item| {
                                    let mut value = serde_json::Map::new();
                                    value.insert(
                                        "id".into(),
                                        serde_json::Value::String(item.id.clone()),
                                    );
                                    value.insert(
                                        "text".into(),
                                        serde_json::Value::String(item.text.clone()),
                                    );
                                    value.insert(
                                        "status".into(),
                                        serde_json::Value::String(item.status.clone()),
                                    );
                                    serde_json::Value::Object(value)
                                })
                                .collect(),
                        ),
                    );
                    serde_json::Value::Object(value)
                })
                .collect(),
        ),
    );
    NativeNode::with_props(NodeKind::Checklist, props)
}

/// The list-node form of a reminder's todos (the reference's notice body).
pub(crate) fn todo_list_node(todos: &[TodoItem]) -> NativeNode {
    let items = todos
        .iter()
        .enumerate()
        .map(|(index, todo)| {
            let mut props = Props::new();
            props.insert(
                "label".to_string(),
                serde_json::Value::Array(vec![span(&todo.content, Some("em")).to_value()]),
            );
            props.set_str("tone", "pending");
            node(NodeKind::Item, props, Vec::new(), Some(&format!("t{index}")))
        })
        .collect();
    node(NodeKind::List, Props::new(), items, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        crate::theme::create_theme(
            crate::theme::builtin::builtin_theme_json("dark").expect("builtin theme"),
            crate::theme::CreateThemeOptions {
                mode: Some(crate::theme::color::ColorMode::TrueColor),
                ..Default::default()
            },
        )
        .expect("theme resolves")
    }

    fn todo(content: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            content: content.to_string(),
            status,
        }
    }

    #[test]
    fn statuses_map_to_checklist_states() {
        let cases = [
            (TodoStatus::Pending, "pending"),
            (TodoStatus::InProgress, "active"),
            (TodoStatus::Completed, "done"),
            (TodoStatus::Abandoned, "dropped"),
        ];
        for (status, expected) in cases {
            assert_eq!(status.checklist_status(), expected);
            assert_eq!(TodoStatus::from_str(status.as_str()), Some(status));
        }
        assert_eq!(TodoStatus::from_str("blocked"), None);
        assert_eq!(TodoStatus::from_str("in_progress"), Some(TodoStatus::InProgress));
    }

    #[test]
    fn closed_todos_are_completed_or_abandoned() {
        assert!(!is_closed_todo(&todo("a", TodoStatus::Pending)));
        assert!(!is_closed_todo(&todo("a", TodoStatus::InProgress)));
        assert!(is_closed_todo(&todo("a", TodoStatus::Completed)));
        assert!(is_closed_todo(&todo("a", TodoStatus::Abandoned)));
    }

    #[test]
    fn checklist_phases_number_items_and_fold_closed_phases() {
        let phases = vec![
            TodoPhase {
                name: "Foundation".into(),
                tasks: vec![
                    todo("parse", TodoStatus::InProgress),
                    todo("test", TodoStatus::Pending),
                ],
            },
            TodoPhase {
                name: String::new(),
                tasks: vec![todo("done", TodoStatus::Completed)],
            },
        ];
        let checklist = todo_checklist_phases(&phases, &|task| task.content == "test");
        assert_eq!(checklist.len(), 2);
        assert_eq!(checklist[0].id, "p0");
        assert_eq!(checklist[0].title, "Foundation");
        assert!(!checklist[0].collapsed);
        let items: Vec<(&str, &str)> = checklist[0]
            .items
            .iter()
            .map(|item| (item.id.as_str(), item.status.as_str()))
            .collect();
        assert_eq!(items, [("p0.0", "active"), ("p0.1", "active")]);
        assert_eq!(checklist[1].title, "");
        assert!(checklist[1].collapsed);
    }

    #[test]
    fn display_text_expands_tabs() {
        assert_eq!(for_display("a\tb"), "a   b");
    }

    #[test]
    fn reminder_header_counts_singular_and_plural_todos() {
        let theme = theme();
        let single = TodoReminder::new(vec![todo("a", TodoStatus::Pending)], 1, 3, &theme);
        assert_eq!(single.header(), "1 incomplete todo - reminder 1/3");
        assert_eq!(single.note(), "reminder 1/3");
        let two = TodoReminder::new(
            vec![todo("a", TodoStatus::Pending), todo("b", TodoStatus::Pending)],
            2,
            3,
            &theme,
        );
        assert_eq!(two.header(), "2 incomplete todos - reminder 2/3");
    }

    #[test]
    fn a_hidden_reminder_renders_no_rows_and_hides_its_node() {
        let theme = theme();
        let mut reminder = TodoReminder::new(vec![todo("a", TodoStatus::Pending)], 1, 3, &theme);
        assert!(!reminder.render(40).is_empty());
        reminder.set_tool_activity_visible(false, &theme);
        assert!(reminder.is_hidden());
        assert!(reminder.render(40).is_empty());
    }

    #[test]
    fn the_checklist_path_is_used_when_the_terminal_lists_the_kind() {
        let theme = theme();
        let reminder = TodoReminder::new(vec![todo("a", TodoStatus::Pending)], 1, 3, &theme);
        let cx = DescribeContext::new(80);
        let described = reminder.describe_native(&cx);
        assert_eq!(described.kind, NodeKind::Checklist);
        assert_eq!(described.props.get_str("mode"), Some("reminder"));
        assert_eq!(described.props.get_str("note"), Some("reminder 1/3"));
        let no_checklist = DescribeContext::with_capabilities(80, &[NodeKind::Row], &[]);
        let fallback = reminder.describe_native(&no_checklist);
        assert_eq!(fallback.kind, NodeKind::Row);
    }

    #[test]
    fn list_node_carries_emphasis_spans_per_todo() {
        let list = todo_list_node(&[
            todo("one", TodoStatus::Pending),
            todo("two", TodoStatus::Pending),
        ]);
        assert_eq!(list.kind, NodeKind::List);
        assert_eq!(list.children.len(), 2);
        assert_eq!(list.children[0].key.as_deref(), Some("t0"));
        assert_eq!(list.children[1].plain_text(), "");
    }

}
