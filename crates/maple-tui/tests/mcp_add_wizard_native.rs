//! Port of `l/test/mcp-add-wizard-native.test.ts` (portable subset).
//!
//! The reference drives the wizard through a TUI sheet and inspects the native
//! describe tree. The crate's wizard is synchronous and host-injected, so these
//! tests drive [`McpAddWizard::handle_native_event`]/[`McpAddWizard::paste_text`]
//! against a fake [`McpAddWizardHost`] and assert the step machine, the probe
//! config, the persisted JSON shape and the `omp.mcpAdd.*` describe nodes.

use std::cell::RefCell;
use std::rc::Rc;

use maple_tui::native::backend::NativeUiEvent;
use maple_tui::native::describe::NativeDescribe;
use maple_tui::native::node::{DescribeContext, NativeNode};
use maple_tui::overlays::mcp_add_wizard::{
	AuthDetection, AuthMethod, AuthType, McpAddWizard, McpAddWizardHost, McpOAuthEndpoints,
	McpServerConfig, McpWizardStep, OAuthFlowOptions, OAuthResult, PHASES, Transport,
	WizardError, WizardPaths, MCP_ADD_ROLE,
};

#[derive(Default)]
struct FakeHost {
	name_error: Option<String>,
	probed: Rc<RefCell<Vec<McpServerConfig>>>,
	test_error: Option<WizardError>,
	oauth: Option<McpOAuthEndpoints>,
	detection: AuthDetection,
}

impl McpAddWizardHost for FakeHost {
	fn validate_server_name(&self, name: &str) -> Option<String> {
		let _ = name;
		self.name_error.clone()
	}

	fn analyze_auth_error(&self, _error: &WizardError, _server_url: Option<&str>) -> AuthDetection {
		self.detection.clone()
	}

	fn discover_oauth_endpoints(
		&mut self,
		_server_url: &str,
		_auth_server_url: Option<&str>,
		_resource_metadata_url: Option<&str>,
		_protected_scopes: Option<&str>,
	) -> Option<McpOAuthEndpoints> {
		self.oauth.clone()
	}

	fn fetch_resource_metadata_scopes(&mut self, _resource_metadata_url: &str) -> Option<String> {
		Some("read write".to_string())
	}

	fn test_connection(&mut self, config: &McpServerConfig) -> Result<(), WizardError> {
		self.probed.borrow_mut().push(config.clone());
		match &self.test_error {
			Some(error) => Err(error.clone()),
			None => Ok(()),
		}
	}

	fn start_oauth(
		&mut self,
		_auth_url: &str,
		_token_url: &str,
		_client_id: &str,
		_client_secret: &str,
		_scopes: &str,
		_options: &OAuthFlowOptions,
	) -> Result<OAuthResult, WizardError> {
		Ok(OAuthResult {
			credential_id: "cred-1".to_string(),
			..OAuthResult::default()
		})
	}
}

type Done = Rc<RefCell<Vec<String>>>;
type Cancelled = Rc<RefCell<Vec<String>>>;

fn paths() -> WizardPaths {
	WizardPaths {
		user: "~/.omp/mcp.json".to_string(),
		project: ".omp/mcp.json".to_string(),
	}
}

fn wizard_with(
	host: FakeHost,
	initial_name: Option<&str>,
) -> (McpAddWizard, Done, Cancelled, Rc<RefCell<Vec<McpServerConfig>>>) {
	let done: Done = Rc::new(RefCell::new(Vec::new()));
	let cancelled: Cancelled = Rc::new(RefCell::new(Vec::new()));
	let probed = host.probed.clone();
	let done_sink = Rc::clone(&done);
	let cancel_sink = Rc::clone(&cancelled);
	let wizard = McpAddWizard::new(
		Box::new(host),
		paths(),
		move |result| {
			done_sink.borrow_mut().push(format!("{}:{}", result.name, result.scope.as_str()));
		},
		move || cancel_sink.borrow_mut().push("cancel".to_string()),
		|| {},
		initial_name,
	);
	(wizard, done, cancelled, probed)
}

