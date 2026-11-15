# maple

TUI coding agent with three distinctions:

1. **RAM-efficient** — pure Rust, one binary (no JS runtime), append-only
   transcripts with bounded scans, blob spill for large artifacts, and a
   per-session memory budget that is monitored and enforced (abort at 80%).
2. **Multi-session collaboration, independent sessions** — daemon-free,
   file-based mesh with flat peers (no host/guest), wire-compatible with the
   `.pi/messenger` format used by pi-messenger.
3. **omp-class UX + resource discipline** — differential renderer, Agent Hub,
   `ask`, keybindings, theme, hashline edits, two-phase autolearn, and
   pre-flight/monitor/abort enforced by the runtime, not just asked of the
   agent.

maple is a **clean-room implementation**: it is written from scratch and
inspired by oh-my-pi (TUI/UX, session, edit, memory) and pi-messenger (flat
cross-session mesh). No code is copied from either — see `THIRD-PARTY-NOTICES`.

## Status

Semua fitur diimplémentasikan langsung, bertahap (build order di `PRD.md` §14).

| # | Fitur | State |
|---|---|---|
| 1 | CLI + session core | **in progress** |
| 2 | TUI renderer | pending |
| 3 | Agent Hub | pending |
| 4 | Mesh (`.pi/messenger` compat) | pending |
| 5 | Resource guard | pending |
| 6 | Hashline edit | pending |
| 7 | Autolearn | pending |
| 8 | Crew (`plan`/`work`/`task.*`) | pending |
| 9 | Hardening + docs | pending |

See `PRD.md` for the full design and `docs/adr/` for decisions.

## Layout

```
.
├── PRD.md                 # product requirements & design (v0.1)
├── KONTRAK.md             # AI Operating Contract v7.0 (governance)
├── docs/
│   ├── adr/               # architecture decision records
│   └── peers/decisions.md # append-only mirror of team memory
└── src/                  # product source (pure Rust crate)
```

`oh-my-pi/` and `pi-messenger/` are reading material only (gitignored).

## Governance

Work follows `KONTRAK.md` (AI Operating Contract v7.0): P1 coordination via
pi-messenger, P2 the work itself, aggressive resource limits
(§SOP-RESLIM) enforced at runtime.

## Coordination

Mesh state lives in `.pi/messenger/` (project-scoped feed) and
`~/.pi/agent/messenger/` (registry/inbox). See `PRD.md` §7.

## License

MIT — see `LICENSE` and `THIRD-PARTY-NOTICES`.
