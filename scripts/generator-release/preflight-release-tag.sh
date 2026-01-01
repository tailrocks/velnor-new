#!/usr/bin/env bash
set -euo pipefail

version="${1:?version is required}"
expected_repository="${2:?repository is required}"
repository="${GITHUB_REPOSITORY:-}"
if [[ "$expected_repository" != "tailrocks/velnor-new" || "$repository" != "$expected_repository" ]]; then
  echo "unexpected release repository" >&2
  exit 1
fi

tag="v${version}"
require_missing() {
  local route="$1" response status body
  response="$(gh api --include "repos/${repository}/${route}" 2>/dev/null)" || true
  status="$(printf '%s\n' "$response" | awk 'NR == 1 { sub(/\r$/, "", $2); if ($1 ~ /^HTTP\// && $2 ~ /^[0-9][0-9][0-9]$/) print $2; exit }')"
  if [[ "$status" != "404" ]]; then
    echo "cannot prove release object is absent: ${route} (HTTP ${status:-unknown})" >&2
    return 1
  fi
  body="$(printf '%s\n' "$response" | awk 'BEGIN { found = 0 } { line = $0; sub(/\r$/, "", line); if (found) print line; else if (line == "") found = 1 }')"
  if ! printf '%s\n' "$body" | jq -e 'type == "object" and .message == "Not Found" and ((.status | tostring) == "404")' >/dev/null; then
    echo "missing or malformed API error body: ${route}" >&2
    return 1
  fi
}

require_missing "git/ref/tags/${tag}"
require_missing "releases/tags/${tag}"
