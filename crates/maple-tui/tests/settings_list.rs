// Port target: l/test/settings-list.test.ts
// Scope: value cycling, search/filter, selection, heading skipping, basic navigation.
// TS-only features (split sidebar layout, section-focus mode, exact flat-section UI)
// are not 1:1 in the Rust flat SettingsList; assertions target shared behavior.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::components::settings_list::{SettingItem, SettingsList, SettingsListTheme};
use maple_tui::keys::Key;
use maple_tui::style::Style;
use maple_tui::symbols::SymbolTheme;

fn plain_theme() -> SettingsListTheme {
    SettingsListTheme {
        label: Style::default(),
        selected_label: Style::default(),
        value: Style::default(),
        selected_value: Style::default(),
        description: Style::default(),
        warning: Style::default(),
        cursor: Style::default(),
        hint: Style::default(),
        heading: Style::default(),
        selected_row: Style::default(),
        warning_mark: "!".to_string(),
        symbols: SymbolTheme::unicode(),
    }
}

#[test]
fn enter_cycles_selected_value() {
    let changes: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let mut list = SettingsList::new(
        vec![SettingItem::new("mode", "Mode", "off").with_values(vec![
            "off".to_string(),
            "on".to_string(),
        ])],
        5,
        plain_theme(),
    );
    list.on_toggle(move |id, value| sink.borrow_mut().push((id.to_string(), value.to_string())));

    list.handle_input(&Key::Enter);
    assert_eq!(
        *changes.borrow(),
        vec![("mode".to_string(), "on".to_string())]
    );
}

#[test]
fn filters_settings_with_search_text() {
    let mut list = SettingsList::new(
        vec![
            SettingItem::new("mode", "Mode", "off").with_values(vec!["off".into(), "on".into()]),
            SettingItem::new("theme.dark", "Theme", "dark").with_values(vec!["dark".into(), "light".into()]),
            SettingItem::new("browser.path", "Browser Path", "")
                .with_description("Executable used for browser launches"),
        ],
        5,
        plain_theme(),
    );

    list.handle_input(&Key::Char('b'));
    let output = list.render(80).join("\n");
    assert!(output.contains("Search: b"));
    assert!(output.contains("Browser Path"));
    assert!(!output.contains("Theme"));
    assert!(!output.contains("Mode"));
}

#[test]
fn escape_clears_search_before_cancel() {
    let cancelled = Rc::new(RefCell::new(0usize));
    let sink = cancelled.clone();
    let mut list = SettingsList::new(
        vec![
            SettingItem::new("mode", "Mode", "off"),
            SettingItem::new("browser.path", "Browser Path", ""),
        ],
        5,
        plain_theme(),
    );
    list.on_cancel(move || *sink.borrow_mut() += 1);

    list.handle_input(&Key::Char('b'));
    assert!(!list.search_query().is_empty());

    list.handle_input(&Key::Esc);
    assert!(list.search_query().is_empty());
    assert_eq!(*cancelled.borrow(), 0);

    list.handle_input(&Key::Esc);
    assert_eq!(*cancelled.borrow(), 1);
}

#[test]
fn headings_are_skipped_by_navigation_and_activation() {
    let changes: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    let mut list = SettingsList::new(
        vec![
            SettingItem::new("__h:a", "Group A", "").heading(),
            SettingItem::new("alpha", "Alpha", "off").with_values(vec!["off".into(), "on".into()]),
            SettingItem::new("__h:b", "Group B", "").heading(),
            SettingItem::new("beta", "Beta", "off").with_values(vec!["off".into(), "on".into()]),
        ],
        10,
        plain_theme(),
    );
    list.on_toggle(move |id, value| sink.borrow_mut().push((id.to_string(), value.to_string())));

    list.handle_input(&Key::Enter);
    assert_eq!(*changes.borrow(), vec![("alpha".into(), "on".into())]);

    list.handle_input(&Key::Down);
    list.handle_input(&Key::Enter);
    assert_eq!(
        *changes.borrow(),
        vec![("alpha".into(), "on".into()), ("beta".into(), "on".into())]
    );

    // wrap past last to first selectable (skipping leading heading)
    list.handle_input(&Key::Down);
    list.handle_input(&Key::Enter);
    assert_eq!(
        *changes.borrow(),
        vec![
            ("alpha".into(), "on".into()),
            ("beta".into(), "on".into()),
            ("alpha".into(), "off".into()),
        ]
    );
}

#[test]
fn selects_item_by_id_and_updates_value_programmatically() {
    let mut list = SettingsList::new(
        vec![
            SettingItem::new("a", "A", "1"),
            SettingItem::new("b", "B", "2"),
        ],
        5,
        plain_theme(),
    );
    assert!(list.select_item("b"));
    assert_eq!(list.get_selected_item().map(|i| i.id.as_str()), Some("b"));
    list.update_value("b", "20");
    assert_eq!(
        list.items().iter().find(|i| i.id == "b").map(|i| i.value.as_str()),
        Some("20")
    );
}

#[test]
fn search_filters_out_headings() {
    let mut list = SettingsList::new(
        vec![
            SettingItem::new("__h", "Group Alpha", "").heading(),
            SettingItem::new("alpha", "Alpha Mode", "off"),
            SettingItem::new("beta", "Beta Mode", "off"),
        ],
        5,
        plain_theme(),
    );
    for ch in "group".chars() {
        list.handle_input(&Key::Char(ch));
    }
    let output = list.render(80).join("\n");
    assert!(!output.contains("Group Alpha"));
    assert!(output.contains("No matching settings"));
}
