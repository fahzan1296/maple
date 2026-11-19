# 05 — Runtime, Application Host, and Lifecycle

## 1. Runtime components

1. `Bootstrap`: validates arguments and constructs the dependency graph.
2. `ApplicationHost`: owns application services and coordinates shutdown.
3. `SessionSupervisor`: tracks session actors and process-wide resources.
4. `SessionActor`: serializes commands for one session.
5. `TurnCoordinator`: drives a single user/model/tool turn.
6. `TaskSupervisor`: owns all asynchronous tasks.
7. `ProcessSupervisor`: owns all child processes.
8. `TerminalLease`: guarantees exclusive terminal mode ownership.
9. `ShutdownCoordinator`: performs ordered bounded teardown.

## 2. Actor model

A session actor owns authoritative session lifecycle and turn sequencing. It receives commands over a bounded mailbox. Commands are validated before mutation. It emits domain events after successful state transitions.

Mailbox rules:
- bounded capacity;
- separate priority lane for cancellation/shutdown;
- reject or backpressure noncritical input when full;
- never block terminal rendering on provider I/O;
- preserve command correlation IDs;
- close mailbox during shutdown.

The actor must not hold a global lock while awaiting provider or filesystem operations. Long-running work is represented by task IDs and completion messages.

## 3. Turn lifecycle

`Accepted -> ContextPrepared -> RequestingModel -> AwaitingToolApproval? -> ExecutingTool? -> ContinuingModel? -> Completed`

Terminal alternatives: `Cancelled`, `Interrupted`, `Failed`, `AbortedByPolicy`.

A turn may include multiple provider/tool cycles. Each external side effect has its own idempotency and audit record. A turn is not considered complete merely because the provider stream ended; the response interpreter must classify the terminal event.

## 4. Cancellation

Cancellation is hierarchical:
- process shutdown token;
- session cancellation token;
- turn cancellation token;
- tool invocation token;
- child process termination token.

A child token may be cancelled by its parent, never the reverse. Cancellation must be checked before network calls, before tool execution, before commit, and during streaming waits.

Cancellation outcomes:
- before side effect: `CancelledNoEffect`;
- after side effect started: `CancelledEffectMayHaveOccurred`;
- after durable commit: `CancelledAfterCommit`.

Never report a guaranteed rollback if an external side effect cannot be reversed.

## 5. Partial initialization

Every bootstrap stage registers a cleanup action immediately after acquiring a resource. If a later stage fails, cleanup runs in reverse acquisition order. Cleanup errors are collected and reported without hiding the primary failure.

## 6. Reentrancy and nested work

The same session actor must not recursively process a new turn while waiting on a tool that calls back into the actor. Tool completions return as messages. Crew child sessions have independent actors and explicit parent/task references.

## 7. Runtime health

Health states: `Starting`, `Ready`, `Degraded`, `Stopping`, `Stopped`, `Failed`. Optional subsystem failures may produce `Degraded`, not `Failed`, when the core remains usable.

## 8. Runtime invariants

- every spawned task is supervised;
- every child process is registered;
- one active terminal lease per terminal device;
- one authoritative session actor per session;
- all queues have finite capacity;
- shutdown has a global deadline;
- no background task may write transcript state outside the owning service.