fn wizard() -> (McpAddWizard, Done, Cancelled, Rc<RefCell<Vec<McpServerConfig>>>) {
	wizard_with(FakeHost::default(), Some("github"))
}

fn action(act: &str) -> NativeUiEvent {
	NativeUiEvent::Action {
		key: Vec::new(),
		act: act.to_string(),
		value: None,
		mods: Vec::new(),
	}
}

fn select(key: &str, item: &str) -> NativeUiEvent {
	NativeUiEvent::Select {
		key: vec![key.to_string()],
		item: item.to_string(),
	}
}

fn find_role<'a>(node: &'a NativeNode, role: &str) -> Option<&'a NativeNode> {
	if node.props.get_str("role") == Some(role) {
		return Some(node);
	}
	node.children.iter().find_map(|child| find_role(child, role))
}

fn node_text(node: &NativeNode) -> String {
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
		out.push_str(&node_text(child));
	}
	out
}

#[test]
fn initial_name_skips_the_name_step_and_cancel_reports_back() {
	let (wizard, _, _, _) = wizard_with(FakeHost::default(), None);
	assert_eq!(wizard.step(), McpWizardStep::Name);

	let (wizard, _, _, _) = wizard_with(FakeHost::default(), Some("   "));
	assert_eq!(wizard.step(), McpWizardStep::Name);

	let (wizard, _, _, _) = wizard();
	assert_eq!(wizard.step(), McpWizardStep::Transport);
	assert_eq!(wizard.state().name, "github");

	let (mut wizard, _, cancelled, _) = wizard();
	wizard.handle_native_event(&action("cancel"));
	assert_eq!(&*cancelled.borrow(), &["cancel".to_string()]);
}

#[test]
fn transport_choice_routes_stdio_to_command_and_remote_to_url() {
	let (mut wizard, _, _, _) = wizard();
	wizard.handle_native_event(&select("choices-transport", "0"));
	assert_eq!(wizard.step(), McpWizardStep::Command);
	assert_eq!(wizard.state().transport, Some(Transport::Stdio));

	let (mut wizard, _, _, _) = wizard();
	wizard.handle_native_event(&select("choices-transport", "1"));
	assert_eq!(wizard.step(), McpWizardStep::Url);
	assert_eq!(wizard.state().transport, Some(Transport::Http));

	let (mut wizard, _, _, _) = wizard();
	wizard.handle_native_event(&select("choices-transport", "2"));
	assert_eq!(wizard.step(), McpWizardStep::Url);
	assert_eq!(wizard.state().transport, Some(Transport::Sse));

	// A keypath for another step is ignored.
	let (mut wizard, _, _, _) = wizard();
	wizard.handle_native_event(&select("choices-scope", "1"));
	assert_eq!(wizard.step(), McpWizardStep::Transport);
	assert_eq!(wizard.state().transport, None);
}

#[test]
fn name_validation_error_blocks_advance_and_reaches_the_native_field() {
	let host = FakeHost {
		name_error: Some("duplicate name".to_string()),
		..FakeHost::default()
	};
	let (mut wizard, _, _, _) = wizard_with(host, None);
	wizard.paste_text("dup");
	wizard.handle_native_event(&action("next"));
	assert_eq!(wizard.step(), McpWizardStep::Name);

	let node = wizard.describe_native(&DescribeContext::new(80));
	let field = find_role(&node, "omp.mcpAdd.field").expect("field node");
	assert_eq!(field.props.get_str("error"), Some("duplicate name"));
	assert_eq!(field.props.get_str("tone"), Some("error"));
}

