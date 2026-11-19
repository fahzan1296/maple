# 25 — Testing and Verification Strategy

## 1. Test pyramid

- domain unit tests: pure invariants and state transitions;
- property tests: IDs, journal ordering, DAG, anchors, path normalization;
- adapter contract tests: provider, filesystem, process, terminal, peer transport;
- integration tests: session turn, resume, tool execution, collaboration;
- fault-injection tests: crash, disk full, partial write, timeout, cancellation;
- performance tests: memory, latency, throughput, queue pressure;
- end-to-end pseudo-terminal tests.

## 2. Fake ports

Every external boundary must have a deterministic fake:
- fake clock;
- fake provider stream;
- in-memory journal;
- temp filesystem;
- fake process supervisor;
- fake peer transport;
- virtual terminal;
- deterministic resource sampler.

## 3. Golden fixtures

Maintain fixtures for:
- journal schema versions;
- provider stream formats;
- Hashline snapshots and edits;
- mesh envelopes;
- memory migrations;
- optional Pi import/export versions.

Fixtures must identify origin and expected compatibility level.

## 4. Fault injection points

Inject failure:
- before and after journal append;
- between blob write and manifest update;
- during atomic rename;
- mid-provider stream;
- before/after child spawn;
- during tool side effect;
- during reservation renewal;
- during task claim;
- during terminal teardown.

## 5. Required invariants

- no invalid event sequence accepted;
- no stale file edit committed;
- no unapproved privileged tool executed;
- no task concurrency cap exceeded;
- no permit leak after cancellation;
- no secret in logs;
- no optional integration required by core tests;
- no terminal raw mode left active on ordinary error paths;
- no recovery replays non-idempotent side effects.

## 6. CI stages

1. formatting;
2. lint;
3. unit tests;
4. property tests with bounded seeds;
5. adapter contract tests;
6. integration tests;
7. security checks;
8. dependency/license checks;
9. docs/link/schema validation;
10. benchmark smoke tests.

Slow stress suites may run on schedule and before releases.

## 7. Test timeout policy

Use the project baseline as an initial ceiling: 60 seconds per test file, 10 minutes for full suite, 5 minutes for build, and 20 minutes for migration. These are policy ceilings to tune from CI measurements, not reasons to kill deterministic tests prematurely.

## 8. Documentation tests

All normative examples and schemas should have machine-checkable fixtures where possible. Mermaid diagrams are reviewed for consistency with state and event catalogs.
