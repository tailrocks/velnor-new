#!/usr/bin/env bash
set -euo pipefail

# Checks the two cancel-service jobs of this run. Each must conclude cancelled and finish well
# inside its 5-minute timeout, so the cancel came from the run being cancelled, not from the timeout.
jobs="$(curl --fail --silent --show-error \
  --header "Authorization: Bearer ${GITHUB_TOKEN}" \
  --header "Accept: application/vnd.github+json" \
  "https://api.github.com/repos/${GITHUB_REPOSITORY}/actions/runs/$GITHUB_RUN_ID/jobs?per_page=100")"
rows="$(printf '%s' "$jobs" | jq -r '.jobs[] | select(.name | startswith("Cancel service / ")) | "\(.name)|\(.conclusion)|\((.completed_at | fromdateiso8601) - (.started_at | fromdateiso8601))"')"
test "$(printf '%s\n' "$rows" | grep -c .)" -eq 2
status=0
while IFS='|' read -r name conclusion seconds; do
  echo "$name conclusion=$conclusion seconds=$seconds"
  if [ "$conclusion" != cancelled ]; then
    echo "$name concluded $conclusion, expected cancelled" >&2
    status=1
  fi
  if [ "$seconds" -ge 240 ]; then
    echo "$name ran $seconds s, so the timeout cancelled it, not an explicit cancel" >&2
    status=1
  fi
done <<ROWS
$rows
ROWS
exit "$status"
