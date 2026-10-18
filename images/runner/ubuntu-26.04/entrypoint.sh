#!/bin/bash
# linux/amd64. One JIT payload from fd 3, or stdin. Never printed or placed in argv.
set +o xtrace
set +o allexport
set -euo pipefail

root="/home/runner"
work="${root}/_work"
listener="${root}/bin/Runner.Listener"
entrypoint_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# Use the conservative 4-KiB-page Linux MAX_ARG_STRLEN of 131072 bytes. An env
# entry includes its name, '=', and NUL, so reserve those bytes from the JIT.
jit_env_name='ACTIONS_RUNNER_INPUT_JITCONFIG'
max=$((131072 - ${#jit_env_name} - 2))

# Docker configuration must not carry JIT. The official listener consumes its
# supported ACTIONS_RUNNER_INPUT_JITCONFIG input from the environment at exec.
if [[ "${ACTIONS_RUNNER_INPUT_JITCONFIG+x}" ]]; then
  echo "jit must arrive on the private input channel" >&2
  exit 2
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
# Keep the payload in this shell until exec. Image ENV/ARG/labels and the
# listener's argv never carry it; the official runner input is a short-lived
# environment assignment for Runner.Listener only.
# DinD links the public socket after dockerd is ready. Wait before the listener.
/usr/local/bin/wait-docker-sock
/usr/local/bin/clear-dockerenv
# A present socket is not a serving API. Refuse to start on a stalled daemon.
# (/var/run/docker.sock and /run/docker.sock are the same file.)
"${entrypoint_dir}/velnor-wait-docker-api" "unix:///var/run/docker.sock"
ACTIONS_RUNNER_INPUT_JITCONFIG="$payload" exec "$listener" run
