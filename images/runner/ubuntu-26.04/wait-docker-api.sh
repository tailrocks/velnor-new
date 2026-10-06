#!/usr/bin/env bash
set -euo pipefail

readonly EXPECTED_INFO='vfs|/var/lib/docker'
readonly INFO_FORMAT='{{.Driver}}|{{.DockerRootDir}}'
readonly MAX_OVERALL_SECONDS=30
readonly MAX_PROBE_SECONDS=2

usage_error() {
  printf 'Docker API readiness: %s\n' "$1" >&2
  exit 2
}

check_socket() {
  case "$1" in
    unix:///*) ;;
    *) usage_error "expected an absolute private Unix socket URL" ;;
  esac
}

check_seconds() {
  local value="$1" maximum="$2"
  [[ "$value" =~ ^[1-9][0-9]?$ ]] && ((value <= maximum))
}

probe_loop() {
  local socket="$1" overall="$2" probe="$3" docker_cli="$4" timeout_cli="$5"
  local deadline=$((SECONDS + overall)) remaining call_seconds response

  while ((SECONDS < deadline)); do
    remaining=$((deadline - SECONDS))
    call_seconds="$probe"
    if ((remaining < call_seconds)); then
      call_seconds="$remaining"
    fi
    if ((call_seconds < 1)); then
      break
    fi
    if response="$("$timeout_cli" --signal=KILL "${call_seconds}s" env \
      -u DOCKER_HOST -u DOCKER_CONTEXT "$docker_cli" --host "$socket" \
      info --format "$INFO_FORMAT" 2>/dev/null)"; then
      if [[ "$response" == "$EXPECTED_INFO" ]]; then
        if ((SECONDS < deadline)); then
          return 0
        fi
        break
      fi
      printf 'Docker API readiness: unexpected info response at %s\n' "$socket" >&2
      return 65
    fi
    sleep 0.2
  done

  printf 'Docker API readiness timed out after %s seconds at %s\n' "$overall" "$socket" >&2
  return 124
}

if [[ "${1:-}" == "--probe-loop" ]]; then
  [[ "$#" -eq 6 ]] || usage_error "invalid internal probe arguments"
  socket="$2"
  overall="$3"
  probe="$4"
  docker_cli="$5"
  timeout_cli="$6"
  check_socket "$socket"
  check_seconds "$overall" "$MAX_OVERALL_SECONDS" \
    || usage_error "overall deadline must be between 1 and ${MAX_OVERALL_SECONDS} seconds"
  check_seconds "$probe" "$MAX_PROBE_SECONDS" \
    || usage_error "per-call timeout must be between 1 and ${MAX_PROBE_SECONDS} seconds"
  unset DOCKER_HOST DOCKER_CONTEXT
  probe_loop "$socket" "$overall" "$probe" "$docker_cli" "$timeout_cli"
  exit $?
fi

[[ "$#" -ge 1 && "$#" -le 3 ]] \
  || usage_error "usage: wait-docker-api.sh unix://SOCKET [overall-seconds] [probe-seconds]"
socket="$1"
overall="${2:-$MAX_OVERALL_SECONDS}"
probe="${3:-$MAX_PROBE_SECONDS}"
check_socket "$socket"
check_seconds "$overall" "$MAX_OVERALL_SECONDS" \
  || usage_error "overall deadline must be between 1 and ${MAX_OVERALL_SECONDS} seconds"
check_seconds "$probe" "$MAX_PROBE_SECONDS" \
  || usage_error "per-call timeout must be between 1 and ${MAX_PROBE_SECONDS} seconds"

docker_cli="$(command -v docker || true)"
[[ -n "$docker_cli" && -x "$docker_cli" ]] \
  || usage_error "Docker CLI is missing or not executable"
timeout_cli="$(command -v timeout || true)"
[[ -n "$timeout_cli" && -x "$timeout_cli" ]] \
  || usage_error "timeout utility is missing; refusing an unbounded API probe"

# Match `images/dind/Dockerfile` daemon.json: vfs driver, data-root /var/lib/docker.
unset DOCKER_HOST DOCKER_CONTEXT
script_path="${BASH_SOURCE[0]}"
if [[ "$script_path" != /* ]]; then
  script_path="$(cd -- "$(dirname -- "$script_path")" && pwd -P)/$(basename -- "$script_path")"
fi

if "$timeout_cli" --signal=KILL "${overall}s" "$script_path" --probe-loop \
  "$socket" "$overall" "$probe" "$docker_cli" "$timeout_cli"; then
  exit 0
else
  status="$?"
fi
if [[ "$status" -eq 124 || "$status" -eq 137 ]]; then
  printf 'Docker API readiness timed out after %s seconds at %s\n' "$overall" "$socket" >&2
else
  printf 'Docker API readiness probe failed at %s (status %s)\n' "$socket" "$status" >&2
fi
exit 1
