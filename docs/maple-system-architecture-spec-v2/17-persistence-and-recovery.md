# 17 — Persistence, Blob Store, Indexes, and Recovery

## 1. Storage architecture

Canonical durable data:
- session manifest;
- append-only event journal;
- transcript payloads;
- content-addressed blob objects;
- memory records;
- crew ledger;
- optional mesh records.

Derived data:
- session list index;
- transcript lookup index;
- search index;
- token estimates;
- UI caches.

Derived data is rebuildable and never authoritative.

## 2. Journal format

JSONL is the baseline human-inspectable format. Each line is one versioned event envelope. Maximum record bytes and nesting depth are bounded. The writer serializes one complete record before append, preventing interleaved fragments from concurrent tasks.

## 3. Durability profiles

- `Fast`: buffered append; best effort on abrupt power loss.
- `Balanced`: flush at turn boundaries and critical mutations.
- `Durable`: sync critical records and parent directory where supported.

The selected profile is visible in diagnostics. No profile can guarantee survival of faulty hardware or filesystems that ignore sync semantics.

## 4. Blob store

Blob key is SHA-256 digest of exact bytes. Write:
1. compute digest;
2. write temp object;
3. flush/sync according to profile;
4. verify digest;
5. atomically publish under digest path;
6. update reference metadata.

Reads verify digest when required by integrity policy. Blob objects are immutable.

## 5. Reference tracking

References are discovered from durable manifests/journals. Reference indexes accelerate collection but are rebuildable. Garbage collection uses mark-and-sweep with a grace period and never deletes objects whose reachability is uncertain.

## 6. Crash recovery

- scan valid journal prefix;
- validate monotonic sequence;
- detect partial final record;
- quarantine invalid tail;
- rebuild projections;
- verify referenced blobs lazily or eagerly by policy;
- identify incomplete turns/tool invocations;
- mark uncertain side effects for review;
- emit recovery report.

## 7. Schema migrations

Migrations are explicit, versioned, and tested against fixtures from every supported schema version. Prefer additive migrations. Never perform destructive migration before a verified backup or recoverable copy exists. Migration is idempotent.

## 8. Atomic metadata

Mutable manifests use temp-write, flush, atomic rename, and parent-directory sync where supported. A failed rename leaves the previous manifest authoritative.

## 9. Disk pressure

When free space crosses warning threshold, reduce cache growth and warn. At critical threshold, reject new large writes and preserve journal integrity. Never delete authoritative data to make room automatically.

## 10. Integrity errors

Checksum mismatch, invalid schema, duplicate sequence, missing parent, or impossible reference is an integrity failure. Preserve evidence and avoid speculative repair that changes user data.
