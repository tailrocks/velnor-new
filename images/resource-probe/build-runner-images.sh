#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

die() {
  printf 'runner image qualification: %s\n' "$1" >&2
  exit 1
}

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || die 'run from a Git checkout'
cd "$repo_root"

source_revision="${VELNOR_SOURCE_REVISION:?missing exact source revision}"
[[ "$source_revision" =~ ^[0-9a-f]{40}$ ]] || die 'source revision must be 40 lowercase hexadecimal characters'

versions="$(python3 - .velnor/runner.lock <<'PY'
import pathlib
import sys
import tomllib

lock = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
runner = lock.get("runner_version")
docker = lock.get("docker_version")
if not isinstance(runner, str) or not runner:
    raise SystemExit("runner version is missing from .velnor/runner.lock")
if not isinstance(docker, str) or not docker:
    raise SystemExit("Docker version is missing from .velnor/runner.lock")
print(runner)
print(docker)
PY
)" || die 'cannot read pinned image versions'
runner_version="${versions%%$'\n'*}"
docker_version="${versions#*$'\n'}"
[[ -n "$runner_version" && -n "$docker_version" && "$versions" == *$'\n'* ]] || die 'invalid pinned image versions'

require_clean_source() {
  local phase="$1"
  local checked_out_revision
  local untracked

  checked_out_revision="$(git rev-parse HEAD)" || die "cannot read Git HEAD $phase"
  [[ "$checked_out_revision" == "$source_revision" ]] || die "Git HEAD changed $phase"
  git diff --quiet HEAD -- || die "source tree differs from HEAD $phase"
  untracked="$(git ls-files --others --exclude-standard)" || die "cannot list untracked source $phase"
  [[ -z "$untracked" ]] || die "source tree has untracked files $phase"
}

require_clean_source 'before image build'

run_id="$(python3 -c 'import uuid; print(uuid.uuid4().hex)')" || die 'cannot create a unique build identifier'
readonly run_id
readonly run_label="io.velnor.image-builder.run=$run_id"
readonly revision_label="org.opencontainers.image.revision=$source_revision"
readonly runner_image="velnor-runner:local-${source_revision:0:12}-$run_id"
readonly dind_image="velnor-dind:local-${source_revision:0:12}-$run_id"
readonly dind_container="velnor-image-smoke-$run_id"
readonly client_container="velnor-runner-smoke-$run_id"
readonly socket_volume="velnor-image-socket-$run_id"
readonly data_volume="velnor-image-data-$run_id"
readonly smoke_args=(--rm --pull=never --platform linux/amd64 --network none)
readonly cleanup_docker_timeout=3
readonly runner_socket_timeout=5

docker_with_timeout() {
  local timeout="$1"
  shift

  python3 - "$timeout" "$@" <<'PY'
import subprocess
import sys

timeout = float(sys.argv[1])
try:
    result = subprocess.run(
        ["docker", *sys.argv[2:]],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
except subprocess.TimeoutExpired:
    print(f"docker command exceeded {timeout:g}-second deadline", file=sys.stderr)
    raise SystemExit(124)
sys.stdout.write(result.stdout)
sys.stderr.write(result.stderr)
raise SystemExit(result.returncode)
PY
}

remove_owned_container() {
  local name="$1"
  local owner
  local inspect_status

  if owner="$(docker_with_timeout "$cleanup_docker_timeout" container inspect \
    --format '{{index .Config.Labels "io.velnor.image-builder.run"}}' "$name" 2>/dev/null)"; then
    if [[ "$owner" == "$run_id" ]] && ! docker_with_timeout "$cleanup_docker_timeout" rm --force "$name" >/dev/null; then
      cleanup_failed=1
    fi
  else
    inspect_status=$?
    if [[ "$inspect_status" -eq 124 ]]; then
      cleanup_failed=1
    fi
  fi
}

cleanup() {
  local result=$?
  local cleanup_failed=0
  local inspect_status
  local owner
  trap - EXIT
  set +e

  remove_owned_container "$client_container"
  remove_owned_container "$dind_container"
  for volume in "$socket_volume" "$data_volume"; do
    if owner="$(docker_with_timeout "$cleanup_docker_timeout" volume inspect \
      --format '{{index .Labels "io.velnor.image-builder.run"}}' "$volume" 2>/dev/null)"; then
      if [[ "$owner" == "$run_id" ]] && ! docker_with_timeout "$cleanup_docker_timeout" volume rm "$volume" >/dev/null; then
        cleanup_failed=1
      fi
    else
      inspect_status=$?
      if [[ "$inspect_status" -eq 124 ]]; then
        cleanup_failed=1
      fi
    fi
  done
  if [[ "$result" -eq 0 && "$cleanup_failed" -ne 0 ]]; then
    result=1
  fi
  exit "$result"
}
trap cleanup EXIT

docker build \
  --platform linux/amd64 \
  --label "$revision_label" \
  --label "$run_label" \
  --tag "$runner_image" \
  images/runner/ubuntu-26.04
docker build \
  --platform linux/amd64 \
  --label "$revision_label" \
  --label "$run_label" \
  --tag "$dind_image" \
  images/dind

inspect_image() {
  local image="$1"
  local metadata

  metadata="$(docker image inspect --format '{{.Os}}|{{.Architecture}}|{{.Id}}|{{index .Config.Labels "org.opencontainers.image.revision"}}|{{index .Config.Labels "io.velnor.image-builder.run"}}' "$image")" \
    || die "cannot inspect image $image"
  IFS='|' read -r image_os image_arch image_id image_revision image_run <<< "$metadata"
  [[ "$image_os" == linux && "$image_arch" == amd64 ]] || die "image must be linux/amd64: $image ($image_os/$image_arch)"
  [[ "$image_id" =~ ^sha256:[0-9a-f]{64}$ ]] || die "image has no immutable SHA-256 ID: $image"
  [[ "$image_revision" == "$source_revision" ]] || die "image source label does not match $source_revision: $image"
  [[ "$image_run" == "$run_id" ]] || die "image build label does not match this run: $image"
  printf 'image=%s image_id=%s platform=%s/%s source_revision=%s run_id=%s\n' \
    "$image" "$image_id" "$image_os" "$image_arch" "$image_revision" "$image_run"
}

inspect_image "$runner_image"
inspect_image "$dind_image"

require_docker_version() {
  local actual="$1"
  local component="$2"
  case "$actual" in
    "Docker version ${docker_version},"*) ;;
    *) die "$component Docker version does not match $docker_version: $actual" ;;
  esac
}

