//! omp-parity surface tests: hub chrome, roster rows, tree rails, picker, roster scan.

use maple::session::store::{new_entry_id, now_rfc3339, Store};
use maple_tui::text_shape::visible_width;

use maple::session::types::SessionEntry;
use maple::tui::app::App;
use maple::tui::app::AskForm;
use maple::tui::hub::row;
use maple::tui::hub::frame::HubFrame;
use maple::tui::hub::roster::{self, Peer, Status};
use maple::tui::picker::Picker;
use maple_tui::Key;
use maple::tui::render;

/// Strip SGR escape sequences so assertions read rendered text.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn peer(id: &str, title: Option<&str>, parent: Option<&str>) -> Peer {
    Peer {
        session_id: id.to_string(),
        title: title.map(str::to_string),
        path: std::path::PathBuf::from("/tmp/maple-peer.jsonl"),
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

#[test]
fn hub_frame_draws_split_borders_and_chips() {
    let frame = HubFrame::new("Agent Hub", 30, 46);
    let top = plain(&frame.top(100, 32));
    let divider = plain(&frame.divider(100, 32));
    let bottom = plain(&frame.bottom(100, 32));

    for needle in ["╭", "┬", "╮", "Agent Hub"] {
        assert!(top.contains(needle), "top border missing {needle:?}: {top:?}");
    }
    for needle in ["├", "┴", "┤"] {
        assert!(divider.contains(needle), "divider missing {needle:?}: {divider:?}");
    }
    for needle in ["╰", "╯"] {
        assert!(bottom.contains(needle), "bottom missing {needle:?}: {bottom:?}");
    }

    let chips = plain(&frame.chips(60, &["Agents", "Activity"], 0));
    assert!(chips.contains("[ Agents ]"), "chips: {chips:?}");
    assert!(chips.contains("Activity"), "chips: {chips:?}");
}

#[test]
fn entry_rows_render_identity_task_metrics_and_compact_variant() {
    let sample = peer("01a1de64-91f0-75a2", Some("hub ui"), None);
    let rows = row::entry_rows(&sample, true, 100, None);
    assert_eq!(rows.len(), 3, "one identity, one task, one metrics row");
    let flat: Vec<String> = rows.iter().map(|row| plain(row)).collect();
    let joined = flat.join("\n");
    assert!(flat[0].contains('❯'), "selected cursor: {}", flat[0]);
    assert!(flat[0].contains("01a1de64"), "identity: {}", flat[0]);
    assert!(joined.contains("ship the hub frame"), "task: {joined}");
    assert!(joined.contains("$0.500"), "cost: {joined}");
    assert!(joined.contains("1.2K tok"), "tokens: {joined}");

    let narrow = row::entry_rows(&sample, false, 40, None)
        .iter()
        .map(|row| plain(row))
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

    picker.handle_key(Key::Char('b'));
    picker.handle_key(Key::Char('e'));
    picker.handle_key(Key::Char('t'));
    assert_eq!(picker.filtered().len(), 1, "query 'bet' matches only beta");

    let chosen = picker.handle_key(Key::Enter);
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
            &SessionEntry::new(
                "message",
                new_entry_id(),
                None,
                now_rfc3339(),
                serde_json::json!({ "text": "do the thing" }),
            ),
        )
        .unwrap();
    store
        .append(
            &file.path,
            &SessionEntry::new(
                "tool",
                new_entry_id(),
                None,
                now_rfc3339(),
                serde_json::json!({ "name": "bash" }),
            ),
        )
        .unwrap();
    store
        .append(
            &file.path,
            &SessionEntry::new(
                "usage",
                new_entry_id(),
                None,
                now_rfc3339(),
                serde_json::json!({ "cost": 0.25, "tokens": 100 }),
            ),
        )
        .unwrap();

    let scanned = roster::scan(&store);
    let peers = scanned.peers;
    let activity = scanned.activity;
    let found = peers
        .iter()
        .find(|peer| peer.session_id == id)
        .expect("session in roster");
    assert_eq!(found.requests, 1);
    assert_eq!(found.tools, 1);
    assert_eq!(found.tokens, 100);
    assert!((found.cost - 0.25).abs() < 1e-9);
    assert_eq!(found.task, "do the thing");
    assert_eq!(found.tool, "bash");
    assert_eq!(found.path, file.path);
    let feed = activity
        .iter()
        .find(|item| item.session_id == id)
        .expect("entry in activity feed");
    assert_eq!(feed.kind, "usage");
    assert!(feed.text.contains("100 tok"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn ask_appends_message_to_peer_transcript() {
    let mut home = std::env::temp_dir();
    home.push(format!("maple-ask-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).unwrap();
    let store = Store::with_home(home.clone(), "/work/maple");
    let peer_file = store.create(Some("peer session")).unwrap();
    let me = store.create(Some("my session")).unwrap();
    store
        .append(
            &peer_file.path,
            &SessionEntry::new(
                "message",
                new_entry_id(),
                None,
                now_rfc3339(),
                serde_json::json!({ "text": "peer says hi" }),
            ),
        )
        .unwrap();

    let my_id = me.header.id.clone();
    let mut app = App::new(store, me, "test-model".to_string());
    app.toggle_hub();
    let index = app
        .hub
        .roster
        .iter()
        .position(|peer| peer.session_id == peer_file.header.id)
        .expect("peer in roster");
    for _ in 0..index {
        app.handle_key(Key::Char('j'));
    }
    app.handle_key(Key::Char('a'));
    assert!(app.hub.ask.is_some(), "ask form opens");
    for c in "hello peer".chars() {
        app.handle_key(Key::Char(c));
    }
    app.handle_key(Key::Enter);
    assert!(app.hub.ask.is_none(), "ask form closes on send");

    let store = Store::with_home(home.clone(), "/work/maple");
    let (_, entries) = store.load(&peer_file.path).unwrap();
    let sent = entries
        .iter()
        .find(|entry| entry.payload.get("text").and_then(|v| v.as_str()) == Some("hello peer"))
        .expect("message delivered to peer transcript");
    assert_eq!(
        sent.payload.get("from").and_then(|v| v.as_str()),
        Some(my_id.as_str())
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn activity_tab_lists_recent_entries() {
    let mut home = std::env::temp_dir();
    home.push(format!("maple-activity-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).unwrap();
    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(Some("activity session")).unwrap();
    store
        .append(
            &file.path,
            &SessionEntry::new(
                "message",
                new_entry_id(),
                None,
                now_rfc3339(),
                serde_json::json!({ "text": "recent work item" }),
            ),
        )
        .unwrap();

    let mut app = App::new(store, file, "test-model".to_string());
    app.toggle_hub();
    assert!(
        !app.hub.activity.is_empty(),
        "activity feed populated from the scan"
    );
    app.handle_key(Key::Tab);
    assert_eq!(app.hub.tab, maple::tui::nav::Tab::Activity);

    let screen: String = maple::tui::render::render_rows(&mut app, 120, 30)
        .iter()
        .map(|row| plain(row))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(screen.contains("recent work item"), "feed row missing:\n{screen}");
    assert!(screen.contains("[ Activity ]"), "chips missing:\n{screen}");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn overlays_fill_the_frame_without_overflowing_width() {
    let mut home = std::env::temp_dir();
    home.push(format!("maple-overlays-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).unwrap();
    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(Some("overlay session")).unwrap();

    let mut app = App::new(store, file, "test-model".to_string());
    app.picker.open(&app_store(&home));
    let rows = render::render_rows(&mut app, 60, 18);
    assert_eq!(rows.len(), 18, "picker fills the frame height");
    for row in &rows {
        assert!(
            visible_width(row) <= 60,
            "picker row overflows: {:?}",
            plain(row)
        );
    }
    app.picker.close();

    app.toggle_hub();
    app.hub.ask = Some(AskForm {
        peer_id: "peer-1".to_string(),
        peer_title: "peer one".to_string(),
        buffer: "hello".to_string(),
    });
    let rows = render::render_rows(&mut app, 60, 18);
    assert_eq!(rows.len(), 18, "ask fills the frame height");
    for row in &rows {
        assert!(
            visible_width(row) <= 60,
            "ask row overflows: {:?}",
            plain(row)
        );
    }
    let screen = rows.iter().map(|row| plain(row)).collect::<Vec<_>>().join("\n");
    assert!(screen.contains("Ask"), "ask title missing:\n{screen}");
    assert!(screen.contains("peer one"), "ask peer missing:\n{screen}");

    let _ = std::fs::remove_dir_all(&home);
}

fn app_store(home: &std::path::Path) -> Store {
    Store::with_home(home.to_path_buf(), "/work/maple")
}
