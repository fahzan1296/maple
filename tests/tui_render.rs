use maple::session::store::{new_entry_id, now_rfc3339, Store};
use maple::session::types::SessionEntry;
use maple::tui::app::App;
use maple::tui::render;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn renderer_draws_transcript_composer_and_statusline() {
    let mut home = std::env::temp_dir();
    home.push(format!("maple-tui-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).unwrap();

    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(Some("render check")).unwrap();
    store
        .append(
            &file.path,
            &SessionEntry::new(
                "message",
                new_entry_id(),
                None,
                now_rfc3339(),
                serde_json::json!({ "text": "hello tui" }),
            ),
        )
        .unwrap();

    let mut app = App::new(store, file, "test-model".to_string());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| render::draw(f, &mut app)).unwrap();

    let screen = screen_of(&terminal, 80, 24);
    for needle in ["maple ·", "hello tui", "message", "type a message…", "test-model"] {
        assert!(screen.contains(needle), "missing {needle:?} in:\n{screen}");
    }
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn hub_panel_lists_peers_and_inspector() {
    let mut home = std::env::temp_dir();
    home.push(format!("maple-hub-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&home).unwrap();

    let store = Store::with_home(home.clone(), "/work/maple");
    let file = store.create(Some("hub ui")).unwrap();
    store
        .append(
            &file.path,
            &SessionEntry::new(
                "usage",
                new_entry_id(),
                None,
                now_rfc3339(),
                serde_json::json!({ "cost": 0.5, "tokens": 42, "tool": "bash" }),
            ),
        )
        .unwrap();

    let mut app = App::new(store, file, "test-model".to_string());
    app.toggle_hub();
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|f| render::draw(f, &mut app)).unwrap();

    let screen = screen_of(&terminal, 120, 24);
    for needle in [
        "Agent Hub",
        "model",
        "test-model",
        "bash",
        "$0.500",
        "Task",
        "Usage",
        "Lineage",
        "[ Agents ]",
        "Activity",
    ] {
        assert!(screen.contains(needle), "missing {needle:?} in:\n{screen}");
    }

    let _ = std::fs::remove_dir_all(&home);
}

fn screen_of(terminal: &Terminal<TestBackend>, width: u16, height: u16) -> String {
    let buffer = terminal.backend().buffer();
    let mut screen = String::new();
    for y in 0..height {
        for x in 0..width {
            screen.push_str(buffer[(x, y)].symbol());
        }
        screen.push('\n');
    }
    screen
}
