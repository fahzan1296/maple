# 18 — Resource Governance and Admission Control

## 1. Policy baseline

These are initial product budgets from the project requirements, not proven enforcement guarantees:

| Resource | Initial target |
|---|---:|
| default process RSS | 256 MiB |
| build/test process RSS | 512 MiB |
| uncoordinated process ceiling | 768 MiB |
| aggregate child RSS | 2 GiB |
| CPU allocation | min(2, nproc/4), with minimum practical fallback |
| file descriptors | 512 |
| child processes | 128 |
| network connections per process | 50 |
| free disk warning floor | max(20% free, 4 GiB) |
| normal network timeout | 15 s |
| long poll ceiling | 60 s |
| test timeout per file | 60 s |
| full test suite ceiling | 10 min |
| build ceiling | 5 min |
| migration ceiling | 20 min |
| long-operation sampling | every 5 s after 60 s |
| abort threshold | 80% of configured hard limit |

Actual values require benchmark and platform validation. CPU formulas must handle low-core systems safely; integer division must not produce an accidental zero-worker configuration.

## 2. Three-stage governance

### Admission
Estimate cost before work. Reject or defer when projected usage exceeds available budget.

### Execution
Issue permits for memory class, CPU slots, process slots, descriptors, network slots, and workspace write locks.

### Monitoring
Sample actual usage, detect runaway operations, warn, throttle, or abort according to policy.

## 3. Resource permit

A permit contains resource class, amount, owner operation ID, expiry/deadline, cancellation token, and release guard. Permits are RAII-managed and released on panic/unwind where Rust guarantees permit destructor execution.

## 4. Resource classes

- interactive latency-sensitive;
- provider/network;
- filesystem read;
- filesystem write;
- child process;
- build/test;
- crew task;
- background maintenance.

Interactive rendering receives reserved capacity so background work cannot starve terminal input.

## 5. OS enforcement levels

Distinguish:
- hard enforcement: OS primitive can prevent exceeding a limit;
- soft enforcement: monitor detects and terminates after threshold;
- advisory: sampled estimate only.

Linux cgroups/rlimits, macOS process APIs, and Windows Job Objects have different semantics. The design must expose enforcement level in diagnostics. Never claim a hard cap where only sampling exists.

## 6. Abort policy

Abort order:
1. stop admission;
2. cancel background tasks;
3. cancel lower-priority work;
4. request child termination;
5. force terminate after grace period;
6. preserve journals and release permits.

Do not terminate the entire Maple process solely because a noncritical cache exceeds its target.

## 7. Resource accounting

Track current, peak, and cumulative:
- RSS;
- CPU time;
- open descriptors;
- child count;
- network connections;
- queue depth;
- blob bytes;
- journal bytes;
- provider tokens;
- tool duration.

Sampling overhead itself must be bounded.

## 8. Verification

Stress tests must include low-memory conditions, process storms, descriptor exhaustion, output flooding, disk full, slow provider, cancellation storms, and multiple peer processes.
