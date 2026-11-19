# 06 — Session, Transcript, Branching, and Turn Records

## 1. Session responsibilities

The session system owns session identity, lifecycle, active branch selection, model/config snapshot references, and session-level command sequencing. It does not own provider transport or terminal widgets.

## 2. Session creation transaction

1. Validate workspace and config.
2. Allocate collision-resistant session ID.
3. Create session directory using exclusive create.
4. Write manifest to temporary file.
5. Flush and atomically rename manifest.
6. Append `SessionCreated`.
7. Publish session in index.
8. Return session handle.

If any step fails, remove only files created by this attempt. Never delete a pre-existing session directory after an ID collision.

## 3. Transcript as an append-only graph

A transcript is a directed acyclic graph of entries. Each entry has at most one parent. Branching occurs when a new entry references an earlier committed entry instead of the current leaf. The original history remains intact.

A branch record contains:
- branch ID;
- parent branch/entry;
- creation timestamp;
- reason;
- current head;
- optional display label.

Branch head updates use compare-and-swap against the expected previous head. This prevents concurrent writers from silently overwriting one another.

## 4. Entry commit protocol

1. Validate entry envelope and payload size.
2. Resolve parent entry.
3. Allocate next sequence under session writer.
4. Persist payload/blob if needed.
5. Append journal record.
6. Flush according to durability policy.
7. Update derived index.
8. Publish event to subscribers.

If index update fails after journal commit, the journal remains authoritative and index rebuild repairs the discrepancy.

## 5. Turn boundaries

Each turn has explicit `TurnStarted` and exactly one terminal event:
- `TurnCompleted`
- `TurnCancelled`
- `TurnFailed`
- `TurnAborted`

Tool calls and results are child records correlated to the turn. A crash with no terminal event yields an interrupted turn during recovery. Recovery must not infer completion from the last assistant text chunk alone.

## 6. Resume policy

Resume loads the last valid journal sequence, rebuilds branch heads, and marks incomplete operations. It MUST NOT replay:
- shell commands;
- file writes;
- network mutations;
- peer sends;
- approvals;
- provider requests that may incur charges.

A user may explicitly retry an operation, producing a new invocation ID and a link to the interrupted attempt.

## 7. Retention and deletion

Deletion is explicit and scoped. Session deletion must account for referenced blobs. Shared blobs are garbage-collected only after reference reachability is computed across all durable manifests. A deleted session cannot cause deletion of a blob still referenced by another session.

## 8. Journal integrity

Each record has length framing or a newline-delimited canonical encoding with strict maximum record size. Recovery scans sequentially, validates schema and checksum, and stops at the first invalid/truncated tail. It preserves the valid prefix and writes a recovery diagnostic. It must not skip malformed middle records and continue as if the journal were contiguous.

## 9. Session invariants

- exactly one terminal event per completed turn;
- sequence numbers strictly increase;
- parent entries precede child entries;
- branch heads reference committed entries;
- session identity is immutable;
- recovery is read/repair, never implicit effect replay.
