#!/usr/bin/env bash
# Wake when the new run changes admission or a tool-seed step finishes.
set -u
PID=60464
RUN=37311577004
REPO=ChainArgos/java-monorepo
EXPECTED=sha256:b5a2b0b621b899090e82521cdd121c17d4a7b16069907b6067cc416bd19199ff
SCRATCH="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
STATE="$SCRATCH/watch-37311577004.state"
export DOCKER_HOST="unix:///Users/donbeave/.orbstack/run/docker.sock"
export PATH="$HOME/.local/share/mise/shims:$PATH"
cd "$HOME" || exit 1

snapshot() {
  local alive runners images jobs
  if kill -0 "$PID" 2>/dev/null; then alive=1; else alive=0; fi
  runners="$(docker ps --filter name=-runner --format '{{.Names}}' 2>/dev/null | grep -c -- '-runner$' || true)"
  images="$(docker ps --filter name=-runner --format '{{.Image}}' 2>/dev/null | sort | uniq -c | tr '\n' ' ')"
  jobs="$(gh api "repos/$REPO/actions/runs/$RUN/jobs?per_page=100" --jq '[.jobs[]|select((.labels//[])|index("ubuntu-26.04-scale-set"))] | "q=\([.[]|select(.status=="queued")]|length) run=\([.[]|select(.status=="in_progress")]|length) ok=\([.[]|select(.conclusion=="success")]|length) bad=\([.[]|select(.conclusion=="failure")]|length) cancel=\([.[]|select(.conclusion=="cancelled")]|length)"' 2>/dev/null || echo api_fail)"
  printf 'alive=%s runners=%s %s images=%s\n' "$alive" "$runners" "$jobs" "$images"
}

prev=""
[ -f "$STATE" ] && prev="$(cat "$STATE")"
while :; do
  now="$(snapshot)"
  if [ "$now" != "$prev" ]; then
    printf '%s\n' "$now" >"$STATE"
    case "$now" in
      alive=0*) echo "ACTION_REQUIRED: daemon $PID exited $now"; exit 1 ;;
      *api_fail*) echo "ACTION_REQUIRED: github api failed $now" ;;
      *bad=[1-9]*) echo "ACTION_REQUIRED: scale-set failure $now" ;;
    esac
    if [ -n "$prev" ]; then
      echo "ACTION_REQUIRED: $prev -> $now"
    fi
    prev="$now"
  fi
  seed="$(gh api "repos/$REPO/actions/runs/$RUN/jobs?per_page=100" --jq '[.jobs[]|select((.labels//[])|index("ubuntu-26.04-scale-set"))|.steps[]?|select(.name=="Restore Velnor tool seed" and .status=="completed")]|length' 2>/dev/null || echo 0)"
  if [ "${seed:-0}" != "0" ]; then
    echo "ACTION_REQUIRED: tool-seed steps completed count=$seed"
    gh api "repos/$REPO/actions/runs/$RUN/jobs?per_page=100" --jq '.jobs[]|select((.labels//[])|index("ubuntu-26.04-scale-set"))|{name,status,runner_name,started:.started_at,steps:[.steps[]?|select(.name=="Set up job" or .name=="Restore Velnor tool seed" or .name=="Prepare pinned tools" or .name=="Checkout")|{name,status,conclusion,started:.started_at,completed:.completed_at}]}'
    exit 0
  fi
  wrong="$(docker ps --filter name=-runner --format '{{.Image}}' 2>/dev/null | grep -v 'velnor-runner:ubuntu-26.04-2.337.0' || true)"
  if [ -n "$wrong" ]; then
    echo "ACTION_REQUIRED: unexpected runner image $wrong"
    exit 1
  fi
  sleep 20
done
