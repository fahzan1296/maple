# Appendix D — Glossary

| Term | Definition |
|---|---|
| Aggregate | Domain object whose invariants are changed transactionally |
| Adapter | Infrastructure implementation of a port |
| Append-only journal | Durable ordered event stream whose committed prefix is not rewritten |
| Branch | Transcript path rooted at a prior entry |
| Capability | Scoped authority to perform a class of operation |
| Compaction | Derived summary of prior context with source references |
| Context snapshot | Immutable assembled provider input |
| Crew | DAG of bounded cooperative tasks |
| Domain | Maple-owned types, rules, and invariants |
| Event | Fact that has already occurred |
| Hashline | Guarded line-edit protocol using snapshot anchors and seen proof |
| Idempotency key | Stable key used to prevent duplicate side effects |
| Interop | Optional translation at an external boundary |
| Lease | Time-bounded ownership claim |
| Maple Core | Required systems independent of external integrations |
| Peer | Another Maple process participating in a workspace mesh |
| Port | Technology-neutral interface for an external capability |
| Projection | Rebuildable read model derived from canonical events |
| Reservation | Advisory lease for a shared resource |
| Side effect | Observable change outside the current in-memory computation |
| Snapshot digest | Content identity of a file version |
| TUI | Terminal user interface |
| Trust label | Provenance classification, not an authorization grant |
| Turn | One user request and its model/tool orchestration cycle |
