#!/usr/bin/env bash
# Watch PR 2087. Print only a failure, a conflict, or a merge-ready result.
set -u
LOG="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/watch-2087.log"
GH="/Users/donbeave/.local/share/mise/shims/gh"
ready_sent=0
while true; do
  if ! "$GH" pr view 2087 --repo ChainArgos/java-monorepo --json state,mergeable,headRefOid,statusCheckRollup >"$LOG.pr.json" 2>>"$LOG"; then
    printf '%s pr_view_error\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 30
    continue
  fi
  head=$(python3 -c 'import json; print(json.load(open("'"$LOG.pr.json"'"))["headRefOid"])')
  if ! "$GH" run list --repo ChainArgos/java-monorepo --commit "$head" --limit 20 --json databaseId,name,status,conclusion,workflowName >"$LOG.runs.json" 2>>"$LOG"; then
    printf '%s run_list_error\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 30
    continue
  fi
  : >"$LOG.jobs.json"
  python3 - "$LOG.runs.json" <<'PY' >"$LOG.ids"
import json, sys
runs = json.load(open(sys.argv[1]))
for run in runs:
    print(run.get("databaseId") or "")
PY
  while read -r run_id; do
    [ -n "$run_id" ] || continue
    if "$GH" run view "$run_id" --repo ChainArgos/java-monorepo --json jobs >"$LOG.one.json" 2>>"$LOG"; then
      python3 -c 'import json; d=json.load(open("'"$LOG.one.json"'")); print(json.dumps({"jobs":[{"name":j.get("name"),"conclusion":j.get("conclusion")} for j in d.get("jobs") or []]}))' >>"$LOG.jobs.json"
    fi
  done <"$LOG.ids"
  token=$(python3 - "$LOG.pr.json" "$LOG.runs.json" "$LOG.jobs.json" <<'PY'
import json, sys
pr = json.load(open(sys.argv[1]))
runs = json.load(open(sys.argv[2]))
job_failed = []
for line in open(sys.argv[3]):
    line = line.strip()
    if not line:
        continue
    doc = json.loads(line)
    for job in doc.get("jobs") or []:
        if job.get("conclusion") in ("failure", "cancelled", "timed_out"):
            job_failed.append(job.get("name") or "job")
if job_failed:
    print("FAILED " + ",".join(job_failed))
    raise SystemExit
state = pr.get("state") or ""
mergeable = pr.get("mergeable") or ""
if state == "MERGED":
    print("DONE")
    raise SystemExit
if state == "CLOSED":
    print("CANCELLED")
    raise SystemExit
if mergeable == "CONFLICTING":
    print("CONFLICT")
    raise SystemExit
if not runs:
    print("WAIT")
    raise SystemExit
failed = []
pending = False
for run in runs:
    conclusion = run.get("conclusion") or ""
    status = run.get("status") or ""
    if conclusion in ("failure", "cancelled", "timed_out", "startup_failure"):
        failed.append(run.get("workflowName") or run.get("name") or "run")
    elif status != "completed":
        pending = True
checks = pr.get("statusCheckRollup") or []
failed_checks = [c.get("name","") for c in checks if c.get("conclusion") in ("FAILURE", "CANCELLED", "TIMED_OUT")]
required = next((c for c in checks if c.get("name") == "Required"), None)
req = (required or {}).get("conclusion") or ""
if failed or (not pending and failed_checks):
    names = ",".join(failed + failed_checks)
    print("FAILED " + names)
elif pending or req != "SUCCESS":
    print("WAIT")
elif mergeable == "MERGEABLE":
    print("READY")
else:
    print("WAIT")
PY
)
  printf '%s head=%s token=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$head" "$token" >>"$LOG"
  case "$token" in
    DONE)
      echo "DONE: PR 2087 merged"
      exit 0
      ;;
    CANCELLED)
      echo "CANCELLED: PR 2087 closed"
      exit 0
      ;;
    CONFLICT)
      echo "ACTION_REQUIRED: PR 2087 conflicts with main"
      exit 0
      ;;
    FAILED*)
      echo "FAILED: PR 2087 $token head $head"
      exit 1
      ;;
    READY)
      if [ "$ready_sent" -eq 0 ]; then
        ready_sent=1
        echo "ACTION_REQUIRED: PR 2087 Required SUCCESS head $head"
      fi
      ;;
  esac
  sleep 30
done
