//! Port of `l/test/debug-server.test.ts`.
//!
//! The crate's debug socket serves NDJSON without a live TUI, so the protocol
//! contract ports directly against `DebugServer`. The reference's "drive a live
//! TUI over the socket" scenario stays ignored: the crate test harness has no
//! TUI instance to attach.

use maple_tui::debug_server::{DebugServer, DebugState};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

fn socket_path(label: &str) -> PathBuf {
	let unique = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.expect("clock")
		.as_nanos();
	std::env::temp_dir().join(format!("maple-debug-{}-{label}-{}.sock", std::process::id(), unique))
}

struct Server {
	path: PathBuf,
	server: DebugServer,
	state: Arc<Mutex<DebugState>>,
	quit: Arc<AtomicBool>,
}

fn start(label: &str) -> Server {
	let path = socket_path(label);
	let state = Arc::new(Mutex::new(DebugState::default()));
	let quit = Arc::new(AtomicBool::new(false));
	let server = DebugServer::start(&path, Arc::clone(&state), Arc::clone(&quit)).expect("bind debug socket");
	Server {
		path,
		server,
		state,
		quit,
	}
}

fn request(server: &Server, payload: &str) -> Value {
	let mut stream = UnixStream::connect(&server.path).expect("connect");
	stream.write_all(payload.as_bytes()).expect("send");
	stream.write_all(b"\n").expect("send");
	let mut reply = String::new();
	BufReader::new(stream)
		.read_line(&mut reply)
		.expect("reply");
	serde_json::from_str(reply.trim_end()).expect("ndjson")
}

#[test]
fn text_op_reports_the_shared_painted_state() {
	let server = start("text");
	server.state.lock().unwrap().lines = vec!["assistant".to_string(), "prompt".to_string()];
	server.state.lock().unwrap().cursor = Some((1, 6));

	let reply = request(&server, r#"{"op":"text"}"#);
	assert_eq!(reply["ok"], json!(true));
	assert_eq!(reply["lines"], json!(["assistant", "prompt"]));
	assert_eq!(reply["cursor"], json!([1, 6]));
}

#[test]
fn info_op_reports_geometry_and_focus() {
	let server = start("info");
	{
		let mut state = server.state.lock().unwrap();
		state.columns = 80;
		state.rows = 24;
		state.overlays = 2;
		state.focused_kind = "editor".to_string();
	}

	let reply = request(&server, r#"{"op":"info"}"#);
	assert_eq!(reply["ok"], json!(true));
	assert_eq!(reply["columns"], json!(80));
	assert_eq!(reply["rows"], json!(24));
	assert_eq!(reply["overlays"], json!(2));
	assert_eq!(reply["focused_kind"], json!("editor"));
}

#[test]
fn keys_op_resolves_key_tokens_and_rejects_unknown_ones() {
	let server = start("keys");
	assert_eq!(request(&server, r#"{"op":"keys","token":"C-c"}"#)["key"], json!("Ctrl('c')"));
	assert_eq!(request(&server, r#"{"op":"keys","token":"a"}"#)["key"], json!("Char('a')"));

	let reply = request(&server, r#"{"op":"keys","token":"X"}"#);
	assert_eq!(reply["ok"], json!(false));
	assert!(reply["error"].as_str().unwrap_or_default().contains("Unknown key token"));
}

#[test]
fn unknown_ops_and_malformed_lines_keep_the_server_serving() {
	let server = start("robust");

	let unknown = request(&server, r#"{"op":"nope"}"#);
	assert_eq!(unknown["ok"], json!(false));
	assert!(unknown["error"].as_str().unwrap_or_default().contains("Unknown op"));

	let malformed = request(&server, "not json");
	assert_eq!(malformed["ok"], json!(false));

	assert_eq!(request(&server, r#"{"op":"text"}"#)["ok"], json!(true));
}

#[test]
fn values_op_reports_an_empty_value_map() {
	let server = start("values");
	let reply = request(&server, r#"{"op":"values"}"#);
	assert_eq!(reply["ok"], json!(true));
	assert_eq!(reply["values"], json!({}));
}

#[test]
fn quit_op_flips_the_quit_flag() {
	let server = start("quit");
	assert!(!server.quit.load(std::sync::atomic::Ordering::Relaxed));
	assert_eq!(request(&server, r#"{"op":"quit"}"#)["ok"], json!(true));
	assert!(server.quit.load(std::sync::atomic::Ordering::Relaxed));
}

fn stopping_the_server_removes_the_socket() {
	let server = start("stop");
	let path = server.path.clone();
	assert!(path.exists());
	drop(server);
	assert!(!path.exists());
}

#[test]
#[ignore = "missing API: a TUI instance to drive over the debug socket"]
fn drives_and_inspects_a_live_tui() {
}
