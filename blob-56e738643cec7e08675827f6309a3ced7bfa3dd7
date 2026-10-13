#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
ENTRYPOINT="$REPO_ROOT/images/runner/ubuntu-26.04/entrypoint.sh"
HELPER="$REPO_ROOT/images/runner/ubuntu-26.04/wait-docker-api.sh"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/velnor-docker-readiness.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT

STUB_BIN="$TEST_DIR/stub-bin"
TEST_IMAGE_DIR="$TEST_DIR/runner-image"
TEST_RUNNER_ROOT="$TEST_DIR/runner"
STALLED_ENTRYPOINT="$TEST_IMAGE_DIR/velnor-runner-entrypoint-stalled"
SAVED_PATH="$PATH"
LISTENER_MARKER="$TEST_DIR/listener-started"
DOCKER_COUNT="$TEST_DIR/docker-count"
DOCKER_LOG="$TEST_DIR/docker.log"
API_LOG="$TEST_DIR/api.log"
TIMEOUT_LOG="$TEST_DIR/timeout.log"
TIMEOUT_EXPIRED_LOG="$TEST_DIR/timeout-expired.log"
TIMEOUT_PID_LOG="$TEST_DIR/timeout-pids.log"
TIMEOUT_ACTIVE_DIR="$TEST_DIR/active-timeouts"
STALLED_TEST_PID=
SOCKET_MARKER="$TEST_DIR/socket-present"
MISSING_DOCKER_BIN="$TEST_DIR/no-docker"
MISSING_TIMEOUT_BIN="$TEST_DIR/no-timeout"
PRODUCTION_SOCKET='unix:///var/run/docker.sock'

