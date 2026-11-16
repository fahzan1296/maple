# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Repository initialized (`master`), `.gitignore` excluding reference material
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
  `serde`/`serde_json`, `sha2`, `time`, `uuid`, `libc`, `maple-tui` (in-repo).
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
- `tests/tui_parity.rs` — 8 tests: hub chrome, roster rows, tree branches, age
  labels, picker, roster scan/activity, ask delivery, activity tab render.
- Hub Activity tab: recent transcript entries across sessions (bounded feed,
  filter, Enter jumps to the source session) and the `a` ask form, which delivers
  a message into the selected peer's transcript with a `from` marker.
- Statusline now surfaces the last status/error message (append, resume, ask).
- `CONTRIBUTING.md` — setup, workflow, definition of done, and label taxonomy.
- `CODE_OF_CONDUCT.md` — community expectations and enforcement.
- `.github/ISSUE_TEMPLATE/` (bug report, feature request) with a chooser
  config, and `.github/PULL_REQUEST_TEMPLATE.md`.
- `docs/README.md` — documentation index (product, governance, ADRs, changelog).
- Repository metadata: description and topics (`rust`, `tui`, `ratatui`,
  `coding-agent`, `ai-agent`, `cli`, `terminal`, `multi-agent`, `llm`).
- Triage labels: `priority: high|medium|low`, `status: triage|in-progress|
  blocked|needs-review`, `component: session|tui|mesh|resource-guard|hashline|
  autolearn|crew`, plus `security` and `performance`.
- `.github/workflows/ci.yml` — CI on pushes to `master` and on pull requests:
  build plus tests on Ubuntu and macOS (`CARGO_BUILD_JOBS=2`,
  `--test-threads=2`) and a clippy pass. Windows stays out of scope for v0.1
  (PRD §15).
- `.github/workflows/release.yml` — a `v*` tag builds release tarballs for
  `x86_64-unknown-linux-gnu` and `aarch64-apple-darwin` and opens a draft
  GitHub release with both artifacts.
- Apps slice #1 (`src/apps/`): `apps::git::avatar` (deterministic mirrored 5x5
  identicon, SHA-256 digest) and `apps::debug::log_formatting` (sanitized,
  wrapped log rows; RFC 3339 and offset-less ISO-8601 timestamps; finite pid).
  `maple-tui` now re-exports `ansi_scan::strip_ansi`.
- `scripts/install.sh` — release build into `~/.local/bin` (or `--prefix DIR`,
  `--global`); `scripts/uninstall.sh` removes it again.
- `scripts/dev.sh` — local CI mirror: build and test with two jobs, then
  `fmt`/`clippy` when those tools are installed.
- CI enforces `KONTRAK.md` §SOP-RESLIM: `timeout-minutes` per job and step,
  sized-runner justification, and 7-day artifact retention.
- Apps slice #2: `apps::debug::terminal_info` (terminal capability snapshot and
  the debug-menu report) plus `CellDimensions` in maple-tui
  (`cell_dimensions` / `set_cell_dimensions`).
- Apps slice #3: `apps::debug::viewer_frame` (rounded panel, fixed header and
  footer rows, persistent scroll viewport with selection reveal) plus the
  `ScrollView` reveal-range API (`ScrollRangeAnchor`, `reveal_range`,
  `logical_row_at`).
- Apps slice #4: `apps::debug::log_viewer` model (filter, pagination, expansion,
  selection, session boundary marker, older-history paging) plus
  `split_log_text` / `build_log_copy_payload`.
- Apps slice #5: `apps::ps_data` — `omp ps` table data (broker scopes, daemon
  rows, `format_command` / `collapse_command` / `daemon_label`, colored state,
  flags and uptime cells, `table_cells` aligned with `TABLE_HEADER`,
  `scope_header`), with the daemon state types declared in-module until the
  daemon port lands.
- Apps slice #6: `apps::autoresearch_data` — experiment baseline selection
  (`format_elapsed`, `is_better`, `current_results`, `find_baseline_result`,
  `find_baseline_metric`, `find_baseline_run_number`,
  `find_baseline_secondary`) plus the `ExperimentResult` / `MetricDef` /
  `MetricDirection` / `AsiValue` types, declared in-module until the
  `tools/autoresearch` port lands.
- Apps slice #7: `apps::git::colors` — truecolor git chrome helpers
  (`GitPalette`, `hex_channels`, `bg_ansi_hex` / `fg_ansi_hex`, `luminance`,
  `with_bg`, `pill`, `chip_fill`, `selection_bg_ansi`, `tint_chip`,
  `soft_pill`); the theme is passed in as a resolved palette instead of a
  global singleton.
- Engine slice #1: `native::spans` — ANSI-styled text → escape-free spans
  (`styled_spans`, `plain_text`, `plain_line`, `style_spans`, `compact_text`,
  `rows_text`, `spans_line`, `styled_background`). Parses CSI/OSC/DCS/APC
  escapes, reverse-maps SGR to theme tokens, coalesces runs; the theme is
  passed in instead of read from a global singleton.
- Engine slice #2: `render::{row_moves, types}` — the unsynchronized row-move
  finder (`find_row_move`, `apply_row_move`, `row_move_sequence`) reproducing
  the reference's out-of-window gain drop via signed bounds, plus `TreeContext`.

### Changed

- `README.md` rewritten using the
  [awesome-readme-template](https://github.com/Louis3797/awesome-readme-template)
  structure: centered header with badges, table of contents, project status,
  repository layout, tech stack, features, getting started, usage, roadmap,
  contributing, FAQ, license, contact, and acknowledgements.
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
