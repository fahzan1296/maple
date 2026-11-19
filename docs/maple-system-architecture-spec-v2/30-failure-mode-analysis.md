# 30 — Failure Mode and Effects Analysis

| Failure | Detection | Containment | Recovery |
|---|---|---|---|
| provider disconnect | stream error/deadline | stop request, preserve partial output | retry only if safe |
| malformed provider frame | decoder validation | reject frame/request | fail turn with diagnostic |
| journal partial tail | framing/checksum | preserve valid prefix | quarantine tail |
| blob digest mismatch | verification | refuse content | restore from source or report loss |
| disk full | write error/free-space monitor | stop noncritical writes | free space; retry explicit operation |
| child process hangs | deadline/sampler | terminate process group/job | record exit/unknown effect |
| child process escapes scope | OS containment/audit | terminate if possible | security incident |
| stale file snapshot | digest mismatch | reject edit | reread and reapply |
| duplicate peer message | dedupe ledger | suppress duplicate | acknowledge prior receipt |
| peer crash | heartbeat/lease | expire ownership | reclaim safe leases |
| reservation race | CAS failure | reject one writer | reread/reconcile |
| crew task lost | lease expiry | mark uncertain | reassign only if idempotent |
| memory poisoning | provenance/policy | keep candidate untrusted | reject/review |
| secret leakage attempt | redaction/policy | block/log safely | rotate credential if exposed |
| terminal escape injection | sanitizer | render escaped text | report suspicious content |
| resource exhaustion | admission/monitor | throttle/cancel | resume after headroom |
| shutdown during tool | cancellation state | bounded process termination | mark side effect status |
| optional adapter crash | adapter supervisor | disable adapter | core remains available |

## Severity dimensions

- impact: user data, confidentiality, integrity, availability;
- likelihood: expected frequency;
- detectability: ease of detection;
- reversibility: ability to restore state;
- external side effect: whether action may have escaped process boundary.

High-impact failures require explicit tests and documented operational behavior.
