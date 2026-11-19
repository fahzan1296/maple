# 04 — Canonical Domain Model

## 1. Identity types

Use distinct newtypes; never interchange raw strings:
- `SessionId`
- `EntryId`
- `BranchId`
- `TurnId`
- `ToolCallId`
- `TaskId`
- `PeerId`
- `ProcessInstanceId`
- `WorkspaceId`
- `ReservationId`
- `BlobDigest`
- `ApprovalId`
- `MemoryItemId`
- `CorrelationId`

IDs are opaque, collision-resistant, serialized as canonical lowercase strings, and validated at boundaries. They do not encode user-controlled paths.

## 2. Session aggregate

Fields:
- immutable `session_id`;
- `created_at`;
- `workspace_id`;
- `working_directory` as validated path metadata;
- lifecycle state;
- active branch;
- model selection;
- configuration snapshot digest;
- event sequence;
- optional parent/continuation reference.

Mutable fields are changed only through session commands. A session does not own transcript entry bodies directly; it references transcript storage.

## 3. Transcript entry

Common envelope:
- `entry_id`
- `session_id`
- `branch_id`
- `parent_entry_id`
- `sequence`
- `timestamp`
- `kind`
- `schema_version`
- `payload_ref` or inline payload
- `provenance`
- `visibility`

Kinds include user message, assistant message, tool call, tool result, system notice, compaction summary, turn boundary, and error record.

Provenance identifies origin: user, model, tool, runtime, imported external data. Role and provenance are separate: a tool-originated message must not become trusted merely because it is represented in an assistant-visible transcript.

## 4. Workspace model

A workspace is a canonical root plus identity metadata:
- canonical root path;
- filesystem identity where available;
- stable workspace fingerprint;
- ignore policy;
- trust level;
- case-sensitivity behavior;
- symlink policy;
- active reservation namespace.

A path is represented as a normalized relative path inside a workspace wherever possible. Absolute paths are used only at OS adapter boundaries.

## 5. Tool invocation model

`ToolInvocation`:
- `tool_call_id`
- `tool_name`
- `schema_version`
- typed arguments
- requesting turn
- origin model/provider
- workspace scope
- requested capability set
- deadline
- idempotency key
- approval state

`ToolOutcome`:
- status;
- normalized result;
- bounded stdout/stderr references;
- exit code where relevant;
- duration;
- resource usage sample;
- artifact references;
- side-effect summary.

## 6. Trust and provenance

Every externally sourced string or document has a trust label and provenance. Trust labels are not authorization by themselves. They inform policy and rendering:
- trusted local configuration;
- user-authored content;
- project content;
- model-generated content;
- tool output;
- peer message;
- imported content;
- unknown origin.

Untrusted text must not be silently elevated into system policy or executable instructions.

## 7. Time model

Use monotonic time for deadlines, durations, leases, and timeout calculations. Use wall-clock UTC for durable timestamps and human-readable diagnostics. A clock rollback must not extend an already-running deadline.

## 8. Serialization

Canonical serialization must:
- include schema version;
- reject unknown required enum variants;
- preserve unknown optional fields only where forward-compatibility is explicitly supported;
- bound strings, arrays, nesting, and byte lengths;
- avoid serializing OS handles, secrets, or raw pointers;
- use explicit migrations for durable schema changes.

## 9. Domain invariants

- IDs are never reused within a durable store.
- A transcript parent must exist before child commit.
- A branch's active head must reference a committed entry.
- A task cannot be both running and terminal.
- A reservation owner must match its peer/session identity.
- A blob digest must match the bytes stored.
- An approval is scoped to one invocation and cannot authorize a different call.
- Imported data retains imported provenance.
