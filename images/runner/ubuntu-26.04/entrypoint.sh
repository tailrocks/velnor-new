#!/bin/bash
# linux/amd64. One JIT payload from fd 3, or stdin. Never printed or placed in argv.
set +o xtrace
set +o allexport
set -euo pipefail

root="/home/runner"
work="${root}/_work"
listener="${root}/bin/Runner.Listener"
entrypoint_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
home_initializer="/usr/local/libexec/velnor/runner-home.py"
docker_host='tcp://127.0.0.1:2375'
# Use the conservative 4-KiB-page Linux MAX_ARG_STRLEN of 131072 bytes. An env
# entry includes its name, '=', and NUL, so reserve those bytes from the JIT.
jit_env_name='ACTIONS_RUNNER_INPUT_JITCONFIG'
max=$((131072 - ${#jit_env_name} - 2))

# Docker configuration must not carry JIT. The official listener consumes its
# supported ACTIONS_RUNNER_INPUT_JITCONFIG input from the environment at exec.
if [[ "${ACTIONS_RUNNER_INPUT_JITCONFIG+x}" ]]; then
  echo "JIT input must arrive through the private input channel." >&2
  exit 2
fi

if [[ "${DOCKER_HOST:-}" != "$docker_host" ]]; then
  echo "The Runner requires its paired private DinD loopback endpoint." >&2
  exit 2
fi
export DOCKER_HOST="$docker_host"
unset DOCKER_CONTEXT

if [[ ! -r "$home_initializer" ]]; then
  echo "The program that copies the Runner seed into the Runner home volume is missing." >&2
  exit 1
fi
python3 "$home_initializer" initialize \
  --seed /opt/runner \
  --home "$root" \
  --manifest /usr/share/velnor/runner-home-manifest.json

if [[ ! -x "$listener" ]]; then
  echo "The Runner listener is missing." >&2
  exit 1
fi

if [[ ! -d "$work" ]]; then
  echo "The Work volume mount is missing." >&2
  exit 1
fi
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
  echo "The JIT input is empty." >&2
  exit 2
fi
if [[ "$size" -gt "$max" ]]; then
  echo "The JIT input is too large." >&2
  exit 2
fi

payload="$(<"$jit_file")"
rm -f "$jit_file"
trap - EXIT
if [[ -z "$payload" ]]; then
  echo "The JIT input is empty." >&2
  exit 2
fi

cd "$root"
# Keep the payload in this shell until exec.
# Do not put JIT in image ENV, ARG, or labels.
# The listener receives this input in its process environment at exec.
# Keep the payload out of the listener's command arguments.
# DinD links the public socket after dockerd is ready. Wait before the listener.
/usr/local/bin/wait-docker-sock
# A present socket is not a serving API. Refuse to start on a stalled daemon.
# Container actions keep using the private Unix socket mount. Runner processes
# use this job's loopback endpoint for services and Testcontainers.
"${entrypoint_dir}/velnor-wait-docker-api" "$docker_host"
# The host approval hook must keep its fixed path.
unset ACTIONS_RUNNER_CONTAINER_HOOKS
export ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER=true
export ACTIONS_RUNNER_HOOK_JOB_STARTED=/usr/local/bin/velnor-job-start-hook
ACTIONS_RUNNER_INPUT_JITCONFIG="$payload" exec "$listener" run
