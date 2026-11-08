#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

: "${GH_TOKEN:?}"
: "${GITHUB_REPOSITORY:?}"
: "${GITHUB_SHA:?}"
: "${GITHUB_RUN_ID:?}"
: "${GITHUB_RUN_ATTEMPT:?}"
: "${RUNNER_NAME:?}"
: "${GITHUB_OUTPUT:?}"
[[ "$GITHUB_REPOSITORY" == tailrocks/velnor-new ]]
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$GITHUB_RUN_ID" =~ ^[1-9][0-9]*$ && "$GITHUB_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]]

source "$(dirname "${BASH_SOURCE[0]}")/runner-family-discovery.sh"
run="$(family_run ci.yml)"
producer_id="$(jq -er '.id | select(. > 0 and floor == .)' <<<"$run")"
producer_attempt="$(jq -er '.run_attempt | select(. > 0 and floor == .)' <<<"$run")"
[[ "$producer_id" != "$GITHUB_RUN_ID" ]]
artifact_id="$(family_artifact "$producer_id" "image-assets-$producer_attempt")"
jobs="$(gh api --paginate --slurp "repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/attempts/$GITHUB_RUN_ATTEMPT/jobs?per_page=100")"
job_id="$(jq -er --arg runner "$RUNNER_NAME" --arg sha "$GITHUB_SHA" --argjson run "$GITHUB_RUN_ID" --argjson attempt "$GITHUB_RUN_ATTEMPT" '
  if length == 0 or (.[0].total_count | type) != "number" then error("incomplete jobs") else .[0].total_count as $total | if (all(.[]; .total_count == $total) and ([.[].jobs[]] | length) == $total) then . else error("incomplete jobs") end end
  | [.[].jobs[] | select(.name == "qualify-images" and .status == "in_progress" and .runner_name == $runner and .head_sha == $sha and .run_id == $run and .run_attempt == $attempt)]
  | if length == 1 and (.[0].id | type) == "number" and .[0].id > 0 then .[0].id else error("missing or ambiguous qualifier job") end
' <<<"$jobs")"
{
  printf 'producer_run_id=%s\n' "$producer_id"
  printf 'producer_run_attempt=%s\n' "$producer_attempt"
  printf 'producer_artifact_id=%s\n' "$artifact_id"
  printf 'qualifier_job_id=%s\n' "$job_id"
} >> "$GITHUB_OUTPUT"
