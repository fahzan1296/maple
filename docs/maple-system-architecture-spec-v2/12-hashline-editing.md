# 12 — Hashline Editing Engine

## 1. Objective

Hashline editing is a guarded edit protocol. It reduces accidental edits against stale or unseen content by requiring stable line anchors and proof that the editor's view corresponds to the current file snapshot.

## 2. Snapshot model

An edit session begins with a snapshot:
- workspace-relative path;
- file digest;
- file identity;
- line-ending mode;
- encoding;
- line count;
- per-line anchor tokens;
- snapshot generation.

Line anchors are derived from line content plus contextual material and a collision-resistant digest. A short display token is only a lookup hint; the engine verifies the full anchor and source snapshot before mutation.

## 3. Seen-line proof

The read tool returns line content and anchor tokens. The edit request must identify the relevant snapshot and anchors. A line is considered seen only if its anchor was issued in a read result for that edit context. Guessing a line number or reusing an anchor from another file/snapshot is rejected.

## 4. Operation model

Supported operations should be small and explicit:
- replace anchored range;
- insert before/after anchor;
- delete anchored range;
- append;
- create file;
- rename only through a separate guarded operation.

Each operation includes expected old content digest or anchors, replacement bytes, and a bounded operation ID.

## 5. Apply algorithm

1. Resolve workspace path.
2. Load current snapshot.
3. Verify file identity and digest.
4. Verify all anchors belong to the issued read snapshot.
5. Resolve anchor positions uniquely.
6. Reject ambiguous or duplicate anchor matches.
7. Apply edits in memory.
8. Validate resulting size/encoding/line endings.
9. Re-check current file identity/digest immediately before commit.
10. Commit through atomic filesystem writer.
11. Record before/after digest and operation metadata.

## 6. Stale recovery

If the file changed:
- do not apply best-effort offsets;
- return `StaleSnapshot` with current digest and a safe re-read instruction;
- optionally produce a non-mutating diff preview;
- require a new read and new anchors.

No automatic fuzzy merge may silently commit. A future explicit merge tool may propose a patch but still requires review and a new guarded commit.

## 7. Conflict handling

A reservation reduces concurrent edits but does not replace digest validation. If two processes race, only one commit against a given expected digest succeeds. The loser re-reads and reconciles.

## 8. Binary and encoding behavior

Default Hashline operates on valid text. Binary data, undecodable content, mixed encodings, and ambiguous line endings require a specialized binary-safe tool or explicit conversion. No implicit lossy conversion.

## 9. Required error codes

`PathDenied`, `SnapshotExpired`, `StaleSnapshot`, `AnchorUnknown`, `AnchorAmbiguous`, `SeenProofMissing`, `RangeInvalid`, `EncodingUnsupported`, `FileTooLarge`, `ReservationConflict`, `CommitFailed`.

## 10. Verification

Property tests must establish that:
- stale snapshots never commit;
- unknown anchors never commit;
- operation order is deterministic;
- failed commits preserve original bytes;
- successful commits produce the expected digest;
- duplicate anchors are detected;
- concurrent commits against one digest have at most one winner.
