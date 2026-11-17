//! Reference: `l/src/setup/scenes/types.ts`.
//!
//! Contracts shared by the setup wizard, its scenes, and the host application.
//!
//! Deviations from the TypeScript original (JS-only shapes, made explicit):
//! - `SetupHost`/`SetupUiHost` are traits, not module singletons; the wizard
//!   receives a `&dyn SetupHost` and scenes receive a `&dyn SetupSceneHost`.
//! - The async host calls (`refreshModels`, `selectModel`, `refreshProvider`,
//!   `saveComposerShape`, `canAutoUpdate`, `saveAutoUpdate`, `markComplete`,
//!   `captureBrowserSession`) are synchronous: the crate has no async runtime,
//!   so the host performs any I/O before returning.
//! - `SetupScene::should_run` is a synchronous `fn` pointer, not `async`.
//! - `performance.now()` becomes [`SetupUiHost::now_ms`].
//! - `NativeUiEvent` (absent from `native/node.rs`) is mirrored as
//!   [`SetupNativeEvent`]; `SgrMouseEvent` and its SGR parser live here because
//!   `mouse.rs` only models decoded crossterm events.

use crate::component::Component;
use crate::keys::Key;
use crate::native::describe::empty_node;
use crate::native::node::{DescribeContext, NativeNode};
use crate::theme::symbols::SymbolPreset;
use crate::theme::theme_class::Theme;

/// Role of the sheet (`picker` or `overlay`) an interactive scene presents in a
/// TSP terminal. Wire bytes; kept verbatim from the reference.
pub const SETUP_SHEET_ROLE: &str = "omp.setup.sheet";

/// Surface role of the setup page (wizard and startup splash) in a TSP terminal.
pub const SETUP_SURFACE_ROLE: &str = "omp.app.setup";

/// One line of status copy kept as raw text plus its theme colour, never as
/// pre-styled ANSI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledLine {
	pub text: String,
	pub color: String,
}

impl StyledLine {
	pub fn new(text: impl Into<String>, color: impl Into<String>) -> Self {
		StyledLine {
			text: text.into(),
			color: color.into(),
		}
	}
}

/// Outcome reported when an onboarding scene finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupSceneResult {
	Done,
	Skipped,
}

impl SetupSceneResult {
	pub fn as_str(self) -> &'static str {
		match self {
			SetupSceneResult::Done => "done",
			SetupSceneResult::Skipped => "skipped",
		}
	}
}

/// A shown overlay; `hide` closes it.
pub trait Overlay {
	fn hide(&self);
}

/// Geometry of a fullscreen setup overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayOptions {
	pub width: String,
	pub max_height: String,
	pub anchor: String,
	pub margin: u16,
	pub fullscreen: bool,
}

impl OverlayOptions {
	/// The setup overlay's fullscreen page geometry.
	pub fn fullscreen() -> Self {
		OverlayOptions {
			width: "100%".to_string(),
			max_height: "100%".to_string(),
			anchor: "top-left".to_string(),
			margin: 0,
			fullscreen: true,
		}
	}
}

/// Terminal capabilities used by setup overlays and the startup splash.
pub trait SetupUiHost {
	/// Terminal columns; the fullscreen wizard paints the whole width.
	fn cols(&self) -> usize;
	/// Terminal rows.
	fn rows(&self) -> usize;
	/// Monotonic clock in milliseconds (the reference's `performance.now()`).
	fn now_ms(&self) -> f64;
	fn request_render(&self);
	fn invalidate(&self);
	fn set_focus(&self, component: Option<&dyn Component>);
	fn show_overlay(
		&self,
		component: Box<dyn Component>,
		options: OverlayOptions,
	) -> Box<dyn Overlay>;
	/// Next pending input, or `None`. The default keeps the wizard idle so a
	/// host that drives the overlay from its own loop need not implement it.
	fn poll_input(&self) -> Option<SetupInput> {
		None
	}
}

/// One pending setup input: a decoded key, a raw SGR mouse report, or a native
/// surface event.
#[derive(Clone, Debug)]
pub enum SetupInput {
	Key(Key),
	Sgr(String),
	Native(SetupNativeEvent),
}