wait_recorded_children() {
  local process_id active active_file
  for _ in {1..60}; do
    active=0
    while IFS= read -r process_id; do
      [[ "$process_id" =~ ^[1-9][0-9]*$ ]] || continue
      if kill -0 "$process_id" 2>/dev/null; then active=1; fi
    done <"$TIMEOUT_PID_LOG"
    for active_file in "$TIMEOUT_ACTIVE_DIR"/*; do
      if [[ -f "$active_file" ]]; then active=1; fi
    done
    ((active == 0)) && return 0
    /bin/sleep 0.05
  done
  echo 'FAIL: timeout test child cleanup exceeded its 3 second bound' >&2
  return 1
}

cleanup_test_dir() {
  local active_file process_id
  if [[ -n "$STALLED_TEST_PID" ]]; then
    kill -TERM "$STALLED_TEST_PID" 2>/dev/null || true
    for _ in {1..20}; do
      if ! kill -0 "$STALLED_TEST_PID" 2>/dev/null; then
        break
      fi
      /bin/sleep 0.05
    done
    kill -KILL "$STALLED_TEST_PID" 2>/dev/null || true
    wait "$STALLED_TEST_PID" 2>/dev/null || true
  fi
  if [[ -d "$TIMEOUT_ACTIVE_DIR" ]]; then
    for active_file in "$TIMEOUT_ACTIVE_DIR"/[0-9]*; do
      [[ -f "$active_file" ]] || continue
      case "$active_file" in *.command|*.status) continue ;; esac
      IFS= read -r process_id <"$active_file" || continue
      if [[ "$process_id" =~ ^[1-9][0-9]*$ ]]; then
        kill -TERM "$process_id" 2>/dev/null || true
      fi
    done
  fi
  rm -rf "$TEST_DIR"
}
trap cleanup_test_dir EXIT

mkdir -p "$STUB_BIN" "$TEST_IMAGE_DIR" "$TEST_RUNNER_ROOT/bin" \
  "$MISSING_DOCKER_BIN" "$MISSING_TIMEOUT_BIN" "$TIMEOUT_ACTIVE_DIR"
touch "$SOCKET_MARKER"

cat >"$STUB_BIN/timeout" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$READINESS_TIMEOUT_LOG"
[[ "$1" == --signal=KILL && "$#" -ge 3 ]] || exit 125
if [[ "$READINESS_TIMEOUT_MODE" == expire ]]; then
  exit 137
fi
if [[ "$READINESS_TIMEOUT_MODE" == stall ]]; then
  timeout_arg="$2"
  timeout_seconds="${timeout_arg%s}"
  [[ "$timeout_seconds" =~ ^[1-9][0-9]?$ ]] || exit 125
  shift 2
  active_file="$READINESS_TIMEOUT_ACTIVE_DIR/$$"
  command_file="$active_file.command"
  status_file="$active_file.status"
  timeout_child=
  stop_timeout_child() {
    local attempt
    [[ -n "$timeout_child" ]] || return 0
    kill -TERM "$timeout_child" 2>/dev/null || true
    for attempt in {1..20}; do
      if ! kill -0 "$timeout_child" 2>/dev/null; then
        break
      fi
      /bin/sleep 0.05
    done
    kill -KILL "$timeout_child" 2>/dev/null || true
    wait "$timeout_child" 2>/dev/null || true
    timeout_child=
  }
  cleanup_timeout_child() {
    stop_timeout_child
    rm -f "$active_file" "$command_file" "$status_file"
  }
  (
    command_pid=
    cleanup_command() {
      if [[ -n "$command_pid" ]]; then
        kill -KILL "$command_pid" 2>/dev/null || true
        wait "$command_pid" 2>/dev/null || true
      fi
      rm -f "$command_file"
    }
    trap cleanup_command EXIT
    trap 'exit 143' HUP INT TERM
    "$@" <&0 &
    command_pid=$!
    printf '%s\n' "$command_pid" >>"$READINESS_TIMEOUT_PID_LOG"
    printf '%s\n' "$command_pid" >"$command_file"
    if wait "$command_pid"; then
      command_status=0
    else
      command_status=$?
    fi
    command_pid=
    rm -f "$command_file"
    printf '%s\n' "$command_status" >"$status_file"
    trap - EXIT HUP INT TERM
  ) <&0 &
  timeout_child=$!
  printf '%s\n' "$timeout_child" >>"$READINESS_TIMEOUT_PID_LOG"
  printf '%s\n' "$timeout_child" >"$active_file"
  trap cleanup_timeout_child EXIT
  trap 'exit 143' HUP INT TERM
  deadline=$((SECONDS + timeout_seconds))
  timed_out=0
  while [[ ! -f "$status_file" ]]; do
    if ((SECONDS >= deadline)); then
      timed_out=1
      printf '%s\n' "$timeout_arg" >>"$READINESS_TIMEOUT_EXPIRED_LOG"
      stop_timeout_child
      break
    fi
    /bin/sleep 0.05
  done
  if ((timed_out)); then
    exit 137
  fi
  if ! IFS= read -r command_status <"$status_file"; then
    exit 125
  fi
  [[ "$command_status" =~ ^[0-9]{1,3}$ ]] || exit 125
  wait "$timeout_child" 2>/dev/null || true
  timeout_child=
  cleanup_timeout_child
  trap - EXIT HUP INT TERM
  exit "$command_status"
fi
shift 2
exec "$@"
STUB

cat >"$STUB_BIN/sleep" <<'STUB'
#!/bin/sh
if [ "${READINESS_TIMEOUT_MODE:-}" = stall ]; then
  exec /bin/sleep "$@"
fi
exit 0
STUB

cat >"$STUB_BIN/dd" <<'STUB'
#!/bin/sh
output=
for argument in "$@"; do
  case "$argument" in
    of=*) output=${argument#of=} ;;
  esac
done
[ -n "$output" ] || exit 2
cat >"$output"
STUB

cat >"$STUB_BIN/stat" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -eq 3 && "$1" == -c && "$2" == %s && -f "$3" ]] || exit 2
size="$(wc -c <"$3")"
printf '%s\n' "$((size + 0))"
STUB

cat >"$STUB_BIN/docker" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$READINESS_DOCKER_LOG"
[[ -e "$READINESS_SOCKET_MARKER" ]] || exit 90
[[ ! ${DOCKER_HOST+x} && ! ${DOCKER_CONTEXT+x} ]] || exit 91
[[ "$#" -eq 5 && "$1" == --host && "$2" == "$READINESS_EXPECTED_SOCKET" \
  && "$3" == info && "$4" == --format \
  && "$5" == '{{.Driver}}|{{.DockerRootDir}}' ]] || exit 92

count=0
if [[ -f "$READINESS_DOCKER_COUNT" ]]; then
  read -r count <"$READINESS_DOCKER_COUNT"
fi
count=$((count + 1))
printf '%s\n' "$count" >"$READINESS_DOCKER_COUNT"
case "$READINESS_DOCKER_MODE" in
  delayed)
    if ((count < READINESS_READY_AFTER)); then
      printf 'API unavailable while socket path exists\n' >>"$READINESS_API_LOG"
      exit 1
    fi
    printf 'responsive API\n' >>"$READINESS_API_LOG"
    printf 'vfs|/var/lib/docker\n'
    ;;
  malformed)
    printf 'unexpected|response\n'
    ;;
  stalled)
    printf 'Docker API probe started and stalled\n' >>"$READINESS_API_LOG"
    exec /bin/sleep 30
    ;;
  *)
    exit 93
    ;;
esac
STUB

cat >"$TEST_RUNNER_ROOT/bin/Runner.Listener" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"$READINESS_LISTENER_MARKER"
STUB

chmod 0755 "$STUB_BIN/timeout" "$STUB_BIN/sleep" "$STUB_BIN/dd" \
  "$STUB_BIN/stat" "$STUB_BIN/docker" \
  "$TEST_RUNNER_ROOT/bin/Runner.Listener"
ln -s "$STUB_BIN/docker" "$MISSING_TIMEOUT_BIN/docker"
awk -v runner_root="$TEST_RUNNER_ROOT" '
  $0 == "root=\"/home/runner\"" {
    print "root=\"" runner_root "\""
    found = 1
    next
  }
  # The integrated entrypoint waits for the socket and clears the dockerenv
  # through absolute image paths this stub root cannot provide. Those steps
  # are orthogonal to the API-readiness gate under test; no-op them here.
  $0 == "/usr/local/bin/wait-docker-sock" { print ":"; next }
  $0 == "/usr/local/bin/clear-dockerenv" { print ":"; next }
  { print }
  END { if (!found) exit 1 }
' "$ENTRYPOINT" >"$TEST_IMAGE_DIR/velnor-runner-entrypoint"
cp "$HELPER" "$TEST_IMAGE_DIR/velnor-wait-docker-api"
chmod 0755 "$TEST_IMAGE_DIR/velnor-runner-entrypoint" \
  "$TEST_IMAGE_DIR/velnor-wait-docker-api"
awk '
  $0 == "\"${entrypoint_dir}/velnor-wait-docker-api\" \"unix:///var/run/docker.sock\"" {
    print "\"${entrypoint_dir}/velnor-wait-docker-api\" \"unix:///var/run/docker.sock\" 2 1"
    found = 1
    next
  }
  { print }
  END { if (!found) exit 1 }
' "$TEST_IMAGE_DIR/velnor-runner-entrypoint" >"$STALLED_ENTRYPOINT"
chmod 0755 "$STALLED_ENTRYPOINT"

helper_line="$(awk 'index($0, "velnor-wait-docker-api") { print NR; exit }' "$ENTRYPOINT")"
listener_line="$(awk 'index($0, "exec \"$listener\" run --jitconfig \"$payload\"") { print NR; exit }' "$ENTRYPOINT")"
if [[ -z "$helper_line" || -z "$listener_line" ]] || ((helper_line >= listener_line)); then
  echo 'FAIL: readiness helper must run before the listener' >&2
  exit 1
fi
grep -Fq 'set -euo pipefail' "$ENTRYPOINT"
grep -Fq '"unix:///var/run/docker.sock"' "$ENTRYPOINT"

run_entrypoint() {
  local name="$1" expected_status="$2" status=0
  shift 2
  rm -f "$LISTENER_MARKER"
  : >"$DOCKER_LOG"
  : >"$API_LOG"
  : >"$TIMEOUT_LOG"
  rm -f "$DOCKER_COUNT"
  if printf 'jit-secret-fixture' | env PATH="$STUB_BIN:$SAVED_PATH" \
    DOCKER_HOST='tcp://untrusted.invalid:2375' DOCKER_CONTEXT='untrusted-context' \
    READINESS_TIMEOUT_LOG="$TIMEOUT_LOG" READINESS_TIMEOUT_MODE="$1" \
    READINESS_DOCKER_LOG="$DOCKER_LOG" READINESS_DOCKER_COUNT="$DOCKER_COUNT" \
    READINESS_API_LOG="$API_LOG" \
    READINESS_SOCKET_MARKER="$SOCKET_MARKER" READINESS_EXPECTED_SOCKET="$PRODUCTION_SOCKET" \
    READINESS_DOCKER_MODE="$2" READINESS_READY_AFTER="${3:-3}" \
    READINESS_LISTENER_MARKER="$LISTENER_MARKER" \
    "$TEST_IMAGE_DIR/velnor-runner-entrypoint" >"$TEST_DIR/$name.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  if [[ "$status" -ne "$expected_status" ]]; then
    cat "$TEST_DIR/$name.log" >&2
    echo "FAIL: $name exited $status, expected $expected_status" >&2
    exit 1
  fi
}

run_stalled_entrypoint() {
  local status=0
  rm -f "$LISTENER_MARKER"
  : >"$DOCKER_LOG"
  : >"$API_LOG"
  : >"$TIMEOUT_LOG"
  : >"$TIMEOUT_EXPIRED_LOG"
  : >"$TIMEOUT_PID_LOG"
  rm -f "$DOCKER_COUNT"
  printf 'jit-secret-fixture' >"$TEST_DIR/stalled-jit"
  env PATH="$STUB_BIN:$SAVED_PATH" \
    DOCKER_HOST='tcp://untrusted.invalid:2375' DOCKER_CONTEXT='untrusted-context' \
    READINESS_TIMEOUT_MODE=stall READINESS_TIMEOUT_LOG="$TIMEOUT_LOG" \
    READINESS_TIMEOUT_EXPIRED_LOG="$TIMEOUT_EXPIRED_LOG" \
    READINESS_TIMEOUT_PID_LOG="$TIMEOUT_PID_LOG" \
    READINESS_TIMEOUT_ACTIVE_DIR="$TIMEOUT_ACTIVE_DIR" \
    READINESS_DOCKER_LOG="$DOCKER_LOG" READINESS_DOCKER_COUNT="$DOCKER_COUNT" \
    READINESS_API_LOG="$API_LOG" READINESS_SOCKET_MARKER="$SOCKET_MARKER" \
    READINESS_EXPECTED_SOCKET="$PRODUCTION_SOCKET" READINESS_DOCKER_MODE=stalled \
    READINESS_LISTENER_MARKER="$LISTENER_MARKER" \
    "$STUB_BIN/timeout" --signal=KILL 4s "$STALLED_ENTRYPOINT" \
    <"$TEST_DIR/stalled-jit" >"$TEST_DIR/stalled-api.log" 2>&1 &
  STALLED_TEST_PID=$!
  wait "$STALLED_TEST_PID" || status=$?
  STALLED_TEST_PID=

  if [[ "$status" -ne 1 ]]; then
    cat "$TEST_DIR/stalled-api.log" >&2
    echo "FAIL: stalled API exited $status, expected public helper timeout status 1" >&2
    exit 1
  fi
  [[ ! -e "$LISTENER_MARKER" ]] || {
    echo 'FAIL: listener started while the Docker API remained stalled' >&2
    exit 1
  }
  [[ -s "$DOCKER_COUNT" ]] || {
    echo 'FAIL: stalled API completed without a Docker probe' >&2
    exit 1
  }
  [[ "$(cat "$DOCKER_COUNT")" =~ ^[1-9][0-9]*$ ]] || {
    echo 'FAIL: stalled API recorded an invalid probe count' >&2
    exit 1
  }
  grep -Fq 'Docker API probe started and stalled' "$API_LOG"
  grep -Fq -- '--signal=KILL 2s ' "$TIMEOUT_LOG"
  grep -Fq "velnor-wait-docker-api --probe-loop $PRODUCTION_SOCKET 2 1 $STUB_BIN/docker $STUB_BIN/timeout" "$TIMEOUT_LOG"
  grep -Fq -- '--signal=KILL 1s' "$TIMEOUT_LOG"
  grep -Fqx '1s' "$TIMEOUT_EXPIRED_LOG"
  grep -Fqx '2s' "$TIMEOUT_EXPIRED_LOG"
  grep -Fq 'timed out after 2 seconds' "$TEST_DIR/stalled-api.log"
  wait_recorded_children
}

run_entrypoint api-becomes-ready 0 run delayed 3
[[ -f "$LISTENER_MARKER" ]] || {
  echo 'FAIL: listener did not start after the Docker API became ready' >&2
  exit 1
}
grep -Fq 'run --jitconfig jit-secret-fixture' "$LISTENER_MARKER"
[[ "$(cat "$DOCKER_COUNT")" == 3 ]]
grep -Fq 'API unavailable while socket path exists' "$API_LOG"
grep -Fq 'responsive API' "$API_LOG"
grep -Fq -- '--signal=KILL 30s' "$TIMEOUT_LOG"
grep -Fq -- '--signal=KILL 2s' "$TIMEOUT_LOG"
grep -Fq -- "--host $PRODUCTION_SOCKET info --format {{.Driver}}|{{.DockerRootDir}}" \
  "$DOCKER_LOG"

run_entrypoint malformed-api 1 run malformed
[[ ! -e "$LISTENER_MARKER" ]]
grep -Fq 'unexpected info response' "$TEST_DIR/malformed-api.log"

run_entrypoint readiness-timeout 1 expire delayed
[[ ! -e "$LISTENER_MARKER" ]]
grep -Fq 'timed out after 30 seconds' "$TEST_DIR/readiness-timeout.log"
[[ ! -s "$DOCKER_LOG" ]]

run_stalled_entrypoint

if PATH="$MISSING_DOCKER_BIN" /bin/bash "$HELPER" \
  "unix://$TEST_DIR/socket" >"$TEST_DIR/missing-docker.log" 2>&1; then
  echo 'FAIL: missing Docker CLI was accepted' >&2
  exit 1
fi
grep -Fq 'Docker CLI is missing or not executable' "$TEST_DIR/missing-docker.log"

if PATH="$MISSING_TIMEOUT_BIN" /bin/bash "$HELPER" \
  "unix://$TEST_DIR/socket" >"$TEST_DIR/missing-timeout.log" 2>&1; then
  echo 'FAIL: missing timeout utility was accepted' >&2
  exit 1
fi
grep -Fq 'timeout utility is missing' "$TEST_DIR/missing-timeout.log"

echo 'runner Docker API readiness tests passed'
