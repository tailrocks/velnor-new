#!/usr/bin/env bash
# Drive copy-job-seeds.sh. Do not mount velnor-seed.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
tool="$root/copy-job-seeds.sh"
work="$(mktemp -d "$root/copy-test.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT
jobs=(
  rust-tron-migration__local
  rust-eth-migration__local
  rust-legacy-grpc-server__local
  rust-eth-processor-app__local
)
src="$work/src"
dest="$work/dest"
mkdir -p "$dest/generator" "$dest/mise" "$dest/rustup"
printf 'keep\n' >"$dest/generator/MARKER"
for job in "${jobs[@]}"; do
  mkdir -p "$src/$job/bundle/obj"
  printf 'obj-%s\n' "$job" >"$src/$job/bundle/obj/a"
  printf 'linux-x64-mbx-gen-dir-dir-rust-778fbc643a80-%s-\n' "$job" >"$src/$job/PREFIX"
done
bash "$tool" "$src" "$dest" "${jobs[@]}"
[ "$(cat "$dest/generator/MARKER")" = "keep" ]
[ -d "$dest/mise" ]
[ -d "$dest/rustup" ]
for job in "${jobs[@]}"; do
  [ "$(cat "$dest/mbx/$job/bundle/obj/a")" = "obj-$job" ]
done
# A second copy replaces one job and keeps the marker.
printf 'obj-2\n' >"$src/rust-tron-migration__local/bundle/obj/a"
bash "$tool" "$src" "$dest" rust-tron-migration__local
[ "$(cat "$dest/mbx/rust-tron-migration__local/bundle/obj/a")" = "obj-2" ]
[ "$(cat "$dest/mbx/rust-eth-migration__local/bundle/obj/a")" = "obj-rust-eth-migration__local" ]
[ "$(cat "$dest/generator/MARKER")" = "keep" ]
if bash "$tool" "$src" "$dest" '../secret'; then
  printf '%s\n' "unsafe job was copied" >&2
  exit 1
fi
[ ! -d "$dest/mbx/secret" ]
[ "$(cat "$dest/generator/MARKER")" = "keep" ]
printf '%s\n' "copy-job-seeds tests passed"
