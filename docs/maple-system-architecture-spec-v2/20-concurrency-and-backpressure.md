# 20 — Concurrency, Queues, and Backpressure

## 1. Concurrency model

Use asynchronous concurrency for network and coordination I/O, bounded worker pools for CPU work, and explicit process supervision for child processes. Do not use async tasks as a substitute for CPU isolation.

## 2. Queue inventory

Every queue must declare:
- owner;
- capacity;
- item size limit;
- priority;
- overflow behavior;
- cancellation behavior;
- metrics;
- shutdown behavior.

Required queues include session mailbox, provider event stream, tool dispatch, terminal input, terminal render invalidations, peer inbox, crew ready queue, background maintenance, and diagnostics.

## 3. Backpressure policy

- interactive input: preserve cancellation and control events; coalesce redundant resize/render events;
- provider stream: apply bounded buffering and cancel if consumer cannot keep up beyond policy;
- tool output: cap and spill to blob;
- peer messages: reject or defer when inbox is full;
- crew tasks: stop claiming when concurrency/resource budget is exhausted;
- logs: bounded ring buffer with dropped-count metric.

Never silently drop durable state transitions.

## 4. Lock discipline

- no lock held across network await;
- no global filesystem lock for unrelated workspaces;
- lock order documented where multiple locks are unavoidable;
- use per-session and per-path coordination;
- prefer immutable snapshots and compare-and-swap;
- avoid nested async mutexes in hot paths.

## 5. Deadlock prevention

Define lock hierarchy. Use timeouts only as detection/recovery, not as a substitute for correct ownership. Actor messages should replace callbacks that would re-enter a locked actor.

## 6. Fairness

Interactive session commands receive priority over background indexing. A continuously busy peer or crew cannot monopolize provider slots. Scheduler uses fair or weighted-fair admission where appropriate.

## 7. Shutdown

Close producers before consumers, drain durable events, cancel noncritical queues, and enforce a deadline. Queue closure must wake blocked senders/receivers.

## 8. Tests

Queue saturation, producer faster than consumer, cancellation during enqueue/dequeue, shutdown while blocked, lock-order stress, and starvation tests.
