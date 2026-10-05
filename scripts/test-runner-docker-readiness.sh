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
SAVED_PATH="$PATH"
LISTENER_MARKER="$TEST_DIR/listener-started"
DOCKER_COUNT="$TEST_DIR/docker-count"
DOCKER_LOG="$TEST_DIR/docker.log"
API_LOG="$TEST_DIR/api.log"
TIMEOUT_LOG="$TEST_DIR/timeout.log"
SOCKET_MARKER="$TEST_DIR/socket-present"
MISSING_DOCKER_BIN="$TEST_DIR/no-docker"
MISSING_TIMEOUT_BIN="$TEST_DIR/no-timeout"
PRODUCTION_SOCKET='unix:///var/run/docker.sock'

mkdir -p "$STUB_BIN" "$TEST_IMAGE_DIR" "$TEST_RUNNER_ROOT/bin" \
  "$MISSING_DOCKER_BIN" "$MISSING_TIMEOUT_BIN"
touch "$SOCKET_MARKER"

cat >"$STUB_BIN/timeout" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$READINESS_TIMEOUT_LOG"
[[ "$1" == --signal=KILL && "$#" -ge 3 ]] || exit 125
if [[ "$READINESS_TIMEOUT_MODE" == expire ]]; then
  exit 137
fi
shift 2
exec "$@"
STUB

cat >"$STUB_BIN/sleep" <<'STUB'
#!/bin/sh
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
