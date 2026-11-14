#!/bin/bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set +o xtrace
set +o allexport
set -euo pipefail

root="/home/runner"
work="${root}/_work"
listener="${root}/bin/Runner.Listener"
entrypoint_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
jit_env_name='ACTIONS_RUNNER_INPUT_JITCONFIG'
max=$((131072 - ${#jit_env_name} - 2))

if [[ "${ACTIONS_RUNNER_INPUT_JITCONFIG+x}" ]]; then
  echo "jit must arrive through private input" >&2
  exit 2
fi

if [[ ! -x "$listener" && -x /opt/runner/bin/Runner.Listener ]]; then
  cp -a /opt/runner/. "$root/"
fi

if [[ ! -x "$listener" ]]; then
  echo "runner listener missing" >&2
  exit 1
fi

mkdir -p "$work"
umask 077
jit_file="$(mktemp /tmp/velnor-jit.XXXXXX)"
chmod 0600 "$jit_file"
trap 'rm -f "$jit_file"' EXIT

read_fd=0
if { : <&3; } 2>/dev/null && [[ ! -t 3 ]]; then
  read_fd=3
fi

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
/usr/local/bin/wait-docker-sock
/usr/local/bin/clear-dockerenv
"${entrypoint_dir}/velnor-wait-docker-api" "unix:///var/run/docker.sock"
umask 022
export ACTIONS_RUNNER_HOOK_JOB_STARTED="/usr/local/bin/velnor-job-start-hook.sh"
ACTIONS_RUNNER_INPUT_JITCONFIG="$payload" exec "$listener" run
