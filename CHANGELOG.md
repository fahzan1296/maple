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
- M0 build was **not** run at the time of this commit: the host was under a
  resource abort condition (load average ≈ 18 on 8 cores, swap in use). See
  `KONTRAK.md` §SOP-RESLIM.3.
