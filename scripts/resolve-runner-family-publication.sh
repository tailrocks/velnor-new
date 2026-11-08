#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
: "${GH_TOKEN:?}"
: "${GITHUB_REPOSITORY:?}"
: "${GITHUB_SHA:?}"
: "${GITHUB_OUTPUT:?}"
: "${VELNOR_FAMILY_DISCOVERY_TIMEOUT_SECONDS:?}"
[[ "$GITHUB_REPOSITORY" == tailrocks/velnor-new ]]
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$VELNOR_FAMILY_DISCOVERY_TIMEOUT_SECONDS" =~ ^[1-9][0-9]*$ && "$VELNOR_FAMILY_DISCOVERY_TIMEOUT_SECONDS" -le 21600 ]]
source "$(dirname "${BASH_SOURCE[0]}")/runner-family-discovery.sh"
deadline=$((SECONDS + VELNOR_FAMILY_DISCOVERY_TIMEOUT_SECONDS))
case "${VELNOR_FAMILY_DISCOVERY_PHASE:?}" in
  producer)
    producer="$(family_run ci.yml "$deadline" true)"
    producer_id="$(jq -er '.id | select(. > 0 and floor == .)' <<<"$producer")"
    producer_attempt="$(jq -er '.run_attempt | select(. > 0 and floor == .)' <<<"$producer")"
    family_require_current_main
    producer_artifact="$(family_artifact "$producer_id" "image-assets-$producer_attempt")"
    family_require_current_main
    {
      printf 'producer_run_id=%s\n' "$producer_id"
      printf 'producer_run_attempt=%s\n' "$producer_attempt"
      printf 'producer_artifact_id=%s\n' "$producer_artifact"
    } >> "$GITHUB_OUTPUT"
    exit 0
    ;;
  qualification) ;;
  *) exit 1 ;;
esac
qualifier="$(family_run runner-image-qualification.yml "$deadline" true)"
qualifier_id="$(jq -er '.id | select(. > 0 and floor == .)' <<<"$qualifier")"
qualifier_attempt="$(jq -er '.run_attempt | select(. > 0 and floor == .)' <<<"$qualifier")"
family_require_current_main
qualification_artifact="$(family_artifact "$qualifier_id" "runner-image-qualification-$qualifier_attempt")"
jobs="$(gh api --paginate --slurp "repos/$GITHUB_REPOSITORY/actions/runs/$qualifier_id/attempts/$qualifier_attempt/jobs?per_page=100")"
qualifier_job_id="$(jq -er --arg sha "$GITHUB_SHA" --argjson run "$qualifier_id" --argjson attempt "$qualifier_attempt" '
  if length == 0 or (.[0].total_count | type) != "number" then error("incomplete jobs") else .[0].total_count as $total
  | if (all(.[]; .total_count == $total) and ([.[].jobs[]] | length) == $total) then . else error("incomplete jobs") end end
  | [.[].jobs[] | select(.name == "qualify-images" and .status == "completed" and .conclusion == "success" and .head_sha == $sha and .run_id == $run and .run_attempt == $attempt)]
  | if length == 1 and (.[0].id | type) == "number" and .[0].id > 0 and (.[0].id | floor) == .[0].id then .[0].id else error("missing or ambiguous qualifier job") end
' <<<"$jobs")"
family_require_current_main
{
  printf 'qualifier_run_id=%s\n' "$qualifier_id"
  printf 'qualifier_run_attempt=%s\n' "$qualifier_attempt"
  printf 'qualifier_job_id=%s\n' "$qualifier_job_id"
  printf 'qualification_artifact_id=%s\n' "$qualification_artifact"
} >> "$GITHUB_OUTPUT"
