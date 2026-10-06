#!/usr/bin/env bash
# Watch generator release 37348474713. Print one line when it finishes.
set -u
SHA=6598f62ca0dd6f7fab07e964810266d072212b51
RUN=37348474713
GH="/Users/donbeave/.local/share/mise/shims/gh"
DIR="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
LOG="$DIR/watch-generator-6598f62.log"
for _ in $(seq 1 80); do
  if ! "$GH" run view "$RUN" --repo tailrocks/velnor-new --json status,conclusion,headSha >"$DIR/gen-6598f62.json" 2>>"$LOG"; then
    printf '%s view_error\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 30
    continue
  fi
  token=$(python3 - "$DIR/gen-6598f62.json" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
status = doc.get("status") or ""
conclusion = doc.get("conclusion") or ""
sha = doc.get("headSha") or ""
if status != "completed":
    print("WAIT")
    raise SystemExit
print(conclusion + " " + sha)
PY
)
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$token" >>"$LOG"
  if [ "$token" = "WAIT" ]; then
    sleep 30
    continue
  fi
  if [ "$token" = "success $SHA" ]; then
    echo "DONE: generator release $RUN success sha $SHA"
    exit 0
  fi
  echo "FAILED: generator release $RUN $token"
  exit 1
done
echo "FAILED: timeout waiting for generator release $RUN"
exit 1
