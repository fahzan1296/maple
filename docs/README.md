# Documentation index

maple's documentation is split by role: **what to build**, **how work is
governed**, and **why decisions were made**.

## Product & design

| Doc | Contents |
|---|---|
| [`../PRD.md`](../PRD.md) | Product requirements and design: goals/non-goals, architecture, session model, mesh contract, TUI, hashline, autolearn, resource guard, security, build order (§14), risks and open questions (§15) |
| [`../README.md`](../README.md) | Project overview and getting-started for new contributors |

## Governance

| Doc | Contents |
|---|---|
| [`../KONTRAK.md`](../KONTRAK.md) | AI Operating Contract v7.0: coordination, resource limits (§SOP-RESLIM), security (§SOP-SEC), injection defense (§SOP-INJECT), shipping |
| [`../SECURITY.md`](../SECURITY.md) | Trust boundaries, reporting process, rules enforced, known limitations |

## Decision records

ADRs are numbered, append-only, and state the decision plus the alternatives
considered. Read them before proposing a change to the affected area.

| ADR | Decision |
|---|---|
| [`adr/0001-clean-room.md`](adr/0001-clean-room.md) | from-scratch Rust re-implementation; oh-my-pi and pi-messenger are read as the specification |
| [`adr/0002-resource-guard.md`](adr/0002-resource-guard.md) | Runtime resource guard: pre-flight, monitoring, abort at 80% |
| [`adr/0003-pure-rust.md`](adr/0003-pure-rust.md) | Pure Rust, one binary, no JS runtime; TUI on ratatui + crossterm |

## Changelog

[`../CHANGELOG.md`](../CHANGELOG.md) — Keep a Changelog format, Semantic
Versioning. Every user-visible change lands under `[Unreleased]`.

## Conventions

- Docs are written in English; PRD prose and contract sections may quote
  Indonesian terms where the source material does.
- File names are lowercase-kebab-case; ADRs are `NNNN-slug.md`.
- Links are relative so the docs work from any branch or clone depth.
