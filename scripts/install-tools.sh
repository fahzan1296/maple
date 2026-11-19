#!/usr/bin/env bash
# Install the prebuilt dev tooling the justfile expects into ~/.local/bin.
# Downloads release binaries only — nothing is compiled, so this stays cheap on
# a memory-constrained host. Idempotent: existing binaries are left alone.
set -euo pipefail

BIN="${HOME}/.local/bin"
mkdir -p "$BIN"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

have() { command -v "$1" >/dev/null 2>&1; }

need() { ! have "$1"; }

from_tar() { # url, asset, member->dest
  local url="$1" asset="$2" member="$3" dest="$4"
  curl -sSfL "$url" -o "$asset"
  tar -xzf "$asset"
  install -m 755 "$member" "$BIN/$dest"
}

if need actionlint; then
  from_tar https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_arm64.tar.gz \
    a.tgz actionlint actionlint
fi

if need typos; then
  from_tar https://github.com/crate-ci/typos/releases/download/v1.52.2/typos-v1.52.2-aarch64-unknown-linux-musl.tar.gz \
    t.tgz typos typos
fi

if need just; then
  from_tar https://github.com/casey/just/releases/download/1.59.0/just-1.59.0-aarch64-unknown-linux-musl.tar.gz \
    j.tgz just just
fi

if need taplo; then
  curl -sSfL https://github.com/tamasfe/taplo/releases/download/0.10.0/taplo-linux-aarch64.gz -o taplo.gz
  gunzip -f taplo.gz
  install -m 755 taplo "$BIN/taplo"
fi

if need cargo-deny; then
  from_tar https://github.com/EmbarkStudios/cargo-deny/releases/download/0.21.0/cargo-deny-0.21.0-aarch64-unknown-linux-musl.tar.gz \
    d.tgz cargo-deny-0.21.0-aarch64-unknown-linux-musl/cargo-deny cargo-deny
fi

if need cargo-nextest; then
  from_tar https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.152/cargo-nextest-0.9.152-aarch64-unknown-linux-gnu.tar.gz \
    n.tgz cargo-nextest cargo-nextest
fi

echo "--- installed ---"
for t in actionlint typos just taplo cargo-deny cargo-nextest; do
  if have "$t"; then printf '%-14s %s\n' "$t" "$("$t" --version 2>&1 | head -1)"; else printf '%-14s ABSENT\n' "$t"; fi
done