#[test]
fn stdio_probe_clears_auth_and_builds_the_persisted_json() {
	let (mut wizard, _, _, probed) = wizard();
	wizard.handle_native_event(&select("choices-transport", "0"));
	wizard.paste_text("bun run server");
	wizard.handle_native_event(&action("next"));
	assert_eq!(wizard.step(), McpWizardStep::Args);
	wizard.paste_text("--port 1");
	wizard.handle_native_event(&action("next"));

	assert_eq!(wizard.step(), McpWizardStep::Scope);
	let note = wizard.connection_note().expect("connection note");
	assert!(note.ok);
	assert!(note.message.contains("No authentication required"));

	let probed = probed.borrow();
	assert_eq!(probed.len(), 1);
	assert_eq!(probed[0].kind, Transport::Stdio);
	assert_eq!(probed[0].command, "bun run server");
	assert_eq!(probed[0].args, vec!["--port".to_string(), "1".to_string()]);
	assert_eq!(probed[0].timeout, Some(5000));
	assert!(probed[0].env.is_empty());
	assert!(probed[0].headers.is_empty());
	assert!(probed[0].auth.is_none());

	let json = wizard.build_config().to_json();
	assert_eq!(json["type"], "stdio");
	assert_eq!(json["command"], "bun run server");
	assert_eq!(json["args"], serde_json::json!(["--port", "1"]));
	assert!(json.get("url").is_none());
}

#[test]
fn remote_url_is_validated_then_probed_and_serialized() {
	let (mut wizard, _, _, _) = wizard();
	wizard.handle_native_event(&select("choices-transport", "1"));
	wizard.paste_text("ftp://example.com/mcp");
	wizard.handle_native_event(&action("next"));
	assert_eq!(wizard.step(), McpWizardStep::Url);
	let node = wizard.describe_native(&DescribeContext::new(80));
	let field = find_role(&node, "omp.mcpAdd.field").expect("field node");
	assert_eq!(field.props.get_str("error"), Some("URL must use http:// or https:// scheme"));

	let (mut wizard, _, _, probed) = wizard();
	wizard.handle_native_event(&select("choices-transport", "1"));
	wizard.paste_text("https://api.example.com/mcp");
	wizard.handle_native_event(&action("next"));
	assert_eq!(wizard.step(), McpWizardStep::Scope);
	assert_eq!(probed.borrow().len(), 1);
	assert_eq!(probed.borrow()[0].kind, Transport::Http);
	assert_eq!(probed.borrow()[0].url, "https://api.example.com/mcp");

	let json = wizard.build_config().to_json();
	assert_eq!(json["type"], "http");
	assert_eq!(json["url"], "https://api.example.com/mcp");
	assert!(json.get("command").is_none());
}

#[test]
fn native_describe_marks_the_phase_strip_and_lists_transport_choices() {
	let (wizard, _, _, _) = wizard();
	let node = wizard.describe_native(&DescribeContext::new(80));
	assert_eq!(node.props.get_str("role"), Some(MCP_ADD_ROLE));

	let progress = find_role(&node, "omp.mcpAdd.progress").expect("progress row");
	assert_eq!(progress.children.len(), PHASES.len());
	let phases: Vec<&NativeNode> = progress
		.children
		.iter()
		.filter(|child| child.props.get_str("role") == Some("omp.mcpAdd.phase"))
		.collect();
	assert_eq!(phases.len(), PHASES.len());
	assert_eq!(node_text(phases[0]), "Name");
	assert_eq!(phases[0].props.get_str("tone"), Some("success"));
	assert_eq!(phases[1].props.get_str("tone"), Some("accent"));
	assert_eq!(phases[2].props.get_str("tone"), Some("muted"));

	let choices = find_role(&node, "omp.mcpAdd.choices").expect("choices list");
	assert_eq!(choices.props.get_str("selected"), Some("0"));
	assert_eq!(choices.key.as_deref(), Some("choices-transport"));
	assert_eq!(choices.children.len(), Transport::ALL.len());
	let labels: Vec<&str> = choices
		.children
		.iter()
		.map(|item| item.props.get_str("label").expect("label"))
		.collect();
	assert_eq!(labels, vec!["Local command", "Remote server", "Server-sent events"]);

	let head = find_role(&node, "omp.mcpAdd.head").expect("head");
	assert!(!node_text(head).is_empty());
}

