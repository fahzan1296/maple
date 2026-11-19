# 09 — Agent Loop and Turn Orchestration

## 1. Responsibility split

- Session actor: owns session state and command order.
- Turn coordinator: coordinates one user turn.
- Context assembler: creates immutable context.
- Provider gateway: communicates with model.
- Response interpreter: classifies output.
- Tool planner: validates proposed calls.
- Tool platform: executes authorized calls.
- Transcript service: commits durable records.

## 2. Main algorithm

```text
accept user input
persist UserMessage
create TurnStarted
repeat while turn is active and iteration budget remains:
    build immutable context snapshot
    call provider with deadline and cancellation
    consume normalized stream events
    if provider completed with text:
        persist assistant output
        if terminal completion: finish turn
    if provider returned tool calls:
        validate each complete call
        apply policy and approval
        execute permitted calls under resource permits
        persist outcomes
        continue model loop
    if provider failed:
        persist typed failure and finish turn
if iteration budget exhausted:
    persist LoopLimitReached and require user continuation
```

## 3. Loop guards

Required bounds:
- maximum model/tool iterations per turn;
- maximum tool calls per response;
- maximum total tool calls per turn;
- maximum cumulative wall-clock duration;
- maximum cumulative provider tokens;
- maximum output bytes;
- maximum nested crew depth;
- maximum retries per provider request.

A loop guard is a hard runtime control, not merely a prompt instruction.

## 4. Parallel tool calls

Parallel execution is permitted only when:
- model capability supports it;
- each tool declares concurrency class;
- no conflicting workspace paths are touched;
- shared resource permits are available;
- result ordering is recorded deterministically.

Default concurrency is conservative. Mutating tools are serialized per conflicting resource unless a transaction protocol proves independence.

## 5. Tool call validation

Validation stages:
1. complete stream assembly;
2. tool name lookup;
3. schema version match;
4. argument size/depth limits;
5. typed deserialization;
6. semantic validation;
7. workspace/path validation;
8. trust and policy evaluation;
9. approval;
10. resource admission;
11. execution.

A failed stage produces a typed tool rejection record and no execution.

## 6. Completion semantics

A turn completes only when:
- model indicates terminal completion;
- all required tool calls have terminal outcomes;
- final assistant record is committed;
- session actor records terminal turn state.

Provider stream closure without a terminal event is an interruption, not success.

## 7. Determinism

The external world is nondeterministic. The orchestration decision sequence should still be reproducible from:
- canonical request snapshot;
- normalized provider events;
- policy snapshot;
- tool results;
- clock/timeout outcomes;
- recorded random seeds where randomness affects scheduling.

## 8. Safety and approval

Approval is bound to exact tool name, canonical argument digest, workspace, invocation ID, and expiry. Any argument mutation invalidates approval. Approval is not inherited by subsequent calls.

## 9. Failure handling

Provider failure preserves already committed messages and tool results. The turn becomes failed or interrupted with a resumable explanation. It must not silently erase partial work.
