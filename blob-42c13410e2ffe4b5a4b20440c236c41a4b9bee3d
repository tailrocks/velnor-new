#!/usr/bin/env bash
# Watch CI for java commit 5390bf1. Silent until Plan fails, CI fails, or Required succeeds.
set -u
SHA=5390bf1f8f126a1ae4c3dc5043d468ec7c586fed
GH="/Users/donbeave/.local/share/mise/shims/gh"
DIR="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
LOG="$DIR/watch-2087-5390bf1.log"
REPO="ChainArgos/java-monorepo"
plan_ok=0
for _ in $(seq 1 240); do
  if ! "$GH" run list --repo "$REPO" --commit "$SHA" --limit 10 --json databaseId,name,status,conclusion,headSha >"$DIR/runs-5390bf1.json" 2>>"$LOG"; then
    printf '%s run_list_error\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 30
    continue
  fi
  python3 - "$DIR/runs-5390bf1.json" <<'PY' >"$DIR/ci-id-5390bf1.txt"
import json, sys
runs = json.load(open(sys.argv[1]))
cid = ""
for run in runs:
    if run.get("name") == "CI" and run.get("headSha","").startswith("5390bf1"):
        cid = str(run.get("databaseId") or "")
        break
print(cid)
PY
  cid=$(cat "$DIR/ci-id-5390bf1.txt")
  if [ -z "$cid" ]; then
    printf '%s waiting_for_run\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 30
    continue
  fi
  if ! "$GH" run view "$cid" --repo "$REPO" --json status,conclusion,jobs >"$DIR/ci-5390bf1.json" 2>>"$LOG"; then
    printf '%s run_view_error %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$cid" >>"$LOG"
    sleep 30
    continue
  fi
  token=$(python3 - "$DIR/ci-5390bf1.json" "$plan_ok" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
plan_ok = sys.argv[2] == "1"
status = doc.get("status") or ""
conclusion = doc.get("conclusion") or ""
jobs = doc.get("jobs") or []
plan = next((j for j in jobs if j.get("name") == "Plan"), None)
if plan and plan.get("conclusion") in ("failure", "cancelled", "timed_out"):
    print("FAILED Plan " + (plan.get("conclusion") or ""))
    raise SystemExit
failed = [j.get("name") or "job" for j in jobs if j.get("conclusion") in ("failure", "cancelled", "timed_out")]
if failed:
    print("FAILED " + ",".join(failed))
    raise SystemExit
if conclusion in ("failure", "cancelled", "timed_out", "startup_failure"):
    print("FAILED CI " + conclusion)
    raise SystemExit
if plan and plan.get("conclusion") == "success" and not plan_ok:
    print("PLAN_OK")
    raise SystemExit
if status == "completed" and conclusion == "success":
    print("CI_SUCCESS")
    raise SystemExit
print("WAIT")
PY
)
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$token" >>"$LOG"
  case "$token" in
    WAIT)
      sleep 30
      ;;
    PLAN_OK)
      plan_ok=1
      echo "PLAN_OK: CI $cid sha $SHA Plan success. Matrix budget passed. Required still running."
      sleep 30
      ;;
    CI_SUCCESS)
      if "$GH" pr view 2087 --repo "$REPO" --json mergeable,mergeStateStatus,statusCheckRollup >"$DIR/pr-5390bf1.json" 2>>"$LOG"; then
        req=$(python3 -c 'import json; c=json.load(open("'"$DIR/pr-5390bf1.json"'")); r=next((x for x in c.get("statusCheckRollup") or [] if x.get("name")=="Required"), {}); print((r.get("conclusion") or "missing")+" "+c.get("mergeable","")+" "+c.get("mergeStateStatus",""))')
        echo "DONE: CI $cid sha $SHA success Required $req"
      else
        echo "DONE: CI $cid sha $SHA success Required unknown"
      fi
      exit 0
      ;;
    FAILED*)
      echo "$token: CI $cid sha $SHA"
      exit 1
      ;;
    *)
      echo "FAILED: unexpected token $token CI $cid"
      exit 1
      ;;
  esac
done
echo "FAILED: timeout waiting for CI sha $SHA"
exit 1
