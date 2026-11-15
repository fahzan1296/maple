# ADR-0003 — Pure Rust implementation

| | |
|---|---|
| Status | Accepted |
| Date | 2026-11-15 |
| Deciders | user (decision), IronWolf (proposal) |
| Review date | 2026-12-15 |

## Context

`PRD.md` v0.1 assumed a TypeScript/Bun base with Rust natives on the hot path
(`bun dev`, `pi-tui`, `pi-edit`), following omp's stack. The user decided on
2026-11-15 that maple is implemented in **pure Rust**: the whole agent — TUI,
session core, mesh client, edit engine, memory, guard — is one Rust codebase.

Reason: the product's first distinction is RAM efficiency across many
independent sessions. A single Rust binary with no Bun/Node runtime and no JS
engine per session gives the smallest idle footprint and the simplest
deployment (one binary).

## Decision

- Implementation language: **Rust** (stable toolchain, edition 2021).
- No TypeScript/Bun runtime, no `pi-tui`/`pi-edit` packages, no Node worker
  processes. This also follows from ADR-0001's clean-room rule.
- Standard crates are allowed (`serde`/`serde_json`, `crossterm`, `ratatui`,
  `clap`, `uuid`, `sha2`, `anyhow`); they are dependencies, not copied source.
- TUI: `ratatui` + `crossterm` (raw mode, alternate screen, key events).
- Session/transcript: JSONL via `serde_json`; blob addressing via `sha2`.
- Natives (grep/glob/bash/ast/text-width) are plain Rust modules — no FFI.
- Targets: Linux + macOS first; Windows later.

## Alternatives

1. **TS/Bun + Rust natives (omp stack, rejected)** — fastest path to omp
   parity, but a Bun runtime per session costs roughly 200–400 MB RSS, which
   works against G1 (idle ≤ 256 MB) and the multi-session goal.
2. **Go (rejected)** — smaller runtime than JVM/Node, but the user asked for
   Rust; GC pauses and a larger base RSS than Rust.
3. **Pure Rust (chosen)** — lowest idle RSS, no GC, one binary; costs more
   implementation time in M1 (renderer, edit engine).

## Consequences

- M1 grows: the differential renderer, JSONL session core, and edit engine are
  written from scratch in Rust.
- Build/test must stay inside §SOP-RESLIM limits: `cargo` with `-j2`, never
  `nproc` parallelism, explicit timeouts.
- `PRD.md` §5/§14 and `README.md` are updated to the Rust stack.
- Tooling: `cargo fmt`/`clippy` in CI; `rust-toolchain.toml` pins the channel.

## Evidence

- User instruction, 2026-11-15: "kita pakainya rust murni karna tujuan kita
  membuat berbasis multi session sih biar hemat ram."
- Host: `rustc 1.99.1`; the cargo registry cache already contains
  serde/serde_json, crossterm, ratatui, uuid, sha2, clap (offline build).
