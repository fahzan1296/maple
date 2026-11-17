// Port target: l/test/tab-bar.test.ts
// Scope: render layout/hints, keyboard cycling, muted tab handling, selection APIs,
// pointer hit zones. Styling assertions are structural (content/width), not ANSI.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::components::tab_bar::{Tab, TabBar, TabBarTheme};
use maple_tui::keys::Key;
use maple_tui::mouse::{MouseButtonKind, MouseEvent, MouseKind};
use maple_tui::style::Style;
use maple_tui::symbols::SymbolTheme;
use maple_tui::text_shape::visible_width;

fn plain_theme() -> TabBarTheme {
    TabBarTheme {
        label: Style::default(),
        active_tab: Style::default(),
        inactive_tab: Style::default(),
        hint: Style::default(),
        muted_tab: None,
        hover_tab: None,
        symbols: SymbolTheme::unicode(),
    }
}

#[test]
fn renders_label_tabs_and_hint_on_one_line() {
    let mut bar = TabBar::new(
        "Settings",
        vec![
            Tab::new("display", "Display"),
            Tab::new("agent", "Agent"),
            Tab::new("input", "Input"),
        ],
        plain_theme(),
    );
    let lines = bar.render(80);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("Settings:"));
    assert!(lines[0].contains("Display"));
    assert!(lines[0].contains("Agent"));
    assert!(lines[0].contains("(tab to cycle)"));
}

#[test]
fn wraps_long_tab_rows_without_empty_lines() {
    let mut bar = TabBar::new(
        "Settings",
        vec![
            Tab::new("display", "Display"),
            Tab::new("agent", "Agent"),
            Tab::new("input", "Input"),
            Tab::new("tools", "Tools"),
            Tab::new("config", "Config"),
            Tab::new("services", "Services"),
            Tab::new("bash", "Bash"),
            Tab::new("lsp", "LSP"),
        ],
        plain_theme(),
    );
    let lines = bar.render(40);
    assert!(lines.len() > 1);
    for line in &lines {
        assert!(visible_width(line) <= 40);
        assert!(visible_width(line) > 0);
    }
}

#[test]
fn collapses_distant_tabs_to_short_labels_before_wrapping() {
    let tabs = (0..8)
        .map(|i| Tab::new(format!("tab{i}"), format!("Section {i}")).with_short("⊕"))
        .collect::<Vec<_>>();
    let mut bar = TabBar::new("", tabs, plain_theme());
    bar.set_show_hint(false);
    let lines = bar.render(60);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("Section 0"));
    assert!(!lines[0].contains("Section 7"));
}

#[test]
fn skips_muted_tabs_during_cycling_and_click_selection() {
    let mut bar = TabBar::new(
        "",
        vec![
            Tab::new("a", "A"),
            Tab::new("b", "B").muted(),
            Tab::new("c", "C"),
        ],
        plain_theme(),
    );
    let changes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = changes.clone();
    bar.on_select(move |_, tab| sink.borrow_mut().push(tab.id.clone()));

    bar.next_tab();
    assert_eq!(*changes.borrow(), vec!["c".to_string()]);

    bar.previous_tab();
    assert_eq!(*changes.borrow(), vec!["c".to_string(), "a".to_string()]);

    assert!(!bar.select_tab("b"));
    assert!(bar.select_tab("c"));
    assert_eq!(
        *changes.borrow(),
        vec!["c".to_string(), "a".to_string(), "c".to_string()]
    );
}

#[test]
fn set_tabs_preserves_active_tab_by_id() {
    let mut bar = TabBar::new(
        "",
        vec![Tab::new("a", "A"), Tab::new("b", "B")],
        plain_theme(),
    );
    bar.select_tab("b");
    bar.set_tabs(vec![Tab::new("b", "B (2)"), Tab::new("a", "A").muted()]);
    assert_eq!(bar.active_tab().map(|t| t.id.as_str()), Some("b"));
}

#[test]
fn resolves_tabs_from_pointer_hit_zones_after_render() {
    let mut bar = TabBar::new(
        "",
        vec![Tab::new("first", "First"), Tab::new("second", "Second")],
        plain_theme(),
    );
    bar.set_show_hint(false);
    let lines = bar.render(80);
    assert!(visible_width(&lines[0]) > 0);

    // ` First ` (7 cells) + gap (2) + ` Second ` => column 0 in first, 9 in second
    assert_eq!(bar.tab_at(0, 1).map(|t| t.id.as_str()), Some("first"));
    assert_eq!(bar.tab_at(0, 10).map(|t| t.id.as_str()), Some("second"));
    assert!(bar.tab_at(1, 1).is_none());

    bar.handle_mouse(&MouseEvent {
        kind: MouseKind::Down(MouseButtonKind::Left),
        column: 10,
        row: 0,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert_eq!(bar.active_tab().map(|t| t.id.as_str()), Some("second"));
}

#[test]
fn keyboard_arrows_and_tab_wrap() {
    let mut bar = TabBar::new(
        "",
        vec![Tab::new("a", "A"), Tab::new("b", "B"), Tab::new("c", "C")],
        plain_theme(),
    );
    bar.handle_input(&Key::Right);
    assert_eq!(bar.active_index(), 1);
    bar.handle_input(&Key::Tab);
    assert_eq!(bar.active_index(), 2);
    bar.handle_input(&Key::Tab);
    assert_eq!(bar.active_index(), 0);
    bar.handle_input(&Key::Left);
    assert_eq!(bar.active_index(), 2);
}
