//! Port of `l/test/shell-trace.test.ts` (portable subset).
//!
//! Divergence: TS exercised shell visualizer rendering (`renderShellVisualization`,
//! `ShellVisualizer`), presentation fixtures, split panes, and ANSI layout.
//! Rust exposes shell-viz model/controller helpers (`default_shell_viz_preferences`,
//! `change_shell_viz_preferences`, `shell_viz_event_action`, `SHELL_VIZ_VIEWS`,
//! `SHELL_VIZ_LAYERS`, `is_shell_viz_node`) but not the same render/inspector
//! surface; this subset ports the control/event semantics that are available.

use maple_tui::tools::shell_viz::{
	change_shell_viz_preferences, default_shell_viz_preferences, is_shell_viz_node, shell_viz_event_action,
	ShellVizUiEvent, ShellVizUiValue, ShellVizView, SHELL_VIZ_LAYERS, SHELL_VIZ_VIEWS,
};

#[test]
fn view_layer_constants_and_node_key_validation() {
	assert_eq!(SHELL_VIZ_VIEWS, [ShellVizView::Raw, ShellVizView::Pseudo]);
	assert_eq!(SHELL_VIZ_LAYERS, ["color", "inline", "idioms", "effects", "unwrap"]);
	assert!(is_shell_viz_node("0:0:1"));
	assert!(is_shell_viz_node("10:2:30"));
	assert!(!is_shell_viz_node("0:0"));
	assert!(!is_shell_viz_node("00:1:2"));
	assert!(!is_shell_viz_node("a:1:2"));
}

#[test]
fn toggles_views_layers_and_preserves_selection_cursor_semantics() {
	let mut prefs = default_shell_viz_preferences();
	prefs.cursor = Some(3.0);
	prefs = change_shell_viz_preferences(&prefs, "select:0:0:2", 4);

	for view in ["raw", "pseudo"] {
		let next = change_shell_viz_preferences(&prefs, &format!("view:{view}"), 4);
		assert_eq!(next.cursor, prefs.cursor);
		assert_eq!(next.selected_node.as_ref().map(|n| n.as_str()), prefs.selected_node.as_ref().map(|n| n.as_str()));
		assert!(matches!(next.view, ShellVizView::Raw | ShellVizView::Pseudo));
		prefs.view = next.view;
	}

	let start = prefs.clone();
	for layer in SHELL_VIZ_LAYERS {
		let next = change_shell_viz_preferences(&start, &format!("layer:{layer}"), 4);
		match layer {
			"color" => assert_ne!(next.layers.color, start.layers.color),
			"inline" => assert_ne!(next.layers.inline, start.layers.inline),
			"idioms" => assert_ne!(next.layers.idioms, start.layers.idioms),
			"effects" => assert_ne!(next.layers.effects, start.layers.effects),
			"unwrap" => assert_ne!(next.layers.unwrap, start.layers.unwrap),
			_ => {}
		}
	}
}

#[test]
fn clamps_seek_rejects_invalid_numeric_and_handles_select_expand_playback() {
	let initial = default_shell_viz_preferences();
	assert_eq!(change_shell_viz_preferences(&initial, "seek:999", 4).cursor, Some(4.0));
	assert_eq!(change_shell_viz_preferences(&initial, "select:1.5", 4), initial);
	assert_eq!(change_shell_viz_preferences(&initial, "select:2", 4), initial);
	let selected = change_shell_viz_preferences(&initial, "select:0:0:2", 4);
	assert_eq!(selected.selected_node.as_ref().map(|n| n.as_str()), Some("0:0:2"));
	let expanded = change_shell_viz_preferences(&initial, "expand:0:0:2", 4);
	assert_eq!(expanded.expanded_nodes.len(), 1);
	let collapsed = change_shell_viz_preferences(&expanded, "expand:0:0:2", 4);
	assert!(collapsed.expanded_nodes.is_empty());

	assert_eq!(change_shell_viz_preferences(&initial, "speed:3", 4), initial);
	let playing = change_shell_viz_preferences(&initial, "play", 4);
	assert!(playing.playing);
	assert_eq!(playing.cursor, Some(0.0));
	let reset = change_shell_viz_preferences(&initial, "reset", 4);
	assert!(!reset.playing);
	assert_eq!(reset.cursor, Some(0.0));
	let stepped = change_shell_viz_preferences(&reset, "step", 4);
	assert_eq!(stepped.cursor, Some(1.0));
	let live = change_shell_viz_preferences(&change_shell_viz_preferences(&initial, "seek:2", 4), "live", 4);
	assert_eq!(live.cursor, None);
}

#[test]
fn normalizes_native_events_to_action_vocabulary() {
	let prefs = default_shell_viz_preferences();
	assert_eq!(
		shell_viz_event_action(&ShellVizUiEvent::Action { act: "shell-viz".into(), value: "view:raw".into() }, &prefs),
		Some("view:raw".into())
	);
	assert_eq!(
		shell_viz_event_action(
			&ShellVizUiEvent::Change { item: "view".into(), value: ShellVizUiValue::Text("pseudo".into()) },
			&prefs
		),
		Some("view:pseudo".into())
	);
	assert_eq!(
		shell_viz_event_action(
			&ShellVizUiEvent::Change { item: "layer:color".into(), value: ShellVizUiValue::Bool(true) },
			&prefs
		),
		None
	);
	assert_eq!(
		shell_viz_event_action(
			&ShellVizUiEvent::Change { item: "layer:color".into(), value: ShellVizUiValue::Bool(false) },
			&prefs
		),
		Some("layer:color".into())
	);
	assert_eq!(
		shell_viz_event_action(&ShellVizUiEvent::Select { item: "0:0:2".into() }, &prefs),
		Some("select:0:0:2".into())
	);
	assert_eq!(
		shell_viz_event_action(
			&ShellVizUiEvent::Change { item: "select".into(), value: ShellVizUiValue::Text("0:0:2".into()) },
			&prefs
		),
		Some("select:0:0:2".into())
	);
}
