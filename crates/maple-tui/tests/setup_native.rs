//! Port of `l/test/setup-native.test.ts`.
//!
//! Deviations:
//! - The reference's fake timers and `advance()` become direct `pump(now_ms)`
//!   calls; the Rust wizard has no repaint loop, so the idle-repaint case
//!   asserts that a scene phase issues no frames instead of counting ticks.
//! - `runStartupSplash` blocks on a real timer, so the splash component is
//!   driven directly.
//! - Probe scenes record their hosts in a thread-local because
//!   `SetupScene::mount` is a plain `fn` pointer with no closure capture.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;

use maple_tui::component::Component;
use maple_tui::keys::Key;
use maple_tui::native::describe::text;
use maple_tui::native::node::{DescribeContext, NativeNode, NodeText, Tone};
use maple_tui::native::state::set_native_rendering;
use maple_tui::setup::scenes::outro::SETUP_OUTRO_MS;
use maple_tui::setup::scenes::splash::SETUP_SPLASH_MS;
use maple_tui::setup::scenes::types::{
	AuthStorage, ComposerPreviewStatusSource, LoginCallbacks, ModelBrowserSource, ModelKind, ModelPerf,
	ModelsSnapshot, OAuthBrowserSessionRequest, Overlay, OverlayOptions, SetupHost, SetupModel,
	SetupNativeEvent, SetupOAuthProvider, SetupScene, SetupSceneController, SetupSceneHost, SetupSceneResult,
	SetupUiHost,
};
use maple_tui::setup::startup_splash::{RunStartupSplashOptions, StartupSplashComponent};
use maple_tui::setup::wizard_overlay::SetupWizardComponent;
use maple_tui::theme::default_theme;
use maple_tui::theme::symbols::SymbolPreset;
use maple_tui::theme::theme_class::Theme;

const SCENE_IDS: [&str; 3] = ["first", "ansi", "last"];
const SCENE_TITLES: [&str; 3] = ["first title", "ansi title", "last title"];

thread_local! {
	static MOUNTED: RefCell<Vec<&'static str>> = RefCell::new(Vec::new());
	static HOSTS: RefCell<Vec<Box<dyn SetupSceneHost>>> = RefCell::new(Vec::new());
}

static NATIVE_LOCK: Mutex<()> = Mutex::new(());

struct ProbeController {
	title: &'static str,
}

impl Component for ProbeController {
	fn render(&mut self, _width: u16) -> Vec<String> {
		Vec::new()
	}
}

impl SetupSceneController for ProbeController {
	fn title(&self) -> &str {
		self.title
	}

	fn describe_scene(&mut self, _cx: &DescribeContext) -> NativeNode {
		text(NodeText::Plain(self.title.to_string()), maple_tui::native::node::Props::new())
	}
}

