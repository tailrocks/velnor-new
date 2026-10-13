#!/bin/bash
set -euo pipefail
export PATH="/usr/bin:/bin:${HOME}/.local/share/mise/shims:${PATH}"
id=37345560436
for _ in $(seq 1 80); do
  json="$(gh run view "$id" --repo tailrocks/velnor-new --json status,conclusion,headSha)"
  status="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["status"])' "$json")"
  conclusion="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["conclusion"] or "")' "$json")"
  sha="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["headSha"])' "$json")"
  if [[ "$status" == "completed" ]]; then
    echo "DONE: generator release ${id} ${conclusion} sha ${sha}"
    if [[ "$conclusion" == "success" && "$sha" == "0ae26c58fac80c597d53232b61f41be4730eac3c" ]]; then
      exit 0
    fi
    exit 1
  fi
  sleep 30
done
echo "FAILED: generator release ${id} still ${status} after wait"
exit 1
