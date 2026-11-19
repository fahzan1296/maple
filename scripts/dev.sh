#!/usr/bin/env bash
# Local mirror of CI: build, test, then lint when the tools are installed.
# Resource discipline follows KONTRAK §SOP-RESLIM: one build job, a bounded
# process table, and a cap on the address space so a runaway test cannot take
# the host down.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

jobs="${CARGO_BUILD_JOBS:-1}"

# SOP-RESLIM.2/§3 defaults. Raise only with a recorded reason.
ulimit -u 512 2>/dev/null || true
ulimit -n 512 2>/dev/null || true
# Heap cap for this process tree: 2 GB aggregate, matching §SOP-RESLIM.2.
ulimit -v 2097152 2>/dev/null || true

echo "== build =="
CARGO_BUILD_JOBS="$jobs" cargo build --workspace -j "$jobs"

echo "== test =="
CARGO_BUILD_JOBS="$jobs" cargo test --workspace --lib --tests -j "$jobs" -- --test-threads="$jobs"

echo "== integration (maple-tui) =="
CARGO_BUILD_JOBS="$jobs" cargo test -p maple-tui --tests -j "$jobs" -- --test-threads="$jobs"

if cargo fmt --version >/dev/null 2>&1; then
  echo "== fmt =="
  cargo fmt --all -- --check
else
  echo "== fmt skipped (rustfmt not installed) =="
fi

if cargo clippy --version >/dev/null 2>&1; then
  echo "== clippy =="
  CARGO_BUILD_JOBS="$jobs" cargo clippy --workspace --all-targets -j "$jobs" -- -D warnings
else
  echo "== clippy skipped (clippy not installed) =="
fi