fn probe_mount<const ID: usize>(host: Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController> {
	MOUNTED.with(|mounted| mounted.borrow_mut().push(SCENE_IDS[ID]));
	HOSTS.with(|hosts| hosts.borrow_mut().push(host));
	Box::new(ProbeController {
		title: SCENE_TITLES[ID],
	})
}

fn probe_scene<const ID: usize>(ansi_only: bool) -> SetupScene {
	SetupScene {
		id: SCENE_IDS[ID],
		title: SCENE_TITLES[ID],
		step: SCENE_IDS[ID],
		min_version: 1,
		ansi_only,
		should_run: None,
		mount: probe_mount::<ID>,
	}
}

struct FakeOverlay;

impl Overlay for FakeOverlay {
	fn hide(&self) {}
}

struct FakeAuth;

impl AuthStorage for FakeAuth {
	fn providers(&self) -> Vec<SetupOAuthProvider> {
		Vec::new()
	}

	fn has_credential(&self, _provider_id: &str) -> bool {
		false
	}

	fn uses_manual_code(&self, _provider_id: &str) -> bool {
		false
	}

	fn login(&mut self, _provider_id: &str, _callbacks: &mut dyn LoginCallbacks) -> Result<(), String> {
		Err("sign-in is not supported by this host".to_string())
	}

	fn refresh(&mut self, _provider_id: &str) {}
}

struct FakeModelSource;

impl ModelBrowserSource for FakeModelSource {
	fn mru_order(&self) -> Vec<String> {
		Vec::new()
	}

	fn model_perf(&self) -> Vec<ModelPerf> {
		Vec::new()
	}
}

struct FakeSetupHost {
	theme: Theme,
	renders: Rc<Cell<usize>>,
	current: Option<SetupModel>,
	auth: FakeAuth,
	models: FakeModelSource,
}

impl SetupUiHost for FakeSetupHost {
	fn cols(&self) -> usize {
		80
	}

	fn rows(&self) -> usize {
		40
	}

	fn now_ms(&self) -> f64 {
		0.0
	}

	fn request_render(&self) {
		self.renders.set(self.renders.get() + 1);
	}

	fn invalidate(&self) {}

	fn set_focus(&self, _component: Option<&dyn Component>) {}

	fn show_overlay(&self, _component: Box<dyn Component>, _options: OverlayOptions) -> Box<dyn Overlay> {
		Box::new(FakeOverlay)
	}
}

impl SetupHost for FakeSetupHost {
	fn theme(&self) -> &Theme {
		&self.theme
	}

	fn status_line(&self) -> Option<&dyn ComposerPreviewStatusSource> {
		None
	}

	fn composer_shape(&self) -> &str {
		"default"
	}

	fn symbol_preset(&self) -> SymbolPreset {
		SymbolPreset::Nerd
	}

	fn color_blind_mode(&self) -> bool {
		false
	}

	fn disabled_providers(&self) -> &[String] {
		&[]
	}

	fn auto_update(&self) -> bool {
		false
	}

	fn auth_storage(&self) -> &dyn AuthStorage {
		&self.auth
	}

	fn model_source(&self) -> &dyn ModelBrowserSource {
		&self.models
	}

	fn get_models(&self) -> ModelsSnapshot {
		ModelsSnapshot {
			current: self.current.clone(),
			..ModelsSnapshot::default()
		}
	}

	fn refresh_models(&self) {}

	fn select_model(&self, _model: &SetupModel, _selector: &str) {}

	fn refresh_provider(&self, _provider: &str) {}

	fn save_composer_shape(&self, _shape: &str) {}

	fn save_symbol_preset(&self, _preset: SymbolPreset) {}

	fn save_color_blind_mode(&self, _enabled: bool) {}

	fn save_theme(&self, _mode: &str, _name: &str) {}

	fn can_auto_update(&self) -> bool {
		false
	}

	fn save_auto_update(&self, _enabled: bool) {}

	fn capture_browser_session(&self, _request: &OAuthBrowserSessionRequest) -> Option<String> {
		None
	}

	fn open_in_browser(&self, _url: &str) {}

	fn mark_complete(&self, _version: u32) {}

	fn play_welcome_intro(&self) {}

	fn show_error(&self, _message: &str) {}

	fn current_theme_name(&self) -> Option<String> {
		Some("dark".to_string())
	}

	fn available_themes(&self) -> Vec<String> {
		vec!["dark".to_string()]
	}

	fn preview_theme(&self, _name: &str) -> Result<(), String> {
		Ok(())
	}

	fn enable_auto_theme(&self) {}

	fn is_light_theme(&self, _name: &str) -> bool {
		false
	}

	fn glyph_protocol(&self) -> bool {
		false
	}
}

fn make_host(current: Option<&str>) -> (Rc<dyn SetupHost>, Rc<Cell<usize>>) {
	let renders = Rc::new(Cell::new(0));
	let model = current.map(|name| SetupModel {
		provider: "provider".to_string(),
		id: "model".to_string(),
		name: name.to_string(),
		kind: ModelKind::Chat,
		priority: None,
	});
	let host = FakeSetupHost {
		theme: default_theme(),
		renders: renders.clone(),
		current: model,
		auth: FakeAuth,
		models: FakeModelSource,
	};
	let host: Rc<dyn SetupHost> = Rc::new(host);
	(host, renders)
}

fn collect(node: &NativeNode, out: &mut Vec<NativeNode>) {
	out.push(node.clone());
	for child in &node.children {
		collect(child, out);
	}
}

fn described(wizard: &mut SetupWizardComponent) -> Vec<NativeNode> {
	let cx = DescribeContext::new(80);
	let root = wizard.describe(&cx);
	let mut out = Vec::new();
	collect(&root, &mut out);
	out
}

fn has_click(nodes: &[NativeNode], action: &str) -> bool {
	nodes.iter().any(|node| {
		node.props
			.get("actions")
			.and_then(|actions| actions.get("click"))
			.and_then(serde_json::Value::as_str)
			== Some(action)
	})
}

fn step_tones(nodes: &[NativeNode]) -> Vec<Tone> {
	nodes
		.iter()
		.filter(|node| node.props.role() == Some("omp.setup.step"))
		.filter_map(|node| node.props.tone())
		.collect()
}

fn action(act: &str) -> SetupNativeEvent {
	SetupNativeEvent::Action {
		key: "0".to_string(),
		act: act.to_string(),
		value: None,
		mods: Vec::new(),
	}
}

fn reset_probe() {
	MOUNTED.with(|mounted| mounted.borrow_mut().clear());
	HOSTS.with(|hosts| hosts.borrow_mut().clear());
}

fn mounted() -> Vec<&'static str> {
	MOUNTED.with(|mounted| mounted.borrow().clone())
}

