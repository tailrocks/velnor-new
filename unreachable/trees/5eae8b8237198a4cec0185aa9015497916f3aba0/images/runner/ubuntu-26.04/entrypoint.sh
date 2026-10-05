#!/bin/bash
# linux/amd64. One JIT payload from fd 3, or stdin. Not printed. Not exported.
set +o xtrace
set +o allexport
set -euo pipefail

root="/home/runner"
work="${root}/_work"
listener="${root}/bin/Runner.Listener"
# Stay under Linux MAX_ARG_STRLEN so --jitconfig fits in one argv slot.
max=131071

if [[ ! -x "$listener" ]]; then
  echo "runner listener missing" >&2
  exit 1
fi

mkdir -p "$work"
umask 077
jit_file="$(mktemp "${work}/jit.XXXXXX")"
chmod 0600 "$jit_file"
trap 'rm -f "$jit_file"' EXIT

read_fd=0
if { : <&3; } 2>/dev/null && [[ ! -t 3 ]]; then
  read_fd=3
fi

# 33*4096 is past max, so an oversized body is rejected instead of passed on.
dd if="/dev/fd/${read_fd}" of="$jit_file" bs=4096 count=33 status=none
chmod 0600 "$jit_file"

size="$(stat -c '%s' "$jit_file")"
if [[ "$size" -eq 0 ]]; then
  echo "empty jit" >&2
  exit 2
fi
if [[ "$size" -gt "$max" ]]; then
  echo "jit too large" >&2
  exit 2
fi

payload="$(<"$jit_file")"
rm -f "$jit_file"
trap - EXIT
if [[ -z "$payload" ]]; then
  echo "empty jit" >&2
  exit 2
fi

cd "$root"
# Shell variable only. No export. Image ENV/ARG/labels never carry this.
exec "$listener" run --jitconfig "$payload"
