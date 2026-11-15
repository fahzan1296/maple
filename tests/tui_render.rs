use maple::session::store::{new_entry_id, now_rfc3339, Store};
use maple::session::types::SessionEntry;
use maple::tui::app::App;
use maple::tui::render;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn renderer_draws_header_transcript_input_and_statusline() {
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

    let app = App::new(store, file, "test-model".to_string());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| render::draw(f, &app)).unwrap();

    let buffer = terminal.backend().buffer();
    let mut screen = String::new();
    for y in 0..24 {
        for x in 0..80 {
            screen.push_str(buffer[(x, y)].symbol());
        }
        screen.push('\n');
    }

    for needle in ["maple", "/work/maple", "test-model", "transcript", "input", "hello tui"] {
        assert!(screen.contains(needle), "missing {needle:?} in:\n{screen}");
    }

    let _ = std::fs::remove_dir_all(&home);
}
