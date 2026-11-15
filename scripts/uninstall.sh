#!/usr/bin/env bash
# Remove a maple binary installed by scripts/install.sh.
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
      sed -n '2,2p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [ -f "$prefix/maple" ]; then
  rm -f "$prefix/maple"
  echo "removed $prefix/maple"
else
  echo "no maple binary in $prefix"
fi
