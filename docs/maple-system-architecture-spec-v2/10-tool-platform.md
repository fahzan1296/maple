# 10 — Tool Platform, Capability Policy, and Process Execution

## 1. Tool model

A tool is a named, versioned capability with:
- input schema;
- output schema;
- read/write classification;
- required capabilities;
- concurrency class;
- resource estimate;
- timeout ceiling;
- approval requirement;
- idempotency behavior;
- audit classification.

## 2. Tool categories

- read-only workspace inspection;
- search and navigation;
- file mutation;
- patch/edit;
- shell/process execution;
- network access;
- memory operations;
- peer messaging;
- crew task control.

Each category has distinct policy defaults. A tool must not acquire broader capability merely because it is invoked by another tool.

## 3. Capability model

Capabilities are explicit tokens scoped to an invocation:
- `WorkspaceRead(root, paths)`
- `WorkspaceWrite(root, paths)`
- `ProcessSpawn(policy_profile)`
- `NetworkConnect(host_policy)`
- `PeerPublish(mesh_id)`
- `MemoryRead(scope)`
- `MemoryPropose(scope)`

Capabilities are issued by the policy gate, short-lived, and not serializable into model context.

## 4. Approval classes

| Class | Example | Default |
|---|---|---|
| read-only | list/read/search | auto-allow within workspace |
| reversible mutation | create/edit temp file | policy-dependent |
| persistent mutation | overwrite/delete | approval |
| process execution | shell/build/test | approval or strict allowlist |
| network mutation | publish/deploy | explicit approval |
| credential access | read secret material | deny to model/tool by default |

Approval UI must display normalized target, operation, and meaningful side-effect summary. Do not display only raw JSON arguments.

## 5. Process execution

Process tool contract:
- executable resolved under policy;
- arguments passed as argument vector, not shell-concatenated strings;
- explicit cwd;
- minimal environment;
- bounded stdin/stdout/stderr;
- deadline;
- process-group/job-object tracking where supported;
- graceful termination then forced termination;
- exit status and signal normalized;
- no orphaned child.

Shell interpretation is a separate high-risk capability and must be clearly distinguished from direct executable invocation.

## 6. Output bounding

Output handling:
- cap bytes in memory;
- spill large output to blob store;
- retain head/tail previews;
- mark truncation;
- preserve encoding validity;
- never drop the fact that truncation occurred.

## 7. Tool result contract

Result includes `status`, `summary`, structured data, artifact refs, side-effect classification, duration, exit code, and truncation metadata. Large content uses blob refs.

## 8. Idempotency

A tool declares one of:
- naturally idempotent;
- idempotent with key;
- non-idempotent;
- compensatable.

Non-idempotent calls require explicit retry semantics. Recovery never blindly re-executes them.

## 9. Security tests

Test path traversal, symlink escape, argument injection, environment leakage, output flooding, fork bombs within available containment, timeout behavior, cancellation races, approval substitution, and stale capability reuse.
