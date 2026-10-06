#!/bin/sh
# Start the private daemon before publishing its socket to the runner.
set -eu
# The image ships the helper; the entrypoint test env does not.
if [ -f /usr/local/libexec/velnor/cgroup-v2-delegation.sh ]; then
  . /usr/local/libexec/velnor/cgroup-v2-delegation.sh
  velnor_enable_cgroup_v2
fi
export PATH="/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
real=/run/docker.sock.real
public=/run/docker.sock
rm -f "$public"
# Do not publish a socket unless the daemon creates it within about 10 seconds.
dockerd --host="unix://${real}" "$@" &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; wait "$pid" || true; exit 0' TERM INT
i=0
while [ ! -S "$real" ]; do
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid" 2>/dev/null || true
    echo "velnor-dind: dockerd exited before socket appeared" >&2
    exit 1
  fi
  i=$((i + 1))
  if [ "$i" -gt 100 ]; then
    echo "velnor-dind: dockerd socket did not appear" >&2
    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    exit 1
  fi
  sleep 0.1
done
ln -sfn docker.sock.real "$public"
wait "$pid"
