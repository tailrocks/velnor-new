#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
mkdir "$test_root/bin" "$test_root/workdir"
cat > "$test_root/bin/timeout" <<'SH'
#!/usr/bin/env bash
while [[ "$1" == --signal=* || "$1" == --kill-after=* ]]; do shift; done
shift
exec "$@"
SH
cat > "$test_root/bin/docker" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$DOCKER_LOG"
if [[ "$1" == inspect ]]; then printf 'true\n'; fi
if [[ "${FAIL_RM:-0}" == 1 && "$1" == rm ]]; then exit 17; fi
SH
chmod 0755 "$test_root/bin/timeout" "$test_root/bin/docker"

container_id='0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
cidfile="$test_root/workdir/container.cid"
printf '%s' "$container_id" > "$cidfile"
export PATH="$test_root/bin:$PATH"
export DOCKER_LOG="$test_root/docker.log"

set +e
bash -c 'source "$1"; resource_probe_cleanup_exit 124 "$2" "$3"' _ \
  "$script_dir/container_lifecycle.sh" "$cidfile" "$test_root/workdir"
status=$?
set -e
[[ "$status" == 124 ]]
expected=$'inspect --format {{.State.Running}} '\
"$container_id"$'\nstop --time 10 '\
"$container_id"$'\nrm --force '\
"$container_id"
[[ "$(cat "$DOCKER_LOG")" == "$expected" ]]
[[ ! -e "$test_root/workdir" ]]

mkdir "$test_root/workdir"
: > "$DOCKER_LOG"
printf '%s' "$container_id" > "$cidfile"
set +e
FAIL_RM=1 bash -c 'source "$1"; resource_probe_cleanup_exit 0 "$2" "$3"' _ \
  "$script_dir/container_lifecycle.sh" "$cidfile" "$test_root/workdir"
status=$?
set -e
[[ "$status" == 1 ]]
[[ "$(cat "$DOCKER_LOG")" == "$expected" ]]
[[ -e "$test_root/workdir/container.cid" ]]

rm -rf "$test_root/workdir"
mkdir "$test_root/workdir"
: > "$DOCKER_LOG"
: > "$cidfile"
set +e
bash -c 'source "$1"; resource_probe_cleanup_exit 0 "$2" "$3"' _ \
  "$script_dir/container_lifecycle.sh" "$cidfile" "$test_root/workdir"
status=$?
set -e
[[ "$status" == 1 ]]
[[ ! -s "$DOCKER_LOG" ]]
[[ -e "$test_root/workdir/container.cid" ]]

printf 'container lifecycle cleanup tests passed\n'
