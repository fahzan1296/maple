//! Debug socket (port of `l/src/debug-server.ts`).
//!
//! `OMP_TUI_DEBUG=<socket path>` starts a line-delimited NDJSON server. Ops:
//! `text`, `info`, `values`, `keys`, `quit`. Unknown ops return
//! `{ok:false,error}`; malformed JSON never kills the server.

use crate::keys::Key;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct DebugState {
    /// Last painted rows.
    pub lines: Vec<String>,
    pub window_top: u16,
    pub alt_screen: bool,
    pub cursor: Option<(u16, u16)>,
    pub columns: u16,
    pub rows: u16,
    pub overlays: usize,
    pub focused_kind: String,
}

pub struct DebugServer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl DebugServer {
    /// Bind `path` and serve until `stop` is called or the path is unusable.
    pub fn start(
        path: impl AsRef<Path>,
        state: Arc<Mutex<DebugState>>,
        quit: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_path = path.clone();
        let handle = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let state = Arc::clone(&state);
                        let quit = Arc::clone(&quit);
                        let _ = handle_client(stream, state, quit);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
            let _ = std::fs::remove_file(&worker_path);
        });
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DebugServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_client(
    stream: UnixStream,
    state: Arc<Mutex<DebugState>>,
    quit: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => dispatch(&request, &state, &quit),
            Err(error) => json!({"ok": false, "error": error.to_string()}),
        };
        writer.write_all(response.to_string().as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
}

fn dispatch(request: &Value, state: &Arc<Mutex<DebugState>>, quit: &Arc<AtomicBool>) -> Value {
    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    match op {
        "text" => {
            let state = state.lock().unwrap();
            json!({
                "ok": true,
                "lines": state.lines,
                "window_top": state.window_top,
                "alt_screen": state.alt_screen,
                "cursor": state.cursor,
            })
        }
        "info" => {
            let state = state.lock().unwrap();
            json!({
                "ok": true,
                "columns": state.columns,
                "rows": state.rows,
                "pid": std::process::id(),
                "overlays": state.overlays,
                "focused_kind": state.focused_kind,
                "alt_screen": state.alt_screen,
            })
        }
        "values" => json!({"ok": true, "values": {}}),
        "keys" => {
            let token = request
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match parse_key_token(token) {
                Some(key) => json!({"ok": true, "key": key}),
                None => json!({"ok": false, "error": format!("Unknown key token: {token}")}),
            }
        }
        "quit" => {
            quit.store(true, Ordering::Relaxed);
            json!({"ok": true})
        }
        other => json!({"ok": false, "error": format!("Unknown op: {other}")}),
    }
}

fn parse_key_token(token: &str) -> Option<String> {
    let mut chars = token.chars();
    let first = chars.next()?;
    let key = match first {
        'C' => Key::Ctrl(chars.next()?),
        'A' => Key::Alt(chars.next()?),
        'M' => Key::Alt(chars.next()?),
        'S' => Key::Shift(chars.next()?),
        'a'..='z' | 'A'..='Z' | '0'..='9' | ' ' => Key::Char(first),
        '\r' => Key::Enter,
        '\t' => Key::Tab,
        '\x1b' => Key::Esc,
        '\x7f' => Key::Backspace,
        _ => return None,
    };
    Some(format!("{key:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn serves_text_and_info_requests() {
        let path = std::env::temp_dir().join(format!("maple-debug-{}.sock", std::process::id()));
        let state = Arc::new(Mutex::new(DebugState {
            lines: vec!["hello".into()],
            columns: 80,
            rows: 24,
            ..DebugState::default()
        }));
        let quit = Arc::new(AtomicBool::new(false));
        let mut server = DebugServer::start(&path, Arc::clone(&state), Arc::clone(&quit)).unwrap();
        thread::sleep(Duration::from_millis(20));
        let response = request(&path, r#"{"op":"text"}"#);
        assert_eq!(response["ok"], true);
        assert_eq!(response["lines"][0], "hello");
        let info = request(&path, r#"{"op":"info"}"#);
        assert_eq!(info["columns"], 80);
        let bad = request(&path, "not json");
        assert_eq!(bad["ok"], false);
        server.stop();
        let _ = std::fs::remove_file(&path);
    }

    fn request(path: &Path, body: &str) -> Value {
        use std::io::BufRead;
        let stream = UnixStream::connect(path).unwrap();
        let mut writer = &stream;
        writer.write_all(body.as_bytes()).unwrap();
        writer.write_all(b"\n").unwrap();
        writer.flush().unwrap();
        let mut reader = std::io::BufReader::new(&stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        serde_json::from_str(response.trim_end()).unwrap()
    }
}