#[test]
fn auth_error_without_auth_requirement_still_reaches_the_scope_step() {
	let host = FakeHost {
		test_error: Some(WizardError::new("connection refused")),
		..FakeHost::default()
	};
	let (mut wizard, _, _, probed) = wizard_with(host, Some("github"));
	wizard.handle_native_event(&select("choices-transport", "0"));
	wizard.paste_text("bun run server");
	wizard.handle_native_event(&action("next"));
	wizard.handle_native_event(&action("next"));

	assert_eq!(wizard.step(), McpWizardStep::Scope);
	let note = wizard.connection_note().expect("connection note");
	assert!(!note.ok);
	assert!(note.message.contains("connection refused"));
	assert!(note.message.contains("Adding server anyway"));
	assert_eq!(probed.borrow().len(), 1);
}

#[test]
fn manual_api_key_flow_persists_env_key_and_completes() {
	let host = FakeHost {
		test_error: Some(WizardError::new("401 unauthorized")),
		detection: AuthDetection {
			requires_auth: true,
			auth_type: Some(AuthType::ApiKey),
			..AuthDetection::default()
		},
		..FakeHost::default()
	};
	let (mut wizard, done, _, _) = wizard_with(host, Some("github"));
	wizard.handle_native_event(&select("choices-transport", "0"));
	wizard.paste_text("bun run server");
	wizard.handle_native_event(&action("next"));
	wizard.handle_native_event(&action("next"));
	assert_eq!(wizard.step(), McpWizardStep::ApiKey);
	assert_eq!(wizard.state().command, "bun run server");

	// A stdio server skips the env/header location choice and names the env var.
	wizard.paste_text("secret-key");
	wizard.handle_native_event(&action("next"));
	assert_eq!(wizard.step(), McpWizardStep::EnvVarName);
	wizard.paste_text("MY_KEY");
	wizard.handle_native_event(&action("next"));
	assert_eq!(wizard.step(), McpWizardStep::Scope);
	assert_eq!(wizard.state().auth_location, None);
	wizard.handle_native_event(&select("choices-scope", "1"));
	assert_eq!(wizard.step(), McpWizardStep::Confirm);
	assert_eq!(wizard.state().scope.map(|scope| scope.as_str()), Some("project"));

	wizard.handle_native_event(&action("next"));
	assert_eq!(&*done.borrow(), &["github:project".to_string()]);
	let config = wizard.build_config();
	assert_eq!(config.kind, Transport::Stdio);
	assert_eq!(config.env.get("MY_KEY").map(String::as_str), Some("secret-key"));
	let json = config.to_json();
	assert_eq!(json["env"]["MY_KEY"], "secret-key");
}

#[test]
fn oauth_flow_stores_the_credential_and_health_checks() {
	let host = FakeHost {
		test_error: Some(WizardError::new("401 unauthorized")),
		detection: AuthDetection {
			requires_auth: true,
			auth_type: Some(AuthType::Oauth),
			oauth: Some(McpOAuthEndpoints {
				authorization_url: "https://auth.example.com/authorize".to_string(),
				token_url: "https://auth.example.com/token".to_string(),
				client_id: Some("client-1".to_string()),
				..McpOAuthEndpoints::default()
			}),
			..AuthDetection::default()
		},
		..FakeHost::default()
	};
	let (mut wizard, _, _, probed) = wizard_with(host, Some("github"));
	wizard.handle_native_event(&select("choices-transport", "1"));
	wizard.paste_text("https://api.example.com/mcp");
	wizard.handle_native_event(&action("next"));

	assert_eq!(wizard.step(), McpWizardStep::Scope);
	assert_eq!(wizard.state().auth_method, AuthMethod::Oauth);
	assert_eq!(wizard.state().oauth_credential_id.as_deref(), Some("cred-1"));
	let note = wizard.connection_note().expect("connection note");
	assert!(note.ok);
	assert!(note.message.contains("Health check passed"));
	// The probe ran twice: once unauthenticated, once with the OAuth config.
	assert_eq!(probed.borrow().len(), 2);
	assert!(probed.borrow()[1].auth.is_some());
	assert_eq!(probed.borrow()[1].auth.as_ref().unwrap().credential_id, "cred-1");
}