runner_listener_version="$(docker run "${smoke_args[@]}" \
  --entrypoint /home/runner/bin/Runner.Listener "$runner_image" --version)" \
  || die 'Runner.Listener version probe failed'
[[ "$runner_listener_version" == "$runner_version" ]] \
  || die "Runner.Listener version does not match $runner_version: $runner_listener_version"

runner_docker_version="$(docker run "${smoke_args[@]}" \
  --entrypoint /usr/local/bin/docker "$runner_image" --version)" \
  || die 'Runner Docker CLI version probe failed'
require_docker_version "$runner_docker_version" 'Runner image'

dind_docker_version="$(docker run "${smoke_args[@]}" \
  --entrypoint /usr/local/bin/docker "$dind_image" --version)" \
  || die 'DinD Docker CLI version probe failed'
require_docker_version "$dind_docker_version" 'DinD image CLI'
dind_daemon_version="$(docker run "${smoke_args[@]}" \
  --entrypoint /usr/local/bin/dockerd "$dind_image" --version)" \
  || die 'DinD daemon version probe failed'
require_docker_version "$dind_daemon_version" 'DinD image daemon'

outer_docker_version="$(docker version --format '{{.Server.Version}}')" \
  || die 'cannot read the outer Docker server version'
outer_docker_id="$(docker info --format '{{.ID}}')" \
  || die 'cannot read the outer Docker daemon ID'
[[ -n "$outer_docker_id" ]] || die 'outer Docker daemon ID is empty'

docker volume create --label "$run_label" "$socket_volume" >/dev/null
docker volume create --label "$run_label" "$data_volume" >/dev/null
docker run --detach \
  --pull=never \
  --platform linux/amd64 \
  --privileged \
  --network none \
  --name "$dind_container" \
  --label "$run_label" \
  --mount "type=volume,source=$socket_volume,target=/run" \
  --mount "type=volume,source=$data_volume,target=/var/lib/docker" \
  "$dind_image" >/dev/null

inner_docker_id="$(python3 - "$dind_container" <<'PY'
import subprocess
import sys
import time

container = sys.argv[1]
deadline = time.monotonic() + 15
last_error = ""
while time.monotonic() < deadline:
    remaining = deadline - time.monotonic()
    try:
        result = subprocess.run(
            [
                "docker",
                "exec",
                container,
                "/usr/local/bin/docker",
                "--host",
                "unix:///var/run/docker.sock",
                "info",
                "--format",
                "{{.ID}}",
            ],
            capture_output=True,
            text=True,
            timeout=min(2, remaining),
            check=False,
        )
    except subprocess.TimeoutExpired:
        last_error = "private DinD readiness probe exceeded its per-call timeout"
    else:
        if result.returncode == 0 and result.stdout.strip():
            print(result.stdout.strip())
            raise SystemExit(0)
        last_error = result.stderr.strip()
    time.sleep(min(0.25, max(0, deadline - time.monotonic())))
if last_error:
    print(last_error, file=sys.stderr)
raise SystemExit("private DinD did not become ready on its Unix socket within 15 seconds")
PY
)" || die 'private DinD readiness check failed'
[[ "$inner_docker_id" != "$outer_docker_id" ]] || die 'private DinD resolved to the outer Docker daemon'

runner_docker_id="$(docker_with_timeout "$runner_socket_timeout" run --pull=never --platform linux/amd64 --network none \
  --name "$client_container" \
  --label "$run_label" \
  --mount "type=volume,source=$socket_volume,target=/run" \
  --entrypoint /usr/local/bin/docker "$runner_image" \
  --host unix:///var/run/docker.sock info --format '{{.ID}}')" \
  || die 'Runner image could not query Docker through the private Unix socket'
[[ "$runner_docker_id" == "$inner_docker_id" ]] \
  || die 'Runner image Unix socket reached a different Docker daemon'

require_clean_source 'after image build and smoke test'

printf 'runner_listener_version=%s runner_docker_version=%s dind_docker_version=%s dind_daemon_version=%s\n' \
  "$runner_listener_version" "$runner_docker_version" "$dind_docker_version" "$dind_daemon_version"
printf 'outer_docker_version=%s outer_docker_id=%s inner_docker_id=%s\n' \
  "$outer_docker_version" "$outer_docker_id" "$inner_docker_id"
printf 'qualified_runner_image=%s qualified_dind_image=%s source_revision=%s\n' \
  "$runner_image" "$dind_image" "$source_revision"
