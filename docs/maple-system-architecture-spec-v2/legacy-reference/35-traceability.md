# Legacy V1 reference

This file is preserved for comparison only. Where it conflicts with V2, V2 is authoritative. In particular, `.pi` compatibility is optional and Maple-native behavior is canonical.

# 35 — Traceability and open questions

## Sources inspected
`PRD.md`, `KONTRAK.md`, `README.md`, `SECURITY.md`, `Cargo.toml`, `src/lib.rs`, `src/main.rs`, `src/cli.rs`, `src/session/{types,store,path,blob}.rs`, `src/resource_guard.rs`, `crates/maple-tui/Cargo.toml`, `crates/maple-tui/docs/CONTRACT.md`, ADR-0001…0004 and recursive Git tree at commit `a40a01d70d346a54b1d65d767ab31f3071aaf726`.

## Current vs target
| Area | Current evidence | Target |
|---|---|---|
| Pure Rust binary | root package, ADR-0003 | preserve |
| TUI toolkit | substantial `crates/maple-tui` tree | preserve/extract boundary |
| Session JSONL | `src/session/store.rs` | harden/evolve |
| Blob spill | `src/session/blob.rs` SHA-256 | streaming/integrity/GC |
| Resource snapshot | `src/resource_guard.rs` | full permit + process-tree monitor |
| Agent loop | PRD intent; not verified complete | design/build from zero |
| Provider gateway | not verified complete in inspected files | design/build from zero |
| Tool platform | PRD intent; not verified complete | design/build from zero |
| Hashline | PRD intent; implementation not verified | design/build from zero |
| Mesh | PRD intent; full implementation not verified | design/build from zero |
| Autolearn | PRD intent; not verified | design/build from zero |
| Crew DAG | PRD optional M4 | design/build from zero |
| Full guard enforcement | source comment notes single-process/no cgroups/macOS gaps | redesign/harden |

## Evidence caveats
Git tree is an inventory, not semantic audit. A path does not prove feature completeness. README/PRD may lag source. Current resource guard explicitly documents limitations. TUI source size does not imply the agent runtime is complete.

## Requirement traceability
G1 → resource governance, concurrency, capacity, tests.  
G2/G3 → mesh, reservations, crew, session/process model.  
G4 → TUI boundary and terminal rendering.  
G5 → memory/autolearn.  
G6 → hashline.  
G7 → guard, recovery and runbooks.  
Clean-room → workspace, release governance and ADRs.

## Open questions
1. Exact provider set for v0.1?
2. Local search/index engine or bounded filesystem traversal first?
3. Which memory backends are launch requirements?
4. Exact pi-messenger compatibility version/fixtures?
5. Crew in v0.1 or later?
6. First-release OS targets?
7. Durability policy for prompts, side effects and claims?
8. Measured idle RSS and 1/4/8-session ceilings on target hardware?
9. Which TUI areas remain unchanged versus adapter refactor?
10. Exact authorization semantics for focus/steer/revive/kill across independent processes?

## Architecture readiness
Ready for implementation only when every system has owner/interface, state machine and failure paths; side effects have policy/resource gates; schemas are versioned; file protocols have crash semantics; requirements map to positive/negative tests; performance targets have measurements; and open decisions are accepted or deferred.
