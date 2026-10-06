#!/bin/bash
# The wait accepts a socket that appears, and fails when it never does.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
sock="$dir/docker.sock"
(
  sleep 0.3
  python3 -c 'import os, socket, sys; s = socket.socket(socket.AF_UNIX); s.bind(sys.argv[1])' "$sock"
) &
WAIT_DOCKER_SOCK="$sock" WAIT_DOCKER_SOCK_TRIES=50 bash "$root/wait-docker-sock.sh"
if WAIT_DOCKER_SOCK="$dir/missing.sock" WAIT_DOCKER_SOCK_TRIES=2 bash "$root/wait-docker-sock.sh"; then
  echo "missing socket was accepted" >&2
  exit 1
fi
echo "wait-docker-sock ok"