/// Application-owned preferences and effects consumed by setup scenes.
pub trait SetupHost: SetupUiHost {
	fn theme(&self) -> &Theme;
	/// Live status-line source the composer preview borrows rows from.
	fn status_line(&self) -> Option<&dyn ComposerPreviewStatusSource>;
	fn composer_shape(&self) -> &str;
	fn symbol_preset(&self) -> SymbolPreset;
	fn color_blind_mode(&self) -> bool;
	fn disabled_providers(&self) -> &[String];
	/// Whether background self-update (`update.auto`) is on.
	fn auto_update(&self) -> bool;
	fn auth_storage(&self) -> &dyn AuthStorage;
	/// Run a provider login through the host's mutable auth storage. The default
	/// reports an unsupported flow; hosts that can sign in override it.
	fn login_provider(
		&self,
		_provider_id: &str,
		_callbacks: &mut dyn LoginCallbacks,
	) -> Result<(), String> {
		Err("provider sign-in is not supported by this host".to_string())
	}
	fn model_source(&self) -> &dyn ModelBrowserSource;
	/// Available and catalog models of every kind; role chips resolve over all
	/// of them, rows list chat models.
	fn get_models(&self) -> ModelsSnapshot;
	fn refresh_models(&self);
	fn select_model(&self, model: &SetupModel, selector: &str);
	fn refresh_provider(&self, provider: &str);
	fn save_composer_shape(&self, shape: &str);
	fn save_symbol_preset(&self, preset: SymbolPreset);
	/// Apply `preset` live without persisting it (the glyph scene's preview).
	/// The default does nothing for hosts with no live symbol table.
	fn preview_symbol_preset(&self, _preset: SymbolPreset) {}
	fn save_color_blind_mode(&self, enabled: bool);
	fn save_theme(&self, mode: &str, name: &str);
	/// Whether this omp can install its own updates; false where something else
	/// owns them (Tern, Nix, a source checkout).
	fn can_auto_update(&self) -> bool;
	fn save_auto_update(&self, enabled: bool);
	/// Capture the browser half of an OAuth flow; `None` when unsupported.
	fn capture_browser_session(&self, request: &OAuthBrowserSessionRequest) -> Option<String>;
	fn open_in_browser(&self, url: &str);
	fn mark_complete(&self, version: u32);
	fn play_welcome_intro(&self);
	fn show_error(&self, message: &str);
	// ── Theme controller (the reference's module-level theme singleton) ──
	fn current_theme_name(&self) -> Option<String>;
	fn available_themes(&self) -> Vec<String>;
	fn preview_theme(&self, name: &str) -> Result<(), String>;
	fn enable_auto_theme(&self);
	fn is_light_theme(&self, name: &str) -> bool;
	/// Whether the terminal's Glyph Protocol handshake confirmed icon support.
	fn glyph_protocol(&self) -> bool;
}

/// Per-scene focus, rendering, completion, and application callbacks.
pub trait SetupSceneHost {
	fn ctx(&self) -> &dyn SetupHost;
	fn request_render(&self);
	fn finish(&self, result: SetupSceneResult);
	fn set_focus(&self, component: Option<&dyn Component>);
	fn restore_focus(&self);
}

/// The real status renderer the composer preview borrows rows from. The crate
/// has no `StatusLineComponent` port; a host that has one implements this.
pub trait ComposerPreviewStatusSource {
	/// Powerline bar with the context gauge (box top border content).
	fn top_border(&self, width: usize, preview_title: Option<&str>) -> (String, usize);
	/// Flush soft-capped powerline band (band composer top row).
	fn band_top_border(&self, width: usize, preview_title: Option<&str>) -> (String, usize);
	/// Plain right-group chip (claude top rule content).
	fn standalone_top_border(&self, width: usize, preview_title: Option<&str>) -> (String, usize);
	/// Plain standalone bottom bar carrying the given segment groups.
	fn bottom_bar(&self, width: usize, groups: &str, preview_title: Option<&str>) -> String;
}

/// Interactive content hosted inside the setup wizard frame.
pub trait SetupSceneController: Component {
	fn title(&self) -> &str;
	fn subtitle(&self) -> Option<&str> {
		None
	}
	fn on_mount(&mut self) {}
	fn on_unmount(&mut self) {}
	/// Render the scene body. `max_lines` is the number of body rows the wizard
	/// will actually display (header and footer already subtracted).
	fn render_body(&mut self, width: u16, max_lines: Option<usize>) -> Vec<String> {
		let _ = max_lines;
		self.render(width)
	}
	/// Route an SGR mouse report; `line`/`col` are 0-based within this
	/// controller's last rendered output.
	fn route_mouse(&mut self, _event: &SgrMouseEvent, _line: i32, _col: i32) {}
	/// Whether [`route_mouse`](Self::route_mouse) hit-tests events. The wizard
	/// falls back to arrow keys for wheel events when this is `false`.
	fn routes_mouse(&self) -> bool {
		false
	}
	/// Native description of the scene body.
	fn describe_scene(&mut self, _cx: &DescribeContext) -> NativeNode {
		empty_node()
	}
	/// Native picker/overlay event routed back to this scene.
	fn handle_native_event(&mut self, _event: &SetupNativeEvent) {}
}