fn finish_first() {
	HOSTS.with(|hosts| {
		hosts.borrow()[0].finish(SetupSceneResult::Done);
	});
}

fn lock_native() -> std::sync::MutexGuard<'static, ()> {
	NATIVE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn startup_splash_finishes_on_deadline_without_repaint_ticks() {
	let _guard = lock_native();
	set_native_rendering(true);
	let (host, renders) = make_host(None);
	let mut splash = StartupSplashComponent::new(
		host,
		RunStartupSplashOptions {
			duration_ms: Some(120.0),
			tick_ms: Some(5.0),
			..RunStartupSplashOptions::default()
		},
	);
	splash.run();
	assert_eq!(renders.get(), 1);
	assert!(!splash.pump(100.0));
	assert!(!splash.is_done());
	assert_eq!(renders.get(), 1);
	assert!(splash.pump(120.0));
	assert!(splash.is_done());
	assert_eq!(renders.get(), 1);
	splash.dispose();
}

#[test]
fn startup_splash_skip_action_ends_it_early() {
	let _guard = lock_native();
	set_native_rendering(true);
	let (host, _renders) = make_host(None);
	let mut splash = StartupSplashComponent::new(
		host,
		RunStartupSplashOptions {
			duration_ms: Some(10_000.0),
			..RunStartupSplashOptions::default()
		},
	);
	splash.run();
	let cx = DescribeContext::new(80);
	let mut nodes = Vec::new();
	collect(&splash.describe(&cx), &mut nodes);
	assert!(has_click(&nodes, "skip"));
	splash.handle_native_event(&action("skip"));
	assert!(splash.is_done());
	assert!(splash.pump(0.0));
}

#[test]
fn wizard_advances_splash_to_scene_on_deadline_and_describes_mounted_scene() {
	let _guard = lock_native();
	set_native_rendering(true);
	reset_probe();
	let (host, renders) = make_host(None);
	let mut wizard = SetupWizardComponent::new(host, vec![probe_scene::<0>(false)]);
	wizard.run();
	assert_eq!(renders.get(), 1);
	assert!(!wizard.pump(SETUP_SPLASH_MS - 1.0));
	assert!(mounted().is_empty());
	assert!(!wizard.pump(SETUP_SPLASH_MS));
	assert_eq!(mounted(), vec!["first"]);
	assert!(described(&mut wizard)
		.iter()
		.any(|node| node.plain_text().contains("first title")));
	assert_eq!(renders.get(), 2);
	wizard.handle_input(&Key::Ctrl('c'));
	assert!(has_click(&described(&mut wizard), "continue"));
	wizard.handle_native_event(&action("continue"));
	assert!(wizard.is_done());
	wizard.dispose();
}

