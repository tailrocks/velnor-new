#!/bin/bash
# The helper removes only /.dockerenv. A missing file is success.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
echo marker >"$dir/.dockerenv"
echo keep >"$dir/keep"
export dir
sudo() {
  if [[ "$1" != "-n" || "$2" != "rm" || "$3" != "-f" || "$4" != "--" || "$5" != "/.dockerenv" ]]; then
    echo "unexpected sudo args" >&2
    exit 1
  fi
  rm -f -- "$dir/.dockerenv"
}
export -f sudo
bash "$root/clear-dockerenv.sh"
if [[ -e "$dir/.dockerenv" ]]; then
  echo "dockerenv remains" >&2
  exit 1
fi
test "$(cat "$dir/keep")" = keep
bash "$root/clear-dockerenv.sh"
echo "clear-dockerenv ok"
