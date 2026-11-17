//! Port of the data-first parts of `l/test/model-picker-native.test.ts`.
//!
//! Deviations:
//! - The reference drives a full TUI (`initTheme`, 40-row terminal, native
//!   describe tree) and inspects `TspPickerProps`; the crate injects
//!   [`ModelPickerRegistry`], [`Theme`] and [`KeybindingsManager`], so only
//!   filtering, selection, mode routing, over-context gating and native
//!   select/activate events are asserted.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::keybindings::KeybindingsManager;
use maple_tui::native::backend::NativeUiEvent;
use maple_tui::native::node::{DescribeContext, NativeNode};
use maple_tui::overlays::model_picker::{
    filter_items, ModelPickerCallbacks, ModelPickerComponent, ModelPickerMode,
    ModelPickerOptions, ModelPickerRegistry, ResolvedRoleModel, MIN_VISIBLE,
    MODEL_PICKER_ROLE, SELECT_ONLY_STATUS_HINT,
};
use maple_tui::setup::scenes::types::{ModelBrowserItem, ModelKind, SetupModel};
use maple_tui::theme::builtin;
use maple_tui::theme::{CreateThemeOptions, Theme, create_theme};

fn theme() -> Theme {
    create_theme(
        builtin::builtin_theme_json("dark").expect("builtin dark theme"),
        CreateThemeOptions::default(),
    )
    .expect("valid theme")
}

fn model(provider: &str, id: &str) -> SetupModel {
    SetupModel {
        provider: provider.to_string(),
        id: id.to_string(),
        name: id.to_string(),
        kind: ModelKind::Chat,
        priority: None,
    }
}

fn item(provider: &str, id: &str) -> ModelBrowserItem {
    let selector = format!("{provider}/{id}");
    ModelBrowserItem {
        provider: provider.to_string(),
        id: id.to_string(),
        model: model(provider, id),
        selector,
    }
}

struct FakeRegistry {
    items: Vec<ModelBrowserItem>,
}

impl ModelPickerRegistry for FakeRegistry {
    fn refresh_if_stale(&mut self) -> Result<bool, String> {
        Ok(false)
    }

    fn items(&self) -> Vec<ModelBrowserItem> {
        self.items.clone()
    }
}

fn catalog() -> FakeRegistry {
    FakeRegistry {
        items: vec![
            item("anthropic", "sonnet"),
            item("openai", "gpt-5"),
            item("openai", "mini"),
        ],
    }
}

type Picks = Rc<RefCell<Vec<String>>>;

fn picker(
    registry: &mut dyn ModelPickerRegistry,
    callbacks: ModelPickerCallbacks,
    options: ModelPickerOptions,
) -> ModelPickerComponent {
    ModelPickerComponent::new(
        theme(),
        KeybindingsManager::new(),
        registry,
        callbacks,
        options,
    )
}

fn pick_callbacks() -> (ModelPickerCallbacks, Picks) {
    let picks: Picks = Rc::new(RefCell::new(Vec::new()));
    let sink = picks.clone();
    (
        ModelPickerCallbacks {
            on_pick: Some(Box::new(move |_model, selector, over_context| {
                sink.borrow_mut()
                    .push(format!("pick:{selector}:{over_context}"));
            })),
            ..Default::default()
        },
        picks,
    )
}

#[test]
fn filter_matches_selector_substrings_ignoring_case() {
    let items = vec![item("anthropic", "sonnet"), item("openai", "gpt-5")];
    let matches: Vec<&str> = filter_items(&items, "SON")
        .iter()
        .map(|item| item.selector.as_str())
        .collect();
    assert_eq!(matches, vec!["anthropic/sonnet"]);
    assert_eq!(filter_items(&items, "  ").len(), 2);
    assert!(filter_items(&items, "nope").is_empty());
}

#[test]
fn constructor_preselects_the_current_selector_in_catalogue_order() {
    let (callbacks, _picks) = pick_callbacks();
    let mut registry = catalog();
    let component = picker(
        &mut registry,
        callbacks,
        ModelPickerOptions {
            current_selector: Some("openai/gpt-5".to_string()),
            ..Default::default()
        },
    );

    assert_eq!(component.mode(), ModelPickerMode::Session);
    assert_eq!(
        component.selected().map(|item| item.selector.as_str()),
        Some("openai/gpt-5")
    );
    let visible: Vec<&str> = component
        .visible_items()
        .iter()
        .map(|item| item.selector.as_str())
        .collect();
    assert_eq!(visible, vec!["anthropic/sonnet", "openai/gpt-5", "openai/mini"]);
}

