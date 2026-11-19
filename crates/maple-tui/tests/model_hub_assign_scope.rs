//! Port of `l/test/model-hub-assign-scope.test.ts` (#12692).
//!
//! Deviations:
//! - The reference stubs `ModelHubRegistry.refresh`/`refreshProvider` and lets
//!   the hub's async refresh call `onCatalogRefreshed`; this crate keeps I/O in
//!   the host, so `on_catalog_refreshed` is never invoked by [`ModelHub`]. That
//!   half of the reference file is a host-layer gap, reported separately.
//! - Sidebar hops while assigning are driven through the public
//!   [`ModelHub::set_active_entry`] / [`ModelHub::set_candidate_items`] API
//!   instead of raw escape sequences.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use maple_tui::overlays::model_browser::{
    CatalogModel, ModelBrowserSource, ModelSummary, RoleInfo,
};
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

fn web_model() -> CatalogModel {
    model("google", "web-search-pro")
}

fn chat_model() -> CatalogModel {
    model("litellm", "chat-only")
}

fn catalog() -> Vec<CatalogModel> {
    vec![web_model(), chat_model()]
}

struct FakeSource;

impl ModelBrowserSource for FakeSource {
    fn known_role_ids(&self) -> Vec<String> {
        vec!["web".to_string()]
    }

    fn get_role_info(&self, role: &str) -> RoleInfo {
        if role == "web" {
            RoleInfo {
                tag: Some("web".to_string()),
                name: "Web search".to_string(),
                ..RoleInfo::named(role)
            }
        } else {
            RoleInfo::named(role)
        }
    }

    fn role_accepts(&self, role: &str, model: &CatalogModel) -> bool {
        role != "web" || model.provider == "google"
    }
}

impl ModelHubSource for FakeSource {}

struct FakeRegistry {
    catalog: Vec<CatalogModel>,
}

impl ModelHubRegistry for FakeRegistry {
    fn get_all(&self) -> Vec<CatalogModel> {
        self.catalog.clone()
    }

    fn get_available(&self) -> Vec<CatalogModel> {
        self.catalog.clone()
    }
}

#[derive(Default)]
struct RecordCallbacks {
    assigned: Rc<RefCell<Vec<String>>>,
}

impl ModelHubCallbacks for RecordCallbacks {
    fn on_assign(
        &self,
        _model: &CatalogModel,
        role: &str,
        _thinking_level: maple_tui::thinking::ConfiguredThinkingLevel,
        selector: &str,
        _scope: Option<maple_tui::overlays::model_hub::ModelRoleSelectionScope>,
    ) -> bool {
        self.assigned
            .borrow_mut()
            .push(format!("{role}:{selector}"));
        true
    }
}

fn hub() -> (ModelHub, Rc<RefCell<Vec<String>>>) {
    let assigned = Rc::new(RefCell::new(Vec::new()));
    let callbacks = RecordCallbacks {
        assigned: Rc::clone(&assigned),
    };
    let hub = ModelHub::new(
        Arc::new(FakeSource),
        Box::new(FakeRegistry { catalog: catalog() }),
        Vec::new(),
        Box::new(callbacks),
        ModelHubOptions::default(),
    );
    (hub, assigned)
}

fn body(hub: &mut ModelHub) -> String {
    hub.render_body(120, 24).join("\n")
}

#[test]
fn assign_mode_filters_out_models_the_role_rejects() {
    let (mut hub, _assigned) = hub();
    hub.start_assign("web");

    let rendered = body(&mut hub);
    assert!(rendered.contains("web-search-pro"), "{rendered}");
    assert!(!rendered.contains("chat-only"), "{rendered}");
}

#[test]
fn rescope_while_assigning_reapplies_the_role_filter() {
    let (mut hub, _assigned) = hub();
    hub.start_assign("web");
    // A scope refresh hands the candidate list back; the role filter must run.
    let items = maple_tui::overlays::model_browser::build_browser_items(&catalog());
    hub.set_candidate_items(items);

    let rendered = body(&mut hub);
    assert!(rendered.contains("web-search-pro"), "{rendered}");
    assert!(!rendered.contains("chat-only"), "{rendered}");
}

#[test]
fn outside_assign_mode_the_provider_scope_still_lists_its_model() {
    let (mut hub, _assigned) = hub();
    hub.set_active_entry("provider:litellm");

    let rendered = body(&mut hub);
    assert!(rendered.contains("chat-only"), "{rendered}");
}

#[test]
fn eligible_provider_scope_keeps_the_accepted_model() {
    let (mut hub, _assigned) = hub();
    hub.start_assign("web");
    hub.set_active_entry("provider:google");

    let rendered = body(&mut hub);
    assert!(rendered.contains("web-search-pro"), "{rendered}");
    assert!(!rendered.contains("chat-only"), "{rendered}");
}
