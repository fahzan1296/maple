# ADR-0002 — Runtime resource guard (§SOP-RESLIM enforcement)

| | |
|---|---|
| Status | Accepted (design) |
| Date | 2026-11-15 |
| Deciders | IronWolf |
| Review date | 2026-12-15 |

## Context

`PRD.md` §11 makes resource discipline a product feature, not an agent
guideline: pre-flight with abort conditions, mandatory monitoring, abort at
80%, and an OOM protocol. `KONTRAK.md` §SOP-RESLIM defines the limits. Nothing
in the tree enforces them; an agent can simply ignore the contract.

## Decision

Add a **resource guard** that sits between the agent and any resource-consuming
operation. Deny by default.

Components:

- `preflight(scope)` — runs the §SOP-RESLIM.3 checks and returns
  `{ ok, aborts: [{ check, observed, threshold }] }`. `scope` ∈
  `default | test | build | coord` selects the limit set (256 / 512 / 768 MB,
  aggregate 2 GB, CPU `min(2, nproc/4)`, FD 512, processes 128, net 50).
- `spawn(cmd, limits)` — wraps every subprocess: `RLIMIT_AS` / `ulimit -v`,
  `timeout`, bounded CPU, FD and process limits; refuses to start on abort.
- `monitor(handle)` — for runs > 60 s, samples RSS/CPU/FD/disk every 5 s and
  kills at 80% of any limit; emits an abort record.
- `oom(record)` — on exit 137 / ENOMEM: stop, record, require diagnosis before
  retry (no blind retry).
- **Session budget** — renderer history and transcript rows are capped; large
  artifacts spill to the blob store by reference.

Limits come from config (`config/resource-guard.json`), are explicit, and are
logged with the command. There is **no bypass flag**; the only override is a
recorded coordination decision (§SOP-RESLIM.5) for high-resource work.

## Interfaces (draft)

```
guard.preflight(scope)        -> { ok: boolean, aborts: Abort[] }
guard.spawn(cmd, { scope, timeoutMs }) -> Handle
guard.monitor(handle)         -> Abort | null
```

## Failure design

Deny by default. A check that cannot run is an abort, not a pass. Abort records
are append-only (`resource-aborts.jsonl`) and mirrored to the feed.

## Negative tests (required before M3 ships)

1. swap > 0 → preflight aborts.
2. load average > nproc/2 → aborts.
3. available memory < 2× expected → aborts.
4. process exceeds 80% of its memory limit → killed, abort recorded.
5. command without a timeout → refused.
6. missing check (e.g. `free` unavailable) → abort, not pass.

## Environment caveat (proot/containers)

`/proc/loadavg` and `/proc/stat` are not guaranteed live: under proot they can
be static bind-mounted files (observed 2026-11-15: identical reads, mtime
2026-11-14, while `uptime` reported a different value). A guard that trusts
`uptime` alone can abort forever or miss real pressure. The guard must derive
CPU pressure from a live source (cgroup CPU deltas, `/proc/<pid>/stat` for its
own children, or a repeated `sysinfo` sample) and treat a stale/frozen load
file as "unknown" → abort (deny by default), not as "busy".

## Consequences

- Every `bash` / build / test path goes through the guard; added latency is one
  `free` + `uptime` sample (< 50 ms), acceptable.
- CI runs the same guard (§SOP-RESLIM.2 containers).
- Adds one small module; keep it inside the M3 size budget (SOP-MOD).

## Evidence

- `KONTRAK.md` §SOP-RESLIM.2–.7.
- Host observation 2026-11-15 06:48: load 17.96 / 18.96 / 19.09 on 8 cores,
  swap 2572 / 3575 MB used, available 1545 MB → M0 build correctly deferred.
- Follow-up 2026-11-15 07:03: `/proc/loadavg` = `0.12 0.07 0.02` (static, mtime
  2026-11-14) while `uptime` reported 21.73 — the load signal is stale/host-level
  under proot. Memory (`/proc/meminfo`) is live: available ≈ 1.0 GB. The guard
  must not trust `uptime` alone.
