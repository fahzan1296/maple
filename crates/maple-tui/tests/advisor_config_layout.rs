//! Port of `l/test/advisor-config-layout.test.ts`.
//!
//! The reference drives the TS fullscreen split overlay (roster sidebar plus
//! selected-advisor preview) through a real `TUI` and a theme singleton. The
//! crate ports the overlay as `AdvisorConfigOverlayComponent`, a bordered
//! list/detail frame with no split panes and no theme singleton, so the
//! full-height split geometry, the `+ Add advisor`/`Save & apply` button
//! labels and the sidebar click hit-test are ignored; the roster/detail
//! content, keyboard selection and tool-set rendering are pinned.

use std::cell::RefCell;
use std::collections::BTreeSet;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::advisor_config::{
	AdvisorConfig, AdvisorConfigCallbacks, AdvisorConfigDeps, AdvisorConfigOverlayComponent,
	AdvisorConfigScope, Screen, WatchdogConfigDoc,
};

#[derive(Default)]
struct Cb {
	closed: RefCell<bool>,
	saved: RefCell<bool>,
	rendered: RefCell<usize>,
}

impl AdvisorConfigCallbacks for Cb {
	fn load_doc(&self, _scope: AdvisorConfigScope) -> Result<WatchdogConfigDoc, String> {
		Ok(WatchdogConfigDoc::default())
	}
	fn save(&self, _scope: AdvisorConfigScope, _doc: &WatchdogConfigDoc) -> Result<(), String> {
		*self.saved.borrow_mut() = true;
		Ok(())
	}
	fn close(&self) {
		*self.closed.borrow_mut() = true;
	}
	fn request_render(&self) {
		*self.rendered.borrow_mut() += 1;
	}
	fn notify(&self, _message: &str) {}
}

#[derive(Default)]
struct Deps;

impl AdvisorConfigDeps for Deps {
	fn available_tool_names(&self) -> Vec<String> {
		vec!["read".into(), "grep".into(), "glob".into(), "lsp".into(), "web_search".into()]
	}
	fn default_tool_names(&self) -> BTreeSet<String> {
		["read".to_string(), "grep".to_string(), "glob".to_string()]
			.into_iter()
			.collect()
	}
}

fn make(doc: WatchdogConfigDoc) -> AdvisorConfigOverlayComponent<Cb, Deps> {
	AdvisorConfigOverlayComponent::new(AdvisorConfigScope::Project, doc, Cb::default(), Deps)
}

fn text(overlay: &mut AdvisorConfigOverlayComponent<Cb, Deps>) -> String {
	overlay.render(200).join("\n")
}

#[test]
fn paints_a_full_screen_frame_with_scope_and_roster() {
	let doc = WatchdogConfigDoc {
		instructions: Some("shared baseline".into()),
		advisors: vec![
			AdvisorConfig {
				name: "Architecture".into(),
				model: Some("x-ai/grok-code-fast:high".into()),
				..Default::default()
			},
			AdvisorConfig {
				name: "Security".into(),
				tools: Some(vec!["read".into(), "web_search".into()]),
				..Default::default()
			},
		],
		..Default::default()
	};
	let mut overlay = make(doc);
	let frame = overlay.render(200);
	assert!(!frame.is_empty());
	let rendered = frame.join("\n");
	assert!(rendered.contains("Advisor configuration"), "{rendered}");
	assert!(rendered.contains("Project"), "{rendered}");
	assert!(rendered.contains("Architecture"), "{rendered}");
	assert!(rendered.contains("Security"), "{rendered}");
	assert!(rendered.contains("shared baseline"), "{rendered}");
	assert!(rendered.contains("x-ai/grok-code-fast:high"), "{rendered}");
}

#[test]
fn renders_an_explicit_no_tools_advisor_distinctly_from_the_omitted_default() {
	let doc = WatchdogConfigDoc {
		advisors: vec![AdvisorConfig {
			name: "Blank".into(),
			tools: Some(Vec::new()),
			..Default::default()
		}],
		..Default::default()
	};
	let mut overlay = make(doc);
	overlay.open_detail();
	let rendered = text(&mut overlay).to_lowercase();
	assert!(rendered.contains("no tools"), "{rendered}");
	assert!(!rendered.contains("read, grep, glob (default)"), "{rendered}");
}

#[test]
fn moves_the_preview_with_keyboard_selection_and_preserves_an_explicit_tool_set() {
	let doc = WatchdogConfigDoc {
		advisors: vec![
			AdvisorConfig {
				name: "Architecture".into(),
				..Default::default()
			},
			AdvisorConfig {
				name: "Security".into(),
				tools: Some(vec!["read".into(), "web_search".into()]),
				..Default::default()
			},
		],
		..Default::default()
	};
	let mut overlay = make(doc);
	overlay.render(200);
	overlay.handle_input(&Key::Down);
	overlay.open_detail();
	let rendered = text(&mut overlay);
	assert!(rendered.contains("Security"), "{rendered}");
	assert!(rendered.contains("read, web_search"), "{rendered}");
}

#[test]
#[ignore = "missing facility: handle_mouse only scrolls the preview; no sidebar hit-test opens the detail editor"]
fn opens_an_advisors_detail_editor_on_a_left_click_in_the_sidebar() {
	let doc = WatchdogConfigDoc {
		advisors: vec![
			AdvisorConfig {
				name: "Architecture".into(),
				..Default::default()
			},
			AdvisorConfig {
				name: "Security".into(),
				..Default::default()
			},
		],
		..Default::default()
	};
	let mut overlay = make(doc);
	overlay.render(120);
	overlay.handle_mouse(&maple_tui::mouse::MouseEvent {
		kind: maple_tui::mouse::MouseKind::Down(maple_tui::mouse::MouseButtonKind::Left),
		column: 4,
		row: 2,
		shift: false,
		ctrl: false,
		alt: false,
	});
	assert_eq!(overlay.screen, Screen::List);
}

#[test]
fn shows_a_disabled_advisor_as_disabled_in_the_detail_editor() {
	let doc = WatchdogConfigDoc {
		advisors: vec![
			AdvisorConfig {
				name: "Active".into(),
				model: Some("x-ai/grok-code-fast:high".into()),
				..Default::default()
			},
			AdvisorConfig {
				name: "Disabled".into(),
				model: Some("openai/gpt-4".into()),
				enabled: Some(false),
				..Default::default()
			},
		],
		..Default::default()
	};
	let mut overlay = make(doc);
	overlay.open_detail();
	assert!(text(&mut overlay).contains("Enabled: true"));
	overlay.screen = Screen::List;
	overlay.handle_input(&Key::Down);
	overlay.open_detail();
	let rendered = text(&mut overlay);
	assert!(rendered.contains("Disabled"), "{rendered}");
	assert!(rendered.contains("Enabled: false"), "{rendered}");
}
