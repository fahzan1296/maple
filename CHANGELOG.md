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
- M0 build was **not** run at the time of this commit: the host was under a
  resource abort condition (load average ≈ 18 on 8 cores, swap in use). See
  `KONTRAK.md` §SOP-RESLIM.3.
