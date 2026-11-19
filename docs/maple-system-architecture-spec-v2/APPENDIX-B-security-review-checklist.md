# Appendix B — Security Review Checklist

## Tool execution
- [ ] Tool names and schema versions are allowlisted.
- [ ] Arguments are bounded and typed.
- [ ] Approval binds exact canonical arguments.
- [ ] Capabilities are scoped and short-lived.
- [ ] Child process uses argv, explicit cwd, and minimal environment.
- [ ] Timeouts and process-tree cleanup are tested.
- [ ] Tool output is bounded and sanitized.

## Filesystem
- [ ] Workspace-relative paths are canonicalized.
- [ ] Parent traversal and symlink escapes are rejected.
- [ ] TOCTOU-sensitive operations use descriptor-relative APIs where available.
- [ ] Atomic writes preserve old file on failure.
- [ ] Special files are rejected by default.

## Persistence
- [ ] Secrets are absent from manifests and journals.
- [ ] Journal record sizes and nesting are bounded.
- [ ] Blob digest is verified.
- [ ] Recovery preserves valid prefix.
- [ ] Garbage collection is conservative.

## Peer mesh
- [ ] Peer messages are schema/size/expiry validated.
- [ ] Duplicate messages are deduplicated.
- [ ] Peer data cannot trigger automatic execution.
- [ ] Workspace identity is checked.
- [ ] Reservation leases cannot authorize stale file writes.

## TUI
- [ ] Untrusted control sequences are escaped.
- [ ] Paste size is bounded.
- [ ] Terminal restoration is tested.
- [ ] Diagnostic rendering does not reveal secrets.

## Optional integrations
- [ ] Adapter disabled by default.
- [ ] Core builds/tests without adapter.
- [ ] External content keeps untrusted provenance.
- [ ] Import cannot execute tools.
- [ ] Adapter failure cannot corrupt native storage.
