#!/usr/bin/env bash
set -euo pipefail

if (($# < 2)) || [[ "$1" != "--" ]]; then
  printf 'usage: %s -- command [argument ...]\n' "$0" >&2
  exit 64
fi
shift

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
bash "$script_dir/build-owned-archive-guard.sh" >/dev/null
exec "$@"
