#!/usr/bin/env bash
set -euo pipefail
unset ACTIONS_RUNNER_INPUT_JITCONFIG || true
expected_docker_host='tcp://localhost:2375'
export DOCKER_HOST="$expected_docker_host"
export DOCKER_CONTEXT='runner-context-must-be-ignored'

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# Keep executable test fixtures on the writable Work volume when TMPDIR points there.
fixture="$(mktemp -d "${TMPDIR:-/tmp}/velnor-jit-input.XXXXXX")"
jit_tmp="$(mktemp -d "/tmp/velnor-jit-input-temp.XXXXXX")"
trap 'rm -rf "$fixture" "$jit_tmp"' EXIT
mkdir -p "$fixture/bin" "$fixture/root/bin" "$fixture/root/_work" "$fixture/input"
linux_max_arg_strlen=131072
jit_env_name='ACTIONS_RUNNER_INPUT_JITCONFIG'
max_payload_bytes=$((linux_max_arg_strlen - ${#jit_env_name} - 2))
linux_page_size=""
if [[ "$(uname -s)" == Linux ]]; then
  linux_page_size="$(getconf PAGESIZE 2>/dev/null || true)"
fi

# Redirect image-only paths in a private copy; the tested entrypoint is otherwise unchanged.
sed \
  -e "s|/tmp/velnor-jit|$jit_tmp/velnor-jit|" \
  -e "s|root=\"/home/runner\"|root=\"$fixture/root\"|" \
  -e "s|/usr/local/libexec/velnor/runner-home.py|$fixture/runner-home.py|" \
  -e "s|/usr/local/bin/wait-docker-sock|$fixture/bin/wait-docker-sock|" \
  "$script_dir/entrypoint.sh" > "$fixture/entrypoint.sh"
chmod 0755 "$fixture/entrypoint.sh"
: > "$fixture/runner-home.py"

cat > "$fixture/bin/python3" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -eq 8 && "$1" == "$HOME_INIT_SCRIPT" && "$2" == initialize ]]
[[ "$3" == --seed && "$4" == /opt/runner ]]
[[ "$5" == --home && "$6" == "$RUNNER_ROOT" ]]
[[ "$7" == --manifest && "$8" == /usr/share/velnor/runner-home-manifest.json ]]
touch "$HOME_INIT_MARKER"
EOF
chmod 0755 "$fixture/bin/python3"

cat > "$fixture/bin/wait-docker-sock" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "$fixture/velnor-wait-docker-api" <<'EOF'
#!/usr/bin/env bash
[[ "$#" -eq 1 && "$1" == "tcp://localhost:2375" ]]
EOF
chmod 0755 "$fixture/bin/wait-docker-sock" "$fixture/velnor-wait-docker-api"

# macOS and Linux use different `dd`/`stat` options. These fixture commands
# provide only the exact interface used by entrypoint.sh.
cat > "$fixture/bin/dd" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
source= destination= block_size= count=
for arg in "$@"; do
  case "$arg" in
    if=*) source="${arg#if=}" ;;
    of=*) destination="${arg#of=}" ;;
    bs=*) block_size="${arg#bs=}" ;;
    count=*) count="${arg#count=}" ;;
  esac
done
[[ "$source" == /dev/fd/[0-9]* && -n "$destination" ]]
[[ "$block_size" =~ ^[0-9]+$ && "$count" =~ ^[0-9]+$ ]]
head -c "$((block_size * count))" < "$source" > "$destination"
EOF
cat > "$fixture/bin/stat" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -eq 3 && "$1" == -c && "$2" == %s ]]
wc -c < "$3" | tr -d '[:space:]'
EOF
chmod 0755 "$fixture/bin/dd" "$fixture/bin/stat"