/// Versioned onboarding scene definition. `Copy` so `ALL_SCENES` and selections
/// can be passed by value.
#[derive(Clone, Copy)]
pub struct SetupScene {
	pub id: &'static str,
	pub title: &'static str,
	/// Short label for the wizard's stepper (`Sign in`, `Model`).
	pub step: &'static str,
	pub min_version: u32,
	/// Configures something only the cell-grid renderer draws (glyphs, composer
	/// shape, theme). A TSP terminal draws those itself, so the wizard skips the
	/// scene while a native surface is live.
	pub ansi_only: bool,
	pub should_run: Option<fn(&dyn SetupHost) -> bool>,
	pub mount: fn(Box<dyn SetupSceneHost>) -> Box<dyn SetupSceneController>,
}

/// A decoded SGR mouse report. Mirrors `l/src/mouse.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SgrMouseEvent {
	/// Raw button code (bit 32 = motion, bit 64 = wheel, low bits = button).
	pub button: u32,
	/// 0-based column of the event.
	pub col: i32,
	/// 0-based row of the event.
	pub row: i32,
	/// True for a release report (`m` suffix).
	pub release: bool,
	/// Vertical wheel direction: -1 up, 1 down, `None` when not a wheel event.
	pub wheel: Option<i8>,
	/// True when the pointer moved (hover or drag) rather than clicked.
	pub motion: bool,
	/// True for a left-button press (not motion, not release, not wheel).
	pub left_click: bool,
}

/// Decode an SGR mouse report, or `None` when `data` is not one.
pub fn parse_sgr_mouse(data: &str) -> Option<SgrMouseEvent> {
	let rest = data.strip_prefix("\x1b[<")?;
	let last = rest.chars().last()?;
	let release = match last {
		'M' => false,
		'm' => true,
		_ => return None,
	};
	let body = &rest[..rest.len() - last.len_utf8()];
	let mut parts = body.split(';');
	let button: u32 = parts.next()?.parse().ok()?;
	let col: i32 = parts.next()?.parse::<i32>().ok()? - 1;
	let row: i32 = parts.next()?.parse::<i32>().ok()? - 1;
	if parts.next().is_some() {
		return None;
	}
	let wheel = if button & 64 != 0 && button & 2 == 0 {
		Some(if button & 1 != 0 { 1 } else { -1 })
	} else {
		None
	};
	let motion = button & 32 != 0 && wheel.is_none();
	let left_click = !release && wheel.is_none() && !motion && button & 3 == 0;
	Some(SgrMouseEvent {
		button,
		col,
		row,
		release,
		wheel,
		motion,
		left_click,
	})
}

/// Decode an SGR mouse report and forward it to `handler`. Returns `false`
/// when `data` is not an SGR mouse report, so callers can fall through.
pub fn route_sgr_mouse_input(data: &str, handler: &mut dyn FnMut(&SgrMouseEvent)) -> bool {
	match parse_sgr_mouse(data) {
		Some(event) => {
			handler(&event);
			true
		}
		None => false,
	}
}

/// The subset of `NativeUiEvent` the setup scenes consume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupNativeEvent {
	/// A node action; `act` is the action id, `value` its scope/sort/row id.
	Action {
		key: String,
		act: String,
		value: Option<String>,
		mods: Vec<String>,
	},
	/// A row was highlighted.
	Select { key: String, item: String },
	/// A row was activated.
	Activate { key: String, item: String },
}

impl SetupNativeEvent {
	/// The picker event carried by `event` for `key`, if it is one.
	pub fn picker(&self, key: &str) -> Option<SetupPickerEvent> {
		match self {
			SetupNativeEvent::Select { key: event_key, item } if event_key == key => {
				Some(SetupPickerEvent::Select(item.clone()))
			}
			SetupNativeEvent::Activate { key: event_key, item } if event_key == key => {
				Some(SetupPickerEvent::Activate(item.clone()))
			}
			SetupNativeEvent::Action { key: event_key, act, value, .. } if event_key == key => {
				Some(SetupPickerEvent::Action {
					act: act.clone(),
					value: value.clone(),
				})
			}
			_ => None,
		}
	}
}

/// A picker event for one key path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupPickerEvent {
	Select(String),
	Activate(String),
	Action { act: String, value: Option<String> },
}

/// A model catalog entry as the setup scenes see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupModel {
	pub provider: String,
	pub id: String,
	pub name: String,
	pub kind: ModelKind,
	/// Provider priority (lower is better); `None` when the provider has none.
	pub priority: Option<i64>,
}

/// Model kind; the setup scenes list `Chat` models and resolve roles over all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
	Chat,
	Reasoning,
	Embedding,
	Other,
}