#[test]
fn typing_filters_then_enter_picks_with_over_context_flag() {
    let (callbacks, picks) = pick_callbacks();
    let mut registry = catalog();
    let mut component = picker(
        &mut registry,
        callbacks,
        ModelPickerOptions {
            current_context_tokens: Some(1_000),
            ..Default::default()
        },
    );
    component.set_context_window_lookup(Box::new(|selector| match selector {
        "openai/mini" => Some(500),
        _ => Some(100_000),
    }));

    // The small-window model is over context; the others are not.
    assert!(!component.is_over_context(&item("anthropic", "sonnet")));
    assert!(component.is_over_context(&item("openai", "mini")));

    component.handle_input(&maple_tui::keys::Key::Char('m'));
    component.handle_input(&maple_tui::keys::Key::Char('i'));
    component.handle_input(&maple_tui::keys::Key::Char('n'));
    assert_eq!(component.query(), "min");
    assert_eq!(
        component.selected().map(|item| item.selector.as_str()),
        Some("openai/mini")
    );

    component.handle_input(&maple_tui::keys::Key::Enter);
    assert_eq!(&*picks.borrow(), &["pick:openai/mini:true".to_string()]);
}

#[test]
fn toggle_task_mode_routes_picks_to_the_task_callback() {
    let tasks: Picks = Rc::new(RefCell::new(Vec::new()));
    let task_sink = tasks.clone();
    let callbacks = ModelPickerCallbacks {
        on_pick_task: Some(Box::new(move |_model, selector| {
            task_sink.borrow_mut().push(selector.to_string());
        })),
        ..Default::default()
    };
    let mut registry = catalog();
    let mut component = picker(
        &mut registry,
        callbacks,
        ModelPickerOptions {
            task_mode_keys: vec!["Tab".to_string()],
            task_selector: Some("openai/gpt-5".to_string()),
            current_selector: Some("anthropic/sonnet".to_string()),
            ..Default::default()
        },
    );

    assert_eq!(component.mode(), ModelPickerMode::Session);
    component.toggle_task_mode();
    assert_eq!(component.mode(), ModelPickerMode::Task);
    assert_eq!(
        component.selected().map(|item| item.selector.as_str()),
        Some("openai/gpt-5")
    );
    assert!(component.footer_hint().contains("Task subagents"));

    component.confirm();
    assert_eq!(&*tasks.borrow(), &["openai/gpt-5".to_string()]);
}

#[test]
fn native_select_and_activate_drive_the_same_confirm_path() {
    let (callbacks, picks) = pick_callbacks();
    let mut registry = catalog();
    let mut component = picker(&mut registry, callbacks, ModelPickerOptions::default());

    component.handle_native_event(&NativeUiEvent::Select {
        key: Vec::new(),
        item: "anthropic/sonnet".to_string(),
    });
    assert_eq!(
        component.selected().map(|item| item.selector.as_str()),
        Some("anthropic/sonnet")
    );
    assert!(picks.borrow().is_empty());

    component.handle_native_event(&NativeUiEvent::Activate {
        key: Vec::new(),
        item: "openai/gpt-5".to_string(),
    });
    assert_eq!(&*picks.borrow(), &["pick:openai/gpt-5:false".to_string()]);
}

#[test]
fn quick_role_mode_routes_a_role_row_to_the_role_callback() {
    let roles: Picks = Rc::new(RefCell::new(Vec::new()));
    let role_sink = roles.clone();
    let callbacks = ModelPickerCallbacks {
        on_pick_role: Some(Box::new(move |role| {
            role_sink.borrow_mut().push(role.role.clone());
        })),
        ..Default::default()
    };
    let mut registry = catalog();
    let mut component = picker(
        &mut registry,
        callbacks,
        ModelPickerOptions {
            quick_roles: vec![ResolvedRoleModel {
                role: "default".to_string(),
                model: model("anthropic", "sonnet"),
                thinking_level: None,
                explicit_thinking_level: false,
            }],
            quick_role_order: vec!["default".to_string()],
            ..Default::default()
        },
    );

    component.handle_input(&maple_tui::keys::Key::Char('@'));
    assert_eq!(component.mode(), ModelPickerMode::Role);
    assert_eq!(
        component.visible_items()
            .iter()
            .map(|item| item.selector.as_str())
            .collect::<Vec<_>>(),
        vec!["@default"]
    );

    component.handle_input(&maple_tui::keys::Key::Enter);
    assert_eq!(&*roles.borrow(), &["default".to_string()]);
}

