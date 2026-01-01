#!/bin/bash
# Wake on four runners, a launch stall, or daemon death for run 37302093121.
set -u
GH=/Users/donbeave/.local/share/mise/shims/gh
export HOME=/Users/donbeave
LOG=/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/watch-admission-37302093121.log
RUN=37302093121
PID=25288
saw_scale=1
scale_since=0
announced_four=0
announced_stall=1
announced_net=0
while true; do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "FAILED: daemon pid $PID exited"
    exit 1
  fi
  runners=$(docker ps --format '{{.Names}}' | grep -cE -- '-runner$' || true)
  now=$(date +%s)
  if ! json=$("$GH" api "repos/ChainArgos/java-monorepo/actions/runs/${RUN}/jobs?per_page=100" 2>>"$LOG"); then
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) poll-fail runners=$runners" >>"$LOG"
    sleep 30
    continue
  fi
  parsed=$(printf '%s' "$json" | python3 -c 'import json,sys
j=json.load(sys.stdin)
plan="none"
queued=0
active=0
for job in j.get("jobs") or []:
    name=job.get("name") or ""
    status=job.get("status") or ""
    if name=="Plan":
        plan=status+"/"+(job.get("conclusion") or "")
    if "Scale Set" not in name:
        continue
    if status=="queued":
        queued+=1
    elif status=="in_progress":
        active+=1
print("%s %s %s" % (plan, queued, active))')
  plan=${parsed%% *}
  rest=${parsed#* }
  queued=${rest%% *}
  active=${rest#* }
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) runners=$runners plan=$plan queued=$queued active=$active" >>"$LOG"
  if [ "$plan" = "completed/failure" ]; then
    echo "FAILED: Plan failed on run $RUN"
    exit 1
  fi
  demand=$((queued + active))
  if [ "$demand" -gt 0 ] && [ "$saw_scale" -eq 0 ]; then
    saw_scale=1
    scale_since=$now
  fi
  if [ "$runners" -ge 4 ] && [ "$announced_four" -eq 0 ]; then
    announced_four=1
    names=$(docker ps --format '{{.Names}}' | grep -E -- '-runner$' | tr '\n' ' ')
    echo "ACTION_REQUIRED: four runners up on run $RUN ($names)"
  fi
  if [ "$announced_net" -eq 0 ] && lsof -p "$PID" 2>/dev/null | grep -q TCP; then
    announced_net=1
    echo "ACTION_REQUIRED: daemon $PID opened a network socket on run $RUN"
  fi
  if [ "$saw_scale" -eq 1 ] && [ "$runners" -eq 0 ] && [ "$announced_stall" -eq 0 ]; then
    wait=$((now - scale_since))
    if [ "$wait" -ge 180 ]; then
      announced_stall=1
      echo "ACTION_REQUIRED: scale-set demand=$demand and zero runners for ${wait}s on run $RUN"
    fi
  fi
  sleep 30
done
