# 23 — Error Taxonomy, Logging, Metrics, and Diagnostics

## 1. Error envelope

- stable code;
- subsystem;
- severity;
- retryability;
- user action;
- correlation ID;
- redacted cause;
- optional structured details.

Errors should be actionable and stable across provider implementations.

## 2. Severity

`Trace`, `Debug`, `Info`, `Warn`, `Error`, `Fatal`. Fatal is reserved for conditions where core invariants cannot be maintained safely.

## 3. Logging

Structured logs include timestamp, level, subsystem, session/turn/task IDs where safe, correlation ID, and event name. Avoid full prompt bodies, secrets, full file contents, and unbounded tool output.

## 4. Metrics

Counters: turns, provider failures, tool denials, recovery events, peer messages, task retries.  
Gauges: RSS, queue depth, active tasks, child count, open descriptors, pending approvals.  
Histograms: provider latency, tool duration, render time, context build time, journal append latency.

Metrics labels must be cardinality-bounded; do not label by arbitrary path, prompt, or user text.

## 5. Tracing

Trace spans follow correlation across turn -> provider -> tool -> persistence. Never include secret-bearing arguments in span attributes.

## 6. Diagnostic bundle

A bundle may include version, platform, config schema status, redacted error summaries, storage health, and selected metrics. It must require user consent before export and must preview included files.

## 7. Error recovery matrix

| Class | Default response |
|---|---|
| validation | reject request, preserve state |
| policy denied | record denial, no execution |
| stale snapshot | require reread |
| provider transient | bounded retry if safe |
| provider permanent | fail turn with recovery guidance |
| disk full | stop new writes, preserve valid journal |
| corrupt tail | quarantine tail, recover valid prefix |
| peer unavailable | degrade collaboration only |
| resource exhausted | backpressure or reject admission |
| terminal failure | restore best effort, report |
| cancellation | record precise side-effect status |

## 8. Observability invariant

Diagnostics must not become a second unbounded persistence system. Every buffer, exporter queue, and diagnostic artifact has a size limit.