#[test]
fn select_only_never_marks_models_over_the_context_window() {
    let (callbacks, picks) = pick_callbacks();
    let mut registry = catalog();
    let mut component = picker(
        &mut registry,
        callbacks,
        ModelPickerOptions {
            select_only: true,
            current_context_tokens: Some(10_000),
            ..Default::default()
        },
    );
    component.set_context_window_lookup(Box::new(|_| Some(1)));

    assert_eq!(component.mode(), ModelPickerMode::Selection);
    assert!(!component.is_over_context(&item("openai", "mini")));
    assert_eq!(ModelPickerMode::Selection.status_hint(), SELECT_ONLY_STATUS_HINT);

    component.handle_input(&maple_tui::keys::Key::Enter);
    // Selection-only chrome reports the row without the over-context flag.
    assert_eq!(
        &*picks.borrow(),
        &["pick:anthropic/sonnet:false".to_string()]
    );
}

fn picker_text(node: &NativeNode) -> String {
    let mut out = String::new();
    if let Some(text) = node.props.text() {
        out.push_str(text);
    }
    if let Some(spans) = node.props.spans() {
        for span in spans {
            out.push_str(&span.t);
        }
    }
    for child in &node.children {
        out.push_str(&picker_text(child));
    }
    out
}

#[test]
fn esc_reports_dismissal_through_the_cancel_callback() {
    let mut registry = catalog();
    let cancelled: Picks = Rc::new(RefCell::new(Vec::new()));
    let sink = cancelled.clone();
    let callbacks = ModelPickerCallbacks {
        on_cancel: Some(Box::new(move || sink.borrow_mut().push("cancel".to_string()))),
        ..Default::default()
    };
    let mut component = picker(&mut registry, callbacks, ModelPickerOptions::default());
    component.handle_input(&maple_tui::keys::Key::Esc);
    assert_eq!(&*cancelled.borrow(), &["cancel".to_string()]);
    // The highlighted row survives a dismissal.
    assert_eq!(component.selected().map(|item| item.selector.as_str()), Some("anthropic/sonnet"));
}

#[test]
fn empty_catalogue_lists_nothing_and_names_the_placeholder() {
    let mut registry = FakeRegistry { items: Vec::new() };
    let (callbacks, _picks) = pick_callbacks();
    let mut component = picker(&mut registry, callbacks, ModelPickerOptions::default());
    assert!(component.visible_items().is_empty());
    assert_eq!(component.selected(), None);
    assert_eq!(component.empty_text(), "No models available");
    let rendered = component.render(60);
    assert!(rendered.iter().any(|line| line.contains("No models available")));
}

#[test]
fn fallback_describe_reports_role_head_status_and_rows() {
    let mut registry = catalog();
    let (callbacks, _picks) = pick_callbacks();
    let component = picker(&mut registry, callbacks, ModelPickerOptions::default());
    let node = component.describe(&DescribeContext::new(80));
    assert_eq!(node.props.get_str("role"), Some(MODEL_PICKER_ROLE));
    assert_eq!(node.props.head().and_then(|head| head.as_plain()), Some("Switch model"));
    let text = picker_text(&node);
    assert!(text.contains("Switch model"));
    assert!(text.contains("anthropic/sonnet"));
    assert!(text.contains("openai/gpt-5"));
}

#[test]
fn terminal_height_bounds_the_list_window() {
    let mut registry = catalog();
    let (callbacks, _picks) = pick_callbacks();
    let mut component = picker(&mut registry, callbacks, ModelPickerOptions::default());
    component.set_terminal_rows(16);
    assert_eq!(component.max_visible(), MIN_VISIBLE);
    component.set_terminal_rows(80);
    assert!(component.max_visible() > MIN_VISIBLE);
    component.set_max_visible(1);
    assert_eq!(component.max_visible(), MIN_VISIBLE);
}
