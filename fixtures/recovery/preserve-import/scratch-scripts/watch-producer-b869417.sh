#!/usr/bin/env bash
# Print only when a producer job on run 37352232818 reaches a conclusion.
set -u
GH="/Users/donbeave/.local/share/mise/shims/gh"
RUN="37352232818"
SEEN=""
for _ in $(seq 1 180); do
  json="$("$GH" api "repos/ChainArgos/java-monorepo/actions/runs/${RUN}/jobs?per_page=100" 2>/dev/null || true)"
  if [ -z "$json" ]; then
    sleep 30
    continue
  fi
  line="$(printf '%s' "$json" | python3 -c '
import json,sys
seen=set(sys.argv[1].split())
data=json.load(sys.stdin)
jobs=data.get("jobs") or []
out=[]
for job in jobs:
    conclusion=job.get("conclusion") or ""
    if conclusion=="":
        continue
    marker=str(job.get("id"))+":"+conclusion
    if marker in seen:
        continue
    name=job.get("name") or ""
    out.append(marker+"\t"+conclusion+"\t"+name)
print("\n".join(out))
' "$SEEN")"
  if [ -n "$line" ]; then
    while IFS="$(printf '\t')" read -r marker conclusion name; do
      [ -n "$marker" ] || continue
      SEEN="$SEEN $marker"
      printf '%s %s %s\n' "$conclusion" "$marker" "$name"
      case "$conclusion" in
        failure|cancelled|timed_out)
          printf 'FAILED: producer %s %s\n' "$RUN" "$name"
          exit 1
          ;;
      esac
    done <<EOF
$line
EOF
  fi
  pending="$(printf '%s' "$json" | python3 -c '
import json,sys
data=json.load(sys.stdin)
jobs=data.get("jobs") or []
print(sum(1 for job in jobs if not job.get("conclusion")))
')"
  if [ "$pending" = "0" ]; then
    printf 'DONE: producer %s all jobs concluded\n' "$RUN"
    exit 0
  fi
  sleep 30
done
printf 'FAILED: producer %s watcher reached its poll limit\n' "$RUN"
exit 1
