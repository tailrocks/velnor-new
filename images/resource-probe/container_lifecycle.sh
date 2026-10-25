#!/usr/bin/env bash

resource_probe_cleanup_container() {
  local cidfile=$1
  local container_id
  local running
  local cidfile_size
  local cidfile_last_byte
  local result=0

  [[ -e "$cidfile" ]] || return 0
  [[ -s "$cidfile" ]] || {
    printf 'refusing cleanup for empty Docker cidfile\n' >&2
    return 1
  }
  cidfile_size="$(wc -c < "$cidfile" | tr -d '[:space:]')"
  case "$cidfile_size" in
    64)
      if ! IFS= read -r container_id < "$cidfile" && [[ -z "$container_id" ]]; then
        return 1
      fi
      ;;
    65)
      IFS= read -r container_id < "$cidfile" || [[ -n "$container_id" ]] || return 1
      cidfile_last_byte="$(tail -c 1 "$cidfile" | od -An -tu1 | tr -d '[:space:]')"
      [[ "$cidfile_last_byte" == 10 ]] || return 1
      ;;
    *)
      printf 'refusing cleanup for malformed Docker cidfile length\n' >&2
      return 1
      ;;
  esac
  if [[ ! "$container_id" =~ ^[0-9a-f]{64}$ ]]; then
    printf 'refusing cleanup for invalid Docker cidfile contents\n' >&2
    return 1
  fi

  running="$(timeout --signal=TERM --kill-after=1s 5s docker inspect --format '{{.State.Running}}' "$container_id" 2>/dev/null)"
  if [[ "$running" == true ]]; then
    timeout --signal=TERM --kill-after=2s 15s docker stop --time 10 "$container_id" >/dev/null 2>&1 || result=$?
  fi
  timeout --signal=TERM --kill-after=2s 15s docker rm --force "$container_id" >/dev/null 2>&1 || result=$?
  return "$result"
}

resource_probe_cleanup_exit() {
  local result=$1
  local cidfile=$2
  local workdir=$3
  local cleanup_failed=0

  trap - EXIT
  set +e
  if ! resource_probe_cleanup_container "$cidfile"; then
    printf 'failed to stop/remove the exact resource-probe container\n' >&2
    cleanup_failed=1
    if (( result == 0 )); then result=1; fi
  fi
  if (( cleanup_failed == 0 )); then
    rm -rf "$workdir" || {
      printf 'failed to remove resource-probe temporary workdir\n' >&2
      if (( result == 0 )); then result=1; fi
    }
  else
    printf 'retaining resource-probe cidfile and workdir for exact recovery\n' >&2
  fi
  exit "$result"
}
