#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/../../.." && pwd -P)"
image="$root/images/runner/ubuntu-26.04/Dockerfile"
entrypoint="$root/images/runner/ubuntu-26.04/entrypoint.sh"
build="$root/images/resource-probe/build-runner-images.sh"
wrapper="$root/images/runner/ubuntu-26.04/job-started-hook.sh"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/velnor-hook-contract.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT

grep -Fq 'COPY --from=job_start_hook job-start-hook /velnor/job-start-hook' "$image"
grep -Fq 'COPY --from=job_started_hook job-started-hook.sh /usr/local/bin/velnor-job-start-hook' "$image"
grep -Fq 'chmod 0555 /velnor/job-start-hook /usr/local/bin/velnor-job-start-hook' "$image"
grep -Fq 'chown root:root /velnor/job-start-hook /usr/local/bin/velnor-job-start-hook' "$image"
grep -Fq -- '--bin velnor-job-start-hook' "$build"
grep -Fq -- "--build-context \"job_start_hook=" "$build"
grep -Fq -- "--build-context \"job_started_hook=" "$build"
grep -Fq 'export ACTIONS_RUNNER_HOOK_JOB_STARTED=/usr/local/bin/velnor-job-start-hook' "$entrypoint"
grep -Fq 'export ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER=false' "$entrypoint"
grep -Fq 'unset ACTIONS_RUNNER_CONTAINER_HOOKS' "$entrypoint"
[[ "$(cat "$wrapper")" == $'#!/bin/bash\nset -euo pipefail\n\nexec /velnor/job-start-hook' ]]

cat > "$fixture/listener" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -eq 1 && "$1" == run ]]
if [[ ${ACTIONS_RUNNER_CONTAINER_HOOKS+x} ]]; then
  container_hooks_state=set
else
  container_hooks_state=unset
fi
printf '%s\n%s\n%s' \
  "${ACTIONS_RUNNER_HOOK_JOB_STARTED-}" \
  "${ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER-}" \
  "$container_hooks_state" > "$HOOK_CAPTURE"
printf '%s' "${ACTIONS_RUNNER_INPUT_JITCONFIG-}" > "$JIT_CAPTURE"
EOF
chmod 0755 "$fixture/listener"

run_launch_fixture() {
  local name="$1"
  local payload="$2"
  {
    printf 'payload=%q\nlistener=%q\n' "$payload" "$fixture/listener"
    sed -n '/^unset ACTIONS_RUNNER_CONTAINER_HOOKS$/,$p' "$entrypoint"
  } > "$fixture/launch-${name}.sh"
  HOOK_CAPTURE="$fixture/hook-${name}" \
    JIT_CAPTURE="$fixture/jit-${name}" \
    ACTIONS_RUNNER_HOOK_JOB_STARTED=/tmp/incorrect-hook \
    ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER=true \
    ACTIONS_RUNNER_CONTAINER_HOOKS=/tmp/incorrect-container-hooks \
    bash "$fixture/launch-${name}.sh"
  [[ "$(cat "$fixture/hook-${name}")" == $'/usr/local/bin/velnor-job-start-hook\nfalse\nunset' ]]
  [[ "$(cat "$fixture/jit-${name}")" == "$payload" ]]
}

run_launch_fixture containerless '{"fixture":"containerless"}'
run_launch_fixture declared-container '{"fixture":"declared-container","container":{"image":"ubuntu:26.04"}}'

python3 - "$entrypoint" <<'PY'
from pathlib import Path
import sys

lines = Path(sys.argv[1]).read_text().splitlines()
required = [
    "unset ACTIONS_RUNNER_CONTAINER_HOOKS",
    "export ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER=false",
    "export ACTIONS_RUNNER_HOOK_JOB_STARTED=/usr/local/bin/velnor-job-start-hook",
    'ACTIONS_RUNNER_INPUT_JITCONFIG="$payload" exec "$listener" run',
]
start = len(lines) - len(required)
if lines[start:] != required:
    raise SystemExit("The Runner hook settings must be fixed at the final exec.")
PY

printf 'job-start hook image contract: PASS\n'
