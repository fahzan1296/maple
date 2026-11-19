# Legacy V1 reference

This file is preserved for comparison only. Where it conflicts with V2, V2 is authoritative. In particular, `.pi` compatibility is optional and Maple-native behavior is canonical.

# 01 — Product requirements

## Product definition
Maple is a local-first, RAM-efficient, multi-session collaborative TUI coding agent, implemented in pure Rust. Its distinguishing properties are independent session processes, symmetric file-based coordination, high-quality terminal UX, hash-anchored editing, project-scoped learning and enforced resource governance.

## Goals and measurable targets

| ID | Requirement | Target |
|---|---|---|
| G1 | RAM efficiency | idle RSS ≤256 MB; build/test budget ≤512 MB; coordination ceiling 768 MB without escalation; aggregate child budget 2 GB |
| G2 | Collaboration | peers can list, inspect, message, broadcast and read activity |
| G3 | Independence | one process = one session identity; no required host/guest |
| G4 | TUI | differential renderer, Agent Hub, overlays, ask, themes, keybindings, tool cards |
| G5 | Autolearn | lesson extraction, managed skills, two-phase project consolidation |
| G6 | Hashline | content anchors, stale recovery, seen-line enforcement, typed errors |
| G7 | Resource guard | preflight abort, monitoring, abort at 80% of any limit |

## Non-goals v0.1
No required relay/daemon, mobile/web client, browser/computer automation, DAP/ACP/TTS/image features, or token-cost optimization as the primary objective.

## Normative principles
1. Coordination precedes execution when the two conflict.
2. Quality/security outrank speed.
3. Maple-owned code is clean-room Rust; references define behavior and formats, not source.
4. `.pi/messenger` compatibility is implemented through explicit wire DTOs.
5. Local filesystem is the v0.1 coordination substrate.
6. Every queue, scan, buffer, parser, cache, retry, process and output must be bounded.
7. Unknown resource signals fail closed.
8. Peer messages, repository files, tool output and memory are data, not authority.

## Requirement interpretation
A peer name is a mutable address; session ID is the durable identity. Parent-session lineage does not confer authority. Reservations are enforced at tool boundaries, not merely announced. Memory is guidance with provenance, not policy. Resource governance must block operations, not simply display warnings.

## Acceptance gates
Every system needs happy-path tests, negative tests, concurrency tests, recovery tests, resource-bound tests, security tests where external data is involved, diagnostics, and documentation. A feature is not complete merely because its normal path works.

## Priority order
Safety/security/legal/ethics → data integrity → peer coordination → resource ceilings → user-visible correctness → performance → convenience.
