# 32 — Requirement Traceability and Open Decisions

## 1. Traceability

| Requirement | Owning subsystem | Primary verification |
|---|---|---|
| standalone Maple operation | app host, CLI | no-Pi startup test |
| one session per process | runtime/session | actor ownership test |
| append-only history | persistence/transcript | journal property tests |
| branchable transcript | transcript | graph invariant tests |
| bounded context | context assembler | token-budget tests |
| provider portability | provider gateway | adapter contract suite |
| safe tool execution | tool platform | policy/security tests |
| guarded edits | Hashline/workspace | stale snapshot race tests |
| native collaboration | peer mesh | multi-process integration |
| bounded crew | crew scheduler | DAG/permit tests |
| staged learning | memory | approval/provenance tests |
| resource governance | resource subsystem | OS-specific tests |
| TUI separation | presentation/app | dependency boundary checks |
| optional Pi interop | adapter | feature-disabled build tests |

## 2. Open decisions

1. Minimum supported Rust version and toolchain cadence.
2. Async runtime choice and whether one runtime is sufficient.
3. Exact provider set and model capability policy.
4. Canonical journal framing details and checksum choice.
5. Workspace-local vs user-global default for peer mesh.
6. Cryptographic peer authentication requirements for same-user local processes.
7. Hashline token grammar and collision display length.
8. Whether crew child work uses child processes or in-process actors by default.
9. Which OS resource limits can be hard-enforced in each target.
10. Whether optional Pi adapter supports import/export only or live bridging.
11. Whether memory requires user approval always or configurable trust tiers.
12. Exact CLI command names and compatibility commitments.

## 3. Definition of done for architecture

A subsystem specification is implementation-ready when it has:
- purpose and non-goals;
- owner and dependency boundaries;
- public commands/events;
- data schema and size limits;
- state machine;
- invariants;
- algorithm/pseudocode;
- concurrency and cancellation;
- resource budgets;
- security/trust rules;
- failure/recovery behavior;
- observability;
- unit/contract/integration tests;
- migration/versioning rules;
- unresolved questions explicitly listed.

## 4. Repository follow-up

Before implementation, conduct a symbol-level audit of all source files and map each existing module to one of:
- preserve (TUI only, subject to review);
- replace with greenfield target;
- delete/deprecate;
- retain as a test fixture/reference;
- unresolved pending deeper audit.

Do not infer implementation completeness from filenames or line counts.
