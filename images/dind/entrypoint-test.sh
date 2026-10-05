#!/bin/bash
# Drives images/dind/entrypoint.sh with stub dockerd and docker.
# Refuses the host: it would replace /usr/local/bin/docker.
set -euo pipefail

if [[ ! -f /.dockerenv ]]; then
  echo "refusing to run outside a container" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")" && pwd)"
digest=sha256:b4be7046918dbd657ffd632610c7b997bacdd33e70194d93f0dbb02d6df492e6

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
cmd=
prev=
for arg in "$@"; do
  case "$prev" in
    --host) prev=; continue ;;
  esac
  case "$arg" in
    --host|--host=*) prev=$arg; continue ;;
  esac
  cmd=$arg
  break
done
case "$cmd" in
  pull) exit "${VELNOR_DIND_PULL_RC:-0}" ;;
  image) printf '%s\n' "${VELNOR_DIND_ARCH:-arm64}"; exit 0 ;;
  *) exit 0 ;;
esac
EOF
  chmod 0755 /usr/local/bin/dockerd /usr/local/bin/docker
}

start_entry() {
  local rc="$1"
  local arch="$2"
  local err="$3"
  rm -f /tmp/dind-stop /run/docker.sock /run/docker.sock.real /tmp/dind-pulls
  : >/tmp/dind-pulls
  VELNOR_DIND_PULL_RC="$rc" VELNOR_DIND_ARCH="$arch" \
    bash "$root/entrypoint.sh" >"$err" 2>&1 &
  entry_pid=$!
}

wait_socket() {
  local i
  for i in $(seq 1 50); do
    if [[ -L /run/docker.sock ]]; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}

stop_entry() {
  touch /tmp/dind-stop
  wait "$1" || true
}

refuse_socket() {
  local pid=$1
  local err=$2
  local i
  for i in $(seq 1 50); do
    if [[ -L /run/docker.sock ]]; then
      echo "socket opened for $err" >&2
      exit 1
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.05
  done
  if [[ -L /run/docker.sock ]]; then
    echo "socket opened for $err" >&2
    exit 1
  fi
  wait "$pid" || true
  if grep -F 'velnor-dind: seeded rabbitmq:3.8.22-management linux/arm64' "$err" >/dev/null; then
    echo "failed seed claimed success in $err" >&2
    exit 1
  fi
  grep -F 'velnor-dind: rabbitmq arm64 seed failed' "$err" >/dev/null
}

install_stubs

start_entry 0 arm64 /tmp/dind-ok.err
wait_socket
grep -F "pull rabbitmq@${digest}" /tmp/dind-pulls >/dev/null
grep -F "tag rabbitmq@${digest} rabbitmq:3.8.22-management" /tmp/dind-pulls >/dev/null
grep -F 'image inspect rabbitmq:3.8.22-management' /tmp/dind-pulls >/dev/null
grep -F 'velnor-dind: seeded rabbitmq:3.8.22-management linux/arm64' /tmp/dind-ok.err >/dev/null
if grep -F 'postgres' /tmp/dind-pulls >/dev/null; then
  echo "postgres pull is not the entrypoint" >&2
  exit 1
fi
stop_entry "$entry_pid"

start_entry 1 arm64 /tmp/dind-fail.err
refuse_socket "$entry_pid" /tmp/dind-fail.err

start_entry 0 amd64 /tmp/dind-arch.err
refuse_socket "$entry_pid" /tmp/dind-arch.err
grep -F 'velnor-dind: rabbitmq architecture is amd64' /tmp/dind-arch.err >/dev/null
echo "dind-entrypoint ok"
