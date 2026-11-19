# 26 — Performance, Capacity, and Benchmark Plan

## 1. Performance principles

Optimize measured bottlenecks, not architectural complexity. Bound memory first, preserve interactive responsiveness, and avoid duplicated representations of large payloads.

## 2. Memory strategy

- stream provider deltas;
- cap in-memory transcript viewport;
- spill large tool output to blobs;
- avoid cloning full transcript on each update;
- use immutable shared payload references where beneficial;
- bound caches and queue capacities;
- avoid retaining provider raw frames after normalization.

## 3. Latency budgets

Proposed targets, subject to hardware baseline:
- local input acknowledgement: under 50 ms at p95;
- TUI render update: under 16–33 ms for ordinary viewport changes;
- session journal append: under 20 ms p95 on normal local storage;
- context assembly: under 250 ms for bounded local context;
- cancellation acknowledgement: under 100 ms for cooperative tasks.

Provider latency is external and reported separately.

## 4. Benchmark workloads

- 10k transcript entries with long history;
- many short streaming deltas;
- large tool output spill;
- 1k file workspace scan;
- large file guarded edit;
- multiple concurrent peer sessions;
- crew DAG with many small tasks;
- low-memory process;
- cold startup and warm resume.

## 5. Measurements

Record wall time, CPU time, peak RSS, allocation count where feasible, bytes read/written, queue depth, render invalidations, provider tokens, and p50/p95/p99.

## 6. Regression policy

Benchmarks use stable fixtures and machine metadata. Avoid hard failing on noisy microbenchmarks; enforce broad regression thresholds on representative workloads. Performance claims must include hardware, build mode, workload, and sample count.

## 7. Capacity model

Capacity is a function of memory budget, provider concurrency, tool process count, queue capacity, blob throughput, and filesystem behavior. The scheduler must degrade gracefully rather than allow unbounded concurrency.
