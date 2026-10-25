#!/bin/bash
# Drives images/dind/entrypoint.sh with stub dockerd and docker.
# Refuses the host: it would replace /usr/local/bin/docker.
set -euo pipefail

if [[ ! -f /.dockerenv ]]; then
  echo "refusing to run outside a container" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")" && pwd)"

install_stubs() {
  cat >/usr/local/bin/dockerd <<'EOF'
#!/bin/bash
printf '%s\n' "$@" >/tmp/dind-dockerd-args
[[ "$#" -eq 3 && "$1" == '--host=unix:///run/docker.sock.real' \
  && "$2" == '--host=tcp://127.0.0.1:2375' && "$3" == '--tls=false' ]] || exit 93
exec python3 - /run/docker.sock.real 127.0.0.1 <<'PY'
import os, socket, sys, time
sock = sys.argv[1]
host = sys.argv[2]
if os.environ.get("VELNOR_DIND_TEST_EXIT_BEFORE_SOCKET") == "1":
    sys.exit(0)
tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
tcp.bind((host, 2375))
tcp.listen(8)
if os.environ.get("VELNOR_DIND_TEST_NO_SOCKET") != "1":
    os.makedirs(os.path.dirname(sock), exist_ok=True)
    try:
        os.remove(sock)
    except FileNotFoundError:
        pass
    server = socket.socket(socket.AF_UNIX)
    server.bind(sock)
    server.listen(1)
while not os.path.exists("/tmp/dind-stop"):
    tcp.settimeout(0.05)
    try:
        conn, _ = tcp.accept()
        conn.close()
    except TimeoutError:
        pass
    time.sleep(0.05)
PY
EOF
  cat >/usr/local/bin/docker <<'EOF'
#!/bin/bash
printf '%s\n' "$*" >>/tmp/dind-docker-invocations
echo "docker must not run during DinD startup: $*" >&2
exit 99
EOF
  chmod 0755 /usr/local/bin/dockerd /usr/local/bin/docker
}

assert_loopback_listener() {
  python3 - <<'PY'
import socket
s = socket.create_connection(("127.0.0.1", 2375), timeout=1)
assert s.getsockname()[0] == "127.0.0.1", s.getsockname()
s.close()
for record in socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET):
    address = record[4][0]
    if address == "127.0.0.1":
        continue
    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    probe.settimeout(0.2)
    assert probe.connect_ex((address, 2375)) != 0, address
    probe.close()
PY
}

run_ready() {
  local err="$1"
  rm -f /tmp/dind-docker-invocations /tmp/dind-dockerd-args /tmp/dind-stop /run/docker.sock /run/docker.sock.real
  sh "$root/entrypoint.sh" >"$err" 2>&1 &
  local pid=$!
  local attempts=0
  while [[ ! -L /run/docker.sock && "$attempts" -lt 50 ]]; do
    sleep 0.05
    attempts=$((attempts + 1))
  done
  [[ -L /run/docker.sock ]]
  [[ "$(cat /tmp/dind-dockerd-args)" == $'--host=unix:///run/docker.sock.real\n--host=tcp://127.0.0.1:2375\n--tls=false' ]]
  assert_loopback_listener
  [[ ! -e /tmp/dind-docker-invocations ]]
  touch /tmp/dind-stop
  wait "$pid"
}

install_stubs
run_ready /tmp/dind-ready.err
if grep -E 'rabbitmq|docker must not run' /tmp/dind-ready.err; then
  echo "DinD startup must not pull workload images" >&2
  exit 1
fi

rm -f /tmp/dind-stop /run/docker.sock /run/docker.sock.real
if env VELNOR_DIND_TEST_EXIT_BEFORE_SOCKET=1 sh "$root/entrypoint.sh" >/tmp/dind-exit.err 2>&1; then
  echo "DinD startup must fail when dockerd exits before creating its socket" >&2
  exit 1
fi
grep -F 'velnor-dind: dockerd exited before socket appeared' /tmp/dind-exit.err >/dev/null
[[ ! -L /run/docker.sock ]]

rm -f /tmp/dind-stop /run/docker.sock /run/docker.sock.real
if timeout 15s env VELNOR_DIND_TEST_NO_SOCKET=1 sh "$root/entrypoint.sh" >/tmp/dind-timeout.err 2>&1; then
  echo "DinD startup must fail when dockerd never creates its socket" >&2
  exit 1
else
  status=$?
  if [[ "$status" -eq 124 ]]; then
    echo "DinD startup timeout exceeded its 15-second test bound" >&2
    exit 1
  fi
fi
grep -F 'velnor-dind: dockerd socket did not appear' /tmp/dind-timeout.err >/dev/null
[[ ! -L /run/docker.sock ]]

rm -f /tmp/dind-stop /tmp/dind-dockerd-args /run/docker.sock /run/docker.sock.real
if sh "$root/entrypoint.sh" --host=tcp://0.0.0.0:2375 >/tmp/dind-override.out 2>&1; then
  echo "DinD entrypoint must reject endpoint overrides" >&2
  exit 1
fi
grep -F 'does not accept command overrides' /tmp/dind-override.out >/dev/null
[[ ! -e /tmp/dind-dockerd-args ]]
echo "dind-entrypoint ok"
