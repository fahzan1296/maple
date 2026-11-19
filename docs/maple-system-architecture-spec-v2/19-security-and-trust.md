# 19 — Security, Trust Boundaries, and Threat Model

## 1. Assets

- user source files;
- credentials and provider tokens;
- session transcripts;
- memory and project knowledge;
- peer messages;
- tool execution authority;
- terminal integrity;
- local data directory;
- build/release artifacts.

## 2. Threat actors

- malicious repository content;
- prompt injection embedded in files or tool output;
- malicious or compromised peer process;
- malicious model output;
- compromised provider response;
- hostile local workspace files;
- accidental user misconfiguration;
- untrusted imported data.

## 3. Trust boundaries

1. User input -> application.
2. Model provider -> response interpreter.
3. Model tool request -> policy gate.
4. Workspace content -> context assembler.
5. Tool output -> transcript/context.
6. Peer message -> mesh receiver.
7. External import -> canonical domain.
8. Terminal output -> terminal emulator.
9. Configuration -> runtime policy.

Every boundary validates schema, size, provenance, and permissions.

## 4. Least privilege

Tools receive only capabilities required for the current invocation. Workspace writes are path-scoped. Network tools use host policy. Child processes receive minimal environment and working directory. Credentials are not exposed to model-visible content.

## 5. Prompt injection defense

Prompt wording alone is insufficient. Enforce through:
- provenance labels;
- tool policy independent of model;
- approval gates;
- path and host allowlists;
- capability scoping;
- bounded output;
- no execution of imported instructions;
- explicit user confirmation for high-impact actions.

## 6. Secret handling

Redact likely tokens, keys, passwords, and sensitive environment values before logging or memory extraction. Redaction is defense-in-depth, not a reason to allow arbitrary secret access. Secret resolver returns secret handles or short-lived material to adapters only.

## 7. Terminal safety

Escape control sequences in untrusted text before rendering. Never pass arbitrary provider output directly as terminal control bytes. Handle OSC, CSI, hyperlinks, and clipboard sequences through explicit safe rendering policy.

## 8. Local data protection

Create directories/files with restrictive permissions where supported. Avoid symlink-following in sensitive storage. Validate ownership and permissions before reading credential-bearing files.

## 9. Audit trail

Record policy decisions, approvals, tool invocations, file commits, peer actions, and import/export. Audit logs must be useful without recording secrets or full sensitive file content.

## 10. Incident response

On suspected compromise:
- stop new tool admission;
- revoke ephemeral capabilities;
- cancel child processes;
- preserve journals;
- mark affected peer/session state;
- avoid destructive cleanup;
- provide a diagnostic bundle with redaction.

## 11. Security acceptance

Security tests cover path escape, symlink races, terminal escape injection, secret leakage, tool approval bypass, replay, peer spoofing, malformed payloads, archive/path bombs, and imported-data trust escalation.
