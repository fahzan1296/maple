// Port target: l/test/select-list.test.ts
// Scope: core selection, type-to-filter/search on overflow, activation, status text,
// and description/scrollbar rendering basics. Advanced TS-only features (icon column,
// rich wrapped description layout, custom truncatePrimary) are out of scope.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::component::Component;
use maple_tui::components::select_list::{SelectItem, SelectList, SelectListTheme};
use maple_tui::keys::Key;
use maple_tui::style::Style;
use maple_tui::symbols::SymbolTheme;

fn plain_theme() -> SelectListTheme {
    SelectListTheme {
        muted: Style::default(),
        selected: Style::default(),
        selected_row: Style::default(),
        border: Style::default(),
        select_label: Style::default(),
        hint: Style::default(),
        symbols: SymbolTheme::unicode(),
    }
}

fn items() -> Vec<SelectItem> {
    vec![
        SelectItem::new("alpha", "Alpha"),
        SelectItem::new("beta", "Beta"),
        SelectItem::new("gamma", "Gamma"),
    ]
}

#[test]
fn renders_items_with_selection_cursor() {
    let mut list = SelectList::new(items(), 5, plain_theme());
    let rendered = list.render(40).join("\n");
    assert!(rendered.contains("> Alpha"));
    assert!(rendered.contains("  Beta"));
    assert!(rendered.contains("  Gamma"));
}

#[test]
fn normalizes_multiline_descriptions_in_render() {
    let mut list = SelectList::new(
        vec![SelectItem::new("t", "test").with_description("Line one\nLine two\nLine three")],
        5,
        plain_theme(),
    );
    let rendered = list.render(80);
    assert_eq!(rendered.len(), 1);
    assert!(!rendered[0].contains('\n'));
    assert!(rendered[0].contains("Line one Line two Line three"));
}

#[test]
fn arrow_navigation_and_wrap() {
    let mut list = SelectList::new(items(), 5, plain_theme());
    assert_eq!(list.selected_key().as_deref(), Some("alpha"));
    list.handle_input(&Key::Down);
    assert_eq!(list.selected_key().as_deref(), Some("beta"));
    list.handle_input(&Key::Up);
    assert_eq!(list.selected_key().as_deref(), Some("alpha"));
    list.handle_input(&Key::Up);
    assert_eq!(list.selected_key().as_deref(), Some("gamma")); // wraps by default
}

#[test]
fn enter_activates_selected_item() {
    let mut list = SelectList::new(items(), 5, plain_theme());
    let selected: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = selected.clone();
    list.on_select(move |item| sink.borrow_mut().push(item.key.clone()));

    list.handle_input(&Key::Down);
    list.handle_input(&Key::Enter);
    assert_eq!(*selected.borrow(), vec!["beta".to_string()]);
}

#[test]
fn type_to_filter_on_overflow_lists() {
    let overflow = vec![
        SelectItem::new("ollama", "Ollama"),
        SelectItem::new("kagi", "Kagi"),
        SelectItem::new("opencode-go", "OpenCode Go"),
        SelectItem::new("tavily", "Tavily"),
    ];
    let mut list = SelectList::new(overflow, 3, plain_theme());
    list.handle_input(&Key::Char('o'));
    list.handle_input(&Key::Char('g'));

    let rendered = list.render(80).join("\n");
    assert!(rendered.contains("OpenCode Go"));
    assert!(!rendered.contains("Ollama"));
    assert!(rendered.contains("Search: og"));
    assert_eq!(list.selected_key().as_deref(), Some("opencode-go"));
}

#[test]
fn printable_keys_inert_when_list_does_not_overflow() {
    let mut list = SelectList::new(
        vec![SelectItem::new("alpha", "Alpha"), SelectItem::new("beta", "Beta")],
        5,
        plain_theme(),
    );
    list.handle_input(&Key::Char('b'));
    let rendered = list.render(80).join("\n");
    assert!(rendered.contains("Alpha"));
    assert!(rendered.contains("Beta"));
    assert!(!rendered.contains("Search:"));
    assert_eq!(list.selected_key().as_deref(), Some("alpha"));
}

#[test]
fn shows_scrollbar_when_overflowing_and_omits_when_fitting() {
    let overflow = (0..8)
        .map(|i| SelectItem::new(format!("v{i}"), format!("Item {i}")))
        .collect::<Vec<_>>();
    let mut list = SelectList::new(overflow, 3, plain_theme());
    let rendered = list.render(40).join("\n");
    assert!(rendered.contains('█'));

    let mut small = SelectList::new(
        vec![SelectItem::new("a", "Alpha"), SelectItem::new("b", "Beta")],
        5,
        plain_theme(),
    );
    assert!(!small.render(40).join("\n").contains('█'));
}

#[test]
fn escape_invokes_cancel_when_no_confirmation_pending() {
    let mut list = SelectList::new(items(), 5, plain_theme());
    let cancelled = Rc::new(RefCell::new(0usize));
    let sink = cancelled.clone();
    list.on_cancel(move || *sink.borrow_mut() += 1);
    list.handle_input(&Key::Esc);
    assert_eq!(*cancelled.borrow(), 1);
}
