//! Port of `l/test/extension-dashboard-native.test.ts`.
//!
//! The TS test drives the native picker/list wire protocol. This crate has no
//! picker node API, so activation and scope switching are exercised through the
//! dashboard's own input handling and rendered output.

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::overlays::extensions::extension_dashboard::{
	ExtensionDashboard, ExtensionDashboardOptions, ExtensionDashboardRuntime,
};
use maple_tui::overlays::extensions::types::{
	DisabledReason, Extension, ExtensionKind, ExtensionProvider, ExtensionSource, ExtensionState,
};

fn extension(kind: ExtensionKind, name: &str, provider: &str) -> Extension {
	Extension {
		id: format!("{}:{name}", kind.as_str()),
		kind,
		name: name.to_string(),
		display_name: name.to_string(),
		description: Some(format!("{name} description")),
		path: format!("/home/u/.omp/{}/{name}.md", kind.as_str()),
		source: ExtensionSource {
			provider: provider.to_string(),
			provider_name: provider.to_string(),
			level: "user".to_string(),
		},
		state: ExtensionState::Active,
		raw: serde_json::json!({ "content": format!("# {name}") }),
		..Extension::default()
	}
}

struct MockRuntime {
	extensions: Vec<Extension>,
	providers: Vec<ExtensionProvider>,
	disabled: Vec<String>,
}

impl ExtensionDashboardRuntime for MockRuntime {
	fn load_extensions(&self, disabled_ids: &[String]) -> Vec<Extension> {
		self.extensions
			.iter()
			.map(|ext| {
				if disabled_ids.contains(&ext.id) {
					Extension {
						state: ExtensionState::Disabled,
						disabled_reason: Some(DisabledReason::ItemDisabled),
						..ext.clone()
					}
				} else {
					ext.clone()
				}
			})
			.collect()
	}
	fn get_providers(&self) -> Vec<ExtensionProvider> {
		self.providers.clone()
	}
	fn get_disabled_extensions(&self) -> Vec<String> {
		self.disabled.clone()
	}
	fn toggle_provider(&mut self, provider_id: &str) -> Option<bool> {
		for p in &mut self.providers {
			if p.id == provider_id {
				p.enabled = !p.enabled;
				return Some(p.enabled);
			}
		}
		None
	}
	fn set_provider_user_source_enabled(&mut self, provider_id: &str, enabled: bool) -> Option<bool> {
		for p in &mut self.providers {
			if p.id == provider_id {
				p.user_source_enabled = enabled;
				return Some(enabled);
			}
		}
		None
	}
	fn toggle_extension(&mut self, extension_id: &str, enabled: bool) -> Option<bool> {
		if enabled {
			self.disabled.retain(|id| id != extension_id);
		} else if !self.disabled.iter().any(|id| id == extension_id) {
			self.disabled.push(extension_id.to_string());
		}
		Some(enabled)
	}
}

fn providers() -> Vec<ExtensionProvider> {
	vec![
		ExtensionProvider {
			id: "native".to_string(),
			display_name: "OMP".to_string(),
			enabled: true,
			user_source_enabled: true,
			foreign_user_source: false,
		},
		ExtensionProvider {
			id: "claude".to_string(),
			display_name: "Claude Code".to_string(),
			enabled: true,
			user_source_enabled: true,
			foreign_user_source: false,
		},
	]
}

fn dashboard(extensions: Vec<Extension>) -> ExtensionDashboard<MockRuntime> {
	ExtensionDashboard::new(ExtensionDashboardOptions {
		runtime: MockRuntime {
			extensions,
			providers: providers(),
			disabled: Vec::new(),
		},
		terminal_height: Some(30),
	})
}

#[test]
fn renders_picker_chrome_and_provider_tabs() {
	let mut dash = dashboard(vec![
		extension(ExtensionKind::Skill, "alpha", "native"),
		extension(ExtensionKind::SlashCommand, "gamma", "claude"),
	]);
	let text = dash.render(100).join("\n");
	assert!(text.contains("Extension Control Center"));
	assert!(text.contains("switch scope"));
	assert!(text.contains("alpha"));
	assert!(text.contains("gamma"));
}

#[test]
fn row_activation_toggles_then_reenables() {
	let mut dash = dashboard(vec![extension(ExtensionKind::Skill, "alpha", "native")]);
	let _ = dash.render(100);
	for _ in 0..6 {
		dash.handle_input(&Key::Down); // land on alpha, the only extension row
	}
	dash.handle_input(&Key::Enter);
	assert!(dash.render(100).join("\n").contains("Disabled"));

	dash.handle_input(&Key::Enter);
	let restored = dash.render(100).join("\n");
	assert!(!restored.contains("Disabled"));
	assert!(restored.contains("Active"));
}

#[test]
fn tab_switches_provider_scope() {
	let mut dash = dashboard(vec![
		extension(ExtensionKind::Skill, "alpha", "native"),
		extension(ExtensionKind::SlashCommand, "gamma", "claude"),
	]);
	let all = dash.render(100).join("\n");
	assert!(all.contains("alpha"));
	assert!(all.contains("gamma"));

	dash.handle_input(&Key::Tab);
	let scoped = dash.render(100).join("\n");
	assert_ne!(scoped.contains("alpha"), scoped.contains("gamma"));
}

#[test]
fn closing_with_escape() {
	let mut dash = dashboard(vec![extension(ExtensionKind::Skill, "alpha", "native")]);
	assert!(!dash.is_closed());
	dash.handle_input(&Key::Esc);
	assert!(dash.is_closed());
}
