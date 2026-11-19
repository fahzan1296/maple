# 01 — Product Definition, Requirements, and Non-Goals

## 1. Product definition

Maple is a local-first coding-agent environment presented through a terminal UI. It accepts user intent, builds a bounded model context, calls a configured model provider, interprets the response, invokes approved tools, records all meaningful state transitions, and returns a user-visible result. It supports multiple independent sessions and optional cooperative execution across local peer processes.

Maple is not a server-first SaaS product. A daemon, remote control plane, mandatory database server, external agent host, or Pi installation is not required for normal use.

## 2. Primary user journeys

### 2.1 Start a session
1. Parse CLI arguments and environment.
2. Resolve configuration and working directory.
3. Validate runtime preconditions and resource headroom.
4. Initialize local storage and session identity.
5. Acquire terminal ownership.
6. Construct application services.
7. Enter the TUI event loop.

Acceptance: startup fails with an actionable typed error and leaves no corrupt partial session if any mandatory stage fails.

### 2.2 Complete an agent turn
1. Accept user input.
2. Persist user message before remote work begins.
3. Build an immutable context snapshot.
4. Select provider/model and validate capabilities.
5. Stream normalized model events.
6. If a tool call is requested, validate schema and policy.
7. Request approval when policy requires it.
8. Execute within deadline and resource permit.
9. Persist tool result and continue the model loop.
10. Persist terminal assistant outcome and render it.

Acceptance: cancellation, provider failure, tool failure, and process interruption are distinguishable and recoverable.

### 2.3 Resume a session
1. Resolve session ID/path.
2. Validate manifest and journal framing.
3. Recover the last valid journal offset.
4. Rebuild derived indexes.
5. Resolve active transcript branch.
6. Reconstruct pending/interrupted turn status.
7. Restore UI state without replaying side effects.

Acceptance: recovery never silently repeats an external side effect.

### 2.4 Collaborate
1. Create or join a Maple-native workspace mesh.
2. Discover peers through local workspace metadata.
3. Exchange typed messages and task claims.
4. Reserve shared files before conflicting edits.
5. Detect stale reservations and peer liveness changes.
6. Reconcile results through explicit conflict handling.

Acceptance: collaboration is optional and does not affect single-process correctness.

## 3. Functional requirements

- FR-001: CLI supports interactive startup, explicit session resume, non-interactive execution, diagnostics, and storage inspection.
- FR-002: one process owns one active interactive session.
- FR-003: transcript entries are append-only and branchable.
- FR-004: context assembly is token-budgeted and provenance-aware.
- FR-005: provider adapters normalize streaming and errors.
- FR-006: tool calls pass schema, policy, approval, and resource gates.
- FR-007: filesystem mutations are workspace-scoped and auditable.
- FR-008: editing uses stable anchors and stale-content detection.
- FR-009: large payloads use immutable content-addressed blobs.
- FR-010: local peer coordination is Maple-native.
- FR-011: crew tasks form a validated DAG with bounded concurrency.
- FR-012: memory learning is staged, reviewable, and provenance-bearing.
- FR-013: resource budgets are centrally admitted and monitored.
- FR-014: terminal restoration is guaranteed on all shutdown paths.
- FR-015: structured logs and diagnostics never expose secrets by default.
- FR-016: optional Pi interoperability is isolated and can be removed without changing core behavior.

## 4. Non-functional requirements

| ID | Requirement | Verification |
|---|---|---|
| NFR-001 | bounded memory growth | stress tests + RSS sampling |
| NFR-002 | bounded task concurrency | scheduler invariant tests |
| NFR-003 | crash-recoverable journal | fault injection at write boundaries |
| NFR-004 | deterministic domain transitions | pure unit/property tests |
| NFR-005 | no terminal corruption | pseudo-terminal integration tests |
| NFR-006 | provider-independent orchestration | fake provider contract suite |
| NFR-007 | no mandatory Pi dependency | build and runtime tests without Pi artifacts |
| NFR-008 | least-privilege tools | policy and sandbox tests |
| NFR-009 | explicit cancellation | cancellation propagation tests |
| NFR-010 | platform-aware resource enforcement | OS-specific integration tests |

## 5. Explicit non-goals

- A distributed cloud scheduler.
- A multi-tenant server.
- A mandatory background daemon.
- A general-purpose plugin marketplace in the first architecture release.
- Dynamic loading of arbitrary native code.
- Silent automatic execution of destructive tools.
- Treating model output as trusted instructions.
- Making the TUI the source of truth for application state.
- Requiring `.pi`, Pi binaries, Pi configuration, Pi runtime APIs, or Pi-specific storage.
- Exact compatibility with an external format unless a named fixture and compatibility level are approved.
- Guaranteeing hard OS resource limits on platforms that only provide sampling.

## 6. Product invariants

1. A session has one authoritative writer.
2. Journal order is monotonic within a session.
3. A branch references an existing parent entry.
4. Tool execution is never authorized solely because the model requested it.
5. Every mutating tool has a workspace and policy context.
6. A file commit must prove that its base snapshot is still valid.
7. Memory candidates do not become trusted global knowledge automatically.
8. Peer messages are untrusted input.
9. Optional integrations cannot become hidden core dependencies.
10. Shutdown is idempotent.
