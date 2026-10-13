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
sock=
for arg in "$@"; do
  case "$arg" in
    --host=unix:*) sock=${arg#--host=unix:} ;;
  esac
done
exec python3 - "$sock" <<'PY'
import os, socket, sys, time
sock = sys.argv[1]
os.makedirs(os.path.dirname(sock), exist_ok=True)
try:
    os.remove(sock)
except FileNotFoundError:
    pass
server = socket.socket(socket.AF_UNIX)
server.bind(sock)
server.listen(1)
while not os.path.exists("/tmp/dind-stop"):
    time.sleep(0.05)
PY
EOF
  cat >/usr/local/bin/docker <<'EOF'
#!/bin/bash
printf '%s\n' "$*" >>/tmp/dind-pulls
rc="${VELNOR_DIND_PULL_RC:-0}"
exit "$rc"
EOF
  chmod 0755 /usr/local/bin/dockerd /usr/local/bin/docker
}

run_once() {
  local rc="$1"
  local err="$2"
  rm -f /tmp/dind-stop /run/docker.sock /run/docker.sock.real /tmp/dind-pulls
  : >/tmp/dind-pulls
  VELNOR_DIND_PULL_RC="$rc" bash "$root/entrypoint.sh" >"$err" 2>&1 &
  local pid=$!
  local i
  for i in $(seq 1 50); do
    if [[ -L /run/docker.sock ]]; then
      break
    fi
    sleep 0.05
  done
  [[ -L /run/docker.sock ]]
  touch /tmp/dind-stop
  wait "$pid"
}

install_stubs
run_once 0 /tmp/dind-ok.err
grep -F 'pull --platform linux/arm64 rabbitmq:3.8.22-management' /tmp/dind-pulls >/dev/null
grep -F 'velnor-dind: seeded rabbitmq:3.8.22-management linux/arm64' /tmp/dind-ok.err >/dev/null
if grep -F 'postgres' /tmp/dind-pulls >/dev/null; then
  echo "postgres pull is not the entrypoint" >&2
  exit 1
fi

run_once 1 /tmp/dind-fail.err
grep -F 'velnor-dind: rabbitmq arm64 seed failed' /tmp/dind-fail.err >/dev/null
[[ -L /run/docker.sock ]]
echo "dind-entrypoint ok"
