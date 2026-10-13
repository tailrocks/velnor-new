#!/bin/sh
# The amd64 rabbitmq:3.8.22-management image is OTP 24 JIT. Nested Rosetta
# exits that broker before "Server startup complete". Manifest b4be7046 is
# the arm64 emu build, which reaches that line. An unscoped pull stores the
# JIT image, so the public socket stays down until inspect reports arm64.
set -eu
export PATH="/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
real=/run/docker.sock.real
public=/run/docker.sock
rm -f "$public"
dockerd --host="unix://${real}" "$@" &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; wait "$pid" || true; exit 0' TERM INT

wait_sock() {
  i=0
  while [ ! -S "$real" ]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid"
      exit $?
    fi
    i=$((i + 1))
    if [ "$i" -gt 100 ]; then
      echo "velnor-dind: dockerd socket did not appear" >&2
      return 1
    fi
    sleep 0.1
  done
}

seed_rabbitmq() {
  digest=sha256:b4be7046918dbd657ffd632610c7b997bacdd33e70194d93f0dbb02d6df492e6
  image=rabbitmq:3.8.22-management
  docker --host "unix://${real}" pull "rabbitmq@${digest}" >&2 || return 1
  docker --host "unix://${real}" tag "rabbitmq@${digest}" "$image" || return 1
  arch=$(docker --host "unix://${real}" image inspect "$image" --format '{{.Architecture}}') || return 1
  if [ "$arch" != "arm64" ]; then
    echo "velnor-dind: rabbitmq architecture is ${arch}" >&2
    return 1
  fi
  echo "velnor-dind: seeded rabbitmq:3.8.22-management linux/arm64" >&2
}

stop_dockerd() {
  kill "$pid" 2>/dev/null || true
  wait "$pid" || true
}

if ! wait_sock; then
  stop_dockerd
  exit 1
fi
if ! seed_rabbitmq; then
  echo "velnor-dind: rabbitmq arm64 seed failed" >&2
  stop_dockerd
  exit 1
fi
ln -sfn docker.sock.real "$public"
wait "$pid"
