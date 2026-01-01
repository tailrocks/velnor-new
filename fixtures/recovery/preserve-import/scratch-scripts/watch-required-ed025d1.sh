#!/usr/bin/env bash
# Watch CI 37349420326. Print only a failed job or Required success.
set -u
SHA=ed025d199af6a376dc454fe6ab86b32a46c8e338
RUN=37349420326
GH="/Users/donbeave/.local/share/mise/shims/gh"
DIR="/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer"
LOG="$DIR/watch-required-ed025d1.log"
PAGE1="$DIR/req-page1.json"
PAGE2="$DIR/req-page2.json"
for _ in $(seq 1 240); do
  if ! "$GH" api "repos/ChainArgos/java-monorepo/actions/runs/${RUN}/jobs?per_page=100&page=1" >"$PAGE1" 2>>"$LOG"; then
    printf '%s view_error\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$LOG"
    sleep 45
    continue
  fi
  total=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("total_count") or 0)' "$PAGE1")
  if [ "$total" -gt 100 ]; then
    "$GH" api "repos/ChainArgos/java-monorepo/actions/runs/${RUN}/jobs?per_page=100&page=2" >"$PAGE2" 2>>"$LOG" || printf '[]' >"$PAGE2"
  else
    printf '%s\n' '{"jobs":[]}' >"$PAGE2"
  fi
  token=$(python3 - "$PAGE1" "$PAGE2" "$SHA" <<'PY'
import json, sys
jobs=[]
for path in (sys.argv[1], sys.argv[2]):
    doc=json.load(open(path))
    jobs.extend(doc.get("jobs") or [])
want=sys.argv[3]
bad=[]
required=None
for job in jobs:
    name=job.get("name") or ""
    conc=job.get("conclusion") or ""
    if name=="Required":
        required=job
    if conc in ("failure","cancelled") and name!="Required":
        bad.append(job)
if bad:
    job=bad[0]
    print("FAIL %s %s %s" % (job.get("conclusion"), job.get("id"), job.get("name")))
    raise SystemExit
if required and required.get("conclusion")=="success":
    head=""
    print("REQUIRED")
    raise SystemExit
print("WAIT %s" % len(jobs))
PY
)
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$token" >>"$LOG"
  case "$token" in
    WAIT*)
      sleep 45
      continue
      ;;
    REQUIRED)
      echo "DONE: Required success CI $RUN sha $SHA"
      exit 0
      ;;
    FAIL*)
      echo "FAILED: $token CI $RUN sha $SHA"
      exit 1
      ;;
    *)
      echo "FAILED: $token CI $RUN sha $SHA"
      exit 1
      ;;
  esac
done
echo "FAILED: timeout waiting for Required on CI $RUN"
exit 1