#[test]
fn wizard_skips_ansi_only_scenes_and_counts_applicable_steps() {
	let _guard = lock_native();
	set_native_rendering(true);
	reset_probe();
	let (host, _renders) = make_host(None);
	let mut wizard = SetupWizardComponent::new(
		host,
		vec![probe_scene::<0>(false), probe_scene::<1>(true), probe_scene::<2>(false)],
	);
	wizard.run();
	wizard.handle_native_event(&action("skip"));
	assert_eq!(step_tones(&described(&mut wizard)), vec![Tone::Accent, Tone::Muted]);
	finish_first();
	assert!(!wizard.pump(0.0));
	assert_eq!(mounted(), vec!["first", "last"]);
	assert_eq!(step_tones(&described(&mut wizard)), vec![Tone::Success, Tone::Accent]);
	wizard.dispose();
}

#[test]
fn wizard_asks_ansi_only_scenes_when_rendering_ansi() {
	let _guard = lock_native();
	set_native_rendering(false);
	reset_probe();
	let (host, _renders) = make_host(None);
	let mut wizard =
		SetupWizardComponent::new(host, vec![probe_scene::<0>(false), probe_scene::<1>(true)]);
	wizard.run();
	wizard.handle_input(&Key::Enter);
	assert!(!wizard.pump(1000.0));
	assert!(wizard
		.render(80)
		.iter()
		.any(|line| line.contains("Setup step 1 of 2")));
	finish_first();
	assert!(!wizard.pump(1000.0));
	assert_eq!(mounted(), vec!["first", "ansi"]);
	wizard.dispose();
}

#[test]
fn idle_scene_does_not_repaint_but_outro_does() {
	let _guard = lock_native();
	set_native_rendering(false);
	reset_probe();
	let (host, renders) = make_host(None);
	let mut wizard = SetupWizardComponent::new(host, vec![probe_scene::<0>(false)]);
	wizard.run();
	assert_eq!(renders.get(), 1);
	wizard.handle_input(&Key::Enter);
	assert_eq!(renders.get(), 2);
	assert!(!wizard.pump(453.0));
	assert_eq!(renders.get(), 3);
	assert!(wizard
		.render(80)
		.iter()
		.any(|line| line.contains("first title")));
	let at_scene = renders.get();
	wizard.pump(1000.0);
	assert_eq!(renders.get(), at_scene);
	wizard.handle_input(&Key::Ctrl('c'));
	assert!(renders.get() > at_scene);
	wizard.dispose();
}

#[test]
fn wizard_completes_without_splash_when_no_scene_applies() {
	let _guard = lock_native();
	set_native_rendering(true);
	reset_probe();
	let (host, renders) = make_host(None);
	let mut wizard = SetupWizardComponent::new(host, vec![probe_scene::<1>(true)]);
	wizard.run();
	assert!(wizard.is_done());
	assert!(mounted().is_empty());
	assert_eq!(renders.get(), 0);
	wizard.dispose();
}

#[test]
fn exit_setup_reaches_outro_reporting_default_model() {
	let _guard = lock_native();
	set_native_rendering(true);
	reset_probe();
	let (host, _renders) = make_host(Some("Opus 4.5"));
	let mut wizard = SetupWizardComponent::new(host, vec![probe_scene::<0>(false)]);
	wizard.run();
	wizard.handle_native_event(&action("skip"));
	assert!(has_click(&described(&mut wizard), "exit"));
	wizard.handle_native_event(&action("exit"));
	let nodes = described(&mut wizard);
	let model = nodes
		.iter()
		.find(|node| node.props.role() == Some("omp.setup.outro.model"))
		.expect("outro model node");
	assert!(model.plain_text().contains("Opus 4.5"));
	assert!(wizard.pump(SETUP_OUTRO_MS));
	assert!(wizard.is_done());
	wizard.dispose();
}
