#!/bin/sh
# Nested Rosetta crashes OTP 24 JIT (beam.smp signal 11). The arm64 image
# of this tag runs natively and is what testcontainers finds on create.
set -eu
export PATH="/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
real=/run/docker.sock.real
public=/run/docker.sock
rm -f "$public"
dockerd --host="unix://${real}" "$@" &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; wait "$pid" || true; exit 0' TERM INT
i=0
while [ ! -S "$real" ]; do
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"
    exit $?
  fi
  i=$((i + 1))
  if [ "$i" -gt 100 ]; then
    echo "velnor-dind: dockerd socket did not appear" >&2
    break
  fi
  sleep 0.1
done
# postgres:18-alpine is the Testcontainers tag. An amd64 pull on this vfs
# daemon exceeds the 60s startup wait. The arm64 image is what create finds.
seed() {
  image=$1
  if docker --host "unix://${real}" pull --platform linux/arm64 "$image" >&2; then
    echo "velnor-dind: seeded ${image} linux/arm64" >&2
  else
    echo "velnor-dind: ${image} linux/arm64 seed failed" >&2
  fi
}
seed rabbitmq:3.8.22-management &
rmq=$!
seed postgres:18-alpine &
pg=$!
wait "$rmq" || true
wait "$pg" || true
ln -sfn docker.sock.real "$public"
wait "$pid"