cat > "$fixture/root/bin/Runner.Listener" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -eq 1 && "$1" == run ]] || exit 81
[[ -f "$HOME_INIT_MARKER" ]] || exit 88
[[ "${DOCKER_HOST:-}" == "tcp://localhost:2375" ]] || exit 89
[[ -z "${DOCKER_CONTEXT:-}" ]] || exit 90
env_name='ACTIONS_RUNNER_INPUT_JITCONFIG'
env_entry_bytes=$((${#env_name} + 1 + ${#ACTIONS_RUNNER_INPUT_JITCONFIG} + 1))
[[ "$env_entry_bytes" -le 131072 ]] || exit 86
if [[ -n "${EXPECTED_ENV_ENTRY_BYTES:-}" ]]; then
  [[ "$env_entry_bytes" -eq "$EXPECTED_ENV_ENTRY_BYTES" ]] || exit 87
fi
if [[ -n "${EXPECTED_LENGTH:-}" ]]; then
  [[ "${#ACTIONS_RUNNER_INPUT_JITCONFIG}" -eq "$EXPECTED_LENGTH" ]] || exit 82
else
  [[ "${ACTIONS_RUNNER_INPUT_JITCONFIG:-}" == "${EXPECTED_CANARY:-}" ]] || exit 83
fi
[[ -z "$(find "$FIXTURE_TMP" -maxdepth 1 -name 'velnor-jit.*' -print -quit)" ]] || exit 84
[[ -z "$(find "$RUNNER_ROOT/_work" -mindepth 1 -print -quit)" ]] || exit 85
printf 'listener-ran\n' > "$LISTENER_MARKER"
EOF
chmod 0755 "$fixture/root/bin/Runner.Listener"

canary='jit-input-canary-7d2a'
printf '%s' "$canary" > "$fixture/input/canary"
printf 'stdin-decoy' > "$fixture/input/decoy"
printf '%s' "$canary" > "$fixture/input/fd3"
python3 -c 'import sys; sys.stdout.write("A" * int(sys.argv[1]))' \
  "$max_payload_bytes" > "$fixture/input/max"
python3 -c 'import sys; sys.stdout.write("A" * int(sys.argv[1]))' \
  "$((max_payload_bytes + 1))" > "$fixture/input/oversize"
: > "$fixture/input/empty"

fail() {
  printf 'jit input test failed: %s\n' "$1" >&2
  exit 1
}

assert_clean_outputs() {
  local payload_file="$1" path payload_bytes output_bytes persistent_files
  payload_bytes="$(wc -c < "$payload_file")"
  for path in "$fixture/stdout" "$fixture/stderr"; do
    if [[ -s "$path" ]]; then
      output_bytes="$(wc -c < "$path")"
      if (( output_bytes >= payload_bytes )) && grep -F -q -f "$payload_file" "$path"; then
        fail "input appeared in process output"
      fi
    fi
  done
  [[ -z "$(find "$jit_tmp" -name 'velnor-jit.*' -print -quit)" ]] \
    || fail "temporary JIT file remained after entrypoint exit"
  [[ -z "$(find "$fixture/root/_work" -mindepth 1 -print -quit)" ]] \
    || fail "JIT data was written to the work volume"
  persistent_files="$(find "$fixture/root" -type f -print | wc -l)"
  [[ "$persistent_files" -eq 1 && -f "$fixture/root/bin/Runner.Listener" ]] \
    || fail "JIT input was persisted in the Runner Home fixture"
}

run_success() {
  local payload_file="$1" mode="$2" expected="$3" entry_bytes="${4:-}" status
  rm -f "$fixture/listener-ran" "$fixture/home-initialized" "$fixture/stdout" "$fixture/stderr"
  status=0
  if [[ "$mode" == fd3 ]]; then
    PATH="$fixture/bin:$PATH" \
      FIXTURE_TMP="$jit_tmp" RUNNER_ROOT="$fixture/root" \
      HOME_INIT_SCRIPT="$fixture/runner-home.py" HOME_INIT_MARKER="$fixture/home-initialized" \
      LISTENER_MARKER="$fixture/listener-ran" EXPECTED_CANARY="$expected" \
      "$BASH" "$fixture/entrypoint.sh" \
      < "$fixture/input/decoy" 3< "$payload_file" \
      > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
  elif [[ "$mode" == length ]]; then
    PATH="$fixture/bin:$PATH" \
      FIXTURE_TMP="$jit_tmp" RUNNER_ROOT="$fixture/root" \
      HOME_INIT_SCRIPT="$fixture/runner-home.py" HOME_INIT_MARKER="$fixture/home-initialized" \
      LISTENER_MARKER="$fixture/listener-ran" EXPECTED_LENGTH="$expected" \
      EXPECTED_ENV_ENTRY_BYTES="$entry_bytes" \
      "$BASH" "$fixture/entrypoint.sh" < "$payload_file" \
      > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
  else
    PATH="$fixture/bin:$PATH" \
      FIXTURE_TMP="$jit_tmp" RUNNER_ROOT="$fixture/root" \
      HOME_INIT_SCRIPT="$fixture/runner-home.py" HOME_INIT_MARKER="$fixture/home-initialized" \
      LISTENER_MARKER="$fixture/listener-ran" EXPECTED_CANARY="$expected" \
      "$BASH" "$fixture/entrypoint.sh" < "$payload_file" \
      > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
  fi
  [[ "$status" -eq 0 && -f "$fixture/listener-ran" ]] \
    || fail "listener did not receive supported JIT environment input"
  [[ -f "$fixture/home-initialized" ]] || fail "Home was not initialized before listener start"
  assert_clean_outputs "$payload_file"
}

run_rejection() {
  local payload_file="$1" expected_status="$2" status
  rm -f "$fixture/listener-ran" "$fixture/home-initialized" "$fixture/stdout" "$fixture/stderr"
  status=0
  PATH="$fixture/bin:$PATH" \
    FIXTURE_TMP="$jit_tmp" RUNNER_ROOT="$fixture/root" \
    HOME_INIT_SCRIPT="$fixture/runner-home.py" HOME_INIT_MARKER="$fixture/home-initialized" \
    LISTENER_MARKER="$fixture/listener-ran" \
    "$BASH" "$fixture/entrypoint.sh" < "$payload_file" \
    > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
  [[ "$status" -eq "$expected_status" && ! -e "$fixture/listener-ran" ]] \
    || fail "invalid input was not rejected before listener start"
  [[ -f "$fixture/home-initialized" ]] || fail "Home was not initialized before private JIT validation"
  assert_clean_outputs "$payload_file"
}

run_success "$fixture/input/canary" stdin "$canary"
run_success "$fixture/input/fd3" fd3 "$canary"
run_success "$fixture/input/max" length "$max_payload_bytes" "$linux_max_arg_strlen"
run_rejection "$fixture/input/empty" 2
run_rejection "$fixture/input/oversize" 2

run_transport_rejection() {
  local mode="$1" rejected_endpoint="${2:-}" status=0
  rm -f "$fixture/listener-ran" "$fixture/home-initialized" "$fixture/stdout" "$fixture/stderr"
  if [[ "$mode" == missing ]]; then
    env -u DOCKER_HOST PATH="$fixture/bin:$PATH" \
      FIXTURE_TMP="$jit_tmp" RUNNER_ROOT="$fixture/root" \
      HOME_INIT_SCRIPT="$fixture/runner-home.py" HOME_INIT_MARKER="$fixture/home-initialized" \
      LISTENER_MARKER="$fixture/listener-ran" \
      "$BASH" "$fixture/entrypoint.sh" < "$fixture/input/canary" \
      > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
  else
    DOCKER_HOST="$rejected_endpoint" PATH="$fixture/bin:$PATH" \
      FIXTURE_TMP="$jit_tmp" RUNNER_ROOT="$fixture/root" \
      HOME_INIT_SCRIPT="$fixture/runner-home.py" HOME_INIT_MARKER="$fixture/home-initialized" \
      LISTENER_MARKER="$fixture/listener-ran" \
      "$BASH" "$fixture/entrypoint.sh" < "$fixture/input/canary" \
      > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
  fi
  [[ "$status" -eq 2 && ! -e "$fixture/listener-ran" ]] \
    || fail "${mode} Docker endpoint was not rejected"
  [[ ! -e "$fixture/home-initialized" ]] \
    || fail "invalid Docker endpoint was accepted past Runner Home initialization"
  assert_clean_outputs "$fixture/input/canary"
}

run_transport_rejection missing
run_transport_rejection old-loopback 'tcp://127.0.0.1:2375'
run_transport_rejection wildcard 'tcp://0.0.0.0:2375'
run_transport_rejection foreign 'tcp://docker:2375'

# JIT supplied through Docker Config.Env is outside the controller's private
# pipe contract and must fail closed before Runner.Listener starts.
rm -f "$fixture/listener-ran" "$fixture/home-initialized" "$fixture/stdout" "$fixture/stderr"
status=0
PATH="$fixture/bin:$PATH" \
  FIXTURE_TMP="$jit_tmp" RUNNER_ROOT="$fixture/root" \
  HOME_INIT_SCRIPT="$fixture/runner-home.py" HOME_INIT_MARKER="$fixture/home-initialized" \
  LISTENER_MARKER="$fixture/listener-ran" \
  ACTIONS_RUNNER_INPUT_JITCONFIG="$canary" \
  "$BASH" "$fixture/entrypoint.sh" < "$fixture/input/empty" \
  > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
[[ "$status" -eq 2 && ! -e "$fixture/listener-ran" ]] \
  || fail "ambient Docker JIT environment was not rejected"
[[ ! -e "$fixture/home-initialized" ]] || fail "ambient JIT was checked after Home initialization"
assert_clean_outputs "$fixture/input/canary"

if [[ "$linux_page_size" == 4096 ]]; then
  printf 'jit input fixture: PASS (stdin, fd3, 131072-byte Linux execve env-entry boundary, rejection, argv, output, persistence)\n'
elif [[ -n "$linux_page_size" ]]; then
  printf 'jit input fixture: PASS (stdin, fd3, 131072-byte env-entry math; Linux page size %s, 4-KiB execve boundary not exercised, rejection, argv, output, persistence)\n' "$linux_page_size"
else
  printf 'jit input fixture: PASS (stdin, fd3, 131072-byte env-entry math; Linux execve boundary not exercised, rejection, argv, output, persistence)\n'
fi
