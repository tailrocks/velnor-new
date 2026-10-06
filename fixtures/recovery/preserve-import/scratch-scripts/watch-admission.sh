#!/usr/bin/env bash
# Wake only when admission state changes. Silent while the same counts hold.
set -u
PID=52163
RUN=37302093121
REPO=ChainArgos/java-monorepo
SCRATCH="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
STATE="$SCRATCH/watch-admission.state"
TRACE="$SCRATCH/watch-admission.trace"
export DOCKER_HOST="unix:///Users/donbeave/.orbstack/run/docker.sock"
export PATH="$HOME/.local/share/mise/shims:$PATH"
cd "$HOME" || exit 1

snapshot() {
  local alive runners jobs
  if kill -0 "$PID" 2>/dev/null; then alive=1; else alive=0; fi
  runners="$(docker ps --filter name=-runner --format '{{.Names}}' 2>/dev/null | grep -c -- '-runner$' || true)"
  jobs="$(gh api "repos/$REPO/actions/runs/$RUN/jobs?per_page=100" --jq '[.jobs[]|select((.labels//[])|index("ubuntu-26.04-scale-set"))] | "q=\([.[]|select(.status=="queued")]|length) run=\([.[]|select(.status=="in_progress")]|length) ok=\([.[]|select(.conclusion=="success")]|length) bad=\([.[]|select(.conclusion=="failure" or .conclusion=="cancelled")]|length)"' 2>/dev/null || echo api_fail)"
  printf '%s runners=%s %s\n' "$alive" "$runners" "$jobs"
}

prev=""
[ -f "$STATE" ] && prev="$(cat "$STATE")"
while :; do
  now="$(snapshot)"
  printf '%s %s\n' "$(date -u +%H:%M:%S)" "$now" >>"$TRACE"
  if [ "$now" != "$prev" ]; then
    printf '%s\n' "$now" >"$STATE"
    case "$now" in
      0\ *) echo "ACTION_REQUIRED: daemon $PID exited $now"; exit 1 ;;
      *api_fail*) echo "ACTION_REQUIRED: github api failed $now" ;;
      *bad=[1-9]*) echo "ACTION_REQUIRED: scale-set job failed $now" ;;
    esac
    if [ -n "$prev" ]; then
      echo "ACTION_REQUIRED: admission $prev -> $now"
    fi
    prev="$now"
    case "$now" in
      *"q=0 run=0 "*"bad=0") echo "DONE: scale-set jobs finished $now"; exit 0 ;;
    esac
  fi
  sleep 30
done
