#!/bin/bash
# Cargo and mbx read CARGO_BUILD_JOBS when -j is absent. The listener loads
# /home/runner/.env before the job. Half of nproc, at least one, so two slots
# do not each take every visible CPU.
set -euo pipefail
root="${RUNNER_ROOT:-/home/runner}"
env_file="${root}/.env"
cpus="$(nproc)"
if [[ ! "$cpus" =~ ^[0-9]+$ ]] || [[ "$cpus" -lt 1 ]]; then
  echo "nproc unusable" >&2
  exit 1
fi
jobs=$((cpus / 2))
if [[ "$jobs" -lt 1 ]]; then
  jobs=1
fi
mkdir -p "$root"
tmp="$(mktemp "${root}/.env.XXXXXX")"
if [[ -f "$env_file" ]]; then
  grep -v '^CARGO_BUILD_JOBS=' "$env_file" >"$tmp" || true
fi
printf 'CARGO_BUILD_JOBS=%s\n' "$jobs" >>"$tmp"
chmod 0644 "$tmp"
mv -f "$tmp" "$env_file"