/// The `modelKind` classifier. The crate has no `pi-catalog` port, so the host
/// supplies the kind through [`SetupModel::kind`]; this helper keeps the
/// reference's call shape for scenes that re-derive it.
pub fn model_kind(model: &SetupModel) -> ModelKind {
	model.kind
}

/// Available and catalog models plus the current selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelsSnapshot {
	pub available: Vec<SetupModel>,
	pub all: Vec<SetupModel>,
	pub current: Option<SetupModel>,
}

/// One selector's measured performance, used by the browser's fact columns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelPerf {
	pub selector: String,
	pub tokens_per_second: Option<f64>,
	pub latency_ms: Option<f64>,
}

/// Host-provided preferences and model-role resolution for the browser.
pub trait ModelBrowserSource {
	fn mru_order(&self) -> Vec<String>;
	fn model_perf(&self) -> Vec<ModelPerf>;
}

/// A resolved role assignment as the browser displays it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleAssignment {
	pub selector: String,
	pub auto_selected: bool,
}

/// Map of role id to its resolved assignment (absent roles are unresolved).
pub type RoleAssignments = std::collections::BTreeMap<String, Option<RoleAssignment>>;

/// One selectable browser row.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelBrowserItem {
	pub provider: String,
	pub id: String,
	pub model: SetupModel,
	pub selector: String,
}

/// An OAuth provider as the setup scene sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupOAuthProvider {
	pub id: String,
	pub name: String,
	/// Provider id the credentials are stored under, when it differs.
	pub store_credentials_as: Option<String>,
}

/// Credential presence and provenance needed by the provider picker.
pub trait AuthStorage {
	fn providers(&self) -> Vec<SetupOAuthProvider>;
	/// Whether a stored credential exists for `provider_id`.
	fn has_credential(&self, provider_id: &str) -> bool;
	/// Whether the provider's flow asks the user to paste a code manually.
	fn uses_manual_code(&self, provider_id: &str) -> bool;
	/// Run the provider login; callbacks drive the dialog.
	fn login(&mut self, provider_id: &str, callbacks: &mut dyn LoginCallbacks) -> Result<(), String>;
	/// Re-run discovery for a provider after a credential changed.
	fn refresh(&mut self, provider_id: &str);
}

/// Live callbacks the login flow uses to update the dialog.
pub trait LoginCallbacks {
	fn show_auth(&mut self, url: &str, instructions: Option<&str>, launch_url: Option<&str>);
	fn show_prompt(&mut self, prompt: &str);
	fn show_progress(&mut self, message: &str);
	/// Ask for a pasted code; `None` cancels.
	fn show_manual_input(&mut self, prompt: &str) -> Option<String>;
}

/// The browser-session request the host must capture. The crate has no
/// `pi-ai/oauth` port; the host fills the URL/instructions it received.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OAuthBrowserSessionRequest {
	pub url: String,
	pub instructions: Option<String>,
}

/// The agent credential database path (`getAgentDbPath`). The crate has no
/// `pi-utils` port; sign-in reads this free function, overridable through
/// `MAPLE_TUI_AGENT_DB`.
pub fn agent_db_path() -> String {
	std::env::var("MAPLE_TUI_AGENT_DB")
		.unwrap_or_else(|_| "~/.maple/agent.db".to_string())
}


#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_sgr_mouse_press_and_wheel() {
		let press = parse_sgr_mouse("\x1b[<0;12;7M").expect("press");
		assert_eq!(press.col, 11);
		assert_eq!(press.row, 6);
		assert!(press.left_click);
		assert_eq!(press.wheel, None);

		let wheel = parse_sgr_mouse("\x1b[<64;3;4M").expect("wheel up");
		assert_eq!(wheel.wheel, Some(-1));
		assert!(!wheel.left_click);

		let release = parse_sgr_mouse("\x1b[<0;1;1m").expect("release");
		assert!(release.release);
		assert!(!release.left_click);
	}

	#[test]
	fn rejects_non_sgr_input() {
		assert!(parse_sgr_mouse("\x1b[A").is_none());
		assert!(parse_sgr_mouse("enter").is_none());
		assert!(!route_sgr_mouse_input("\x1b[A", &mut |_| {}));
	}

	#[test]
	fn picker_event_only_matches_its_key() {
		let event = SetupNativeEvent::Action {
			key: "picker".to_string(),
			act: "save".to_string(),
			value: None,
			mods: Vec::new(),
		};
		assert_eq!(
			event.picker("picker"),
			Some(SetupPickerEvent::Action {
				act: "save".to_string(),
				value: None
			})
		);
		assert_eq!(event.picker("other"), None);
	}

	#[test]
	fn scene_result_names() {
		assert_eq!(SetupSceneResult::Done.as_str(), "done");
		assert_eq!(SetupSceneResult::Skipped.as_str(), "skipped");
	}
}
