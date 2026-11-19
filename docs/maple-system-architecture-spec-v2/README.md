# Maple — Greenfield System Architecture Specification

**Edition:** Architecture Specification V2  
**Status:** Proposed target; not an implementation claim  
**Repository baseline previously inspected:** `fahzan1296/maple`, commit `a40a01d70d346a54b1d65d767ab31f3071aaf726`  
**Scope:** Documentation and architecture only. No source-code changes are part of this deliverable.

## 1. Executive decision

Maple is a standalone, pure-Rust, single-binary TUI coding-agent platform. The target system is designed from first principles around Maple-owned domain models, protocols, persistence, execution, and resource governance. The existing TUI is the sole intended reusable implementation asset; its presentation contract is retained and adapted behind a strict application boundary. All non-TUI runtime and domain systems are greenfield.

**Pi compatibility is optional.** Maple MUST operate correctly when Pi is absent, when no `.pi` directory exists, and when all interoperability features are disabled. No core type, database schema, runtime state machine, peer-discovery algorithm, session lifecycle, or mandatory startup path may depend on Pi. Interoperability belongs in an isolated adapter boundary and is disabled unless explicitly enabled or invoked.

## 2. Normative language

- **MUST / MUST NOT**: mandatory invariant.
- **SHOULD / SHOULD NOT**: default design; deviations require a documented reason.
- **MAY**: optional capability.
- **Core**: required Maple-owned runtime.
- **Optional integration**: independently disableable adapter.
- **External format**: data accepted or emitted at a boundary, never the internal source of truth.

## 3. Product and architecture goals

1. A user can start, run, resume, inspect, and stop a Maple session without any external agent runtime.
2. A single process owns one interactive session; multiple independent processes may collaborate as symmetric peers.
3. Session history is append-only and recoverable after interruption.
4. Large payloads are stored as immutable content-addressed blobs.
5. The agent loop is deterministic at the orchestration layer, cancellable, bounded, observable, and testable.
6. Tools are typed capabilities with explicit policy, approval, deadlines, and resource budgets.
7. File edits are guarded by snapshot identity, content anchors, seen-line evidence, and atomic commit.
8. Collaboration uses a Maple-native protocol and storage layout.
9. Crew execution is a bounded DAG scheduler, not an unbounded task spawner.
10. Resource policy is enforced at admission, execution, and monitoring boundaries.
11. The TUI renders state; it does not own business logic, persistence, provider calls, or process supervision.
12. Every subsystem has explicit ownership, contracts, invariants, error semantics, and tests.

## 4. Evidence and certainty labels

| Label | Meaning |
|---|---|
| VERIFIED CURRENT | Directly observed in the previously inspected repository snapshot |
| SPECIFIED | Present in the product/architecture requirements |
| PROPOSED TARGET | Design decision in this edition |
| INFERRED | Necessary implication, not an explicit source requirement |
| OPEN | Requires owner decision, platform measurement, or compatibility fixture |

This edition does not convert a repository path inventory into proof of working behavior. The previous inspection covered product documents, selected core sources, TUI contract, ADRs, manifests, and a recursive tree inventory. It was not a semantic line-by-line audit of every source symbol.

## 5. Reading order

1. `01-product-and-non-goals.md`
2. `02-architecture-principles.md`
3. `03-system-context-and-deployment.md`
4. `04-domain-model.md`
5. `05-runtime-and-lifecycle.md`
6. `06-session-and-transcript.md`
7. `07-context-and-compaction.md`
8. `08-provider-gateway.md`
9. `09-agent-orchestration.md`
10. `10-tool-platform.md`
11. `11-workspace-and-filesystem.md`
12. `12-hashline-editing.md`
13. `13-native-peer-mesh.md`
14. `14-optional-pi-interop.md`
15. `15-crew-scheduler.md`
16. `16-memory-and-learning.md`
17. `17-persistence-and-recovery.md`
18. `18-resource-governance.md`
19. `19-security-and-trust.md`
20. `20-concurrency-and-backpressure.md`
21. `21-tui-and-terminal.md`
22. `22-cli-and-configuration.md`
23. `23-errors-and-observability.md`
24. `24-rust-workspace-and-module-map.md`
25. `25-testing-and-verification.md`
26. `26-performance-and-capacity.md`
27. `27-build-release-and-operations.md`
28. `28-data-contract-catalog.md`
29. `29-state-machine-catalog.md`
30. `30-failure-mode-analysis.md`
31. `31-architecture-decisions.md`
32. `32-requirement-traceability.md`
33. `APPENDIX-A-sequence-diagrams.md`
34. `APPENDIX-B-security-review-checklist.md`
35. `APPENDIX-C-platform-matrix.md`
36. `APPENDIX-D-glossary.md`

## 6. Global boundary rule

The dependency direction is:

`TUI / CLI -> Application Use Cases -> Domain Services -> Ports -> Infrastructure Adapters`

The domain MUST NOT import terminal libraries, provider SDKs, filesystem implementations, operating-system APIs, Pi types, or concrete database drivers. Adapters implement ports. The application layer coordinates use cases and transactions. The domain owns invariants.

## 7. Greenfield migration rule

Existing source is evidence and a reusable TUI asset, not an architectural mandate. Before reusing any non-TUI implementation, it must be explicitly reviewed and accepted as an exception. No accidental carry-over of legacy module boundaries, persistence assumptions, or integration coupling is allowed.

## 8. Deliverable limitations

This is a normative target design, not executable software, a proof of implementation, or a certification of OS-level resource enforcement. Numeric budgets are starting policy values and require benchmark validation on supported platforms.
