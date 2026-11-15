# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Repository initialized (`main`), `.gitignore` excluding reference material
  and local mesh/agent state.
- `LICENSE` (MIT, clean-room) and `THIRD-PARTY-NOTICES` listing oh-my-pi and
  pi-messenger as references only (no code reused).
- `README.md` with status, phase table, layout, and governance pointer.
- `SECURITY.md` with scope, reporting, and threat boundaries.
- `docs/adr/0001-clean-room.md` — clean-room implementation decision and
  module inspiration map.
- `docs/adr/0002-resource-guard.md` — design of the runtime resource guard.
- `KONTRAK.md` — AI Operating Contract v7.0.
- `Cargo.toml` — pure-Rust crate (`edition = "2021"`), deps: `anyhow`, `clap`,
  `serde`/`serde_json`, `sha2`, `time`, `uuid`, `libc`, `ratatui`, `crossterm`.
- Session core (`src/session/`): JSONL transcript types (`SessionHeader`
  v3, `SessionEntry`, `Identity`), hex-encoded cwd layout under
  `~/.maple/agent/sessions/`, SHA-256 blob spill, bounded scan
  (header + tail over an 8 MiB threshold), one-writer lease, and
  non-blocking `branch` lineage.
- CLI (`src/cli.rs`, `src/main.rs`): `maple headless` prints a JSON session
  summary, `maple list` prints session headers, global `--resume/--title/--model`.
- TUI (`src/tui/`): transcript / composer / statusline layout, agent hub
  (`h`: roster sidebar + inspector, `t` tree rails, `/` filter), session picker
  overlay (`/resume`), slash commands (`/new`, `/fork`, `/hub`), `q`/Ctrl-C quit,
  `n` new session, `j`/`k` scroll, `i` edit + Enter send.
- `tests/session_store.rs` — 8 tests: roundtrip, branch lineage, open by
  stem/id, list ordering, blob addressing, lease contention, bounded scan.
- `tests/tui_render.rs` — asserts the rendered buffer contains the transcript,
  composer, statusline, and message text.
- `tests/tui_parity.rs` — 6 tests: hub frame chrome, roster rows (wide/compact
  metrics), tree branches, age labels, picker filter/select, roster aggregation.

### Changed

- `PRD.md` aligned with the clean-room decision (ADR-0001): fork/trim wording
  removed, mesh default `.pi/messenger/`, M0 acceptance restated as a clean-room
  scaffold.
- `docs/adr/0002-resource-guard.md`: environment caveat — `/proc/loadavg` and
  `/proc/stat` can be static under proot; the guard must use live CPU metrics.
- Roadmap changed: no M0–M6 phase gates. All features are implemented directly,
  incrementally, in the build order in `PRD.md` §14 (user decision,
  2026-11-15).
- Stack decision: pure Rust (ADR-0003) — one binary, no Bun/Node; TUI on
  `ratatui` + `crossterm`.

### Notes

- No fork: maple is implemented from scratch, inspired by oh-my-pi and
  pi-messenger (user decision, 2026-11-15).
- M0 build and tests **were** run on the host (2026-11-15): `cargo build`
  and `cargo test` green (9 tests). The host was under memory pressure
  (swap in use), so the build ran with `-j2`; see `KONTRAK.md` §SOP-RESLIM.3.
- TUI idle RSS measured at **4.3 MB** (debug build, 80x24 pty), well under
  the 256 MB budget in `PRD.md` §14 feature 2.
