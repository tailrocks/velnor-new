#!/usr/bin/env bash
# Wake on a named-job phase change, a scale-set failure, or daemon death.
set -u
PID=60464
RUN=37311577004
REPO=ChainArgos/java-monorepo
SCRATCH="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
STATE="$SCRATCH/watch-warm-37311577004.state"
export DOCKER_HOST="unix:///Users/donbeave/.orbstack/run/docker.sock"
export PATH="$HOME/.local/share/mise/shims:$PATH"
cd "$HOME" || exit 1

snapshot() {
  local alive
  if kill -0 "$PID" 2>/dev/null; then alive=1; else alive=0; fi
  local runners
  runners="$(docker ps --filter name=-runner --format '{{.Names}}' 2>/dev/null | grep -c -- '-runner$' || true)"
  local jobs
  jobs="$(gh api "repos/$REPO/actions/runs/$RUN/jobs?per_page=100" --jq '[.jobs[]|select((.name|test("tron-migration / Velnor|eth-migration / Velnor|legacy-grpc-server / Velnor|eth-processor-app / Velnor")) or ((.labels//[])|index("ubuntu-26.04-scale-set") and (.conclusion=="failure")))] | .[] | "\(.name) status=\(.status) conclusion=\(.conclusion//"-") runner=\(.runner_name//"-") step=\([.steps[]?|select(.status=="in_progress")|.name]|first // "-")"' 2>/dev/null || echo api_fail)"
  printf 'alive=%s runners=%s\n%s\n' "$alive" "$runners" "$jobs"
}

prev=""
[ -f "$STATE" ] && prev="$(cat "$STATE")"
while :; do
  now="$(snapshot)"
  if [ "$now" != "$prev" ]; then
    printf '%s\n' "$now" >"$STATE"
    case "$now" in
      alive=0*) echo "ACTION_REQUIRED: daemon $PID exited"; echo "$now"; exit 1 ;;
      *api_fail*) echo "ACTION_REQUIRED: github api failed" ;;
      *conclusion=failure*) echo "ACTION_REQUIRED: scale-set failure"; echo "$now"; exit 1 ;;
    esac
    if [ -n "$prev" ]; then
      echo "ACTION_REQUIRED: named-job change"
      echo "$now"
    fi
    prev="$now"
  fi
  sleep 30
done
