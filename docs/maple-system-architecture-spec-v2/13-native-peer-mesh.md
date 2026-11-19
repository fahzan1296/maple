# 13 — Maple-Native Peer Mesh

## 1. Product rule

Peer collaboration is a Maple feature. Its canonical protocol, identity model, message envelope, storage layout, liveness rules, reservations, and conflict semantics are owned by Maple. External compatibility is not a protocol dependency.

## 2. Goals and non-goals

Goals: local multi-process coordination, asynchronous messaging, task claims, file reservations, peer liveness, bounded storage, recovery after process exit.

Non-goals: consensus across unreliable networks, global distributed transactions, remote internet federation, leader election for all work, guaranteed exactly-once delivery.

## 3. Peer identity

`PeerId` identifies a logical participant. `ProcessInstanceId` identifies one process incarnation. PID is metadata only. Each peer record includes workspace ID, session ID, process start, protocol version, capability flags, heartbeat sequence, and expiry.

## 4. Native message envelope

Required fields:
- protocol version;
- message ID;
- sender peer ID;
- sender process instance ID;
- recipient peer ID or topic;
- message kind;
- created timestamp;
- expiry;
- correlation ID;
- payload schema version;
- payload bytes/digest.

Payloads are typed and size-bounded. Unknown required message kinds are rejected. Expired messages are not delivered as current work.

## 5. Delivery semantics

Baseline is at-least-once delivery with deduplication by message ID. Exactly-once delivery is not claimed. Receivers persist a bounded dedupe ledger before acknowledging durable processing. Acknowledgement means accepted by the receiver's durable inbox, not necessarily completed.

## 6. Storage layout

A Maple-owned workspace mesh directory contains:
- workspace identity manifest;
- peer presence records;
- append-only message segments;
- per-peer inbox/outbox cursors;
- reservation records;
- task coordination records;
- bounded diagnostic metadata.

Use atomic temp-write/rename for mutable manifests and append-only records for feeds. Directory permissions must restrict access to the current user where supported.

## 7. Liveness

Heartbeat cadence and expiry are configurable within safe bounds. Watchers improve latency; polling is the correctness fallback. A peer is `Suspect` after missed heartbeat threshold and `Expired` after lease timeout. Do not immediately treat a transient pause as permanent failure.

## 8. Reservations

Reservation fields:
- reservation ID;
- workspace-relative path or resource key;
- owner peer/process/session;
- creation time;
- lease expiry;
- heartbeat sequence;
- purpose;
- expected base digest.

Reservations are advisory coordination. File digest compare-and-swap remains the final correctness check. Expired reservations are reclaimable after lease validation.

## 9. Security

Peer messages are untrusted. Validate sender identity against workspace membership policy, schema, length, expiry, and path scope. Never execute a peer-provided command automatically. Mesh directories are not a secret store.

## 10. Backpressure and retention

Bound inbox bytes, feed segment size, pending messages, and per-peer outstanding deliveries. Slow peers cannot cause unbounded memory or disk growth. Retention may remove acknowledged expired messages only after required audit and dedupe windows.

## 11. Failure behavior

- watcher failure -> polling fallback;
- peer crash -> lease expiry;
- duplicate delivery -> dedupe;
- corrupt segment tail -> preserve valid prefix and quarantine tail;
- incompatible protocol -> reject with version diagnostic;
- disk full -> stop accepting new durable messages and surface degraded state.

## 12. Native protocol evolution

Protocol versions use explicit compatibility ranges. New optional fields may be added compatibly; changing message meaning requires a new version. Golden fixtures test old/new peers. Migration must not rewrite an active append-only feed in place.
