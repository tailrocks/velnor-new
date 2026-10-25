#!/bin/bash
set -euo pipefail

sock="${WAIT_DOCKER_SOCK:-/run/docker.sock}"
tries="${WAIT_DOCKER_SOCK_TRIES:-6000}"
i=0
while [[ ! -S "$sock" ]]; do
  i=$((i + 1))
  if [[ "$i" -gt "$tries" ]]; then
    echo "docker socket missing" >&2
    exit 1
  fi
  sleep 0.1
done
