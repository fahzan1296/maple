# 15 — Crew Task Graph and Bounded Scheduler

## 1. Model

A crew is a set of typed tasks with dependencies represented as a directed acyclic graph. Each task may run in an isolated child session or a constrained execution context. The scheduler is local and bounded.

## 2. Task schema

- task ID;
- crew ID;
- description;
- input references;
- dependency IDs;
- required capabilities;
- workspace scope;
- resource class;
- retry policy;
- deadline;
- claim lease;
- status;
- output references;
- completion summary.

## 3. Validation

Before execution:
- ensure unique IDs;
- ensure all dependencies exist;
- reject cycles;
- enforce maximum graph nodes/edges/depth;
- validate input references;
- verify capability policy;
- estimate resource demand;
- ensure no forbidden workspace overlap.

## 4. Scheduler states

`Pending -> Ready -> Claimed -> Running -> Succeeded`

Alternatives: `Blocked`, `RetryWait`, `Failed`, `Cancelled`, `Expired`, `NeedsReview`.

A task is ready only when all dependencies succeeded or an explicit policy permits a degraded dependency result.

## 5. Claim leases

A claim is a time-bounded lease with owner peer/process, attempt number, and heartbeat. Lease expiry makes the task eligible for reassignment only after checking whether an external side effect may still be running. Reassignment must not duplicate non-idempotent work.

## 6. Concurrency

Limits:
- maximum active tasks globally;
- maximum active tasks per crew;
- maximum child processes;
- maximum provider concurrency;
- maximum concurrent writes per workspace;
- aggregate memory and CPU budget.

The scheduler obtains resource permits before starting a task. It releases permits in all terminal paths.

## 7. Failure policy

Retry only if the failure class is retryable and the task's idempotency contract permits it. A task with uncertain side effects transitions to `NeedsReview`, not automatic retry.

## 8. Result aggregation

Crew summary includes task outcomes, failed dependencies, partial completion, artifact references, and unresolved conflicts. It must not hide failures behind a single “success” status.

## 9. Cancellation

Canceling a crew prevents new claims and propagates cancellation to running tasks. Each task reports whether side effects were not started, may have occurred, or committed.

## 10. Verification

Property tests validate DAG ordering, no task starts before dependencies, no permit leaks, lease-expiry race handling, bounded concurrency, and stable recovery from scheduler restart.
