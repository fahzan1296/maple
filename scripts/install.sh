#!/usr/bin/env bash
# Build and install the maple binary.
#
#   scripts/install.sh              # -> ~/.local/bin/maple
#   scripts/install.sh --prefix DIR
#   scripts/install.sh --global     # -> /usr/local/bin/maple
set -euo pipefail

prefix="${PREFIX:-$HOME/.local/bin}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      [ "$#" -ge 2 ] || { echo "--prefix needs a value" >&2; exit 2; }
      prefix="$2"
      shift 2
      ;;
    --global)
      prefix="/usr/local/bin"
      shift
      ;;
    -h | --help)
      sed -n '2,6p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

jobs="${CARGO_BUILD_JOBS:-2}"
echo "building maple (jobs=$jobs)…"
CARGO_BUILD_JOBS="$jobs" cargo build --release

mkdir -p "$prefix"
if [ ! -w "$prefix" ]; then
  echo "no write access to $prefix; rerun with --prefix DIR or PREFIX=DIR" >&2
  exit 1
fi

install -m 0755 target/release/maple "$prefix/maple"
echo "installed $prefix/maple"

case ":$PATH:" in
  *":$prefix:"*) ;;
  *) echo "note: $prefix is not on PATH; add it or invoke the full path" >&2 ;;
esac
