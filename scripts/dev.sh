#!/usr/bin/env bash
# Local mirror of CI: build, test, then lint when the tools are installed.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

jobs="${CARGO_BUILD_JOBS:-2}"

echo "== build =="
CARGO_BUILD_JOBS="$jobs" cargo build --workspace --all-targets -j "$jobs"

echo "== test =="
CARGO_BUILD_JOBS="$jobs" cargo test --workspace --all-targets -j "$jobs" -- --test-threads="$jobs"

if cargo fmt --version >/dev/null 2>&1; then
  echo "== fmt =="
  cargo fmt --all -- --check
else
  echo "== fmt skipped (rustfmt not installed) =="
fi

if cargo clippy --version >/dev/null 2>&1; then
  echo "== clippy =="
  CARGO_BUILD_JOBS="$jobs" cargo clippy --workspace --all-targets -j "$jobs"
else
  echo "== clippy skipped (clippy not installed) =="
fi
