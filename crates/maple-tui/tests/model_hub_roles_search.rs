//! Port of `l/test/model-hub-roles-search.test.ts`.
//!
//! Deviations: the reference inspects `TspPickerProps` (`query`, `strip`,
//! `scope`) from a describe tree; the crate exposes state through rendering and
//! [`ModelHub::active_entry`], so the same behavior is asserted on those.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::model_browser::{CatalogModel, ModelBrowserSource, ModelSummary};
use maple_tui::overlays::model_hub::{
    ModelHub, ModelHubCallbacks, ModelHubOptions, ModelHubRegistry, ModelHubSource,
};

fn model(provider: &str, id: &str) -> CatalogModel {
    CatalogModel {
        provider: provider.to_string(),
        id: id.to_string(),
        summary: ModelSummary {
            name: id.to_string(),
            ..ModelSummary::default()
        },
    }
}

fn catalog() -> Vec<CatalogModel> {
    vec![model("anthropic", "opus")]
}

struct FakeSource;

impl ModelBrowserSource for FakeSource {
    fn known_role_ids(&self) -> Vec<String> {
        vec!["default".to_string()]
    }
}

impl ModelHubSource for FakeSource {}

struct FakeRegistry;

impl ModelHubRegistry for FakeRegistry {
    fn get_all(&self) -> Vec<CatalogModel> {
        catalog()
    }

    fn get_available(&self) -> Vec<CatalogModel> {
        catalog()
    }
}

#[derive(Default)]
struct RecordCallbacks {
    saved: Rc<RefCell<Vec<String>>>,
}

impl ModelHubCallbacks for RecordCallbacks {
    fn on_save_preset(&self, name: &str) {
        self.saved.borrow_mut().push(name.to_string());
    }
}

fn hub() -> (ModelHub, Rc<RefCell<Vec<String>>>) {
    let saved = Rc::new(RefCell::new(Vec::new()));
    let callbacks = RecordCallbacks {
        saved: Rc::clone(&saved),
    };
    let hub = ModelHub::new(
        Arc::new(FakeSource),
        Box::new(FakeRegistry),
        Vec::new(),
        Box::new(callbacks),
        ModelHubOptions::default(),
    );
    (hub, saved)
}

fn screen(hub: &mut ModelHub) -> String {
    hub.render(120).join("\n")
}

#[test]
fn with_no_query_the_roles_rows_keep_their_letter_commands() {
    let (mut hub, saved) = hub();
    hub.set_active_entry("roles");
    assert_eq!(
        hub.active_entry().map(|entry| entry.entry.id.as_str()),
        Some("roles")
    );

    // `s` is the save-preset command, not a search keystroke.
    hub.handle_input(&Key::Char('s'));
    let rendered = screen(&mut hub);
    assert!(rendered.contains("Preset name:"), "{rendered}");
    assert!(saved.borrow().is_empty(), "strip is open, not submitted");
}

#[test]
fn a_live_query_keeps_taking_printable_keys_on_the_roles_rows() {
    let (mut hub, _saved) = hub();
    for ch in ['s', 'o', 'n'] {
        hub.handle_input(&Key::Char(ch));
    }

    // Dive into the Roles rows with a query live.
    hub.set_active_entry("roles");
    hub.handle_input(&Key::Char('n'));
    hub.handle_input(&Key::Char('e'));

    // The keys kept editing the query and moved focus back to the list.
    assert_eq!(
        hub.active_entry().map(|entry| entry.entry.id.as_str()),
        Some("all")
    );
    let rendered = screen(&mut hub);
    assert!(rendered.contains("sonne"), "{rendered}");
    assert!(!rendered.contains("Preset name:"), "{rendered}");
}

#[test]
fn backspace_on_a_live_query_stays_on_the_list() {
    let (mut hub, _saved) = hub();
    for ch in ['s', 'o', 'n'] {
        hub.handle_input(&Key::Char(ch));
    }
    hub.set_active_entry("roles");
    hub.handle_input(&Key::Backspace);

    assert_eq!(
        hub.active_entry().map(|entry| entry.entry.id.as_str()),
        Some("all")
    );
    let rendered = screen(&mut hub);
    assert!(rendered.contains("so"), "{rendered}");
    assert!(!rendered.contains("son"), "{rendered}");
}
