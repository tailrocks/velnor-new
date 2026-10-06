#!/usr/bin/env bash
# Copy one producer payload into a seed mbx directory.
# The reader expects DEST/PREFIX and DEST/bundle.
# PREFIX is one line. It must match the job prefix.
# This script does not mount Docker volumes.
set -euo pipefail

die() {
  printf '%s\n' "$1" >&2
  exit 1
}

workers_holding_seed() {
  if [ -n "${MBX_SEED_BUSY_CMD:-}" ]; then
    "$MBX_SEED_BUSY_CMD"
    return
  fi
  DOCKER_HOST="${DOCKER_HOST:-unix:///Users/donbeave/.orbstack/run/docker.sock}" \
    docker ps -a --filter volume=velnor-seed --format '{{.Names}}'
}

producer_conclusion() {
  if [ -n "${MBX_SEED_PRODUCER_CMD:-}" ]; then
    "$MBX_SEED_PRODUCER_CMD"
    return
  fi
  gh="/Users/donbeave/.local/share/mise/shims/gh"
  "$gh" api repos/ChainArgos/java-monorepo/actions/runs/37352232818 --jq .conclusion
}

daemon_hash() {
  if [ -n "${MBX_SEED_DAEMON_CMD:-}" ]; then
    "$MBX_SEED_DAEMON_CMD"
    return
  fi
  shasum -a 256 "/Users/donbeave/Library/Application Support/Velnor/velnor-host" | awk '{print $1}'
}

refuse_live_while_busy() {
  if [ "${MBX_SEED_LIVE:-}" != "1" ]; then
    return 0
  fi
  busy="$(workers_holding_seed || true)"
  if [ -n "$busy" ]; then
    die "seed volume is held by workers"
  fi
  conclusion="$(producer_conclusion || true)"
  if [ "$conclusion" != "success" ]; then
    die "producer run 37352232818 is ${conclusion:-absent}"
  fi
  hash="$(daemon_hash || true)"
  if [ "$hash" != "391fbaf956ab69a66ff3b9b507962ec2cbb0062f4960719f7a1d37b7ed5253d9" ]; then
    die "daemon hash is ${hash:-absent}"
  fi
}

require_regular_dir() {
  path="$1"
  label="$2"
  if [ -L "$path" ]; then
    die "$label is a symlink"
  fi
  if [ ! -d "$path" ]; then
    die "$label is not a directory"
  fi
}

main() {
  payload="${1:-}"
  dest="${2:-}"
  [ -n "$payload" ] || die "payload path is empty"
  [ -n "$dest" ] || die "dest path is empty"
  require_regular_dir "$payload" "payload"
  prefix_file="${payload}/PREFIX"
  bundle="${payload}/bundle"
  if [ -L "$prefix_file" ]; then
    die "PREFIX is a symlink"
  fi
  [ -f "$prefix_file" ] || die "PREFIX is missing"
  require_regular_dir "$bundle" "bundle"
  prefix="$(awk 'NR==1 { print; exit }' "$prefix_file")"
  [ -n "$prefix" ] || die "PREFIX is empty"
  case "$prefix" in
    *$'\n'* | */* | *' '* | -* | *[!A-Za-z0-9_.-]*) die "PREFIX is not a job prefix" ;;
    *- ) ;;
    *) die "PREFIX does not end with a hyphen" ;;
  esac
  refuse_live_while_busy
  case "$dest" in
    "$payload" | "$payload"/*) die "dest is inside the payload" ;;
  esac
  parent="$(dirname "$dest")"
  require_regular_dir "$parent" "dest parent"
  stage="${parent}/.mbx-seed-stage.$$"
  rm -rf "$stage"
  mkdir -p "$stage"
  cp -R "$bundle" "${stage}/bundle"
  printf '%s\n' "$prefix" >"${stage}/PREFIX"
  if [ -e "$dest" ]; then
    if [ -L "$dest" ]; then
      rm -rf "$stage"
      die "dest is a symlink"
    fi
    backup="${parent}/.mbx-seed-backup.$$"
    rm -rf "$backup"
    mv "$dest" "$backup"
    if ! mv "$stage" "$dest"; then
      mv "$backup" "$dest"
      die "dest replace failed"
    fi
    rm -rf "$backup"
  else
    mv "$stage" "$dest"
  fi
  [ -f "${dest}/PREFIX" ] || die "dest PREFIX is missing"
  [ -d "${dest}/bundle" ] || die "dest bundle is missing"
  printf '%s\n' "seed payload copied"
}

main "$@"
