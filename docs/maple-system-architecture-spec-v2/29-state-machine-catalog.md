# 29 — State Machine Catalog

## 1. Session

`Created -> Initializing -> Ready -> Active -> Idle -> Closing -> Closed`

Failure paths: `InitializationFailed`, `RecoveryRequired`, `Failed`. Closed is terminal. Reopening creates a new process incarnation, not a mutation of process identity.

## 2. Turn

`Accepted -> ContextBuilding -> AwaitingProvider -> Interpreting -> AwaitingApproval? -> ToolRunning? -> Continuing -> Completed`

Terminal: `Cancelled`, `Failed`, `Aborted`, `Interrupted`. Exactly one terminal transition.

## 3. Tool invocation

`Proposed -> Validating -> Denied | AwaitingApproval | Admitted -> Running -> Succeeded | Failed | TimedOut | Cancelled | OutcomeUnknown`

`OutcomeUnknown` is required when cancellation/disconnect occurs after a side effect may have begun.

## 4. Peer

`Discovered -> Active -> Suspect -> Expired -> Retired`

A peer may return from Suspect to Active if a valid heartbeat arrives before expiry. Expired leases do not authorize new work.

## 5. Reservation

`Requested -> Granted -> Renewed* -> Released`

Alternative terminal states: `Expired`, `Revoked`, `Conflict`. Renewal must prove ownership and lease continuity.

## 6. Crew task

`Pending -> Ready -> Claimed -> Running -> Succeeded`

Alternatives: `Blocked`, `RetryWait`, `Failed`, `Cancelled`, `NeedsReview`, `Expired`.

## 7. Memory candidate

`Extracted -> Validating -> Redacted -> Deduplicated -> ReviewPending -> Approved -> Committed`

Alternatives: `Rejected`, `Expired`, `Superseded`. No direct `Extracted -> Committed` path.

## 8. Shutdown

`Running -> Quiescing -> Cancelling -> Reaping -> Flushing -> RestoringTerminal -> Stopped`

Failure to flush must not skip terminal restoration. A bounded emergency path records incomplete cleanup.
