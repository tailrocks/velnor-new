#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/velnor-wait-docker-api.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin"

cat >"$fixture/bin/timeout" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -ge 3 && "$1" == --signal=KILL && "$2" =~ ^[1-9][0-9]?s$ ]]
shift 2
exec "$@"
EOF
cat >"$fixture/bin/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ -z "${DOCKER_HOST:-}" && -z "${DOCKER_CONTEXT:-}" ]]
[[ "$#" -eq 5 && "$1" == --host && "$2" == tcp://127.0.0.1:2375 \
  && "$3" == info && "$4" == --format && "$5" == '{{.Driver}}|{{.DockerRootDir}}' ]]
printf '%s\n' 'vfs|/var/lib/docker'
EOF
chmod 0755 "$fixture/bin/timeout" "$fixture/bin/docker"

PATH="$fixture/bin:$PATH" DOCKER_HOST='tcp://127.0.0.1:2375' \
  DOCKER_CONTEXT=must-be-ignored "$script_dir/wait-docker-api.sh" \
  tcp://127.0.0.1:2375 2 1

for endpoint in unix:///var/run/docker.sock tcp://0.0.0.0:2375 tcp://docker:2375; do
  if PATH="$fixture/bin:$PATH" "$script_dir/wait-docker-api.sh" "$endpoint" 2 1 \
      >"$fixture/out" 2>"$fixture/err"; then
    echo "wait-docker-api accepted non-private endpoint: $endpoint" >&2
    exit 1
  fi
  grep -F 'fixed private DinD loopback endpoint' "$fixture/err" >/dev/null
done

echo 'wait-docker-api endpoint checks passed'
