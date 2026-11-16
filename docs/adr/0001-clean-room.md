# ADR-0001 — Clean-room re-implementation from oh-my-pi and pi-messenger

| | |
|---|---|
| Status | Accepted (amended 2026-11-16) |
| Date | 2026-11-15 (amended 2026-11-16) |
| Deciders | user (decision), IronWolf (proposal) |
| Review date | 2026-12-15 |

## Context

`PRD.md` §4.3 originally proposed forking oh-my-pi (MIT) as the base, on the
grounds that it is the lowest-risk path and already contains the TUI, session
core, hashline edit, and autolearn. pi-messenger supplies the flat cross-session
mesh format.

Forking was rejected by the user: maple must be implemented from scratch
(clean-room), with both projects used as inspiration only.

## Decision

- maple is a **clean-room re-implementation**: the Rust source is written from
  scratch, and **no code is copied, vendored, or imported** from oh-my-pi or
  pi-messenger.
- Both projects are the specification. Behaviour and file/wire formats are
  reproduced 1:1 from their source; expression (source text) is not.
- The `.pi/messenger` on-disk format is implemented natively so existing
  pi-messenger agents can interoperate (PRD §7 — compatibility, not imitation).
- `LICENSE` is MIT, copyright maple contributors only. `THIRD-PARTY-NOTICES`
  records the references and the versions read.

## Alternatives

1. **Fork oh-my-pi (rejected)** — fastest, but explicitly ruled out by the
   user; it also imports a large feature debt (PRD §15) and attribution
   obligations.
2. **Vendor selected omp crates (rejected)** — still copying expression; ruled
   out with the fork.
3. **Clean-room (chosen)** — slower, but no licensing/attribution carry-over,
   no inherited non-goal surface, and a codebase small enough to hold on its
   own memory budget (PRD G1).

## Consequences

- M0/M1 must build the TUI renderer, session core, and edit engine ourselves.
  Expect a larger M1 than the fork path implied.
- Interfaces are re-derived from the reference source (`l/src/**`, read-only)
  and `oh-my-pi/docs/*`; every ported module cites the file it reproduces in
  its module doc, and `team.memory/port-coverage.md` tracks the mapping.
- No third-party copyright lines are required beyond the reference table.

## Amendment 2026-11-16

The user mandated a total port: every file under `l/` (the oh-my-pi TypeScript
source) and `l/test` is to be reproduced in Rust, 1:1. That supersedes the
original "inspiration only" framing — the reference source is now the binding
specification, not merely reading material. The clean-room constraint is kept
in its verifiable form: no source is copied or vendored; every line under
`crates/` is written fresh, in Rust, from that specification.

## Evidence

- User instruction, 2026-11-15: "jangan di fork, impl dari 0 kita tapi inspirasi
  dari mreka".
- oh-my-pi v18.20.2, commit `7c49c11` (read 2026-11-15): `docs/tui.md`,
  `docs/session.md`, `docs/tools/edit.md`, `docs/memory.md`.
- pi-messenger v0.15.2 (read 2026-11-15): `store.ts`, `lib.ts`, `feed.ts`.
- User instruction, 2026-11-16: "impl sepenuhnya @l/" — port the whole `l/`
  tree 1:1; the clean-room claim must be corrected to match.

## Review

If a module later requires a vendored dependency, that dependency must be
listed in `THIRD-PARTY-NOTICES` with license and version before merge.
