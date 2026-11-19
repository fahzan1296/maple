# Legacy V1 reference

This file is preserved for comparison only. Where it conflicts with V2, V2 is authoritative. In particular, `.pi` compatibility is optional and Maple-native behavior is canonical.

# 04 — Complete system and subsystem encyclopedia

This is a logical decomposition, not a mandate for one crate per item.

## A. Application host
A1 Bootstrap coordinator: resolves CLI/config/cwd, constructs services, unwinds partial initialization.  
A2 CLI router: validates command/argument bounds; delegates to shared application services.  
A3 Lifecycle supervisor: tracks active tasks, child processes, terminal ownership, mesh presence and shutdown hooks.

## B. Session runtime
B1 Identity manager: immutable session ID, mutable display name, PID/process marker, cwd/model.  
B2 Session event loop: serializes authoritative state transitions through a bounded mailbox.  
B3 Turn coordinator: context snapshot, provider call, tool dispatch, terminal outcome.  
B4 Lifecycle state: created/ready/running/cancelling/interrupted/completed/aborted/failed.

## C. Conversation/context
C1 Typed message model with provenance separate from role.  
C2 Transcript tree: append-only entries, parent links, active leaf.  
C3 Context assembler: system contract, user messages, history, tools, memory and workspace context under token budgets.  
C4 Compaction: derived summary with source references; never deletes original history.

## D. Agent engine
D1 Prompt planner builds immutable canonical request.  
D2 Model executor streams normalized events and handles deadline/cancellation.  
D3 Response interpreter distinguishes text, tool call, refusal and completion.  
D4 Tool planner validates schema, policy, reservation and resource estimate.

## E. Provider gateway
E1 Provider registry; E2 credential resolver; E3 model capability catalog; E4 request adapter; E5 bounded transport; E6 stream decoder/normalizer; E7 usage normalizer; E8 typed error mapper.

## F. Tool platform
F1 Tool registry; F2 policy gate; F3 read tools; F4 workspace mutation tools; F5 shell/process tool; F6 result normalizer; F7 approval broker; F8 artifact spill adapter.

## G. Editing engine
G1 Snapshot reader; G2 content hash/anchor resolver; G3 operation parser/interpreter; G4 seen-line verifier; G5 stale recovery; G6 reservation check; G7 atomic commit writer.

## H. Mesh
H1 Peer registry; H2 inbox delivery; H3 append-only feed; H4 lock manager; H5 heartbeat/liveness; H6 reservation service; H7 team memory; H8 watcher/poll fallback.

## I. Crew
I1 Task schema; I2 DAG validator; I3 wave scheduler; I4 claim lease; I5 completion ledger; I6 reassignment/handoff protocol.

## J. Memory
J1 Session lesson extractor; J2 project/global lesson store; J3 skill manager; J4 two-phase consolidator; J5 bounded retriever; J6 secret redactor/provenance validator.

## K. Persistence
K1 JSONL session store; K2 SHA-256 blob store; K3 rebuildable metadata index; K4 atomic file utility; K5 schema migration/repair; K6 conservative garbage collector.

## L. Governance
L1 Resource sampler; L2 preflight engine; L3 permit manager; L4 process-tree monitor; L5 abort coordinator; L6 trust classifier; L7 secret redactor; L8 audit recorder.

## M. Presentation
M1 TUI toolkit; M2 application view models; M3 semantic input mapper; M4 terminal session manager; M5 Agent Hub; M6 transcript/composer/tool cards.

## N. Operations
N1 structured diagnostics; N2 metrics/traces; N3 virtual terminal/fake provider test harness; N4 CI/release; N5 runbooks/ADR/schema governance.

## Ownership rule
A system owns its state machine and invariants. A crate is only a packaging boundary. Session, transcript and context can be logical systems inside one crate if that preserves clear ownership.
