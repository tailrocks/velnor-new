#!/usr/bin/env bash
# Watch generator release run 37342595240. Print only a terminal result.
set -u
RUN=37342595240
LOG="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/watch-gen-37342595240.log"
GH="/Users/donbeave/.local/share/mise/shims/gh"
while true; do
  if ! "$GH" run view "$RUN" --repo tailrocks/velnor-new --json status,conclusion,jobs >"$LOG.json" 2>>"$LOG"; then
    printf '%s gh_error\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 30
    continue
  fi
  python3 - "$LOG.json" >>"$LOG" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
status = doc.get("status") or ""
conclusion = doc.get("conclusion") or ""
jobs = doc.get("jobs") or []
failed = [j.get("name","") for j in jobs if j.get("conclusion") in ("failure", "cancelled", "timed_out")]
print(f"status={status} conclusion={conclusion} failed={','.join(failed)}")
if failed:
    print("TOKEN FAILED " + ",".join(failed))
elif conclusion == "success":
    print("TOKEN DONE")
elif conclusion in ("failure", "cancelled", "timed_out"):
    print("TOKEN " + conclusion.upper())
PY
  token=$(awk '/^TOKEN /{print}' "$LOG" | tail -1)
  case "$token" in
    "TOKEN DONE")
      echo "DONE: generator release 37342595240 success sha 3d77eed756fc79cb7bfbba8df7c4f0f0db289d09"
      exit 0
      ;;
    "TOKEN FAILURE"*|"TOKEN FAILED"*|"TOKEN CANCELLED"*|"TOKEN TIMED_OUT"*)
      echo "FAILED: generator release 37342595240 $token"
      exit 1
      ;;
  esac
  sleep 30
done
