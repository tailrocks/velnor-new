#!/usr/bin/env bash
# Stage four producer payloads into mbx/<job-id>/{PREFIX,bundle}.
# This script does not mount Docker volumes.
set -euo pipefail

root="$(cd "$(dirname "$0")" && pwd)"
tool="$root/populate-mbx-seed.sh"

die() {
  printf '%s\n' "$1" >&2
  exit 1
}

jobs=(
  rust-tron-migration__local
  rust-eth-migration__local
  rust-legacy-grpc-server__local
  rust-eth-processor-app__local
)

main() {
  payload_root="${1:-}"
  stage="${2:-}"
  [ -n "$payload_root" ] || die "payload root is empty"
  [ -n "$stage" ] || die "stage path is empty"
  if [ -L "$payload_root" ] || [ ! -d "$payload_root" ]; then
    die "payload root is not a directory"
  fi
  if [ -L "$stage" ]; then
    die "stage is a symlink"
  fi
  mkdir -p "$stage/mbx"
  if [ -L "$stage/mbx" ] || [ ! -d "$stage/mbx" ]; then
    die "stage mbx is not a directory"
  fi
  for job in "${jobs[@]}"; do
    case "$job" in
      *[!A-Za-z0-9_.-]*) die "job id is unsafe" ;;
    esac
    src="$payload_root/mbx-single-bundle-$job"
    if [ -L "$src" ] || [ ! -d "$src" ]; then
      die "payload for $job is not a directory"
    fi
    prefix_file="$src/PREFIX"
    if [ -L "$prefix_file" ] || [ ! -f "$prefix_file" ]; then
      die "PREFIX for $job is missing"
    fi
    prefix="$(awk 'NR==1 { print; exit }' "$prefix_file")"
    case "$prefix" in
      *"-${job}-") ;;
      *) die "PREFIX does not name $job" ;;
    esac
    bash "$tool" "$src" "$stage/mbx/$job"
  done
  for job in "${jobs[@]}"; do
    [ -f "$stage/mbx/$job/PREFIX" ] || die "staged PREFIX for $job is missing"
    [ -d "$stage/mbx/$job/bundle" ] || die "staged bundle for $job is missing"
  done
  printf '%s\n' "four job seeds staged"
}

main "$@"
