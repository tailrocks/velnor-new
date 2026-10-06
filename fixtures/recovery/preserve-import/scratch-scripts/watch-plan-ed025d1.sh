#!/usr/bin/env bash
# Watch Plan on CI run 37349420326. Print one line when Plan finishes.
set -u
SHA=ed025d199af6a376dc454fe6ab86b32a46c8e338
RUN=37349420326
GH="/Users/donbeave/.local/share/mise/shims/gh"
DIR="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
LOG="$DIR/watch-plan-ed025d1.log"
JSON="$DIR/ci-ed025d1.json"
for _ in $(seq 1 120); do
  if ! "$GH" run view "$RUN" --repo ChainArgos/java-monorepo --json status,conclusion,headSha,jobs >"$JSON" 2>>"$LOG"; then
    printf '%s view_error\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 30
    continue
  fi
  token=$(python3 - "$JSON" "$SHA" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
want = sys.argv[2]
sha = doc.get("headSha") or ""
status = doc.get("status") or ""
if sha != want:
    print("BADSHA " + sha)
    raise SystemExit
plan = None
for job in doc.get("jobs") or []:
    if job.get("name") == "Plan":
        plan = job
        break
if plan is None:
    if status == "completed":
        print("NO_PLAN " + (doc.get("conclusion") or ""))
    else:
        print("WAIT")
    raise SystemExit
pstatus = plan.get("status") or ""
pconc = plan.get("conclusion") or ""
if pstatus != "completed":
    print("WAIT")
    raise SystemExit
print(pconc)
PY
)
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$token" >>"$LOG"
  case "$token" in
    WAIT)
      sleep 30
      continue
      ;;
    success)
      echo "DONE: Plan success CI $RUN sha $SHA"
      exit 0
      ;;
    *)
      echo "FAILED: Plan $token CI $RUN sha $SHA"
      exit 1
      ;;
  esac
done
echo "FAILED: timeout waiting for Plan on CI $RUN"
exit 1
