//! omp-parity surface tests: hub chrome, roster rows, tree rails, picker, roster scan.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use maple::session::store::Store;
use maple::tui::app::App;
use maple::tui::hub::frame::HubFrame;
use maple::tui::hub::row;
use maple::tui::hub::roster::{self, Peer, Status};
use maple::tui::picker::Picker;
use ratatui::text::Line;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn peer(id: &str, title: Option<&str>, parent: Option<&str>) -> Peer {
    Peer {
        session_id: id.to_string(),
        title: title.map(str::to_string),
        cwd: "/work/maple".to_string(),
        model: "test-model".to_string(),
        status: Status::Active,
        task: "ship the hub frame".to_string(),
        tool: "bash".to_string(),
        cost: 0.5,
        duration_ms: 1_500,
        requests: 7,
        tools: 3,
        tokens: 1_200,
        parent: parent.map(str::to_string),
        registered_at: "2026-11-15T08:00:00Z".to_string(),
        age_ms: 120_000,
    }
}

fn concat(line: &Line<'_>) -> String {
    line.spans.iter().map(|span| span.content.as_ref()).collect()
}

#[test]
fn hub_frame_draws_split_borders_and_chips() {
    let frame = HubFrame::new("Agent Hub", 30, 46);
    let top = concat(&frame.top(100, 32));
    let divider = concat(&frame.divider(100, 32));
    let bottom = concat(&frame.bottom(100, 32));

    for needle in ["╭", "┬", "╮", "Agent Hub"] {
        assert!(top.contains(needle), "top border missing {needle:?}: {top:?}");
    }
    for needle in ["├", "┴", "┤"] {
        assert!(divider.contains(needle), "divider missing {needle:?}: {divider:?}");
    }
    for needle in ["╰", "╯"] {
        assert!(bottom.contains(needle), "bottom missing {needle:?}: {bottom:?}");
    }

    let chips = concat(&frame.chips(60, &["Agents", "Activity"], 0));
    assert!(chips.contains("[ Agents ]"), "chips: {chips:?}");
    assert!(chips.contains("Activity"), "chips: {chips:?}");
}

#[test]
fn entry_rows_render_identity_task_metrics_and_compact_variant() {
    let sample = peer("01a1de64-91f0-75a2", Some("hub ui"), None);
    let rows = row::entry_rows(&sample, true, 100, None);
    assert_eq!(rows.len(), 3, "one identity, one task, one metrics row");
    let flat: Vec<String> = rows.iter().map(concat).collect();
    let joined = flat.join("\n");
    assert!(flat[0].contains('❯'), "selected cursor: {}", flat[0]);
    assert!(flat[0].contains("01a1de64"), "identity: {}", flat[0]);
    assert!(joined.contains("ship the hub frame"), "task: {joined}");
    assert!(joined.contains("$0.500"), "cost: {joined}");
    assert!(joined.contains("1.2K tok"), "tokens: {joined}");

    let narrow = row::entry_rows(&sample, false, 40, None)
        .iter()
        .map(concat)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(narrow.contains(" · "), "compact metrics use dot separators: {narrow}");
}

#[test]
fn tree_branches_prefix_children_with_rails() {
    let peers = vec![
        peer("root", Some("root"), None),
        peer("a", Some("A"), Some("root")),
        peer("b", Some("B"), Some("root")),
        peer("c", Some("C"), Some("a")),
    ];
    let branches = row::tree_branches(&peers);
    assert_eq!(branches["root"], "  ");
    assert_eq!(branches["a"], "├─");
    assert_eq!(branches["b"], "└─");
    assert_eq!(branches["c"], "│ └─");
}

#[test]
fn age_labels_scale_with_magnitude() {
    assert_eq!(row::age_label(30_000), "30s");
    assert_eq!(row::age_label(120_000), "2m");
    assert_eq!(row::age_label(7_200_000), "2h");
    assert_eq!(row::age_label(90_000_000), "1d");
}

#[test]
fn picker_filters_and_selects_newest_match() {
    let mut home = std::env::temp_dir();
    home.push(format!("maple-picker-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).unwrap();
    let store = Store::with_home(home.clone(), "/work/maple");
    store.create(Some("alpha session")).unwrap();
    store.create(Some("beta session")).unwrap();

    let mut picker = Picker::new();
    picker.open(&store);
    assert_eq!(picker.filtered().len(), 2);

    picker.handle_key(key(KeyCode::Char('b')));
    picker.handle_key(key(KeyCode::Char('e')));
    picker.handle_key(key(KeyCode::Char('t')));
    assert_eq!(picker.filtered().len(), 1, "query 'bet' matches only beta");

    let chosen = picker.handle_key(key(KeyCode::Enter));
    assert!(chosen.is_some(), "Enter returns the selected session path");
    assert!(!picker.visible, "selection closes the picker");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn roster_scan_aggregates_usage_task_and_tool() {
    let mut home = std::env::temp_dir();
    home.push(format!("maple-roster-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).unwrap();
    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(Some("roster check")).unwrap();
    let id = file.header.id.clone();

    store
        .append(
            &file.path,
            &maple::session::types::SessionEntry::new(
                "message",
                maple::session::store::new_entry_id(),
                None,
                maple::session::store::now_rfc3339(),
                serde_json::json!({ "text": "do the thing" }),
            ),
        )
        .unwrap();
    store
        .append(
            &file.path,
            &maple::session::types::SessionEntry::new(
                "tool",
                maple::session::store::new_entry_id(),
                None,
                maple::session::store::now_rfc3339(),
                serde_json::json!({ "name": "bash" }),
            ),
        )
        .unwrap();
    store
        .append(
            &file.path,
            &maple::session::types::SessionEntry::new(
                "usage",
                maple::session::store::new_entry_id(),
                None,
                maple::session::store::now_rfc3339(),
                serde_json::json!({ "cost": 0.25, "tokens": 100 }),
            ),
        )
        .unwrap();

    let peers = roster::scan(&store);
    let found = peers.iter().find(|peer| peer.session_id == id).expect("session in roster");
    assert_eq!(found.requests, 1);
    assert_eq!(found.tools, 1);
    assert_eq!(found.tokens, 100);
    assert!((found.cost - 0.25).abs() < 1e-9);
    assert_eq!(found.task, "do the thing");
    assert_eq!(found.tool, "bash");

    let _ = std::fs::remove_dir_all(&home);
}
