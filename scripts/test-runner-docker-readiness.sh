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
STATUS_PAUSE_MARKER="$TEST_DIR/status-pause"
STATUS_WAIT_LOG="$TEST_DIR/status-wait.log"
STATUS_RELEASE_MARKER="$TEST_DIR/status-release"
STALLED_TEST_PID=
SOCKET_MARKER="$TEST_DIR/socket-present"
MISSING_DOCKER_BIN="$TEST_DIR/no-docker"
MISSING_TIMEOUT_BIN="$TEST_DIR/no-timeout"
PRODUCTION_SOCKET='unix:///var/run/docker.sock'

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}
stop_pid() {
  local process_id="$1" wait_child="${2:-yes}" wait_status=0 signal_failed=0
  if ! kill -TERM "$process_id" 2>/dev/null && kill -0 "$process_id" 2>/dev/null; then
    signal_failed=1
  fi
  for _ in {1..20}; do
    kill -0 "$process_id" 2>/dev/null || break
    /bin/sleep 0.05
  done
  if kill -0 "$process_id" 2>/dev/null \
    && ! kill -KILL "$process_id" 2>/dev/null && kill -0 "$process_id" 2>/dev/null; then
    signal_failed=1
  fi
  if [[ "$wait_child" == yes ]]; then
    wait "$process_id" 2>/dev/null || wait_status=$?
  fi
  ((wait_status != 127 && signal_failed == 0)) && ! kill -0 "$process_id" 2>/dev/null
}
export -f stop_pid
wait_recorded_children() {
  local process_id active
  for _ in {1..60}; do
    active=0
    while IFS= read -r process_id; do
      [[ "$process_id" =~ ^[1-9][0-9]*$ ]] || continue
      if kill -0 "$process_id" 2>/dev/null; then active=1; fi
    done <"$TIMEOUT_PID_LOG"
    compgen -G "$TIMEOUT_ACTIVE_DIR/*" >/dev/null && active=1
    ((active == 0)) && return 0
    /bin/sleep 0.05
  done
  printf 'FAIL: timeout test child cleanup exceeded its 3 second bound\n' >&2
  return 1
}

