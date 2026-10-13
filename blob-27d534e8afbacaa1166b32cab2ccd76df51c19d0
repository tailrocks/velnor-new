#!/usr/bin/env bash
# Drive stage-mbx-seeds.sh. Do not mount velnor-seed.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
tool="$root/stage-mbx-seeds.sh"
prefix_for() {
  printf '%s\n' "linux-x64-mbx-velnor-mbx-1.21.1-share-out-dir-disabled-v1-action-1687e54eb349cadf61fa38b5813a77875489e8e6-dir-dir-rust-778fbc643a80-${1}-"
}
work="$(mktemp -d "$root/stage-test.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT
jobs=(
  rust-tron-migration__local
  rust-eth-migration__local
  rust-legacy-grpc-server__local
  rust-eth-processor-app__local
)
payload="$work/payloads"
stage="$work/stage"
mkdir -p "$payload"
for job in "${jobs[@]}"; do
  dir="$payload/mbx-single-bundle-$job"
  mkdir -p "$dir/bundle/obj"
  printf '%s\n' "obj-$job" >"$dir/bundle/obj/a"
  prefix_for "$job" >"$dir/PREFIX"
done
bash "$tool" "$payload" "$stage"
for job in "${jobs[@]}"; do
  got="$(awk 'NR==1 { print; exit }' "$stage/mbx/$job/PREFIX")"
  [ "$got" = "$(prefix_for "$job")" ]
  [ "$(cat "$stage/mbx/$job/bundle/obj/a")" = "obj-$job" ]
done
# A foreign PREFIX is refused and does not create that dest.
bad="$payload/mbx-single-bundle-rust-tron-migration__local/PREFIX"
printf '%s\n' "linux-x64-mbx-other-rust-eth-migration__local-" >"$bad"
rm -rf "$stage"
if bash "$tool" "$payload" "$stage"; then
  printf '%s\n' "foreign PREFIX was accepted" >&2
  exit 1
fi
[ ! -d "$stage/mbx/rust-tron-migration__local" ]
printf '%s\n' "stage-mbx-seeds tests passed"
