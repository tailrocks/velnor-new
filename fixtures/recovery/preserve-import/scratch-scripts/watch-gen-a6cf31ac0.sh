#!/bin/bash
set -eu
GH=/Users/donbeave/.local/share/mise/shims/gh
id=37355790362
while true; do
  json="$("$GH" api repos/tailrocks/velnor-new/actions/runs/$id --jq '{status,conclusion}')"
  status="$(printf '%s' "$json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])')"
  conclusion="$(printf '%s' "$json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["conclusion"] or "")')"
  if [ "$status" = "completed" ]; then
    echo "DONE: generator release $id $conclusion sha a6cf31ac0198000fc842b14f732855cf6120f480"
    if [ "$conclusion" = "success" ]; then
      exit 0
    fi
    exit 1
  fi
  sleep 60
done
