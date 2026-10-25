#!/usr/bin/env bash
set -euo pipefail

readonly image='velnor-resource-probe:linux-amd64'
readonly archive='velnor-resource-probe-linux-amd64.tar'
readonly target_dir="${CARGO_TARGET_DIR:-crates/velnor-runner/target}"
readonly source_sha="${VELNOR_SOURCE_SHA:?missing exact source SHA}"
readonly source_revision="${VELNOR_SOURCE_REVISION:?missing resource-probe source revision}"
readonly authority_sha="${VELNOR_WORKFLOW_AUTHORITY_SHA:?missing workflow authority SHA}"

[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]]
[[ "$source_revision" == "$source_sha" ]]
[[ "$authority_sha" =~ ^[0-9a-f]{40}$ ]]
[[ "${GITHUB_REPOSITORY:?missing repository}" == tailrocks/velnor-new ]]
[[ "${GITHUB_REF:?missing source ref}" == refs/heads/main ]]
[[ "${GITHUB_SHA:?missing workflow SHA}" == "$source_sha" ]]

readonly release_validator="${VELNOR_RESOURCE_PROBE_RELEASE_BIN:?missing native resource-probe release validator}"
[[ "$release_validator" == /* && -f "$release_validator" && -x "$release_validator" && ! -L "$release_validator" ]] \
  || { printf 'native resource-probe release validator must be an absolute executable regular file\n' >&2; exit 1; }

python3 -B images/resource-probe/test_write_manifest.py
python3 -B images/resource-probe/test_validate_smoke.py
bash images/resource-probe/test_container_lifecycle.sh

workdir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/resource-probe-build.XXXXXXXX")"
readonly workdir
source images/resource-probe/container_lifecycle.sh
readonly context="$workdir/context"
readonly smoke_root="$workdir/docker-root"
readonly smoke_output="$workdir/smoke-output.jsonl"
readonly smoke_error="$workdir/smoke-error.log"
readonly inspect_json="$workdir/loaded-inspect.json"
readonly smoke_cidfile="$workdir/smoke-container.cid"
mkdir -m 0755 "$context" "$smoke_root"
trap 'resource_probe_cleanup_exit "$?" "$smoke_cidfile" "$workdir"' EXIT

cp "$target_dir/x86_64-unknown-linux-musl/release/velnor-resource-probe" "$context/resource-probe"
cp images/resource-probe/Dockerfile "$context/Dockerfile"
chmod 0555 "$context/resource-probe"
file_description="$(file -b "$context/resource-probe")"
case "$file_description" in
  *'ELF 64-bit LSB'*x86-64*static*) ;;
  *) printf 'resource probe is not a static ELF64 x86-64 executable: %s\n' "$file_description" >&2; exit 1 ;;
esac
python3 - "$context/resource-probe" "$source_revision" <<'PY'
import pathlib
import sys

binary = pathlib.Path(sys.argv[1]).read_bytes()
revision = sys.argv[2].encode("ascii")
if revision not in binary:
    raise SystemExit("resource probe does not contain its source revision")
PY

docker build --platform linux/amd64 --network none --build-arg "SOURCE_SHA=$source_sha" --tag "$image" "$context"
chmod 0755 "$smoke_root"
timeout --signal=TERM --kill-after=10s 180s docker run --cidfile "$smoke_cidfile" --pull=never --platform linux/amd64 --cgroupns=host --pid=host --cpu-period=100000 --cpu-quota=10000 --memory=134217728 --memory-swap=134217728 --pids-limit=4 --network none --read-only --cap-drop=ALL --security-opt no-new-privileges:true --user 65532:65532 --mount "type=bind,source=$smoke_root,target=/velnor/docker-root,readonly" --mount "type=bind,source=/sys/fs/cgroup,target=/sys/fs/cgroup,readonly" "$image" > "$smoke_output" 2> "$smoke_error"
test ! -s "$smoke_error"
"$release_validator" validate-smoke-schema5 "$smoke_output" "$smoke_root"
python3 -B images/resource-probe/validate_smoke.py "$smoke_output" "$smoke_root"
docker image save --platform linux/amd64 --output "$archive" "$image"
test -s "$archive"
docker image load --platform linux/amd64 --input "$archive"
docker image inspect --platform linux/amd64 "$image" > "$inspect_json"
"$release_validator" write-manifest "$inspect_json" "$archive" RESOURCE_PROBE_MANIFEST.json

printf 'validated linux/amd64 resource probe image %s\n' "$source_sha"
