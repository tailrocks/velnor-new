#!/usr/bin/env bash
# Wake only when a named scale-set job changes status, or the daemon exits.
# API errors stay in the log file. Runner-count changes stay in the log file.
set -u
PID=60464
RUN=37311577004
REPO=ChainArgos/java-monorepo
SCRATCH="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
STATE="$SCRATCH/watch-named-37311577004.state"
LOG="$SCRATCH/watch-named-37311577004.log"
export DOCKER_HOST="unix:///Users/donbeave/.orbstack/run/docker.sock"
export PATH="$HOME/.local/share/mise/shims:$PATH"
cd "$HOME" || exit 1

named_jobs() {
  gh api "repos/$REPO/actions/runs/$RUN/jobs?per_page=100" --jq '
    .jobs[]
    | select(.name | test("^(tron-migration|eth-migration|legacy-grpc-server|eth-processor-app) / "))
    | select(.name | contains("Velnor Scale Set"))
    | "\(.name)\t\(.status)\t\(.conclusion // "-")\t\(.runner_name // "-")"
  ' 2>>"$LOG"
}

prev=""
[ -f "$STATE" ] && prev="$(cat "$STATE")"
while :; do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "ACTION_REQUIRED: daemon $PID exited"
    exit 1
  fi
  runners="$(docker ps --filter name=-runner --format '{{.Names}}' 2>/dev/null | grep -c -- '-runner$' || true)"
  if jobs="$(named_jobs)"; then
    printf '%s\n' "$jobs" >"$STATE"
    printf '%s runners=%s\n%s\n' "$(date -u +%H:%M:%S)" "$runners" "$jobs" >>"$LOG"
    if [ -n "$prev" ] && [ "$jobs" != "$prev" ]; then
      echo "ACTION_REQUIRED: named-job change"
      printf '%s\n' "$jobs"
    fi
    prev="$jobs"
  else
    printf '%s api_fail runners=%s\n' "$(date -u +%H:%M:%S)" "$runners" >>"$LOG"
  fi
  sleep 30
done
