# 31 — Architecture Decision Records

## ADR-001: Maple-native core

**Decision:** all canonical domain models, storage, runtime, and collaboration protocols are Maple-owned.  
**Reason:** prevent external ecosystem coupling and permit independent evolution.  
**Consequence:** adapters perform explicit translation at boundaries.

## ADR-002: Pi compatibility is optional

**Decision:** Pi interoperability is a separately packaged, disabled-by-default adapter.  
**Reason:** Maple must be independently usable and testable.  
**Consequence:** no Pi type or storage assumption in core; compatibility claims require fixtures and named versions.

## ADR-003: Preserve TUI, rebuild non-TUI systems

**Decision:** reuse/extract the existing TUI as a presentation asset after contract review; implement non-TUI domain/runtime systems greenfield.  
**Reason:** user direction and clean ownership boundaries.  
**Consequence:** no legacy implementation is grandfathered in without review.

## ADR-004: Append-only session journal

**Decision:** durable session events are append-only; indexes are derived.  
**Reason:** crash recovery, provenance, auditability.  
**Consequence:** schema evolution and compaction must preserve source history.

## ADR-005: Content-addressed blobs

**Decision:** large immutable payloads use SHA-256-addressed storage.  
**Reason:** deduplication, integrity verification, bounded journal records.  
**Consequence:** garbage collection requires reference reachability.

## ADR-006: Single-process session actor

**Decision:** one actor owns one session's authoritative state transitions.  
**Reason:** simple ordering and fewer shared mutable locks.  
**Consequence:** long work must be delegated and completed through messages.

## ADR-007: Native peer mesh

**Decision:** Maple-native local coordination is canonical; external messenger formats are optional adapters.  
**Reason:** independent lifecycle and protocol ownership.  
**Consequence:** native compatibility matrix and migration policy are required.

## ADR-008: Guarded Hashline editing

**Decision:** edits require current snapshot, verified anchors, seen-line proof, and atomic commit.  
**Reason:** prevent stale-context writes.  
**Consequence:** ambiguous/stale edits fail closed.

## ADR-009: Bounded DAG crew scheduler

**Decision:** crew tasks use a validated DAG, finite concurrency, and lease-based claims.  
**Reason:** predictable resource use and recoverable orchestration.  
**Consequence:** exactly-once distributed execution is not promised.

## ADR-010: TUI as projection

**Decision:** TUI consumes view models and emits intents; domain state remains outside UI.  
**Reason:** testability and future headless mode.  
**Consequence:** application layer owns presentation projections.

## ADR-011: Explicit resource enforcement levels

**Decision:** distinguish hard OS enforcement from sampling/advisory limits.  
**Reason:** platform APIs differ.  
**Consequence:** diagnostics and docs cannot overstate guarantees.
