#!/usr/bin/env bash
# Print only a failure or Required success for main CI run 37352233522.
set -u
GH="/Users/donbeave/.local/share/mise/shims/gh"
RUN="37352233522"
SHA="b869417b5dfec6134a17b21915f659bc27bdfd73"
for _ in $(seq 1 240); do
  json="$("$GH" api "repos/ChainArgos/java-monorepo/actions/runs/${RUN}/jobs?per_page=100" 2>/dev/null || true)"
  if [ -z "$json" ]; then
    sleep 30
    continue
  fi
  result="$(printf '%s' "$json" | python3 -c '
import json,sys
data=json.load(sys.stdin)
jobs=data.get("jobs") or []
for job in jobs:
    name=job.get("name") or ""
    conclusion=job.get("conclusion") or ""
    if name!="Required" and conclusion in ("failure","cancelled","timed_out"):
        print("FAILED")
        print(name)
        raise SystemExit
for job in jobs:
    if job.get("name")=="Required" and job.get("conclusion")=="success":
        print("DONE")
        raise SystemExit
print("WAIT")
')"
  kind="$(printf '%s\n' "$result" | head -n 1)"
  if [ "$kind" = "FAILED" ]; then
    name="$(printf '%s\n' "$result" | sed -n '2p')"
    printf 'FAILED: main CI %s %s sha %s\n' "$RUN" "$name" "$SHA"
    exit 1
  fi
  if [ "$kind" = "DONE" ]; then
    printf 'DONE: Required success main CI %s sha %s\n' "$RUN" "$SHA"
    exit 0
  fi
  sleep 30
done
printf 'FAILED: main CI %s watcher reached its poll limit\n' "$RUN"
exit 1