cleanup_test_dir() {
  local cleanup_root="$1" record_export="${2:-}" process_id signal_failed=0
  if [[ -n "$STALLED_TEST_PID" ]] && ! stop_pid "$STALLED_TEST_PID"; then
    signal_failed=1
  fi
  while IFS= read -r process_id; do
    [[ "$process_id" =~ ^[1-9][0-9]*$ ]] || continue
    stop_pid "$process_id" no || signal_failed=1
  done <"$TIMEOUT_PID_LOG"
  ((signal_failed == 0)) || fail 'failed to signal or reap a recorded test child'
  rm -rf "$TIMEOUT_ACTIVE_DIR"
  if [[ -n "$record_export" ]]; then
    cp "$TIMEOUT_PID_LOG" "$record_export"
  fi
  rm -rf "$cleanup_root"
}
trap 'cleanup_test_dir "$TEST_DIR"' EXIT

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
  status_file="$active_file.status"
  status_temp_file="$status_file.tmp"
  timeout_child=
  printf '%s\n' "$$" >>"$READINESS_TIMEOUT_PID_LOG"
  stop_timeout_child() {
    [[ -n "$timeout_child" ]] || return 0
    stop_pid "$timeout_child"
    timeout_child=
  }
  cleanup_timeout_child() {
    stop_timeout_child
    rm -f "$active_file" "$status_file" "$status_temp_file"
  }
  (
    command_pid=
    cleanup_command() {
      [[ -z "$command_pid" ]] || stop_pid "$command_pid"
    }
    trap cleanup_command EXIT
    trap 'exit 143' HUP INT TERM
    "$@" <&0 &
    command_pid=$!
    printf '%s\n' "$command_pid" >>"$READINESS_TIMEOUT_PID_LOG"
    command_status=0
    wait "$command_pid" || command_status=$?
    command_pid=
    printf '%s\n' "$command_status" >"$status_temp_file"
    if [[ "${READINESS_STATUS_PUBLISH_PAUSE_FOR:-}" == "$timeout_arg" ]]; then
      : >"$READINESS_STATUS_PAUSE_MARKER"
      for _ in {1..60}; do
        [[ -e "$READINESS_STATUS_RELEASE_MARKER" ]] && break
        /bin/sleep 0.05
      done
      [[ -e "$READINESS_STATUS_RELEASE_MARKER" ]] || exit 125
    fi
    mv "$status_temp_file" "$status_file"
  ) <&0 &
  timeout_child=$!
  printf '%s\n' "$timeout_child" >>"$READINESS_TIMEOUT_PID_LOG"
  printf '%s\n' "$timeout_child" >"$active_file"
  trap cleanup_timeout_child EXIT
  trap 'exit 143' HUP INT TERM
  deadline=$((SECONDS + timeout_seconds))
  while [[ ! -f "$status_file" ]] && ((SECONDS < deadline)); do
  if [[ -f "$status_temp_file" && -e "$READINESS_STATUS_PAUSE_MARKER" \
    && -n "${READINESS_STATUS_WAIT_LOG:-}" ]]; then
      [[ ! -e "$status_file" ]] || exit 125
      : >"$READINESS_STATUS_WAIT_LOG"
      : >"$READINESS_STATUS_RELEASE_MARKER"
    fi
    /bin/sleep 0.05
  done
  if [[ ! -f "$status_file" ]]; then
    printf '%s\n' "$timeout_arg" >>"$READINESS_TIMEOUT_EXPIRED_LOG"
    stop_timeout_child
    exit 137
  fi
  IFS= read -r command_status <"$status_file"
  wait "$timeout_child" 2>/dev/null || true
  timeout_child=
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
  fail 'readiness helper must run before the listener'
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
  printf 'jit-secret-fixture' | env PATH="$STUB_BIN:$SAVED_PATH" \
    DOCKER_HOST='tcp://untrusted.invalid:2375' DOCKER_CONTEXT='untrusted-context' \
    READINESS_TIMEOUT_LOG="$TIMEOUT_LOG" READINESS_TIMEOUT_MODE="$1" \
    READINESS_DOCKER_LOG="$DOCKER_LOG" READINESS_DOCKER_COUNT="$DOCKER_COUNT" \
    READINESS_API_LOG="$API_LOG" \
    READINESS_SOCKET_MARKER="$SOCKET_MARKER" READINESS_EXPECTED_SOCKET="$PRODUCTION_SOCKET" \
    READINESS_DOCKER_MODE="$2" READINESS_READY_AFTER="${3:-3}" \
    READINESS_LISTENER_MARKER="$LISTENER_MARKER" \
    "$TEST_IMAGE_DIR/velnor-runner-entrypoint" >"$TEST_DIR/$name.log" 2>&1 || status=$?
  if [[ "$status" -ne "$expected_status" ]]; then
    cat "$TEST_DIR/$name.log" >&2
    fail "$name exited $status, expected $expected_status"
  fi
}
start_stalled_command() {
  local watchdog="$1" output="$2"
  shift 2
  env PATH="$STUB_BIN:$SAVED_PATH" \
    DOCKER_HOST='tcp://untrusted.invalid:2375' DOCKER_CONTEXT='untrusted-context' \
    READINESS_TIMEOUT_MODE=stall READINESS_TIMEOUT_LOG="$TIMEOUT_LOG" \
    READINESS_TIMEOUT_EXPIRED_LOG="$TIMEOUT_EXPIRED_LOG" \
    READINESS_TIMEOUT_PID_LOG="$TIMEOUT_PID_LOG" \
    READINESS_TIMEOUT_ACTIVE_DIR="$TIMEOUT_ACTIVE_DIR" \
    READINESS_STATUS_PUBLISH_PAUSE_FOR=8s \
    READINESS_STATUS_PAUSE_MARKER="$STATUS_PAUSE_MARKER" \
    READINESS_STATUS_WAIT_LOG="$STATUS_WAIT_LOG" READINESS_STATUS_RELEASE_MARKER="$STATUS_RELEASE_MARKER" \
    READINESS_DOCKER_LOG="$DOCKER_LOG" READINESS_DOCKER_COUNT="$DOCKER_COUNT" \
    READINESS_API_LOG="$API_LOG" READINESS_SOCKET_MARKER="$SOCKET_MARKER" \
    READINESS_EXPECTED_SOCKET="$PRODUCTION_SOCKET" READINESS_DOCKER_MODE=stalled \
    READINESS_LISTENER_MARKER="$LISTENER_MARKER" \
    "$STUB_BIN/timeout" --signal=KILL "$watchdog" "$@" \
    <"$TEST_DIR/stalled-jit" >"$output" 2>&1 &
  STALLED_TEST_PID=$!
}

