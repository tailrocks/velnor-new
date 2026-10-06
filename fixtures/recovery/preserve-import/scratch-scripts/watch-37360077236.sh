#!/usr/bin/env bash
# Wake when Plan fails, a scale-set job starts or fails, or run 37360077236 ends.
# Stdout is only FAILED, DONE, or ACTION_REQUIRED.
set -uo pipefail
GH=/Users/donbeave/.local/share/mise/shims/gh
RUN=37360077236
DIR=/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer
LOG="$DIR/watch-37360077236.log"
SEEN="$DIR/watch-37360077236.seen"
JSON="$DIR/watch-37360077236.json"
touch "$SEEN"
while true; do
  if "$GH" run view "$RUN" --repo ChainArgos/java-monorepo --json status,conclusion,headSha,jobs >"$JSON" 2>>"$LOG"; then
    token="$(python3 - "$JSON" "$SEEN" <<'PY'
import json, pathlib, sys
run = json.loads(pathlib.Path(sys.argv[1]).read_text())
seen_path = pathlib.Path(sys.argv[2])
seen = set(seen_path.read_text().split())
status = run.get("status")
conclusion = run.get("conclusion")
sha = run.get("headSha")
lines = []
terminal = None
for job in run.get("jobs") or []:
    name = job.get("name") or ""
    st = job.get("status")
    conc = job.get("conclusion")
    jid = job.get("databaseId")
    if name == "Plan" and conc in ("failure", "cancelled", "timed_out"):
        terminal = f"FAILED: Plan {conc} run 37360077236 sha {sha}"
    if "Velnor Scale Set" in name:
        key = f"{jid}:{st}:{conc}"
        if key not in seen:
            seen.add(key)
            if st == "in_progress":
                lines.append(f"ACTION_REQUIRED: scale-set started {name} id {jid}")
            if conc in ("failure", "cancelled", "timed_out"):
                terminal = f"FAILED: {name} {conc} id {jid} run 37360077236"
            if conc == "success":
                lines.append(f"ACTION_REQUIRED: scale-set success {name} id {jid}")
seen_path.write_text(" ".join(sorted(seen)) + "\n")
if terminal:
    print(terminal)
elif lines:
    print("\n".join(lines))
PY
)"
    if [ -n "$token" ]; then
      printf '%s\n' "$token"
      case "$token" in
        FAILED:*|DONE:*) exit 0 ;;
      esac
    fi
    status="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("status"))' "$JSON")"
    conclusion="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("conclusion"))' "$JSON")"
    if [ "$status" = "completed" ]; then
      if [ "$conclusion" = "success" ]; then
        echo "DONE: run 37360077236 success sha c506d60105be7e97439402f8b5a9dc624e001bf1"
      else
        echo "FAILED: run 37360077236 ${conclusion} sha c506d60105be7e97439402f8b5a9dc624e001bf1"
      fi
      exit 0
    fi
  else
    echo "view failed" >>"$LOG"
  fi
  sleep 30
done
