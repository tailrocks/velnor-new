#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
set +x
: "${GITHUB_REPOSITORY:?}" "${GITHUB_SHA:?}" "${GITHUB_RUN_ID:?}" "${GITHUB_RUN_ATTEMPT:?}" "${GITHUB_REF:?}" "${GITHUB_EVENT_NAME:?}" "${VELNOR_DEFAULT_BRANCH:?}" "${VELNOR_REHEARSAL_ENVIRONMENT:?}" "${VELNOR_REVIEWER_ID:?}" "${GH_TOKEN:?}"
[[ "$GITHUB_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]
[[ "$GITHUB_SHA" =~ ^[a-f0-9]{40}$ && "$GITHUB_RUN_ID" =~ ^[1-9][0-9]*$ && "$GITHUB_RUN_ATTEMPT" = 1 ]]
[[ "$VELNOR_REHEARSAL_ENVIRONMENT" =~ ^[A-Za-z0-9_.-]+$ && "$VELNOR_REVIEWER_ID" =~ ^[1-9][0-9]*$ ]]
git check-ref-format --branch "$VELNOR_DEFAULT_BRANCH" >/dev/null
encoded_branch="$(jq -rn --arg branch "$VELNOR_DEFAULT_BRANCH" '$branch | @uri')"
[[ "$GITHUB_EVENT_NAME" = workflow_dispatch && "$GITHUB_REF" = "refs/heads/$VELNOR_DEFAULT_BRANCH" ]]
api="https://api.github.com/repos/$GITHUB_REPOSITORY"
read_api() {
  curl --silent --show-error --fail --proto '=https' --tlsv1.2 --connect-timeout 10 --max-time 30 --max-filesize 1048576 \
    -H 'Accept: application/vnd.github+json' -H "Authorization: Bearer $GH_TOKEN" -H 'X-GitHub-Api-Version: 2022-11-28' "$1"
}
policy="$(read_api "$api/environments/$VELNOR_REHEARSAL_ENVIRONMENT")"
printf '%s' "$policy" | jq -e --arg environment "$VELNOR_REHEARSAL_ENVIRONMENT" --argjson reviewer "$VELNOR_REVIEWER_ID" '
  .name == $environment and .can_admins_bypass == false and .deployment_branch_policy.protected_branches == true and
  .deployment_branch_policy.custom_branch_policies == false and
  ([.protection_rules[] | select(.type == "required_reviewers") | .reviewers[] |
    select(.type == "User" and .reviewer.type == "User" and .reviewer.id == $reviewer)] | length == 1)
' >/dev/null
branch="$(read_api "$api/branches/$encoded_branch")"
printf '%s' "$branch" | jq -e --arg sha "$GITHUB_SHA" '.protected == true and .commit.sha == $sha' >/dev/null
environment_id="$(printf '%s' "$policy" | jq -er '.id | select(type == "number" and . > 0 and floor == .)')"
[[ "${VELNOR_REHEARSAL_STAGE:-}" = "" || "${VELNOR_REHEARSAL_STAGE:-}" = approved ]]
if [[ "${VELNOR_REHEARSAL_STAGE:-}" = approved ]]; then
  reviews="$(read_api "$api/actions/runs/$GITHUB_RUN_ID/approvals")"
  printf '%s' "$reviews" | jq -e --arg environment "$VELNOR_REHEARSAL_ENVIRONMENT" --argjson reviewer "$VELNOR_REVIEWER_ID" --argjson environment_id "$environment_id" '
    type == "array" and length == 1 and .[0].state == "approved" and
    .[0].user.type == "User" and .[0].user.id == $reviewer and
    ([.[0].environments[] | select(.name == $environment and .id == $environment_id)] | length == 1)
  ' >/dev/null
fi
