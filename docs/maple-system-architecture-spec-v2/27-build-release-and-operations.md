# 27 — Build, Release, and Operational Runbooks

## 1. Build profiles

- development: fast incremental compile, debug assertions;
- CI: reproducible locked dependency graph and all required tests;
- release: optimized binary, stripped according to platform policy;
- diagnostic: symbols and richer tracing without secret disclosure.

## 2. Reproducibility

Use lockfile, pinned toolchain policy, controlled build environment, and recorded build metadata. Release artifacts include version, commit, target triple, and feature set.

## 3. Release gates

- all mandatory tests pass;
- schema migrations tested;
- default build excludes optional Pi adapter;
- security review complete;
- platform matrix verified;
- release notes document breaking changes;
- rollback artifact available.

## 4. Upgrade behavior

On startup, detect schema version and run migration only after validation and backup/recovery preparation. Migration failure leaves old data readable where possible and does not start mutating operations.

## 5. Runbook: storage corruption

1. stop new writes;
2. preserve original directory;
3. copy diagnostic metadata;
4. scan valid journal prefix;
5. verify blob digests;
6. rebuild derived indexes;
7. produce recovery report;
8. require explicit action for destructive repair.

## 6. Runbook: provider outage

1. classify provider error;
2. stop unsafe retries;
3. preserve partial turn;
4. show provider status;
5. allow user to retry or switch provider;
6. do not discard committed tool results.

## 7. Runbook: peer conflict

1. stop conflicting write;
2. inspect reservation owner/lease;
3. read current file digest;
4. compare candidate changes;
5. require guarded rebase/re-read;
6. commit against new digest.

## 8. Runbook: resource pressure

1. stop new background admission;
2. cancel low-priority tasks;
3. terminate runaway child processes;
4. preserve journal;
5. report measured vs enforced limit;
6. resume only after headroom returns.

## 9. Support bundle

Include redacted version/platform/config schema/storage diagnostics and user-selected logs. Preview before export.
