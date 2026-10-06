#!/bin/bash
set -u
GH=/Users/donbeave/.local/share/mise/shims/gh
cd "$HOME" || exit 1
want=14abee98719d658e88fc58466d64aee9aabd4385
for _ in $(seq 1 45); do
  if ! out=$("$GH" pr view 86 --repo tailrocks/velnor-new --json headRefOid,state,statusCheckRollup 2>/dev/null); then
    echo "ACTION_REQUIRED: gh pr view 86 failed"
    exit 1
  fi
  printf '%s' "$out" | python3 -c '
import json, sys
data = json.load(sys.stdin)
head = data.get("headRefOid", "")
state = data.get("state", "")
want = "14abee98719d658e88fc58466d64aee9aabd4385"
checks = data.get("statusCheckRollup") or []
req = next((item for item in checks if item.get("name") == "Required"), None)
if state != "OPEN":
    print("ACTION_REQUIRED: PR 86 state %s head %s" % (state, head))
    raise SystemExit(0)
if head != want:
    print("ACTION_REQUIRED: PR 86 head moved to %s" % head)
    raise SystemExit(0)
if req and req.get("status") == "COMPLETED":
    print("ACTION_REQUIRED: PR 86 Required %s head %s" % (req.get("conclusion"), head))
    raise SystemExit(0)
raise SystemExit(2)
'
  code=$?
  if [ "$code" -eq 0 ]; then
    exit 0
  fi
  sleep 60
done
echo "ACTION_REQUIRED: PR 86 Required still pending after 45 minutes"
