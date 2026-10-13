#!/bin/bash
# Wake when no Scale Set job on run 37288470200 is in Cargo restore or shared steps.
set -u
GH=/Users/donbeave/.local/share/mise/shims/gh
export HOME=/Users/donbeave
LOG=/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/watch-shared-steps.log
while true; do
  if ! json=$("$GH" api "repos/ChainArgos/java-monorepo/actions/runs/37288470200/jobs?per_page=100" 2>>"$LOG"); then
    echo "poll-fail" >>"$LOG"
    sleep 60
    continue
  fi
  line=$(printf '%s' "$json" | python3 -c 'import json,sys
j=json.load(sys.stdin)
busy=[]
for job in j.get("jobs") or []:
    if job.get("status")!="in_progress":
        continue
    labels=",".join(job.get("labels") or [])
    if "scale-set" not in labels:
        continue
    cur=[s.get("name") or "" for s in (job.get("steps") or []) if s.get("status")=="in_progress"]
    heavy=[name for name in cur if name in ("Run shared steps","Restore Cargo sources")]
    if heavy:
        busy.append("%s:%s" % (job.get("id"), "|".join(heavy)))
print("busy" if busy else "clear")
print(" ".join(busy))')
  state=${line%%$'\n'*}
  rest=${line#*$'\n'}
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $state $rest" >>"$LOG"
  if [ "$state" = "clear" ]; then
    echo "ACTION_REQUIRED: shared steps clear. $rest"
    exit 0
  fi
  sleep 60
done