run_stalled_entrypoint() {
  local status=0 force_failure="${1:-0}" watchdog=8s
  rm -f "$LISTENER_MARKER"
  : >"$DOCKER_LOG"
  : >"$API_LOG"
  : >"$TIMEOUT_LOG"
  : >"$TIMEOUT_EXPIRED_LOG"
  : >"$TIMEOUT_PID_LOG"
  rm -f "$STATUS_PAUSE_MARKER" "$STATUS_WAIT_LOG" "$STATUS_RELEASE_MARKER" "$DOCKER_COUNT"
  printf 'jit-secret-fixture' >"$TEST_DIR/stalled-jit"
  if [[ "$force_failure" == 1 ]]; then watchdog=20s; fi
  start_stalled_command "$watchdog" "$TEST_DIR/stalled-api.log" "$STALLED_ENTRYPOINT"
  if [[ "$force_failure" == 1 ]]; then
    for _ in {1..60}; do
      if grep -Fq 'Docker API probe started and stalled' "$API_LOG"; then
        exit 73
      fi
      /bin/sleep 0.05
    done
    exit 74
  fi
  wait "$STALLED_TEST_PID" || status=$?
  STALLED_TEST_PID=
  if [[ "$status" -ne 1 ]]; then
    cat "$TEST_DIR/stalled-api.log" >&2
    fail "stalled API exited $status, expected public helper timeout status 1"
  fi
  [[ ! -e "$LISTENER_MARKER" ]] || fail 'listener started while the Docker API remained stalled'
  [[ "$(cat "$DOCKER_COUNT")" =~ ^[1-9][0-9]*$ ]] || fail 'stalled API completed without a valid Docker probe'
  grep -Fq 'Docker API probe started and stalled' "$API_LOG"
  grep -Fq -- '--signal=KILL 2s ' "$TIMEOUT_LOG"
  grep -Fq "velnor-wait-docker-api --probe-loop $PRODUCTION_SOCKET 2 1 $STUB_BIN/docker $STUB_BIN/timeout" "$TIMEOUT_LOG"
  grep -Fq -- '--signal=KILL 1s' "$TIMEOUT_LOG"
  grep -Fqx '1s' "$TIMEOUT_EXPIRED_LOG"
  grep -Fqx '2s' "$TIMEOUT_EXPIRED_LOG"
  grep -Fq 'timed out after 2 seconds' "$TEST_DIR/stalled-api.log"
  [[ -e "$STATUS_PAUSE_MARKER" && -e "$STATUS_WAIT_LOG" ]]
  wait_recorded_children
}
run_forced_failure_cleanup_case() {
  local status=0 failure_dir="$TEST_DIR/forced-failure" pid_export="$TEST_DIR/failure-pids" process_id
  (
    TIMEOUT_ACTIVE_DIR="$failure_dir/active-timeouts"
    TIMEOUT_PID_LOG="$failure_dir/timeout-pids.log"
    mkdir -p "$TIMEOUT_ACTIVE_DIR"
    trap 'cleanup_test_dir "$failure_dir" "$pid_export"' EXIT
    run_stalled_entrypoint 1
  ) || status=$?
  ((status == 73)) || fail "forced cleanup scenario exited $status, expected 73"
  [[ -s "$pid_export" ]] || fail 'cleanup did not preserve its drained child PID record'
  while IFS= read -r process_id; do
    [[ "$process_id" =~ ^[1-9][0-9]*$ ]] || fail 'cleanup exported an invalid child PID'
    ! kill -0 "$process_id" 2>/dev/null || fail "forced cleanup left child process $process_id alive"
  done <"$pid_export"
}
expect_helper_error() {
  local name="$1" path="$2" message="$3"
  if PATH="$path" /bin/bash "$HELPER" "unix://$TEST_DIR/socket" \
    >"$TEST_DIR/$name.log" 2>&1; then
    fail "missing $name was accepted"
  fi
  grep -Fq "$message" "$TEST_DIR/$name.log"
}
run_entrypoint api-becomes-ready 0 run delayed 3
[[ -f "$LISTENER_MARKER" ]] || fail 'listener did not start after the Docker API became ready'
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
run_forced_failure_cleanup_case
expect_helper_error missing-docker "$MISSING_DOCKER_BIN" 'Docker CLI is missing or not executable'
expect_helper_error missing-timeout "$MISSING_TIMEOUT_BIN" 'timeout utility is missing'
echo 'runner Docker API readiness tests passed'
